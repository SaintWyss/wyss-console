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

## M1 project workspace on the real desktop (M1-13)

`m1_gui.py` runs the built application on a nested display it makes for the run (a virtual KWin
compositor and Xwayland, each on a session bus of its own), with its own home, state and runtime
directory, a disposable terminal host and synthetic projects built by the ignored test
`projects::tests::perf_world` through the commands the window sends. It measures from outside: key
or click to the first frame that shows the answer (`ffmpeg` x11grab at 60 frames a second, so a
figure is quantised to about 17 ms and carries the capture's own latency), and the owned tree's
memory (RSS, PSS), processor time, threads, descriptors and wake-ups while idle with the view
shown and hidden. `m1_gui_runs.sh` makes three runs of each of three shapes (1 terminal and 2
workspaces; 16 terminals and 32 workspaces; 16 terminals in one workspace); `m1_gui_report.py`
reads the run files and prints the figures and the proposed budgets. Everything it starts is a
direct child launched through `owned.py` and adopted by descriptor (`adopt_tree`), and the script
reports, without touching them, any process that still carries the run's directory afterwards.

```sh
npx tauri build --no-bundle -- --locked              # the release binary, once
cargo test --locked --bin wyss-console --no-run      # the test executable that builds the world
benchmarks/m1_gui_runs.sh src-tauri/target/release/wyss-console \
  src-tauri/target/debug/deps/wyss_console-<hash> OUT_DIR
python3 -I benchmarks/m1_gui_report.py OUT_DIR
```

`m1_gui.py --shape none` starts the application with no projects and the release gate off (what an installed
build looks like before adoption); `--seed DIR` copies `layout.json`, `native/` and `configuration/trusted-writers` of
a backup into the run's own state, so the import can be rehearsed on a copy of real records (the originals are
only read).

## Historical A0 evidence

`test_owned.py` freezes six exact A0 source paths from baseline `43ed2fd` by
whole-file SHA-256. Their eight findings are child PID/group cleanup, namespace
cleanup, and a signal-zero liveness probe in the historical PTY drivers and their
negative controls. They are real uses of bare-PID APIs, not name-based process
selection and not false matches. Rewriting them would invalidate the recorded A0
evidence. This exception preserves that evidence; it does not certify those
drivers for new desktop tests. Use `Owned` for new harnesses.

No directory, basename, or signalling expression is exempt. Any byte change
revokes an exception, and the regression checks appended dangerous code and
changed paths. All other scripts, including new research scripts, remain scanned.
