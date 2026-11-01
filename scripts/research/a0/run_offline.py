"""Reproduce A0 offline suites in a NEW run-root; never write the historical evidence.

Only fake providers run. Historical reference code/captures must match the committed manifest.
Usage: python3 -B scripts/research/a0/run_offline.py --historical-root PATH --output-root NEW_PATH
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import unittest

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
MANIFEST = REPO / 'docs/research/arsenal-a0-2026-10-29/evidence/MANIFEST-v2.json'
CONTRACT = HERE / 'tests' / 'offline-suite-contract.json'
UNIT_SUITES = ('fail_closed', 'targeted_contracts')
SUITES = UNIT_SUITES + ('validator_v2', 'screen_v2', 'oracle_v2', 'driver_v2', 'e2e_offline')


def expected_checks(suite):
    checks = json.loads(CONTRACT.read_text())[suite]
    if not isinstance(checks, list) or not checks or len(set(checks)) != len(checks):
        raise ValueError('invalid suite coverage contract: ' + suite)
    return set(checks)


def suite_inventory_errors(results, expected):
    missing, extra = set(expected) - set(results), set(results) - set(expected)
    return (['suite_missing:' + name for name in sorted(missing)]
            + ['suite_unexpected:' + name for name in sorted(extra)])


def validate_report(suite, path):
    """Exit 0 alone is insufficient: every planned check must report a literal true."""
    expected = expected_checks(suite)
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError) as e:
        return 0, ['report_missing_or_invalid:' + type(e).__name__]
    errors = []
    if suite in UNIT_SUITES:
        if not isinstance(data, dict):
            return 0, ['unit_report_not_object']
        checks = data.get('checks')
        if data.get('suite') != suite or type(data.get('tests_run')) is not int or data['tests_run'] != len(expected):
            errors.append('unit_run_count_or_identity_mismatch')
    else:
        checks = data
    if not isinstance(checks, dict):
        return 0, errors + ['checks_not_object']
    if set(checks) != expected:
        errors.append('check_inventory_mismatch')
    if not checks:
        errors.append('zero_checks')
    if any(not isinstance(v, dict) or v.get('pass') is not True for v in checks.values()):
        errors.append('check_not_passed')
    return len(checks), errors


class RecordingResult(unittest.TextTestResult):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.checks = {}

    def startTest(self, test):
        super().startTest(test)
        self.checks[test.id()] = {'pass': False}

    def addSuccess(self, test):
        super().addSuccess(test)
        self.checks[test.id()] = {'pass': True}


def report_unit(suite, path):
    """Execute only the closed offline unittest suites and save IDs/outcomes/counts."""
    if suite not in UNIT_SUITES:
        raise ValueError('unknown unit suite')
    tests = unittest.TestLoader().discover(str(HERE / 'tests'), pattern='test_' + suite + '.py')
    result = unittest.TextTestRunner(resultclass=RecordingResult).run(tests)
    path.write_text(json.dumps({'suite': suite, 'tests_run': result.testsRun, 'checks': result.checks}, indent=2) + '\n')
    _, errors = validate_report(suite, path)
    return 0 if result.wasSuccessful() and not errors else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--historical-root', type=Path, required=True)
    parser.add_argument('--output-root', type=Path, required=True)
    args = parser.parse_args()
    historical = args.historical_root.resolve(strict=True)
    output = args.output_root.resolve()
    if output.is_relative_to(historical) or output.is_relative_to(REPO):
        parser.error('output must be outside both historical evidence and the checkout')
    expected_suites = set(json.loads(CONTRACT.read_text()))
    inventory_errors = suite_inventory_errors(dict.fromkeys(SUITES), expected_suites)
    if inventory_errors or not expected_suites:
        parser.error('invalid suite inventory: ' + ','.join(inventory_errors))
    # Never reuse an output directory, even following a failed run.
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    manifest = json.loads(MANIFEST.read_text())
    refs = {rel: meta for rel, meta in manifest['files'].items()
            if rel.startswith('harness/') or
            (rel.startswith('evidence/isolation/') and rel.endswith('/markers/requests.jsonl')) or
            rel in ('evidence/A1-I/markers/screen.raw', 'evidence/A1-I-retest/markers/screen.raw')}
    for rel, meta in refs.items():
        source = historical / rel
        if source.is_symlink() or not source.resolve().is_relative_to(historical):
            raise RuntimeError('unsafe historical path: ' + rel)
        data = source.read_bytes()
        if hashlib.sha256(data).hexdigest() != meta['sha256']:
            raise RuntimeError('historical hash mismatch: ' + rel)
        target = output / rel
        target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        target.write_bytes(data)
    # Updated harness code is separate from the frozen v1 references and captures.
    for source in (HERE / 'harness-v2').rglob('*.py'):
        target = output / 'harness-v2' / source.relative_to(HERE / 'harness-v2')
        target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        target.write_bytes(source.read_bytes())
    results_dir = output / 'results'
    results_dir.mkdir(mode=0o700)
    results = {}
    for suite in SUITES:
        script = HERE / 'tests' / ('test_' + suite + '.py')
        report = results_dir / (suite + '.json')
        command = ([sys.executable, '-B', '-I', str(Path(__file__).resolve()), 'unit-report', suite, str(report)]
                   if suite in UNIT_SUITES else [sys.executable, '-B', '-I', str(script), str(output), str(report)])
        p = subprocess.run(command, capture_output=True, text=True)
        (results_dir / (suite + '.out')).write_text(p.stdout + p.stderr)
        count, errors = validate_report(suite, report)
        passed = p.returncode == 0 and not errors
        results[suite] = {'exit': p.returncode, 'command': command, 'passed': passed,
                          'checks': count, 'expected_checks': len(expected_checks(suite)), 'errors': errors}
        print(suite, 'PASS' if passed else 'FAIL', 'exit=' + str(p.returncode),
              f'checks={count}/{len(expected_checks(suite))}', flush=True)
    inventory_errors = suite_inventory_errors(results, expected_suites)
    (results_dir / 'summary.json').write_text(json.dumps(results, indent=2) + '\n')
    print('Evidence:', results_dir, flush=True)
    return 0 if not inventory_errors and all(r['passed'] for r in results.values()) else 1


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == 'unit-report':
        parser = argparse.ArgumentParser(description='Internal offline unittest report adapter')
        parser.add_argument('mode', choices=['unit-report'])
        parser.add_argument('suite', choices=UNIT_SUITES)
        parser.add_argument('report', type=Path)
        args = parser.parse_args()
        sys.exit(report_unit(args.suite, args.report))
    sys.exit(main())
