# Terminal ownership and byte flow

Current layered contract: [terminal service](../../src-tauri/src/terminal/mod.rs),
[bounded DTOs](../../src-tauri/src/terminal/contracts.rs),
[host](../../src-tauri/src/host/core.rs),
[desktop client](../../src/lib/desktop/terminal-client.ts),
[controller](../../src/lib/features/terminal/controller.ts) and
[renderer](../../src/lib/features/terminal/renderer.ts).

## Ownership and lifecycle

The backend uses portable-pty; xterm plus FitAddon owns the visible terminal.
The application does not explicitly select a renderer backend or load a GPU addon; do not
infer a measured renderer/backend guarantee from the library name. The separate screen helper maintains recovery state; it does
not own a PTY master or Managed authorization. These are distinct responsibilities.

TerminalId, ConsoleSessionId and ManagedSessionId are different identity domains.
An observed PID is not identity or authority. Each terminal worker owns its PTY,
child and bounded operations outside the UI runtime; registry locks do not cover
PTY I/O or waiting. Closing uses owned handles/groups, never a process-name scan.

The persistent host can outlive the GUI. Detaching a window disposes its view
without closing a surviving host terminal; explicit close releases the resource.
Local fallback cannot promise survival of its own process. Restore/attach does
not authorize a fresh shell or agent launch, and closing the final tab does not
automatically create a replacement. Natural exit keeps its final view until close.

Input admission stops before explicit close; an already admitted write may have
reached the kernel. Close wakes blocked I/O, cancels pending writes, signals owned
foreground/shell groups, escalates/reaps within its bounded procedure and retains
unreaped resources on timeout. Detached hostile processes are not contained merely
by ordinary terminal process-group cleanup.

## Shell, directory and geometry

Resolve the account's configured shell, then an available absolute SHELL, then
/bin/sh. Backend Native recipes and user launch configurations have their own
reviewed paths; the ordinary terminal create DTO is not arbitrary spawn/argv.
Only the child gets TERM=xterm-256color; no global shell profile is changed.

An explicit cwd must be an existing canonical directory; invalid input never falls
back. For omitted cwd, a defined HOME must be absolute and valid. Only absent HOME
falls back to the account directory. Native path diagnostics can be lossy for
non-UTF-8 names and are not an exact native-path wire contract.

Backend rows/columns are 1–512 with zero pixel dimensions; the renderer measures
visible containers and uses at least two columns. Fit coalesces changes, skips
identical sizes and retains only the latest desired resize behind one in flight.
Hidden sessions do not receive user focus/input or an unmeasurable fit.

## Bounded transport

| Layer                   | Current bound/behavior                                                                                                           |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Terminal service        | 16 resources, including starting/exited-undrained ones; eight control requests and at most one pending write per terminal.       |
| Direct terminal I/O     | 64 KiB output ring, 16 KiB output batch, 4 KiB input request, one unacknowledged batch.                                          |
| Persistent host history | Separate 512 KiB byte ring with absolute offsets, independent client cursors and explicit truncation when a reader falls behind. |
| Controller input        | 64 KiB including the in-flight chunk; one write of at most 4 KiB at a time.                                                      |
| Renderer                | Volatile scrollback bounded by appearance settings; no manager transcript cache or durable terminal content.                     |

PTY data stays bytes, not lines or independently decoded UTF-8/ANSI fragments.
The desktop uses Uint8Array. `onData` is UTF-8 encoded and `onBinary` preserves
binary values. Terminal content is never injected as application HTML.

The direct credit path returns a batch receipt only after renderer processing;
one pull/write remains in flight. The host's cursor/history and attach-token
protocol is a different layer and must not be described as an unlimited replay
or as the direct terminal's 64 KiB ring. Slow consumers have bounded behavior;
close/control stays available independently of full output buffers.

Busy may be retried only when the host refused admission and no bytes were
written. Partial/ambiguous writes must not be resent. The generic HostLink currently retries
`host_unavailable` without distinguishing a lost reply; this is a known violation of the
required policy, tracked by [ENG-02](../ROADMAP.md#eng-02). The stricter Projects Owner
transport does not initialize/reconnect/retry that way. Input overload fails visibly,
never grows an unbounded queue or silently redirects bytes to another terminal.
EOF can precede reaping: drain bounded final output and obtain the exit state;
explicit close may discard remaining output and truncation must be disclosed.

## View ownership and display security

Each keyed live session retains its own controller/renderer across tab switches.
Background output/emulator replies continue; hidden user input is blocked
separately because disabling stdin can also suppress required terminal replies.
Already queued input remains bound to its original TerminalId.

Creation is serialized; failed and closing resources count toward admission and
remain explicit, closable states. Late creation after disposal is cleaned up once.
Close/disposal is idempotent; listeners, timers, observers and renderer callbacks
are released without transferring ownership. Development HMR is best effort.

External terminal links do not open destinations. xterm's generated CSS requires
the scoped CSS CSP allowances; this does not permit inline JavaScript, eval,
remote scripts or arbitrary frontend filesystem/process/network access. Appearance
settings and panes now exist; old single-view/no-settings sprint limits do not.

See [screen fidelity](terminal-fidelity.md),
[host persistence](terminal-fidelity.md#4-what-survives-what),
[workspace identity](workspace.md) and [current acceptance](../STATUS.md).

## Project-pane coverage limit

The direct terminal controller's input queue bound and the ordinary snapshot attach path
must not be attributed automatically to the separate Projects SessionPane. Its current
path replays a bounded ring and chains 4096-byte writes without an aggregate input-queue
ceiling. It also needs partial/unknown outcome propagation rather than a universal
"not sent" error. See the [project journey contract](workspace.md#7-journey-semantics-and-implementation-gaps)
and REQ-02/REQ-03 in the roadmap. Source/test inspection is not an executed acceptance result.

## Terminal layout invariants

These TI/PS rules apply to the terminal split tree; the project metadata tree has its
own [closed schema and bounds](workspace.md). They do not replace each other's limits.

| ID  | Invariant                                                                                                                                                                                             |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TI1 | A tab has a root; a tab whose last pane closes is closed                                                                                                                                              |
| TI2 | Every split has at least two children                                                                                                                                                                 |
| TI3 | No split has a child split of the same orientation: such a child is merged into its parent, its children taking shares of its weight                                                                  |
| TI4 | Weights are positive integers summing to 10,000 within each split                                                                                                                                     |
| TI5 | Pane ids are unique in the workspace; every pane is in exactly one tab                                                                                                                                |
| TI6 | A non-empty tab has exactly one focused pane; a non-empty workspace has exactly one active tab                                                                                                        |
| TI7 | At most one pane per tab is zoomed; zoom never changes the tree                                                                                                                                       |
| TI8 | Depth at most 8; terminals in the workspace, ended ones included, at most 16                                                                                                                          |
| TI9 | The tree is the only source of geometry: no pane has a size of its own except what §11.2 computes from the tree and the container                                                                     |
| PS1 | Every **visible** pane's PTY has the size of its drawn grid, within one coalescing period (60 ms) and one round trip after the last geometry change. This is FR-TERM-05 extended to panes             |
| PS2 | A hidden pane (another tab, behind a zoom, constrained display) keeps its last size; it is refitted when it becomes visible, before it takes keys                                                     |
| PS3 | Each terminal has its own coalescing: one resize in flight and the latest desired size. Dragging a divider sends at most one resize per pane per 60 ms, and the final size always after the drag ends |
| PS4 | Sizes are clamped to 1 to 512 rows and columns; a pane whose grid would exceed 512 is padded                                                                                                          |
| PS5 | A new pane's terminal is created at its computed size, so that the first byte the shell prints is at the final size (as built for tabs)                                                               |
| PS6 | A font-size change refits every visible pane; hidden ones on becoming visible                                                                                                                         |
