# ADR 0057 — A parser helper per terminal: the screen, kept apart from the PTY

Status: **Proposed; not accepted** · Date: 2026-10-24 · Owner: Santiago

The engineering below is built and tested on this branch. Accepting this ADR is what would let a release
rely on it; nothing here changes an installed host, a unit file or a user setting.

## Context

The host (F9) owns each terminal's PTY master and keeps a 512 KiB byte ring, so that a window that opens
later can read "what was printed". That is not a screen: a window that attached to `vim`, `less` or `htop`
that had been running for an hour drew only the tail of the stream into a blank terminal and then nudged the
PTY size to make the program redraw. Modes set long ago (the alternate screen, mouse reporting, bracketed
paste), the cursor, scroll regions and what the screen looked like were not known to anyone but the program.

M4 asks for the screen state to be kept, bounded and honest about what is missing, without making a parser bug
or a hostile byte stream able to end a terminal.

## Decision (proposed)

One **parser helper process per terminal** (`wyss-screen`), started and supervised by the host. The host keeps
every PTY master and the byte ring; the helper receives the ring's bytes through a typed pipe protocol,
keeps the screen, and answers snapshots. A snapshot is **a byte stream that redraws the screen** (the
"repaint") plus the offset in the terminal's output where it ends; a window that attaches draws it and then
reads on from that offset.

| Property            | What it is                                                                                                                                                                                                                                                         |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Failure domain      | The helper is a separate process. It owns no PTY, no credential and no authority and answers nothing but the typed frames below. Its death costs the screen state, never the terminal or its siblings.                                                             |
| Source of truth     | The host's ring. After any failure a new helper is fed the ring from its start and the link says whether that is **exact** (the ring starts at byte 0) or a **tail** (`missing` bytes before it are not in the screen). It never claims more.                      |
| Size                | One authority: the PTY's. The helper's size follows what the host last set it to, by value (a command that could be lost is not used), applied after the bytes that preceded the change.                                                                           |
| Output is data      | The screen engine acts on nothing it parses. A clipboard write (OSC 52), a hyperlink (OSC 8), a window operation (CSI t, DECCOLM), a query that wants an answer, and device/application strings are **counted by closed name and dropped**. Repaints contain none. |
| Replay              | Bytes are replayed into the parser only. Nothing is written to the PTY by reattaching, restarting a helper, or a snapshot; no command or prompt is repeated. A test counts the executions.                                                                         |
| Where it is drawn   | The window's own terminal (xterm.js) draws the repaint and parses the live stream itself. The width table of the engine is therefore xterm.js's (`UnicodeV6`), ported with its source; a second table (modern/tmux-like) exists for the reference corpus.          |
| Failure of the link | A host without a helper (not found, too weak a sandbox, restart budget used) answers `term_screen` with `available: false` and a closed reason; the window replays the ring as before and **says so** ("Output replayed · no screen state was kept").              |

### The protocol (`screen::ipc`, version 1)

Binary frames on the helper's standard input and output: 4-byte big-endian length, 1-byte kind, payload. Strict:
an unknown kind, a short or long payload, a frame over its bound, a field out of range is an error and ends the
conversation with a closed word (`Refused{code}`) and exit code 3; nothing is skipped or guessed.

| Direction   | Frame                                                                             | Bound                                                |
| ----------- | --------------------------------------------------------------------------------- | ---------------------------------------------------- |
| host→helper | `Hello` (version, rows, cols, history lines, width table, required layers)        | first frame only; rows/cols 1..512; history ≤ 20,000 |
| host→helper | `Feed` (offset, bytes)                                                            | ≤ 64 KiB; offsets must be contiguous                 |
| host→helper | `Resize`, `Snapshot` (history lines), `Ping`, `Shutdown`                          |                                                      |
| helper→host | `Ready` (pid, layers applied, Landlock ABI), `Fed`, `Snapshot`, `Pong`, `Refused` | a frame ≤ 5 MiB; a repaint ≤ 4 MiB                   |

Requests ≤ 256 KiB. The helper is lock-step: one answer per request.

### Isolation of the helper (`screen::sandbox`)

Applied by the helper to itself **before it reads a byte**; the host names the layers it requires and the helper
refuses to run without them (exit code 4, `SandboxUnavailable`). There is no degraded mode.

| Layer                      | What it does                                                                                                                                                                                                                                                                            | Observed by (probe tests, with the unrestricted baseline)                                                               |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| no-new-privs, not dumpable | cannot gain privileges; `/proc/<pid>/exe` and memory are not readable by other processes of the user                                                                                                                                                                                    | applied bits; the GUI harness could not read `exe`                                                                      |
| rlimits                    | address space 384 MiB, 8 descriptors, no core, no file writes (`FSIZE` 0), CPU backstop                                                                                                                                                                                                 | a 1 GiB mapping fails `ENOMEM`; descriptors stop below 8                                                                |
| Landlock                   | every filesystem access handled and none granted; TCP bind/connect handled (ABI ≥ 4); abstract unix sockets and signals outside the domain scoped (ABI ≥ 6)                                                                                                                             | `open` of a readable file, a create, `/proc/self/environ`: `EACCES`; TCP connect `EACCES`; signal to the parent `EPERM` |
| seccomp (deny-list)        | `socket`, `connect`, `bind`, `listen`, `accept*`, `execve*`, `fork`/`clone`/`clone3`, `ptrace`, `process_vm_*`, `mount`, `unshare`, `setns`, `bpf`, `perf_event_open`, `userfaultfd`, `io_uring_*`, `keyctl`, `memfd_create`, module calls and a few more end the process with `SIGSYS` | each of socket, tcp connect, fork, exec, ptrace ends in `SIGSYS`; the parser's own calls still work                     |

The helper is started with no environment, `/` as its directory, three descriptors, a new session, and
`PR_SET_PDEATHSIG` so that it dies with the host. It is a binary of its own (no window toolkit is linked).

**Not claimed:** a kernel exploit from inside a seccomp+Landlock domain, a same-user attacker reading the host's
memory (that attacker already owns the PTYs), protection of pathname unix sockets on Landlock ABI < 9 (the
helper cannot create sockets at all under seccomp, which is what covers it), or that a deny-list is complete.

### Recovery and what is reported

`Health`: starting, healthy, restarting, unavailable(reason). `Fidelity`: exact, tail{missing}, unavailable.
A helper that ends is replaced at once from the ring; at most 3 restarts in 60 s, then `unavailable
(restart_budget)` for the rest of that window and one more try after it. Each exchange has a deadline (3 s); a
helper that does not answer is killed through the handle the host holds for it. The health and fidelity ride on
every read the window already makes; nothing polls. An idle terminal's link thread sleeps (a test measures its
wake-ups).

### Lifecycle truth (`host::persistence`)

A table, computed from facts read on this machine (where the host runs in the cgroup tree, logind's
`KillUserProcesses`, whether lingering is on), of what each event does to the terminals: window close, detach,
screen lock, logout with and without lingering, suspend/resume, host crash, reboot or power-off. Reboot, power
loss and a host crash **end** terminals and nothing relaunches a command; the others are stated with how they are
known (tested, reasoned, or read from this machine). **Lingering is the owner's**: it is changed only by an
explicit request carrying its confirmation word, runs `loginctl enable-linger` or `disable-linger` for the account
and nothing else (no unit is written, no service restarted), and is read back, not assumed.

## Options considered

| Option                                      | Effect                                                                                                                                                                  |
| ------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **A. A helper process per terminal** (this) | A parser fault is contained and restartable; 16 helpers of a few MiB each. Costs a process per terminal and a protocol.                                                 |
| B. A parser thread inside the host          | Cheapest, but a parser panic or runaway allocation takes the host, and with it every PTY master. Contradicts M4's failure-domain requirement.                           |
| C. A headless xterm.js in a Node helper     | Identical semantics to the page, but pulls a JavaScript runtime into the host's trust base and 30–60 MiB per terminal.                                                  |
| D. An existing Rust terminal-emulator crate | Less code to own, a new dependency in the host's build (and a Wayland/GUI-free subset to audit). The repo's rule is to approve dependencies one by one; none was added. |

## Consequences and what is still open

- Dependencies: **none added.** The sandbox uses the `libc` that `nix` already re-exports.
- A repaint is a faithful redraw of the engine's state, which a differential test compares with tmux on 17 recorded
  real streams (vim, htop, less, man, nested tmux, curses, a shell, wide characters, scroll regions, alternate
  screens, mouse and paste modes, hostile sequences). The differences found were fixed or documented in the
  acceptance record; tmux and xterm.js still differ from each other in places (tab stops, emoji clusters).
- Reflow on resize is not done: the program is sent the size and redraws. Custom tab stops, the G1 charset
  shift and text selection are not carried across an attach.
- Width: the engine uses xterm.js's Unicode 6 table while the window does. If the window later loads xterm's
  Unicode 11 or grapheme add-on, the table must change with it (`Limits::width`).
- **Owner decisions**: accept this ADR; accept that a host without Landlock/seccomp support shows replay
  instead of a screen (no degraded sandbox); whether lingering should ever be suggested at install time (the
  engineering only shows its state and an explicit control, default off).
- Rollback: delete `libexec/wyss-console/wyss-screen` (or stop the helpers): every terminal keeps running and the
  window falls back to replaying the ring. No data is migrated; nothing in a store changes.
