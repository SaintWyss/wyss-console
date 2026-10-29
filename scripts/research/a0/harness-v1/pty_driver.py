"""PTY session driver for A0 interactive cases: incremental screen model (screen.py) plus a
deterministic, bounded, audited Ctrl-C exit sequence.

Exit sequence (fixes the pilot defect: second Ctrl-C sent 1.5 s later, outside the provider's
confirmation window):
  1. send Ctrl-C; 2. wait up to CONFIRM_WAIT for the confirmation text to be *visible now*;
  3. send the second Ctrl-C immediately (latency recorded); 4. wait up to EXIT_WAIT for exit.
  At most MAX_ROUNDS rounds; then SIGTERM, then SIGKILL, and the exit is reported as forced.
Every step is appended to `audit` with a monotonic timestamp.
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
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(cwd)
            os.execve(argv[0], argv, env)
        self.pid, self.fd = pid, fd
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.log("spawned", argv=argv[0])

    def log(self, event, **kw):
        self.audit.append({"t": round(time.monotonic() - self.t0, 3), "event": event, **kw})

    def pump(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if self.reap():
                return False
            r, _, _ = select.select([self.fd], [], [], 0.05)
            if r:
                try:
                    chunk = os.read(self.fd, 65536)
                except OSError:
                    return False
                if not chunk:
                    return False
                self.raw.extend(chunk)
                self.screen.feed(chunk)
        return True

    def reap(self):
        if self.status is not None:
            return True
        done, st = os.waitpid(self.pid, os.WNOHANG)
        if done:
            self.status = st
            self.log("child_exited", status=st,
                     code=os.waitstatus_to_exitcode(st))
            return True
        return False

    def visible(self, text):
        return text in self.screen.text()

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
        forced = False
        if not self.reap():
            forced = True
            os.kill(self.pid, signal.SIGTERM)
            self.log("sent_sigterm")
            end = time.monotonic() + 3
            while time.monotonic() < end and not self.reap():
                time.sleep(0.05)
            if not self.reap():
                os.kill(self.pid, signal.SIGKILL)
                self.log("sent_sigkill")
                _, st = os.waitpid(self.pid, 0)
                self.status = st
        code = os.waitstatus_to_exitcode(self.status) if self.status is not None else None
        clean = (not forced) and code == 0
        self.log("exit_result", clean=clean, forced=forced, code=code)
        return {"clean": clean, "forced": forced, "code": code}
