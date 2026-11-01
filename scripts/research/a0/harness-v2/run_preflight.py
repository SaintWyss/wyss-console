"""v2 isolation preflight: the probe runs through the SAME launcher and argv builder as v2 cases
(sized tmpfs HOME/work//tmp, rlimits, json-status fd, key on stdin). No provider executes: the
Claude binary is bind-mounted and hashed only. Host negative control as in v1.
Usage: python3 -I run_preflight.py <A0_ROOT> <out_dir>
"""
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import integrity  # noqa: E402
import limits  # noqa: E402
from launch import run_sandbox  # noqa: E402
from sandbox import argv, base_env  # noqa: E402

ROOT = os.path.realpath(sys.argv[1])
OUT = os.path.realpath(sys.argv[2])
CLAUDE = os.path.realpath(os.path.expanduser("~/.local/share/claude/versions/2.1.312"))
CLAUDE_SHA = "4f4178ce41d83e23341ebfbaecdbcf8ea6bf7baea0690025abe49eeec3a5d01d"
RL = {"RLIMIT_FSIZE": [limits.FSIZE, limits.FSIZE], "RLIMIT_CORE": [0, 0],
      "RLIMIT_NOFILE": [limits.NOFILE, limits.NOFILE], "RLIMIT_CPU": [limits.CPU_S, limits.CPU_S]}


def main():
    assert OUT.startswith(ROOT + os.sep)
    os.makedirs(OUT, mode=0o700, exist_ok=False)
    markers = os.path.join(OUT, "markers")
    os.makedirs(markers, mode=0o700)
    integrity.main(os.path.join(OUT, "integrity-before.json"))
    host_pids = ",".join(str(p["pid"]) for p in integrity.provider_pids())
    env = base_env({"A0_EXPECTED_CLAUDE_SHA256": CLAUDE_SHA, "A0_HOST_PIDS": host_pids,
                    "A0_EXPECT_SIZE_TMP": f"{limits.TMP_TMPFS // 1024}k",
                    "A0_EXPECT_SIZE_HOME": f"{limits.HOME_TMPFS // 1024}k",
                    "A0_EXPECT_SIZE_WORK": f"{limits.WORK_TMPFS // 1024}k",
                    "A0_EXPECT_SIZE_MARKERS": f"{limits.MARKERS_TMPFS // 1024}k",
                    "A0_EXPECT_RLIMITS": json.dumps(RL)})
    build = lambda fd: argv(harness=HERE, env=env, claude_binary=CLAUDE,  # noqa: E731
                            status_fd=fd,
                            command=["/usr/bin/python3", "-I", "/a0/harness/preflight_probe.py"])
    r = run_sandbox(build, key=b"", timeout=60)
    Path(OUT, "command.json").write_text(json.dumps(r["argv"], indent=1))
    Path(OUT, "sandbox.stdout.json").write_text(r["stdout"])
    Path(OUT, "sandbox.stderr.txt").write_text(r["stderr"])
    henv = dict(os.environ)
    henv["A0_HOST_CONTROL"] = "1"
    c = subprocess.run([sys.executable, "-I", os.path.join(HERE, "preflight_probe.py")],
                       capture_output=True, text=True, timeout=60, stdin=subprocess.DEVNULL, env=henv)
    Path(OUT, "host-control.stdout.json").write_text(c.stdout)
    integrity.main(os.path.join(OUT, "integrity-after.json"))
    cmp = subprocess.run([sys.executable, "-I", os.path.join(HERE, "compare_integrity.py"),
                          os.path.join(OUT, "integrity-before.json"), os.path.join(OUT, "integrity-after.json")],
                         capture_output=True, text=True)
    sb = json.loads(r["stdout"]) if r["stdout"].strip().startswith("{") else {"checks": {}, "all_pass": False}
    hc = json.loads(c.stdout) if c.stdout.strip().startswith("{") else {"checks": {}}
    summary = {
        "sandbox_all_pass": sb["all_pass"],
        "sandbox_checks": len(sb["checks"]),
        "sandbox_passed": sum(v["pass"] for v in sb["checks"].values()),
        "sandbox_failed": sorted(k for k, v in sb["checks"].items() if not v["pass"]),
        "host_control_failed": sorted(k for k, v in hc["checks"].items() if not v["pass"]),
        "launcher": {k: r[k] for k in ("exit", "timed_out", "killed", "members_max", "leftovers", "pid_ns",
                                       "duration_s", "markers_peak")},
        "integrity": cmp.returncode == 0,
    }
    Path(OUT, "summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps(summary, indent=1))
    launcher_ok = (r["exit"] == 0 and not r["timed_out"] and r["killed"] is None
                   and r["pid_ns"] is not None and r["readers_complete"] and not r["leftovers"])
    control_ok = c.returncode == 0 and bool(summary["host_control_failed"])
    return 0 if summary["sandbox_all_pass"] and summary["integrity"] and launcher_ok and control_ok else 1


if __name__ == "__main__":
    sys.exit(main())
