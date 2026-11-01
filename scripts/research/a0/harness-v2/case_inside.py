"""In-sandbox case runner (v2).

1. Reads the per-case HMAC key from stdin, then points stdin at /dev/null.
2. Copies the read-only fixture (/a0/fixture/{home,work}) into the size-capped tmpfs HOME/work.
3. Emits a live, HMAC-chained record for: fixture digests, every mock request (as it is
   recorded), the system-under-test's exit, final HOME/work digests and every /markers file,
   then exports the /markers files themselves (FILE_CHUNK/FILE_END): /markers is a capped
   tmpfs and the sandbox has no writable host bind.
4. Runs the system under test headless (H) or in a PTY (I) inside try/finally, so the END
   record is always written and the child is always torn down.
`spec["sut_argv"]` defaults to the real provider path; offline tests substitute a fake.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time

sys.path.insert(0, "/a0/harness")
from mock_anthropic import ConnectLogger, MockAnthropic  # noqa: E402
from sandbox import SYNTH_KEY  # noqa: E402
from stream import Emitter  # noqa: E402

M = "/markers"
key = bytes.fromhex(sys.stdin.readline().strip() or "00")
devnull = os.open("/dev/null", os.O_RDONLY)
os.dup2(devnull, 0)
os.close(devnull)
out = os.fdopen(os.dup(1), "w", buffering=1)
em = Emitter(key, out)
del key


def tree(path):
    res = {}
    for d, dirs, files in os.walk(path):
        dirs[:] = [x for x in dirs if x != ".git"]
        for n in files:
            p = os.path.join(d, n)
            if os.path.islink(p):
                res[os.path.relpath(p, path)] = "link"
            else:
                try:
                    res[os.path.relpath(p, path)] = hashlib.sha256(open(p, "rb").read()).hexdigest()
                except OSError as e:
                    res[os.path.relpath(p, path)] = f"unreadable:{e.errno}"
    return dict(sorted(res.items()))


CAP_TEXT = 1024 * 1024


def safe_write(path, data, mode="w"):
    """Best-effort write into /markers (which the system under test can fill); never raises."""
    try:
        with open(path, mode) as f:
            f.write(data)
    except OSError as e:
        em.emit("WRITE_ERROR", path=os.path.relpath(path, M), errno=e.errno)


def copy_in(src, dst):
    if os.path.isdir(src):
        for name in os.listdir(src):
            s, d = os.path.join(src, name), os.path.join(dst, name)
            if os.path.isdir(s) and not os.path.islink(s):
                shutil.copytree(s, d, symlinks=True)
            else:
                shutil.copy2(s, d, follow_symlinks=False)


spec = json.load(open("/a0/case/spec.json"))
em.emit("START", case=spec.get("id"), mode=spec["mode"])
copy_in("/a0/fixture/home", "/home/probe")
copy_in("/a0/fixture/work", "/work")
em.emit("FIXTURE", home=tree("/home/probe"), work=tree("/work"))
mock = MockAnthropic(f"{M}/requests.jsonl", spec["script"], port=18080,
                     on_record=lambda b: em.emit("REQUEST", sha256=hashlib.sha256(b).hexdigest(), bytes=len(b)))
proxy = ConnectLogger(f"{M}/proxy-attempts.log").start()
mock.start()
safe_write(f"{M}/proxy-attempts.log", "", "a")
env = dict(os.environ)
env.update({
    "ANTHROPIC_API_KEY": SYNTH_KEY, "ANTHROPIC_BASE_URL": mock.url,
    "HTTPS_PROXY": proxy.url, "HTTP_PROXY": proxy.url, "https_proxy": proxy.url, "http_proxy": proxy.url,
    "NO_PROXY": "127.0.0.1,localhost", "no_proxy": "127.0.0.1,localhost",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "DISABLE_AUTOUPDATER": "1",
    "DISABLE_TELEMETRY": "1", "DISABLE_ERROR_REPORTING": "1", "DISABLE_BUG_COMMAND": "1",
})
sut = spec.get("sut_argv") or ["/opt/a0/bin/claude"]
args = [a.replace("{OVERLAY}", "/a0/case/overlay.json") for a in spec.get("args", [])]
result = {"mode": spec["mode"], "argv": sut + args + (["-p", spec["prompt"]] if spec["mode"] == "H" else [])}
t0 = time.time()
session = None
try:
    if spec["mode"] == "H":
        try:
            r = subprocess.run(result["argv"], cwd="/work", env=env, capture_output=True, text=True,
                               timeout=spec.get("sut_timeout", 120), stdin=subprocess.DEVNULL,
                               start_new_session=True)
            result.update(exit=r.returncode, timed_out=False,
                          stdout=r.stdout[:CAP_TEXT], stderr=r.stderr[:CAP_TEXT])
        except subprocess.TimeoutExpired as e:
            so = (e.stdout or b"")
            result.update(exit=None, timed_out=True,
                          stdout=(so.decode("utf-8", "replace") if isinstance(so, bytes) else so)[:CAP_TEXT], stderr="")
    else:
        from pty_driver import PtySession
        session = PtySession(sut + args, env, "/work")
        session.pump(spec.get("startup_wait", 12))
        session.send(spec["prompt"].encode(), "typed prompt")
        session.pump(1)
        session.send(b"\r", "enter")
        deadline = time.time() + 60
        while time.time() < deadline and mock.calls == 0 and session.pump(0.5):
            pass
        session.log("requests_seen", n=mock.calls)
        rendered = session.wait_visible(spec.get("expect_screen", "ok from mock"), 20)
        session.log("response_visible" if rendered else "response_not_visible")
        session.pump(2)
        result.update(exit=session.exit_with_ctrl_c(), response_rendered=rendered, timed_out=mock.calls == 0)
except Exception as e:  # recorded, never swallowed silently
    result["harness_error"] = f"{type(e).__name__}: {e}"
finally:
    try:
        if session is not None:
            session.close(reason="finally")
            safe_write(f"{M}/screen.raw", bytes(session.raw), "wb")
            result["stdout"] = session.screen.seen_text()[:CAP_TEXT]
            result["screen_visible"] = session.screen.visible_text()[:CAP_TEXT]
            result["pty_eof"] = session.eof
            result["screen_unknown"] = session.screen.unknown
            result["screen_pending"] = len(session.screen.pending)
            result["stderr"] = ""
            result["driver_audit"] = session.audit[-200:]
    except Exception as e:
        result["teardown_error"] = f"{type(e).__name__}: {e}"
    result["duration_s"] = round(time.time() - t0, 1)
    result["mock_calls"] = mock.calls
    for stop in (mock.stop, proxy.stop):
        try:
            stop()
        except Exception as e:
            result.setdefault("stop_errors", []).append(f"{type(e).__name__}")
    em.emit("SUT_DONE", result=result)
    em.emit("FINAL", home=tree("/home/probe"), work=tree("/work"), markers=tree(M))
    # Export /markers (kernel-capped tmpfs) through the authenticated stream; nothing in the
    # sandbox can write the host disk directly.
    import base64
    for d, _, files in os.walk(M):
        for n in sorted(files):
            p = os.path.join(d, n)
            rel = os.path.relpath(p, M)
            try:
                data = open(p, "rb").read()
            except OSError as e:
                em.emit("FILE_ERROR", path=rel, errno=e.errno)
                continue
            for i in range(0, max(len(data), 1), 49152):
                em.emit("FILE_CHUNK", path=rel, offset=i, b64=base64.b64encode(data[i:i + 49152]).decode())
            em.emit("FILE_END", path=rel, sha256=hashlib.sha256(data).hexdigest(), size=len(data))
    em.emit("END")
