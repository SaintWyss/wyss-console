"""Read-only snapshot of the real system that A0 must not affect (plan §2.4).

Hashes non-secret configuration, records only stat() of credential files (never opened),
the live Codex daemon identity and the set of PIDs running a provider binary.
Usage: python3 -I integrity.py <out.json>
"""
import hashlib
import json
import os
import sys

H = os.path.expanduser("~")
HASHED = [
    "~/.claude/settings.json", "~/.claude/settings.local.json", "~/.claude/AGENTS.md",
    "~/.codex/config.toml", "~/.codex/hooks.json", "~/.codex/AGENTS.md",
    "/etc/codex/managed_config.toml",
    "~/.codex/app-server-daemon/daemon.pid",
    "~/.local/share/claude/versions/2.1.312",
    "~/.codex/packages/standalone/releases/0.159.0-x86_64-unknown-linux-musl/bin/codex",
]
# Written continuously by the owner's running Claude sessions (including the one running this
# harness); recorded, judged separately, never read beyond its digest.
VOLATILE = ["~/.claude.json"]
STAT_ONLY = ["~/.claude/.credentials.json", "~/.codex/auth.json", "~/.claude/daemon/control.key"]
PROVIDER_EXES = {
    os.path.realpath(os.path.expanduser("~/.local/share/claude/versions/2.1.312")),
    os.path.realpath(os.path.expanduser(
        "~/.codex/packages/standalone/releases/0.159.0-x86_64-unknown-linux-musl/bin/codex")),
}


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def st(path):
    try:
        s = os.stat(path)
        return {"size": s.st_size, "mtime_ns": s.st_mtime_ns, "ino": s.st_ino, "mode": oct(s.st_mode)}
    except FileNotFoundError:
        return None


def provider_pids():
    out = []
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        try:
            exe = os.path.realpath(os.readlink(f"/proc/{p}/exe"))
        except OSError:
            continue
        if exe in PROVIDER_EXES:
            out.append({"pid": int(p), "exe": exe.replace(H, "~")})
    return sorted(out, key=lambda x: x["pid"])


def main(out):
    snap = {"hashed": {}, "volatile": {}, "stat_only": {}, "codex_daemon_sockets": {}}
    for p in HASHED:
        q = os.path.expanduser(p)
        snap["hashed"][p] = sha(q) if os.path.exists(q) else None
    for p in VOLATILE:
        q = os.path.expanduser(p)
        snap["volatile"][p] = {"sha256": sha(q) if os.path.exists(q) else None, "stat": st(q)}
    for p in STAT_ONLY:
        snap["stat_only"][p] = st(os.path.expanduser(p))
    d = "/tmp/codex-daemon-1000"
    if os.path.isdir(d):
        for n in sorted(os.listdir(d)):
            snap["codex_daemon_sockets"][n] = st(os.path.join(d, n))
    snap["provider_pids"] = provider_pids()
    with open(out, "w") as f:
        json.dump(snap, f, indent=1, sort_keys=True)


if __name__ == "__main__":
    main(sys.argv[1])
