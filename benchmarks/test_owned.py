"""Negative tests for the ownership rule: failure, timeout and cleanup never reach a process that
this test did not launch. The "decoy" is another process running the very same executable path, as
the owner's application does next to a copy under test.

    python3 -I -m unittest benchmarks.test_owned     (from the repository root)
"""
import os
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from owned import NotOurs, Owned


class Base(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.exe = os.path.join(self.dir, "unit")
        shutil.copy("/bin/sleep", self.exe)  # the same path for the decoy and the launch
        self.decoy = subprocess.Popen([self.exe, "300"])
        time.sleep(0.1)
        self.assertIsNone(self.decoy.poll())

    def tearDown(self):
        alive = self.decoy.poll() is None
        self.decoy.kill()  # our own handle
        self.decoy.wait()
        shutil.rmtree(self.dir, ignore_errors=True)
        self.assertTrue(alive, "the decoy was ended")


class OwnedTests(Base):
    def test_normal_end_leaves_the_decoy(self):
        with Owned([self.exe, "300"], self.exe) as o:
            pid = o.pid
            self.assertTrue(o.alive())
        self.assertFalse(os.path.exists(f"/proc/{pid}"))

    def test_an_exception_inside_still_ends_only_the_launch(self):
        with self.assertRaises(ValueError):
            with Owned([self.exe, "300"], self.exe) as o:
                pid = o.pid
                raise ValueError("measurement failed")
        self.assertFalse(os.path.exists(f"/proc/{pid}"))

    def test_a_process_that_ignores_termination_is_ended_through_its_descriptor(self):
        sh = os.path.join(self.dir, "sh")
        shutil.copy("/bin/sh", sh)
        # Block in the shell itself: killing a shell waiting on `sleep` orphaned the
        # decoy's child after this test passed. The decoy must keep the same shell path.
        decoy_sh = subprocess.Popen([sh, "-c", "read -r unused"], stdin=subprocess.PIPE)
        try:
            started = time.time()
            with Owned([sh, "-c", 'trap "" TERM; while :; do sleep 1; done'], sh) as o:
                time.sleep(0.3)
                o.stop(grace=1.0)
            self.assertLess(time.time() - started, 6)
            self.assertFalse(o.alive())
            self.assertIsNone(decoy_sh.poll())
        finally:
            decoy_sh.kill()
            decoy_sh.wait()
            decoy_sh.stdin.close()

    def test_a_launch_that_is_not_the_expected_binary_ends_itself_and_nothing_else(self):
        other = shutil.which("true") and os.path.join(self.dir, "other")
        shutil.copy("/bin/sleep", other)
        with self.assertRaises(NotOurs):
            Owned([other, "300"], self.exe)
        # the launched copy is gone, the decoy at the expected path is alive (tearDown checks)
        out = subprocess.run(["ls", "/proc"], capture_output=True, text=True).stdout.split()
        self.assertFalse(any(self._exe(p) == other for p in out if p.isdigit()))

    @staticmethod
    def _exe(pid):
        try:
            return os.path.realpath(f"/proc/{pid}/exe")
        except OSError:
            return None

    def test_a_failure_while_launching_ends_the_launch_and_a_fd_failure_does_not_leak_it(self):
        import owned as module

        real = os.pidfd_open
        seen = []

        def failing(pid):
            seen.append(pid)
            raise OSError("no pidfd")

        os.pidfd_open = failing
        try:
            with self.assertRaises(OSError):
                module.Owned([self.exe, "300"], self.exe)
        finally:
            os.pidfd_open = real
        self.assertFalse(os.path.exists(f"/proc/{seen[0]}"), "the launched child was left running")
        self.assertIsNone(self.decoy.poll())

    def test_an_error_while_signalling_still_closes_everything_and_ends_the_launch(self):
        o = Owned([self.exe, "300"], self.exe)
        pid = o.pid
        o._send = lambda fd, sig: False  # the descriptor path fails: our own Popen handle is used
        o.stop(grace=2.0)
        self.assertFalse(os.path.exists(f"/proc/{pid}"))
        self.assertIsNone(o.fd)

    def test_a_sigchld_handler_that_could_reap_the_child_is_refused(self):
        old = signal.signal(signal.SIGCHLD, lambda *_: None)
        try:
            with self.assertRaises(RuntimeError):
                Owned([self.exe, "300"], self.exe)
        finally:
            signal.signal(signal.SIGCHLD, old)

    def test_ignoring_sigchld_is_refused_because_linux_then_reaps_children_itself(self):
        old = signal.signal(signal.SIGCHLD, signal.SIG_IGN)
        try:
            with self.assertRaises(RuntimeError):
                Owned([self.exe, "300"], self.exe)
        finally:
            signal.signal(signal.SIGCHLD, old)

    def test_the_default_sigchld_disposition_still_works(self):
        self.assertEqual(signal.getsignal(signal.SIGCHLD), signal.SIG_DFL)
        with Owned([self.exe, "300"], self.exe) as o:
            self.assertTrue(os.path.exists(f"/proc/{o.pid}"))

    def test_a_process_that_already_exited_is_not_signalled_again(self):
        shell = os.path.join(self.dir, "owned-wait")
        shutil.copy("/bin/sh", shell)
        release = os.path.join(self.dir, "owned-release")
        os.mkfifo(release)
        env = os.environ.copy()
        env["OWNED_TEST_RELEASE_FIFO"] = release
        with Owned(
            [shell, "-c", 'IFS= read -r < "$OWNED_TEST_RELEASE_FIFO"'],
            shell,
            env=env,
        ) as o:
            # Construction returns only after _verify_launch accepts the executable. The FIFO
            # then releases this exact child, so process exit cannot race constructor validation.
            with open(release, "w") as writer:
                writer.write("exit\n")
            o.proc.wait()
            signals = []
            o._send = lambda fd, sig: signals.append(sig)
            o.stop()  # no error, no signal to anything
            o.stop()  # idempotent
            self.assertEqual(signals, [], "a reaped child must not be signalled again")
        self.assertIsNone(self.decoy.poll())

    def test_a_signal_through_a_dead_descriptor_reaches_nobody(self):
        o = Owned([self.exe, "300"], self.exe)
        fd = os.dup(o.fd)
        o.stop()
        self.assertFalse(o._send(fd, signal.SIGKILL))  # gone: refused, not redirected
        os.close(fd)

    def test_interruption_runs_the_cleanup(self):
        with self.assertRaises(KeyboardInterrupt):
            with Owned([self.exe, "300"], self.exe) as o:
                pid = o.pid
                os.kill(os.getpid(), signal.SIGTERM)  # to ourselves, to prove the handler
                time.sleep(2)
        self.assertFalse(os.path.exists(f"/proc/{pid}"))

    def test_only_a_child_of_the_owned_process_can_be_adopted(self):
        sh = os.path.join(self.dir, "sh2")
        shutil.copy("/bin/sh", sh)
        with Owned([sh, "-c", f"{self.exe} 300 & wait"], sh) as o:
            time.sleep(0.4)
            self.assertIsNone(o.adopt_child("not-a-name"))
            kid = o.adopt_child("unit")
            self.assertIsNotNone(kid)
            self.assertNotEqual(kid, self.decoy.pid)  # same name and path, not a child
            self.assertEqual(len(o.adopted), 1)
        for _ in range(50):  # the kernel needs a moment to finish a process that was killed
            if self._gone(kid):
                break
            time.sleep(0.1)
        self.assertTrue(self._gone(kid), "the adopted child outlived its owner")

    def test_a_whole_tree_is_adopted_from_the_owned_process_and_ended_with_it_and_a_decoy_is_not(self):
        sh = os.path.join(self.dir, "sh3")
        shutil.copy("/bin/sh", sh)
        # a child that starts a child of its own, which ignores termination
        script = f'{sh} -c \'trap "" TERM; {self.exe} 300 & wait\' & wait'
        with Owned([sh, "-c", script], sh) as o:
            time.sleep(0.6)
            seen = o.adopt_tree()
            self.assertGreaterEqual(seen, 2)
            adopted = list(o.adopted)
            self.assertNotIn(self.decoy.pid, adopted)
            # adopting again changes nothing
            self.assertEqual(o.adopt_tree(), 0)
        for _ in range(50):
            if all(self._gone(p) for p in adopted):
                break
            time.sleep(0.1)
        self.assertTrue(all(self._gone(p) for p in adopted), "a descendant outlived its owner")
        self.assertIsNone(self.decoy.poll())

    @staticmethod
    def _gone(pid):
        try:
            with open(f"/proc/{pid}/stat") as f:
                return f.read().rsplit(")", 1)[1].split()[0] == "Z"
        except OSError:
            return True


class NoKillByNameTests(unittest.TestCase):
    """The lint half of the rule: a script that ends or finds processes by name cannot be added."""

    # Shell and Node: any `kill` command, selection of processes by name or listing.
    SHELL = re.compile(
        r"(^|[\s;&|(`$/])(kill|pkill|killall|pidof|fuser|skill)(\s|$)|\bprocess\.kill\b|\bpgrep\b|\bps\b[^|]*\|\s*(grep|awk)|\bxargs\s+kill|\bos\.(kill|killpg)\b"
    )
    # Python: signals to a bare PID or process group, a shell, or a listing used to choose.
    PYTHON = re.compile(
        r"\bos\.(kill|killpg|system|popen)\(|\bshell\s*=\s*True|\bpidfd_send_signal\b|"
        r"\bsubprocess\.\w+\(\s*\[?\s*[\"'](kill|pkill|killall|pgrep|pidof|ps|fuser)[\"']|\bpsutil\b|\bpgrep\b|\bpkill\b|\bkillall\b"
    )
    # Only owned.py may signal, and only through descriptors it holds.
    MAY_SIGNAL = {"owned.py"}

    @classmethod
    def findings(cls, name, line):
        if name.endswith(".py"):
            return name not in cls.MAY_SIGNAL and bool(cls.PYTHON.search(line)) or (
                name in cls.MAY_SIGNAL and bool(re.search(r"\bos\.(kill|killpg|system)\(|\bshell\s*=\s*True|\bpsutil\b", line))
            )
        return bool(cls.SHELL.search(line))

    def test_the_rule_catches_the_script_that_caused_the_incident(self):
        old = [
            "WEB=$(pgrep -P $APP -x WebKitWebProces | head -1)",
            "kill -TERM $APP",
            "APP=$(pgrep -x -n wyss-console)",
            "pkill -x xclip",
            "os.kill(pid, 9)",
        ]
        old += [
            'kill -- "$APP"',
            'kill -s TERM "$APP"',
            "kill 1234",
            "  xargs kill",
            "ps aux | grep wyss | awk '{print $2}'",
            "cd x; kill $PID",
            "command kill 1234",
            "/bin/kill -9 5",
            "process.kill(pid, 9)",
        ]
        for line in old:
            self.assertTrue(self.findings("x.sh", line), line)
        for line in ["os.kill(1, 9)", "os.killpg(g, 9)", "os.system('kill 1')", "subprocess.run(['pkill', 'x'])", "subprocess.run('x', shell=True)", "signal.pidfd_send_signal(fd, 9)"]:
            self.assertTrue(self.findings("x.py", line), line)
        for line in ["self.child.kill()", "child.kill(signal)", "# kill in a comment is stripped by the caller", "timeout 15 \"$binary\" --wyss-host-stop"]:
            self.assertFalse(self.findings("x.mjs", line.split("#")[0]), line)
        self.assertFalse(self.findings("x.py", "self.p.kill()"))

    def test_no_script_finds_or_ends_a_process_by_name(self):
        root = os.path.dirname(HERE)
        bad = []
        for sub in ("benchmarks", "scripts"):
            for dirpath, _, names in os.walk(os.path.join(root, sub)):
                for n in names:
                    if not n.endswith((".sh", ".py", ".mjs", ".js")) or n == "test_owned.py":
                        continue
                    path = os.path.join(dirpath, n)
                    with open(path, errors="replace") as f:
                        for i, line in enumerate(f, 1):
                            code = line.split("#", 1)[0] if n.endswith((".sh", ".py")) else line
                            if self.findings(n, code):
                                bad.append(f"{os.path.relpath(path, root)}:{i}: {line.strip()}")
        self.assertEqual(bad, [])


if __name__ == "__main__":
    unittest.main()
