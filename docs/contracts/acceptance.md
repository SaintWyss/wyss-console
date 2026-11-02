# Acceptance and performance measurement

[Requirements](../REQUIREMENTS.md) own target behavior and all 31 FP/AC groups;
[STATUS](../STATUS.md) records implemented versus qualified capabilities and blocking gaps.
Source/unit/fixture evidence, real GUI/provider journeys, physical-device tests and owner
acceptance are separate levels. A documentation review cannot close product acceptance.
Current source is not certified by a dated report for another installed binary.

## Evidence and preservation

Use exact source/build/provider versions and disclose OS/kernel/WebKitGTK/GTK/compositor,
hardware, load, governor/power, IME and sync/snapshot conditions. Record real steps, result,
failure/NOT_RUN reason, negative controls and limits; unsupported is not PASS. Keep raw
execution artifacts with the run/PR rather than appending them to the repository. Git
retains previous reports. Do not check in duplicate screenshot/log/result inventories.

Real acceptance uses the owner's normal desktop/HOME/XDG/session bus/wallet. Disposable
private HOME/keyring/bus procedures from old reports are retired; ordinary synthetic unit
fixtures and intrinsic runtime isolation remain valid. Do not activate ignored paid-provider,
credential, migration or service tests simply to refresh documentation.

Benchmark cleanup may signal only the owned, unreaped child through the owned-process
harness/pidfd. Never select processes by executable/name/path or rediscover a reused PID.
The harness assumes the sole default SIGCHLD reaper; do not combine it with another reaper.
An old cleanup incident ended live work: retaining this rule matters more than retaining
its incident diary. Do not close other apps, reset services, drop caches or reboot as routine
measurement preparation. Cold-start runs requiring such operations need their own plan;
otherwise label first-run-after-boot/cache conditions honestly.

## Cost boundaries and method

Report desktop plus WebKit helpers as Console cost, authority and its Managed workers
separately, and shells/Native provider trees separately. Use PSS where available; report
uncertainty in ownership rather than quietly charge a provider to the Console or omit it.
Use monotonic timestamps, matched workloads, warmups and at least three independent runs.
Retain failed/outlier samples; report median/p95 and spread (IQR; bootstrap 95% confidence
interval with 10,000 resamples when appropriate). Latency tails require enough samples;
three runs alone are not a p99 claim. Compare identical packaged builds/settings/corpora.
Ghostty is a performance reference, not a dependency or automatic budget.

Numeric targets from old design documents were provisional. Agree reference budgets before
calling a measurement PASS. Regression comparisons include baseline uncertainty. No
optimization may skip authorization, audit, integrity, flushes or recovery for a speed gain.
Hidden views avoid view work while output continues draining; adapt new allocations under
pressure without silently discarding existing output or weakening a security check.

## Metric catalogue

| ID  | Metric                       | Boundary                                                                                                                                                                                 | Workload                                                                             |
| --- | ---------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| M01 | Cold start                   | From `exec` of the packaged binary to the first shell prompt drawn (a marker printed by the shell reaches the screen); first-run/cache state disclosed                                   | No layout file; one terminal; authority running; a variant with the authority absent |
| M02 | Warm start                   | The same, third consecutive run, page cache warm                                                                                                                                         | As M01                                                                               |
| M03 | Idle processor, visible      | Console processor time over five minutes divided by wall time, one terminal at a prompt, window visible, no input                                                                        | One idle shell; cursor not blinking (default non-blinking cursor)                    |
| M04 | Idle processor, hidden       | The same with the window minimised                                                                                                                                                       | As M03                                                                               |
| M05 | Idle memory                  | Sum of proportional set size of every Console process after two minutes idle                                                                                                             | As M03                                                                               |
| M06 | Memory per terminal          | Increase of M05 per added terminal, with empty scrollback and with 5,000 lines                                                                                                           | 1 to 16 idle terminals                                                               |
| M07 | Memory per native agent      | One Native agent: its process tree's proportional set size after start and after one hour idle. **The provider's cost, never the Console's**                                             | A fake agent (BC-08); real agents only with authorisation                            |
| M08 | Input latency                | From the key event to the frame on which the echoed glyph is drawn, at an idle prompt                                                                                                    | Typing at 10 keys per second in bash with no prompt hooks                            |
| M09 | PTY throughput               | Time from starting to print BC-01 to the shell prompt drawn after it; bytes per second                                                                                                   | `cat` of BC-01                                                                       |
| M10 | Render throughput            | The same with BC-02, BC-03, BC-04 and BC-05                                                                                                                                              | `cat` of each                                                                        |
| M11 | Responsiveness under load    | Longest interval in which the page handled no input event while M09 runs                                                                                                                 | A synthetic input source at 50 events per second during M09                          |
| M12 | Scrollback memory            | M05 with one terminal holding 5,000, 50,000 and the maximum lines of BC-01                                                                                                               | Preference set per run                                                               |
| M13 | Resize latency               | From the end of a window resize to the PTY at its final size and the screen redrawn                                                                                                      | Resize by fixed steps; a full-screen program running                                 |
| M14 | Runaway output               | Memory and M11 while a program prints without end for sixty seconds                                                                                                                      | `yes`                                                                                |
| M15 | Paste                        | Time to deliver 4 KiB and 64 KiB to the program; behaviour beyond 64 KiB                                                                                                                 | A program that counts received bytes                                                 |
| M16 | Watcher cost                 | M03 and M05 with the configuration watcher armed; time from a file's change to its change record; cost of one full rescan                                                                | BC-07 with 10, 100 and 1,000 files                                                   |
| M17 | Many sessions                | M11 and M03 with 16 terminals, half printing BC-01 in a loop                                                                                                                             | 16 panes in 4 tabs                                                                   |
| M18 | Authority at rest            | The authority's processor and memory with no session                                                                                                                                     | Authority started, idle                                                              |
| M19 | First inventory              | From the Configuration view being shown to the first provider's inventory drawn                                                                                                          | BC-07 and a large real-shaped set (counts only)                                      |
| M20 | Divider drag                 | Resize requests per pane per second, and frames over 16.7 ms, while a divider is dragged for five seconds                                                                                | Two panes with full-screen programs                                                  |
| M21 | Restore                      | M01 with a layout of 16 panes in 4 tabs, against M01 without one                                                                                                                         | BC-09                                                                                |
| M22 | Search                       | From the search request to the match count drawn                                                                                                                                         | BC-06 in a 50,000-line scrollback                                                    |
| M23 | Idle terminals               | Wake-ups per second of Console threads, IPC calls per second, and M03, with 16 idle terminals, hidden and shown                                                                          | 16 idle shells                                                                       |
| M24 | Scrollback memory, all panes | M12 summed over 16 panes at 5,000 and at 50,000 lines                                                                                                                                    | 16 terminals filled with BC-01                                                       |
| M25 | Start-up breakdown           | Timestamps of M01's stages: process start, GTK initialised, services constructed, window created, page loaded, application mounted, terminal created, first byte, first prompt drawn     | As M01                                                                               |
| M26 | Agent launch overhead        | From "Start" in the launcher to the agent identified, minus the same shell's start-up time measured by a hand-typed start                                                                | BC-08 with an empty profile                                                          |
| M27 | Supervision cost             | Processor time of one sampling round of 16 sessions' process trees, and rounds per second, with the Sessions view shown and hidden                                                       | 16 fake agents, each with 10 children                                                |
| M28 | Vault operation              | Seal and verify of a record; decryption for a restoration plan; excluding dialogs, consent and the custody's own prompt; custody fetch reported apart                                    | Records of 1 KiB, 64 KiB, 256 KiB with a **test** custody, never the owner's keyring |
| M29 | Direct write                 | write transaction from `preparing` to `applied`, excluding the settling time, which is reported as configured                                                                            | BC-07 files of 4 KiB, 64 KiB and 4 MiB, on the fixture filesystem                    |
| M30 | Start-up repair              | Work and time of the configuration-intent repair at start, with full stores and 0, 1 and 64 open intents, and its effect on M01                                                          | Fixture stores at their bounds                                                       |
| M31 | Long-run stability           | Slope per hour of Console proportional set size, open descriptors, threads and page timers over eight hours                                                                              | 16 terminals: 4 printing intermittently, 12 idle; tabs switched every minute         |
| M32 | Interface interaction        | Input to next frame for: switching views, opening the palette, one keystroke of palette filtering, the Sessions list of 50 rows, the Configuration view's first paint from its inventory | Fixture data at the page bounds (50 rows, 256 items, 2,000 palette entries)          |
| M33 | Scrolling                    | Share of frames over 16.7 ms while scrolling 5,000 lines of scrollback by page keys and by wheel                                                                                         | BC-01 and BC-02 in scrollback                                                        |

The reproducible corpus families are:

| ID    | Workload                                                                                                                    |
| ----- | --------------------------------------------------------------------------------------------------------------------------- |
| BC-01 | Plain ASCII, 80-column lines, 64 MiB                                                                                        |
| BC-02 | Dense colour: 256-colour and true-colour attributes on every cell, 16 MiB                                                   |
| BC-03 | Wide characters: CJK, combining marks and emoji, 16 MiB                                                                     |
| BC-04 | Very long lines without newlines, 8 MiB                                                                                     |
| BC-05 | Full-screen redraws with cursor addressing, as a full-screen program produces them                                          |
| BC-06 | 50,000 lines with markers at known positions and counts                                                                     |
| BC-07 | Configuration trees of 10, 100 and 1,000 files, counts shaped like the owner's (counts only)                                |
| BC-08 | A fake agent executable: prints its arguments and environment, idles, prints on a schedule, spawns a set number of children |
| BC-09 | Layout files of 1, 4 and 16 panes, with and without remembered agents                                                       |

Record exact generated bytes, seed and digest for comparisons. Existing `benchmarks/`
scripts own executable fixtures; this catalogue does not claim every benchmark is
implemented or qualified. Use test custody for synthetic cryptographic costs; actual
wallet/dialog costs are separate authorized measurements.

## Release and operational acceptance

Close applicable negative and recovery paths as well as happy paths: stale identities,
external writers, lost acknowledgments, crash-at-dispatch, disk-full, corrupt/newer stores,
missing provider/helper, denied capabilities and partial history. Provider authentication,
extensions and refresh need real build-specific evidence. Recheck source-to-installed-binary
correspondence, live-session preservation, consistent WAL backups, restore/rollback loss
window and physical Android/IME/accessibility/lifecycle journeys. Review parser/B1 and the
proposed Managed decisions without promoting their status. Whole-product acceptance remains
NOT_ACCEPTED / RELEASE_BLOCKED until the [remaining roadmap](../ROADMAP.md) closes or the
owner explicitly changes scope.
