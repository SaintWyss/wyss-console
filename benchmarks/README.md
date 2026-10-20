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

## Backend measurements (in-process, release)

`src-tauri/src/terminal/bench_tests.rs` measures the terminal **backend** (M06, M08, M09, M10, M14)
with the real PTY and service, with no window, WebKit or renderer. All `#[ignore]`; run by hand:

```sh
WYSS_BENCH_OUT=out.json cargo test --release --lib terminal::bench_tests -- \
  --ignored --nocapture --test-threads=1
python3 -I benchmarks/aggregate.py out-run1.json out-run2.json out-run3.json
```

They generate the corpora of architecture 15 §11.2 from a fixed seed in a private temporary file
that they remove by its exact path, and record the machine's state (load, memory, swap, power).
Nothing is dropped from a sample list. `threads.py` reads one process's threads (processor time and
wake-ups); `idle_ab.py` compares two builds of the GUI idle on an isolated display. `threads.py` starts and
ends nothing. `idle_ab.py` ends only the process it launched, through `owned.py` (a direct child held
by a pidfd; no process is ever found by name or path; see `docs/incidents/2026-10-20-benchmark-ended-live-session.md`).
`python3 -I benchmarks/test_owned.py` is the negative test of that rule and also fails a script that
uses `pkill`, `pgrep`, `killall` or a `kill` on a variable.
