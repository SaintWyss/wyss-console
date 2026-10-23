"""Ownership-checked processes for the benchmark scripts.

A benchmark may end a process only if it launched it and still holds the handle it got at the
launch. No process is ever found by its name, its executable or its place in a listing: the owner's
own application has the same names and paths as the copies under test, and that was how their
running window and terminal host were ended on 2026-10-20.

How the guarantees are kept:

* The process is a direct child of this one (`subprocess.Popen`). Until this process reaps it, its
  PID cannot be given to another process, so the PID is not reused between launch and end.
* A pidfd is opened at once and every signal goes through it (`pidfd_send_signal`): the signal can
  only reach the process the descriptor was opened on, and a process that is gone yields an error,
  never another process.
* The launch is verified (parent is this process, executable is the expected one) before the handle
  is trusted; a mismatch ends only the process this class launched and raises.
* Descendants are *adopted* by pidfd after the same kind of check, and only adopted ones are ever
  signalled. Anything else that remains is reported, not touched.
* This class must be the only code that reaps its child (no SIGCHLD handler, no `waitpid(-1)`):
  that is what keeps the PID from being reused, and the constructor refuses any SIGCHLD disposition but the default (a handler or SIG_IGN).
* `with Owned(...)` ends the process on every exit: normal, exception, timeout, SIGINT, SIGTERM.

Nothing here reads a process list to decide whom to end.
"""
import os
import signal
import subprocess
import time


class NotOurs(RuntimeError):
    """The process is not what this class launched; nothing outside the launch was signalled."""


def _stat(pid):
    """(ppid, starttime) from /proc/<pid>/stat, or None when the process is not there."""
    try:
        with open(f"/proc/{pid}/stat") as f:
            text = f.read()
    except OSError:
        return None
    fields = text.rsplit(")", 1)[1].split()
    return int(fields[1]), int(fields[19])


def _children(pid):
    """The direct children of `pid`, from the kernel's own list for it."""
    out = []
    try:
        for task in os.listdir(f"/proc/{pid}/task"):
            with open(f"/proc/{pid}/task/{task}/children") as f:
                out.extend(int(x) for x in f.read().split())
    except OSError:
        pass
    return out


class Owned:
    def __init__(self, argv, expect_exe, env=None, log=None):
        self.expect_exe = os.path.realpath(expect_exe)
        self.proc = None
        self.fd = None
        self.adopted = {}
        self.leftovers = []
        self._old = {}
        if signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL:
            # The unreaped-child guarantee (and the fallback through Popen) needs this to be the
            # only code that reaps its children. A handler could reap; SIG_IGN makes Linux
            # auto-reap, which frees the PID for reuse while we still hold it.
            raise RuntimeError("SIGCHLD is not at its default: the child could be reaped behind this class's back")
        self._install_handlers()  # before the launch: no gap in which an interrupt leaves it loose
        try:
            self.proc = subprocess.Popen(
                argv, env=env, stdin=subprocess.DEVNULL,
                stdout=log if log is not None else subprocess.DEVNULL,
                stderr=subprocess.STDOUT, start_new_session=True,
            )
            self.pid = self.proc.pid
            self.fd = os.pidfd_open(self.pid)
            self._verify_launch()
        except BaseException:
            held = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
            try:
                self._abandon_launch()
            finally:
                self._restore_handlers()
                signal.pthread_sigmask(signal.SIG_SETMASK, held)
            raise

    def _abandon_launch(self):
        """The launch failed part-way: end what Popen created, through the Popen handle (an
        unreaped child, so its PID is still ours), and close whatever was opened."""
        try:
            if self.proc is not None and self.proc.poll() is None:
                self.proc.kill()
                self.proc.wait()
        finally:
            if self.fd is not None:
                os.close(self.fd)
                self.fd = None

    def _verify_launch(self):
        try:
            stat = _stat(self.pid)
            exe = os.path.realpath(f"/proc/{self.pid}/exe")
        except OSError:
            stat, exe = None, None
        if stat is None or stat[0] != os.getpid() or exe != self.expect_exe:
            raise NotOurs(f"launch is not the expected binary {self.expect_exe}")

    # -- signalling: only through descriptors this object holds --------------------------------
    def _send(self, fd, sig):
        try:
            signal.pidfd_send_signal(fd, sig)
            return True
        except OSError:
            return False

    def alive(self):
        return self.proc is not None and self.proc.poll() is None

    def adopt_child(self, comm):
        """Adopt the direct child of the owned process whose `comm` is `comm`; returns its pid.
        Candidates come from this process's own children list, then are checked after the
        descriptor is open, so a PID that changed hands in between is rejected."""
        try:
            with open(f"/proc/{self.pid}/task/{self.pid}/children") as f:
                kids = f.read().split()
        except OSError:
            return None
        for text in kids:
            pid = int(text)
            before = _stat(pid)
            try:
                with open(f"/proc/{pid}/comm") as f:
                    name = f.read().strip()
            except OSError:
                continue
            if name != comm or before is None or before[0] != self.pid:
                continue
            try:
                fd = os.pidfd_open(pid)
            except OSError:
                continue
            if _stat(pid) != before:
                os.close(fd)
                continue
            self.adopted[pid] = fd
            return pid
        return None

    def adopt_tree(self):
        """Adopt every descendant of the owned process that is there now, by descriptor.

        The candidates are read from the children lists of this process and then of each process
        adopted before, never from a listing of the system; each is checked after its descriptor
        is open, so a PID that changed hands in between is rejected. A helper that a bus or a
        window starts on demand is thereby ended with its parent if it outlives it. Returns how
        many were adopted."""
        adopted = 0
        todo = [self.pid]
        while todo:
            parent = todo.pop()
            for pid in _children(parent):
                before = _stat(pid)
                if before is None or before[0] != parent:
                    continue
                if pid in self.adopted:
                    todo.append(pid)
                    continue
                try:
                    fd = os.pidfd_open(pid)
                except OSError:
                    continue
                if _stat(pid) != before:
                    os.close(fd)
                    continue
                self.adopted[pid] = fd
                todo.append(pid)
                adopted += 1
        return adopted

    def stop(self, grace=10.0):
        """End what was launched and what was adopted. Idempotent. Interruptions are held back
        while it runs, and a failure in one step does not skip the next."""
        held = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
        try:
            try:
                if self.proc is not None and self.fd is not None:
                    if self.proc.poll() is None:
                        if not self._send(self.fd, signal.SIGTERM):
                            self.proc.send_signal(signal.SIGTERM)  # our unreaped child
                        try:
                            self.proc.wait(timeout=grace)
                        except subprocess.TimeoutExpired:
                            if not self._send(self.fd, signal.SIGKILL):
                                self.proc.kill()
                            self.proc.wait(timeout=grace)
            finally:
                if self.fd is not None:
                    os.close(self.fd)
                    self.fd = None
                # Adopted descendants still running after their parent went: ours, by descriptor.
                for pid, fd in list(self.adopted.items()):
                    try:
                        if self._send(fd, 0):
                            self._send(fd, signal.SIGKILL)
                            self.leftovers.append(pid)
                    finally:
                        os.close(fd)
                        del self.adopted[pid]
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, held)

    def _install_handlers(self):
        for sig in (signal.SIGINT, signal.SIGTERM):
            self._old[sig] = signal.signal(sig, self._interrupted)

    def _restore_handlers(self):
        for sig, handler in self._old.items():
            signal.signal(sig, handler)
        self._old = {}

    def __enter__(self):
        return self

    def _interrupted(self, signum, _frame):
        raise KeyboardInterrupt(f"signal {signum}")

    def __exit__(self, *_):
        try:
            self.stop()
        finally:
            self._restore_handlers()
        return False
