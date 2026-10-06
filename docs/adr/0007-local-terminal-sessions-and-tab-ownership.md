# 0007 — Local terminal sessions and tab ownership

Status: Accepted

Date: 2026-10-06

## Decision

Sprint 1C adds a focused frontend `TerminalSessionManager`. Each local human-terminal
session has one `ConsoleSessionId` (the existing branded local UUID), one returned
`TerminalId`, one controller and one xterm renderer. These identities are independent;
neither PID, DOM identity nor insertion index identifies a terminal resource.
`ManagedSessionId` is not used. This is Console-local presentation ownership, not
managed-agent state or authority. ADR 0002 remains unchanged.

The manager coordinates startup, creation, activation, close and disposal. Controllers
retain the existing six-command desktop client and exact per-resource IO ownership.
The tab UI contains no PTY IPC logic. A workspace lease retires the previous manager
before starting a replacement, including development remounts; every keyed session
panel attaches its renderer/controller once. HMR cleanup remains best effort.

## Creation and limits

Startup creates exactly one terminal. Creation admission is serialized until its
backend request resolves; repeated requests during creation do not allocate additional
sessions. Titles use monotonic creation order (`Terminal 1`, etc.). At most 16 session
panels are retained, including starting, exited, failed and closing panels. This
conservative frontend bound preserves the backend's separate 16-resource bound.
Reaching it displays an error and leaves existing terminals intact. Closing a retained
panel releases its slot; resources are never silently recycled.

A failed create/renderer remains an explicitly failed, closable tab, not an indefinitely
loading ghost. It exposes only a concise error and does not block subsequent creation.
Closing during creation stops input immediately; a late returned TerminalId is closed
once before removing its panel. Closing before DOM attachment creates no backend
resource. No delayed callback can transfer ownership to another session.

## Visibility, IO and geometry

The horizontally scrollable tab strip reveals the active tab and close control on
selection and container resize, without moving keyboard focus.

Every session panel stays mounted until closed. Switching changes visibility, never
controller/renderer identity. All live controllers continue consuming byte batches,
including hidden sessions: PTY → bounded backend → one IPC batch → xterm processing
callback → next pull/ACK. ADRs 0005/0006's bounds are unchanged. There is no manager
transcript cache; each renderer has 5,000 volatile scrollback lines plus its bounded
screen. Background shells remain subject to normal bounded transport backpressure,
not tab visibility.

Only the active controller drives fit/resize. Deactivation cancels pending fit and
unsent desired dimensions; an already admitted resize may finish. Hidden sessions
retain their previous geometry. Activation schedules a measurable fit and focus using
the existing 60 ms coalescing, skipping duplicate sizes. It never fits a hidden panel.

Hidden panels/hosts are inert, and their renderer is blurred. xterm's public custom
key handler plus DOM input/paste/composition guards reject hidden user input. Its
`disableStdin` option is reserved for lifecycle input shutdown: disabling it for hidden
running shells would also suppress emulator protocol replies needed by applications.
Those replies continue to the controller's own TerminalId. Accepted input already
queued before switching can finish only on that same ID; it is never redirected to
the later active session. Output likewise uses each controller's fixed ID.

## Close, exit and shutdown

Close stops input/IO synchronously, invokes the controller's memoized cleanup, disposes
the renderer/listeners/observer, then removes that session. Repeated manager closes
share the same promise. Closing an inactive tab leaves selection intact. Closing the
active tab selects the first surviving tab to its right, otherwise the nearest left
one; closing the final tab leaves an empty workspace with an explicit New Terminal
action. There is no automatic replacement after intentional close.

Natural exit retains the tab and final screen, marks it exited, disables input and
stops its output loop after final drain. Other sessions remain unaffected. Failed
sessions are isolated similarly. Normal graphical exit keeps the existing Rust
shutdown of every registered local resource; workspace disposal also owns all its
controllers, not just the selected one.

All session metadata, selection, order and scrollback are volatile. Restart creates
fresh identities and shells. Human terminal survival across Console process crash is
not guaranteed at this stage. No persistence, Event Bus, lifecycle EventEnvelope
producer, capability/CSP expansion or managed-agent authority is introduced.

## Deferred

Split panes, drag ordering, persistent sessions/restoration, dynamic shell titles,
global terminal shortcuts, profiles, settings, managed-agent sessions and control-plane
integration remain **deferred**.
