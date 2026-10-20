# Installed application measurements

Run in the owner's normal desktop, HOME, XDG directories and wallet. Never start a private
user environment, close someone else's workload, drop caches or reconfigure the machine
for this script. Identify the exact app, host, authority and optional Ghostty PIDs first.

```sh
python3 benchmarks/idle.py --ui GUI_PID --host HOST_PID --authority AUTHORITY_PID \
  --reference GHOSTTY_PID --seconds 300 --out .artifacts/acceptance/idle-UNIQUE.json \
  --conditions 'Record visibility, idle terminal count, warm-up and competing workloads'
```

The sampler reads only process metadata and executable hashes. It changes no application
state. It keeps raw monotonic samples and refuses PID reuse; an unstable helper set makes
CPU unavailable. UI/WebKit, host, authority root and Ghostty root are separate. Shells and
native agents are not included in these root-only buckets. A fallback in-process host
cannot be measured as a distinct host by this procedure.

The report is **one observation**, not canonical P1 acceptance: M03/M04 need verified
visibility and three sessions of repeated five-minute windows; M05 needs two minutes idle;
M18 needs the full authority/runtime boundary. Independent-session medians, p95 and
confidence intervals, startup-to-drawn-prompt, input/render latency, throughput, panes and
reconnect measurements still need their own procedures (architecture 15).
