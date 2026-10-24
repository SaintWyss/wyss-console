#!/usr/bin/env python3
"""Repeated, comparable measurements of the Claude Managed stack (offline, mock provider).

    python3 -I benchmarks/m3_perf.py --sdk-root <verified venv> --runs 7 --out <dir>

Runs `real_tests::perf` (see src-tauri/src/f8/claude/real_tests/perf.rs), keeps every raw sample and
writes a report with min / p50 / p95 / max per figure. With n runs the p99 is the maximum, so it is
reported as that. The "proposed budget" column is 1.5 x p95, rounded up, and is only a proposal for
the owner to accept or change; nothing in the build enforces it.
"""
import argparse
import json
import math
import os
import platform
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)


def pct(values, q):
    values = sorted(values)
    return values[min(len(values) - 1, math.ceil(q * len(values)) - 1)]


def get(d, path):
    for k in path.split("."):
        d = d[k]
    return d


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sdk-root", required=True)
    ap.add_argument("--runs", type=int, default=7)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    env = dict(os.environ, WYSS_CLAUDE_SDK_ROOT=os.path.realpath(args.sdk_root), WYSS_PERF_RUNS=str(args.runs))
    run = subprocess.run(
        ["node", "scripts/tool.mjs", "cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "--lib",
         "real_tests::perf", "--", "--ignored", "--nocapture"],
        cwd=REPO, env=env, capture_output=True, text=True, timeout=1800)
    samples = [json.loads(l.split("PERF ", 1)[1]) for l in run.stderr.splitlines() if l.startswith("PERF ")]
    if run.returncode != 0 or not samples:
        print(run.stderr[-2000:], file=sys.stderr)
        return 1
    os.makedirs(args.out, exist_ok=True)
    with open(os.path.join(args.out, "raw.jsonl"), "w") as f:
        for s in samples:
            f.write(json.dumps(s) + "\n")
    cpu = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), "?")
    mem = next((l.split(":", 1)[1].strip() for l in open("/proc/meminfo") if l.startswith("MemTotal")), "?")
    figures = [
        ("ready_ms", "ms", "launch to runtime ready (cold: bridge, SDK import, CLI start)"),
        ("bind_ms", "ms", "bind a conversation"),
        ("first_content_ms", "ms", "turn start to first streamed text"),
        ("turn_ms", "ms", "turn start to completed (short answer)"),
        ("stop_ms", "ms", "stop request to whole tree reaped"),
        ("at_ready.pss_kib", "KiB", "process-tree PSS when ready"),
        ("after_turn.pss_kib", "KiB", "process-tree PSS after a turn"),
        ("at_ready.rss_kib", "KiB", "process-tree RSS when ready"),
        ("at_ready.processes", "", "processes in the tree"),
        ("at_ready.threads", "", "threads in the tree"),
        ("at_ready.fds", "", "open file descriptors in the tree"),
        ("idle.cpu_percent", "%", "CPU of the tree while idle (5 s)"),
        ("idle.context_switches_per_s", "/s", "context switches of the tree while idle"),
        ("left_after_stop", "", "processes left after stop"),
    ]
    report = {"runs": args.runs, "host": {"cpu": cpu, "mem_total": mem, "kernel": platform.release(),
              "loadavg_at_end": open("/proc/loadavg").read().split()[:3]}, "profiles": {}}
    lines = [f"# M3 performance — Claude Managed, offline mock provider\n",
             f"{args.runs} runs per profile on `{cpu}` ({mem}), kernel {platform.release()}.",
             "Fixture workload: one short scripted answer; no network, no real provider, no credential.",
             "p99 equals the maximum with this many runs. *Proposed* = 1.5 × p95 rounded up: a proposal for the",
             "owner, not an enforced budget.\n"]
    for profile in sorted({s["profile"] for s in samples if "profile" in s}):
        rows = [s for s in samples if s.get("profile") == profile]
        report["profiles"][profile] = {}
        lines += [f"## {profile} (n={len(rows)})\n", "| figure | min | p50 | p95 | max | proposed |", "|---|---|---|---|---|---|"]
        for path, unit, label in figures:
            v = [get(r, path) for r in rows]
            st = {"min": min(v), "p50": pct(v, .5), "p95": pct(v, .95), "max": max(v)}
            budget = math.ceil(st["p95"] * 1.5) if st["p95"] else 0
            report["profiles"][profile][path] = {**st, "proposed_budget": budget, "unit": unit, "samples": v}
            lines.append(f"| {label} | {st['min']} | {st['p50']} | {st['p95']} | {st['max']} | {budget} {unit} |")
        by = {}
        for r in rows:
            for name, val in r["idle"]["cpu_percent_by_process"].items():
                by.setdefault(name, []).append(val)
        lines += ["", "Idle CPU by process (p50 %): " + ", ".join(f"`{n}` {pct(v, .5)}" for n, v in sorted(by.items())), ""]
    cancel = [s["cancel_ms"] for s in samples if "cancel_ms" in s]
    report["cancel_ms"] = {"min": min(cancel), "p50": pct(cancel, .5), "p95": pct(cancel, .95), "max": max(cancel), "samples": cancel}
    lines += [f"## Cancel while streaming (n={len(cancel)})\n",
              f"request to the interruption notice: min {min(cancel)} · p50 {pct(cancel, .5)} · p95 {pct(cancel, .95)} · max {max(cancel)} ms\n",
              "## Not measured here\n",
              "- Codex on the same workload: needs a Codex runtime started on this machine, which this pass did not do",
              "  (no provider credential or configuration was read or created). The same harness shape applies; the",
              "  comparison stays PROVIDER_EVIDENCE_REQUIRED.",
              "- Real provider latency, quota and entitlement."]
    with open(os.path.join(args.out, "report.md"), "w") as f:
        f.write("\n".join(lines) + "\n")
    with open(os.path.join(args.out, "report.json"), "w") as f:
        json.dump(report, f, indent=1)
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
