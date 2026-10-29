"""Offline tests for harness-v2/pty_driver.py against harness-v2/fake_tui.py (no provider), with
negative controls against frozen harness/pty_driver.py. Usage: test_driver_v2.py <A0_ROOT> <out.json>"""
import importlib.util
import json
import os
import sys
import time

ROOT = sys.argv[1]
V2 = os.path.join(ROOT, "harness-v2")
sys.path.insert(0, V2)
import pty_driver  # noqa: E402
from pty_driver import PtySession  # noqa: E402


def load_v1():
    """Frozen v1 driver + v1 screen, isolated under distinct module names."""
    s = importlib.util.spec_from_file_location("screen", os.path.join(ROOT, "harness", "screen.py"))
    m = importlib.util.module_from_spec(s)
    saved = sys.modules.get("screen")
    sys.modules["screen"] = m
    s.loader.exec_module(m)
    d = importlib.util.spec_from_file_location("pty_driver_v1", os.path.join(ROOT, "harness", "pty_driver.py"))
    dm = importlib.util.module_from_spec(d)
    d.loader.exec_module(dm)
    sys.modules["screen"] = saved
    return dm


V1 = load_v1()
FAKE = [sys.executable, "-I", os.path.join(V2, "fake_tui.py")]
results, audits = {}, {}


def env(mode="normal"):
    return {"PATH": "/usr/bin", "FAKE_MODE": mode, "FAKE_WINDOW": "0.8", "TERM": "xterm-256color"}


def case(name, ok, detail):
    results[name] = {"pass": bool(ok), "detail": detail}
    print(("PASS " if ok else "FAIL ") + name, json.dumps(detail, default=str)[:300])


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


# 1. positive: clean exit x5 (regression of the pilot fix)
for i in range(5):
    with PtySession(FAKE, env(), "/tmp") as s:
        ready = s.wait_visible("fake tui ready", 3)
        res = s.exit_with_ctrl_c()
        lat = [a["seconds"] for a in s.audit if a["event"] == "second_ctrl_c_latency"]
    case(f"positive_clean_exit_{i}", ready and res["clean"] and lat and lat[0] < 0.5, {"res": res, "lat": lat})

# 2. drain: last bytes written right before exit must be captured
with PtySession(FAKE, env("burst_exit"), "/tmp") as s:
    s.pump(5)
    got = b"BURST-END" in s.raw
    n = len(s.raw)
case("drain_captures_tail_before_exit", got, {"bytes": n})
miss = 0
for _ in range(5):
    o = V1.PtySession(FAKE, env("burst_exit"), "/tmp")
    o.pump(5)
    miss += b"BURST-END" not in o.raw
    try:
        os.close(o.fd)
    except OSError:
        pass
case("NEGCTL_v1_drain_misses_tail", miss > 0, {"v1_missed_runs_of_5": miss})

# 3. visible-now: confirmation that scrolled off must NOT count as visible
pty_driver.EXIT_WAIT = 2.0
with PtySession(FAKE, env("scroll_confirm"), "/tmp") as s:
    s.wait_visible("fake tui ready", 3)
    s.send(b"\x03", "ctrl-c-1")
    s.pump(0.5)
    v2_vis = s.visible("Press Ctrl-C again to exit")
    v2_seen = "Press Ctrl-C again to exit" in s.screen.seen_text()
case("visible_now_excludes_scrollback", (not v2_vis) and v2_seen, {"visible": v2_vis, "audit_seen": v2_seen})
o = V1.PtySession(FAKE, env("scroll_confirm"), "/tmp")
o.pump(1)
o.send(b"\x03", "ctrl-c-1")
o.pump(0.5)
v1_vis = o.visible("Press Ctrl-C again to exit")
os.kill(o.pid, 9)
os.waitpid(o.pid, 0)
case("NEGCTL_v1_visible_matches_scrollback", v1_vis, {"v1_visible": v1_vis})

# 4. fragmented confirmation (split CSI + UTF-8 across writes) is still detected
with PtySession(FAKE, env("split_confirm"), "/tmp") as s:
    s.wait_visible("fake tui ready", 3)
    res = s.exit_with_ctrl_c()
case("fragmented_confirmation_detected_and_clean_exit", res["clean"], res)

# 5. no confirmation: bounded, forced, never clean
pty_driver.EXIT_WAIT = 2.0
t = time.monotonic()
with PtySession(FAKE, env("ignore"), "/tmp") as s:
    s.wait_visible("fake tui ready", 3)
    res = s.exit_with_ctrl_c()
    pid = s.pid
el = time.monotonic() - t
case("no_confirm_forced_bounded", res["forced"] and not res["clean"] and el < 2 * pty_driver.CONFIRM_WAIT + 6
     and not alive(pid), {"res": res, "elapsed_s": round(el, 2)})

# 6. robust close on exception: child killed, reaped, fd closed
pid = fd = None
try:
    with PtySession(FAKE, env("hang"), "/tmp") as s:
        pid, fd = s.pid, s.fd
        s.wait_visible("fake tui ready", 3)
        raise RuntimeError("injected")
except RuntimeError:
    pass
fd_closed = False
try:
    os.fstat(fd)
except OSError:
    fd_closed = True
case("close_on_exception_kills_reaps_closes", (not alive(pid)) and fd_closed and s.closed,
     {"alive": alive(pid), "fd_closed": fd_closed, "audit_tail": s.audit[-3:]})

# 7. close() idempotent
s.close()
case("close_idempotent", s.closed, {})

# 8. pilot timing still does not exit (fake fidelity control)
s = PtySession(FAKE, env(), "/tmp")
s.wait_visible("fake tui ready", 3)
s.send(b"\x03", "1")
s.pump(1.5)
s.send(b"\x03", "2 after 1.5 s")
s.pump(2)
still = not s.reap()
s.close()
case("NEGCTL_pilot_timing_does_not_exit", still, {"still_running": still})

json.dump(results, open(sys.argv[2], "w"), indent=1, default=str)
print("ALL_PASS" if all(r["pass"] for r in results.values()) else "SOME_FAIL")
sys.exit(0 if all(r["pass"] for r in results.values()) else 1)
