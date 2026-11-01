"""The host-side bwrap process must not inherit the owner's environment. No provider runs."""
import importlib.util
import io
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

PROBE = Path(__file__).resolve().parents[1] / 'probe'


def load(name):
    spec = importlib.util.spec_from_file_location('probe_env_' + name, PROBE / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ProbeEnvironmentTests(unittest.TestCase):
    def test_run_uses_empty_host_environment_and_preserves_explicit_child_environment(self):
        sandbox = load('sandbox')
        with patch.dict(os.environ, {'A0_REVIEW_POISON': 'synthetic'}), patch.object(sandbox.subprocess, 'run') as run:
            argv = sandbox.argv(home='/fixture', workdir='/home/probe', env={'A0_EXPLICIT': 'yes'}, command=['/fake'])
            sandbox.run(argv, timeout=3)
        self.assertEqual(run.call_args.kwargs.get('env'), {})
        self.assertIn('--clearenv', argv)
        self.assertIn('A0_EXPLICIT', argv)
        self.assertNotIn('A0_REVIEW_POISON', argv)

    def test_bridge_session_popen_also_uses_empty_host_environment(self):
        # Isolate the probe's absolute module imports from other test suites' modules.
        sandbox = load('sandbox')
        matrix = Mock()
        with patch.dict(sys.modules, {'sandbox': sandbox, 'matrix': matrix}):
            driver = load('driver')
        proc = Mock(stdout=io.BytesIO(), stderr=io.BytesIO())
        with tempfile.TemporaryDirectory(prefix='a0-probe-env-') as root, \
                patch.dict(os.environ, {'A0_REVIEW_POISON': 'synthetic'}), \
                patch.object(driver.subprocess, 'Popen', return_value=proc) as spawn, \
                patch.object(driver.threading.Thread, 'start'):
            driver.BridgeSession('/fixture-venv', root=root, extra_env={'A0_EXPLICIT': 'yes'})
        self.assertEqual(spawn.call_args.kwargs.get('env'), {})
        argv = spawn.call_args.args[0]
        self.assertIn('--clearenv', argv)
        self.assertIn('A0_EXPLICIT', argv)
        self.assertNotIn('A0_REVIEW_POISON', argv)
        matrix.seed.assert_not_called()


if __name__ == '__main__':
    unittest.main()
