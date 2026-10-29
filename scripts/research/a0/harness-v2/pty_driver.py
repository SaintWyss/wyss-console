"""PTY session driver for A0 interactive cases (v2).

v1 defects fixed (independent review 2026-10-29):
- `visible()` searched scrollback/history -> it now checks only the current screen grid;
- `pump()` returned as soon as the child was reaped, possibly before the last bytes were read
  -> after exit the driver drains the master fd until EOF/EIO (bounded by DRAIN_S);
- no guaranteed teardown -> `close()` (idempotent, also used by the context manager) kills
  the child's whole process group (the child is a session leader after pty.fork), reaps it,
  drains and closes the fd; every step is audited.
Exit sequence unchanged in intent: Ctrl-C, wait (<= CONFIRM_WAIT) for the confirmation text
to be visible now, immediate second Ctrl-C, wait (<= EXIT_WAIT); <= MAX_ROUNDS; else forced.
"""
import os
import pty
import select
import signal
import time

from screen import Screen

CONFIRM_TEXT = "Press Ctrl-C again to exit"
CONFIRM_WAIT = 3.0
EXIT_WAIT = 10.0
MAX_ROUNDS = 2
DRAIN_S = 2.0


class PtySession:
    def __init__(self, argv, env, cwd, cols=120, rows=40):
        import fcntl
        import struct
        import termios
        self.t0 = time.monotonic()
        self.audit = []
        self.raw = bytearray()
        self.screen = Screen(cols, rows)
        self.status = None
        self.closed = False
        self.eof = False
        pid, fd = pty.fork()
        if pid == 0:
            try:
                os.chdir(cwd)
                os.execve(argv[0], argv, env)
            finally:
                os._exit(127)
        self.pid, self.fd = pid, fd
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.log("spawned", argv=argv[0])

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close(reason="context_exit" if exc[0] is None else f"exception:{exc[0].__name__}")
        return False

    def log(self, event, **kw):
        self.audit.append({"t": round(time.monotonic() - self.t0, 3), "event": event, **kw})

    def _read_once(self, timeout):
        if self.eof:
            return False
        r, _, _ = select.select([self.fd], [], [], timeout)
        if not r:
            return True
        try:
            chunk = os.read(self.fd, 65536)
        except OSError:
            self.eof = True
            return False
        if not chunk:
            self.eof = True
            return False
        self.raw.extend(chunk)
        self.screen.feed(chunk)
        return True

    def drain(self, limit=DRAIN_S):
        """Read everything still buffered on the master side (until EOF/EIO or `limit`)."""
        end = time.monotonic() + limit
        before = len(self.raw)
        while time.monotonic() < end and self._read_once(0.05):
            pass
        self.log("drained", bytes=len(self.raw) - before, eof=self.eof)

    def pump(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if not self._read_once(0.05):
                break
            if self.reap():
                break
        if self.reap():
            self.drain()
            return False
        return not self.eof

    def reap(self):
        if self.status is not None:
            return True
        done, st = os.waitpid(self.pid, os.WNOHANG)
        if done:
            self.status = st
            self.log("child_exited", status=st, code=os.waitstatus_to_exitcode(st))
            return True
        return False

    def visible(self, text):
        return text in self.screen.visible_text()

    def wait_visible(self, text, timeout):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if self.visible(text):
                return True
            if not self.pump(0.05):
                return self.visible(text)
        return False

    def send(self, data, label):
        os.write(self.fd, data)
        self.log("sent", what=label)

    def _kill_group(self, sig):
        try:
            os.killpg(self.pid, sig)
            self.log("killpg", sig=int(sig))
        except ProcessLookupError:
            pass

    def exit_with_ctrl_c(self):
        for rnd in range(1, MAX_ROUNDS + 1):
            if self.reap():
                break
            self.send(b"\x03", f"ctrl-c-1 round {rnd}")
            t = time.monotonic()
            seen = self.wait_visible(CONFIRM_TEXT, CONFIRM_WAIT)
            self.log("confirm_visible" if seen else "confirm_not_seen", round=rnd,
                     after_s=round(time.monotonic() - t, 3))
            if self.reap():
                break
            if not seen:
                continue
            t2 = time.monotonic()
            self.send(b"\x03", f"ctrl-c-2 round {rnd}")
            self.log("second_ctrl_c_latency", seconds=round(t2 - t, 3))
            end = time.monotonic() + EXIT_WAIT
            while time.monotonic() < end and not self.reap():
                self.pump(0.1)
            if self.reap():
                break
        forced = not self.reap()
        if forced:
            self.close(reason="exit_sequence_failed")
        self.drain()
        code = os.waitstatus_to_exitcode(self.status) if self.status is not None else None
        clean = (not forced) and code == 0
        self.log("exit_result", clean=clean, forced=forced, code=code)
        return {"clean": clean, "forced": forced, "code": code}

    def close(self, reason="close"):
        """Idempotent teardown: SIGTERM then SIGKILL to the whole process group, reap, drain, close."""
        if self.closed:
            return
        self.log("close", reason=reason)
        if not self.reap():
            self._kill_group(signal.SIGTERM)
            end = time.monotonic() + 3
            while time.monotonic() < end and not self.reap():
                self._read_once(0.05)
            if not self.reap():
                self._kill_group(signal.SIGKILL)
                _, st = os.waitpid(self.pid, 0)
                self.status = st
                self.log("child_killed", status=st)
        # No signal after the leader is reaped: its PGID could then belong to someone else.
        # Inside the sandbox, stray members die with the PID namespace (checked by the host).
        self.drain()
        try:
            os.close(self.fd)
        except OSError:
            pass
        self.closed = True
