"""Declared resource limits and their enforcement (v2).

v1 defects (independent review): the declared 2 GiB storage cap was not implemented; cgroup
namespace was presented as if it bounded resources; leftover detection was name/path based.

Enforcement layers, from strongest to weakest:
- kernel, per sandbox: HOME, project and /tmp are tmpfs mounts with `--size` caps;
  RLIMIT_FSIZE / RLIMIT_CORE / RLIMIT_NOFILE / RLIMIT_CPU set in the bwrap launcher (inherited).
- kernel, per sandbox: /markers is a size-capped tmpfs too; the sandbox has NO writable bind
  on the host disk. Evidence leaves only through the HMAC stream, read by the host with a hard
  byte cap (STREAM_CAP): the host writes at most what it read.
- host watchdog (legacy, only if a writable bind is passed): polled /markers size.
- host pre-flight, per case: the whole A0 root must be below ROOT_CAP before launching.
- leftovers: every process whose PID namespace equals the sandbox's (from bwrap
  --json-status-fd) is a member; members after exit are leftovers and are killed by namespace.
"""
import os
import resource
import signal
import time

MiB = 1024 * 1024
ROOT_CAP = 2 * 1024 * MiB
MARKERS_CAP = 256 * MiB  # host watchdog (legacy path; v2 cases use the tmpfs below)
MARKERS_TMPFS = 16 * MiB  # kernel cap on in-sandbox evidence
HOME_TMPFS = 256 * MiB
WORK_TMPFS = 128 * MiB
TMP_TMPFS = 256 * MiB
FSIZE = 64 * MiB
NOFILE = 4096
CPU_S = 600
STREAM_CAP = 48 * MiB  # > base64(MARKERS_TMPFS) + records


def require_ptrace_restriction():
    """The provider must not be allowed to read its harness parent's HMAC key from memory.

    This is an admission check, not a change to the host's sysctl. Unavailable is unsafe.
    """
    try:
        with open("/proc/sys/kernel/yama/ptrace_scope") as f:
            scope = int(f.read().strip())
    except (OSError, ValueError) as e:
        raise RuntimeError("ptrace_restriction_unavailable") from e
    if scope not in (1, 2, 3):
        raise RuntimeError("ptrace_restriction_required")


def preexec(fsize=FSIZE, nofile=NOFILE, cpu=CPU_S):
    """Return a preexec_fn applying the rlimits in the launcher child before exec(bwrap)."""
    def apply():
        resource.setrlimit(resource.RLIMIT_FSIZE, (fsize, fsize))
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        resource.setrlimit(resource.RLIMIT_NOFILE, (nofile, nofile))
        resource.setrlimit(resource.RLIMIT_CPU, (cpu, cpu))
    return apply


def du(path):
    total = 0
    for d, dirs, files in os.walk(path, onerror=lambda e: None):
        for n in files:
            try:
                total += os.lstat(os.path.join(d, n)).st_blocks * 512
            except OSError:
                pass
    return total


def ns_members(pid_ns_inode):
    """PIDs (host view) whose PID namespace is the sandbox's. Read-only /proc inspection."""
    want = f"pid:[{pid_ns_inode}]"
    out = []
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        try:
            if os.readlink(f"/proc/{p}/ns/pid") == want:
                out.append(int(p))
        except OSError:
            continue
    return sorted(out)


def kill_namespace(pid_ns_inode, init_pid):
    """Kill the namespace through its init (PID 1 inside): the kernel then kills every member.
    Only signals a PID verified to be in the sandbox's namespace at the moment of signalling."""
    members = ns_members(pid_ns_inode)
    if init_pid in members:
        try:
            os.kill(init_pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    end = time.monotonic() + 5
    while time.monotonic() < end and ns_members(pid_ns_inode):
        time.sleep(0.05)
    return ns_members(pid_ns_inode)
