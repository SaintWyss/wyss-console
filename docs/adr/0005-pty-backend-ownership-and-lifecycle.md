# 0005 — PTY backend, ownership and lifecycle

Status: Accepted

Date: 2026-10-06

## Decision and evidence

Sprint 1A uses `portable-pty 0.9.0`, with default features disabled and no optional
features. It is the focused PTY abstraction used by WezTerm, supplies spawn,
reader/writer, resize, child exit status and Unix foreground-group access. The
existing base specification §5.3 and research note G identify it as a reuse
candidate. `alacritty_terminal`, `wezterm-term` and libghostty are emulation/state
candidates, not necessary for this backend sprint.

- [portable-pty API](https://docs.rs/portable-pty/0.9.0/portable_pty/)
- [Unix backend source](https://docs.rs/portable-pty/0.9.0/src/portable_pty/unix.rs.html)
- [Tauri application command ACL](https://docs.rs/tauri-build/2.7.1/tauri_build/struct.AppManifest.html)

Local compatibility is verified with real Linux PTYs and the account's `/bin/fish`.
`nix 0.28.0` matches portable-pty's existing dependency and adds focused polling,
nonblocking fd configuration, signal/process-group and account lookup APIs. Its
direct features are `fs`, `poll`, `signal`, `process`, `user`; portable-pty also
requires `term`. No new async runtime or terminal renderer is added. UUID's `v4`
feature supplies logical IDs and short-lived output receipt tokens.

## Local ownership and concurrency

The one application crate contains a Tauri-independent `terminal` service with
an in-memory registry. `TerminalId` identifies a logical local human terminal;
`observed_pid` is an OS observation, never its identity or authorization.
Neither this registry nor shell execution owns managed-agent authority.

Each terminal has one dedicated worker owning PTY handles and the child. The
worker uses nonblocking master IO and `poll` with a wake socket; the 100 ms timed
poll also observes child exit, without a spinning reader or one task per chunk.
Output reads, writes, resize and process operations run outside the UI runtime.
Create/write/resize/close commands cross `spawn_blocking`; a 32-operation admission
limit is applied before enqueueing blocking work. Status/output calls perform only
short, bounded in-memory operations. Registry locks never cover PTY IO or waits.

Up to 16 resources, including creation/exited-undrained resources, are admitted.
Explicit close removes the runtime and retains at most 128 small closed-status
tombstones. Older tombstones expire to `unknown_terminal`. Late completed closes
and abandoned creation attempts reclaim slots on subsequent creation.

## Shell, cwd and size

Resolve the configured account shell through UID/account lookup first, then an
available absolute executable `$SHELL`, then `/bin/sh`. The frontend supplies no
executable, argv, environment, PID or process group. The shell is spawned directly
with `-i`, without an extra shell wrapper. Its executable and resolved cwd are
returned as local diagnostics; shell configuration is not modified.

Explicit cwd must canonicalize to an existing directory; failure never falls back.
An omitted cwd defaults to the account home directory. Rows/columns must be
1–512; initial dimensions are explicit. Pixel dimensions are zero until a renderer
provides a concrete requirement. Non-UTF-8 native cwd names can be used internally;
the current diagnostic display is lossy, not an exact native-path wire contract.

Without a renderer, `TERM=dumb` correctly avoids claiming xterm capabilities.
Local fish testing with `xterm-256color` blocked on emulator capability queries.
Sprint 1B must select TERM consistently with its real renderer and test the
handshake. This is not a shell-profile customization or backend ANSI parser.

## Transient byte transport and bounds

PTY output is bytes, not text lines, UTF-8 fragments or ANSI tokens. No terminal
bytes are EventEnvelope events or durable data. The frontend bridge returns
`Uint8Array`; JSON IPC encodes each byte as an integer 0–255. No UTF-8 conversion
occurs in transport. Only the removable diagnostic uses a streaming TextDecoder
for its text preview, where invalid UTF-8 can appear as replacement characters.

Per terminal:

- reader scratch buffer: 4 KiB;
- output ring: 64 KiB, with a fixed logical capacity;
- output pull: at most 16 KiB, one unacknowledged batch;
- input request: 1–4,096 bytes;
- control queue: eight requests, with at most one pending write;
- diagnostic text: last 8,192 UTF-16 code units, no stored history.

When the output ring fills, the worker stops reading the PTY. Kernel buffers are
finite, so producers eventually block. Close/control wakeups remain independent
of a full output ring. No producer task or unbounded content queue is created.

`terminal_read_output` initially takes `ack: null`. A nonempty batch carries a UUID
`batch_id`. Only the matching acknowledgement in the next pull releases its credit;
missing/stale receipts reject. Empty batches have null receipts. The frontend bridge
coalesces concurrent pulls and returns credit on its next sequential read. Thus a
stalled consumer can retain at most one output batch plus the bounded backend ring.
Even the JSON expansion of a batch is bounded (at most roughly 64 KiB plus metadata).
The one-consumer pull/receipt protocol is a transient transport, not event replay.
A lost response does not silently resend or grow history; explicit close remains
available. Sprint 1B must preserve these bounds when connecting a renderer.

## Lifecycle and cleanup

Internal creation starts at `starting`, becomes `running` after spawn, and returns
structured `spawn_failed` / `internal_pty` on failure. Input is transported without
interpreting commands. Unknown or exited/closing/closed IDs reject mutations.
Write deadlines are one second, with a two-second caller wait; partial-write
errors include accepted byte counts so callers must not blindly resend all input.
Full queues report `busy`; expired queued operations are not executed.

Natural exit is observed and reaped, producing `exited` with exit code/signal.
The worker drains at most 16 reads of up to 4 KiB, within remaining output
capacity, and releases PTY handles. Concurrent consumers cannot extend this budget.
If extra bytes must be discarded at exit, `truncated` marks the loss. Buffered
output remains readable until explicit close; completed resources require close
for normal registry removal.

Explicit close atomically stops new input admission, wakes the worker, cancels
pending writes, sends SIGHUP to the foreground and shell groups, and releases all
master reader/writer handles. In-flight input can already have reached the kernel.
After 500 ms without child exit it escalates to SIGKILL, including the direct shell.
The worker reaps the child before completion and joining; close waits at most two
seconds. Repeated/concurrent close reports `already_closed`, with only one successful
close. Remaining buffered output is deliberately discarded by close.

If the kernel delays exit beyond the deadline, close returns `close_timeout`; the
bounded worker/resource remains tracked and continues reaping. This does not claim
an impossible guarantee about uninterruptible kernel processes. Its eventual
closed slot is reclaimed before later creation. Process-group cleanup is local
terminal cleanup, not containment of programs deliberately detached from the TTY.

Graceful graphical exit requests shutdown for all local terminals together and
waits off the UI thread under a shared two-second deadline. Hiding/collapsing the
diagnostic surface does not request termination. Abrupt process death is different:

**Human terminal survival across Console process crash is not guaranteed in Sprint 1A.**

There are no managed-agent sessions here. ADR 0002's independent authority and
managed-agent crash/restart invariant remain unchanged.

## IPC and deferred mechanisms

Application ACL registers and grants only `get_app_info`, `create_terminal`,
`terminal_write`, `terminal_read_output`, `terminal_resize`, `close_terminal`, and
`terminal_status` to the local main window. DTOs reject extra operation fields.
No shell/filesystem/process plugin, arbitrary spawn, remote origin or network
permission exists. Errors serialize a small code and partial byte count; internal
diagnostics are excluded from frontend serialization.

Lifecycle metadata is currently a service snapshot, not an EventEnvelope producer.
A lifecycle producer and any Event Bus remain **deferred**. Also deferred: xterm.js,
ANSI rendering, tabs/panes, scrollback UI/storage, terminal restoration, persistent
PTYs, external authority, agents, adapters, permissions and routing. The diagnostic
component and smoke helper are removable when Sprint 1B provides the renderer.
