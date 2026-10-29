"""Runs inside the sandbox: starts the mock Messages API and a refusing proxy logger on the
private loopback, then the provider in mode H (`-p ping`) or I (PTY, type `ping`).
All evidence goes to /markers. Usage: python3 -I a1_inside.py H|I
"""
import json
import os
import pty
import select
import signal
import subprocess
import sys
import time

sys.path.insert(0, "/a0/harness")
from mock_anthropic import ConnectLogger, MockAnthropic  # noqa: E402
from sandbox import SYNTH_KEY  # noqa: E402

mode = sys.argv[1]
M = "/markers"
mock = MockAnthropic(f"{M}/requests.jsonl", [{"text": "ok from mock"}], port=18080).start()
proxy = ConnectLogger(f"{M}/proxy-attempts.log").start()
open(f"{M}/proxy-attempts.log", "a").close()
env = dict(os.environ)
env.update({
    "ANTHROPIC_API_KEY": SYNTH_KEY,
    "ANTHROPIC_BASE_URL": mock.url,
    "HTTPS_PROXY": proxy.url, "HTTP_PROXY": proxy.url,
    "https_proxy": proxy.url, "http_proxy": proxy.url,
    "NO_PROXY": "127.0.0.1,localhost", "no_proxy": "127.0.0.1,localhost",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    "DISABLE_AUTOUPDATER": "1", "DISABLE_TELEMETRY": "1",
    "DISABLE_ERROR_REPORTING": "1", "DISABLE_BUG_COMMAND": "1",
})
argv = ["/opt/a0/bin/claude"] + (["-p", "ping"] if mode == "H" else [])
result = {"mode": mode, "argv": argv, "env_names": sorted(env)}
t0 = time.time()


def messages_count():
    try:
        return sum(1 for l in open(f"{M}/requests.jsonl")
                   if json.loads(l)["path"].startswith("/v1/messages")
                   and not json.loads(l)["path"].startswith("/v1/messages/count_tokens"))
    except FileNotFoundError:
        return 0


if mode == "H":
    try:
        r = subprocess.run(argv, cwd="/work", env=env, capture_output=True, text=True,
                           timeout=120, stdin=subprocess.DEVNULL)
        result.update(exit=r.returncode, timed_out=False)
        open(f"{M}/stdout.txt", "w").write(r.stdout)
        open(f"{M}/stderr.txt", "w").write(r.stderr)
    except subprocess.TimeoutExpired as e:
        result.update(exit=None, timed_out=True)
        open(f"{M}/stdout.txt", "w").write((e.stdout or b"").decode("utf-8", "replace") if isinstance(e.stdout, bytes) else (e.stdout or ""))
else:
    from pty_driver import PtySession
    s = PtySession(argv, env, "/work")
    s.pump(12)
    s.send(b"ping", "typed ping")
    s.pump(1)
    s.send(b"\r", "enter")
    deadline = time.time() + 60
    while time.time() < deadline and messages_count() == 0 and s.pump(0.5):
        pass
    s.log("requests_seen", n=messages_count())
    rendered = s.wait_visible("ok from mock", 20)
    s.log("response_visible" if rendered else "response_not_visible")
    s.pump(2)
    exit_res = s.exit_with_ctrl_c()
    s.pump(0.5)
    result.update(exit=exit_res, response_rendered=rendered, timed_out=messages_count() == 0)
    open(f"{M}/screen.raw", "wb").write(bytes(s.raw))
    open(f"{M}/screen.txt", "w").write(s.screen.seen_text())
    open(f"{M}/screen-final.txt", "w").write(s.screen.text())
    json.dump(s.audit, open(f"{M}/driver-audit.json", "w"), indent=1)

result["duration_s"] = round(time.time() - t0, 1)
result["messages_requests"] = messages_count()
json.dump(result, open(f"{M}/inside-result.json", "w"), indent=1)
mock.stop()
proxy.stop()
