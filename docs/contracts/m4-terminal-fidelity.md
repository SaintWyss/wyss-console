# M4 contract — persistent terminal fidelity and lifecycle

Status: engineering contract for [M4](../product/roadmap.md#m4--persistent-terminal-fidelity-and-lifecycle);
decisions in [ADR 0057](../adr/0057-terminal-screen-parser-helper.md) (proposed). Test oracles are named so that a
change to any line here changes a test that says so.

## 1. What a window may ask the host

All are protocol-1 additions announced in `hello.features` (`screen_snapshot`, `lingering_control`); a host
without the name answers `unknown_op` and the window replays the ring as before.

| Operation             | Request                                | Answer                                                                                                                                                                                                                                         | Oracle                                                               |
| --------------------- | -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `term_screen`         | `{terminal_id, scrollback_lines≤1000}` | `available: true` with `repaint` (base64, ≤ 600 KiB), `at` (the output offset where it ends), `size`, `alt_active`, `title`, `history{included, omitted, evicted}`, `refused{…counts}`, `parser`; or `available: false` with a closed `reason` | `host::tests::screen::a_late_client_receives…`, `…without_a_helper…` |
| `term_parser`         | `{terminal_id}`                        | `health`, `fidelity`, `missing_bytes`, `restarts`, `last_failure`, `sandbox{layers, landlock_abi}`, `helper_pid`, `fed`                                                                                                                        | `host::tests::screen::killing_the_parser_helper…`                    |
| `term_read` (changed) | as before                              | as before plus `parser{health, fidelity, restarts}`: the status rides on a read the window already makes                                                                                                                                       | `tests/terminal-attach.test.ts`                                      |
| `term_list` (changed) | as before                              | each row also carries `parser`                                                                                                                                                                                                                 | `host::tests::screen::without_a_helper…`                             |
| `host_persistence`    | none                                   | what survives what, from facts read on this machine (§4)                                                                                                                                                                                       | `host::tests::lifecycle::…`, `host::persistence::tests`              |
| `host_lingering`      | `{enable, confirm}`                    | the same facts after the change, read back; `confirm` must be exactly `enable-lingering` / `disable-lingering`                                                                                                                                 | `host::tests::lifecycle::lingering_is_changed_only_by…`              |

Desktop commands (each has its Tauri permission): `terminal_attach` calls `term_screen`, validates the answer
(sizes 1..512, repaint within its bound, an offset) and **stages** it with a one-use `token`, answering `mode: screen`
or `mode: replay`; it moves nothing. `terminal_attach_commit {terminal_id, token, restored}` is the second half: the
read position moves to where the screen ends only when the window says it **drew** it (`restored: true` with the
token handed out last); `restored: false`, or no token because the answer could not be read, sends the reader back to
the start of what the host kept, so a screen that failed to draw loses no output. A window that cannot tell the host
it drew the screen does not read on and shows the terminal as failed. `host_persistence`, `host_lingering`.

## 2. Invariants

1. **One size.** The PTY's. `term_resize` sets it and records the output offset where it happened; the parser is resized
   exactly there, between the bytes written before and after (`a_resize_takes_effect_between_the_bytes…`).
   A hidden pane never fits or resizes (`tests/terminal-sessions.test.ts`, existing).
2. **Exact or labelled.** A screen is `exact` only if the parser consumed every byte since the terminal began;
   after a restart from a ring that no longer starts at byte 0 it is `tail{missing}` and the window says so.
3. **Never replay to the program.** Nothing the parser, the link or an attach does writes to the PTY
   (`attaching_again_and_again_creates_nothing…` counts the executions of a typed command).
4. **Failure stays local.** A helper that dies, hangs, lies, exceeds a limit or is killed costs the screen state of
   that terminal only; the PTY, its siblings and the host continue (`killing_the_parser_helper…`, the real-process
   `host_process_survival` test, `screen::link::tests`).
5. **Output is data.** Clipboard, hyperlink, window-operation, query and string sequences are counted and dropped; a
   repaint contains none of them and the window's renderer has no handler that could act (`a_repaint_contains_nothing…`,
   `has_no_way_for_output_to_write_the_clipboard…`).
6. **Bounded and local.** Screen ≤ 512×512, history ≤ 20,000 lines and ≤ 256 Ki cells, title ≤ 128 characters, OSC ≤ 1 KiB,
   strings ≤ 4 KiB, repaint ≤ 4 MiB (host answer ≤ 600 KiB), frame ≤ 256 KiB in / 5 MiB out, helper address space
   ≤ 384 MiB, descriptors ≤ 8, restarts ≤ 3 per 60 s.
7. **No polling.** An idle terminal's link thread has no wake-ups (`a_host_with_an_idle_terminal_waits…`).

## 3. Failure words

`reason` of `available: false` and `parser.reason`: `helper_missing`, `host_without_screen`, `timeout`, `too_large`,
`busy`, `stopped`, `spawn_failed`, `restart_budget`, `refused`, `exited`, `fell_behind`, `write_failed`,
`bad_frame_or_exit`, `bad_ack`, `bad_hello`, `sandbox_unavailable`, `disconnected`. None carries bytes of the
terminal's output or of the request.

## 4. What survives what

Computed, not asserted: `host_persistence` reads the host's cgroup (user manager or session scope), logind's
`KillUserProcesses` and the lingering record, and reports each of: closing the window, leaving a terminal, locking
the screen, logging out with lingering on and off (the row that matches this machine now is marked), suspend and
resume, the host stopping, reboot or power-off. Closing the window, detaching, locking and suspending leave the
terminals running; the host stopping and a reboot or power loss **end** them, and **nothing relaunches a command**.
Logout depends on the facts above and may be reported as not certain. Each row says how it is known.

## 5. Layout files

`layout_write` takes an exclusive lock (a file beside the layout), looks at the layout without following a link and
without trusting its size, and never replaces a file written by a newer version (`layout_newer`). An unreadable one,
or one longer than 1 MiB, is kept beside the live one (`layout.json.unreadable-<seconds>[-n]`, never over an earlier
one, three at most) before a usable layout is written; a link, a special file or someone else's file in its place is
refused and left alone. The file is judged again just before it is replaced and the write is abandoned if it is not
the one judged. A writer that does not use the lock can still change it in the one `rename`; that is the limit
(`host::layout::tests`). Reading an unreadable or newer layout gives an empty, usable view.

## 6. Out of scope here (owner or later milestones)

Accepting ADR 0057; installing the helper (it is in the package; nothing is installed by this branch); turning
lingering on; reflow on resize; the Unicode 11 table (needs the window's renderer to load it); a gentler refusal of
an oversized paste (today the terminal fails visibly and sends nothing, as accepted earlier).
