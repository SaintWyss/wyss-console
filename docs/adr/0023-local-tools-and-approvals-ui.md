# ADR 0023: Local Tools and approvals in the window

Status: Accepted
Date: 2026-10-09

## Decision

A session can be put to work in **Local Tools** from the window, and the approvals its
turns wait on are read and answered there. No developer client is needed for it.

```text
New session -> Local Tools -> folder -> Create & activate    (or: a bound session -> Enable Local Tools)
  -> wyss-control stops the runtime if it serves another mode, starts it isolated for that folder, attaches
  -> a turn -> Codex proposes a command or a file change -> wyss-control holds it as a pending approval
  -> the window shows it, under the turn and in Approvals -> Approve or Deny
  -> wyss-control checks the action again, answers Codex, and the same turn goes on or ends
```

```text
Svelte            the user's action: one of two modes, an approval it was shown, Approve or Deny
  -> Tauri        five more narrow commands: one mode intent, two reads, two decisions
  -> TurnService / SessionService
  -> ControlClient
  -> wyss-control  decides the profile, the isolation, the lease, the approval and the continuation
  -> managed Codex
```

Control Protocol v1 is unchanged: no method, field, schema or storage version was added.
The desktop composes operations the core already accepts (ADRs 0013, 0014, 0016, 0018).
It decides none of what is the authority's: whether a local-tools runtime may start,
whether its isolation holds, whether a command or a change is permissible, what a lease
allows, whether an approval is still pending, whether the action is still the one that
was shown, whether the provider may go on.

**Inference stays the default.** A session gets tools only when the user chose Local
Tools for it, in so many words, and nothing that is approved is remembered: there is no
"always allow", no trusted folder, no rule and no policy editing.

## One runtime, one mode: what Local Tools costs

This is the fact the rest follows from, read from the core and measured with Codex
0.159.0 before anything was written.

- **`wyss-control` supervises one managed Codex runtime.** A generation of it has one
  profile for its whole life: `codex_inference_v1`, or `codex_local_tools_v1` for
  **exactly one workspace**. There is no second runtime and no per-session profile.
- **Changing the mode, or the Local Tools workspace, means stopping that runtime and
  starting another.** `runtime/start` on a live generation that names another profile or
  workspace is refused (`invalid_transition`). `runtime/stop` detaches **every** session
  attached to the runtime, whichever session asked.
- **A stop is refused while a managed turn is active.** `wyss-control` answers
  `runtime/stop` with `active_turn_exists` and changes nothing: the turn, the runtime
  and every attachment stay as they were. This is the authority's rule since the
  security closure of this phase, described below; the desktop does not carry it.
- **A session keeps the mode it was bound in.** Codex keeps a conversation in the home
  of the profile that created it. `managedSession/runtime/resume` of an inference
  session under a local-tools runtime is refused by the provider (`provider_rejected`),
  and so is the reverse. Measured in both directions.
- **The profile is not durable.** After `wyss-control` restarts, the runtime it brings
  up is the inference one.
- **Nothing durable records a session's mode.** The binding names a workspace, not a
  profile. A mode exists canonically only while the session is attached
  (`attachment_profile`).

So "Enable Local Tools" cannot convert a session that already talked to Codex in
Inference, and the window does not pretend it can:

| In the window                                       | What it means                                                                                                                                            |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **New session**, mode **Inference** (preselected)   | As before (ADR 0021).                                                                                                                                    |
| **New session**, mode **Local Tools**               | The session is bound and attached under a local-tools runtime for the chosen folder. It is a Local Tools session from its first turn.                    |
| **Enable Local Tools** on a bound, detached session | The runtime is brought up in Local Tools for that session's workspace and the session is attached. Works for a session that was bound in Local Tools.    |
| **Use Inference** on a bound, detached session      | The reverse, offered while the runtime runs in Local Tools.                                                                                              |
| The same, for a session bound in the other mode     | The runtime is switched, the provider refuses the attachment, and the window says the session was created in the other mode. The runtime stays switched. |
| A session attached in one mode, asked for the other | Refused before anything is asked of the runtime (`mode_fixed`).                                                                                          |

Every action that may stop the runtime says so first, in the window: it restarts the
managed Codex runtime, sessions attached now are detached, and a session created in the
other mode cannot be resumed in this one.

- **For a session that is already bound** it is asked for only after one explicit
  confirmation that carries those words.
- **In the composer** the page of sessions says what the one runtime runs now. When
  that is another mode, or Local Tools for another folder, the composer says what runs,
  what the activation restarts it in and what that does to attached sessions, and its
  action is named for it: **Restart Codex & activate** instead of **Create &
  activate**. This holds for an Inference session created while Codex runs in Local
  Tools as much as for the reverse. It is said from what the window last read, and
  folders are compared as text; whether a restart happens is decided when the action
  runs, by what the authority answers then.

Other sessions are not migrated, not reattached and not told anything they did not ask:
they show `Detached` when they are read next, because that is what they are.

## The mode model

Three values, read from the authority and never kept by the window:

| Mode             | When                                                                           |
| ---------------- | ------------------------------------------------------------------------------ |
| **Inference**    | The session is attached with `inference_only_v1`.                              |
| **Local Tools**  | The session is attached with `local_tools_v1`.                                 |
| **Not attached** | Anything else. No mode is claimed for a session the authority does not attach. |

The page of sessions also carries what the one runtime is: its mode while it is wanted
running and ready, and the Local Tools workspace. The Sessions heading says it ("Codex
runs in Local Tools for rag-corp"), each row and the details show the session's mode,
and the Agent view names it beside the agent: `Codex · Inference`,
`Codex · Local Tools`.

## Asking for a mode

`managed_session_activate` gained one field, `mode`, and `managed_session_mode` takes an
operation identity, a session, the revision that was shown and a mode. `mode` is a
closed enum of two values, `inference` and `local_tools`. Unknown fields are refused.

- **The window names no profile.** The safety profile is chosen in Rust from the mode:
  none for Inference, which is the authority's default; `codex_local_tools_v1` with the
  workspace for Local Tools.
- **The window names no folder for an existing session.** `managed_session_mode` has no
  path. The workspace is the one the authority holds in the session's binding, read at
  that moment. For a new session it is the folder the native dialog returned in this
  run, as in ADR 0021, and whether it may be a workspace is the authority's to say.
- **Tauri never launches Codex** and shells out to nothing. Starting and stopping are
  requests; `wyss-control` launches, isolates, attests and supervises, or refuses.

### The sequence

One step per call, as activation already was (ADR 0021): a call reads what the session
and the runtime are now, asks for the one operation that is missing and says how far it
got. The window repeats the same action until a read says `Ready`.

```text
runtime ready, serving another mode or workspace  -> (no turn running) runtime/stop  -> Stopping the managed runtime…
runtime stopping                                  -> nothing                         -> Stopping the managed runtime…
runtime not up                                    -> runtime/start, the mode asked   -> Starting Codex in Local Tools…
runtime starting                                  -> nothing                         -> Starting Codex in Local Tools…
unbound, runtime ready                            -> managedSession/runtime/create   -> Creating workspace binding…
bound, detached                                   -> managedSession/runtime/resume   -> Attaching…
bound, attached in the mode asked                 -> nothing                         -> Ready
```

- **Whether the running generation is the one wanted** is the authority's answer. The
  same folder can be spelled two ways, so when the texts differ the desktop asks
  `runtime/start` for the mode and the workspace it wants: the authority admits that on
  a live generation exactly when it is that generation's own profile and workspace, and
  launches nothing; otherwise it answers `invalid_transition`.
- **Each mutation has its own identity**, derived from the action's identity and the
  step (`stop`, `start`, `create`, `attach`), so a step asked again is the same intent.
  A stop or a start answered with the receipt of the one this action already asked for
  is not asked a second time: the runtime that is up now is somebody else's, and the
  action ends as "Codex is not available".
- **It is not a transaction and nothing is rolled back.** After a stop the runtime is
  stopped, whatever happens next. The intermediate states are the real ones and are
  shown as they are read.
- **A runtime the authority contained for safety is not started again** from the window
  (`inference_isolation_violation`, `safety_unproven`).

### Never under a running turn

**`wyss-control` enforces it.** A `runtime/stop` is admitted only while no managed turn
is active, whichever client sends it: the desktop, the developer client or any other.
While one is, the request is answered `active_turn_exists` before anything durable
changes. No admission is withdrawn, no session is detached, no event is written and the
turn is not touched.

- **Active** is what the authority holds as not ended: a turn that was admitted and not
  yet written to Codex, one that runs, one that waits on an approval, and one whose
  cancel was asked for and has not ended yet. Both the core's own active turn and the
  durable record are consulted; either is enough to refuse.
- **It cannot be raced.** The core admits a turn and takes a stop on the same thread,
  one request at a time. Whichever of the two it takes first decides: a turn admitted
  first makes the stop a refusal, and a stop admitted first leaves no ready runtime for
  the gate to admit a turn on. There is no order in which both are accepted, and
  nothing a client read earlier takes part in the decision.
- **It is not a permanent block.** Once the turn is terminal the same stop is admitted.
  A turn that does not end by itself is cancelled, and a cancel the provider does not
  confirm ends the turn by the interrupt deadline.
- **The refusal comes before the receipt lookup**, as the admission of a start does: a
  stop repeated under its own identity while a turn runs is refused, not answered with
  an earlier receipt.
- A stop is the one explicit request this applies to. The runtime exiting by itself, a
  containment by the authority and the authority's own shutdown still end a turn, as
  ADRs 0015 and 0016 describe.

**The desktop reads first, as a courtesy.** Before it would ask for a stop it reads
whether a managed turn is running: the last turn the durable stream names as admitted,
and its status. If it has not ended the action is refused as **"A turn is running"**
and no stop is sent. This holds for either mode: an Inference session created while
Codex runs in Local Tools goes through the same reading. That reading spares a request
that would be refused; it is not what keeps the runtime up. When the reading is stale
and the stop is sent, the authority's `active_turn_exists` is shown with the same
sentence, and the action can be asked again once the turn ended.

## Workspace and isolation

The desktop adds nothing to the isolation of ADR 0013 and removes nothing from it. A
local-tools generation runs in the authority's namespace, with its private Codex home,
for the one workspace it was started for, and the authority attests it before it reports
ready. If that cannot be proven the runtime does not become ready, the activation ends
"Codex is not available", and nothing falls back to a weaker mode: the session stays
unattached.

A folder the authority does not accept as a workspace is reported as "the folder cannot
be a workspace", as in ADR 0021.

## The lease of a Local Tools turn

The authority accepts an approval only under the lease its turn was **admitted** under,
and only if that lease carries `command_execute` or `file_change` (ADR 0016). So the
scopes are fixed when the turn starts:

| The session is attached with | Scopes of the turn's lease                     |
| ---------------------------- | ---------------------------------------------- |
| `inference_only_v1`          | `turn_start`                                   |
| `local_tools_v1`             | `turn_start`, `command_execute`, `file_change` |

The profile is read from the authority at that moment; the window sends nothing that
could widen it. Everything else is as in ADR 0022: asked for only when the gate names a
lease as what is missing, 1800 seconds, kept while the turn runs, a lease somebody else
holds used as it is. An approval of a turn that somebody else admitted under a lease
without those scopes is shown like any other and can be denied, since a refusal needs
no authority (ADR 0016); approving it is refused by the gate and shown as denied by
the authority.

## Approvals

### The contract

| Operation                              | What the authority does                                                                                                                                                     |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `managedSession/turn/approval/pending` | The approval a turn waits on.                                                                                                                                               |
| `managedSession/turn/approval/status`  | One approval: its record and, **while it is held**, its action: the whole command and its directory, or one file of the change set with one 8 KiB page of its diff.         |
| `managedSession/turn/approval/respond` | `accept`, `decline` or `cancel` at the revision the caller saw, with an intent identity. Checks the lease, the fingerprint and the isolation again before anything is sent. |

States: `pending`, `accepted`, `declined`, `cancelled`, `expired`, `invalidated`,
`indeterminate`. At most one approval is pending in the whole authority, because one
turn runs at a time. An approval waits until the earlier of five minutes and the expiry
of its turn's lease. There is no operation that lists approvals; a request for one is a
durable event, `authority.managed_approval.requested`, that names it.

### Discovery

The index the desktop already keeps of the durable stream (ADR 0022) also remembers the
approvals it names: the last sixteen. A list reads the durable head, brings the index up
to it and then reads the **eight** newest from the authority, each with
`approval/status`. Nothing is shown from the event alone: an approval is pending because
the authority said so in that read.

### What crosses

| Crosses                                                                                                   | Stays in Rust                                            |
| --------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| The approval's identity, as the handle it is read and answered with                                       | The provider's request, item, thread and turn identities |
| Its session and its turn                                                                                  | The runtime instance and generation                      |
| Kind, state, revision, requested, deadline, ended, why it ended, what was decided                         | The lease and its scopes                                 |
| `decidable`: pending with its action still held. A hint; the authority decides                            | The action fingerprint and the external-effect marker    |
| The workspace the authority holds for that session                                                        | The raw provider payload                                 |
| A command: the whole text and its directory                                                               | Anything of the model's reasoning                        |
| A change set: how many files, and one file at a time: path, kind, move target, size, one page of its diff | Environment, credentials, the Codex home                 |

The types that carry an action print `<sensitive>` in diagnostics, and nothing of an
action is logged. The window validates every answer whole and refuses one with a field
it does not know.

**A command is text.** It is shown in a `<pre>` exactly as the authority presents it,
never as markup, and nothing in the desktop parses it, completes it, or builds a command
from it. The desktop runs nothing: what runs is what the authority approved, inside the
runtime's namespace.

**A change set is bounded and its truncation is said.** The authority bounds it (64
files, 128 KiB a file, 256 KiB in all). The window reads the first file with the list,
then up to eight more one at a time, and a diff longer than a page shows how many bytes
of how many are on screen with **Show the rest**, which reads it to its end, page after
page. **Approve is offered only when every file and every diff was read to its end.**
That is a presentation rule, so that nobody approves what was not on screen; the
authority's own check does not depend on it. Deny needs no reading.

### One reader

`ApprovalsController` lives for the application's lifetime, above the views, and is the
only thing in the window that reads approvals. The Approvals view and the card under a
turn show its state.

- It is fed by the shell's one authority follower. No second watcher exists.
- **It reads when the durable head moves, and only while somebody has a use for it**:
  the Approvals view is shown, or a turn is being followed. With nobody interested
  nothing is read, whatever happens at the authority. A list read is the head, the
  durable events since the last one, and one `approval/status` and one binding read for
  each approval listed; with no approval in the stream it is the first two.
- A reading of a turn that names an approval the controller does not show makes it read
  once; if the list does not have it either, it is not asked again until the head moves.
- **One timer**, set for the deadline that comes first, fires once to read what became
  of that approval. Nothing is polled, and no timer is left after `dispose`.
- One list at a time, one file at a time, one page at a time. An answer to anything but
  the latest list is dropped.
- When the connection is lost everything read is dropped: nothing read on a connection
  that is gone is shown, or can be decided, as pending.

### In the Agent view

A running turn that waits shows the approval **under that turn**: what Codex wants to
do, the command and its directory or the files and their diffs, the deadline, and
**Approve** and **Deny**. The card is shown only when the approval the authority names
for that turn is the one the controller read for that turn; an approval of another turn
or another session is never drawn there. The turn stays `Running`, because that is what
it is. **Cancel turn** remains, and is a different action.

### The Approvals view

`Approvals` in the sidebar is a real view now, with a mark on it while something waits.

| State            | Shown                                                                                                                              |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Loading          | "Reading approvals…"                                                                                                               |
| Nothing pending  | "Nothing is waiting for a decision", and what Local Tools is for                                                                   |
| Pending          | The same card as under the turn, with the session's workspace and the turn named                                                   |
| Offline          | Nothing as pending; that `wyss-control` is not connected                                                                           |
| Read failed      | That it could not be read, with **Try again**. Nothing unverified is shown in its place                                            |
| Decision pending | "Asking wyss-control to approve it…" / "…to deny it…", both buttons disabled                                                       |
| Decision failed  | Why, in one sentence; **Retry** only when repeating can help; **Dismiss** once it was re-read                                      |
| Ended            | The last ones, newest first: Approved, Denied, Expired, No longer applies, Outcome unknown, with the reason the authority recorded |

### Approve and Deny

`managed_approval_approve` and `managed_approval_deny` each take an intent identity, the
approval and the revision that was shown. Nothing else, and unknown fields are refused:
there is no decision field, no scope, no lease, no fingerprint, no session-wide flag.
The command is the decision. Approve sends `accept`. Deny sends `decline`.

**Deny is not Cancel turn.** It refuses this one action. What happens to the turn is
the provider's and the authority's, and the window shows what a reading says: with the
Codex contract the authority speaks, a declined command ends the turn `interrupted` /
`user_cancel` and a declined file change lets it go on. The authority records a turn
the user cancelled with the same reason, so the window words it for both: "Stopped at
your request", under the line that says the approval was declined. `cancel` as an
approval decision is not offered; **Cancel turn** exists for that.

**After Approve** the card says the authority is being asked. A receipt means the
authority committed the decision; the window then reads, and the approval is shown
`Approved` when a list says so. The same turn goes on: no `turn/start` is sent, by
anybody. What Codex does next (running, a further approval, completed, failed,
indeterminate) is shown as it is read.

### Idempotency

- **One decision is one user action with one identity**, made when the button is
  pressed. While it is unsettled no other decision is sent for that approval: not a
  second click, not the other button.
- **A lost answer is retried with the same identity, the same decision and the same
  revision.** The authority answers that with what it committed the first time.
- **Approve never becomes Deny by itself**, or the reverse. A failed decision can be let
  go of only after the approval was read again and is still there to decide; what the
  user presses then is another action.
- A refusal that repeating cannot change (`changed`, `not_allowed`, `permission_denied`)
  is not offered for retry. The approval is read again either way.
- A decision on an approval that already ended is refused by the authority at any
  revision, and shown as "the approval had already ended".

### Expiry

The deadline is the authority's. The window shows it and, when it passes, reads what
became of the approval instead of deciding that it expired. A decision that races the
deadline is answered by the authority, and its refusal is what is shown. An approval
that ended in any way has no buttons.

## Tool activity and privacy

Unchanged from ADR 0022. A command or a file change in a turn is one line that says it
happened, and the output of a command does not cross. The one new thing that crosses is
the action under approval, to the person who has to decide it, while the authority holds
it. Once the approval ends the authority no longer holds the action, and the window
shows the approval's record without it.

## Recovery

- **The desktop restarts.** It has nothing of its own. It reads the sessions, their
  modes, the turns and the approvals; a pending approval is shown again from what the
  authority holds, undecided.
- **The authority restarts.** Everything read is dropped. A pending approval comes back
  `invalidated` / `authority_restarted`, its turn `indeterminate`, the sessions
  detached, and the runtime in Inference. A Local Tools session has to be entered
  again, explicitly.
- **The runtime goes away while an approval waits**, because it exited or the authority contained it; a stop is refused while the turn is active. The approval ends `invalidated`, with the reason the authority recorded, and the turn `indeterminate`.
- **A decision's answer is lost.** Shown as an unknown outcome with **Retry**, which
  sends the same decision. A reconnection sends nothing by itself.
- Nothing here ever resends a turn, answers an approval the user did not answer, or
  makes a second identity for a decision.

## Bounds

| What                                 | Bound                                      |
| ------------------------------------ | ------------------------------------------ |
| Approvals remembered from the stream | 16                                         |
| Approvals read for one list          | 8                                          |
| Files read without being asked       | 1 with the list, then 8                    |
| Diff page                            | 8 KiB (the authority's)                    |
| Command, directory                   | 16 KiB, 4 KiB (the authority's)            |
| Change set                           | 64 files, 128 KiB a file (the authority's) |
| Timers                               | 1, only while an approval is pending       |

## Security boundary

- The window is granted five more of this application's own commands
  (`managed_session_mode`, `managed_approvals`, `managed_approval_read`,
  `managed_approval_approve`, `managed_approval_deny`) and nothing else: no filesystem,
  shell, process, socket, dialog or event permission of the framework.
- No command takes a control method or a payload to pass through. There is no
  `control_request`, `runtime_request` or `approval_request`.
- The window cannot name a safety profile, a workspace for an existing session, a lease,
  a scope, a lifetime, a fingerprint or a policy.
- An approval identity is a handle. A made-up one names nothing; one of another session
  names that approval, which is shown as what it is, under its own session, and decided
  by the authority on its own terms.
- Approve is a request. The authority checks, when it arrives: the approval still
  pending at that revision, the turn's own lease and its scope, the action against the
  fingerprint it presented, the workspace confinement.

## Limits and residual risk

- **A stop under a turn is the authority's to refuse, and it does.** The desktop's own
  reading before a stop is two requests and can be stale; nothing depends on it. What a
  client cannot do any more is use `runtime/stop` to end a turn that hangs: the turn is
  cancelled first.
- **A session has no mode while it is detached.** The window cannot say, before trying,
  that a detached session was created in the other mode. It says so after the provider
  refused, and by then the runtime was switched. The confirmation says this can happen.
- **One Local Tools workspace at a time.** Working with tools in two folders means
  switching the runtime between them, detaching the sessions of the first.
- **An approval waits five minutes** at most. There is no way to extend it.
- **A session cannot be bound while a turn is running.** The authority answers `busy`,
  and the window says so with **Retry**. A second Local Tools session for the same
  folder is created between turns.
- **A plain Resume of a detached session can be refused by Codex** when the runtime is
  in the other mode. The window says a session resumes only in the mode it was created
  in and points at the mode action; it cannot offer only the right one, because the
  session's mode is not known.
- **The Approve rule is the window's.** Another client can approve without reading.
- **The bridge mapping of the two decision commands** (approve to `accept`, deny to
  `decline`) is covered by the service tests, the real-authority tests and the desktop
  acceptance; a Tauri command with managed state cannot be called from a unit test.
- The discovery index keeps sixteen approvals; an older one that is still named by a
  turn is read through that turn.
- The non-Linux build of the bridge is a stub that answers `unavailable`. It was not
  compiled here.
- As with ADRs 0019 to 0022, no party other than the session that built this reviewed
  it before this record was written. Its tests were checked with mutation controls
  instead, which is not the same thing.

## Deferred

Remembering a decision in any form (always allow, trusted folders, rules, policy);
editing a proposed action; more than one runtime, or one per session; a durable record
of a session's mode; an event and history
view (Phase 3F); rendering Markdown; other adapters.

## Acceptance evidence

Accepted on Linux (CachyOS, KWin 6.7.5). Every authority ran in an isolated temporary
XDG directory and every workspace was a disposable temporary folder. No production state
of the authority was read or written.

**Service, against the real `wyss-control` process**
(`src-tauri/tests/approvals_process.rs`, the Codex fixture as the runtime, started in the
authority's real namespace), five tests:

- Local Tools asked for by name: the start carries the local-tools profile and the
  workspace, the session is attached with `local_tools_v1`, and the turn's lease is
  audited as `turn_start`, `command_execute`, `file_change`. The approval is listed and
  read with its command and directory, and with no provider, lease, runtime or
  fingerprint identity in any answer. Approve at a stale revision is refused; at the
  shown one the authority commits it, the provider is answered once, the command runs
  once and the same turn completes with one `turn/start`. The same intent again is its
  receipt; a new decision on the ended approval is refused at any revision; an approval
  identity that names nothing is `not_found`.
- A command denied: the provider is sent `cancel`, nothing runs, the turn ends
  `interrupted`, and the approval is `declined` / `client_decision`.
- A file change: shown with its path, kind and diff; a file or a page it does not have
  is refused as a request; denied, the turn goes on and nothing is written; approved in
  another turn, the file is written once.
- One runtime, one mode: an inference session and a Local Tools session cannot be
  attached at once; entering the other mode under a running turn is refused as
  `turn_active` and sends no stop; with no turn running it stops, starts and attaches;
  after an authority restart the runtime is back in inference and both sessions are
  detached.
- An approval that ends undecided: its lease expiring (three seconds, another client's
  lease) and the authority restarted under it, each read as what the authority
  recorded, with no Accept sent for either. A stop of the runtime under a pending
  approval is refused, and the approval stays pending and decidable at the revision it
  had.

**Tests.** 510 Rust tests and 458 window tests passed when the phase was accepted; 519 and 458 pass since the two closures below. Eighty-three mutation controls, one
per adversarial focus, are each killed by a test: 54 in the window (a second decision
while one is pending, Approve sent as Deny, a decision at another revision, a lost
decision retried under a new identity, a refused decision retried, an unsettled decision
dismissed unread, a decision resent after a reread, pending approvals kept across a lost
connection, the deadline never read, a timer left after dispose, approvals read with
nobody interested, the same head read again, a missing approval asked in a loop, two
lists, two file reads or two page reads at once, an unbounded file listing, a file or a
page of another change shown, a partial change approvable, any approval shown under a
turn, a command or a diff rendered as markup, an "Always allow" button, unknown fields,
an unbounded diff page, command or list, page arithmetic unchecked, an ended approval
decidable, a decision that contradicts its state, an answer about another approval or
page, a decision sent as data, the composer opening in Local Tools, the chosen mode not
sent, a mode entered without confirmation, a mode for a session without a workspace, a
mode request that names a folder, a mode claimed for a detached session, a restart of
the shared runtime not announced, a generic, a runtime or an always-allow permission)
and 29 in Rust (requests that carry more, inference naming a workspace, Local Tools
without its profile, an inference turn with tool scopes, a Local Tools turn without
them, a session scope in a turn lease, a runtime stopped under a turn, a running turn
read as none, a guard that never asks the authority, a stop asked twice, a runtime
taken for the other mode or another workspace, an attached session moved to the other
mode, a contained runtime started again, Deny sent as `cancel`, Approve sent as
`decline`, a receipt or a read of another approval, a decision at no revision, an
unbounded list, an ended approval decidable, approvals never discovered). Three of them
found a missing test, and none a defect in the code under test.

**Real desktop**, once, on the release build of `57f7d13`, in a nested compositor with
real pointer and keyboard input and nothing sent to the user's session. The Codex
fixture was the runtime, started by the authority in its real namespace:

1. A session created in Inference (the preselected mode) is Ready, shows `Inference`,
   and a turn in it completes: `Codex · Inference`.
2. A new session with **Local Tools** chosen: the composer says "Codex runs in Inference
   now, and wyss-control runs one Codex runtime. Activating this session restarts it in
   Local Tools for this folder, and the sessions attached to it are detached. A session
   created in Inference cannot be resumed in Local Tools." and its action reads
   **Restart Codex & activate**.
3. Pressed, the window shows "Stopping the managed runtime…" with the session still
   without a workspace, then the session Ready.
4. The session is Ready in Local Tools: `Codex · Ready · Local Tools`, the heading says
   "Codex runs in Local Tools for ws", and the inference session is `Detached`.
5. A second Local Tools session for the same folder is created with no restart
   announced and none made: the runtime's revision stayed at the three that its first start, the stop and the second start account for.
6. A turn in a Local Tools session: "Codex wants to run a command", the command and its
   directory as text, the deadline, **Approve** and **Deny**, under that turn.
7. The same approval in `Approvals`, with the session's folder and the turn named, and a
   mark on the sidebar entry.
8. The other Local Tools session's tab shows no turn and no approval while that one
   waits.
9. **Approve** in `Approvals`: the approval is listed `Approved`, and the turn shows
   "Approval accepted", "Command started", "Command output", "Command finished" and ends
   `Completed`. The provider was sent one `turn/start` for it and one Accept; the
   command ran once. The command's output is not shown.
10. **Deny** under a turn of the other session: "Approval declined", the turn
    `Interrupted`, "Stopped at your request". Nothing more ran, and the first session's
    tab showed its own completed turn throughout.
11. A turn left waiting: going to the Console, Sessions and Approvals and back, and
    resizing the window, neither answered it nor started a turn. The terminal opened at
    launch took input during it: the same PTY, through an authority restart and the changes of mode before it.
12. At its deadline, five minutes after it was asked for, the card and its buttons are
    gone, the turn is `Interrupted` with "The approval it waited on expired undecided",
    and `Approvals` lists it `Expired`, "Nobody decided it in time". Nothing was
    accepted.
13. The authority killed under a pending approval: the window says it is offline, the
    card gives way to a notice with no buttons, and `Approvals` shows nothing as
    pending. After the restart the approval is listed "No longer applies", "wyss-control
    restarted while it waited", the sessions are `Detached` with mode "Not attached",
    Codex runs in Inference, and the provider was sent no decision.
14. **Enable Local Tools** on a detached session asks for one confirmation with the
    impact in words, then the session is Ready in Local Tools; the other one then offers
    **Resume** and **Use Inference**.
15. A session for another folder asked for while a turn waited: "A turn is running, and
    the runtime it runs on is not restarted under it." The runtime was not touched. (In
    the exploratory run on the build before; the code path did not change.)
16. A file change: "Codex wants to change a file", `Add`, the path, the diff as text.
    Approved, the file is in the workspace with that content and the turn completes with
    "File change applied".
17. At 960×640 nothing overflows. At 600 and 320 pixels the card's fields stack, the
    command wraps, the sidebar becomes a rail, and the mode stays in the Agent header on
    a line of its own.
18. `_KDE_NET_WM_BLUR_BEHIND_REGION` was set on the composited window; nothing in window
    creation or `desktop/glass.rs` changed.
19. No provider identity, lease, fingerprint or reasoning appears in any capture. The
    process measured 52 descriptors, 10 sockets and 51 threads right after a folder dialog and 49, 9 and 47 to 48 at three later points, across six turns.

**Real Codex, Local Tools**, through the window, once, with Codex 0.159.0 and a
disposable folder that held one file, `hello.txt`:

- The session was created in Local Tools from the window. The window showed "Starting
  Codex in Local Tools…" while the authority launched Codex in its namespace, then the
  session Ready in Local Tools.
- **Approve.** Asked to print the file, Codex proposed `/usr/bin/bash -lc 'cat
hello.txt'` in that folder. The window showed it under the turn. Approved, the command
  ran, the same turn went on and ended `Completed` with the file's one line in the
  answer.
- **Deny.** Asked again in a second turn, Codex proposed the same command. Denied, the
  turn ended `Interrupted`; no command was started.
- The authority recorded one `thread/start`, two `turn/start`, one Accept and one
  `cancel` to the provider, no interrupt; two approvals requested, one `accepted` and
  `confirmed`, one `declined`; four leases granted and revoked: `session_create`,
  `session_resume`, and twice `turn_start` with `command_execute` and `file_change`.
- The folder held `hello.txt`, unchanged, and nothing else afterwards. The user's Codex
  login was read in place by the authority's namespace and was the same file before and
  after, by content, size and time. Codex kept its conversation in the private home of
  the local-tools profile, inside the temporary state directory.

Four defects were found on the real desktop before the accepted build and fixed in it:
the composer announced a restart of the shared runtime only in general terms, and not at
all for an Inference session created while Codex ran in Local Tools; its text was
squeezed into a narrow column beside its buttons; a turn ended by a denied command read
"Cancelled at your request"; and the mode was dropped from the Agent header at narrow
widths. A refused Resume was also reworded to point at the mode.

Not seen on the real desktop, and covered by tests: a decision whose answer is lost
(Retry and Dismiss), a diff longer than a page (Show the rest), a change set of more
than nine files, and a Resume that Codex refuses because the session was created in the
other mode (measured against real Codex at the protocol, before the window was built).
The real-desktop run used a synthetic session with GTK's accessibility bridge disabled,
as in ADRs 0020 to 0022.

## Security closure

An independent review of this phase found no critical or high issue and one medium:
the guard against stopping the runtime under a turn lived in the desktop. It read
whether a managed turn was running and then sent `runtime/stop`, two requests with
nothing to keep another client from starting a turn between them, and the authority
admitted the stop. The turn ended `indeterminate` / `runtime_stopped`. This record had
listed it as a known limit; it is closed here, in the core.

**The fix.** `wyss-control` refuses `runtime/stop` with `active_turn_exists` while a
managed turn is starting or running, as described under "Never under a running turn".
The check is one function of the core's turn owner, called where the core takes a
client's request, before the receipt lookup and before anything that withdraws an
admission, detaches a session or writes an event. Turn admission happens on that same
thread, so no lock was added. The code that ended a turn as `runtime_stopped` after a
stop is gone, and a stop no longer withdraws a turn's admission. `active_turn_exists`
already existed in Control Protocol v1; nothing was added to it. In the desktop the one
change is that this refusal of a stop is said as "A turn is running".

**Evidence.**

- Unit, in the core (`control/turn_approval_tests.rs`): a stop is refused for a turn
  that is admitted and not yet dispatched, running, waiting on an approval and being
  cancelled; refusing changes nothing of the turn; the durable record alone is enough
  to refuse; once the turn completed, failed or was interrupted the stop is admitted.
- The exact ordering, against the real `wyss-control` process
  (`tests/turns_process.rs`): one client reads that no turn is running; another starts a
  turn, which the authority admits; the first sends `runtime/stop` and is refused,
  three times, with and without an expected revision. The runtime keeps its revision
  and instance and stays ready, the session stays attached, the turn stays running at
  its revision under its lease, and the durable stream gains no event. The desktop
  service, given a reading that says no turn runs, asks for the other mode: the
  authority refuses its stop and the action fails as a running turn. The turn is then
  cancelled and ends `interrupted` / `user_cancel`; the same stop is admitted, the
  runtime stops, and after a new start and a resume the session runs another turn,
  which is protected the same way.
- Racing, against the real process: ten rounds of a turn start and a stop released
  together from two connections, the stop held back on half of them. Every round ends
  with exactly one of the two accepted, and no turn the authority holds was ever ended
  by a stop.
- The other process suites: a stop under a pending approval is refused and the approval
  stays pending and decidable; a stop racing an Accept is refused and the Accept
  decides; the mode switch of this phase still stops, starts and attaches when no turn
  runs, and is refused under one.
- Thirteen mutation controls, each killed by a test: the guard removed; inverted; not
  consulted where turns are admitted, so that only the desktop's reading is left; a stop
  that proceeds after being refused; a turn ended by a refused stop; sessions detached
  by a refused stop; the durable record ignored; a refused stop withdrawing a turn's
  admission; the refusal not said as a running turn.
- Desktop smoke on the release build of `1e7fad0`, in the nested compositor: with a turn
  running, the developer client's `runtime/stop` answers `active_turn_exists` and the
  runtime stays ready at its revision with no new event; asking for Local Tools from the
  window is refused as "A turn is running"; the terminal opened at launch takes input;
  the turn is still `Running`, is cancelled and ends `Interrupted`; the same request for
  Local Tools then stops the runtime, starts it for the folder and leaves the session
  Ready in Local Tools.

## Receipt replay

The security closure left one defect it had found and described: a `runtime/stop` sent
again under an intent that had already committed was answered with its durable receipt
and changed no desired state, yet the core still ran what follows a stop. Every attached
session was detached, and a binding request that happened to be in flight became
indeterminate, on a runtime that kept running. The independent review confirmed it as a
medium defect of idempotency. It is closed here.

**The invariant.** A runtime mutation that already committed, sent again with the same
payload, is answered with the same receipt and causes no new runtime or attachment side
effect: no session is detached, the desired state, the actual state, the revision and
the instance are as they were, no child is stopped or launched, no managed turn is
touched and no event is written.

**The fix.** After the store answers a runtime mutation, the core decides what is left
to carry out from whether a transition committed now. The store gives every committed
runtime mutation the next revision, and the core carries a revision out before it takes
another request; a receipt at or below the revision already carried out is one whose
effects happened, and it has none left. Only a stop that committed now detaches and
reconciles, and only a start that committed now reconciles. That decision is one
function of the request, the store's answer and that revision. No identity is compared
outside the store and nothing about idempotency is kept in memory. When the receipt
becomes durable, when the witness advances and what an intent is answered with are
unchanged, and so is Control Protocol v1.

- **The active-turn guard is still asked first.** An old stop intent sent while a turn is
  active is refused with `active_turn_exists`, like a new one, and is not answered with
  its receipt.
- **A new stop is still a stop.** Under a new intent it commits the next revision,
  detaches every session and reaps the child.
- **A start was already harmless.** On the code before this closure a start sent again
  changed nothing that can be observed, attached or stopped; what ran for it was a
  reconciliation that finds nothing to do at an unchanged revision. It now runs nothing,
  by the same decision.
- **A binding request in flight keeps its admission.** The admission of a binding request
  the supervisor has not written yet is withdrawn before a stop, a lease revocation or
  renewal, or a close commits, so that nothing leaves after that commit. It used to stay
  withdrawn even when the mutation turned out to change nothing, and the request then
  failed as not dispatched. It is now given back in that case, as a turn's admission
  already was: for a receipt answered again and for a mutation the store refused. One
  that did change something leaves it withdrawn.
- The other things the core does after the store answers were read for the same
  pattern. Recomputing the next lease expiry after a lease operation or a close only
  catches up with the clock and is the same whether or not the answer is a receipt. A
  lease operation or a close answered again with its receipt still leaves a binding
  request in flight withdrawn; telling such a receipt from a fresh commit is not
  something the core can read from the answer today, and it was left as it is.

**Evidence.**

- Unit, in the core: the decision returns nothing to carry out for a receipt at or below
  the carried revision, for a stop and for a start, for the latest transition repeated
  and for an older one, and for a mutation the store refused; a stop that committed now
  detaches and reconciles and a start reconciles. A binding request in flight loses its
  admission once, gets it back, keeps it withdrawn after a change, and has none to give
  back once a stop ended it.
- The exact ordering, against the real `wyss-control` process
  (`tests/runtime_process.rs`): start, a session bound and attached, stop A, the runtime
  stopped and the session detached; start B, the session resumed; stop A again, four
  times, on two connections. Each answer is the receipt of A. The runtime is ready on
  the same instance at the revision of start B, the same child runs, the session is
  attached, and the durable stream is as long as it was, with three changes of desired
  state in it. The session then runs a turn; under it the old stop intent and a new one
  are both refused, and both starts sent again are their receipts and change nothing of
  the turn. After the turn, stop C under a new intent commits the next revision, the
  session is detached and the child is gone; starts sent again then launch nothing. On
  the code before this closure the test fails where the session is found detached.
- Across restarts: stop A commits and the authority restarts; A is answered with its
  receipt while the runtime stays stopped, again after a later start and resume, and
  again after a second restart in which the authority brought the runtime up by itself.
  Nothing changes any of the three times, and a new stop still stops.
- A binding request kept in flight by the fixture, before it is written to the provider:
  stop A sent again and a stop at a stale revision arrive meanwhile; released, the
  request is written once and the session is bound. On the code before this closure
  that binding became indeterminate. With its lease revoked instead, the request is
  never written and the session stays unbound.
- Thirteen mutation controls, each killed by a test, one of them only after the last
  test above was added: the unchanged answer ignored; a detach on every stop the core
  answers; a receipt answered again that still reconciles; the latest receipt taken for
  a fresh transition; the receipt compared against nothing carried out; a new stop
  suppressed, or reconciled without detaching; a new start suppressed; the admission of
  a binding request not given back, given back after a change, or never withdrawn. A
  reconciliation on an unchanged revision changes nothing a process test can see, so
  that control is killed at the decision.
- Desktop smoke on the release build of `06d479b`, in the nested compositor: a session
  in Inference, a Local Tools session that restarts the runtime, **Use Inference** back
  again; the developer client stops the runtime under intent A and starts it, the
  session is resumed from the window and is Ready; A sent twice more answers revision 6
  while the runtime stays ready at revision 7 on the same instance with no new event,
  and the window still shows the session Ready after a refresh; a new stop takes
  revision 8, the runtime stops and the window shows the session detached; the terminal
  opened at launch takes input throughout.
