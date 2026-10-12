# 15 — Performance engineering

Domain Q of [04](04-TARGET-ARCHITECTURE.md). What is measured, how, against what, and the
budgets proposed. **No measurement of Wyss Console exists** `[R]` (no benchmark in the
repository; none was run for this set). Every number below is a proposed target, not a result.

## 1. Position

The owner named Ghostty as the reference for speed, lightness and comfort. Ghostty is a
reference to measure against, not a dependency and not a claim. Wyss Console is a WebView
application (Tauri 2, WebKitGTK) drawing with xterm's DOM renderer `[R]` (ADR 0006); a native
GPU terminal starts from a different floor. Whether the gap matters to the owner is a question
for numbers, and no technology is changed before they exist (AD-17).

## 2. What is measured

| ID  | Metric                    | Definition                                                                                                        | Requirement     |
| --- | ------------------------- | ----------------------------------------------------------------------------------------------------------------- | --------------- |
| M01 | Cold start                | From process start to the first shell prompt drawn, first run after boot (page cache dropped)                     | NFR-PERF-01     |
| M02 | Warm start                | The same, on the third consecutive run                                                                            | NFR-PERF-02     |
| M03 | Idle processor            | Share of one core used by all the Console's processes over five minutes, one terminal at a prompt, window visible | NFR-PERF-03     |
| M04 | Idle processor, hidden    | The same with the window minimised                                                                                | NFR-PERF-03     |
| M05 | Idle memory               | Sum of proportional set size of every process of the Console, one terminal, after two minutes                     | NFR-PERF-04     |
| M06 | Memory per terminal       | Increase of M05 per additional terminal, empty scrollback and full scrollback (5,000 lines)                       | NFR-PERF-05     |
| M07 | Memory per native agent   | The agent's own process tree, reported separately: it is the provider's cost, not the Console's                   | FR-NAT-07       |
| M08 | Input latency             | From key press to the glyph on screen, at an idle prompt                                                          | NFR-PERF-06     |
| M09 | PTY throughput            | Time to consume a fixed plain-text file printed to the terminal, and the bytes per second                         | NFR-PERF-07     |
| M10 | Render throughput         | The same with dense colour and cursor movement                                                                    | NFR-PERF-07     |
| M11 | Responsiveness under load | Longest interval in which the page handled no input while M09 runs                                                | NFR-PERF-07, 10 |
| M12 | Scrollback memory         | Memory at 5,000, 50,000 and the maximum number of lines                                                           | NFR-PERF-05     |
| M13 | Resize latency            | From the end of a window resize to the PTY having its final size and the screen redrawn                           | NFR-PERF-08     |
| M14 | Runaway output            | Memory and responsiveness while a program prints without end for sixty seconds                                    | NFR-PERF-10     |
| M15 | Paste                     | Time to deliver 4 KiB, 64 KiB; behaviour beyond the bound                                                         | FR-TERM-07      |
| M16 | Watcher cost              | M03 and M05 with the configuration watcher armed on the fixture set; cost of one rescan                           | NFR-PERF-09     |
| M17 | Many sessions             | M11 and M03 with the maximum of terminals, half of them printing                                                  | NFR-PERF-11     |
| M18 | Authority at rest         | Processor and memory of `wyss-control` with no session                                                            | NFR-PERF-03     |
| M19 | First inventory           | Time for the configuration inventory of one provider on the fixture set and on a large real-shaped set            | 13 §6           |

## 3. Procedures

Reproducible, scripted, each producing a file of raw numbers and a summary. They are
specifications; none has been run.

| Metric        | Procedure                                                                                                                                                                                                                        | Tools assumed present                                              | Not yet decided                                                     |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------- |
| M01, M02      | Start the packaged binary 30 times with a shell that prints a marker and exits its start-up; time from `exec` to the marker reaching the page (a debug build flag emits a timestamp when xterm first writes)                     | A stopwatch harness of our own                                     | Whether the timestamp hook is acceptable in a release build (OQ-26) |
| M03, M04, M18 | Sample per-process CPU time from `/proc/<pid>/stat` at start and end of the window; report the difference                                                                                                                        | `/proc`                                                            | —                                                                   |
| M05, M06, M12 | Read `Pss` from `/proc/<pid>/smaps_rollup` for every process of the Console, including the WebKit helper processes                                                                                                               | `/proc`                                                            | —                                                                   |
| M08           | Two methods, to be compared: instrumented (timestamp at key event in the page and at the frame after xterm rendered) and external (a high-speed camera or a latency tester)                                                      | The instrumented one needs nothing                                 | Which external method (OQ-27)                                       |
| M09, M10, M11 | Print fixed corpora (plain ASCII; dense SGR; wide Unicode; long lines) from a file with `cat`; time until the shell prompt returns and the page has drawn it; record the longest input gap from a second, synthetic input source | Corpora checked into a test directory                              | Corpus contents                                                     |
| M13           | Resize by fixed steps through the window manager; time until `stty size` inside the terminal reports the final size                                                                                                              | A nested display, as the project already uses for acceptance `[O]` | —                                                                   |
| M14           | `yes` for sixty seconds; sample M05 every second; M11                                                                                                                                                                            | —                                                                  | —                                                                   |
| M16, M19      | Arm the watcher on a generated tree of 10, 100 and 1,000 files; measure rest cost; touch one file and measure time to the change record                                                                                          | Fixture generator                                                  | —                                                                   |

Rules for every run: the packaged build, not a development build; the reference machines of
§5; three repetitions, median and 95th percentile reported; the compositor, display server,
font and shell recorded with the result; an input method active and inactive, since the owner
uses one `[O]`.

### 3.1 Comparison with Ghostty

The same procedures, on the same machine, the same session, the same font size and window
size, the same shell with the same start-up files. Reported side by side as absolute numbers
and as a ratio. Where a procedure cannot be applied to Ghostty unchanged (the instrumented
latency method cannot), only the external method is compared. The comparison is published in
the project with its conditions; "comparable to Ghostty" is never written without it.

## 4. Proposed budgets

Provisional. Each is restated after the baseline (P1) as "no worse than the baseline" for what
is already good enough, and as a target with a deadline for what is not. They are written here
so that a regression has something to be measured against from the first day.

| Metric | Proposed budget (reference notebook)                                                                           | Basis                                                                           |
| ------ | -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| M01    | ≤ 1,500 ms median                                                                                              | A guess for a WebView start; to be replaced                                     |
| M02    | ≤ 700 ms median                                                                                                | Same                                                                            |
| M03    | ≤ 0.5 % of one core                                                                                            | A terminal at rest should do nothing; a blinking cursor is off by default `[R]` |
| M04    | ≤ 0.1 %                                                                                                        | Hidden means no drawing                                                         |
| M05    | To be set from the baseline; proposed ceiling 300 MiB                                                          | WebKitGTK's own floor is unknown here `[N]`                                     |
| M06    | ≤ 10 MiB empty; ≤ 25 MiB with 5,000 lines                                                                      | To be replaced                                                                  |
| M08    | ≤ 20 ms median, ≤ 40 ms 95th percentile, instrumented                                                          | Two frames at 60 Hz as the median target                                        |
| M09    | The window never stops handling input for more than 100 ms (M11); throughput reported, no floor until measured | Responsiveness matters more than speed of a flood                               |
| M13    | ≤ 100 ms after the last resize event                                                                           | The fit is debounced at 60 ms today `[R]` (`controller.ts:119`)                 |
| M14    | Memory bounded by the scrollback setting; M11 as above                                                         | Output is held to 64 KiB and released on acknowledgement `[R]`                  |
| M16    | No measurable difference in M03; one change record within 500 ms; a full rescan of 100 files within 50 ms      | NFR-PERF-09                                                                     |
| M17    | M11 ≤ 150 ms; M03 scales with what is printing, not with what is open                                          | —                                                                               |
| M18    | ≤ 0.1 % of one core; memory reported                                                                           | —                                                                               |
| M19    | ≤ 200 ms for the fixture set                                                                                   | 13 §6                                                                           |

A budget marked "to be replaced" is honest about being a placeholder. None of these is a
promise to the user.

## 5. Reference machines

| Class    | Description `[O]`                                                                     | Why                                                |
| -------- | ------------------------------------------------------------------------------------- | -------------------------------------------------- |
| Notebook | A 2020 four-core mobile processor, 16 GB, integrated and discrete graphics, 1920×1080 | The owner's daily machine; the constrained case    |
| Desktop  | A current eight-core desktop processor with a discrete GPU                            | The owner's second machine; the unconstrained case |

Both run the same distribution, desktop and display server. Results are tied to the machine,
the session type (X11 or Wayland) and the compositor, and are reported with them.

## 6. Where the cost is expected

Hypotheses to test, in the order the baseline should examine them. None is established.

| Hypothesis                                                                                     | How P1 confirms or rejects it                                                     |
| ---------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| The WebView's own processes dominate idle memory                                               | M05 broken down per process                                                       |
| The DOM renderer limits render throughput and makes M11 the weak point                         | M10 and M11 with a profile of the page                                            |
| Each output batch crosses the IPC as an acknowledged request; the round trip bounds throughput | M09 with batch size and count recorded (`OUTPUT_BATCH` is 16 KiB `[R]`)           |
| Start-up time is the WebView, not the Rust backend or the authority link                       | M01 with timestamps at process start, window created, page loaded, first terminal |
| Hidden terminals cost nothing                                                                  | M03 with 16 terminals, 15 hidden                                                  |
| The authority link's watch does not wake the page at rest                                      | M03 with and without the authority                                                |

## 7. Optimisation order

Only after P1, and only where a number says so:

1. Anything that runs at rest and should not (timers, watches, polling).
2. Start-up: defer what the first prompt does not need.
3. Output path: batch size and acknowledgement cadence, within the existing bounds.
4. Renderer options inside xterm (its other renderers are add-ons and new dependencies: DN-04).
5. Only if 1 to 4 leave a gap the owner judges unacceptable: study a different terminal core,
   as a research phase with its own ADR, comparing at least the effect on the configuration
   and session views, on accessibility and on maintenance.

## 8. Guard rails

| Guard                                                                            | Phase   |
| -------------------------------------------------------------------------------- | ------- |
| The benchmark scripts live in the repository and run on demand                   | P1      |
| Every phase's acceptance repeats M03, M05 and M16 and compares with the baseline | From F1 |
| A change that adds a timer or a watch states its rest cost                       | Always  |
| No view does work while hidden (13 §6)                                           | Always  |

## 9. Not verified

Everything. This document specifies; P1 measures.
