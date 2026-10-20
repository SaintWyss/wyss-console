#!/usr/bin/env python3
"""P1 M03/M05/M18: read-only samples of the installed app in the real session.

Never starts/stops a process, changes HOME/XDG, drops caches or reads terminal output.
UI helpers and host are counted separately from the authority and shell/agent workloads.
One run is one observation, not the full three-session canonical acceptance matrix.
"""
import argparse
import hashlib
import json
import os
import platform
import statistics
import time
from pathlib import Path


def identity(pid):
    proc = Path(f"/proc/{pid}")
    if proc.stat().st_uid != os.getuid():
        raise ValueError("foreign process")
    fields = (proc / "stat").read_text().rsplit(")", 1)[1].split()
    return int(fields[19]), int(fields[11]) + int(fields[12])


def children(pid):
    found = set()
    for task in Path(f"/proc/{pid}/task").iterdir():
        try:
            found.update(int(p) for p in (task / "children").read_text().split())
        except FileNotFoundError:
            pass
    return found


def helpers(root, excluded):
    pending, found = [root], set()
    while pending:
        pid = pending.pop()
        if pid in found or pid in excluded:
            continue
        if len(found) >= 128:
            raise ValueError("process-tree bound exceeded")
        found.add(pid)
        # The desktop's children are its WebKit/GTK helpers. PTYs belong to the
        # independent host, which is not recursively counted below.
        pending.extend(children(pid))
    return found


def sample(pid):
    start, ticks = identity(pid)
    memory = {}
    for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
        key, _, value = line.partition(":")
        if key in {"Pss", "Rss", "Swap", "SwapPss"}:
            memory[key.lower() + "_kib"] = int(value.split()[0])
    if "pss_kib" not in memory:
        raise ValueError("PSS unavailable")
    return {"pid": pid, "start_ticks": start, "cpu_ticks": ticks, **memory}


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int((len(ordered) - 1) * fraction + .5))]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ui", type=int, required=True)
    parser.add_argument("--host", type=int, required=True)
    parser.add_argument("--authority", type=int)
    parser.add_argument("--reference", type=int, help="Ghostty root PID; its shell is excluded")
    parser.add_argument("--seconds", type=int, default=300)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--conditions", required=True)
    args = parser.parse_args()
    if not 1 <= args.seconds <= 3600:
        parser.error("seconds must be between 1 and 3600")
    roots = {"ui": args.ui, "host": args.host}
    if args.authority:
        roots["authority"] = args.authority
    if args.reference:
        roots["reference"] = args.reference
    if len(set(roots.values())) != len(roots):
        parser.error("bucket roots must be distinct")
    expected = {name: identity(pid)[0] for name, pid in roots.items()}
    executables = {
        name: {"path": os.readlink(f"/proc/{pid}/exe"),
               "sha256": hashlib.sha256(Path(f"/proc/{pid}/exe").read_bytes()).hexdigest()}
        for name, pid in roots.items()
    }
    rows = []
    started = time.monotonic()
    for index in range(args.seconds + 1):
        for name, pid in roots.items():
            if identity(pid)[0] != expected[name]:
                raise ValueError(f"{name} PID changed; run invalid")
        buckets = {
            "ui_and_helpers": [sample(p) for p in sorted(helpers(args.ui, set(roots.values()) - {args.ui}))],
            "host": [sample(args.host)],
        }
        if args.authority:
            buckets["authority_root_only"] = [sample(args.authority)]
        if args.reference:
            buckets["reference_root_only"] = [sample(args.reference)]
        rows.append({"seconds": time.monotonic() - started, "buckets": buckets})
        if index < args.seconds:
            time.sleep(max(0, started + index + 1 - time.monotonic()))
    summary = {}
    elapsed = rows[-1]["seconds"] - rows[0]["seconds"]
    for name in rows[0]["buckets"]:
        first = {p["pid"]: p for p in rows[0]["buckets"][name]}
        last = {p["pid"]: p for p in rows[-1]["buckets"][name]}
        stable = all(
            {p["pid"]: p["start_ticks"] for p in row["buckets"][name]}
            == {pid: p["start_ticks"] for pid, p in first.items()}
            for row in rows
        )
        pss = [sum(p["pss_kib"] for p in row["buckets"][name]) for row in rows]
        ticks = sum(last[p]["cpu_ticks"] - first[p]["cpu_ticks"] for p in first) if stable else None
        summary[name] = {
            "stable_process_set": stable,
            "cpu_percent_one_core": 100 * ticks / os.sysconf("SC_CLK_TCK") / elapsed if stable else None,
            "pss_median_kib": statistics.median(pss),
            "pss_p95_kib": percentile(pss, .95),
            "pss_max_kib": max(pss),
            "rss_median_kib": statistics.median(sum(p["rss_kib"] for p in row["buckets"][name]) for row in rows),
            "swap_median_kib": statistics.median(sum(p["swap_kib"] for p in row["buckets"][name]) for row in rows),
            "swap_pss_median_kib": statistics.median(sum(p.get("swappss_kib", 0) for p in row["buckets"][name]) for row in rows)
                if all("swappss_kib" in p for row in rows for p in row["buckets"][name]) else None,
        }
    report = {
        "schema_version": 1,
        "metrics": ["M03_single_observation", "M05_samples"] + (["M18_authority_root_only"] if args.authority else []),
        "machine": platform.node(), "kernel": platform.release(), "executables": executables,
        "conditions": args.conditions, "duration_seconds": elapsed,
        "canonical_matrix_complete": False, "ghostty_comparison": "root_process_only" if args.reference else "NOT_RUN",
        "summary": summary, "samples": rows,
    }
    # O_EXCL: a new observation must never overwrite prior raw evidence.
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as output:
        json.dump(report, output, indent=2)
        output.write("\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
