# 15 — Performance engineering

> **2026-10-22 consolidation:** this document retains dated design/evidence and detailed
> contracts. Its present-tense status/decision tables describe their stated baseline,
> not necessarily current implementation. Read the [current product set](../product/README.md),
> [verified gaps](../product/implementation-status.md) and [requirement crosswalk](../product/traceability.md)
> first. F8 is active P2/V8, ADR0051 Accepted, cap1; P1 remains available. New product
> direction does not supersede accepted security ADRs or turn proposed tests into PASS.

> Current evidence (2026-10-19): `../acceptance/p1-2026-10-19.md` records two partial
> measurements of the real installed application, including Ghostty already present on
> the machine. `benchmarks/idle.py` exists. Historical “none measured” statements below
> describe the planning baseline; canonical P1 coverage/budgets remain unverified.
> Follow the real-environment testing policy in `../CHECKPOINT.md`.

Domain Q of [04](04-TARGET-ARCHITECTURE.md). What is measured, where its boundary is, how it is
measured, against what provisional target, what the design does when a target cannot be met, and
which parts of the existing design are likely to cost the most. **No measurement of Wyss Console
exists** `[R]`: no benchmark is in the repository and none was run for this set. Every number below
is a provisional target or a figure **computed from the code**, never a result.

**Revision of 2026-10-13 (performance pass).** The metric catalogue was extended from M19 to M33,
with M20 to M24 taken over from [05 §18](05-TERMINAL-AND-NATIVE-SESSIONS.md); every metric now has
a boundary, a workload, a procedure, an aggregation, a provisional target and a regression
threshold. Added: cost boundaries (§1.2), requirements by area (§9), degradation under load (§10),
risks found in the existing code (§6), the benchmark specification (§11). By the author of the set;
**unreviewed**.

## 1. Position

The owner named Ghostty as the reference for speed, lightness and comfort. Ghostty is a reference
to measure against, not a dependency and not a claim. Wyss Console is a web-view application
(Tauri 2, WebKitGTK) drawing with xterm's DOM renderer `[R]` (ADR 0006); a native GPU terminal
starts from a different floor. Whether the gap matters to the owner is a question for numbers, and
no technology is changed before they exist (AD-17).

### 1.1 Three kinds of statement

| Mark  | Means here                                                                                                       |
| ----- | ---------------------------------------------------------------------------------------------------------------- |
| `[R]` | An implementation fact read in the code, with its file: a bound, a timer, a thread, a poll interval              |
| `[P]` | A design expectation or a provisional target of this set                                                         |
| `[C]` | A figure **computed** from `[R]` facts (for example wake-ups per second from a poll interval). Not a measurement |
| `[N]` | Not known; the procedure that would settle it is named                                                           |

Measured evidence would carry the machine, the build, the date and the run's file. None exists.

### 1.2 Whose cost it is

The product is judged on its own cost. The programs it runs are the user's workload and are
reported apart, never added to the Console's budget and never used to excuse it.

| Bucket | What                                                                                                         | Counted in the Console's budget           | Identified by                                                                  |
| ------ | ------------------------------------------------------------------------------------------------------------ | ----------------------------------------- | ------------------------------------------------------------------------------ |
| B1     | The desktop process: Rust backend, GTK main loop, terminal workers, services' threads                        | Yes                                       | The PID the benchmark started                                                  |
| B2     | The web view's helper processes (WebKitGTK's web process, network process and any other it spawns)           | Yes                                       | Children of B1 whose executable is WebKitGTK's `[N]` for the exact set (OQ-47) |
| B3     | The authority, `wyss-control`, and its managed runtime                                                       | Reported separately (M18); its own budget | Its systemd user unit's cgroup and process tree                                |
| B4     | Shells started in terminals                                                                                  | No; reported as the floor of the workload | Children of B1's PTYs                                                          |
| B5     | Native agents' process trees: Claude Code, Codex, OpenCode, Hermes, and what they start (MCP servers, tools) | No; reported per agent (M07)              | The session's child and its descendants (05 §6)                                |
| B6     | Local Tools sandboxes of the managed plane                                                                   | No; part of B3's report                   | B3's tree                                                                      |

Every report gives B1 + B2 as "Wyss Console", B3 alone, and B4, B5 apart. A metric that cannot
separate them says so and is not compared against a Console budget.

### 1.3 Principles

| #   | Principle                                                                                                                                            |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| PP1 | Nothing runs at rest that does not have to: no timer, watch or poll without a stated rest cost (§8)                                                  |
| PP2 | The first prompt depends on nothing but the terminal: no configuration, vault, registry, history or authority work on its critical path (§9, area 1) |
| PP3 | Hidden views do no view work; hidden terminals keep reading output, because a program must not block (05 §18)                                        |
| PP4 | Every cost scales with what is shown or active, not with what exists                                                                                 |
| PP5 | **No optimisation weakens authorisation, audit, persistence or data integrity** (§8): no skipped flush, check, verification, confirmation or record  |
| PP6 | Under load the product degrades in a declared way and never loses terminal output, user data or a security check (§10)                               |
| PP7 | Measure before changing; change one thing at a time; keep the baseline                                                                               |

## 2. What is measured

Requirements are those of 02 §2.1. "Console" is buckets B1 + B2. Workloads and corpora are §11.2.

| ID  | Metric                       | Boundary: what is counted, from when to when                                                                                                                                             | Workload                                                                             | Requirement     |
| --- | ---------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | --------------- |
| M01 | Cold start                   | From `exec` of the packaged binary to the first shell prompt drawn (a marker printed by the shell reaches the screen); page cache dropped                                                | No layout file; one terminal; authority running; a variant with the authority absent | NFR-PERF-01     |
| M02 | Warm start                   | The same, third consecutive run, page cache warm                                                                                                                                         | As M01                                                                               | NFR-PERF-02     |
| M03 | Idle processor, visible      | Console processor time over five minutes divided by wall time, one terminal at a prompt, window visible, no input                                                                        | One idle shell; cursor not blinking (the default `[R]`, `settings/model.rs:106`)     | NFR-PERF-03     |
| M04 | Idle processor, hidden       | The same with the window minimised                                                                                                                                                       | As M03                                                                               | NFR-PERF-03     |
| M05 | Idle memory                  | Sum of proportional set size of every Console process after two minutes idle                                                                                                             | As M03                                                                               | NFR-PERF-04     |
| M06 | Memory per terminal          | Increase of M05 per added terminal, with empty scrollback and with 5,000 lines                                                                                                           | 1 to 16 idle terminals                                                               | NFR-PERF-05     |
| M07 | Memory per native agent      | B5 of one agent: its process tree's proportional set size after start and after one hour idle. **The provider's cost, never the Console's**                                              | A fake agent (BC-08); real agents only with authorisation                            | FR-NAT-07       |
| M08 | Input latency                | From the key event to the frame on which the echoed glyph is drawn, at an idle prompt                                                                                                    | Typing at 10 keys per second in bash with no prompt hooks                            | NFR-PERF-06     |
| M09 | PTY throughput               | Time from starting to print BC-01 to the shell prompt drawn after it; bytes per second                                                                                                   | `cat` of BC-01                                                                       | NFR-PERF-07     |
| M10 | Render throughput            | The same with BC-02, BC-03, BC-04 and BC-05                                                                                                                                              | `cat` of each                                                                        | NFR-PERF-07     |
| M11 | Responsiveness under load    | Longest interval in which the page handled no input event while M09 runs                                                                                                                 | A synthetic input source at 50 events per second during M09                          | NFR-PERF-07, 10 |
| M12 | Scrollback memory            | M05 with one terminal holding 5,000, 50,000 and the maximum lines of BC-01                                                                                                               | Preference set per run                                                               | NFR-PERF-05     |
| M13 | Resize latency               | From the end of a window resize to the PTY at its final size and the screen redrawn                                                                                                      | Resize by fixed steps; a full-screen program running                                 | NFR-PERF-08     |
| M14 | Runaway output               | Memory and M11 while a program prints without end for sixty seconds                                                                                                                      | `yes`                                                                                | NFR-PERF-10     |
| M15 | Paste                        | Time to deliver 4 KiB and 64 KiB to the program; behaviour beyond 64 KiB                                                                                                                 | A program that counts received bytes                                                 | FR-TERM-07      |
| M16 | Watcher cost                 | M03 and M05 with the configuration watcher armed; time from a file's change to its change record; cost of one full rescan                                                                | BC-07 with 10, 100 and 1,000 files                                                   | NFR-PERF-09     |
| M17 | Many sessions                | M11 and M03 with 16 terminals, half printing BC-01 in a loop                                                                                                                             | 16 panes in 4 tabs                                                                   | NFR-PERF-11     |
| M18 | Authority at rest            | B3's processor and memory with no session                                                                                                                                                | Authority started, idle                                                              | NFR-PERF-03     |
| M19 | First inventory              | From the Configuration view being shown to the first provider's inventory drawn                                                                                                          | BC-07 and a large real-shaped set (counts only)                                      | 13 §6           |
| M20 | Divider drag                 | Resize requests per pane per second, and frames over 16.7 ms, while a divider is dragged for five seconds                                                                                | Two panes with full-screen programs                                                  | 05 §11.3        |
| M21 | Restore                      | M01 with a layout of 16 panes in 4 tabs, against M01 without one                                                                                                                         | BC-09                                                                                | 05 §15.3        |
| M22 | Search                       | From the search request to the match count drawn                                                                                                                                         | BC-06 in a 50,000-line scrollback                                                    | 05 §13          |
| M23 | Idle terminals               | Wake-ups per second of Console threads, IPC calls per second, and M03, with 16 idle terminals, hidden and shown                                                                          | 16 idle shells                                                                       | 05 §18          |
| M24 | Scrollback memory, all panes | M12 summed over 16 panes at 5,000 and at 50,000 lines                                                                                                                                    | 16 terminals filled with BC-01                                                       | 05 §18          |
| M25 | Start-up breakdown           | Timestamps of M01's stages: process start, GTK initialised, services constructed, window created, page loaded, application mounted, terminal created, first byte, first prompt drawn     | As M01                                                                               | NFR-PERF-01     |
| M26 | Agent launch overhead        | From "Start" in the launcher to the agent identified (05 §17.5), minus the same shell's start-up time measured by a hand-typed start                                                     | BC-08 with an empty profile                                                          | FR-NAT-01       |
| M27 | Supervision cost             | Processor time of one sampling round of 16 sessions' process trees, and rounds per second, with the Sessions view shown and hidden                                                       | 16 fake agents, each with 10 children                                                | FR-NAT-07       |
| M28 | Vault operation              | Seal and verify of a record; decryption for a restoration plan; excluding dialogs, consent and the custody's own prompt; custody fetch reported apart                                    | Records of 1 KiB, 64 KiB, 256 KiB with a **test** custody, never the owner's keyring | 27 §5           |
| M29 | Direct write                 | SM-WRITE from `preparing` to `applied`, excluding the settling time, which is reported as configured                                                                                     | BC-07 files of 4 KiB, 64 KiB and 4 MiB, on the fixture filesystem                    | 07 §5.4         |
| M30 | Start-up repair              | Work and time of the repair of 14 §4 at start, with full stores and 0, 1 and 64 open intents, and its effect on M01                                                                      | Fixture stores at their bounds                                                       | 14 §4, §9       |
| M31 | Long-run stability           | Slope per hour of Console proportional set size, open descriptors, threads and page timers over eight hours                                                                              | 16 terminals: 4 printing intermittently, 12 idle; tabs switched every minute         | NFR-PERF-04, 10 |
| M32 | Interface interaction        | Input to next frame for: switching views, opening the palette, one keystroke of palette filtering, the Sessions list of 50 rows, the Configuration view's first paint from its inventory | Fixture data at the page bounds (50 rows, 256 items, 2,000 palette entries)          | 13 §6           |
| M33 | Scrolling                    | Share of frames over 16.7 ms while scrolling 5,000 lines of scrollback by page keys and by wheel                                                                                         | BC-01 and BC-02 in scrollback                                                        | NFR-PERF-08     |

M20 to M24 were first written in 05 §18; this table is now their definition, with the same
budgets.

## 3. Procedures

Reproducible, scripted when the phase that runs them is authorised, each producing a file of raw
numbers and a summary (§11.4). They are specifications; **none has been run**, and no harness
exists.

### 3.1 Common protocol

| Element          | Rule                                                                                                                                                                                                                                   |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Build            | The packaged build of one commit, never a development build. The commit, the build flags and the WebKitGTK, GTK and kernel versions are recorded                                                                                       |
| Machine state    | On mains power; no other user application open; the replication tool's and the snapshot tool's state recorded (§5.2). Nothing on the machine is reconfigured for a benchmark without the owner's word                                  |
| Warm-up          | Per metric, §3.2. The first run after a build or a boot is discarded unless the metric is a cold one                                                                                                                                   |
| Repetitions      | Per metric, §3.2; at least three independent sessions of the Console for any metric that is not a start                                                                                                                                |
| Aggregation      | Median, 95th percentile, interquartile range and a 95 % confidence interval of the median by bootstrap (10,000 resamples). Raw samples are kept. No sample is removed; values beyond three interquartile ranges are flagged            |
| Timing source    | A monotonic clock. Page timestamps are taken with the page's high-resolution clock and related to the backend's by one recorded offset per run                                                                                         |
| Process sampling | `/proc/<pid>/stat` for processor time; `/proc/<pid>/smaps_rollup` for proportional set size; `/proc/<pid>/fd` and `/proc/<pid>/task` for descriptors and threads; cgroup `cpu.stat` and `memory.current` of B3's unit as a cross-check |
| Wake-ups         | Context switches per thread from `/proc/<pid>/task/*/status` (voluntary switches) over the window; a kernel tracer only if the owner allows it (§11.3)                                                                                 |
| Result           | The record of §11.4, per metric, per machine                                                                                                                                                                                           |

### 3.2 Per metric

| Metric             | Procedure                                                                                                                                                                                               | Warm-up, repetitions                                 | Aggregate                    |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | ---------------------------- |
| M01, M25           | Drop the page cache (an act that needs the owner's word and administrator rights; otherwise "first run after boot"); start the binary; the shell prints a marker; the timestamp hook records the stages | None; 30 runs, each after a fresh boot or cache drop | Median, p95                  |
| M02                | Start, close, three times; measure the third                                                                                                                                                            | Two discarded; 30 measured                           | Median, p95                  |
| M03, M04, M18      | Processor time of each bucket at the start and end of five minutes                                                                                                                                      | Two minutes after the prompt; 3 sessions × 3 windows | Mean of windows; max         |
| M05, M06, M12, M24 | Proportional set size of every Console process                                                                                                                                                          | Two minutes idle after filling; 3 sessions           | Median                       |
| M07                | As M05 for B5; at start and after one hour                                                                                                                                                              | One run per agent kind                               | Reported as measured         |
| M08                | Instrumented: key event timestamp in the page to the frame after the renderer wrote the echo. External: the method of OQ-27                                                                             | 100 keys discarded; 1,000 measured, 3 sessions       | Median, p95, p99             |
| M09, M10           | Time from the command's start to the prompt drawn after it; bytes over time                                                                                                                             | One discarded; 10 runs per corpus                    | Median                       |
| M11, M17           | Largest gap between handled synthetic input events during the load                                                                                                                                      | 3 runs                                               | Max, p99 of gaps             |
| M13, M20           | Window manager resize steps; `stty size` in the pane after the last step; resize requests counted at the backend                                                                                        | 20 resizes or 5 drags; 3 sessions                    | Median, p95; max rate        |
| M14                | `yes` for sixty seconds; M05 each second; M11                                                                                                                                                           | 3 runs                                               | Max memory; max gap          |
| M15                | Paste of 4 KiB, 64 KiB, 64 KiB + 1 byte; the program counts                                                                                                                                             | 10 each                                              | Median; correctness          |
| M16                | Arm the watcher on BC-07; rest five minutes; touch one file and time the change record; force a full rescan                                                                                             | 3 sessions × each tree size                          | Median, p95                  |
| M19, M32           | Input to next frame, from page timestamps                                                                                                                                                               | 5 discarded; 50 measured                             | Median, p95                  |
| M21                | M01 with BC-09 against without                                                                                                                                                                          | As M01                                               | Difference of medians        |
| M22                | Search for BC-06's markers                                                                                                                                                                              | 3 discarded; 30 measured                             | Median, p95                  |
| M23                | M03 and the wake-up and IPC counts with 16 idle terminals, window shown, then minimised                                                                                                                 | As M03                                               | Mean per second              |
| M26                | Launch from the sheet and by hand in the same shell, alternating                                                                                                                                        | 3 discarded; 20 pairs                                | Median of differences        |
| M27                | Processor time of the sampling thread per round                                                                                                                                                         | 60 rounds × 3 sessions                               | Median, p95                  |
| M28                | Each operation against the test custody; custody fetch timed apart                                                                                                                                      | 10 discarded; 100 measured per size                  | Median, p95                  |
| M29                | Each operation on the fixture filesystem with settling configured; settling reported, not counted                                                                                                       | 5 discarded; 50 per size                             | Median, p95                  |
| M30                | Start with fixture stores; time repair and M01                                                                                                                                                          | As M01                                               | Median                       |
| M31                | Eight hours; samples every minute                                                                                                                                                                       | First hour excluded from the slope                   | Least-squares slope per hour |
| M33                | Frame times from the page while scrolling                                                                                                                                                               | 3 sessions × 10 scrolls                              | Share of frames over 16.7 ms |

### 3.3 Comparison with Ghostty

The same procedures, on the same machine, the same session, the same font family and size, the
same window size, the same shell with the same start-up files, Ghostty's defaults otherwise,
its version recorded. Compared, as absolute numbers and as a ratio: M01, M02 (time to prompt),
M03, M04, M05, M06 (whole process tree, both terminals), M08 (external method only: the
instrumented method has no equivalent in Ghostty), M09, M10, M11 (by the external input source),
M12, M13, M14, M20 (Ghostty's own splits). Not compared, because Ghostty has no counterpart:
M15's bound, M16, M19, M21 to M23's IPC counts, M25 to M32. "Comparable to Ghostty" is never
written without the published comparison and its conditions.

## 4. Provisional budgets

Provisional. Each is restated after P1 as "no worse than the baseline" for what is already good
enough, and as a target with a deadline for what is not. They are set for **Acer**, the
constrained machine; Santiago-PC's numbers are reported beside them (§5). A budget marked
"placeholder" has no basis but a guess and says so. None is a promise to the user.

### 4.1 Hard limits already built `[R]`

Not targets: bounds the design relies on.

| Limit                               | Value                                           | Evidence                                |
| ----------------------------------- | ----------------------------------------------- | --------------------------------------- |
| Terminals per window                | 16, counting starting and exited-undrained ones | `terminal/contracts.rs`; ADR 0005, 0007 |
| Output held per terminal; per batch | 64 KiB; 16 KiB, one unacknowledged batch        | ADR 0005                                |
| Input queued in the page            | 64 KiB; 4 KiB per write                         | `controller.ts:14`; ADR 0006            |
| Scrollback per terminal             | 500 to 50,000 lines, default 5,000              | `settings/model.rs:12`, `:104`          |
| Resize coalescing                   | 60 ms; one in flight                            | ADR 0006                                |
| Managed turns at once               | One (C-04)                                      | ADR 0015                                |
| Native sessions at once             | At most 16, one per terminal                    | 05 §10.2, TI8                           |

### 4.2 Targets

| Metric | Provisional target (Acer)                                                                                                                                            | Basis                                                                                                               | Regression threshold                        |
| ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| M01    | ≤ 1,500 ms median. Placeholder                                                                                                                                       | A guess for a web-view start                                                                                        | +10 % or +2 IQR of the baseline, the larger |
| M02    | ≤ 700 ms median. Placeholder                                                                                                                                         | The same                                                                                                            | As M01                                      |
| M03    | ≤ 0.5 % of one core                                                                                                                                                  | A terminal at rest should do nothing; the cursor does not blink by default `[R]`                                    | +0.2 percentage points                      |
| M04    | ≤ 0.1 %                                                                                                                                                              | Hidden means no drawing                                                                                             | +0.05 points                                |
| M05    | Ceiling 300 MiB; set from the baseline                                                                                                                               | WebKitGTK's floor is unknown (OQ-47)                                                                                | +5 % or +5 MiB, the larger                  |
| M06    | ≤ 10 MiB empty; ≤ 25 MiB with 5,000 lines. Placeholder                                                                                                               | —                                                                                                                   | +10 %                                       |
| M07    | None: the provider's cost. Reported                                                                                                                                  | B5                                                                                                                  | —                                           |
| M08    | ≤ 20 ms median, ≤ 40 ms p95, instrumented                                                                                                                            | Two frames at 60 Hz as the median                                                                                   | +10 % of the median or +2 ms                |
| M09    | No floor until measured; reported                                                                                                                                    | Responsiveness matters more than the speed of a flood                                                               | −15 % throughput                            |
| M10    | Reported                                                                                                                                                             | —                                                                                                                   | −15 %                                       |
| M11    | ≤ 100 ms                                                                                                                                                             | A pause a user notices                                                                                              | +20 ms                                      |
| M12    | Reported; decides DN-40                                                                                                                                              | —                                                                                                                   | +10 %                                       |
| M13    | ≤ 100 ms after the last resize event                                                                                                                                 | The fit is coalesced at 60 ms `[R]`                                                                                 | +20 ms                                      |
| M14    | Memory bounded by the scrollback setting; M11 ≤ 100 ms                                                                                                               | Output held to 64 KiB until acknowledged `[R]`                                                                      | Any growth beyond the bound                 |
| M15    | 64 KiB delivered ≤ 250 ms; beyond the bound fails visibly with nothing sent                                                                                          | 16 writes of 4 KiB `[R]`                                                                                            | +20 %; any correctness failure              |
| M16    | No measurable difference in M03; one change record ≤ 500 ms; a full rescan of 100 files ≤ 50 ms                                                                      | NFR-PERF-09; 07 §7                                                                                                  | +20 %                                       |
| M17    | M11 ≤ 150 ms; M03 scales with what prints, not with what is open                                                                                                     | —                                                                                                                   | +20 ms                                      |
| M18    | ≤ 0.1 % of one core; memory reported                                                                                                                                 | —                                                                                                                   | +0.05 points                                |
| M19    | ≤ 200 ms on the fixture set                                                                                                                                          | 13 §6                                                                                                               | +20 %                                       |
| M20    | ≤ 1 resize per pane per 60 ms; M11 ≤ 100 ms                                                                                                                          | 05 §11.3                                                                                                            | Any excess rate                             |
| M21    | ≤ M01 + 10 %                                                                                                                                                         | Hidden tabs' shells start when shown (05 §15.3)                                                                     | As M01                                      |
| M22    | < 200 ms for 50,000 lines                                                                                                                                            | On the page's thread (05 §13)                                                                                       | +20 %                                       |
| M23    | M03 with 16 idle terminals ≤ 1 % of one core; wake-ups and IPC calls reported                                                                                        | §6, PR-01: the built design is computed to make about 50 IPC calls and 160 worker wake-ups per second at rest `[C]` | +20 %                                       |
| M24    | Reported; decides DN-40                                                                                                                                              | —                                                                                                                   | +10 %                                       |
| M25    | No stage of the configuration engine, the vault, the registry repair, the layout of hidden tabs or the authority link precedes "first prompt drawn"                  | PP2                                                                                                                 | Any such stage on the path                  |
| M26    | ≤ 100 ms beyond the shell's own start-up                                                                                                                             | One registry write, one profile resolution, one recipe (05 §17)                                                     | +20 %                                       |
| M27    | ≤ 5 ms of processor per round of 16 trees; at most one round per 2 s; none while hidden                                                                              | 13 S2                                                                                                               | +20 %; any round while hidden               |
| M28    | Seal and verify of 256 KiB ≤ 50 ms, custody excluded; custody fetch reported. A passphrase's key derivation is slow on purpose and is **not** a target               | Authenticated encryption of 256 KiB is far below a frame on current processors `[N]`                                | +20 %                                       |
| M29    | Reported; no target before the filesystem's flush cost is known. The settling time is a safety parameter (07 §5.4; OQ-31), never shortened for speed                 | Flushes on the owner's filesystem `[N]`                                                                             | +20 %                                       |
| M30    | Adds nothing to M01 (repair runs after the first prompt; §9, area 13); completes ≤ 100 ms with no open intent                                                        | 14 §4                                                                                                               | Any effect on M01                           |
| M31    | After the first hour: proportional set size ≤ 1 MiB per hour; descriptors, threads and timers flat                                                                   | Every store and buffer is bounded (14 §3; 05 §13)                                                                   | Any slope above the target                  |
| M32    | ≤ 100 ms for a view switch; ≤ 50 ms to open the palette; ≤ 16 ms per filtering keystroke; ≤ 100 ms for the Configuration view's first paint once its inventory is in | One frame for typing; a perceptible-pause limit for the rest                                                        | +20 %                                       |
| M33    | ≤ 5 % of frames over 16.7 ms                                                                                                                                         | —                                                                                                                   | +2 points                                   |

A regression is confirmed only by a second run on the same machine. A confirmed regression blocks
the acceptance of the phase that caused it, unless the owner accepts it in writing with its reason
(§11.5).

## 5. Reference machines

### 5.1 The two computers

| Name        | Description `[O]`                                                                     | Role                                                                      |
| ----------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Acer        | A 2020 four-core mobile processor, 16 GB, integrated and discrete graphics, 1920×1080 | The owner's daily machine; the constrained case; **budgets are set here** |
| Santiago-PC | A current eight-core desktop processor with a discrete GPU                            | The unconstrained case; reported beside Acer                              |

Both run the same distribution and desktop (KDE Plasma) and kernel 6.18.55 (30 §6) `[O]`. Results
are tied to the machine, the session type (X11 or Wayland), the compositor and the GPU path, and
are reported with them. A target met on Santiago-PC and missed on Acer is missed.

### 5.2 Confounding factors, recorded with every run

| Factor                                                     | Why it matters                                                          | What is done                                                                 |
| ---------------------------------------------------------- | ----------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Processor frequency governor, thermal state, power source  | Changes every latency and throughput figure                             | Recorded; mains power; a cool-down of two minutes between heavy runs         |
| The replication tool (scanning, transfers)                 | Reads disks and wakes regularly                                         | Its state recorded; pausing it is the owner's act, never assumed             |
| The snapshot tool (Santiago-PC takes one each hour, 30 §7) | Disk and processor activity at the hour                                 | No run spans the hour on Santiago-PC; the time of the last snapshot recorded |
| Compositor and session type                                | Frame pacing; whether the transparent window is composited (`glass.rs`) | Recorded; the window mode (`native`, `transparent`, `opaque`) recorded       |
| The web view's rendering path (GPU, DMA-BUF, software)     | Memory and frame times                                                  | Recorded from the web view's own report `[N]` (OQ-47)                        |
| The input method                                           | Adds work to every key                                                  | Runs with it on and off (the owner uses one `[O]`)                           |
| Fonts and font cache                                       | First draw time                                                         | The same font; cache state recorded                                          |
| Page cache                                                 | Start times                                                             | Cold and warm runs kept apart                                                |
| Other applications and the authority's state               | Background load                                                         | None open; authority state recorded                                          |

## 6. Risks found in the existing design

Read in the code on 2026-10-13. Each is a hypothesis about cost, not a finding of slowness: none was
measured.

| ID    | Finding and evidence `[R]`                                                                                                                                                                                                                                                                                         | Impact `[C]` or `[N]`                                                                                                                                                                                   | Alternatives                                                                                                                                                                                                                                                                                                                                                                                               | Trade-offs                                                                                                                                                                                               | Validation                                    |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| PR-01 | **Idle terminals are polled twice.** The page pulls output from every live terminal with an idle back-off of 10 ms doubling to 320 ms (`controller.ts`, `output()`, ADR 0006), hidden ones included (ADR 0007). Each terminal's backend worker wakes every 100 ms to observe child exit (`terminal/worker.rs:426`) | Per idle terminal about 3 IPC calls and 10 worker wake-ups per second; with 16, about 50 and 160 `[C]`. Its share of M03 is unknown. Typing is not delayed: input wakes the page's idle wait (ADR 0006) | (a) Keep. (b) A longer idle cap (1 to 2 s): fewer calls, but output from a background program after a quiet spell appears up to that late. (c) The backend notifies the page that a batch is ready; the page then pulls with the same acknowledgement protocol: no idle calls, credit unchanged. (d) The worker waits on a process descriptor for child exit instead of a 100 ms timeout: no idle wake-ups | (c) changes ADR 0006's transport and needs an ADR; it keeps every bound. (d) needs the platform's process-descriptor support through the libraries in use (OQ-48). (b) trades a latency the user can see | M23; DN-41; OQ-48                             |
| PR-02 | **Hidden terminals parse their output.** Hidden panes keep consuming output (ADR 0007), so the renderer parses it into its buffer even when nothing is drawn                                                                                                                                                       | Cost proportional to output, not to the number of hidden terminals; nothing at rest                                                                                                                     | (a) Keep: required, or programs block. (b) Defer parsing of hidden panes by buffering bytes: rejected, it moves memory into an unbounded queue or blocks the program                                                                                                                                                                                                                                       | Correctness first (PP3, PP6)                                                                                                                                                                             | M17                                           |
| PR-03 | **Every view is mounted at start.** `App.svelte` mounts the six views and constructs their controllers at start, shown or not; the bundle is built without code splitting (`vite.config.ts`)                                                                                                                       | Parse, compile and first layout of every view precede or compete with the first prompt `[N]`                                                                                                            | (a) Keep. (b) Mount the Console view first and the others on first show. (c) Split the bundle per view and load on first show                                                                                                                                                                                                                                                                              | (b) and (c) make the first visit to a view slower by its mount time (M32); (c) adds loading states                                                                                                       | M25, M01, M32                                 |
| PR-04 | **Services start before the window.** `desktop.rs` `run()` constructs the managed-session service, the authority link (one thread), the preferences (one file read) and the lifecycle follower (one thread), probes the display for transparency (`glass.rs`), then builds the window                              | The lifecycle follower installs or starts nothing unless the link reports no authority (`bootstrap.rs`); the link connects in its own thread. Likely small; unmeasured `[N]`                            | (a) Keep. (b) Construct the authority link and the lifecycle follower after the window is shown                                                                                                                                                                                                                                                                                                            | (b) delays the authority's state in the top bar by the same amount                                                                                                                                       | M25                                           |
| PR-05 | **Infinite animations while a state is pending.** `.pill[data-tone='pending'] .dot`, `.sessions-status .dot` and `.turn-activity.live .dot` pulse forever (`app.css:307`, `:666`, `:1166`), stopped only by reduced motion                                                                                         | While a pill stays pending (an authority that does not answer) the page repaints continuously `[N]`                                                                                                     | (a) Keep. (b) Pulse a few times, then a static indicator with its word. (c) Static indicators only                                                                                                                                                                                                                                                                                                         | The pulse tells the user something is in progress; its word already does (13 §1.1)                                                                                                                       | M03 with the authority absent                 |
| PR-06 | **The translucent window.** On KDE under X11 with a compositor the window is created transparent and asks KWin to blur what is behind it (`glass.rs`, `plan`; ADR 0025, checklist row 26)                                                                                                                          | Blurring behind a window is the compositor's work: its cost falls in KWin, outside buckets B1 and B2, and would be missed by a measurement of the Console's processes alone `[N]`                       | (a) Keep. (b) Opaque by preference. (c) Measure KWin's processor time beside the Console's in each window mode                                                                                                                                                                                                                                                                                             | A look already built `[R]`; (b) is the owner's choice                                                                                                                                                    | M03, M10, M33 in each mode, with KWin sampled |
| PR-07 | **The DOM renderer per pane.** xterm's DOM renderer (ADR 0006), one per terminal; panes multiply live renderers                                                                                                                                                                                                    | Render throughput and layout cost on resize grow with visible panes `[N]`                                                                                                                               | (a) Keep. (b) Another renderer of the same library: a new dependency (DN-04) with its own memory cost                                                                                                                                                                                                                                                                                                      | AD-17: no change before numbers                                                                                                                                                                          | M10, M13, M20, M33                            |
| PR-08 | **Scrollback has no global bound.** Each terminal's scrollback is its own preference, up to 50,000 lines `[R]`; 16 terminals at 50,000 are not bounded as a whole                                                                                                                                                  | Memory unknown; potentially the largest item of M05 `[N]`                                                                                                                                               | DN-40                                                                                                                                                                                                                                                                                                                                                                                                      | Trimming loses what the user may want to read; not trimming may exhaust memory                                                                                                                           | M12, M24                                      |
| PR-09 | **Long waits hold threads.** The authority view and the lifecycle view are read by long waits of 20 s (`desktop/authority.rs`, `desktop/lifecycle.rs`), each on a blocking thread, at most four at once                                                                                                            | One wake-up per 20 s per follower; two threads parked. Small `[C]`                                                                                                                                      | Keep                                                                                                                                                                                                                                                                                                                                                                                                       | —                                                                                                                                                                                                        | M23 counts them                               |
| PR-10 | **Reconnection without an authority.** The link retries with a back-off from 250 ms to 5 s (`authority/link.rs`)                                                                                                                                                                                                   | One attempt per 5 s at rest when no authority answers `[C]`                                                                                                                                             | Keep                                                                                                                                                                                                                                                                                                                                                                                                       | —                                                                                                                                                                                                        | M03 with the authority absent                 |
| PR-11 | **Supervision walks process trees.** Sampling reads `/proc` for every process under each session (05 §6); agents start interpreters, servers and tools                                                                                                                                                             | Proportional to the number of processes, only while the overview is visible `[P]`                                                                                                                       | (a) As designed. (b) Sample only the session's own process and its direct children                                                                                                                                                                                                                                                                                                                         | (b) under-reports what an agent costs (M07)                                                                                                                                                              | M27                                           |
| PR-12 | **Writes flush on a copy-on-write filesystem.** The write protocol flushes the intent, the staged file, both directories and the outcome (07 §5.4); the vault flushes records and the index                                                                                                                        | Flush latency on the owner's filesystem is unknown and may be the largest part of M29 `[N]`                                                                                                             | None that removes a flush: every one of them is a correctness step (PP5)                                                                                                                                                                                                                                                                                                                                   | Speed is not traded for integrity                                                                                                                                                                        | M29, M30                                      |

What the code does **not** show as a risk: the configuration engine, the vault and the native
registry do not exist yet, so nothing of them is on today's start-up path; their placement off it
is a design rule (§9) to be held by M25.

## 7. Optimisation order

Only after P1, and only where a number says so:

1. Anything that runs at rest and should not (timers, watches, polling): PR-01, PR-05.
2. Start-up: defer what the first prompt does not need: PR-03, PR-04.
3. Output path: batch size and acknowledgement cadence, within the existing bounds.
4. Renderer options inside xterm (its other renderers are add-ons and new dependencies: DN-04).
5. Only if 1 to 4 leave a gap the owner judges unacceptable: study a different terminal core,
   as a research phase with its own ADR, comparing at least the effect on the configuration
   and session views, on accessibility and on maintenance.

## 8. Guard rails

| Guard                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Phase   |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| The benchmark scripts live in the repository and run on demand                                                                                                                                                                                                                                                                                                                                                                                                       | P1      |
| Every phase's acceptance repeats M03, M05, M16 and M23 and compares with the baseline                                                                                                                                                                                                                                                                                                                                                                                | From F1 |
| A change that adds a timer, a watch, a poll or a thread states its rest cost and is measured by M23                                                                                                                                                                                                                                                                                                                                                                  | Always  |
| No view does work while hidden (13 §6)                                                                                                                                                                                                                                                                                                                                                                                                                               | Always  |
| **Never done to save time or memory**: skip or batch a flush the write protocol or the vault requires; skip the hash check, the read-back, the verification or the settling of 07 §5.4; keep the vault's key beyond an operation (27 §5.3); skip a native dialog, a confirmation, a journal entry or an audit record; drop terminal output or input; widen a bound; sample, scan or rescan while hidden; cache content of a configuration entry or of a vault record | Always  |

## 9. Requirements by area

What the design must hold, area by area, with its metrics. `[P]` throughout, `[R]` where marked.

| #   | Area                                                  | Requirement                                                                                                                                                                                                                                                                | Metrics            |
| --- | ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ |
| 1   | Cold and warm start                                   | The critical path is: process, GTK, window, page, Console view, one terminal, first prompt. The configuration engine, the vault (27 §5.2), the native registry's repair, the start-up repair of 14 §4, the layout of hidden tabs and every view but the Console are off it | M01, M02, M21, M25 |
| 2   | Processor at rest                                     | PP1. Visible and idle: no animation, timer or poll beyond those of §6 with a stated cost. Minimised: no drawing                                                                                                                                                            | M03, M04, M18, M23 |
| 3   | Memory                                                | Per process (B1, B2 apart) and per terminal; every buffer bounded (§4.1)                                                                                                                                                                                                   | M05, M06, M12, M24 |
| 4   | Several panes and sessions                            | Cost scales with what prints and what is shown; at most 16 terminals and 16 native sessions (§4.1)                                                                                                                                                                         | M17, M20, M23      |
| 5   | Hidden terminals and polling                          | Output keeps flowing under the credit protocol; no input; no fitting; the idle cost is PR-01's question                                                                                                                                                                    | M23                |
| 6   | Input latency and responsiveness under load           | Input is independent of output (ADR 0006); a flood never stops the page handling input for more than the M11 target                                                                                                                                                        | M08, M11, M17      |
| 7   | PTY throughput and flow control                       | The credit protocol of ADR 0005 and 0006 `[R]` is kept: the backend stops reading when its ring is full; nothing is dropped                                                                                                                                                | M09, M10, M14      |
| 8   | Search and scrollback                                 | Search is on demand, add-on loaded on first use, plain text, a match list capped at 1,000 (05 §13); scrollback bounded per terminal and, by DN-40, in total                                                                                                                | M12, M22, M24, M33 |
| 9   | Renderer and interface interactions                   | Lists paged (50) or windowed; the Configuration view from a bounded projection (256 items); no view work while hidden (13 §6)                                                                                                                                              | M13, M20, M32, M33 |
| 10  | Agent launch and supervision                          | One profile resolution and one registry write per launch; the foreground observation only on its occasions (05 §6.1); sampling only while shown                                                                                                                            | M26, M27           |
| 11  | Configuration inventory and watchers                  | Inventory on first show; watches on directories, events filtered by name before queueing; rescans bounded and never while hidden (07 §7.2)                                                                                                                                 | M16, M19           |
| 12  | Vault, encryption, restoration, transactional writing | The vault's code and key are touched only by an operation that needs them (27 §5.2); a direct write's latency is dominated by its flushes and its settling time, both correctness steps (PR-12)                                                                            | M28, M29           |
| 13  | Persistence, recovery, restart                        | Start-up repair runs after the first prompt; every source with an open intent is held at `repair_pending` until repair has finished, so no mutation is admitted early (14 §4). The layout is saved on change, not on a timer (05 §15.1)                                    | M21, M30           |
| 14  | Long-running stability                                | Every store and buffer has a bound; listeners, timers and IPC requests are released with their owner (ADR 0006, 0007 `[R]`); eight-hour stability                                                                                                                          | M31                |
| 15  | Both computers                                        | Budgets on Acer; Santiago-PC reported; each run with its confounders (§5)                                                                                                                                                                                                  | All                |

## 10. Degradation under load

What the product does when a resource runs short, and what it never gives up.

| ID    | Condition                                      | Behaviour                                                                                                       | Never sacrificed                                                      |
| ----- | ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| DG-01 | A program prints faster than the page can draw | Backpressure: the backend stops reading the PTY when its ring is full; the program blocks `[R]`                 | Terminal output: no byte dropped                                      |
| DG-02 | Many terminals print at once                   | Each is served in turn under its own credit; visible ones are drawn, hidden ones parsed                         | Input routing; output of any terminal                                 |
| DG-03 | The terminal limit is reached                  | New terminals, splits and launches refused with their sentence (C-06)                                           | Existing terminals                                                    |
| DG-04 | Input beyond 64 KiB queued                     | The terminal fails visibly and closes `[R]`                                                                     | No input silently dropped or reordered                                |
| DG-05 | The watcher overflows or hits its bounds       | Degraded mode: rescans on a period, every state shown with its age (07 §7.2)                                    | No stale state shown as current; mutations still re-read their target |
| DG-06 | The keyring is slow or locked                  | Operations that need the vault wait or are refused; everything else goes on (27 §5.7)                           | No fallback custody; no key cached longer                             |
| DG-07 | The disk is full or slow                       | Mutations refused (`journal_unavailable`, `storage_failed`); a layout save failure is said once                 | No partial file; no record removed to make room                       |
| DG-08 | Scrollback memory grows large                  | DN-40                                                                                                           | Nothing trimmed without the policy the owner chose                    |
| DG-09 | Supervision of many process trees              | Sampling only while shown, at most one round per 2 s; a round that overruns its budget is skipped, never queued | The registry's states, which come from reaping, not sampling          |
| DG-10 | Search over a large scrollback                 | Plain text only; matches capped; "out of date" instead of re-searching on every batch                           | —                                                                     |
| DG-11 | The authority does not answer                  | Back-off up to 5 s `[R]`; the managed group says so; terminals unaffected                                       | No managed action shown as done                                       |

## 11. Benchmark specification

For P1 and every later phase's performance check. Specified, **not written and not run**: no
harness, script or fixture is added by this document.

### 11.1 Suites

| Suite     | Metrics                 | Needs                                                         |
| --------- | ----------------------- | ------------------------------------------------------------- |
| BS-START  | M01, M02, M21, M25, M30 | The timestamp hook (OQ-26); BC-09; fixture stores             |
| BS-IDLE   | M03, M04, M18, M23      | Process sampling                                              |
| BS-MEM    | M05, M06, M12, M24, M31 | Process sampling; BC-01                                       |
| BS-INPUT  | M08, M11, M15           | The external latency method (OQ-27); a synthetic input source |
| BS-FLOW   | M09, M10, M14, M17      | BC-01 to BC-05                                                |
| BS-UI     | M13, M20, M32, M33      | Page frame timing; the window manager's resize                |
| BS-SEARCH | M22                     | BC-06                                                         |
| BS-AGENT  | M07, M26, M27           | BC-08; real agents only with authorisation                    |
| BS-CONFIG | M16, M19, M29           | BC-07 on a fixture filesystem of the same kind as the owner's |
| BS-VAULT  | M28                     | A test custody in an isolated session (§11.3)                 |

### 11.2 Corpora and fixtures

Generated deterministically from a seed, so that every run sees the same bytes. Kept as
generators, not as large files.

| ID    | Content                                                                                                                     |
| ----- | --------------------------------------------------------------------------------------------------------------------------- |
| BC-01 | Plain ASCII, 80-column lines, 64 MiB                                                                                        |
| BC-02 | Dense colour: 256-colour and true-colour attributes on every cell, 16 MiB                                                   |
| BC-03 | Wide characters: CJK, combining marks and emoji, 16 MiB                                                                     |
| BC-04 | Very long lines without newlines, 8 MiB                                                                                     |
| BC-05 | Full-screen redraws with cursor addressing, as a full-screen program produces them                                          |
| BC-06 | 50,000 lines with markers at known positions and counts                                                                     |
| BC-07 | Configuration trees of 10, 100 and 1,000 files, from the fixtures of 07 §10, counts shaped like the owner's (counts only)   |
| BC-08 | A fake agent executable: prints its arguments and environment, idles, prints on a schedule, spawns a set number of children |
| BC-09 | Layout files of 1, 4 and 16 panes, with and without remembered agents                                                       |

### 11.3 Instrumentation

| Instrument                       | State                                                                                                                             |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| A timestamp at each stage of M25 | In a build flag; whether it may exist in packaged builds is OQ-26                                                                 |
| Page frame and input timing      | The page's own timing interfaces, in the benchmark build only                                                                     |
| `/proc` and cgroup sampling      | An external sampler, run beside the Console                                                                                       |
| External input-to-photon latency | OQ-27                                                                                                                             |
| Kernel tracing of wake-ups       | Only with the owner's word; otherwise voluntary context switches from `/proc`                                                     |
| A test custody for M28           | A secret service in an isolated session bus, never the owner's keyring; whether one can be run that way on his computers is `[N]` |

### 11.4 Result record

One record per run, kept with the baseline P1 delivers (`docs/spec/performance-baseline.md` and its raw data, 19 §3, P1) when P1 is authorised: the metric,
the machine, the build commit, the date, the session type, the compositor, the window mode, the
web view's and the kernel's versions, the governor, the power source, the input method state, the
replication and snapshot tools' state, the number of samples, median, p95, interquartile range,
the confidence interval, and the path of the raw samples. A summary table per machine and the
Ghostty comparison (§3.3) beside it.

### 11.5 Regression policy

The baseline is P1's result, per machine. A later phase is compared on the same machine with the
same procedure; a regression beyond §4.2's threshold, confirmed by a second run, blocks the
phase's acceptance unless the owner accepts it in writing with the reason. The baseline is replaced
only by a new P1-style run the owner accepts. A budget is never relaxed to make a phase pass.

## 12. Not verified, and not decided

| Item                                                                                             | State     |
| ------------------------------------------------------------------------------------------------ | --------- |
| Every number of §4                                                                               | OQ-17: P1 |
| A timestamp hook in packaged builds                                                              | OQ-26     |
| The external latency method                                                                      | OQ-27     |
| The web view's process set, memory floor, rendering path, and whether hidden content is laid out | OQ-47     |
| Event-driven replacements for the two idle polls of PR-01                                        | OQ-48     |
| The page and the backend after a renderer crash                                                  | OQ-46     |
| A global bound on scrollback memory                                                              | DN-40     |
| The terminal output transport, if M23 fails                                                      | DN-41     |
