"""Independent tests of the exit driver against fake_tui.py (no provider).
Positive: deterministic clean exit. Negative control 1: the pilot's timing (second Ctrl-C after
1.5 s) does NOT exit, proving the fake reproduces the defect. Negative control 2: a TUI that
never confirms is bounded and reported as forced, never as clean.
"""
import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import pty_driver  # noqa: E402
from pty_driver import PtySession  # noqa: E402

FAKE = [sys.executable, "-I", os.path.join(HERE, "fake_tui.py")]
results, audits = {}, {}


def env(mode="normal"):
    return {"PATH": "/usr/bin", "FAKE_MODE": mode, "FAKE_WINDOW": "0.8", "TERM": "xterm-256color"}


def case(name, ok, detail):
    results[name] = ok
    print(("PASS " if ok else "FAIL ") + name, json.dumps(detail))


# Positive, 5 repetitions: must be clean every time
for i in range(5):
    s = PtySession(FAKE, env(), "/tmp")
    ready = s.wait_visible("fake tui ready", 3)
    t = time.monotonic()
    res = s.exit_with_ctrl_c()
    lat = [a["seconds"] for a in s.audit if a["event"] == "second_ctrl_c_latency"]
    audits[f"positive_{i}"] = s.audit
    case(f"positive_clean_exit_{i}", ready and res["clean"] and lat and lat[0] < 0.5
         and time.monotonic() - t < 5, {"res": res, "latency": lat})
    if i == 0:
        case("screen_cursor_spacing_live", "ok from mock" in s.screen.seen_text(), {})

# Negative control 1: pilot timing
s = PtySession(FAKE, env(), "/tmp")
s.wait_visible("fake tui ready", 3)
s.send(b"\x03", "ctrl-c-1 (pilot timing)")
s.pump(1.5)
s.send(b"\x03", "ctrl-c-2 after 1.5 s (pilot timing)")
s.pump(2)
still_running = not s.reap()
case("negative_pilot_timing_does_not_exit", still_running, {"still_running": still_running})
os.kill(s.pid, 9)
os.waitpid(s.pid, 0)
audits["negative_pilot_timing"] = s.audit

# Negative control 2: never confirms -> bounded, forced, not clean
pty_driver.EXIT_WAIT = 2.0
s = PtySession(FAKE, env("ignore"), "/tmp")
s.wait_visible("fake tui ready", 3)
t = time.monotonic()
res = s.exit_with_ctrl_c()
elapsed = time.monotonic() - t
case("negative_no_confirm_is_forced_and_bounded",
     res["forced"] and not res["clean"] and elapsed < 2 * pty_driver.CONFIRM_WAIT + 5,
     {"res": res, "elapsed_s": round(elapsed, 2)})
audits["negative_no_confirm"] = s.audit

json.dump({"results": results, "audits": audits}, open(sys.argv[1], "w"), indent=1)
print("ALL_PASS" if all(results.values()) else "SOME_FAIL")
sys.exit(0 if all(results.values()) else 1)
