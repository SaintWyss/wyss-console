"""Isolation probe (plan §2.2 / owner condition 2). Runs inside the sandbox; with
A0_HOST_CONTROL=1 it runs on the host as the negative control and skips every active step
(no outbound connect, no write outside its own temp dir). Prints JSON; never prints an
environment value, only names.
"""
import errno
import json
import os
import socket
import sys

HOST = os.environ.get("A0_HOST_CONTROL") == "1"
import pwd  # noqa: E402
REAL_HOME = pwd.getpwuid(os.getuid()).pw_dir  # resolved at runtime, no home path in code
checks = {}


def check(name, ok, detail=None):
    checks[name] = {"pass": bool(ok), "detail": detail}


# 1 identity and namespaces
check("no_new_privs", "NoNewPrivs:\t1" in open("/proc/self/status").read())
uid_map = open("/proc/self/uid_map").read().split()
check("user_namespace_mapped", uid_map[:3] != ["0", "0", "4294967295"], uid_map[:3])

# 2 disposable HOME / XDG
home = os.environ.get("HOME", "")
xdg = {k: os.environ.get(k, "") for k in
       ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME")}
check("home_is_disposable", home == "/home/probe", home if not HOST else "<host HOME>")
check("xdg_inside_home", all(v.startswith("/home/probe/") for v in xdg.values()),
      {k: bool(v) for k, v in xdg.items()})
check("real_home_absent", not os.path.exists(REAL_HOME))
check("home_dir_listing_only_probe", sorted(os.listdir("/home")) == ["probe"],
      sorted(os.listdir("/home")) if not HOST else "<host>")

# 3 private /tmp, no host Codex daemon
tmp_fs = None
for line in open("/proc/self/mountinfo"):
    parts = line.split()
    if parts[4] == "/tmp":
        tmp_fs = parts[parts.index("-") + 1]
check("tmp_is_private_tmpfs", tmp_fs == "tmpfs" and len(os.listdir("/tmp")) == 0,
      {"fstype": tmp_fs, "entries": len(os.listdir("/tmp"))})
check("no_host_codex_daemon_dir", not os.path.exists("/tmp/codex-daemon-1000"))
daemon_sock_ok = True
if os.path.isdir("/tmp/codex-daemon-1000"):
    daemon_sock_ok = False
check("no_host_codex_socket_reachable", daemon_sock_ok)

# 4 credentials
cred_paths = [REAL_HOME + p for p in ("/.claude/.credentials.json", "/.codex/auth.json",
                                      "/.claude.json", "/.claude/daemon/control.key")]
check("no_real_credential_paths", not any(os.path.exists(p) for p in cred_paths))
names = sorted(os.environ)
allowed = {"PATH", "HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME",
           "LANG", "TMPDIR", "TERM", "A0_EXPECTED_CLAUDE_SHA256", "A0_HOST_PIDS",
           "A0_EXPECT_SIZE_TMP", "A0_EXPECT_SIZE_HOME", "A0_EXPECT_SIZE_WORK", "A0_EXPECT_RLIMITS",
           "A0_EXPECT_SIZE_MARKERS"}
extra = [n for n in names if n not in allowed and not (n == "PWD" and os.environ[n] in ("/work", "/"))]
check("env_only_allowed_names", not extra, extra if not HOST else f"{len(extra)} extra names")
sensitive = [n for n in names if any(s in n for s in ("KEY", "TOKEN", "SECRET", "PASSWORD", "AUTH"))]
check("no_sensitive_env_names", not sensitive, sensitive if not HOST else f"{len(sensitive)} names")

# 5 inherited file descriptors
fds = {}
for fd in os.listdir("/proc/self/fd"):
    try:
        fds[fd] = os.readlink(f"/proc/self/fd/{fd}")
    except OSError:
        pass
listing_fd = [fd for fd, t in fds.items() if t == "/proc/self/fd" or t.startswith("/proc/") and t.endswith("/fd")]
others = {fd: t for fd, t in fds.items() if fd not in ("0", "1", "2") and fd not in listing_fd}
check("no_inherited_extra_fds", not others, others if not HOST else f"{len(others)} extra fds")

# 6 session bus / display
bus_env = [n for n in ("DBUS_SESSION_BUS_ADDRESS", "DISPLAY", "WAYLAND_DISPLAY",
                       "XDG_RUNTIME_DIR", "SSH_AUTH_SOCK") if n in os.environ]
check("no_bus_display_env", not bus_env, bus_env)
paths = ["/run/user/1000", "/run/dbus/system_bus_socket", "/tmp/.X11-unix"]
check("no_bus_display_paths", not any(os.path.exists(p) for p in paths),
      [p for p in paths if os.path.exists(p)])
x11_abstract = "not_attempted"
if not HOST:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        s.connect("\0/tmp/.X11-unix/X0")
        x11_abstract = "CONNECTED"
    except OSError as e:
        x11_abstract = errno.errorcode.get(e.errno, str(e.errno))
    finally:
        s.close()
check("no_x11_abstract_socket", x11_abstract != "CONNECTED", x11_abstract)

# 7 network
ifaces = [l.split(":")[0].strip() for l in open("/proc/net/dev").read().splitlines()[2:]]
routes = open("/proc/net/route").read().splitlines()[1:]
check("only_loopback_interface", ifaces == ["lo"], ifaces if not HOST else f"{len(ifaces)} ifaces")
check("no_ipv4_routes", len(routes) == 0, len(routes))
outbound = {}
if not HOST:
    for label, addr in (("public_1.1.1.1:443", ("1.1.1.1", 443)),
                        ("lan_host_192.168.100.5:22", ("192.168.100.5", 22)),
                        ("lan_gw_192.168.100.1:53", ("192.168.100.1", 53))):
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(3)
        try:
            s.connect(addr)
            outbound[label] = "CONNECTED"
        except OSError as e:
            outbound[label] = errno.errorcode.get(e.errno, str(e.errno)) if e.errno else type(e).__name__
        finally:
            s.close()
    try:
        socket.getaddrinfo("api.anthropic.com", 443)
        outbound["dns_api.anthropic.com"] = "RESOLVED"
    except OSError as e:
        outbound["dns_api.anthropic.com"] = type(e).__name__
    lo = socket.socket()
    lo.bind(("127.0.0.1", 0))
    lo.listen(1)
    outbound["loopback_listen"] = "ok"
    lo.close()
check("no_outbound_connect", outbound and not any(v in ("CONNECTED", "RESOLVED") for v in outbound.values()),
      outbound or "not_attempted")

# 8 PID namespace
pids = sorted(int(p) for p in os.listdir("/proc") if p.isdigit())
host_pids = [int(x) for x in os.environ.get("A0_HOST_PIDS", "").split(",") if x]
check("pid_namespace_small", len(pids) < 10, len(pids))
check("host_provider_pids_invisible", not any(p in pids for p in host_pids),
      {"host_pids_checked": len(host_pids)})
try:
    env1 = open("/proc/1/environ", "rb").read()
    check("pid1_environ_empty", env1 == b"", len(env1))
except OSError as e:
    check("pid1_environ_empty", False, str(e))

# 9 filesystem write boundaries
def can_write(path):
    try:
        with open(path, "wb"):
            pass
        os.unlink(path)
        return True
    except OSError:
        return False

if not HOST:
    check("usr_readonly", not can_write("/usr/a0-probe"))
    check("etc_readonly", not can_write("/etc/a0-probe"))
    check("root_readonly", not can_write("/a0-probe-root"))
    check("harness_readonly", not can_write("/a0/harness/a0-probe"))
    try:
        open("/opt/a0/bin/claude", "r+b").close()
        check("provider_binary_readonly", False)
    except OSError:
        check("provider_binary_readonly", True)
    check("work_writable", can_write("/work/.a0-probe"))
    check("markers_writable", can_write("/markers/.a0-probe"))
    import hashlib
    h = hashlib.sha256()
    with open("/opt/a0/bin/claude", "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    check("provider_binary_digest", h.hexdigest() == os.environ.get("A0_EXPECTED_CLAUDE_SHA256"),
          h.hexdigest())

# 11 v2 resource limits (kernel-enforced): sized tmpfs mounts and inherited rlimits
if not HOST:
    sized = {}
    for line in open("/proc/self/mountinfo"):
        parts = line.split()
        mp = parts[4]
        if mp in ("/tmp", "/home/probe", "/work", "/markers"):
            sep = parts.index("-")
            fstype, opts = parts[sep + 1], parts[sep + 3] if len(parts) > sep + 3 else ""
            size = next((o.split("=", 1)[1] for o in opts.split(",") if o.startswith("size=")), None)
            sized[mp] = {"fstype": fstype, "size": size}
    want = {k: os.environ.get(f"A0_EXPECT_SIZE_{n}") for k, n in
            (("/tmp", "TMP"), ("/home/probe", "HOME"), ("/work", "WORK"), ("/markers", "MARKERS"))}
    check("v2_sized_tmpfs_mounts",
          all(sized.get(k, {}).get("fstype") == "tmpfs" and sized[k]["size"] == want[k] for k in want),
          {"mounts": sized, "expected_size_kib": want})
    writable_disk = []
    for line in open("/proc/self/mountinfo"):
        parts = line.split()
        sep = parts.index("-")
        fstype, opts = parts[sep + 1], parts[5].split(",")
        if "rw" in opts and fstype not in ("tmpfs", "proc", "devpts", "mqueue", "devtmpfs", "sysfs"):
            writable_disk.append({"mount": parts[4], "fstype": fstype})
    check("v2_no_writable_host_disk_bind", not writable_disk, writable_disk)
    import resource
    lim = {n: resource.getrlimit(getattr(resource, n)) for n in
           ("RLIMIT_FSIZE", "RLIMIT_CORE", "RLIMIT_NOFILE", "RLIMIT_CPU")}
    exp = json.loads(os.environ.get("A0_EXPECT_RLIMITS", "{}"))
    check("v2_rlimits_applied", bool(exp) and all(list(lim[k]) == v for k, v in exp.items()),
          {"actual": lim, "expected": exp})

# 10 mounts: nothing of the real home
mounts = [l.split()[4] for l in open("/proc/self/mountinfo")]
check("no_real_home_mount", not any(m.startswith(REAL_HOME) for m in mounts),
      sorted(set(mounts)) if not HOST else f"{len(mounts)} mounts")

json.dump({"mode": "host_control" if HOST else "sandbox",
           "all_pass": all(c["pass"] for c in checks.values()),
           "checks": checks}, sys.stdout, indent=1)
