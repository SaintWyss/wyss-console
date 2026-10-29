"""Generic in-sandbox case runner. Reads /a0/case/spec.json (read-only bind), starts the
loopback mock with the case's script and the refusing proxy logger, runs the provider headless
(H) or in a PTY (I). Evidence goes to /markers."""
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, "/a0/harness")
from mock_anthropic import ConnectLogger, MockAnthropic  # noqa: E402
from sandbox import SYNTH_KEY  # noqa: E402

M = "/markers"
spec = json.load(open("/a0/case/spec.json"))
mock = MockAnthropic(f"{M}/requests.jsonl", spec["script"], port=18080).start()
proxy = ConnectLogger(f"{M}/proxy-attempts.log").start()
open(f"{M}/proxy-attempts.log", "a").close()
env = dict(os.environ)
env.update({
    "ANTHROPIC_API_KEY": SYNTH_KEY, "ANTHROPIC_BASE_URL": mock.url,
    "HTTPS_PROXY": proxy.url, "HTTP_PROXY": proxy.url, "https_proxy": proxy.url, "http_proxy": proxy.url,
    "NO_PROXY": "127.0.0.1,localhost", "no_proxy": "127.0.0.1,localhost",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "DISABLE_AUTOUPDATER": "1",
    "DISABLE_TELEMETRY": "1", "DISABLE_ERROR_REPORTING": "1", "DISABLE_BUG_COMMAND": "1",
})
args = [a.replace("{OVERLAY}", "/a0/case/overlay.json") for a in spec["args"]]
argv = ["/opt/a0/bin/claude"] + args
result = {"mode": spec["mode"], "argv": argv + (["-p", spec["prompt"]] if spec["mode"] == "H" else [])}
t0 = time.time()


def messages_count():
    try:
        return sum(1 for l in open(f"{M}/requests.jsonl")
                   if json.loads(l)["path"].startswith("/v1/messages")
                   and not json.loads(l)["path"].startswith("/v1/messages/count_tokens"))
    except FileNotFoundError:
        return 0


if spec["mode"] == "H":
    try:
        r = subprocess.run(result["argv"], cwd="/work", env=env, capture_output=True, text=True,
                           timeout=120, stdin=subprocess.DEVNULL)
        result.update(exit=r.returncode, timed_out=False)
        open(f"{M}/stdout.txt", "w").write(r.stdout)
        open(f"{M}/stderr.txt", "w").write(r.stderr)
    except subprocess.TimeoutExpired:
        result.update(exit=None, timed_out=True)
else:
    from pty_driver import PtySession
    s = PtySession(argv, env, "/work")
    s.pump(12)
    s.send(spec["prompt"].encode(), "typed prompt")
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
    result.update(exit=exit_res, response_rendered=rendered, timed_out=messages_count() == 0)
    open(f"{M}/screen.raw", "wb").write(bytes(s.raw))
    open(f"{M}/stdout.txt", "w").write(s.screen.seen_text())
    open(f"{M}/stderr.txt", "w").write("")
    json.dump(s.audit, open(f"{M}/driver-audit.json", "w"), indent=1)

result["duration_s"] = round(time.time() - t0, 1)
result["messages_requests"] = messages_count()
json.dump(result, open(f"{M}/inside-result.json", "w"), indent=1)
mock.stop()
proxy.stop()
