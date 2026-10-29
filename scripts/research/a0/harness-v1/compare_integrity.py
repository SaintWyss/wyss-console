"""Compare two integrity snapshots. Usage: python3 -I compare_integrity.py before.json after.json
Exit 0 = no harness-attributable change; 1 = stop condition S1/S7.
Volatile entries (written by the owner's running Claude sessions) and provider PIDs are
reported, not judged here: PID changes are compared against the sandbox's own PID namespace.
"""
import json
import sys

b, a = (json.load(open(p)) for p in sys.argv[1:3])
bad = []
for sec in ("hashed", "stat_only", "codex_daemon_sockets"):
    for k in sorted(set(b[sec]) | set(a[sec])):
        if b[sec].get(k) != a[sec].get(k):
            bad.append(f"{sec}:{k}")
vol = {k: (b["volatile"][k]["sha256"] == a["volatile"][k]["sha256"]) for k in b["volatile"]}
bp = {p["pid"] for p in b["provider_pids"]}
ap = {p["pid"] for p in a["provider_pids"]}
out = {"stop_changes": bad, "volatile_unchanged": vol,
       "provider_pids_before": sorted(bp), "provider_pids_after": sorted(ap),
       "new_provider_pids": sorted(ap - bp), "gone_provider_pids": sorted(bp - ap),
       "verdict": "PASS" if not bad and not (ap - bp) and not (bp - ap) else "CHECK"}
print(json.dumps(out, indent=1))
sys.exit(0 if out["verdict"] == "PASS" else 1)
