"""Directed regressions for the four REQUEST CHANGES findings. No real providers."""
import copy
import base64
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import test_fail_closed as fixture
from screen import render

oracle = fixture.oracle
ISOLATION_KEYS = ('integrity', 'namespace_verified', 'no_leftovers', 'no_unexpected_proxy',
                  'owned_files_unchanged', 'not_killed_for_storage', 'launcher_complete')


def result(ev=None):
    r = fixture.result(ev)
    r['isolation'] = dict.fromkeys(ISOLATION_KEYS, True)
    return r


def authenticated(recs):
    key = b't' * 32
    out = io.StringIO()
    em = fixture.stream.Emitter(key, out)
    for rec in recs:
        em.emit(rec['kind'], **{k: v for k, v in rec.items() if k != 'kind'})
    verified, problems, _ = fixture.stream.verify(key, out.getvalue())
    return oracle.verify_evidence(verified, problems, [fixture.LINE], fixture.MARKERS)


def load_runner():
    spec = importlib.util.spec_from_file_location('a0_runner_contract', HERE.parent / 'run_offline.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RecordContractTests(unittest.TestCase):
    def test_valid_authenticated_sequence_passes(self):
        ev = authenticated(fixture.records())
        self.assertTrue(ev['complete'])
        self.assertEqual(oracle.judge(fixture.CASE, result(ev), fixture.RUN, fixture.SKILLS)['verdict'], 'PASS')

    def test_request_before_fixture_never_passes(self):
        recs = fixture.records(); recs[1], recs[2] = recs[2], recs[1]
        ev = authenticated(recs)
        self.assertFalse(ev['complete'])
        self.assertEqual(oracle.judge(fixture.CASE, result(ev), fixture.RUN, fixture.SKILLS)['verdict'], 'INCONCLUSIVE')

    def test_event_types_are_confined_to_their_phases(self):
        events = [fixture.records()[2], {'kind': 'FILE_CHUNK', 'path': 'x', 'offset': 0, 'b64': ''},
                  {'kind': 'FILE_END', 'path': 'x', 'sha256': hashlib.sha256(b'').hexdigest(), 'size': 0}]
        # REQUEST only during RUNNING; exported files only after FINAL and before END.
        legal = {'REQUEST': {2, 3}, 'FILE_CHUNK': {5}, 'FILE_END': {5}}
        for event in events:
            for index in range(7):
                if index in legal[event['kind']]:
                    continue
                with self.subTest(kind=event['kind'], index=index):
                    recs = fixture.records(); recs.insert(index, event)
                    self.assertFalse(authenticated(recs)['complete'])

    def test_every_required_transition_is_mandatory_and_unique(self):
        for kind in ['START', 'FIXTURE', 'SUT_DONE', 'FINAL', 'END']:
            with self.subTest(kind=kind):
                self.assertFalse(authenticated([r for r in fixture.records() if r['kind'] != kind])['complete'])
                recs = fixture.records(); recs.insert(1, next(r for r in recs if r['kind'] == kind))
                self.assertFalse(authenticated(recs)['complete'])
        for malformed in [None, {'kind': []}, {'kind': {}}]:
            self.assertTrue(oracle.record_contract_problems([malformed]))

    def test_exports_are_allowed_after_final(self):
        recs = fixture.records()
        recs[5:5] = [{'kind': 'FILE_CHUNK', 'path': 'requests.jsonl', 'offset': 0,
                      'b64': base64.b64encode(fixture.LINE.encode()).decode()},
                     {'kind': 'FILE_END', 'path': 'requests.jsonl', 'sha256': fixture.DIGEST, 'size': len(fixture.LINE)}]
        self.assertTrue(authenticated(recs)['complete'])

    def test_diagnostic_errors_remain_incomplete(self):
        for event, index in [({'kind': 'WRITE_ERROR'}, 3), ({'kind': 'FILE_ERROR'}, 5)]:
            recs = fixture.records(); recs.insert(index, event)
            self.assertFalse(authenticated(recs)['complete'])


class AnsiContractTests(unittest.TestCase):
    def test_supported_combinations_preserve_alternate_screen(self):
        s = render(b'MAIN\x1b[?25;1049;2004hALT\x1b[?1049;25;2004l')
        self.assertEqual(s.unknown, 0)
        self.assertEqual(s.visible_lines()[0], 'MAIN')
        self.assertIn('ALT', s.seen_text())

    def test_unknown_parameters_are_all_counted(self):
        for raw, expected in [(b'\x1b[?6h', 1), (b'\x1b[?6;7h', 2),
                              (b'\x1b[?6;1049h', 1), (b'\x1b[?1049;6h', 1),
                              (b'\x1b[?6;1049;7;25h', 2), (b'\x1b[?1049;6l', 1)]:
            with self.subTest(raw=raw): self.assertEqual(render(raw).unknown, expected)

    def test_unknown_combination_reaches_the_screen_gate(self):
        c = {**fixture.CASE, 'mode': 'I'}
        for raw, expected in [(b'\x1b[?25;1049h', 'PASS'), (b'\x1b[?6;1049h', 'INCONCLUSIVE')]:
            r = result()
            r['inside'] = {'exit': {'clean': True, 'forced': False, 'code': 0}, 'timed_out': False,
                           'response_rendered': True, 'pty_eof': True, 'screen_unknown': render(raw).unknown,
                           'screen_pending': 0}
            self.assertEqual(oracle.judge(c, r, fixture.RUN, fixture.SKILLS)['verdict'], expected)


class IsolationContractTests(unittest.TestCase):
    def test_complete_real_producer_map_passes(self):
        from run_case import isolation_checks
        launcher = {'pid_ns': 123, 'init_pid': 456, 'leftovers': [], 'killed': None, 'exit': 0,
                    'timed_out': False, 'readers_complete': True}
        iso = isolation_checks(launcher, True, [], [])
        self.assertEqual(set(iso), set(ISOLATION_KEYS))
        r = result(); r['isolation'] = iso
        self.assertEqual(oracle.judge(fixture.CASE, r, fixture.RUN, fixture.SKILLS)['verdict'], 'PASS')

    def test_missing_isolation_is_inconclusive(self):
        variants = [{}, {'integrity': True}, None]
        for key in ISOLATION_KEYS:
            iso = dict.fromkeys(ISOLATION_KEYS, True); del iso[key]; variants.append(iso)
        for iso in variants:
            with self.subTest(iso=iso):
                r = result(); r['isolation'] = iso
                self.assertEqual(oracle.judge(fixture.CASE, r, fixture.RUN, fixture.SKILLS)['verdict'], 'INCONCLUSIVE')

    def test_known_false_stops_even_with_other_missing_evidence(self):
        for key in ISOLATION_KEYS:
            for iso in [{key: False}, {**dict.fromkeys(ISOLATION_KEYS, True), key: False}]:
                with self.subTest(iso=iso):
                    r = result(); r['isolation'] = iso
                    self.assertEqual(oracle.judge(fixture.CASE, r, fixture.RUN, fixture.SKILLS)['verdict'], 'STOP_ISOLATION')

    def test_unknown_truthy_values_do_not_count_as_verified(self):
        for key in ISOLATION_KEYS:
            for value in [None, 1, 'true']:
                with self.subTest(key=key, value=value):
                    r = result(); r['isolation'][key] = value
                    self.assertEqual(oracle.judge(fixture.CASE, r, fixture.RUN, fixture.SKILLS)['verdict'], 'INCONCLUSIVE')

    def test_tamper_remains_fail_with_unknown_isolation(self):
        r = result(); r['isolation'] = {}; r['evidence']['tampered'] = True
        self.assertEqual(oracle.judge(fixture.CASE, r, fixture.RUN, fixture.SKILLS)['verdict'], 'FAIL')

    def test_invalid_spec_cannot_bypass_isolation_contract(self):
        c = copy.deepcopy(fixture.CASE); c['expect']['requests'] = [None, None]
        for iso, expected in [({}, 'INCONCLUSIVE'), ({'integrity': False}, 'STOP_ISOLATION')]:
            r = result(); r['isolation'] = iso
            self.assertEqual(oracle.judge(c, r, fixture.RUN, fixture.SKILLS)['verdict'], expected)


class OfflineRunnerContractTests(unittest.TestCase):
    def test_success_requires_exact_reported_checks(self):
        runner = load_runner()
        with tempfile.TemporaryDirectory(prefix='a0-suite-report-') as root:
            path = Path(root) / 'report.json'
            with patch.object(runner, 'expected_checks', return_value={'first', 'second'}):
                path.write_text(json.dumps({'first': {'pass': True}, 'second': {'pass': True}}))
                self.assertEqual(runner.validate_report('screen_v2', path), (2, []))
                for payload in [{}, {'first': {'pass': True}}, {'first': {'pass': True}, 'second': {'pass': False}},
                                {'first': {'pass': 1}, 'second': {'pass': True}},
                                {'first': {'pass': True}, 'other': {'pass': True}}]:
                    path.write_text(json.dumps(payload))
                    self.assertTrue(runner.validate_report('screen_v2', path)[1])
                path.unlink()
                self.assertTrue(runner.validate_report('screen_v2', path)[1])

    def test_unittest_zero_discovery_is_not_pass(self):
        runner = load_runner()
        with tempfile.TemporaryDirectory(prefix='a0-zero-discovery-') as root:
            path = Path(root) / 'unit.json'
            with patch.object(unittest.TestLoader, 'discover', return_value=unittest.TestSuite()), \
                    patch.object(runner, 'expected_checks', return_value={'test.required'}), \
                    patch('sys.stderr', new_callable=io.StringIO):
                self.assertNotEqual(runner.report_unit('fail_closed', path), 0)
                self.assertTrue(runner.validate_report('fail_closed', path)[1])

    def test_omitted_suite_is_rejected(self):
        runner = load_runner()
        self.assertTrue(runner.suite_inventory_errors({'fail_closed': {'exit': 0}}, {'fail_closed', 'screen_v2'}))
        self.assertEqual(runner.suite_inventory_errors({'fail_closed': {}, 'screen_v2': {}}, {'fail_closed', 'screen_v2'}), [])


if __name__ == '__main__':
    unittest.main()
