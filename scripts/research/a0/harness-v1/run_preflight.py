"""AP-0 preflight: run the isolation probe inside the exact sandbox the pilot will use, plus the
host negative control. No provider is executed (the Claude binary is bind-mounted and hashed
only). Usage: python3 -I run_preflight.py <A0_ROOT>
"""
import json
import os
import shlex
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import integrity  # noqa: E402
from sandbox import argv, base_env  # noqa: E402

ROOT = os.path.realpath(sys.argv[1])
H = os.path.join(ROOT, "harness")
EV = os.path.join(ROOT, "evidence", "preflight")
CLAUDE = os.path.realpath(os.path.expanduser("~/.local/share/claude/versions/2.1.312"))
CLAUDE_SHA = "4f4178ce41d83e23341ebfbaecdbcf8ea6bf7baea0690025abe49eeec3a5d01d"


def inside(path):
    p = os.path.realpath(path)
    assert p.startswith(ROOT + os.sep), p
    return p


def main():
    os.makedirs(EV, mode=0o700, exist_ok=True)
    home = inside(os.path.join(ROOT, "homes", "preflight"))
    work = inside(os.path.join(ROOT, "work", "preflight"))
    markers = inside(os.path.join(EV, "markers"))
    for d in (home, work, markers):
        os.makedirs(d, mode=0o700, exist_ok=True)
    integrity.main(os.path.join(EV, "integrity-before.json"))
    host_pids = ",".join(str(p["pid"]) for p in integrity.provider_pids())
    env = base_env({"A0_EXPECTED_CLAUDE_SHA256": CLAUDE_SHA, "A0_HOST_PIDS": host_pids})
    cmd = argv(home=home, work=work, markers=markers, harness=H, env=env, claude_binary=CLAUDE,
               command=["/usr/bin/python3", "-I", "/a0/harness/preflight_probe.py"])
    with open(os.path.join(EV, "command.txt"), "w") as f:
        f.write(" ".join(shlex.quote(c) for c in cmd) + "\n")
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=60, stdin=subprocess.DEVNULL,
                       close_fds=True, env={})
    open(os.path.join(EV, "sandbox.stdout.json"), "w").write(r.stdout)
    open(os.path.join(EV, "sandbox.stderr.txt"), "w").write(r.stderr)
    # Negative control: the same probe on the host, passive checks only.
    henv = {k: v for k, v in os.environ.items()}
    henv["A0_HOST_CONTROL"] = "1"
    c = subprocess.run([sys.executable, "-I", os.path.join(H, "preflight_probe.py")],
                       capture_output=True, text=True, timeout=60, stdin=subprocess.DEVNULL,
                       env=henv, close_fds=True)
    open(os.path.join(EV, "host-control.stdout.json"), "w").write(c.stdout)
    open(os.path.join(EV, "host-control.stderr.txt"), "w").write(c.stderr)
    integrity.main(os.path.join(EV, "integrity-after.json"))
    sb = json.loads(r.stdout) if r.stdout.strip().startswith("{") else None
    hc = json.loads(c.stdout) if c.stdout.strip().startswith("{") else None
    summary = {
        "sandbox_exit": r.returncode,
        "sandbox_all_pass": bool(sb and sb["all_pass"]),
        "sandbox_failed": sorted(k for k, v in (sb or {}).get("checks", {}).items() if not v["pass"]),
        "host_control_exit": c.returncode,
        "host_control_failed_checks": sorted(k for k, v in (hc or {}).get("checks", {}).items()
                                             if not v["pass"]),
    }
    json.dump(summary, open(os.path.join(EV, "summary.json"), "w"), indent=1)
    print(json.dumps(summary, indent=1))
    if r.stderr.strip():
        print("sandbox stderr:", r.stderr.strip()[:2000])


if __name__ == "__main__":
    main()
