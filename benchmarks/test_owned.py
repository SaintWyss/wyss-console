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
        decoy_sh = subprocess.Popen([sh, "-c", "sleep 300"])
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

    def test_a_process_that_already_exited_is_not_signalled_again(self):
        with Owned([self.exe, "0"], self.exe) as o:
            o.proc.wait()
            o.stop()  # no error, no signal to anything
            o.stop()  # idempotent
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

    @staticmethod
    def _gone(pid):
        try:
            with open(f"/proc/{pid}/stat") as f:
                return f.read().rsplit(")", 1)[1].split()[0] == "Z"
        except OSError:
            return True


class NoKillByNameTests(unittest.TestCase):
    """The lint half of the rule: a script that ends or finds processes by name cannot be added."""

    FORBIDDEN = re.compile(r"\b(pkill|killall|pgrep|pidof|fuser)\b|\bkill\s+(-\w+\s+)?[\"$`(]|\bos\.kill\(|\bsubprocess\.[a-z_]+\(\s*\[\s*[\"'](kill|pkill)")

    def test_the_rule_catches_the_script_that_caused_the_incident(self):
        old = [
            "WEB=$(pgrep -P $APP -x WebKitWebProces | head -1)",
            "kill -TERM $APP",
            "APP=$(pgrep -x -n wyss-console)",
            "pkill -x xclip",
            "os.kill(pid, 9)",
        ]
        for line in old:
            self.assertTrue(self.FORBIDDEN.search(line), line)
        self.assertFalse(self.FORBIDDEN.search("self.child.kill()"))

    def test_no_script_finds_or_ends_a_process_by_name(self):
        root = os.path.dirname(HERE)
        bad = []
        for sub in ("benchmarks", "scripts"):
            for dirpath, _, names in os.walk(os.path.join(root, sub)):
                for n in names:
                    if not n.endswith((".sh", ".py", ".mjs")) or n == "test_owned.py":
                        continue
                    path = os.path.join(dirpath, n)
                    with open(path, errors="replace") as f:
                        for i, line in enumerate(f, 1):
                            code = line.split("#", 1)[0] if n.endswith((".sh", ".py")) else line
                            if self.FORBIDDEN.search(code):
                                bad.append(f"{os.path.relpath(path, root)}:{i}: {line.strip()}")
        self.assertEqual(bad, [])


if __name__ == "__main__":
    unittest.main()
