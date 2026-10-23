#!/usr/bin/env python3
"""Reads the run files of `m1_gui.py` and prints, per shape, the figures of each run and of all runs
pooled, and the reference budgets that follow from them. Nothing is dropped: a failed sample
(`None`) is counted and shown, an outlier stays in the pooled figures.

    python3 -I benchmarks/m1_gui_report.py docs/acceptance/evidence/m1-perf-2026-10-23

The budgets are a proposal for the owner to agree or change, not an approval. The rule is stated
here so it can be argued with: a latency budget is one and a half times the worst p95 of any single
run, rounded up to 25 ms; a memory budget is one and a quarter times the largest proportional set
of any run, rounded up to 25 MiB; processor time and wake-ups are twice and one and a half times
the largest of any run. What the Projects view adds while shown is judged by the difference with it
hidden (the older view, the host and the shells are the same in both): twice the largest CPU
difference, at least 1 point, and one and a half times the largest wake-up difference, at least 100.
"""
import glob
import json
import math
import os
import statistics
import sys


def pct(values, q):
    values = sorted(values)
    return values[min(len(values) - 1, int(q * len(values)))] if values else None


def figures(values):
    ok = [v for v in values if v is not None]
    return {"n": len(values), "failed": len(values) - len(ok),
            "p50": pct(ok, 0.5), "p95": pct(ok, 0.95), "p99": pct(ok, 0.99),
            "max": max(ok) if ok else None}


def row(label, f, unit="ms"):
    if f["p50"] is None:
        return f"| {label} | {f['n']} | {f['failed']} | - | - | - | - |"
    return (f"| {label} | {f['n']} | {f['failed']} | {f['p50']:.0f} | {f['p95']:.0f} | "
            f"{f['p99']:.0f} | {f['max']:.0f} |")


def samples(run, key):
    block = run["events"].get(key)
    if not block:
        return None
    raw = block["samples"]
    return [s["ms"] if isinstance(s, dict) else s for s in raw]


def up(value, step):
    return math.ceil(value / step) * step


def main(root):
    shapes = {}
    for path in sorted(glob.glob(os.path.join(root, "shape-*", "run*.json"))):
        shape = os.path.basename(os.path.dirname(path))
        shapes.setdefault(shape, []).append(json.load(open(path)))
    out = []
    budgets = {}
    for shape, runs in sorted(shapes.items()):
        runs.sort(key=lambda r: r["run"])
        w = runs[0].get("world", {})
        out.append(f"### {shape}: {w.get('terminals')} terminals, {w.get('projects')} project(s)\n")
        out.append("Runs: " + ", ".join(
            f"#{r['run']} (loadavg {' '.join(r['loadavg'])}, {r['mem_available_mib']} MiB available, "
            f"leftovers {len(r.get('leftovers') or [])})" for r in runs) + "\n")
        budget = budgets.setdefault(shape, {})
        out.append("| latency (ms) | n | failed | p50 | p95 | p99 | max |")
        out.append("| --- | --- | --- | --- | --- | --- | --- |")
        for key, label in (("restore_ms", "restore (cold start of the Projects view)"),
                           ("switch_ms", "project switch"),
                           ("input_to_render_ms", "input to render (screen clear)"),
                           ("echo_to_render_ms", "input to render (one character)")):
            pooled = []
            worst = 0
            for run in runs:
                values = samples(run, key)
                if values is None:
                    continue
                f = figures(values)
                out.append(row(f"{label}, run {run['run']}", f))
                pooled.extend(values)
                if f["p95"] is not None:
                    worst = max(worst, f["p95"])
            if pooled:
                out.append(row(f"**{label}, all runs**", figures(pooled)))
                budget[key] = up(1.5 * worst, 25)
        out.append("")
        out.append("| idle state | run | processes | RSS MiB | PSS MiB | threads | fds | CPU % of a core | wake-ups/s |")
        out.append("| --- | --- | --- | --- | --- | --- | --- | --- | --- |")
        for state in ("console_only", "projects_shown", "projects_hidden"):
            pss_max = cpu_max = wake_max = 0
            for run in runs:
                s = run.get("idle", {}).get(state, {}).get("summary")
                if not s:
                    continue
                out.append(f"| {state} | {run['run']} | {s['processes']}{'' if s['stable_set'] else ' (set changed)'} | "
                           f"{s['rss_mib']:.0f} | {s['pss_mib']:.0f} | {s['threads']} | {s['fds']} | "
                           f"{s['cpu_percent_of_one_core']:.2f} | {s['wakeups_per_second']:.0f} |")
                pss_max = max(pss_max, s["pss_mib"])
                cpu_max = max(cpu_max, s["cpu_percent_of_one_core"])
                wake_max = max(wake_max, s["wakeups_per_second"])
            if pss_max:
                budget[state] = {"pss_mib": up(1.25 * pss_max, 25), "cpu": round(max(0.5, 2 * cpu_max), 1),
                                 "wakeups": up(1.5 * wake_max, 5)}
        out.append("")
        # What the Projects view adds: the same application, the same older view and host, with the
        # view shown against with it hidden (its panes unmounted), run by run.
        out.append("| added by the Projects view while shown (shown minus hidden) | run | PSS MiB | CPU % of a core | wake-ups/s |")
        out.append("| --- | --- | --- | --- | --- |")
        add_cpu = add_wake = 0
        for run in runs:
            a = run.get("idle", {}).get("projects_shown", {}).get("summary")
            b = run.get("idle", {}).get("projects_hidden", {}).get("summary")
            if a and b:
                d_cpu = a["cpu_percent_of_one_core"] - b["cpu_percent_of_one_core"]
                d_wake = a["wakeups_per_second"] - b["wakeups_per_second"]
                out.append(f"| | {run['run']} | {a['pss_mib'] - b['pss_mib']:+.0f} | {d_cpu:+.2f} | {d_wake:+.0f} |")
                add_cpu = max(add_cpu, d_cpu)
                add_wake = max(add_wake, d_wake)
        budget["added"] = {"cpu": round(max(1.0, 2 * add_cpu), 1), "wakeups": up(max(100, 1.5 * add_wake), 25)}
        out.append("")
    out.append("### Proposed reference budgets (not approved)\n")
    out.append("| shape | restore ms | switch ms | input ms (clear) | input ms (one char) | "
               "whole tree PSS MiB (shown) | the view adds, CPU % / wake-ups per second |")
    out.append("| --- | --- | --- | --- | --- | --- | --- |")
    for shape, b in sorted(budgets.items()):
        shown, added = b.get("projects_shown", {}), b.get("added", {})
        out.append(f"| {shape} | {b.get('restore_ms', '-')} | {b.get('switch_ms', '-')} | "
                   f"{b.get('input_to_render_ms', '-')} | {b.get('echo_to_render_ms', '-')} | "
                   f"{shown.get('pss_mib', '-')} | {added.get('cpu', '-')} / {added.get('wakeups', '-')} |")
    print("\n".join(out))


if __name__ == "__main__":
    main(sys.argv[1])
