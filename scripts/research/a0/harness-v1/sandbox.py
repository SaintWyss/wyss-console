"""bwrap argv for the A0 pilot. Adapted from wyss-console src-tauri/claude-bridge/probe/sandbox.py
(read, not modified): every namespace unshared (network included: only a private loopback),
empty environment, private /tmp, read-only system dirs, the real HOME never mounted.
"""
import os

BWRAP = "/usr/bin/bwrap"
SYSTEM_RO = ["/usr", "/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/ld.so.conf.d", "/etc/ssl",
             "/etc/ca-certificates", "/etc/passwd", "/etc/group", "/etc/nsswitch.conf",
             "/etc/localtime"]
SYNTH_KEY = "sk" + "-ant-api03-synthetic-wyss-a0-" + "0" * 60  # split like claude-bridge/probe (scanner-safe)


def argv(*, home, work, markers, harness, env, command, claude_binary=None, extra_ro=()):
    a = [BWRAP, "--unshare-all", "--die-with-parent", "--new-session", "--clearenv",
         "--cap-drop", "ALL", "--hostname", "a0-sandbox",
         "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--dir", "/run"]
    for lib in ("bin", "sbin", "lib", "lib64"):
        if os.path.islink("/" + lib):
            a += ["--symlink", os.readlink("/" + lib), "/" + lib]
    for p in SYSTEM_RO:
        if os.path.exists(p):
            a += ["--ro-bind", p, p]
    a += ["--dir", "/home", "--bind", home, "/home/probe",
          "--bind", work, "/work",
          "--bind", markers, "/markers",
          "--ro-bind", harness, "/a0/harness"]
    if claude_binary:
        a += ["--ro-bind", claude_binary, "/opt/a0/bin/claude"]
    for src, dst in extra_ro:
        a += ["--ro-bind", src, dst]
    a += ["--remount-ro", "/", "--chdir", "/work"]
    for k, v in env.items():
        a += ["--setenv", k, v]
    return a + ["--"] + list(command)


def base_env(extra=None):
    env = {
        "PATH": "/opt/a0/bin:/usr/bin",
        "HOME": "/home/probe",
        "XDG_CONFIG_HOME": "/home/probe/.config",
        "XDG_DATA_HOME": "/home/probe/.local/share",
        "XDG_STATE_HOME": "/home/probe/.local/state",
        "XDG_CACHE_HOME": "/home/probe/.cache",
        "LANG": "C.UTF-8",
        "TMPDIR": "/tmp",
        "TERM": "xterm-256color",
    }
    env.update(extra or {})
    return env
