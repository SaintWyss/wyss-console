#!/usr/bin/env python3
"""P1 idle cost by thread: processor time and wake-ups per thread of ONE process over a window.

Read-only (`/proc/<pid>/task/*/stat` and `status`). `wakeups/s` is voluntary context switches per
second: how often the thread went to sleep and was woken. An idle thread that polls shows its poll
rate here; one that waits shows almost none. Never starts or ends anything.

    python3 -I benchmarks/threads.py PID SECONDS
"""
import os, sys, time
pid = int(sys.argv[1]); secs = float(sys.argv[2])
def snap():
    out = {}
    for t in os.listdir(f"/proc/{pid}/task"):
        try:
            stat = open(f"/proc/{pid}/task/{t}/stat").read().rsplit(")",1)
            name = stat[0].split("(",1)[1]
            f = stat[1].split()
            ticks = int(f[11]) + int(f[12])
            st = dict(l.split(":\t",1) for l in open(f"/proc/{pid}/task/{t}/status").read().splitlines() if ":\t" in l)
            out[t] = (name, ticks, int(st["voluntary_ctxt_switches"]), int(st["nonvoluntary_ctxt_switches"]))
        except FileNotFoundError:
            pass
    return out
a = snap(); t0 = time.monotonic(); time.sleep(secs); b = snap(); dt = time.monotonic() - t0
hz = os.sysconf("SC_CLK_TCK")
rows = []
for t, (name, ticks, v, nv) in b.items():
    if t in a:
        rows.append((name, t, (ticks - a[t][1]) / hz / dt * 100, (v - a[t][2]) / dt, (nv - a[t][3]) / dt))
for name, t, cpu, wake, nonv in sorted(rows, key=lambda r: -r[2]):
    print(f"{name:<22} tid={t:<8} cpu={cpu:6.3f}%  wakeups/s={wake:7.1f}  involuntary/s={nonv:6.1f}")
print(f"total cpu = {sum(r[2] for r in rows):.3f}% of one core, wakeups/s = {sum(r[3] for r in rows):.1f}, window {dt:.1f}s")
