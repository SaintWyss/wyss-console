# ADR 0021: Session activation and workspace binding from the desktop

Status: Accepted
Date: 2026-10-09

## Decision

A managed Codex session can be created and made usable entirely from the window. The
user chooses a project folder in a native dialog and presses one button; the session
ends bound to that folder and attached to the managed runtime, ready for a turn. No
developer client is needed on that path.

```text
Svelte            the user's action, a session it was shown, a folder the user selected
  -> Tauri        one activation step at a time; which operation a step takes is decided here
  -> SessionService (src-tauri/src/sessions)
  -> shared ControlClient, one short-lived connection per step
  -> wyss-control  decides every step
  -> Codex runtime launched, supervised and attested by the authority alone
```

The desktop composes operations the core already accepts. Control Protocol v1 is
unchanged: no method, field, schema or storage version was added. The desktop decides
none of what is the authority's: whether a folder may be a workspace, whether a lease is
valid, whether the runtime may start, whether a binding may be created, whether a
provider thread exists, whether an external effect happened, whether a session is ready.

Managed turns and chat are not part of this. Nothing here sends a prompt.

## The contract activation conforms to

Read from the core before anything was written (ADRs 0012, 0013, 0014, 0018):

| Operation                        | What the authority does                                                                                                                                                                                                                                  |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `runtime/status`                 | Desired `running`/`stopped` and actual `stopped`/`starting`/`initializing`/`ready`/`stopping`/`failed`, with a diagnostic.                                                                                                                               |
| `runtime/start`                  | Every new intent commits desired `running` and one more control revision. A worker is launched only if none exists: a start while one is starting or ready launches nothing. A failed runtime is not restarted by itself; a new start intent admits one. |
| `executionGate/status`           | Why the gate denies an action now. Informational. It names the lease before it names the runtime.                                                                                                                                                        |
| `executionLease/grant`           | One active lease per session, with scopes and a lifetime of 1 to 3600 seconds. `lease_already_active` while another stands.                                                                                                                              |
| `executionLease/revoke`          | Revokes an active lease at its revision.                                                                                                                                                                                                                 |
| `managedSession/runtime/create`  | Admitted under `session_create` with the runtime ready. Validates and canonicalizes the workspace **before** the gate. Creates one provider thread and returns the binding **detached**. No exactly-once guarantee.                                      |
| `managedSession/runtime/resume`  | Admitted under `session_resume`. Attaches an existing binding to the running runtime.                                                                                                                                                                    |
| `managedSession/runtime/status`  | `unbound`, `creating`, `indeterminate`, `bound_detached`, `bound_attached`, and the open operation.                                                                                                                                                      |
| `managedSession/runtime/abandon` | Gives up on an indeterminate create, named by its operation. Audited. Removes no provider thread.                                                                                                                                                        |

Three facts of that contract shape everything below:

- **A binding is created detached.** Creating never attaches. A usable session takes two
  gated operations with two different scopes, and a session has one active lease at a
  time.
- **Every processed intent is remembered with its canonical payload.** The same intent
  with the same payload returns the original receipt and changes nothing; with another
  payload it is `intent_conflict`. A receipt is what was true when it was written: the
  receipt of a lease still says `active` after the lease expired, and the receipt of a
  resume says nothing about the runtime that is running now.
- **A create whose provider outcome is unknown blocks the session** as `indeterminate`
  until it is abandoned. The authority never retries it.

## Activation is taken one step at a time

There is no transaction across those operations: any of them can succeed and the next
one fail. So the workflow is not written as one. `managed_session_activate` takes **one
step**. A step reads what the session and the runtime are now, asks the authority for
the one operation that is missing, and answers how far that got. The window sends the
same request again until the answer is `ready`.

| The authority reports          | The step asks for               | Answer                    |
| ------------------------------ | ------------------------------- | ------------------------- |
| unbound, runtime not up        | `runtime/start`                 | `runtime_starting`        |
| unbound, runtime starting      | nothing                         | `runtime_starting`        |
| unbound, runtime ready         | `managedSession/runtime/create` | `bound`                   |
| creating                       | nothing                         | `creating`                |
| bound, detached, this action's | `managedSession/runtime/resume` | `ready`, once it was read |
| bound, attached, this action's | nothing                         | `ready`                   |
| bound by something else        | nothing                         | `already_bound` (refused) |
| indeterminate                  | nothing                         | `uncertain` (not refused) |

This gives the properties the phase needs without a workflow engine:

- Every state the user can be left in is a state the authority has: prepared and unbound,
  creating, indeterminate, bound and detached, bound and attached. No "active" exists
  that the authority does not hold.
- A step is re-entrant. A retry after an unknown outcome, a second window, a restart of
  the desktop or of the authority all continue from what is canonical.
- Nothing is in flight between steps. Closing the window interrupts at most one
  exchange.
- Progress is real: each stage is an answer.

`ready` is answered only after the session was read as attached on the same connection.
A resume receipt alone is not taken for an attachment.

## Workspace selection boundary

The window is given no filesystem. `choose_workspace` takes nothing and shows GTK's own
folder chooser (`GtkFileChooserNative`, select-folder action), modal to the main window,
on GTK's thread. The page cannot draw on it, read from it or suggest a starting folder.
It can select a folder and nothing else. What crosses back is the one path selected, or
null when the dialog was dismissed.

- GTK is already what draws the window; no dialog plugin, portal client or dependency
  was added, and no framework permission (`dialog:`, `fs:`, `shell:`, `core:`) is
  granted. The capability file names the application's own commands only.
- The window names no path of its own. The desktop remembers the paths the dialog
  returned in this run (eight at most) and `managed_session_activate` is refused,
  before anything is asked, for a `workspace` that is not exactly one of them. A window
  that was taken over can activate a session in a folder the user picked in the dialog
  during this run, and in no other.
- A path the protocol cannot carry (not UTF-8, longer than 256 bytes, a control
  character) is refused by the dialog command instead of being cut to fit.

### The folder is a request

Selecting a folder authorizes nothing. The path goes to `wyss-control` exactly as it was
selected, and the authority validates it as it always has: absolute, existing, a
directory, canonicalized (a link is bound as what it resolves to), within its length
bound, and, for an isolated local-tools generation, the one workspace that generation
serves. The desktop does not canonicalize, compare or pre-approve.

The authority validates the workspace before the gate, so a refused folder costs no
lease and no provider request. It is reported as `workspace_invalid`, separately from a
runtime that is not ready.

## Runtime readiness and the start decision

**Activation asks the authority to start the runtime.** The alternative, a separate
"start the agent" control the user has to find first, makes the normal path depend on
knowing the runtime exists. Pressing "Create & activate" is an explicit act whose
meaning includes "have Codex running", and `runtime/start` is an accepted operation
whose admission is the authority's.

What the desktop does and does not do:

- It reads `runtime/status` first and asks for a start **only when nothing is on its
  way**: desired stopped, or a runtime that is stopped or failed. While one is
  starting, initializing or ready, no start is sent.
- It never launches Codex. It sends one typed request; the authority launches,
  supervises, attests and reaps the runtime, or refuses.
- It asks for the default profile: no workspace is named and none is isolated for. The
  local-tools profile serves one workspace per generation and is a decision for the
  phase that runs tools.
- A generation the authority contained for safety (`inference_isolation_violation`,
  `safety_unproven`) is **not** restarted from here. The step answers
  `runtime_unavailable` and sends nothing. Restarting it stays a deliberate act with
  the developer client. This is the desktop declining to ask, never permitting.
- One start for one action. The start intent is the action's own (see Intents). If the
  authority answers it with the receipt it already gave, the start this action asked
  for did not bring a runtime up: the step answers `runtime_unavailable` instead of
  asking again. A new click is a new request.
- A start the authority refuses is `start_refused`; one it could not answer is unknown
  and retryable.

### Waiting

`runtime_starting` and `creating` mean "ask again when something changed". The window
does not poll for that. It already follows the authority view (ADR 0019), which changes
when the runtime changes state and when a durable event is committed: that change ends
the wait and the same request is sent once. No second follower, `audit/watch` or
connection exists for it.

Should a change never be reported, the request is sent again every five seconds, and
after sixty seconds of waiting in all the action ends as `runtime_unavailable`,
retryable. The authority's own limits (ten seconds for a runtime request and for a
binding) are far below both.

## The `session_create` lease

Binding is gated, and the desktop neither bypasses the gate nor holds permission. The
sequence is the one resume already used (ADR 0020), now shared by both:

1. Ask for the operation. Under a lease already in place, or for an intent the
   authority already holds, this is the whole exchange.
2. On `gate_denied`, read `executionGate/status`.
3. Only if the reason is a lease that is missing, not active or expired: ask the
   authority to grant one.
4. Ask again, with the same intent.
5. Ask the authority to revoke the lease it granted.

For a binding the lease is exactly `session_create`, for **fifteen seconds**. The
authority checks the lease when it admits the request and once more as it writes to the
runtime, both moments after the grant; the binding it admitted finishes whatever the
lease does afterwards. Fifteen seconds covers those checks and the core's queue. The
attachment that follows takes its own lease, exactly `session_resume`, for thirty
seconds, as before: the first one is revoked before the second is asked for, because a
session has one active lease.

No other scope is ever requested: not `turn_start`, not `command_execute`, not
`file_change`, and never two scopes in one lease.

**A lease somebody else holds** is used as it is when it grants the scope, and obeyed
when it does not: the operation is denied and the lease is neither widened, renewed,
replaced nor revoked. The desktop revokes only a lease whose identity came back from its
own grant.

### When the lease cannot be given back

If the answer to the grant or to the revoke is lost, the desktop does not know whether
a lease of its own stands. It does not pretend to. The grant, exactly as it was sent,
and the identity of its revoke are kept in memory until the lease would have expired by
itself. The next call for that session settles it first:

- Revoke not answered: the same revoke is asked again, with the same identity.
- Grant not answered: the authority is asked whether a lease stands. If none does,
  nothing of the desktop's stands and the record is dropped; the grant is **not** sent
  again, because sending a grant that was never taken would grant it. If one does, the
  same grant is sent: the authority answers with the receipt of the desktop's own lease
  or refuses because the standing lease is somebody else's. Only a lease named by that
  receipt is revoked.

If no further call comes for that session, nothing is asked: the lease expires within
its fifteen or thirty seconds and the authority audits `execution_lease.expired`.

## Binding

`managedSession/runtime/create` with the session, the revision the window saw and the
folder. The authority creates one provider thread with its durability barrier (start,
archive, unarchive) and commits the binding, detached.

Before attaching a session that is already bound, the step asks whose binding it is.
The authority answers this action's own create request with its receipt only if this
action made the binding; for any other binding it refuses, and nothing can be created
because the session is bound. A binding this action did not make is reported as
`already_bound`: it is not attached in the name of a folder the user did not get.
Resume is the action for it.

The provider thread, the runtime instance, the lease and the operation stay on the Rust
side. The window sees the binding state and the workspace.

## Provider uncertainty

Creating a binding may create a thread at the provider, and there is no exactly-once
guarantee for it. The desktop adds none and claims none.

- When the authority answers `external_effect_unknown` or `binding_indeterminate`, the
  action ends as `uncertain`. It is not retryable, it is not worded as a refusal and it
  is not drawn as an error.
- While the authority holds the attempt as indeterminate, **no binding request is
  sent** for that session: not with the same identity, not with a new one, not by a
  retry, a new click, a restart of the desktop or of the authority. The step reads
  `indeterminate` and stops there.
- While the authority is still waiting on the runtime (`creating`), nothing is sent
  either.
- An answer that was lost in transit is a different case: the outcome is unknown to the
  desktop, not to the authority. Retry sends the **same** binding intent with the same
  payload. If it had committed, the authority returns its receipt; if it is still
  running, `runtime_operation_in_progress`; if the authority holds it as indeterminate,
  `binding_indeterminate`. A second thread cannot come out of it.

The way out of `indeterminate` is the core's own: `managedSession/runtime/abandon`. The
window offers it as **Discard attempt**, asked about once and said plainly: the
authority records that the attempt was abandoned, and a thread Codex may have created
for it is left behind. The window names no attempt; the desktop abandons the operation
the authority reports. After that the session is unbound and a new action can bind it.
That new binding is a new provider thread by the user's explicit choice, which is the
trade the core defined (ADR 0014).

## Intents

The window names the whole user action once (`operation_id`, minted when the action
starts and kept across its retries). It is used as it is for `managedSession/prepare`.
Every other mutation of the action has an identity of its own:

| Mutation                         | Identity                                                                   |
| -------------------------------- | -------------------------------------------------------------------------- |
| `managedSession/prepare`         | The action's own.                                                          |
| `runtime/start`                  | Derived from the action: SHA-256 over a domain tag, `start`, the identity. |
| `managedSession/runtime/create`  | Derived, `create`.                                                         |
| `managedSession/runtime/resume`  | Derived, `attach`.                                                         |
| `executionLease/grant`           | Fresh for each grant; kept while its answer is unknown.                    |
| `executionLease/revoke`          | Fresh for each lease; kept until it is answered.                           |
| `managedSession/runtime/abandon` | The identity of that action (Discard attempt).                             |

A derived identity is a function of the action and the step, so a step asked again is
the same intent with the same payload, with nothing to persist and nothing to lose on a
restart. No two operations share an identity: the authority compares an intent with the
payload it was first used for, so reusing one across operations would be a conflict.

The leases are not the user's action. A lease receipt can be stale, so a grant identity
is reused only to learn which lease an unanswered grant produced, never to obtain a
lease again.

What the product guarantees is therefore: idempotent authority intents, canonical
recovery and explicit uncertainty. It does **not** guarantee exactly-once across the
workflow, and it does not guarantee that an activation is atomic.

## Canonical recovery

Events remain wake hints and the view is always a page that was read (ADR 0020). During
an activation the answers of its steps drive the wording of the progress line; what a
session is shown to be comes from pages. After `bound` the page is read, and shows the
session detached. After `ready` the page is read, and that page is what shows `Ready`.

`SessionPage` is version 2: two more presentation hints, `activatable` (prepared, with
no binding and no attempt at one) and `recoverable` (prepared, with an indeterminate
attempt). Like `resumable` and `closable` they exist so the window encodes no rule, and
they authorize nothing.

After an authority restart the bindings are there, detached. An activation that was
interrupted before it attached goes on: the authority answers its binding request with
the receipt it kept, and the session is attached to the new runtime. An activation that
had already attached is not replayed into a new attachment: its attach intent is one
the authority already answered, the session is read as detached, and that is what is
reported. Resume attaches it.

## Navigation and shutdown

The sessions controller is created once with the application (ADR 0020). An activation
belongs to it, not to the view: switching between Console and Sessions neither sends a
second request nor drops the one in flight, and the wait is ended by the same authority
observation whichever view is shown. A second click while an action is pending sends
nothing.

Closing the window while an activation runs:

- The exchange in flight is cancelled within one poll interval and no later call is
  admitted. Nothing is sent to the authority.
- No compensation is attempted. The desktop cannot undo a provider effect and does not
  pretend to: it sends no abandon, no close, no stop.
- What the authority and the provider hold is what there is. A lease the desktop was
  granted and did not get to revoke expires by itself.
- The next launch reads it: prepared and unbound, bound and detached, or uncertain,
  each with the action that fits it.

## Failures

The bridge still answers a product code and a retry hint and nothing else. Three codes
were added to ADR 0020's table:

| Code                | From                                                                              |
| ------------------- | --------------------------------------------------------------------------------- |
| `start_refused`     | `runtime/start` refused by the authority.                                         |
| `workspace_invalid` | `workspace_invalid` on a binding; a path the dialog returned that cannot be sent. |
| `already_bound`     | `binding_already_exists`; a binding this action did not make.                     |

`invalid` now also covers a `workspace` the dialog did not return.
`runtime_unavailable` also covers a runtime contained for safety, a start that brought
nothing up and a wait that reached its limit. `uncertain` is never retryable.

In the window a failed action whose outcome is not known (`unavailable`, `uncertain`)
is drawn as a caution; a refusal keeps the error tone.

## Presentation

The view holds the shell's existing tokens, pills and accent. The style pipeline's
direction layers were not rerun: the request fixed the direction as the existing one.

- **New session** opens a composer with the project (the folder as the dialog returned
  it, or "No folder selected"), the agent, **Choose folder**, **Create & activate** and
  **Cancel**. Nothing can be typed. The button is disabled until a folder is chosen.
- The same composer activates an existing prepared session with no workspace, from its
  details (**Activate**). No second session is created.
- Progress is one line: "Preparing session…", "Starting Codex…", "Creating workspace
  binding…", "Attaching…". Which one is shown follows from the request in flight, the
  last answer, the page and the runtime the authority view reports.
- A session bound and attached is shown as **Ready**. Detached keeps **Resume**. An
  uncertain one shows what happened and **Discard attempt**, and offers neither
  Activate nor Resume. A closed one offers nothing.

Creating a prepared session with no workspace is no longer offered by itself.

## Security boundary

- The trust boundary is unchanged: the authority's same-UID `SO_PEERCRED` check.
- The window's reach grew by two intents and one dialog. A window that was taken over
  can now also: ask for the runtime to be started with the default profile; bind a
  prepared session to a folder the user selected in the native dialog during this run,
  and attach it; give up on an attempt the authority holds as uncertain. It still
  cannot name a path, a control method, an adapter, a scope, a lease, a lifetime, a
  safety profile, a provider thread or a runtime; cannot stop the runtime; cannot start
  a turn or answer an approval.
- Leases the desktop can cause: `session_create` for fifteen seconds and
  `session_resume` for thirty, each around one operation of its own, each revoked or
  left to expire. Nothing wider.
- The dialog is GTK's. It runs in the desktop process and shows the user's filesystem
  to the user, not to the page.
- A same-user process impersonating the authority on the socket path (ADR 0019) now
  also receives the selected folder path.

## Limits and residual risk

- **No atomicity and no exactly-once.** Stated above; it is the core's contract.
- **A lease can outlive a lost connection** by fifteen or thirty seconds when no further
  call is made for that session.
- **A redundant start is possible** if another client starts the runtime between the
  status read and the start. The authority launches no second worker; it does commit
  one more control revision and one more audit event.
- **An abandon whose answer is lost** and that had committed answers `not_allowed` when
  retried, because the session no longer has an attempt to name. The page shows the
  session unbound.
- **The default profile only.** A session bound under one runtime profile keeps its
  thread in that profile's Codex home (ADR 0016). Choosing the local-tools profile is
  left to the turn phase, which is the one that needs it.
- **128 attachments per runtime generation** is the core's bound. Closed sessions keep
  theirs until the runtime restarts; past it a binding or a resume answers `busy`.
- **A runtime starts only from the build tree.** The runtime's working directory is
  still compiled into the binary (ADR 0018, residual risks), so an authority moved away
  from its build tree refuses to start one, and activation reports that.
- **The dialog's memory is this run's.** After a restart a folder has to be selected
  again. There is no list of recent projects.
- GTK decides between its own chooser and a portal one. The acceptance used GTK's own.
- The non-Linux build of the bridge is a stub that answers `unavailable`. It was not
  compiled here.
- The real-desktop run used a synthetic session with GTK's accessibility bridge
  disabled, as in ADR 0020.
- As with ADRs 0019 and 0020, no party other than the session that built this reviewed
  it. Its tests were checked with seventy-seven mutation controls instead, which is not
  the same thing.

## Deferred

Managed turns, streaming and approvals in the window; the local-tools profile; runtime
stop from the desktop; a list of recent projects; lease management as a visible concept;
other adapters.

## Acceptance evidence

Accepted on Linux (CachyOS, KWin 6.7.5). Every authority ran in an isolated temporary
XDG directory and every workspace was an empty temporary folder. No production state of
the authority was read or written.

**Service, against the real `wyss-control` process**
(`src-tauri/tests/sessions_process.rs`, the Codex fixture as the runtime):

- From a stopped runtime: the first step commits one `desired_state_changed` and
  nothing else; the next binds under a lease audited as `["session_create"]` with
  fifteen seconds between grant and expiry, `granted`, `runtime_bound`, `revoked` in
  that order, one `thread/start`, `thread/archive` and `thread/unarchive` and no
  `thread/resume`; the next attaches under `["session_resume"]` for thirty seconds.
  No lease is left. The same action three more times writes nothing and sends nothing
  to Codex. A second session is bound with no second start.
- The authority killed in the middle of an activation: nothing is compensated, both
  bindings are there detached, the interrupted action attaches with no second thread,
  and the finished one is reported as not attached instead of being replayed.
- A missing folder, a file, a relative path and one of three hundred bytes: refused as
  `workspace_invalid` with no event, no lease and no thread. A link is bound as what it
  resolves to.
- A lease another client holds: one without the scope denies and is left intact; one
  with both scopes is used for both steps and is still theirs. A session another client
  bound is `already_bound`, with their lease standing and without it. Closed, unknown
  and stale sessions are refused with no lease left.
- A Codex that never answers the thread request: `uncertain`, not retryable; the
  session `indeterminate`; the same action, a new action and an authority restart send
  no second `thread/start`. Abandon is audited with the authority's operation and the
  action's intent; then a new action binds and attaches it.
- Twelve activations next to a running observer: six connections each, all closed, the
  observer's one connection never lost, the same descriptors and threads as before,
  one runtime start in all, as many leases revoked as granted and none expired.

**Service, against a scripted peer** (`src-tauri/src/sessions/activation_tests.rs`):
the exact request sequence of every step, its scopes, lifetimes and identities; a start
asked once and never while starting; every refusal of a start; a contained runtime;
a revoke and a grant whose answers are lost, in both the "taken" and "never taken"
cases, with somebody else's lease standing; an uncertain binding under the same and a
new identity; a binding answer lost and settled by the same intent; a binding this
action did not make; a receipt of an attachment with no attachment; shutdown cancelling
a binding nobody answers; abandon.

**Bridge** (`src-tauri/src/desktop`): each request accepts exactly its fields; a
`workspace` is admitted only when the dialog returned exactly that text; the capability
and the build manifest name exactly the known commands.

**Window** (`tests/session-activation.test.ts`, `tests/sessions.test.ts`,
`tests/sessions-view.test.ts`): the dialog flow; create and activate as one identity;
an existing session; waiting on the authority view, the sparse recheck and the limit;
navigation; refusals, lost answers and uncertainty; the authority lost in the middle;
discard; and the mounted application for each of them.

**Mutation controls.** Seventy-seven mutants of this workflow, each killed by a test:
thirty-three in the window and forty-four in Rust. Among them: another scope, two
scopes and a turn scope in the lease; a lease of an hour; a lease never revoked, asked
for first, or asked for whatever the gate says; a fresh binding identity on every step;
the attach or the start sharing the binding's identity; a step borrowing the action's
identity; a binding sent after an unknown outcome or while one is being created; a
foreign lease revoked; a lost grant sent again blindly; a lost revoke or grant
forgotten; a record of an expired lease still asked about or never dropped; any
binding taken as the action's own; a start while starting; a replayed
start taken as progress; a contained runtime restarted; a receipt taken as an
attachment; uncertain marked retryable or worded as a refusal; a window-named path, a
path matched by prefix, an unvalidated dialog path; requests accepting unknown fields;
a generic command and a filesystem permission in the capability; and, in the window, a
second click, a retry under a new identity, a second session on retry, polling instead
of waiting, a wait without a limit, a folder kept across composers, a suggestion passed
to the dialog, an attempt named by the window, and ready shown before it was read.

**Real Codex.** One bounded run, through the real desktop (below), with the real
`codex` 0.159.0 as the authority's runtime and the user's own Codex login, read in
place and not copied. The authority's stream for it, with the time of each commit:

```text
00.000  managed_session.prepared
00.004  runtime.desired_state_changed        running
01.990  runtime.instance_ready               0.159.0
01.993  execution_lease.granted              session_create
02.816  managed_session.runtime_bound
02.817  execution_lease.revoked              session_create
02.819  execution_lease.granted              session_resume
02.916  managed_session.runtime_attached
02.916  execution_lease.revoked              session_resume
```

The authority then reported the session `bound_attached` to the selected folder under
the inference-only profile, no active lease, and these provider methods written in all:
`initialize`, `initialized`, `model/list`, `config/read`, `hooks/list`, one
`thread/start`, one `thread/archive`, one `thread/unarchive`, one `thread/resume`.
**No `turn/start`**: no prompt was sent and no model turn ran. Its one trace outside
the sandbox is one new two-line rollout file in the user's Codex home, the empty thread
of that binding. The runtime was then stopped through the authority.

**Real desktop.** The release `wyss-console` on a nested X11 display managed and
composited by KWin/X11 (rootful Xwayland on a headless compositor, so no input reached
the developer's session), with an isolated home, against the release `wyss-control`.
Driven with real pointer and keyboard input and checked against the authority through
the developer client. The run was made on the binaries of `5183753`. The caution
styling of `e66bd5c` changes markup and CSS only; on that binary the real Codex run
and the uncertain view were made. `271318e` drops lease records that outlived their
lease, which no step of the run depends on; on its binary a last create and activate
was made, from a stopped runtime to `Ready`, and the window closed.

- **Sessions and New session.** Two sessions prepared beforehand were listed. The
  composer showed "No folder selected" and a disabled **Create & activate**; clicking it
  sent nothing.
- **Native folder selection.** GTK's chooser opened modal over the window. Cancel left
  the composer as it was and the authority with two sessions. In it a file is greyed
  and cannot be selected. A selected folder appeared in the composer as its path.
- **Create & activate, runtime stopped.** One click. The window showed "Starting Codex…"
  with the top bar's agent pill at `Starting`, then the session `Ready` with its
  workspace. At the authority: `prepared`, `desired_state_changed`, `instance_ready`,
  `granted [session_create]`, `runtime_bound`, `revoked`, `granted [session_resume]`,
  `runtime_attached`, `revoked`. With the real Codex the line read "Starting Codex…"
  for two seconds and then "Creating workspace binding…".
- **An existing session.** One of the two prepared beforehand: **Activate** in its
  details, a folder, **Activate**. Bound and attached to that folder; no session was
  created and no start was asked for.
- **Double click and navigation.** A double click on **Activate** followed by four view
  switches: one binding and one attachment for that session at the authority.
- **A folder the authority refuses.** A folder selected in the dialog and removed
  before the click. The session was prepared; the window said the authority did not
  accept the folder and that nothing was bound, with no Retry; the authority held one
  `prepared` and nothing else; the session offered **Activate**.
- **Authority stopped and restarted (SIGTERM), then killed (SIGKILL).** Offline, the
  sessions were cleared and every control disabled. Back, without restarting the
  desktop, they were listed with the bound ones `Detached`. **Resume** attached one:
  `granted [session_resume]`, `runtime_attached`, `revoked`.
- **Window closed during a binding.** With a Codex that never answers, the window
  showed "Creating workspace binding…", the authority `creating` and an active
  `session_create` lease. The window was closed: the desktop ended by itself with no
  panic. Afterwards the authority held the attempt as `indeterminate`, audited, and the
  lease the desktop could not give back expired by itself (`execution_lease.expired`).
  Nothing else was written.
- **Uncertain, and its recovery.** The next launch showed that session `Uncertain` with
  "Activation outcome uncertain" and **Discard attempt**, and offered neither Activate
  nor Resume. Discard, after its confirmation, left `runtime_operation_abandoned` at
  the authority and the session with **Activate**. With the window kept open through a
  second unanswered attempt, the runtime had failed: the window asked the authority to
  start it (`desired_state_changed`), the attempt ended `indeterminate`, the lease was
  revoked, and the window said "Activation outcome uncertain", drawn as a caution, with
  no Retry. After a discard and an authority that answers, the same session was
  activated to `Ready`. Each of the five `thread/start` the fixture had received by
  then was one binding the user asked for: three answered and the two unanswered
  attempts. None was sent by itself.
- **Console.** One terminal per desktop run: the same shell process before and after
  an authority restart, five activations with their dialogs, ten refreshes and twelve
  view switches, with what was typed before all of it still in its scrollback.
- **Resources.** 44 descriptors, 9 sockets and 42 threads at launch; 52, 10 and 51
  once the folder dialog had been used, which is GTK's own one-time cost; the same 52,
  10 and 51 after four more activations with their dialogs, ten refreshes and twelve
  view switches. One connection at the authority throughout: the observer's.
- **Sizes.** At 960×640 the composer is one row above list and details. At 600 px its
  buttons wrap; at 380 and 320 px they stack. Nothing overflows.
- **Window material.** `_KDE_NET_WM_BLUR_BEHIND_REGION` was set on the composited
  window. As before, the blur itself was not looked at on a GPU session, and nothing in
  `desktop/glass.rs` or window creation changed.
