"""bwrap argv for A0 (v2).

Retained from v1 unchanged: --unshare-all, --die-with-parent, --new-session, --clearenv,
--cap-drop ALL, private --proc/--dev, read-only system dirs, the real HOME never mounted,
--remount-ro /, provider binary read-only, launcher started with an EMPTY environment.

Added in v2 (independent review 2026-10-29):
- HOME (/home/probe), project (/work) and /tmp are size-capped tmpfs mounts (kernel-enforced);
  fixtures are staged read-only at /a0/fixture and copied in by the in-sandbox harness;
- --json-status-fd: the host learns the sandbox's PID-namespace inode and init PID;
- /markers is a size-capped tmpfs; no writable bind on the host disk at all. Evidence
  leaves only via the HMAC stream (FILE records), written host-side under a byte cap.
"""
import os

import limits

BWRAP = "/usr/bin/bwrap"
SYSTEM_RO = ["/usr", "/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/ld.so.conf.d", "/etc/ssl",
             "/etc/ca-certificates", "/etc/passwd", "/etc/group", "/etc/nsswitch.conf",
             "/etc/localtime"]
SYNTH_KEY = "sk" + "-ant-api03-synthetic-wyss-a0-" + "0" * 60  # split like claude-bridge/probe (scanner-safe)


def argv(*, harness, env, command, fixture=None, claude_binary=None, extra_ro=(),
         status_fd=None, home_size=limits.HOME_TMPFS, work_size=limits.WORK_TMPFS,
         tmp_size=limits.TMP_TMPFS, markers_size=limits.MARKERS_TMPFS):
    a = [BWRAP, "--unshare-all", "--die-with-parent", "--new-session", "--clearenv",
         "--cap-drop", "ALL", "--hostname", "a0-sandbox"]
    if status_fd is not None:
        a += ["--json-status-fd", str(status_fd)]
    a += ["--proc", "/proc", "--dev", "/dev",
          "--size", str(tmp_size), "--tmpfs", "/tmp", "--dir", "/run"]
    for lib in ("bin", "sbin", "lib", "lib64"):
        if os.path.islink("/" + lib):
            a += ["--symlink", os.readlink("/" + lib), "/" + lib]
    for p in SYSTEM_RO:
        if os.path.exists(p):
            a += ["--ro-bind", p, p]
    a += ["--dir", "/home",
          "--size", str(home_size), "--tmpfs", "/home/probe",
          "--size", str(work_size), "--tmpfs", "/work",
          "--size", str(markers_size), "--tmpfs", "/markers",
          "--ro-bind", harness, "/a0/harness"]
    if fixture:
        a += ["--ro-bind", fixture, "/a0/fixture"]
    if claude_binary:
        a += ["--ro-bind", claude_binary, "/opt/a0/bin/claude"]
    for src, dst in extra_ro:
        a += ["--ro-bind", src, dst]
    a += ["--remount-ro", "/", "--chdir", "/"]
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
