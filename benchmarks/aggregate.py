#!/usr/bin/env python3
"""Reads the JSON lines the backend benchmarks wrote (`WYSS_BENCH_OUT`), one file per run, and
prints per metric the figures of each run and of all runs pooled. Nothing is removed or
adjusted: a run that is an outlier stays and shows. Standard library only; reads files it is given.
"""
import json
import statistics
import sys


def pooled(values):
    values = sorted(values)
    n = len(values)
    return {
        "n": n,
        "median": statistics.median(values),
        "min": values[0],
        "max": values[-1],
        "p95": values[min(n - 1, int(0.95 * n))],
    }


def main(paths):
    runs = {}
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            for line in handle:
                record = json.loads(line)
                runs.setdefault(record["metric"], []).append(record)
    for metric, records in sorted(runs.items()):
        print(f"== {metric} ({len(records)} runs) — {records[0]['boundary']}")
        for index, record in enumerate(records, 1):
            result = record["result"]
            if "mib_per_second" in result:
                s = result["mib_per_second"]
                print(f"  run {index}: {s['median']:.1f} MiB/s median "
                      f"(95% CI {s['median_ci95'][0]:.1f}–{s['median_ci95'][1]:.1f}, n={s['n']}), "
                      f"loadavg {record['conditions']['loadavg'].split()[0]}")
            elif "all" in result:
                s = result["all"]
                print(f"  run {index}: median {s['median']:.1f} µs, p95 {s['p95']:.1f}, p99 {s['p99']:.1f} "
                      f"(95% CI {s['median_ci95'][0]:.1f}–{s['median_ci95'][1]:.1f}, n={s['n']})")
            elif "kib_per_terminal_0_to_16" in result:
                s = result["kib_per_terminal_0_to_16"]
                print(f"  run {index}: {s['median']:.0f} kiB per idle terminal (backend share), "
                      f"sessions {[round(x) for x in [0]]}")
            else:
                print(f"  run {index}: {json.dumps(result)[:300]}")
        key = ("mib_per_second", "median") if "mib_per_second" in records[0]["result"] else None
        if key:
            print("  pooled medians of runs:", pooled([r["result"][key[0]][key[1]] for r in records]))
        elif "all" in records[0]["result"]:
            print("  pooled medians of runs (µs):", pooled([r["result"]["all"]["median"] for r in records]))
        elif "kib_per_terminal_0_to_16" in records[0]["result"]:
            print("  pooled (kiB):", pooled([r["result"]["kib_per_terminal_0_to_16"]["median"] for r in records]))


if __name__ == "__main__":
    main(sys.argv[1:])
