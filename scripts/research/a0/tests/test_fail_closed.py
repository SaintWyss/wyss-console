"""Portable regressions for the independent review's fail-open findings. No providers."""
import copy
import hashlib
import io
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'harness-v2'))
import limits
import oracle
import stream
from sandbox import SYNTH_KEY

RUN = 'review'
SKILLS = ['alpha', 'beta']
LINE = json.dumps({'path': '/v1/messages', 'has_key': True,
                   'key_sha256': hashlib.sha256(SYNTH_KEY.encode()).hexdigest(),
                   'body': {'messages': [{'role': 'user', 'content': 'CANARY-DESC-alpha-review'}]}}) + '\n'
DIGEST = hashlib.sha256(LINE.encode()).hexdigest()
MARKERS = {'requests.jsonl': DIGEST}
CASE = {'id': 'review', 'expect': {'requests': [1, 1], 'desc_present': ['alpha'], 'desc_absent': ['beta']}}


def records():
    return [{'kind': 'START', 'case': 'review', 'mode': 'H'},
            {'kind': 'FIXTURE', 'home': {}, 'work': {}},
            {'kind': 'REQUEST', 'sha256': DIGEST, 'bytes': len(LINE.encode())},
            {'kind': 'SUT_DONE', 'result': {'mode': 'H', 'exit': 0, 'timed_out': False}},
            {'kind': 'FINAL', 'home': {}, 'work': {}, 'markers': MARKERS},
            {'kind': 'END'}]


def evidence(recs=None, files=None):
    return oracle.verify_evidence(records() if recs is None else recs, [], [LINE], MARKERS if files is None else files)


def result(ev=None):
    return {'analysis': oracle.analyse(RUN, [LINE], SKILLS), 'evidence': evidence() if ev is None else ev,
            'inside': {'mode': 'H', 'exit': 0, 'timed_out': False},
            'isolation': {'integrity': True, 'namespace_verified': True, 'no_leftovers': True,
                          'no_unexpected_proxy': True, 'owned_files_unchanged': True,
                          'not_killed_for_storage': True, 'launcher_complete': True}, 'validator': None,
            'markers_present': [], 'work_ran': [], 'victim_present': True, 'stdout': 'ok', 'stderr': ''}


class EvidenceTests(unittest.TestCase):
    def test_complete_positive(self):
        self.assertEqual(oracle.judge(CASE, result(), RUN, SKILLS)['verdict'], 'PASS')

    def test_each_required_record_must_exist(self):
        for kind in ['START', 'FIXTURE', 'SUT_DONE', 'FINAL', 'END']:
            with self.subTest(kind=kind):
                recs = [r for r in records() if r['kind'] != kind]
                self.assertFalse(evidence(recs)['complete'])

    def test_duplicate_and_reordered_records_are_incomplete(self):
        for recs in [records() + [{'kind': 'END'}], list(reversed(records())),
                     records()[:2] + [records()[1]] + records()[2:]]:
            self.assertFalse(evidence(recs)['complete'])

    def test_extra_export_is_tampering(self):
        self.assertTrue(evidence(files={**MARKERS, 'unexpected': 'x'})['tampered'])

    def test_missing_request_export_is_not_complete(self):
        recs = records()
        recs[-2]['markers'] = {}
        self.assertFalse(evidence(recs, {})['complete'])

    def test_malformed_payloads_never_complete(self):
        for index, field, value in [(1, 'home', None), (3, 'result', []), (4, 'markers', None)]:
            with self.subTest(field=field):
                recs = records(); recs[index][field] = value
                self.assertFalse(evidence(recs)['complete'])

    def test_request_digest_mismatch_fails(self):
        recs = records(); recs[2]['sha256'] = '0' * 64
        self.assertEqual(oracle.judge(CASE, result(evidence(recs)), RUN, SKILLS)['verdict'], 'FAIL')

    def test_stream_correct_wrong_key_and_missing_end(self):
        out = io.StringIO(); key = b'k' * 32; emitter = stream.Emitter(key, out)
        for rec in records():
            emitter.emit(rec['kind'], **{k: v for k, v in rec.items() if k != 'kind'})
        self.assertEqual(stream.verify(key, out.getvalue())[1], [])
        self.assertTrue(stream.verify(b'z' * 32, out.getvalue())[1])
        self.assertIn('missing_END', stream.verify(key, '\n'.join(out.getvalue().splitlines()[:-1]))[1])


class SpecTests(unittest.TestCase):
    def test_invalid_spec_refused_before_staging_or_launch(self):
        from run_case import run_case
        with patch('run_case.stage_fixture') as stage, patch('run_case.run_sandbox') as launch:
            r = run_case('/unused', '/unused/evidence', {'id': 'invalid', 'sut_argv': ['/fake'],
                         'expect': {'requests': [0, 0], 'stdout_contains': [], 'stdout_absent': []}})
            self.assertEqual(r['verdict'], 'INCONCLUSIVE')
            stage.assert_not_called(); launch.assert_not_called()

    def test_empty_controls_do_not_pass(self):
        c = {'id': 'empty', 'expect': {'requests': [0, 0], 'stdout_contains': [], 'stdout_absent': []}}
        self.assertTrue(oracle.lint(c))

    def test_bad_ranges_and_unknown_keys_are_rejected(self):
        for change in [{'requests': [None, None]}, {'requests': [-1, 1]}, {'requests': [2, 1]},
                       {'requests': [True, 1]}, {'requests': '1,2'}, {'typo': ['beta']},
                       {'desc_present': 'alpha'}, {'desc_absent': ['']}, {'stdout_contains': ['']},
                       {'victim_present': 1}, {'validator': 'yes'}, {'exit_code': False},
                       {'tool_attempt': {}}, {'tool_attempt': {'text_contains': '', 'is_error': True}}]:
            with self.subTest(change=change):
                c = copy.deepcopy(CASE); c['expect'].update(change)
                self.assertTrue(oracle.lint(c))

    def test_unknown_canary_is_not_evidence_of_absence(self):
        c = copy.deepcopy(CASE); c['expect']['desc_absent'] = ['misspelled']
        self.assertNotEqual(oracle.judge(c, result(), RUN, SKILLS)['verdict'], 'PASS')

    def test_legitimate_zero_request_refusal(self):
        c = {'id': 'refusal', 'expect': {'requests': [0, 0], 'stdout_contains': ['disabled'], 'body_absent': ['beta']}}
        r = result(); r['analysis'] = oracle.analyse(RUN, [], SKILLS); r['stdout'] = 'disabled'
        self.assertEqual(oracle.judge(c, r, RUN, SKILLS)['verdict'], 'PASS')
        r['stdout'] = ''
        self.assertEqual(oracle.judge(c, r, RUN, SKILLS)['verdict'], 'FAIL')


class RuntimeTests(unittest.TestCase):
    def test_unsupported_cell_modes_are_reported(self):
        from screen import render
        self.assertEqual(render(b'\x1b[?25lok').unknown, 0)
        for raw in [b'\x1b[4h', b'\x1b[?6h', b'\x1b[?7l', b'\x1bc']:
            with self.subTest(raw=raw):
                self.assertGreater(render(raw).unknown, 0)

    def test_runner_errors_and_timeout_never_pass(self):
        for inside in [{'exit': 0}, {'exit': 0, 'timed_out': True},
                       {'exit': 0, 'timed_out': False, 'harness_error': 'synthetic'},
                       {'exit': 0, 'timed_out': False, 'teardown_error': 'synthetic'},
                       {'exit': 0, 'timed_out': False, 'stop_errors': ['synthetic']},
                       {'exit': 19, 'timed_out': False}]:
            with self.subTest(inside=inside):
                r = result(); r['inside'] = inside
                self.assertNotEqual(oracle.judge(CASE, r, RUN, SKILLS)['verdict'], 'PASS')

    def test_expected_nonzero_exit_is_valid_negative_control(self):
        c = copy.deepcopy(CASE); c['expect']['exit_code'] = 19
        r = result(); r['inside']['exit'] = 19
        self.assertEqual(oracle.judge(c, r, RUN, SKILLS)['verdict'], 'PASS')

    def test_pty_requires_clean_drain_supported_screen_and_response(self):
        c = {**CASE, 'mode': 'I'}
        inside = {'mode': 'I', 'exit': {'clean': True, 'code': 0, 'forced': False}, 'timed_out': False,
                  'response_rendered': True, 'pty_eof': True, 'screen_unknown': 0, 'screen_pending': 0}
        r = result(); r['inside'] = inside
        self.assertEqual(oracle.judge(c, r, RUN, SKILLS)['verdict'], 'PASS')
        for field, value in [('pty_eof', False), ('screen_unknown', 1), ('screen_pending', 1),
                             ('response_rendered', False), ('exit', {'clean': False, 'code': 0, 'forced': True})]:
            with self.subTest(field=field):
                r['inside'] = {**inside, field: value}
                self.assertNotEqual(oracle.judge(c, r, RUN, SKILLS)['verdict'], 'PASS')


class IsolationTests(unittest.TestCase):
    def test_preflight_rejects_launcher_and_host_control_failures(self):
        import importlib.util
        from tempfile import TemporaryDirectory
        from types import SimpleNamespace
        path = Path(__file__).resolve().parents[1] / 'harness-v2' / 'run_preflight.py'
        variants = [({}, True, 0, 0), ({'exit': 1}, True, 0, 1),
                    ({'timed_out': True}, True, 0, 1), ({'killed': 'timeout'}, True, 0, 1),
                    ({'readers_complete': False}, True, 0, 1), ({'pid_ns': None}, True, 0, 1),
                    ({}, False, 0, 1), ({}, True, 1, 1)]
        for delta, control_discriminates, control_exit, expected in variants:
            with self.subTest(delta=delta, control=control_discriminates, exit=control_exit), \
                    TemporaryDirectory(prefix='a0-preflight-gate-') as root, \
                    patch.object(sys, 'argv', ['run_preflight.py', root, root + '/out']):
                spec = importlib.util.spec_from_file_location('a0_preflight_gate', path)
                module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
                r = {'stdout': json.dumps({'all_pass': True, 'checks': {'probe': {'pass': True}}}),
                     'stderr': '', 'argv': [], 'exit': 0, 'timed_out': False, 'killed': None,
                     'members_max': 2, 'leftovers': [], 'pid_ns': 123, 'duration_s': 0,
                     'markers_peak': 0, 'readers_complete': True, **delta}
                host = SimpleNamespace(stdout=json.dumps({'checks': {'probe': {'pass': not control_discriminates}}}),
                                       returncode=control_exit)
                with patch.object(module.integrity, 'main'), patch.object(module.integrity, 'provider_pids', return_value=[]), \
                        patch.object(module, 'run_sandbox', return_value=r), \
                        patch.object(module.subprocess, 'run', side_effect=[host, SimpleNamespace(returncode=0)]), \
                        patch('sys.stdout', new_callable=io.StringIO):
                    self.assertEqual(module.main(), expected)

    def test_yama_required_before_any_launch(self):
        for val in ['0\n', '', 'bad']:
            with patch('builtins.open', return_value=io.StringIO(val)):
                with self.assertRaises(RuntimeError):
                    limits.require_ptrace_restriction()
        with patch('builtins.open', side_effect=OSError('unavailable')):
            with self.assertRaises(RuntimeError):
                limits.require_ptrace_restriction()
        for val in ['1\n', '2\n', '3\n']:
            with patch('builtins.open', return_value=io.StringIO(val)):
                limits.require_ptrace_restriction()

    def test_unsafe_yama_does_not_spawn(self):
        from launch import run_sandbox
        with patch('limits.require_ptrace_restriction', side_effect=RuntimeError('unsafe')), \
                patch('subprocess.Popen') as spawn:
            with self.assertRaises(RuntimeError):
                run_sandbox(lambda fd: ['/no-executable'])
            spawn.assert_not_called()

    def test_missing_status_kills_reaps_and_closes_pipes(self):
        from launch import run_sandbox
        from unittest.mock import Mock
        p = Mock(stdout=io.BytesIO(), stderr=io.BytesIO(), stdin=io.BytesIO())
        p.poll.return_value = None
        def kill(*args):
            p.poll.return_value = 0
        with patch('limits.require_ptrace_restriction'), patch('subprocess.Popen', return_value=p), \
                patch('launch._read_status_first', return_value=None), patch('launch._kill', side_effect=kill) as stop:
            with self.assertRaisesRegex(RuntimeError, 'sandbox_identity_unavailable'):
                run_sandbox(lambda fd: ['/no-executable'])
            stop.assert_called_once()
            p.wait.assert_called_once()
            self.assertTrue(p.stdout.closed and p.stderr.closed and p.stdin.closed)

    def test_missing_namespace_is_unknown_not_no_leftovers(self):
        from run_case import isolation_checks
        r = {'pid_ns': None, 'init_pid': None, 'leftovers': [], 'killed': None, 'exit': 0,
             'timed_out': False, 'readers_complete': True}
        self.assertFalse(isolation_checks(r, True, [], [])['namespace_verified'])
        r.update(pid_ns=123, init_pid=456)
        self.assertTrue(all(isolation_checks(r, True, [], []).values()))
        self.assertFalse(isolation_checks(r, True, [], None)['owned_files_unchanged'])
        r['readers_complete'] = False
        self.assertFalse(isolation_checks(r, True, [], [])['launcher_complete'])


class ReproductionTests(unittest.TestCase):
    def test_fresh_output_and_hash_checked_reference_copy(self):
        import importlib.util
        from tempfile import TemporaryDirectory
        from types import SimpleNamespace
        path = Path(__file__).resolve().parents[1] / 'run_offline.py'
        spec = importlib.util.spec_from_file_location('a0_offline_runner', path)
        module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
        with TemporaryDirectory(prefix='a0-reproduction-') as root:
            root = Path(root); historical = root / 'historical'; historical.mkdir()
            (historical / 'harness').mkdir()
            source = historical / 'harness/reference.py'; source.write_text('# frozen reference\n')
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps({'files': {'harness/reference.py': {
                'sha256': hashlib.sha256(source.read_bytes()).hexdigest()}}}))
            module.MANIFEST = manifest
            module.SUITES = ('fail_closed',)
            module.CONTRACT = root / 'suite-contract.json'
            module.CONTRACT.write_text(json.dumps({'fail_closed': ['fixture_check']}))
            original_argv = ['run_offline.py', '--historical-root', str(historical), '--output-root']
            def completed(command, **kwargs):
                Path(command[-1]).write_text(json.dumps({'suite': 'fail_closed', 'tests_run': 1,
                                                        'checks': {'fixture_check': {'pass': True}}}))
                return SimpleNamespace(returncode=0, stdout='', stderr='')
            with patch.object(module.subprocess, 'run', side_effect=completed) as run, \
                    patch('sys.stdout', new_callable=io.StringIO), patch('sys.stderr', new_callable=io.StringIO):
                with patch.object(sys, 'argv', original_argv + [str(historical / 'forbidden')]):
                    with self.assertRaises(SystemExit): module.main()
                run.assert_not_called()
                output = root / 'new'
                with patch.object(sys, 'argv', original_argv + [str(output)]):
                    self.assertEqual(module.main(), 0)
                    self.assertEqual((output / 'harness/reference.py').read_bytes(), source.read_bytes())
                    with self.assertRaises(FileExistsError): module.main()
                # A process that exits successfully but reports no executed checks must fail.
                run.side_effect = lambda *a, **kw: SimpleNamespace(returncode=0, stdout='', stderr='')
                empty_output = root / 'empty-success'
                with patch.object(sys, 'argv', original_argv + [str(empty_output)]):
                    self.assertEqual(module.main(), 1)
                summary = json.loads((empty_output / 'results/summary.json').read_text())
                self.assertFalse(summary['fail_closed']['passed'])
                self.assertEqual(summary['fail_closed']['checks'], 0)
                run.reset_mock()
                source.write_text('# changed since freeze\n')
                with patch.object(sys, 'argv', original_argv + [str(root / 'tampered')]):
                    with self.assertRaisesRegex(RuntimeError, 'historical hash mismatch'): module.main()
                run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
