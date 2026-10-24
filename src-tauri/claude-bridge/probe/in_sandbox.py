"""Runs inside the namespace: starts the mock API on a fixed loopback port, then the bridge as a
child with this process's stdio, and exits with the bridge's status."""
import json, os, subprocess, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from mock_anthropic import MockAnthropic

script = json.loads(os.environ.get("WYSS_MOCK_SCRIPT", "null"))
mock = MockAnthropic("/tmp/requests.jsonl", script, port=18080).start()
bridge = os.environ["WYSS_BRIDGE"]
p = subprocess.Popen([sys.executable, "-I", bridge], stdin=sys.stdin, stdout=sys.stdout, stderr=sys.stderr, env={k: v for k, v in os.environ.items() if k != "WYSS_MOCK_SCRIPT"})
rc = p.wait()
try:
    import shutil
    shutil.copy("/tmp/requests.jsonl", "/markers/requests.jsonl")  # evidence for the test that launched us
except OSError:
    pass
sys.exit(rc)
