"""bubblewrap launcher for isolated SDK experiments.

The child gets no network beyond a private loopback, a throw-away HOME, an empty environment
plus what the caller names, and read-only system directories. Nothing of the real user's
home, credentials, keyring, D-Bus or display is visible. `--unshare-all` includes the
network namespace; bubblewrap brings that namespace's loopback up, which is all the mock
API needs.
"""

import os
import subprocess

BWRAP = "/usr/bin/bwrap"
SYSTEM_RO = ["/usr", "/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/ld.so.conf.d", "/etc/ssl",
             "/etc/ca-certificates", "/etc/passwd", "/etc/group", "/etc/nsswitch.conf", "/etc/localtime"]


def argv(*, home, workdir, ro=(), rw=(), env, command, extra=()):
    a = [BWRAP, "--unshare-all", "--die-with-parent", "--new-session", "--clearenv",
         "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"]
    for lib in ("bin", "sbin", "lib", "lib64"):
        if os.path.islink("/" + lib):
            a += ["--symlink", os.readlink("/" + lib), "/" + lib]
    for p in SYSTEM_RO:
        if os.path.exists(p):
            a += ["--ro-bind", p, p]
    for p in ro:
        a += ["--ro-bind", p, p]
    a += ["--bind", home, "/home/probe"]
    for p in rw:
        a += ["--bind", p, p]
    a += list(extra)
    a += ["--chdir", workdir]
    for k, v in env.items():
        a += ["--setenv", k, v]
    return a + ["--"] + list(command)


def base_env(base_url, extra=None):
    env = {
        "PATH": "/usr/bin",
        "HOME": "/home/probe",
        "LANG": "C.UTF-8",
        "TMPDIR": "/tmp",
        # Synthetic credential: it authenticates nothing anywhere.
        "ANTHROPIC_API_KEY": "sk" + "-ant-api03-synthetic-wyss-probe-" + "0" * 60,
        "ANTHROPIC_BASE_URL": base_url,
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
        "DISABLE_AUTOUPDATER": "1",
        "DISABLE_TELEMETRY": "1",
        "DISABLE_ERROR_REPORTING": "1",
        "DISABLE_BUG_COMMAND": "1",
    }
    env.update(extra or {})
    return env


def run(args, timeout, stdin=None):
    return subprocess.run(args, capture_output=True, timeout=timeout, input=stdin, text=True, env={})
