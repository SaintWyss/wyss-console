# ADR 0019: Desktop link to the external authority

Status: Accepted
Date: 2026-10-08

## Decision

The desktop connects to `wyss-control` and only reads. One application-lifetime
`AuthorityService` owns at most one connection, made with the same typed `ControlClient`
the developer client uses, and reduces what the authority answers to a narrow view the
window renders as status.

```text
Svelte
  -> Tauri desktop (one command: authority_state)
  -> AuthorityService (src-tauri/src/authority)
  -> shared ControlClient (src-tauri/src/control/client.rs)
  -> Unix socket
  -> wyss-control
```

`wyss-control` remains the only authority. The desktop makes no authorization or
security decision, holds no canonical state, never opens the authority's database,
never starts the authority and never sends it a mutation. The requests it sends are
`authority/hello`, `authority/status`, `runtime/status`, `audit/read` and
`audit/watch`. An authority that is absent, restarted, killed, incompatible or
malformed changes what the view reports and nothing else: local human terminals do
not depend on it.

This is the read side of the product only. Managed sessions, turns and approvals have
no UI yet, and nothing here prepares, starts, grants or answers anything.

## What crosses each boundary

Between the authority and the desktop: the existing Wyss Control Protocol v1, unchanged.
No method, field, schema or storage version was added or altered.

Between the desktop and the window: one command, `authority_state`, with one optional
parameter, the view revision the caller already has. It returns `AuthorityView` v1:

| Field        | Content                                                                                        |
| ------------ | ---------------------------------------------------------------------------------------------- |
| `revision`   | Grows by one with every change of the view. It is the caller's wait cursor.                    |
| `connection` | `connecting`, `synchronizing`, `connected`, `waiting`, `stopped`.                              |
| `reason`     | Why the last attempt ended: `absent`, `lost`, `incompatible`, `malformed`, `unsafe_path`.      |
| `authority`  | Service and protocol version, restarts seen, durable head, prepared and closed session counts. |
| `agent`      | Adapter, desired and actual runtime state, runtime version, model count.                       |

`authority` and `agent` are present only while `connection` is `connected`. Whenever
the link is not connected they are cleared, so the window never shows facts older than
the connection they came from.

What does not cross: the authority instance id, the stream id, any session, turn, lease
or approval identity, runtime diagnostics, the isolated workspace path, provider method
counters, events and their payloads. Version text is shown only when it is at most 64
printable ASCII characters. The window validates the view field by field and treats
anything unexpected as a failed bridge, not as data.

There is no generic passthrough: the window cannot name a control method, and the
command rejects unknown fields. The capability file gains exactly one permission,
`allow-authority-state`. No Tauri event permission was added: the command is a bounded
long poll (twenty seconds) with its own admission limit of four waiting calls, separate
from the terminal service's blocking permits.

## The shared client

`ControlClient` is reused, not reimplemented. Three additions made that possible:

- `connect_cancellable` takes a flag its owner can set. The handshake and every later
  exchange then end with `transport_error` within one poll interval (50 ms) instead of
  at their deadline. `connect_in_runtime` is the same connection with a flag nobody sets.
- `socket_path` names where an authority of a runtime directory listens. It validates
  nothing; only connecting decides whether what is there may be spoken to.
- A well-formed handshake or status that declares another protocol version now parses
  as `protocol_version_unsupported`, and another storage schema as
  `storage_incompatible`, instead of `invalid_request`. This is client-side naming of an
  answer the client already refused. The wire and the authority are unchanged.

## Link states

| State           | Meaning                                                               | Leaves on                                        |
| --------------- | --------------------------------------------------------------------- | ------------------------------------------------ |
| `connecting`    | Looking for the socket and exchanging the handshake.                  | handshake → `synchronizing`; failure → `waiting` |
| `synchronizing` | An authority answered; its state and missed events are being read.    | snapshot → `connected`; failure → `waiting`      |
| `connected`     | The view is canonical as of its head and the stream is being watched. | failure → `waiting`                              |
| `waiting`       | The last attempt ended. The next one starts after a bounded delay.    | delay elapsed → `connecting`                     |
| `stopped`       | The application is closing. Terminal.                                 | never                                            |

Every state but `stopped` goes to `stopped` on shutdown. The transition function is a
total table and is tested exhaustively; a transition outside it is ignored in release
builds and asserts in debug builds.

Failures are classified once:

| Reason         | Cause                                                                                                         |
| -------------- | ------------------------------------------------------------------------------------------------------------- |
| `absent`       | No socket, a refused connection (a killed authority leaves its socket behind) or a handshake nobody answered. |
| `lost`         | An authority that had answered stopped: end of file, a frame cut short, a timeout, `storage_unavailable`.     |
| `incompatible` | `protocol_version_unsupported` or `storage_incompatible`, from the peer or from parsing its answer.           |
| `malformed`    | Anything else that breaks the protocol: a bad frame, a wrong correlation, a skipped sequence, another stream. |
| `unsafe_path`  | The runtime directory or socket is not this user's private one, or the peer is another user.                  |

A missing socket is `absent` before any validation runs: an authority that never ran
left nothing to validate. Everything that exists is validated by the client exactly as
before.

## Reconnection

The delay before the next attempt starts at 250 ms, doubles and never exceeds 5 s. It
resets when a snapshot is published. The delay is a cancellable wait, not a sleep. An
attempt never creates, removes or changes anything in the runtime directory.

The reason of the last failure stays in the view through the next `connecting`, until
an authority answers, so the status does not flicker between attempts.

## Instance and stream tracking

The link remembers, across connections, the authority instance and stream it last
greeted and the last sequence it received on that stream.

- A different instance on the same stream is a restart: `restarts` grows and the cursor
  is kept. Within one connection the client already rejects an answer from another
  instance.
- A different stream is another store: the cursor means nothing there and is dropped.
- A status that names another stream than the handshake did ends the connection.

## Snapshot, replay and watch

The view is never derived from events. It is always a canonical reading:
`authority/status`, then `runtime/status`. The durable stream only says when to read
again and proves that nothing was skipped. Three rules close the race between a
snapshot and the events around it:

1. The head is read before what it describes, so a snapshot is never older than its head.
2. The cursor only moves by events that were received, one sequence at a time.
3. A snapshot is published only when its head is at or past the cursor.

An event committed before the head was read is already in the snapshot. One committed
after it is past the cursor, so `audit/watch` returns it and the snapshot is read again.
There is no third case.

On a connection:

1. Read a snapshot.
2. Place the cursor. With no cursor for this stream it goes to the snapshot's head.
   With one, the events after it are replayed with `audit/read`, fifty at a time, until
   the cursor reaches the head. A gap longer than 2000 events, or a cursor past the
   head, is not replayed: the cursor moves to the head, which the snapshot describes.
3. If replay ran past the snapshot's head, read the snapshot again (rule 3).
4. Publish, then `audit/watch` from the cursor. When it returns, receive its events and
   read the snapshot again, whether or not anything arrived.

A received sequence that is not exactly the cursor plus one is a broken stream: the
connection ends as `malformed` with the cursor where it was, and the next connection
replays from there. This mirrors the authority's own invariant that a page is contiguous
from its cursor.

An empty watch still re-reads the snapshot. That bounds how stale a change that writes
no event can be: twenty seconds (the watch wait), one second while the runtime is
starting, initializing or stopping. `Initializing`, for instance, has no event.

If the authority answers `capacity_exceeded` because its four watcher slots are taken,
the link reads with `audit/read` instead and pauses one second between empty reads, on
the same connection.

## What the authority pays

A connected desktop holds one of eight client slots and, while it waits, one of four
watcher slots. It sends a request at least every twenty seconds, inside the authority's
five-second idle expiry only because a watch is always in flight. It keeps no
subscriber state on the authority and consumes nothing.

## Shutdown

`ExitRequested` closes the terminals first and then the link. Stopping the link sets its
flag, which cancels the exchange in flight, publishes `stopped` and joins the worker
(bounded to three seconds; it answers within one poll interval). The socket closes and
nothing is sent: the authority sees an end of file. A waiting `authority_state` call
returns at once with the stopped view.

## Human terminal independence

The link is a separate managed state with its own thread, its own admission counter
and its own frontend loop. No terminal command reads it and it reads no terminal state.
In the window, the terminal workspace and the authority follower share nothing; a
failed `authority_state` call changes one pill.

Authority unavailable is not terminal unavailable. That was accepted on the real
desktop with the authority absent, starting, restarting, killed, incompatible and
malformed (see Acceptance evidence).

## Frontend

`followAuthority` keeps one `authority_state` call in flight and passes the last
revision it saw. A failed or invalid answer shows `Unavailable` and is retried after one
second; an unchanged answer is followed by a 250 ms floor so no bridge behavior can
turn the loop hot; a `stopped` view ends it.

The top bar shows three statuses: `Terminal` (the local PTY, formerly labelled
`Runtime`), the managed agent (`Codex`, or `Agent · Unknown` while the authority is not
connected) and `Authority`. The session summary and the event head are the authority
pill's tooltip; the count is also inline above 1040 px. As the window narrows, status
steps back before the view title does: the version at 940 px, the agent pill at 800 px,
the authority pill at 560 px. Each width is where the widest labels stop fitting; the
default 960 px window shows all three statuses and the version. The existing tokens and
pill styles are used as they were; no token was added.

## Failure semantics

| Situation                       | Link                        | Shown                                  |
| ------------------------------- | --------------------------- | -------------------------------------- |
| No authority                    | `waiting` / `absent`        | Authority Offline, Agent Unknown       |
| Authority starts                | `connected` within 5 s      | Connected, real agent state            |
| Authority exits (SIGTERM)       | `lost`, then `absent`       | Reconnecting, then Offline             |
| Authority killed (SIGKILL)      | `lost`, then `absent`       | Reconnecting, then Offline             |
| Authority restarts              | `connected`, `restarts` + 1 | Connected; missed events replayed once |
| Another protocol or schema      | `waiting` / `incompatible`  | Incompatible; nothing read             |
| Malformed frames or correlation | `waiting` / `malformed`     | Rejected; nothing shown                |
| Socket lost, same instance      | `lost`, then `connected`    | Reconnecting, then Connected           |
| Runtime directory not private   | `waiting` / `unsafe_path`   | Unsafe; no handshake sent              |
| Desktop closes                  | `stopped`                   | —                                      |

## Limits and residual risk

- The boundary is the authority's: same-UID `SO_PEERCRED`. A process of the same user
  can impersonate an authority on the socket path when none is running. It can then
  show false status in the top bar. It gains nothing else: the desktop sends it only
  reads and derives no permission from what it answers.
- An authority that hangs without closing its socket is noticed when the watch deadline
  passes: up to twenty-five seconds. A handshake nobody answers takes five.
- A change that writes no durable event is stale for up to twenty seconds, one while
  the runtime is settling.
- With the watcher slots full, the desktop polls once a second and events arrive up to
  a second late.
- The delay resets as soon as a snapshot is published, so a peer that synchronizes and
  then fails every time is retried every 250 ms. That is the floor, and it is bounded.
- A newer authority whose handshake result has a shape this build cannot parse reads as
  `malformed`, not `incompatible`. Only a declared version mismatch is named precisely.
- Replay is bounded to 2000 events. Past that the desktop does not see the events in
  between; the view is still canonical. Nothing in this phase consumes event content.
- The non-Linux build of the bridge is a stub that reports `stopped`. It was not
  compiled here: the authority, and this project's validation, are Linux-only.
- The link, like the fixes of ADR 0018's closure, was not reviewed by a party other
  than the session that built it. Its tests were checked with mutation controls
  instead (see below), which is not the same thing.

## Deferred

The managed-sessions UI, any mutation from the desktop, a session list or event feed in
the projection, turn and approval views, authority autostart or supervision, and
packaging. None of them is implied by this link.

## Acceptance evidence

Accepted on Linux (CachyOS, KWin 6.7.5), with every authority in an isolated temporary
XDG directory. No production state was read or written and no real Codex was launched.

**Link, against a scripted authority that speaks the real wire protocol**
(`src-tauri/src/authority/tests.rs`). The link sends one request at a time, so a commit
attached to "the n-th request of a method" lands at one exact point of its sequence in
every run:

- A commit injected at each of seven points of the first synchronization and each of
  nine points of a reconnection with a two-page replay is seen exactly once: in the
  snapshot when it precedes the head read, in the stream otherwise. In these tests an
  empty watch waits its full thirty seconds, so a lost update fails the test instead of
  being repaired by the periodic re-read.
- Absent, started later, restarted (new instance, same stream, sixty missed events
  replayed in two pages), killed with a socket left behind, socket lost on the same
  instance, another stream, a gap over the replay bound, a head behind the cursor.
- Incompatible: a refused handshake, another envelope version, another declared
  protocol, another storage schema. Malformed: not JSON, an oversized length prefix, a
  wrong request id, a result of another kind, a wrong instance, a wrong stream, a
  truncated frame, a skipped sequence. A silent peer. Full watcher slots. An unsafe
  socket mode, an unsafe directory mode, no runtime directory.
- Shutdown cancels a parked thirty-second watch and an unanswered handshake in under
  two seconds and sends nothing. Twenty-four reconnections keep one live connection and
  an exact cursor.

Ten mutations of the link were each killed by this suite: reading the runtime before
the head, publishing a snapshot behind the cursor, accepting any sequence, replaying
from the start on first sight, not re-reading after an empty watch, moving the cursor
by the head instead of by events, keeping the cursor across streams, keeping stale data
on failure, a shutdown that does not cancel, and a socket leaked on every reconnection.

**Link, against the real `wyss-control` process** (`src-tauri/tests/authority_process.rs`):
absent to connected to live events committed by a second client, with the authority's
head equal to that client's commits alone; SIGTERM, restart and SIGKILL with state
kept and the one event committed around a reconnection received exactly once; twenty-four
alternating SIGTERM and SIGKILL restarts with the process holding the same descriptors
and one link thread at every step, none after shutdown, and the authority still serving
other clients; a connected link holding exactly one of the four watcher slots.

**Window** (`tests/authority.test.ts`, `tests/app-shell.test.ts`): the view parser
against the same fixture the Rust serialization is compared with, thirty rejected
shapes, every link state and agent state mapped, the follower's cursor, single flight,
retry and stop, and the terminal workspace created once and untouched while the
authority is offline, unavailable, answering a non-view, silent, lost and back.

**Real desktop.** The release `wyss-console` was run on a nested X11 display managed
and composited by KWin/X11 (rootful Xwayland on a headless compositor, so no input
reached the developer's session), with an isolated home, against the release
`wyss-control` and the repository's Codex runtime fixture as its `codex`. Thirty checks
passed, with captures of each state:

- No authority: `Authority Offline`, `Agent Unknown`, terminal running and typing.
- Authority started: `Connected`, one kernel-visible connection, one more socket and no
  more threads than before. A session prepared by the developer client moved the count
  to `1 session`; `runtime/start` moved the agent from `Stopped` to `Ready`. The
  authority's head was exactly the developer client's three commits.
- With the authority connected: typing, paste of the exact clipboard text, a second
  tab with its own shell, switching back, a PTY resize from 26×81 to 30×99 on window
  resize, scrollback after 400 lines, Ctrl+C interrupting a foreground job, closing a
  tab.
- SIGTERM: `Reconnecting`, then `Offline`, no connection held, typing unaffected;
  restart: `Connected` with state kept. SIGKILL: socket left behind, `Offline`, typing
  unaffected; restart: `Connected`. Ten more alternating restarts: the same sockets and
  one link thread as when first connected. In a separate run with an idle desktop,
  twenty restarts left the process at exactly the 47 descriptors, 11 sockets and one
  link thread it had before them.
- A peer refusing the protocol version: `Incompatible`. A peer answering a frame that
  is not JSON: `Rejected`. Each saw only handshakes, the terminal kept typing, and a
  real authority was found again afterwards.
- The window at ten widths from 1100 to 330 px: the view title stays whole down to
  540 px and status steps back in the documented order.
- Closing the window ended the desktop without a panic and left the authority running,
  with no connection and its runtime still ready.
- `_KDE_NET_WM_BLUR_BEHIND_REGION` was set on the window under the composited KDE
  session: the native glass mode was selected. The blur itself was not looked at on a
  GPU session, and nothing in `desktop/glass.rs` or window creation changed.

## Later implementation status

Since ADR 0020 the desktop also sends three managed-session intents. They travel over
separate short-lived connections owned by another service; the link described here is
unchanged, still sends only the five reads above, and is what wakes the Sessions view.
"The desktop only reads" above describes this link, no longer the whole desktop.
