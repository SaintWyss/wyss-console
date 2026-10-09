# ADR 0020: Managed sessions in the desktop

Status: Accepted
Date: 2026-10-08

## Decision

The `Sessions` view is real. It lists the managed sessions `wyss-control` holds, shows
one in detail, and sends three product intents: prepare a session, resume one, close
one. These are the first mutations the desktop sends, and the only ones.

```text
Svelte            a session it was shown, the revision it saw, the identity of its action
  -> Tauri        four narrow commands, one product failure code
  -> SessionService (src-tauri/src/sessions)
  -> shared ControlClient, one short-lived connection per call
  -> wyss-control  decides
```

The desktop requests and the authority decides. Nothing in the window or in Tauri
decides whether a session may be created, resumed or closed, whether a lease is valid,
whether the runtime permits an operation or whether a close is canonical. Control
Protocol v1 is unchanged: no method, field, schema or storage version was added.

## The session contract as it exists

Nothing below was designed here. It is what the core already does (ADRs 0011, 0013, 0014) and what this phase conforms to.

| Operation                       | Needs                                                             | Result                                                                      |
| ------------------------------- | ----------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `managedSession/prepare`        | An intent, an adapter. No lease, no runtime, no workspace.        | A session, `prepared`, revision 1.                                          |
| `managedSession/list`           | A cursor and a limit of at most 50.                               | Sessions in creation order, oldest first, with the next cursor.             |
| `managedSession/get`            | A session.                                                        | Its record.                                                                 |
| `managedSession/runtime/status` | A session.                                                        | `unbound`, `creating`, `indeterminate`, `bound_detached`, `bound_attached`. |
| `managedSession/close`          | An intent, the session, its revision.                             | `closed`, revision 2; its active lease is revoked in the same transaction.  |
| `managedSession/runtime/resume` | An intent, the session, its revision, **and the execution gate**. | The existing binding attached to the running runtime.                       |

A managed session has two control states and they are its only revisions: `prepared`
(1) and `closed` (2). Closed is terminal. A binding (one workspace, one Codex thread) is
permanent once created, and its attachment to a runtime is transient: a runtime exit,
a runtime stop and an authority restart all leave the session `bound_detached`.

**Resume is not a session state transition.** The product word maps to exactly one
existing operation, `managedSession/runtime/resume`, which attaches a session that is
already bound. The authority admits it through the execution gate under an effective
active lease that grants `session_resume`, with the runtime running, ready and
compatible. A session with no binding cannot be resumed; a closed one never can.

Every mutation carries an intent identity. The authority records each processed intent
with its canonical payload: the same intent with the same payload returns the original
receipt and changes nothing; the same intent with another payload is `intent_conflict`.

## What the window is given

`SessionPage` v1, from `managed_sessions_page`:

| Field       | Content                                                                     |
| ----------- | --------------------------------------------------------------------------- |
| `head`      | The durable head read before the sessions. The page is at least this new.   |
| `sessions`  | At most 25, newest first.                                                   |
| `older`     | The cursor of the next older page, when older sessions exist.               |
| `truncated` | The bounded walk ended before the newest session (more than 2000 sessions). |

Each session: its identity, adapter, control state, revision, creation and update
times, its position in creation order (the paging cursor), its binding state, the
workspace of its binding, and two presentation hints, `resumable` and `closable`.

The session identity crosses because it is the handle of the product's own actions.
The workspace path crosses because it is what tells one session from another. What does
not cross: the Codex thread identity, the runtime instance, the operation and lease
identities, the stream and authority instance, intents, events and their payloads.

`resumable` and `closable` say whether offering an action makes sense. They are derived
from the canonical record and binding and exist so the window encodes no rule of its
own. They authorize nothing: every action is sent to the authority and is the
authority's to refuse.

The window validates a page field by field, as it does the authority view. A page with
an unknown field, an order that is not newest first, a workspace without a binding or a
hint the shown state rules out is a failed read, not data.

## The narrow mutation boundary

Four commands, each with one typed request that rejects unknown fields:

| Command                   | Request                                        | Answer           |
| ------------------------- | ---------------------------------------------- | ---------------- |
| `managed_sessions_page`   | `before`: a position, or null                  | `SessionPage` v1 |
| `managed_session_prepare` | `intent_id`                                    | the session's id |
| `managed_session_resume`  | `intent_id`, `session_id`, `expected_revision` | the session's id |
| `managed_session_close`   | `intent_id`, `session_id`, `expected_revision` | the session's id |

There is no generic passthrough. No command takes a method name, and the window cannot
choose an adapter, a workspace, a scope, a lease, a lifetime or a page size: a prepared
session is a Codex session because that is the one adapter the authority runs, and the
bridge says so, not the window. The capability file names exactly these commands next
to the existing ones, and a test compares it with the build manifest.

An answer to an intent is a receipt: the authority accepted this intent for this
session. It carries no state. What the session is now is read.

No filesystem capability was added. Preparing a session needs no workspace, so there is
nothing to pick; no dialog, path reader or picker exists in the window.

## Who owns the connection

The observer of ADR 0019 is untouched. It keeps its one connection, its one
`audit/watch` and its cursor, and sends what it sent before.

`SessionService` is a second application-lifetime service with no connection of its
own. Each call opens one `ControlClient` connection, sends its typed requests and closes
it. Calls run one at a time, so the desktop holds at most one of these next to the
observer: two of the authority's eight client slots at most, and still one of its four
watcher slots. Nothing is sent on the watch connection but reads, so its
synchronization assumptions stand.

A connection per call is the natural shape here: the authority expires an idle
connection after five seconds, a call is a handful of exchanges, and a mutation that
fails mid-flight must not take the observer's cursor down with it.

Admission is separate from the terminal's permits and from the view's waiting slots:
at most four session calls are admitted, the rest answer `busy`. Shutdown sets a flag
that cancels the exchange in flight within one poll interval and refuses every later
call before it touches the socket.

## Idempotency

The window names each user action with one intent identity, generated when the action
starts. It is the identity sent to the authority, unchanged.

- A second click while an action is pending sends nothing. The buttons are disabled
  too, but that is presentation: the controller refuses a second action by itself.
- When the outcome is unknown (the authority did not answer, or stopped answering
  mid-exchange), the failure is marked retryable and Retry sends **the same intent
  with the same payload**. If the first attempt had committed, the authority returns
  its receipt; if not, it happens now. Either way it happens once.
- A refusal is not retryable. The action is over; a new click is a new intent.
- `SessionService` never retries on its own and never invents an identity for the
  action. A lost answer is reported, not papered over.

The idempotency that makes this safe is the authority's. The desktop only has to keep
the identity stable across its own retry, which is why the identity is generated where
the retry originates.

## Leases and the gate

Prepare and close need no lease, and none is involved.

Resume is gated. The desktop does not bypass the gate, does not hold or forge
permission and does not decide the outcome. It runs the sequence the core already
accepts, each step a request the authority may refuse:

1. Ask the authority to resume. If a lease that grants `session_resume` is in place, or
   the intent was already processed, this is the whole exchange.
2. On `gate_denied`, read `executionGate/status`. It is informational and explains a
   refusal the authority already made.
3. Only if the reason is that a lease is missing, not active or expired: ask the
   authority to grant one, for exactly `session_resume` and thirty seconds.
4. Ask again, with the same intent.
5. Ask the authority to revoke the lease it granted.

A lease somebody else holds is used as it is and never renewed, widened, replaced or
revoked. One that does not grant `session_resume` denies the resume and stays as it
was. Any other reason (the runtime is stopped, the session is closed) is the answer:
no lease is asked for. The grant and the revoke carry identities of their own; only the
resume carries the user's.

The user pressing Resume is the explicit act a lease stands for, and the lease asked
for is the narrowest one that act needs: one scope, for the length of one operation.
Leaving it in place would also keep any other grant from succeeding until it expired,
since a session has at most one active lease.

If the connection is lost between the grant and the revoke, the lease is not revoked.
It grants `session_resume` only and expires within thirty seconds.

When the runtime is stopped and no lease exists, the gate names the lease first. A
resume attempt then costs a grant and a revoke before it reports the runtime. The
window does not offer Resume while the runtime is not ready, so this takes a request
the window did not shape.

## List, pages and bounds

The protocol lists oldest first and pages forward only. A product list is newest
first, so a page is found by walking the list forward, fifty at a time, and keeping the
last twenty-five sessions before the cursor. Only the page is kept. Each session shown
costs one `managedSession/runtime/status` read; sessions walked past cost none.

The walk stops after forty protocol pages: two thousand sessions. Past that the page is
marked `truncated` and the window says so. Sessions are never derived from events and
never accumulated: the window holds one page of at most twenty-five, and a stack of at
most eighty cursors for the pages it walked back through.

A read is O(sessions / 50) requests. That is the price of a forward-only list and is
paid on every refresh; a descending list would be a protocol change and was not made.

## Canonical refresh

Events remain wake hints. The view is always a page that was read.

The window already follows the authority view (ADR 0019), which carries the durable
head and the count of authority restarts. That is the wake signal; no second follower,
watcher or poll exists. A page records the head read before it. The rule is:

> Read again when the authority's head is past the head the page was read at, or the
> authority is another instance. Read once for each head.

The head is read before the sessions, so a page is never older than its head. An event
committed during a read is either already in the page, or past its head, in which case
the view reports it and the page is read again. One read is in flight at a time; events
that arrive meanwhile are one more read, not one each. "Once for each head" bounds a
peer whose pages never catch up: it is read once per reported head, not in a loop.

After an accepted intent the page is read at once, without waiting for the event. After
a refusal it is read too: a refusal usually means the session is not what was shown.
Nothing is shown optimistically; an accepted close shows `Closed` when a read says so.

Sessions are read only while their view is shown. On the Console no session request is
made at all.

A change that writes no durable event (a binding that became indeterminate because the
authority stopped tracking it) is not announced. `Refresh` reads on demand.

## Navigation lifecycle

Both views stay mounted. Leaving the Console hides its section and makes it inert; the
terminal workspace, its tabs, its PTYs and its renderers are the same objects when it
is shown again, and the active terminal fits its host and takes focus.

The sessions controller is created once with the application, not with the view.
Switching views creates no service, no listener, no follower and no Tauri call beyond
the read a stale page needs. Twelve round trips between the two views make one terminal
creation, one renderer, one parked `authority_state` call and one page read.

## Offline behavior

When the authority is not connected the Sessions view stays reachable and says why,
with the reason the top bar already shows. Session data is cleared the moment the link
is not connected, together with any open prompt: nothing canonical outlives the
connection it was read on. Every mutation control is disabled and the controller
refuses to send. An action that was in flight still settles, and its retry waits for
the authority.

The Console does not depend on any of it.

## Failures

The bridge answers a product code and a retry hint, and nothing else:

| Code                  | From                                                                             |
| --------------------- | -------------------------------------------------------------------------------- |
| `unavailable`         | No authority, transport loss, timeout, `storage_unavailable`. Outcome unknown.   |
| `incompatible`        | Another protocol or storage version.                                             |
| `invalid`             | A revision no session can have. Decided in the desktop, before anything is sent. |
| `not_found`           | `session_not_found`                                                              |
| `changed`             | `stale_revision`                                                                 |
| `not_allowed`         | `invalid_transition`, or the gate naming a session that is not prepared.         |
| `not_bound`           | `binding_not_found`                                                              |
| `runtime_unavailable` | The gate naming the runtime; `workspace_invalid`.                                |
| `permission_denied`   | The gate naming the lease or its scope; `lease_already_active`.                  |
| `busy`                | `runtime_operation_in_progress`, `capacity_exceeded`, `active_turn_exists`.      |
| `rejected`            | `provider_rejected`                                                              |
| `uncertain`           | `external_effect_unknown`, `binding_indeterminate`                               |
| `conflict`            | `intent_conflict`                                                                |
| `internal`            | Anything else, including an answer out of contract.                              |

The authority's own error stays in the Rust value for tests and diagnostics and is not
serialized. The window words each code as one sentence; no protocol text, method name
or field name is shown. An unknown outcome is never worded as a refusal.

## Presentation

The view uses the shell's existing tokens, radii, pills and accent; no token was added
and the shell was not redesigned. The style pipeline's direction layers were not rerun:
the request fixed the direction as the existing one, and the view holds it.

Above 600 px of content the list and the details sit side by side; below, the details
sit above the list, with their actions first so they are never under the fold. Rows
truncate; nothing overflows at the 320 px minimum.

The details panel has a control of its own to hide it: in the smallest window it covers
the list.

Close is the one action that cannot be taken back, so it is asked about once, inline.
Creating a session is asked about too, because a session can be closed but never
deleted. Neither is a modal.

## Security boundary

- The trust boundary is unchanged: the authority's same-UID `SO_PEERCRED` check. A
  process of the same user could already send these requests; the desktop gains no
  privilege by sending them.
- The window's reach grew by exactly three intents, each naming one session and one
  revision. A compromised window can prepare sessions, close sessions and attach bound
  ones to the running runtime. It cannot start or stop the runtime, bind a workspace,
  start a turn, answer an approval, or obtain a lease with any scope but
  `session_resume`, any lifetime but thirty seconds, or for any purpose but one resume.
- A same-user process impersonating the authority on the socket path (ADR 0019) now
  also receives session intents and can answer false session lists. It learns session
  identities and intent identities, which authorize nothing.
- Tampering with a request is refused at the bridge when it changes the request's
  shape, and by the authority when it changes its meaning: another session, a stale
  revision or a reused intent are the authority's refusals.

## Limits and residual risk

- **Binding is not in this phase.** The desktop cannot bind a workspace
  (`managedSession/runtime/create`) or start the runtime. A session created in the
  window is prepared and unbound, and Resume applies to sessions bound through another
  client. Binding creates a provider thread with no exactly-once guarantee and an
  indeterminate outcome to manage, and belongs with the turn UI that needs it.
- More than two thousand sessions cannot be paged to the newest. A descending list in
  the protocol would remove both the bound and the walk.
- A read is not a transaction: the list and each binding are separate reads. A change
  in between is repaired by the next read, which the head rule guarantees.
- A lease the desktop asked for can outlive a lost connection by thirty seconds.
- Resume with the runtime stopped and no lease writes a grant and a revoke to the
  audit stream before it is denied.
- The non-Linux build of the bridge is a stub that answers `unavailable`. It was not
  compiled here.
- The real-desktop run used a synthetic session with GTK's accessibility bridge
  disabled (see Desktop acceptance). A real session was not used.
- As with ADR 0019, no party other than the session that built this reviewed it. Its
  tests were checked with forty mutation controls instead, which is not the same thing.

## Deferred

Workspace binding and its indeterminate outcomes, runtime start and stop from the
desktop, lease management as a visible concept, managed turns, streaming, approvals,
other adapters, a descending or filtered session list.

## Acceptance evidence

Accepted on Linux (CachyOS, KWin 6.7.5). Every authority ran in an isolated temporary
XDG directory; no production state was read or written and no real Codex was launched.
Where a runtime was needed it was the repository's Codex fixture.

**Service, against the real `wyss-control` process**
(`src-tauri/tests/sessions_process.rs`):

- Prepare, read and close, with the projection compared field by field with the
  authority's own record; the same prepare and the same close sent four times each
  leave one event each; a prepare whose commit was made by another client under the
  same intent returns that session; a stale revision, an unknown session and a reused
  intent are refused and change nothing.
- Sixty-two sessions paged newest first in pages of 25, 25 and 12, each session once,
  one connection per read.
- An authority that was never there, stopped with SIGTERM and killed with SIGKILL:
  every call answers `unavailable` and retryable, the same service finds the next
  instance, and an intent refused three times while nobody answered happens once.
- Next to a running observer: forty prepare/read/close/read cycles, every change
  announced by the observer's one watch, each event received once, the observer's one
  connection never lost, four session connections a cycle and every one closed, and
  the process holding the same descriptors and threads as before. With the watcher slots full, a session
  read is still served.
- Resume with the Codex fixture as the runtime: attached under a lease the authority
  granted and revoked around it (`granted`, `runtime_attached`, `revoked`, in that
  order), one `thread/resume`; the same intent three more times sends nothing; another
  client's lease is used and left intact; a lease without the scope denies and is left
  intact; a stale revision is refused before any lease; a stopped runtime denies and
  leaves no lease; a closed session is refused.

**Service, against a scripted peer** (`src-tauri/src/sessions/tests.rs`): an answer
lost after the request was taken, and the same intent settling it; the exact resume
sequence, scope, lifetime and identities; every gate reason; a list that never ends,
walked to forty pages and no further; four answers out of contract; five refused
handshakes; shutdown cancelling a call nobody answers; the full error table.

**Bridge** (`src-tauri/src/desktop/sessions.rs`): each request accepts exactly its
fields; the capability and the build manifest name exactly the known commands;
admission and its recovery.

**Window** (`tests/sessions.test.ts`, `tests/sessions-view.test.ts`): the page parser
against the fixture the Rust serialization is compared with, forty rejected shapes; the
exact IPC payloads; the controller's reads, wake rule, single flight, intents, retry
and paging; and the mounted application: navigation, empty, loading, failed, populated,
details, create, retry, close confirmation, resume, offline, live refresh, paging, and
twelve round trips to the Console and back.

**Mutation controls.** Forty mutants of the new paths were each killed: nineteen in the
window (a second intent on a second click, a retry with a new identity, a refusal
retried, sessions kept after the link is lost, a read loop, no wake on a new head, reads
while hidden, an optimistic close, a revision that was not shown, an extra request
field, a trusted unknown failure, unknown page fields, a hint on a closed session, a
close without confirmation, mutations enabled offline, resume always offered, the
terminal unmounted on navigation, a second follower) and twenty-one in Rust (a wider
lease scope, a longer lease, a lease never revoked, a lease asked for whatever the gate
says, a lease asked for first, each of the three intents replaced by a fresh identity,
a close at a convenient revision, an impossible revision sent on, an unbounded walk,
the oldest page instead of the newest, a list that does not advance, a stopped service
still connecting, a shutdown that does not cancel, the authority's error serialized,
the thread identity in the projection, a hint for an unbound session, unknown request
fields, a window-chosen adapter, a generic command in the capability).

**Real desktop.** See [Desktop acceptance](#desktop-acceptance).

## Desktop acceptance

The release `wyss-console` was run on a nested X11 display managed and composited by
KWin/X11 (rootful Xwayland on a headless compositor, so no input reached the
developer's session), with an isolated home, against the release `wyss-control` with
the repository's Codex fixture as its `codex`. Each step was driven with real pointer
and keyboard input and checked against the authority through the developer client.

The whole run was made on the binaries of commit `934666c`. Three presentation defects
found in it (a hint that repeated the line below it, "yet" on a closed session, a
heading cut to one letter at 320 px) and one gap (no control to hide the details panel,
which covers the list in the smallest window) were fixed in `7d68365`, which changes
wording, markup and CSS only. On that binary the affected views were captured again and
a final smoke was run: type in the Console, create a session, close it, close the
window.

- **Launch, with sessions already there.** The authority held one prepared session
  bound to a workspace, one closed session and a ready runtime before the desktop
  started. Both were listed, newest first, with `1 prepared · 1 closed`, and the
  authority held one connection: the observer's.
- **Create.** `New session`, `Create session`. The authority's list gained exactly one
  session, the one the window then showed selected; its stream gained one `prepared`.
- **Resume.** On the bound session. At the authority: `bound_attached`, no active
  lease, and `execution_lease.granted`, `managed_session.runtime_attached`,
  `execution_lease.revoked`, in that order; the fixture received one `thread/resume`.
- **Close.** With the confirmation open and unanswered the authority still held the
  session prepared at revision 1. After `Close session`: closed at revision 2, one
  `closed` event, and the window showed it closed with both actions disabled.
- **Authority stopped (SIGTERM).** The sessions were cleared, `Refresh` and
  `New session` disabled, the view said wyss-control was unavailable, and the desktop
  held no connection. Clicks on the disabled controls did nothing.
- **Authority restarted.** Without restarting the desktop the sessions were listed
  again with the selection kept. The bound session was shown `Detached`, as the new
  instance holds it, and Resume attached it again (a second `thread/resume`).
- **Authority killed (SIGKILL).** Its socket stayed behind; the view went offline. A
  restart brought the sessions back, detached again.
- **Console.** One terminal for the whole run: the same shell process before and after
  every navigation, the stop and the kill. Four commands typed at different moments,
  two of them while the authority was down, were all in its scrollback at the end.
- **Live refresh and pages.** Thirty sessions committed by the developer client while
  the view was open appeared without a click; the pager appeared at twenty-five;
  `Older` showed the remaining eight and `Newer` returned.
- **Resources.** 44 descriptors, 9 sockets, 41 to 43 threads, one link thread, one
  shell and one connection at the authority: the same after both outages, the thirty
  live commits, twenty round trips between Console and Sessions and fifteen refreshes.
- **Sizes.** At 960×640 list and details sit side by side. At 600 px the details sit
  above the list and a session was created there. At 380 and 320 px nothing overflows;
  at 320×480 hiding the details gives the list back.
- **Window material.** `_KDE_NET_WM_BLUR_BEHIND_REGION` was set on the composited
  window. As in ADR 0019 the blur itself was not looked at on a GPU session, and
  nothing in `desktop/glass.rs` or window creation changed.
- **Closing the window** ended the desktop by itself, without a panic, and left the
  authority running with its runtime ready, no connection, and its state intact. Its
  stream held the developer client's commits and the window's own: three prepares, two
  closes and two resumes with their leases. Nothing else.

**About the synthetic session.** The desktop's session there is a bare
`dbus-run-session` with no systemd user manager, so the accessibility bus does not
come up reliably. With GTK's default accessibility bridge the desktop crashed at start
in 4 of 24 launches, each time in `libatk-bridge-2.0.so` at the same offset (and in none
of 12 more under a debugger). With the
bridge disabled (`NO_AT_BRIDGE=1`, GTK's switch for a session without that bus) 25 of
25 launches started, and the run above was made that way with no crash recorded by the
kernel. The crash is in the bridge and depends on that bus, not on this change; it was
not looked for in a real desktop session.

## Later implementation status

Since ADR 0021 the desktop also binds a workspace and asks the authority to start the
runtime. `SessionPage` is version 2, with two more hints (`activatable`, `recoverable`);
the bridge has two more intents (activate, abandon) and one native folder dialog; and
a new session is created together with its activation, no longer by itself. The lease
sequence described above for Resume is unchanged and is now shared with binding, under
`session_create`. A lease the desktop could not give back is remembered and settled by
the next call for that session instead of only being left to expire. "Binding is not in
this phase" and the reach described under Security boundary describe this ADR's phase,
no longer the desktop.
