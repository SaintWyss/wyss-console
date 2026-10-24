"""Serves the mock Messages API on loopback for tests that start the real bridge: prints
`PORT <n>` once listening and serves until its stdin closes.
Usage: serve_mock.py <script.json> <log-file>"""
import json, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mock_anthropic import MockAnthropic

script = json.load(open(sys.argv[1]))
mock = MockAnthropic(sys.argv[2], script).start()
print("PORT %d" % mock.server.server_address[1], flush=True)
sys.stdin.read()
mock.stop()
