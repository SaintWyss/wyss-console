"""Host-side sandbox launcher with enforcement (v2).

Replaces v1's bare `subprocess.run(..., timeout=...)`:
- bwrap starts in its own session (killable as a group while alive), EMPTY environment, rlimits
  applied in preexec, only the json-status fd passed;
- the per-case HMAC key goes in on stdin, which is then closed;
- stdout/stderr are read by threads with a byte cap (STREAM_CAP); overflow kills the sandbox;
- a watchdog enforces the wall-clock timeout and the /markers storage cap;
- PID-namespace members are sampled during the run; members still present after bwrap exits
  are leftovers and the namespace is killed through its init;
- a `finally` guarantees kill + reap + fd close on every path (including exceptions).
"""
import json
import os
import signal
import subprocess
import threading
import time

import limits


class _Reader(threading.Thread):
    def __init__(self, f, cap, on_overflow):
        super().__init__(daemon=True)
        self.f, self.cap, self.on_overflow = f, cap, on_overflow
        self.buf = bytearray()
        self.overflow = False

    def run(self):
        while True:
            chunk = self.f.read1(65536) if hasattr(self.f, "read1") else self.f.read(65536)
            if not chunk:
                return
            if len(self.buf) + len(chunk) > self.cap:
                self.overflow = True
                self.on_overflow()
                return
            self.buf.extend(chunk)


def run_sandbox(build_argv, *, key=b"", timeout=180, markers=None, markers_cap=limits.MARKERS_CAP,
                stream_cap=limits.STREAM_CAP, rlimits=None, inject_failure=None):
    """build_argv(status_fd) -> argv. Returns a result dict; never leaves a live sandbox behind."""
    r_status, w_status = os.pipe()
    res = {"timed_out": False, "killed": None, "status": [], "members_max": 0,
           "members_seen": [], "leftovers": [], "exit": None, "markers_peak": 0}
    p = None
    out_r = err_r = None
    t0 = time.monotonic()
    try:
        argv = build_argv(w_status)
        res["argv"] = argv
        p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             env={}, pass_fds=(w_status,), start_new_session=True,
                             preexec_fn=limits.preexec(**(rlimits or {})))
        os.close(w_status)
        w_status = None
        kill = lambda why: _kill(p, res, why)  # noqa: E731
        out_r = _Reader(p.stdout, stream_cap, lambda: kill("stream_cap"))
        err_r = _Reader(p.stderr, stream_cap, lambda: kill("stream_cap"))
        out_r.start()
        err_r.start()
        status_line = _read_status_first(r_status, 10)
        if status_line:
            res["status"].append(status_line)
        ns = (status_line or {}).get("pid-namespace")
        init_pid = (status_line or {}).get("child-pid")
        res["pid_ns"], res["init_pid"] = ns, init_pid
        try:
            p.stdin.write(key + b"\n")
            p.stdin.close()
        except (BrokenPipeError, OSError):
            pass
        if inject_failure == "after_launch":
            raise RuntimeError("injected failure after launch (test)")
        while p.poll() is None:
            if ns:
                m = limits.ns_members(ns)
                res["members_max"] = max(res["members_max"], len(m))
                if len(res["members_seen"]) < 50:
                    res["members_seen"].append(len(m))
            if markers:
                used = limits.du(markers)
                res["markers_peak"] = max(res["markers_peak"], used)
                if used > markers_cap:
                    _kill(p, res, "markers_cap")
            if time.monotonic() - t0 > timeout:
                res["timed_out"] = True
                _kill(p, res, "timeout")
            time.sleep(0.25)
        res["exit"] = p.returncode
    finally:
        if p is not None and p.poll() is None:
            _kill(p, res, res.get("killed") or "finally")
        if p is not None:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                pass
        for rd in (out_r, err_r):
            if rd is not None:
                rd.join(timeout=5)
        rest = _drain_fd(r_status)
        res["status"] += [json.loads(l) for l in rest.splitlines() if l.strip().startswith("{")]
        os.close(r_status)
        if w_status is not None:
            os.close(w_status)
        ns = res.get("pid_ns")
        if ns:
            left = limits.ns_members(ns)
            if left:
                res["leftovers"] = left
                res["leftovers_after_kill"] = limits.kill_namespace(ns, res.get("init_pid"))
        res["stdout"] = bytes(out_r.buf).decode("utf-8", "replace") if out_r else ""
        res["stderr"] = bytes(err_r.buf).decode("utf-8", "replace") if err_r else ""
        res["stdout_overflow"] = bool(out_r and out_r.overflow)
        res["duration_s"] = round(time.monotonic() - t0, 2)
    return res


def _kill(p, res, why):
    if res.get("killed") is None:
        res["killed"] = why
    if p.poll() is None:  # bwrap still alive: its PGID is still ours to signal
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def _read_status_first(fd, timeout):
    import select
    buf = b""
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r:
            c = os.read(fd, 4096)
            if not c:
                break
            buf += c
            if b"\n" in buf or b"}" in buf:
                try:
                    return json.loads(buf.split(b"\n")[0].decode())
                except ValueError:
                    continue
    return None


def _drain_fd(fd):
    import select
    out = b""
    while True:
        r, _, _ = select.select([fd], [], [], 0.2)
        if not r:
            break
        c = os.read(fd, 4096)
        if not c:
            break
        out += c
    return out.decode("utf-8", "replace")
