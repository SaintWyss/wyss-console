#!/usr/bin/env python3
"""M1-13 on the real desktop: the built application on an isolated display, with its own home, state,
runtime directory, terminal host and synthetic projects, measured from outside.

    python3 -I benchmarks/m1_gui.py --binary src-tauri/target/release/wyss-console \
        --shape b --run 1 --out .artifacts/m1-gui/run1 [--steps steps.json]

Everything it starts is a direct child launched through `owned.py` and ended through the handle it
got at the launch (the display, the host, the application, the frame grabber); nothing is found by
its name. The owner's application, host, authority and display are never named: every path, socket
and display here is made for the run, and the process refuses to start on a display that exists.

What is measured (contract §5, M1-13):

* input to render: a key is injected into the focused pane of the nested display and the time to
  the first frame that shows the program's answer is taken (frames: `ffmpeg` x11grab, 60 per
  second, so a figure is quantised to about 17 ms and carries the capture's own latency);
* switching: a click on a project (or Scratch) to the first frame equal to that project's settled
  picture;
* restore: the click that opens the Projects view of an application started over an existing
  store and host, to the first frame equal to the settled picture;
* idle: for a stretch of seconds, with the view shown and with it hidden, the owned tree's
  resident and proportional memory, processor time, threads, descriptors and voluntary context
  switches (the wake-ups). The tree is the application, its WebKit processes and the host.

The raw samples are written, none dropped; outliers stay.
"""
import argparse
import json
import os
import random
import shlex
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from owned import NotOurs, Owned  # noqa: E402
import m3_steps  # noqa: E402
import m4_steps  # noqa: E402

SCREEN = (1500, 1400)
DISPLAY = ":79"
WAYLAND = "wl-m1"
BASE_PATH = "/usr/local/bin:/usr/bin:/bin"


def monotonic_ns():
    return time.monotonic_ns()


# ---- the display ---------------------------------------------------------------------------------


class Nest:
    """kwin_wayland (virtual) and Xwayland, each on a session bus of its own made for the run, on a
    display that did not exist. Whatever a bus starts on demand is adopted by descriptor and ends
    with it."""

    def __init__(self, root, log_dir):
        if os.path.exists(f"/tmp/.X11-unix/X{DISPLAY[1:]}"):
            raise SystemExit(f"display {DISPLAY} exists: not touching it")
        self.run = os.path.join(root, "nest")
        os.makedirs(self.run, mode=0o700)
        os.makedirs(os.path.join(root, "nest-home"), exist_ok=True)
        self.parts = []
        self.log_dir = log_dir
        self.root = root
        w, h = SCREEN
        try:
            bus = self.bus("nest-bus")
            base = {"PATH": BASE_PATH, "HOME": os.path.join(root, "nest-home"),
                    "XDG_RUNTIME_DIR": self.run, "LANG": "C.UTF-8", "DBUS_SESSION_BUS_ADDRESS": bus}
            # The compositor has file capabilities, which hide its executable from /proc, so its
            # identity cannot be verified at launch. A shell that is verified starts it, and the
            # compositor is then adopted as that shell's child.
            command = shlex.join(["/usr/bin/kwin_wayland", "--virtual", "--width", str(w),
                                  "--height", str(h), "--socket", WAYLAND, "--no-lockscreen",
                                  "--no-global-shortcuts"]) + " & wait"
            self._launch("compositor", ["/bin/sh", "-c", command], "/bin/sh", base)
            self._wait(lambda: os.path.exists(os.path.join(self.run, WAYLAND)), 20, "the Wayland socket")
            self._launch("xwayland", ["/usr/bin/Xwayland", DISPLAY, "-geometry", f"{w}x{h}", "-shm",
                                      "-nolisten", "tcp", "-ac"],
                         "/usr/bin/Xwayland", dict(base, WAYLAND_DISPLAY=WAYLAND))
            self._wait(lambda: self.display_ok(), 20, "the display")
            time.sleep(1.5)
        except BaseException:
            self.stop()
            raise

    def bus(self, name):
        """A session bus for this run: a direct child, its socket inside the run's directory."""
        path = os.path.join(self.root, f"{name}.sock")
        self._launch(name, ["/usr/bin/dbus-daemon", "--session", "--nofork", f"--address=unix:path={path}"],
                     "/usr/bin/dbus-daemon", {"PATH": BASE_PATH, "HOME": os.path.join(self.root, "nest-home"),
                                              "XDG_RUNTIME_DIR": self.run})
        self._wait(lambda: os.path.exists(path), 10, "the bus")
        return f"unix:path={path}"

    def _launch(self, name, argv, exe, env):
        log = open(os.path.join(self.log_dir, f"{name}.log"), "wb")
        part = Owned(argv, exe, env=env, log=log)
        self.parts.append((name, part, log))

    @staticmethod
    def _wait(check, seconds, what):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if check():
                return
            time.sleep(0.2)
        raise RuntimeError(f"{what} did not come up")

    def display_ok(self):
        r = subprocess.run(["/usr/bin/xdpyinfo", "-display", DISPLAY], capture_output=True)
        return r.returncode == 0

    def stop(self):
        for _, part, log in reversed(self.parts):
            part.adopt_tree()
            part.stop()
            log.close()


# ---- reading a process ---------------------------------------------------------------------------

CLK = os.sysconf("SC_CLK_TCK")


def read_status(pid):
    try:
        with open(f"/proc/{pid}/status") as f:
            return dict(l.split(":\t", 1) for l in f.read().splitlines() if ":\t" in l)
    except OSError:
        return None


def descendants(pid):
    """The process and everything under it, from the children lists of the kernel."""
    out, todo = [], [pid]
    while todo:
        p = todo.pop()
        out.append(p)
        try:
            for t in os.listdir(f"/proc/{p}/task"):
                with open(f"/proc/{p}/task/{t}/children") as f:
                    todo.extend(int(x) for x in f.read().split())
        except OSError:
            pass
    return sorted(set(out))


def comm_of(pid):
    try:
        with open(f"/proc/{pid}/comm") as f:
            return f.read().strip()
    except OSError:
        return "?"


def sample(pids):
    """One reading of every process in `pids`: memory, processor time, threads, descriptors and
    voluntary context switches."""
    rows = {}
    for pid in pids:
        st = read_status(pid)
        if st is None:
            continue
        try:
            with open(f"/proc/{pid}/smaps_rollup") as f:
                roll = dict(l.split(":", 1) for l in f.read().splitlines()[1:] if ":" in l)
            pss = int(roll["Pss"].split()[0])
        except (OSError, KeyError, ValueError):
            pss = None
        try:
            with open(f"/proc/{pid}/stat") as f:
                fields = f.read().rsplit(")", 1)[1].split()
            ticks = int(fields[11]) + int(fields[12])
        except (OSError, IndexError, ValueError):
            ticks = None
        voluntary = 0
        by_thread = {}
        try:
            for t in os.listdir(f"/proc/{pid}/task"):
                try:
                    with open(f"/proc/{pid}/task/{t}/status") as f:
                        for line in f:
                            if line.startswith("voluntary_ctxt_switches"):
                                by_thread[t] = int(line.split()[1])
                                voluntary += by_thread[t]
                except OSError:
                    pass
        except OSError:
            pass
        try:
            fds = len(os.listdir(f"/proc/{pid}/fd"))
        except OSError:
            fds = None
        rows[pid] = {
            "comm": comm_of(pid),
            "rss_kb": int(st["VmRSS"].split()[0]) if "VmRSS" in st else None,
            "pss_kb": pss,
            "cpu_ticks": ticks,
            "threads": int(st["Threads"]),
            "fds": fds,
            "voluntary": voluntary,
            "voluntary_by_thread": by_thread,
        }
    return rows


def idle_window(owned_pids, seconds, interval=1.0):
    """Samples the tree every `interval` seconds for `seconds`; returns the raw samples and the
    rates between the first and the last."""
    raw = []
    t_start = time.monotonic()
    while True:
        now = time.monotonic()
        tree = []
        for pid in owned_pids():
            tree.extend(descendants(pid))
        raw.append({"t": now - t_start, "procs": sample(sorted(set(tree)))})
        if now - t_start >= seconds:
            break
        time.sleep(max(0.0, interval - (time.monotonic() - now)))
    first, last = raw[0], raw[-1]
    span = last["t"] - first["t"]
    common = [p for p in last["procs"] if p in first["procs"]]
    cpu = sum(last["procs"][p]["cpu_ticks"] - first["procs"][p]["cpu_ticks"] for p in common) / CLK
    # Wake-ups are the voluntary context switches, counted between each pair of neighbouring
    # samples thread by thread: a thread that was born in between counts all it has, one that
    # ended in between counts nothing more, so a thread that comes and goes cannot make the total
    # go backwards.
    wake = 0
    for before, after in zip(raw, raw[1:]):
        for pid, now in after["procs"].items():
            was = before["procs"].get(pid, {"voluntary_by_thread": {}})["voluntary_by_thread"]
            for tid, count in now["voluntary_by_thread"].items():
                wake += max(0, count - was.get(tid, 0))
    total = lambda key: sum((v[key] or 0) for v in last["procs"].values())  # noqa: E731
    return raw, {
        "seconds": span,
        "processes": len(last["procs"]),
        "stable_set": set(first["procs"]) == set(last["procs"]),
        "rss_mib": total("rss_kb") / 1024,
        "pss_mib": total("pss_kb") / 1024,
        "threads": total("threads"),
        "fds": total("fds"),
        "cpu_percent_of_one_core": cpu / span * 100 if span else None,
        "wakeups_per_second": wake / span if span else None,
        "by_process": {f"{p}:{last['procs'][p]['comm']}": {
            "rss_mib": (last["procs"][p]["rss_kb"] or 0) / 1024,
            "pss_mib": (last["procs"][p]["pss_kb"] or 0) / 1024} for p in sorted(last["procs"])},
    }


# ---- frames --------------------------------------------------------------------------------------


class Frames:
    """The nested display's region, 60 frames a second, grey, with the time each one arrived."""

    def __init__(self, region, log_dir):
        self.x, self.y, self.w, self.h = region
        self.frames = []  # (monotonic_ns, ndarray)
        self.lock = threading.Lock()
        self.stop_flag = False
        self.log = open(os.path.join(log_dir, "ffmpeg.log"), "wb")
        env = {"PATH": BASE_PATH, "HOME": os.environ.get("HOME", "/")}
        self.grab = Owned(
            ["/usr/bin/ffmpeg", "-loglevel", "quiet", "-f", "x11grab", "-draw_mouse", "0",
             "-framerate", "60", "-video_size", f"{self.w}x{self.h}",
             "-i", f"{DISPLAY}+{self.x},{self.y}", "-pix_fmt", "gray", "-f", "rawvideo", "-"],
            "/usr/bin/ffmpeg", env=env, log=subprocess.PIPE)
        self.thread = threading.Thread(target=self._read, daemon=True)
        self.thread.start()

    def _read(self):
        size = self.w * self.h
        stream = self.grab.proc.stdout
        while not self.stop_flag:
            data = stream.read(size)
            if len(data) < size:
                return
            frame = np.frombuffer(data, dtype=np.uint8).reshape(self.h, self.w)
            with self.lock:
                self.frames.append((monotonic_ns(), frame))
                if len(self.frames) > 900:  # fifteen seconds
                    del self.frames[:300]

    def latest(self):
        with self.lock:
            return self.frames[-1][1].copy() if self.frames else None

    def since(self, t0):
        with self.lock:
            return [(t, f) for t, f in self.frames if t >= t0]

    def stop(self):
        self.stop_flag = True
        self.grab.stop()
        self.thread.join(timeout=3)
        self.log.close()


def differs(a, b, level=24):
    """Fraction of pixels that differ by more than `level` grey levels."""
    return float(np.mean(np.abs(a.astype(np.int16) - b.astype(np.int16)) > level))


def time_to(frames, t0, reference, tolerance=0.002, timeout_s=8.0):
    """Milliseconds from `t0` to the first later frame equal to `reference`, or None."""
    end = t0 + int(timeout_s * 1e9)
    seen = 0
    while monotonic_ns() < end:
        got = frames.since(t0)
        for t, f in got[seen:]:
            if differs(f, reference) <= tolerance:
                return (t - t0) / 1e6
        seen = len(got)
        time.sleep(0.005)
    return None


def time_to_change(frames, t0, base, threshold=0.01, timeout_s=5.0):
    """Milliseconds from `t0` to the first later frame that is not `base` any more: the first
    visible answer to what was done at `t0`."""
    end = t0 + int(timeout_s * 1e9)
    seen = 0
    while monotonic_ns() < end:
        got = frames.since(t0)
        for t, f in got[seen:]:
            if differs(f, base) > threshold:
                return (t - t0) / 1e6
        seen = len(got)
        time.sleep(0.005)
    return None


def settle(frames, seconds=1.2, tolerance=0.0005):
    """Waits until the picture has not changed for `seconds`, and returns it."""
    last = None
    since = monotonic_ns()
    deadline = since + int(20e9)
    while monotonic_ns() < deadline:
        now = frames.latest()
        if now is None:
            time.sleep(0.05)
            continue
        if last is None or differs(now, last) > tolerance:
            last = now
            since = monotonic_ns()
        elif monotonic_ns() - since >= seconds * 1e9:
            return now
        time.sleep(0.05)
    raise RuntimeError("the picture never settled")


# ---- the actions ---------------------------------------------------------------------------------


class Pointer:
    """One xdotool reading commands from a pipe, so an event is injected at once, with no process
    to start first."""

    def __init__(self, env):
        self.proc = subprocess.Popen(["/usr/bin/xdotool", "-"], stdin=subprocess.PIPE, env=env,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def do(self, line):
        self.proc.stdin.write((line + "\n").encode())
        self.proc.stdin.flush()

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=5)
        except Exception:  # noqa: BLE001
            pass


def shot(env, path):
    subprocess.run(["/usr/bin/import", "-window", "root", path], env=env, check=False,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)


def pct(values, q):
    values = sorted(values)
    if not values:
        return None
    return values[min(len(values) - 1, int(q * len(values)))]


def summary(values):
    if not values:
        return {"n": 0}
    return {"n": len(values), "min": min(values), "p50": pct(values, 0.5), "p95": pct(values, 0.95),
            "p99": pct(values, 0.99), "max": max(values), "mean": statistics.fmean(values)}


# ---- the run -------------------------------------------------------------------------------------


def build_world(root, shape, test_binary, log_dir):
    """Runs the ignored test that makes the synthetic projects, workspaces and shells through the
    commands the window sends, against the disposable host and the run's own state directory."""
    env = {"PATH": BASE_PATH, "WYSS_M1_WORLD": root, "WYSS_M1_SHAPE": shape,
           "HOME": os.path.join(root, "home"), "XDG_STATE_HOME": os.path.join(root, "state"),
           "XDG_RUNTIME_DIR": os.path.join(root, "run")}
    with open(os.path.join(log_dir, "world.log"), "wb") as log:
        done = subprocess.run(
            [test_binary, "--ignored", "--exact",
             "projects::tests::perf_world::build_the_world_of_the_measurements", "--nocapture"],
            env=env, stdout=log, stderr=subprocess.STDOUT, timeout=120)
    if done.returncode != 0:
        raise RuntimeError("the world could not be built; see world.log")
    with open(os.path.join(root, "world.json")) as f:
        return json.load(f)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--shape", choices=["a", "b", "c", "none"], required=True, help="none: no projects, and the release gate stays off")
    ap.add_argument("--run", type=int, required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--test-binary", default="", help="the built unit-test executable (not needed for shape none)")
    ap.add_argument("--steps", help="JSON list of calibration steps; no measurement is made")
    ap.add_argument("--seed", help="a directory whose layout.json, native/ and configuration/trusted-writers are copied into the run's own state (a rehearsal on a copy of real records)")
    ap.add_argument("--m3-fixture", action="store_true",
                    help="also host the Claude Managed fixture runtime in the application (needs --m3-sdk-root and --m3-script; the binary must be built with --features m3-fixture)")
    ap.add_argument("--m3-sdk-root", default="", help="the hash-locked virtual environment the Managed fixture runs (read-only)")
    ap.add_argument("--m3-script", default="", help="the mock provider's script (a JSON list of responses)")
    ap.add_argument("--m3-seed", action="append", default=[], metavar="NAME=TEXT",
                    help="a file to create in the Managed fixture's workspace (\\n is a newline); repeatable")
    ap.add_argument("--m2-fixtures", action="store_true",
                    help="put fixture-only Claude/Codex CLIs and a disposable instruction file in this run's HOME")
    ap.add_argument("--idle-seconds", type=int, default=60)
    ap.add_argument("--switches", type=int, default=40)
    ap.add_argument("--keys", type=int, default=40)
    ap.add_argument("--phases", default="restore,switch,input,idle", help="comma list; a first start is always made")
    args = ap.parse_args()

    binary = os.path.realpath(args.binary)
    os.makedirs(args.out, exist_ok=True)
    root = tempfile.mkdtemp(prefix="m1p-", dir="/tmp/claude-1000")
    os.chmod(root, 0o700)
    for sub in ("run", "state", "config", "data", "cache", "home", "logs"):
        os.makedirs(os.path.join(root, sub), mode=0o700, exist_ok=True)
    app_args = []
    if args.m3_fixture:
        if not (args.m3_sdk_root and args.m3_script):
            raise SystemExit("--m3-fixture needs --m3-sdk-root and --m3-script")
        for sub in ("m3-state", "m3-ws"):
            os.makedirs(os.path.join(root, sub), mode=0o700, exist_ok=True)
        shutil.copy(args.m3_script, os.path.join(root, "m3-script.json"))
        for seed in args.m3_seed:
            name, _, text = seed.partition("=")
            if "/" in name or not name:
                raise SystemExit("--m3-seed names a file in the workspace root")
            with open(os.path.join(root, "m3-ws", name), "w") as f:
                f.write(text.replace("\\n", "\n"))
        app_args = ["--m3-fixture-sdk-root", os.path.realpath(args.m3_sdk_root),
                    "--m3-fixture-state", os.path.join(root, "m3-state"),
                    "--m3-fixture-workspace", os.path.join(root, "m3-ws"),
                    "--m3-fixture-script", os.path.join(root, "m3-script.json")]
    fixture_bin = None
    if args.m2_fixtures:
        fixture_bin = os.path.join(root, "fixture-bin")
        os.makedirs(fixture_bin, mode=0o700)
        for name, version in (("claude", "2.1.309 (Claude Code)"),
                              ("codex", "codex-cli 0.159.0")):
            script = ("#!/bin/sh\n"
                      "if [ \"${1-}\" = \"--version\" ]; then\n"
                      f"  printf '%s\\n' '{version}'\n"
                      "  exit 0\n"
                      "fi\n"
                      f"printf '%s\\n' 'M2_FIXTURE_{name.upper()}_STARTED'\n"
                      f"while IFS= read -r line; do printf '%s\\n' 'M2_FIXTURE_{name.upper()}_INPUT' \"$line\"; done\n")
            path = os.path.join(fixture_bin, name)
            with open(path, "w") as f:
                f.write(script)
            os.chmod(path, 0o700)
        os.makedirs(os.path.join(root, "home", ".claude"), mode=0o700)
        with open(os.path.join(root, "home", ".claude", "CLAUDE.md"), "w") as f:
            f.write("# Disposable M2 GUI fixture\n\nInitial instruction text for edit, template, and Undo checks.\n")
        os.chmod(os.path.join(root, "home", ".claude", "CLAUDE.md"), 0o600)
    logs = os.path.join(root, "logs")
    if args.seed:
        # Copies, into the run's own private state: the originals are only read.
        mine = os.path.join(root, "state", "wyss-console")
        os.makedirs(os.path.join(mine, "configuration"), mode=0o700, exist_ok=True)
        for name in ("layout.json", os.path.join("configuration", "trusted-writers")):
            src = os.path.join(args.seed, name)
            if os.path.exists(src):
                shutil.copyfile(src, os.path.join(mine, name))
                os.chmod(os.path.join(mine, name), 0o600)
        if os.path.isdir(os.path.join(args.seed, "native")):
            shutil.copytree(os.path.join(args.seed, "native"), os.path.join(mine, "native"))
            for dirpath, dirs, files in os.walk(os.path.join(mine, "native")):
                os.chmod(dirpath, 0o700)
                for f in files:
                    os.chmod(os.path.join(dirpath, f), 0o600)
    nest = host = app = frames = pointer = keyring = keyring_log = None
    result = {"run": args.run, "shape": args.shape, "binary": binary,
              "loadavg": open("/proc/loadavg").read().split()[:3],
              "mem_available_mib": int([l for l in open("/proc/meminfo") if l.startswith("MemAvailable")][0].split()[1]) // 1024,
              "screen": SCREEN, "display": DISPLAY, "events": {}}
    try:
        nest = Nest(root, logs)
        x_env = {"PATH": BASE_PATH, "DISPLAY": DISPLAY, "HOME": os.path.join(root, "nest-home")}
        app_env = {
            "PATH": f"{fixture_bin}:{BASE_PATH}" if fixture_bin else BASE_PATH,
            "HOME": os.path.join(root, "home"),
            "XDG_RUNTIME_DIR": os.path.join(root, "run"), "XDG_STATE_HOME": os.path.join(root, "state"),
            "XDG_CONFIG_HOME": os.path.join(root, "config"), "XDG_DATA_HOME": os.path.join(root, "data"),
            "XDG_CACHE_HOME": os.path.join(root, "cache"), "DISPLAY": DISPLAY, "GDK_BACKEND": "x11",
            "NO_AT_BRIDGE": "1", "GTK_USE_PORTAL": "0", "XDG_CURRENT_DESKTOP": "KDE",
            "KDE_SESSION_VERSION": "6", "LANG": "C.UTF-8", "SHELL": os.environ.get("SHELL", "/bin/sh"),
            "DBUS_SESSION_BUS_ADDRESS": nest.bus("app-bus"),
        }
        if fixture_bin:
            # Keep the disposable M2 GUI accessible to AT-SPI driven test tooling.
            app_env.pop("NO_AT_BRIDGE", None)
            # The isolated Secret Service lets the M2 GUI exercise sealed Undo without
            # reading or using the account's real keyring. Its password exists only in this
            # temporary HOME and the owned daemon is stopped before the private bus.
            control = os.path.join(root, "run", "fixture-keyring")
            os.makedirs(control, mode=0o700)
            password = os.path.join(root, "home", ".fixture-keyring-passphrase")
            with open(password, "w") as f:
                f.write("M2 disposable keyring passphrase\n")
            os.chmod(password, 0o600)
            starter = os.path.join(root, "run", "start-fixture-keyring.sh")
            with open(starter, "w") as f:
                f.write("#!/bin/sh\n/usr/bin/gnome-keyring-daemon --components=secrets --foreground --unlock --control-directory \"$XDG_RUNTIME_DIR/fixture-keyring\" < \"$HOME/.fixture-keyring-passphrase\"\n")
            os.chmod(starter, 0o700)
            keyring_log = open(os.path.join(logs, "keyring.log"), "wb")
            keyring = Owned(["/bin/sh", starter], "/bin/sh", env=app_env,
                            log=keyring_log)
            secret_attrs = ["service", "wyss-m2-fixture", "account", "integration-probe"]
            stored = subprocess.run(
                ["/usr/bin/secret-tool", "store", "--label", "M2 disposable fixture", *secret_attrs],
                input=b"fixture marker only\n", capture_output=True, env=app_env,
                timeout=8, check=False,
            )
            looked_up = subprocess.run(
                ["/usr/bin/secret-tool", "lookup", *secret_attrs],
                capture_output=True, env=app_env, timeout=8, check=False,
            )
            if stored.returncode != 0 or looked_up.returncode != 0 or looked_up.stdout != b"fixture marker only\n":
                raise RuntimeError("the disposable M2 Secret Service fixture did not round trip")
            print("M2 fixture Secret Service round trip: PASS", flush=True)
        # The host, apart from any window, as the application would have it.
        with open(os.path.join(logs, "host.log"), "wb") as hostlog:
            host = Owned([binary, "--wyss-host"], binary, env=app_env, log=hostlog)
            socket_path = os.path.join(root, "run", "wyss-console", "host.sock")
            end = time.monotonic() + 15
            while not os.path.exists(socket_path):
                if time.monotonic() > end or not host.alive():
                    raise RuntimeError("the disposable host did not come up")
                time.sleep(0.1)
            if args.shape == "none":
                world = {"shape": "none", "projects": []}
            else:
                world = build_world(root, args.shape, os.path.realpath(args.test_binary), logs)
            result["world"] = {"shape": world["shape"], "projects": len(world["projects"]),
                               "terminals": sum(len(p["terminals"]) for p in world["projects"])}
            pointer = Pointer(x_env)
            if args.steps:
                app_ctl = AppCtl(binary, app_env, logs, app_args)
                app = app_ctl.app
                time.sleep(7.0 if not app_args else 10.0)
                place_window(x_env)
                time.sleep(1.5)
                try:
                    run_steps(json.load(open(args.steps)), x_env, pointer, args.out, args.run,
                              world, socket_path, (app.pid,), root, app_ctl, host.pid)
                finally:
                    app = app_ctl.app
                print("host alive:", host.alive(), "code:", host.proc.poll())
                print(json.dumps(host_list(socket_path))[:1500])
                return 0
            measure(args, result, world, None, x_env, pointer, host, None, logs, binary, app_env, root)
    except NotOurs as why:
        print(f"ABORT: {why}", file=sys.stderr)
        return 1
    finally:
        if frames:
            frames.stop()
        if pointer:
            pointer.close()
        if app:
            app.stop()
        if host:
            host.adopt_tree()
            host.stop()
        if keyring:
            keyring.adopt_tree()
            keyring.stop()
        if keyring_log:
            keyring_log.close()
        if nest:
            nest.stop()
        with open(os.path.join(args.out, f"run{args.run}.json"), "w") as f:
            json.dump(result, f, indent=1, default=str)
            f.write("\n")
        for name in ("app.log", "host.log", "world.log", "keyring.log"):
            src = os.path.join(root, "logs", name)
            if os.path.exists(src):
                shutil.copy(src, os.path.join(args.out, f"run{args.run}.{name}"))
        result["leftovers"] = leftovers(root)
        with open(os.path.join(args.out, f"run{args.run}.json"), "w") as f:
            json.dump(result, f, indent=1, default=str)
            f.write("\n")
        mounted = [l.split()[1] for l in open("/proc/mounts") if l.split()[1].startswith(root)]
        if mounted:
            print(f"left {root}: still mounted {mounted}", file=sys.stderr)
        else:
            shutil.rmtree(root, ignore_errors=True)
    return 0


def real_paths_state():
    """Metadata only (never contents) of the installed Console's own directories, which a disposable
    run must not touch: existence, change time, size and entry count."""
    out = {}
    home = os.path.expanduser("~")
    for rel in (".config/wyss-console", ".local/state/wyss-console", ".local/share/wyss-console", ".local/opt/wyss-console"):
        p = os.path.join(home, rel)
        try:
            st = os.lstat(p)
            entries = len(os.listdir(p)) if os.path.isdir(p) else None
            out[rel] = {"mtime_ns": st.st_mtime_ns, "size": st.st_size, "entries": entries}
        except OSError:
            out[rel] = None
    return out


def leftovers(root):
    """Processes of this user that still carry this run's directory in their environment. Only
    read: it reports, it ends nothing."""
    found = []
    needle = root.encode()
    for entry in os.listdir("/proc"):
        if not entry.isdigit() or int(entry) == os.getpid():
            continue
        try:
            with open(f"/proc/{entry}/environ", "rb") as f:
                if needle in f.read():
                    found.append({"pid": int(entry), "comm": comm_of(entry)})
        except OSError:
            continue
    return found


def run_steps(steps, x_env, pointer, out, run, world=None, socket_path=None,
              session_pid=(None,), fixture_root=None, app_ctl=None, host_pid=None):
    """Calibration: click, type, wait and take pictures, to read the screen by eye."""
    fixture_baseline = None
    fixture_snapshots = {}
    fs_baseline = None
    m4_ctx = {}
    for step in steps:
        kind = step[0]
        if os.environ.get("M1_TRACE_HOST") and socket_path:
            try:
                rows = host_list(socket_path)["ok"]
                print(f"before {step[:3]}: terminals={len(rows)} {[r['terminal']['state'] for r in rows]}", flush=True)
            except Exception as e:  # noqa: BLE001
                print(f"before {step[:3]}: host list failed {e}", flush=True)
        if kind == "click":
            pointer.do(f"mousemove {step[1]} {step[2]} click 1")
        elif kind == "key":
            pointer.do("key " + step[1])
        elif kind == "type":
            pointer.do("type --delay 40 " + step[1])
        elif kind == "wait":
            time.sleep(step[1])
        elif kind == "shot":
            shot(x_env, os.path.join(out, f"cal{run}-{step[1]}.png"))
        elif kind == "threads":
            # CPU and wake-ups by thread of the application and of its web process, for a window.
            for pid in descendants(session_pid[0]):
                if comm_of(pid) in ("WebKitWebProces", "wyss-console"):
                    done = subprocess.run([sys.executable, "-I", os.path.join(HERE, "threads.py"), str(pid), str(step[1])],
                                          capture_output=True, text=True)
                    with open(os.path.join(out, f"threads{run}-{step[2]}-{comm_of(pid)}-{pid}.txt"), "w") as f:
                        f.write(done.stdout)
        elif kind == "hostwrite":
            host_write(socket_path, world["projects"][0]["terminals"][0]["terminal"],
                       step[1].encode().decode("unicode_escape").encode())
        elif kind == "restart-app":
            # The window ends and a new one opens over the same host and state; the host and its
            # terminals are not touched. The new window is waited for like the first.
            app_ctl.restart()
            session_pid = (app_ctl.app.pid,)
            time.sleep(7.0 if not app_ctl.args else 10.0)
            place_window(x_env)
            time.sleep(1.5)
        elif kind.startswith("m4-"):
            ctx = {"socket": socket_path, "host_pid": host_pid, "m4": m4_ctx}
            ok, detail = m4_steps.check((kind[3:].replace("-", "_"), *step[1:]), ctx)
            print(f"{kind} {step[1:]}: {'ok' if ok else 'FAILED'} {detail}", flush=True)
            if not ok:
                raise AssertionError(f"{kind} {step[1:]} failed: {detail}")
        elif kind.startswith("m3-"):
            if fixture_root is None:
                raise RuntimeError("m3 checks need --m3-fixture")
            ctx = {"state": os.path.join(fixture_root, "m3-state"), "ws": os.path.join(fixture_root, "m3-ws"),
                   "root": fixture_root}
            ok, detail = m3_steps.check((kind[3:].replace("-", "_"), *step[1:]), ctx)
            print(f"{kind} {step[1:]}: {'ok' if ok else 'FAILED'} {detail}", flush=True)
            if not ok:
                raise AssertionError(f"{kind} {step[1:]} failed: {detail}")
        elif kind == "fs-baseline":
            fs_baseline = real_paths_state()
        elif kind == "fs-compare":
            now = real_paths_state()
            changed = {k: [fs_baseline.get(k), now[k]] for k in now if fs_baseline is None or fs_baseline.get(k) != now[k]}
            with open(os.path.join(out, f"fs-{run}-{step[1]}.json"), "w") as f:
                json.dump({"compared": sorted(now), "changed": changed}, f, indent=2)
                f.write("\n")
            if changed:
                raise AssertionError(f"the installed Console's own directories changed: {sorted(changed)}")
        elif kind == "fs-tree":
            tree = []
            for d, dirs, files in os.walk(fixture_root):
                for n in sorted(dirs + files):
                    p = os.path.join(d, n)
                    if os.path.islink(p) or os.path.isdir(p):
                        tree.append([os.path.relpath(p, fixture_root), "dir" if os.path.isdir(p) else "link"])
                    elif os.path.isfile(p):
                        tree.append([os.path.relpath(p, fixture_root), os.path.getsize(p)])
            with open(os.path.join(out, f"tree-{run}-{step[1]}.json"), "w") as f:
                json.dump(tree, f, indent=1)
                f.write("\n")
        elif kind == "native-state":
            rows = host_call(socket_path, "native_sessions", {"sample": False})
            state = rows.get("ok", [])
            summary = [{"id": row.get("id"), "restart_of": row.get("restart_of"),
                        "terminal_id": row.get("terminal_id"),
                        "agent_id": row.get("agent_id"), "state": row.get("state"),
                        "version": row.get("version"), "profile": row.get("profile"),
                        "restarts": row.get("restarts"), "error": row.get("error")}
                       for row in state]
            with open(os.path.join(out, f"native-{run}-{step[1]}.json"), "w") as f:
                json.dump(summary, f, indent=2)
                f.write("\n")
            print(f"native state {step[1]}:", json.dumps(summary), flush=True)
        elif kind == "assert-native-count":
            rows = host_call(socket_path, "native_sessions", {"sample": False}).get("ok", [])
            if len(rows) != step[1]:
                raise AssertionError(f"expected {step[1]} Native sessions; got {len(rows)}")
            if len(step) > 2 and rows[0].get("restarts") != step[2]:
                raise AssertionError(f"expected restart count {step[2]}; got {rows[0].get('restarts')}")
        elif kind == "assert-native-restart":
            rows = host_call(socket_path, "native_sessions", {"sample": False}).get("ok", [])
            replaced = [row for row in rows if row.get("restart_of")]
            old = [row for row in rows if row.get("state") == "stopped"]
            if (len(rows) != 2 or len(replaced) != 1 or len(old) != 1
                    or replaced[0].get("state") != "running"
                    or replaced[0].get("restarts") != 1
                    or replaced[0].get("restart_of") != old[0].get("id")
                    or replaced[0].get("terminal_id") == old[0].get("terminal_id")):
                raise AssertionError(f"restart did not preserve/replace as expected: {rows}")
        elif kind == "fixture-file-snapshot":
            import hashlib
            if fixture_root is None:
                raise RuntimeError("fixture file checks require --m2-fixtures")
            path = os.path.join(fixture_root, "home", ".claude", "CLAUDE.md")
            data = open(path, "rb").read()
            digest = hashlib.sha256(data).hexdigest()
            if step[1] == "initial":
                fixture_baseline = digest
            elif step[2] == "same" and digest != fixture_baseline:
                raise AssertionError(f"fixture file {step[1]} should match its initial SHA-256")
            elif step[2] == "different" and digest == fixture_baseline:
                raise AssertionError(f"fixture file {step[1]} should differ from its initial SHA-256")
            elif step[2] == "same-as" and digest != fixture_snapshots.get(step[3]):
                raise AssertionError(f"fixture file {step[1]} should match snapshot {step[3]}")
            snapshot = {"label": step[1], "bytes": len(data), "sha256": digest}
            fixture_snapshots[step[1]] = digest
            with open(os.path.join(out, f"fixture-{run}-{step[1]}.json"), "w") as f:
                json.dump(snapshot, f, indent=2)
                f.write("\n")
            print(f"fixture file {step[1]}: {json.dumps(snapshot)}", flush=True)
        elif kind == "fixture-template-snapshot":
            import hashlib
            if fixture_root is None:
                raise RuntimeError("fixture template checks require --m2-fixtures")
            path = os.path.join(fixture_root, "home", "instructions.wyss-template")
            data = open(path, "rb").read()
            snapshot = {"label": step[1], "bytes": len(data),
                        "sha256": hashlib.sha256(data).hexdigest()}
            with open(os.path.join(out, f"template-{run}-{step[1]}.json"), "w") as f:
                json.dump(snapshot, f, indent=2)
                f.write("\n")
            print(f"fixture template {step[1]}: {json.dumps(snapshot)}", flush=True)
        elif kind == "scroll":
            pointer.do(f"mousemove {step[1]} {step[2]} click --repeat {step[3]} --delay 20 {step[4]}")


# Where the controls are on the 1500 x 1400 screen (read from pictures of the application; the rail
# of projects is sixty pixels a row, the pane of the single-pane shapes is the first at its top).
NAV = {"console": (75, 115), "projects": (75, 159), "settings": (75, 599)}
SCRATCH = (350, 325)
TAKE_INPUT = (1342, 232)
REGION = (486, 256, 994, 600)


def project_at(n):
    return (350, 388 + 60 * (n - 1))


def host_write(socket_path, terminal, data):
    """Types into a terminal through the host's own protocol (setup, never the thing measured)."""
    import socket
    import struct
    body = json.dumps({"op": "term_write", "params": {"terminal_id": terminal, "data": list(data)}}).encode()
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(10)
        sock.connect(socket_path)
        sock.sendall(struct.pack(">I", len(body)) + body)
        head = sock.recv(4)
        if len(head) == 4:
            sock.recv(struct.unpack(">I", head)[0])


def host_list(socket_path):
    """What the host says of its terminals (read only)."""
    import socket
    import struct
    body = json.dumps({"op": "term_list", "params": None}).encode()
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(10)
        sock.connect(socket_path)
        sock.sendall(struct.pack(">I", len(body)) + body)
        head = sock.recv(4)
        size = struct.unpack(">I", head)[0]
        data = b""
        while len(data) < size:
            data += sock.recv(size - len(data))
    return json.loads(data)


def host_call(socket_path, op, params):
    """One bounded read-only host request used to record Native fixture state."""
    import socket
    import struct
    body = json.dumps({"op": op, "params": params}).encode()
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(10)
        sock.connect(socket_path)
        sock.sendall(struct.pack(">I", len(body)) + body)
        head = sock.recv(4)
        size = struct.unpack(">I", head)[0]
        data = b""
        while len(data) < size:
            data += sock.recv(size - len(data))
    return json.loads(data)


class AppCtl:
    """The application window of a scenario, which a step may end and open again over the same host
    and state (the host and its terminals are not part of it)."""

    def __init__(self, binary, env, logs, args):
        self.binary, self.env, self.logs, self.args = binary, env, logs, list(args)
        self.count = 0
        self.app = self._start()

    def _start(self):
        self.count += 1
        return Session(self.binary, self.env, self.logs, "app" if self.count == 1 else f"app{self.count}", self.args)

    def restart(self):
        self.app.stop()
        self.app = self._start()


class Session:
    """The application on the nested display, started over the run's host and store, on the bus
    made for the run."""

    def __init__(self, binary, app_env, logs, name, args=()):
        self.log = open(os.path.join(logs, f"{name}.log"), "wb")
        self.owned = Owned([binary, *args], binary, env=app_env, log=self.log)
        self.pid = self.owned.pid

    def stop(self):
        self.owned.adopt_tree()
        self.owned.stop()
        self.log.close()


def place_window(x_env):
    subprocess.run(["/usr/bin/xdotool", "search", "--onlyvisible", "--name", "Wyss Console",
                    "windowmove", "0", "0", "windowsize", str(SCREEN[0]), str(SCREEN[1])],
                   env=x_env, check=False, timeout=15)


def click(pointer, xy):
    t0 = monotonic_ns()
    pointer.do(f"mousemove {xy[0]} {xy[1]} click 1")
    return t0


def idle(label, owned_pids, seconds, result):
    raw, figures = idle_window(owned_pids, seconds)
    result["idle"][label] = {"summary": figures, "raw": raw}
    return figures


def measure(args, result, world, layout, x_env, pointer, host, app, logs, binary, app_env, root):
    socket_path = os.path.join(root, "run", "wyss-console", "host.sock")

    def trace(label):
        if os.environ.get("M1_TRACE_HOST"):
            try:
                rows = host_list(socket_path)["ok"]
                print(f"{label}: terminals={len(rows)} {[r['terminal']['state'] for r in rows]}", flush=True)
            except Exception as e:  # noqa: BLE001
                print(f"{label}: host list failed {e}", flush=True)

    projects = world["projects"]
    result["idle"] = {}
    result["events"] = {}
    session = None
    frames = None

    def owned_pids():
        return [p for p in (session.pid if session else None, host.pid) if p]

    try:
        # --- restore: the application started over the store and the host, three times -------------
        restores = []
        phases = args.phases.split(",")
        for attempt in range(3 if "restore" in phases else 1):
            if session:
                session.stop()
                time.sleep(2.0)
            trace("before app")
            session = Session(binary, app_env, logs, f"app{attempt}")
            time.sleep(7.0)
            trace("app up")
            place_window(x_env)
            time.sleep(1.5)
            if attempt == 0 and "idle" in phases:
                # What the application costs before the Projects view is ever opened.
                idle("console_only", owned_pids, args.idle_seconds, result)
            frames = Frames(REGION, logs)
            time.sleep(1.0)
            click(pointer, NAV["projects"])
            time.sleep(2.0)
            trace("projects open")
            settle(frames)
            t0 = click(pointer, project_at(1))
            # The settled picture of this first open is what a later one must equal.
            end = monotonic_ns() + int(12e9)
            first = frames.latest()
            while monotonic_ns() < end and (differs(frames.latest(), first) < 0.002):
                time.sleep(0.005)
            picture = settle(frames, 1.5)
            if attempt == 0:
                ref_first = picture
            ms = time_to(frames, t0, picture, tolerance=0.0015, timeout_s=12.0)
            restores.append(ms)
            frames.stop()
            frames = None
        result["events"]["restore_ms"] = {"samples": restores, **summary([r for r in restores if r])}

        trace("restore done")
        # --- the references: every project's settled picture, and Scratch's -----------------------
        frames = Frames(REGION, logs)
        time.sleep(1.0)
        targets = {"scratch": SCRATCH}
        for n in range(1, len(projects) + 1):
            targets[f"p{n:02d}"] = project_at(n)
        refs = {}
        for name, xy in targets.items():
            click(pointer, xy)
            time.sleep(1.8)
            refs[name] = settle(frames, 1.0)
        # --- switching -------------------------------------------------------------------------------
        if "switch" not in phases:
            args.switches = 0
        rng = random.Random(1)
        names = list(targets)
        order, previous = [], None
        while len(order) < args.switches:
            pick = rng.choice(names)
            if pick != previous:
                order.append(pick)
                previous = pick
        switches = []
        order = order or ["scratch"]
        click(pointer, targets[order[0]])
        time.sleep(1.5)
        current = order[0]
        for name in order[1:]:
            # a pair of pictures too alike to tell apart is not a measurement
            if differs(refs[name], refs[current]) < 0.003:
                continue
            time.sleep(0.6)
            t0 = click(pointer, targets[name])
            ms = time_to(frames, t0, refs[name], tolerance=0.0015, timeout_s=8.0)
            switches.append({"to": name, "ms": ms})
            current = name
        result["events"]["switch_ms"] = {"samples": switches,
                                         **summary([s["ms"] for s in switches if s["ms"] is not None])}
        result["events"]["switch_failed"] = sum(1 for s in switches if s["ms"] is None)

        trace("switching done")
        # --- input to render (the single-pane shapes) ----------------------------------------------
        if world["shape"] in ("a", "b") and "input" in phases:
            click(pointer, targets["p01"])
            time.sleep(1.8)
            click(pointer, TAKE_INPUT)
            time.sleep(1.0)
            # No window manager is running, so the window is never made active; a click inside the
            # terminal gives the page the keyboard, as it would be given to a person's first click.
            click(pointer, (980, 700))
            time.sleep(0.5)
            terminal = projects[0]["terminals"][0]["terminal"]
            trace("input taken")
            watch_log = []
            if os.environ.get("M1_TRACE_HOST"):
                def watch():
                    last = None
                    while not stop_watch.is_set():
                        try:
                            rows = host_list(socket_path)["ok"]
                            now = [(r["terminal"]["state"], r["terminal"].get("exit"), r["terminal"].get("error")) for r in rows]
                        except Exception as e:  # noqa: BLE001
                            now = [("list-failed", str(e), None)]
                        if now != last:
                            watch_log.append((round(time.monotonic(), 2), now))
                            last = now
                        time.sleep(0.05)
                stop_watch = threading.Event()
                threading.Thread(target=watch, daemon=True).start()
            keys, echoes = [], []
            for i in range(args.keys + 1):
                # Setup, not measured: a screenful of long lines, then the picture is left alone.
                host_write(socket_path, terminal, b"seq -f ABCDEFGHIJKLMNOPQRSTUVWXYZ%g 1 60\r")
                time.sleep(0.9)
                trace(f"after write {i}")
                if os.environ.get("M1_DEBUG_SHOTS") and i < 2:
                    shot(x_env, os.path.join(os.environ["M1_DEBUG_SHOTS"], f"input-{i}.png"))
                    with open(os.path.join(os.environ["M1_DEBUG_SHOTS"], f"frame-{i}.gray"), "wb") as raw:
                        raw.write(frames.latest().tobytes())
                filled = settle(frames, 0.8)
                if os.environ.get("M1_DEBUG_SHOTS") and i < 2:
                    with open(os.path.join(os.environ["M1_DEBUG_SHOTS"], f"filled-{i}.gray"), "wb") as raw:
                        raw.write(filled.tobytes())
                t0 = monotonic_ns()
                pointer.do("key ctrl+l")
                ms = time_to_change(frames, t0, filled)
                trace(f"after key {i}")
                after = frames.since(t0)
                result["events"].setdefault("input_debug", []).append(
                    {"frames_after": len(after),
                     "max_diff": max((differs(f, filled) for _, f in after), default=None),
                     "filled_nonblank": float(np.mean(filled > 40))})
                settle(frames, 0.8)
                # One character at the prompt: the smallest thing a person types.
                base = frames.latest()
                t1 = monotonic_ns()
                pointer.do("key x")
                echo = time_to_change(frames, t1, base, threshold=0.00004, timeout_s=3.0)
                time.sleep(0.4)
                pointer.do("key ctrl+u")
                time.sleep(0.3)
                if i == 0:
                    continue  # the first one only warms the path
                keys.append(ms)
                echoes.append(echo)
            if os.environ.get("M1_TRACE_HOST"):
                stop_watch.set()
                print("watch:", watch_log, flush=True)
                click(pointer, NAV["console"])
                time.sleep(1.5)
                shot(x_env, os.path.join(os.environ["M1_DEBUG_SHOTS"], "console-after.png"))
            result["events"]["input_to_render_ms"] = {"samples": keys, **summary([k for k in keys if k])}
            result["events"]["echo_to_render_ms"] = {"samples": echoes, **summary([k for k in echoes if k])}
        frames.stop()
        frames = None

        # --- idle with the view shown, and with it hidden -------------------------------------------
        if "idle" in phases:
            click(pointer, targets["p01"])
            time.sleep(2.5)
            idle("projects_shown", owned_pids, args.idle_seconds, result)
            click(pointer, NAV["settings"])
            time.sleep(2.5)
            idle("projects_hidden", owned_pids, args.idle_seconds, result)
    finally:
        if frames:
            frames.stop()
        if session:
            result["session_pid_last"] = session.pid
            session.stop()


if __name__ == "__main__":
    sys.exit(main())
