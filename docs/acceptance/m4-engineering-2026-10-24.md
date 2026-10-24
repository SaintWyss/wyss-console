# M4 engineering acceptance record — 2026-10-24

**Branch:** `feat/m4-terminal-fidelity` (from PR #24, `integration/m2-m3` at `6c944dd`) · **Design:** [ADR 0057](../adr/0057-terminal-screen-parser-helper.md) (proposed) ·
**Contract:** [m4-terminal-fidelity](../contracts/m4-terminal-fidelity.md)

**Result:** `ENGINEERING_TESTS_PASSED` for the M4 scope below · the one independent review of this branch returned
**FAIL** (nine findings, all addressed in this branch; the fixes have **not** been re-reviewed) · `SECURITY_REVIEW_PENDING` ·
`OWNER_APPROVAL_REQUIRED` (ADR 0057; lingering stays the owner's) · `RELEASE_BLOCKED`.
Nothing here installs, restarts or migrates anything: the installed Console, its host and its systemd settings were not
touched; lingering was never turned on (the one function that does it is only ever run with a fake `loginctl` in tests).

## What was built

| Roadmap item                                           | What exists                                                                                                                                                                                                                          | Status                                                          |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------- |
| Isolated, individually restartable parser helpers      | `wyss-screen`: one process per terminal, typed strict IPC, Landlock + seccomp + rlimits + no-new-privs applied before the first byte, restart budget, eager replacement, replay from the host's ring only                            | `IMPLEMENTED_AND_TESTED`                                        |
| Screen / alternate buffer / cursor / modes / regions   | VT state machine (normal + alternate buffers, cursor and saved cursor, scroll regions, origin/insert/wrap/keypad/mouse/paste/focus modes, SGR incl. 256/truecolor/colon forms, wide characters, combining marks, line-drawing set)   | `IMPLEMENTED_AND_TESTED` (limits below)                         |
| Bounded volatile checkpoints, explicit history gaps    | Repaint (a byte stream that redraws screen, modes and up to 2,000 history lines) with the output offset it ends at; `exact` / `gap{skipped}` / `tail{missing}` / `unavailable` fidelity, shown by the window                         | `IMPLEMENTED_AND_TESTED`                                        |
| One authoritative size                                 | The PTY's; the parser follows it by value after the preceding bytes; a hidden pane never fits or resizes (existing test)                                                                                                             | `IMPLEMENTED_AND_TESTED`                                        |
| Persistent lifecycle / reattach                        | `terminal_attach` draws the kept screen, then reads on from where it ends; dead terminals keep their last screen and say exited; no replay, no duplicate process; older hosts and hosts without a helper fall back to the old replay | `IMPLEMENTED_AND_TESTED`                                        |
| Truthful lock/logout/suspend/power-off + lingering     | `host_persistence` computes what survives what from this machine's facts; lingering is changed only by an explicit, confirmed request, read back, reversible; a settings card shows it                                               | `IMPLEMENTED_AND_TESTED` (reboot/power-off/host-crash reasoned) |
| Terminal bytes are data (no clipboard/URL/app actions) | Counted-and-dropped sequences, repaints free of them, renderer with no handler or add-on that could act (source test)                                                                                                                | `IMPLEMENTED_AND_TESTED`                                        |
| Layout files: newer untouched, corrupt kept aside      | `host::layout` (AC-FP-TERM-02)                                                                                                                                                                                                       | `IMPLEMENTED_AND_TESTED`                                        |
| Reference-screen corpus, fuzz, resource bounds         | 17 real/scripted streams recorded against tmux; round-trip property tests; 200,000 mutated streams; flood, kill, OOM-ceiling and hostile-output tests                                                                                | `IMPLEMENTED_AND_TESTED`                                        |

## Acceptance criteria

| Criterion                                                                                                    | Evidence                                                                                                                                                                                                                                                                                                   | Result                                                                                             |
| ------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| **AC-FP-TERM-01** split/zoom/resize/close/search preserve focus and PTY geometry; keys/IME reach the program | Existing terminal, pane and renderer suites unchanged and green (1,060 frontend tests); hidden renderers do not fit/resize; a resize burst under output ends with the parser at the PTY's final size                                                                                                       | `PASS` for what is covered; real IME/dead-key/screen-reader acceptance on a reference PC `NOT_RUN` |
| …oversized paste sends nothing                                                                               | Unchanged: past 64 KiB the terminal fails visibly and writes nothing (`terminal-controller.test.ts`)                                                                                                                                                                                                       | `PASS` (the harsh form is kept; a gentler refusal is an owner choice)                              |
| …hostile title/escape/clipboard corpus produces no application action                                        | Parser counts 52/8/CSI-t/DA/DSR/DCS/APC; repaint contains none; renderer registers no OSC/CSI handler, no add-on but fit, links are inert; 200k mutated streams keep every bound                                                                                                                           | `PASS`                                                                                             |
| **AC-FP-TERM-02** close/reopen GUI                                                                           | Isolated desktop: terminal running `less`, window closed and reopened, same terminal id and pid, screen redrawn exactly (`(END)` in the repaint and on screen)                                                                                                                                             | `PASS`                                                                                             |
| …parser crash                                                                                                | Helper killed (SIGKILL) in the desktop run and in host tests: terminal untouched, helper replaced, screen exact again (54–155 ms); killed repeatedly: parser `unavailable`, terminal untouched, window says "Screen parser unavailable", reopened window says "Output replayed · no screen state was kept" | `PASS`                                                                                             |
| …host crash                                                                                                  | Real host process killed: its terminal's program is hung up, its helper dies with it (PDEATHSIG), nothing answers, a new host holds no terminal and resurrects none                                                                                                                                        | `PASS`                                                                                             |
| …logout with / without lingering, suspend, power-off have distinct outcomes                                  | Reported distinctly from facts read on this machine, each with how it is known; the outcomes themselves (logout, suspend) were **reasoned or read from configuration, not exercised** (no logout, suspend or power cycle was performed)                                                                    | `PARTIAL`: the account is truthful and tested; the events were not triggered                       |
| …reattachment matches reference screens                                                                      | Repaint of every corpus screen redraws it cell for cell in this engine; this engine matches tmux's screen on all 17; exact-vs-xterm.js rendering is shown by the desktop run (screenshots), not by a pixel comparison                                                                                      | `PASS` (tmux as reference) / xterm.js comparison `PARTIAL`                                         |
| …no restart while old descendants may still act; newer/corrupt layouts                                       | Attaching never starts or writes anything; restarts remain the explicit Native restart with its checks (M2); `layout_newer` leaves the file byte for byte, an unreadable one is kept aside (3 kept)                                                                                                        | `PASS`                                                                                             |
| **AC-FP-SES-03** (host-side terminal part)                                                                   | Attach after a crash leaves exactly one terminal, the same pid, one execution of a typed command                                                                                                                                                                                                           | `PASS` for terminals; the authority part belongs to M2/M3                                          |
| DoD: faithful attach, bounded memory, no claimed survival of dead processes, no automatic duplicate launch   | above; memory below; the lifecycle table says power-off and a host crash end terminals                                                                                                                                                                                                                     | `PASS`                                                                                             |

## Test results (this branch's head)

| Area                           | Command                                                                                            | Result                                                                                                                       |
| ------------------------------ | -------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| Rust format, check, clippy     | `npm run check:rust`; clippy `--all-targets --features m3-fixture -D warnings`                     | PASS                                                                                                                         |
| Rust tests                     | `cargo test --features m3-fixture`                                                                 | PASS: 1,418 lib, 535 bin, 15 helper-process, 6 corpus, 1 real-host survival, plus the older process/contract suites          |
| Frontend                       | `format:check`, `lint`, `typecheck`, `npm test`                                                    | PASS: 1,054 tests                                                                                                            |
| Repository scan                | `npm run scan:repo`                                                                                | PASS                                                                                                                         |
| Isolated desktop (nested KWin) | `python3 -I benchmarks/m1_gui.py … --steps benchmarks/m4_gui_scenarios/attach.json`                | PASS: 6 pictures, host checks `m4-*` all ok, `leftovers: []` ([evidence](evidence/m4-2026-10-24/gui/))                       |
| Mutation fuzz, release         | `FUZZ_ITERATIONS=200000 cargo test --release --test screen_corpus -- --ignored long_mutation_fuzz` | PASS: 200,000 mutated real streams, 411 MB, 15 s (rerun after the review fixes), every bound and the repaint round trip held |

### Reference-screen corpus (tmux as the independent implementation)

17 byte streams recorded from real programs and scripted sequences run in tmux 3.8 on a private socket
(`benchmarks/screen_corpus.py`): vim (editing, split, numbers, search), htop (tree view, setup menu), less, man, top,
tmux in tmux, a curses application, an interactive bash session, `ls --color`, 256-colour / truecolour / attribute
tables, wide characters and combining marks, line-drawing sets, scroll regions with IL/DL/RI/SU/SD, alternate-screen
round trips, tab/wrap/insert/ICH/DCH/ECH/REP, save/restore and origin mode, mouse/paste/focus modes, and hostile
output. For each, the Wyss engine's state after the raw stream is compared with tmux's own interpretation of the same
bytes: visible text, **every cell's character, width and attributes** (through tmux's escape rendering), cursor
(including the wrap-pending position), alternate-screen, cursor-visible, origin, wrap, insert, keypad and mouse flags
and the scroll region. All 17 agree. What the comparison found and was fixed: erase operations must not clear a
pending wrap (`top`), the three mouse tracking modes are mutually exclusive, a wide character at the right edge
wraps whole, a repaint must place a hidden normal screen's cursor by absolute position and a saved cursor outside the
region under origin mode needs a full-screen region, history lines must be pushed out of the visible area with
line feeds, and the repaint of a pending-wrap cursor must re-write the last cell. What differs from tmux **by
design** and is documented: DECCOLM (tmux clears the screen; the engine counts and refuses a width change), custom tab
stops (tmux has none; its tab cells are not compared), emoji/ZWJ/flag clusters (tmux and xterm.js disagree; the corpus
avoids them and the engine follows xterm.js's table because that is what draws the window), and sixel (not in the
corpus: tmux draws image placeholders).

Hygiene of the corpus: it records only programs the script started (htop and top are restricted to its own `sleep`
processes), in a neutral directory, with no home path or host name in any file (a test scans the repository).

### Isolation, observed from inside the helper (probe tests, each with an unrestricted baseline)

On this machine (kernel 6.18.55, Landlock ABI 7): reading `/etc/passwd`, creating a file and opening
`/proc/self/environ` succeed without Landlock and fail with `EACCES` with it; TCP connect works (`ECONNREFUSED`)
without and gets `EACCES` with; signalling the parent works without and gets `EPERM` with; `socket`, `connect`,
`fork`, `execve` and `ptrace` end the process with `SIGSYS` under seccomp and work without; a 1 GiB mapping fails with
`ENOMEM` and descriptors stop under 8 with rlimits; with all layers the parser still works. A helper that is
required to be sandboxed and cannot be exits with code 4 and the link never uses a weaker one (`a_helper_without_the_required_isolation_is_not_used`).
`NOT_RUN` here: other kernels or Landlock ABIs below 6 (those probes skip with a message), and any attacker with a
kernel exploit (not claimed).

### Performance (reference PC: this machine; release build; raw samples in [evidence](evidence/m4-2026-10-24/perf/))

| Measure                                                              | p50                         | p95                     | Notes                                                                                                                                                        |
| -------------------------------------------------------------------- | --------------------------- | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Parser throughput, plain text                                        | 183 MiB/s                   | 189 MiB/s               | 7 runs of 32 MiB (before the ASCII run fast path: 19 MiB/s)                                                                                                  |
| Parser throughput, corpus mix                                        | 116 MiB/s                   | 117 MiB/s               | escapes, colours, cursor motion, wide characters                                                                                                             |
| Repaint 24×80 with 2,000 history lines (167 KB)                      | 1.38 ms                     | 1.82 ms                 | was 0.78 / 0.86 ms before the review fixes: the 4 MiB bound is now checked per row while rendering; 60×200: 1.37 / 1.50 ms; without history (2 KB): 0.012 ms |
| Helper start to ready, all layers applied                            | 0.60 ms                     | 0.87 ms                 | 30 runs                                                                                                                                                      |
| Helper RSS idle / with 2,000 lines of history / after 8 MiB of flood | 3.2 / 6.3 / 6.6 MiB         | 3.3 MiB (idle)          | one thread, no descriptors beyond the three; 16 terminals: 53 MiB for all helpers                                                                            |
| IPC round trip: ping / one line fed / 500-line snapshot              | 0.005 / 0.006 / 0.28 ms     | 0.006 / 0.008 / 0.45 ms |                                                                                                                                                              |
| IPC feed throughput (64 KiB frames)                                  | 194 MiB/s                   | 199 MiB/s               |                                                                                                                                                              |
| `term_screen` through the host, 500 lines                            | 0.38 ms                     | 0.97 ms                 | 40 calls on a terminal with 1,500 lines printed; repaint 32 KB                                                                                               |
| Terminal create to first attachable screen                           | 4.6 ms                      | 5.5 ms                  | 10 runs                                                                                                                                                      |
| Helper killed to healthy and fed again                               | 106 ms                      | 156 ms                  | 3 runs (includes the 50·n ms back-off of the restart budget)                                                                                                 |
| Idle cost of a terminal's parser (helper + link)                     | 0 ticks, 0 wake-ups in 10 s | —                       | measured on the helper; the link thread's wake-ups are asserted `< 0.5/s` (measured 0.0)                                                                     |
| Threads in the host process with 16 terminals                        | 83                          | —                       | 5 per terminal (pump, PTY worker, link, its two pipe threads) plus the server                                                                                |

Limits of these figures: one machine, a loaded desktop session, no budget was proposed or applied, the window's own
rendering and the webview are not part of them, and the 16-terminal figures are an idle shell each.

## Defects found and fixed while building

(After the independent review, see its table.) A snapshot request starved by a flood of wakes (found by the host measurement: `busy`); a link that restarted in a
loop when output outran the parser (now a stated `gap`, no restart); a lost helper replaced lazily (now eagerly,
within the budget) and a link that gave up never trying again (it does after the window); a CSS variable; the
packaging list and the fixture check missing the third binary; a settings-headings test and a capability-name filter
that matched a new command name; three flaky host tests that counted the echo of a command instead of its output.

## Not done / unqualified

- Reflow on resize; custom tab stops, the shifted charset and text selection are not carried across an attach.
- The Unicode 11 / grapheme table (the window loads xterm.js's default Unicode 6; the engine matches it).
- Real IME, dead-key and screen-reader acceptance on a reference PC; a pixel comparison with xterm.js.
- Logout, suspend and power-cycle were not performed (the lifecycle table says how each row is known).
- Oversized paste still fails the terminal visibly (it sends nothing) rather than being refused gently.
- No budgets (p95 limits) were set; no Codex/Claude Native session was exercised through the new attach (the engine
  and the host are provider-neutral; M2's Native fixtures are not part of this record).

## Owner decisions pending

ADR 0057 (the helper per terminal, the required layers with no degraded mode, the width table); whether the package
should ship the helper (it is in the package list; nothing is installed here); whether install should ever suggest
lingering; the gentler paste refusal; any budgets.

## Independent review

One independent review (Codex, read-only, over `d6790ce`) was requested and run. **Verdict: FAIL as a release
basis.** It could not run the host tests in its sandbox (`NOT_RUN` there); the findings came from reading and from
reproducing small cases. All nine were confirmed and fixed here, each with a regression test; **the fixes were not
reviewed again** (one review per milestone was asked for), so nothing below is an independent verdict.

| #   | Severity | Finding                                                                                                          | Fix and test                                                                                                                                                                        |
| --- | -------- | ---------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F01 | High     | helper trust accepted a group-writable file and checked a path that was then executed (race)                     | `screen::locate`: no group/other write bit, ancestors verified, executed through the verified descriptor (`/proc/self/fd/N`); 8 tests incl. the descriptor surviving a swapped path |
| F02 | High     | a helper that floods unasked frames blocked the reader and `Session::end` joined it for ever                     | the receiver is dropped before the joins; `a_helper_that_floods_unasked_frames_cannot_hold_the_link_or_its_end`                                                                     |
| F03 | Medium   | C1 characters (U+009D/U+009C) were stored as cells and re-emitted in the repaint: an OSC 52 reached the renderer | C1 controls are interpreted as controls, counted, never stored; `c1_controls_do_nothing_a_clipboard_write_included_and_never_reach_a_repaint`                                       |
| F04 | Medium   | the ring read discarded the effective offset: a ring that moved between asking and reading shifted the stream    | `ByteSource::read` returns `(offset, bytes)`; the link skips to it (or starts the screen there); `a_ring_that_moves_between_asking_and_reading…`                                    |
| F05 | Medium   | layout write: oversize file taken for absent, check/rename race, reusable aside names, links followed            | lock + no-follow look + oversize kept + unique aside names + re-check before replace; 5 new `host::layout` tests                                                                    |
| F06 | Medium   | a resize had no byte boundary, and continuous output could starve commands                                       | `SizeLog` by stream offset, frames never cross a change, a 1 MiB quantum then queued work; `a_resize_takes_effect_between…`, `a_long_backlog_is_fed_in_quanta…`                     |
| F07 | Medium   | the repaint "budget" was not enforced                                                                            | rows checked against 4 MiB while rendering; `TooLarge` answer, helper keeps its screen; process test with a 512×512 screen of unique colours                                        |
| F08 | Medium   | attach committed the read cursor before the window had drawn anything                                            | two-phase attach (`terminal_attach_commit`), host-side validation, one-use token, rollback to replay; Rust and vitest tests                                                         |
| F09 | Low      | `offset + length` could overflow in the helper                                                                   | checked add, `BadFrame`; process test                                                                                                                                               |

Of the review's nine claims, one passed (hostile output is data) before these fixes and the other eight were tied to
the findings above. The wording that overstated them in the ADR and the contract was corrected.

## Preservation and rollback

Nothing installed, restarted or migrated; no systemd unit or setting changed; the isolated desktop run compared the
installed Console's own directories before and after (`fs-1-installed-console-untouched.json`: no change); all
processes of the runs were direct children ended through their handles (`leftovers: []`); the corpus recorder used a
private tmux socket and ended its servers through it. **Rollback:** remove `libexec/wyss-console/wyss-screen` (or
stop the helpers): every terminal keeps running and the window replays the ring as before; no data was migrated.
