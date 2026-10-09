# ADR 0022: Managed turns and the Agent view

Status: Accepted
Date: 2026-10-09

## Decision

A Ready managed session can be given an objective from the window. The user opens the
session in the `Agents` view, writes what Codex should do and presses Enter; the
authority starts one managed turn for it, the answer is shown as it arrives, and the
turn ends in the state the authority recorded.

```text
Svelte            the user's action, a session or a turn it was shown, the text the user wrote
  -> Tauri        four narrow commands: two reads, two intents
  -> TurnService  (src-tauri/src/turns), over SessionService
  -> ControlClient
  -> wyss-control  decides every start, every cancel, every lease and every approval
  -> managed Codex
```

The desktop composes operations the core already accepts. Control Protocol v1 is
unchanged: no method, field, schema or storage version was added. The desktop decides
none of what is the authority's: whether a turn may start, whether a lease is valid,
whether a tool may run, whether an approval is needed, what the canonical output is,
whether a cancel succeeded, whether an external effect happened.

Approvals are not answered here. A turn that waits on one is shown waiting; nothing in
the window can approve, deny or edit, and no command to do so exists.

## The contract turns conform to

Read from the core before anything was written (ADRs 0014, 0015, 0016, 0017, 0018):

| Operation                              | What the authority does                                                                                                                                                                                                                           |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `managedSession/turn/start`            | Admitted by the execution gate under an active lease with `turn_start`, an attached binding and a ready runtime of the matching profile. Takes the session, the revision the caller saw and a prompt of 1 to 8192 bytes. One turn runs at a time. |
| `managedSession/turn/status`           | The turn's record: `starting`, `running`, `completed`, `interrupted`, `failed`, `indeterminate`, its revision, when and why it ended, and whether its stream is still held.                                                                       |
| `managedSession/turn/events`           | A page of the turn's stream after a sequence, at most fifty events. No blocking form exists.                                                                                                                                                      |
| `managedSession/turn/cancel`           | Asks the provider to interrupt, at the revision the caller saw. Idempotent for the same intent; `invalid_transition` for a new intent once the turn ended.                                                                                        |
| `managedSession/turn/approval/pending` | The approval the turn waits on, when it waits on one.                                                                                                                                                                                             |
| `executionLease/grant`, `revoke`       | One active lease per session, scopes and a lifetime of 1 to 3600 seconds.                                                                                                                                                                         |
| `audit/read`                           | The durable event stream, in pages.                                                                                                                                                                                                               |

Facts of that contract that shape everything below:

- **A turn runs under the lease it was admitted under, for as long as it runs.** When
  that lease expires the turn ends `lease_expired`; when it is revoked, `lease_revoked`.
- **Wyss stores no conversation text.** The prompt is fingerprinted and passed on. The
  output stream is a ring held in memory: 256 events or 256 KiB for a turn, for the last
  eight turns, and gone when the authority restarts.
- **There is no state named "waiting for approval".** A turn that waits is `running`
  with a pending approval.
- **There is no operation that lists turns.** A turn's admission is a durable event,
  `authority.managed_turn.admitted`, that names its session and the turn.
- **The authority has no stream for a turn that has put nothing out yet.** It answers
  such a read as a gap, with a marker at the position after the cursor it was asked
  from. The same answer is given for a turn whose stream it no longer holds.
- **A nonterminal turn found at startup becomes `indeterminate`**, reason
  `authority_restarted`.
- **A session activated from the window runs in the inference profile**, where Codex
  is given no tools (ADR 0014, 0021). Tool activity and approvals only occur in a
  local-tools runtime, which the window cannot start.

## Starting a turn

`managed_turn_start` takes an intent identity, a session, the session revision the
window was shown and the text. It takes nothing else, and unknown fields are refused:
the window cannot name a provider method, a thread, a runtime instance, a lease, a
scope, a lifetime, a model, a tool, a sandbox or a safety profile.

The text goes to the authority as one typed field. It is never part of a command line,
a path or a log: the request's `Debug` form prints `StartTurnRequest(<sensitive>)`. A
text the protocol cannot carry is refused before anything is asked.

One start is one user action with one identity, made when the user submits. Sent again
unchanged it is the same turn. The window never makes a second identity for a start
whose answer was lost: it offers **Retry**, which sends the same identity, the same
text and the same revision, and it holds the composer until that start is retried or
dismissed.

## The turn's lease

The desktop asks first. Only when the gate names a lease as what is missing does it ask
the authority for one: scope `turn_start` alone, for 1800 seconds.

- **It is kept while the turn runs.** A session operation gives its lease back as soon
  as it was answered (ADR 0020, 0021). A turn cannot: revoking its lease ends it. The
  lease is given back by the reading that finds the turn ended.
- **1800 seconds is the turn's time budget when nobody gives the lease back**, for
  example because the desktop closed. It is not renewed. A turn that needs longer ends
  `lease_expired`, and the window says so.
- **A lease somebody else holds is used as it is and never touched**: neither widened,
  nor replaced, nor revoked.
- **A start whose answer is lost keeps the lease**, because the turn may be running
  under it. The same intent settles it.
- A lease kept for an earlier turn that ended without the desktop reading its end is
  given back before the next start, so every turn is admitted under a lease of its own.
- A start the authority refuses leaves no lease behind.

Nothing about a lease reaches the window.

## Reading a turn

`managed_turn_read` takes a turn, the cursor the last reading returned and whether to
wait. It answers the stream events after the cursor, then the turn's canonical record,
then the approval it waits on, read in that order on one connection.

- **Cursors are opaque decimal text**, handed back unchanged. The window never computes
  one.
- **A turn that has put nothing out is not a gap.** The authority's marker for it is at
  a position derived from the cursor, not a position in a stream; taking it as the next
  cursor steps over the first real event. Such a reading is returned empty with the
  cursor where it was. It is a gap only when the turn has ended, and then the cursor
  still does not move.
- **A turn shown as ended comes with a stream read after it ended.** If the record says
  the turn ended and the events were read before that, the events are read once more.
  A follower that stops at "ended, nothing more" therefore has all of the output.
- What the authority dropped is reported as a gap and shown as one.

### What crosses

| Stream event                     | What the window is given                         |
| -------------------------------- | ------------------------------------------------ |
| Assistant message delta          | Its text. The only provider text that crosses.   |
| Reasoning summary                | That the model is reasoning. No text.            |
| Command proposed, started, ended | That it happened. No command, directory or text. |
| Command output                   | That there was output. No text.                  |
| File change                      | That it happened. No path and no diff.           |
| Approval required                | That one was asked for.                          |
| Approval resolved                | The state it ended in, if the contract names it. |

The record crosses as state, revision, times, reason and whether output is still held.
The provider thread and turn, the lease, the runtime instance and the approval's
identity, fingerprint and content do not cross. Raw reasoning never leaves the
authority in the first place.

### Following

The core has no blocking read for the stream, so following a turn is a paced read in
the desktop: it looks every 200 ms and answers as soon as there is an event or the turn
has ended, or after 1.5 s with what there is. The window asks again from the cursor it
was given. Nothing is read while no turn is followed.

Readings use connections of their own, one at a time, so a session call never waits
behind one. Admission is counted apart from session calls and terminals: four reads and
two intents at most.

## Finding the turns of a session

`managed_turns` reads the durable stream for admission events, once from at most two
thousand events behind its head and then only what was appended, and asks the authority
for the record of each of the last twenty turns of the session. When more was appended
than one read covers, the newest events are the ones read. Either way the list says
that older turns exist that it did not read.

A record that names another session than the stream did fails the list.

## The Agent view

`Agents` is enabled in the sidebar. A session is opened in it from its details in
`Sessions` (**Open agent**, offered when the session has a workspace). Up to eight
sessions stay open as tabs. The session every turn on screen belongs to, and every
turn started goes to, is always named above the transcript with its binding state and
workspace.

Each submission is a real managed turn. The view is a list of turns, oldest first, not
a free-form chat:

- the input, for turns started in this window since it opened; for any other turn it
  says that Wyss keeps no conversation text;
- the answer, as plain text, in the order of the stream;
- mediated actions as one line each, named by what happened;
- the turn's state by the authority's own names: Starting, Running, Completed,
  Interrupted, Failed, Indeterminate, with the recorded reason in one sentence.

The composer is a multiline field. Enter runs, Shift+Enter is a new line, and an Enter
that ends an input-method composition is not a submission. Run is not offered without a
Ready session, with a turn running, with a start unsettled or with a text that is blank
or longer than a turn can carry; the hint says which. There are no templates and no
model, skill or tool selectors.

**Text is rendered as text.** Nothing Codex or the user wrote is interpreted as HTML or
Markdown; line breaks are kept by `white-space: pre-wrap`.

### State lives above the view

`AgentController` lives for the application's lifetime and is fed by the shell's one
authority observation. Going to another view cancels nothing and starts nothing; the
turn goes on being read and the draft is kept.

**One follower.** At most one reading is in flight, whatever the number of sessions
open. It reads what the authority still holds of the shown session's ended turns, then
follows the turn that has not ended. A reading that brought nothing waits 250 ms before
the next, a failed one is tried again after two seconds or when the authority changes,
and an ended turn is never read twice.

The list of a session is read when the authority's durable head moves, and only while
the view is shown.

### Waiting on an approval

Shown as a notice on the running turn: what kind of action was asked for and until
when, and that this view cannot answer it. The turn stays `Running`, because that is
what it is. Cancelling the turn is the one action offered.

### Cancel

`managed_turn_cancel` takes an intent, the turn and the revision that was shown. The
answer is a receipt. The turn is shown as "Cancelling…" until a reading says what
became of it; nothing says "Interrupted" before the authority does. A cancel that was
refused, or whose answer was lost, is said as that, with the turn as it was last read.

### Recovery

- **The desktop restarts.** The window has nothing of its own. It reads the session's
  turns, finds the one that runs and follows it from what the authority still holds.
  Nothing is sent to start it again.
- **The authority restarts.** What was read stays on screen, marked as read before the
  connection was lost. Answers in flight are dropped. When the authority is back the
  session and the turn are read: the turn is `indeterminate`, its later output is
  marked as not kept, and the session shows that it has to be resumed.
- **The window closes during a turn.** Nothing more is sent. The turn goes on, under a
  lease that expires by itself.

### Bounds

| What                                   | Bound                              |
| -------------------------------------- | ---------------------------------- |
| Answer text kept per turn              | 200,000 characters; oldest dropped |
| Items kept per turn                    | 400                                |
| Turns shown per session                | 20                                 |
| Sessions open                          | 8                                  |
| Draft                                  | 32,768 characters                  |
| Events per reading                     | 50                                 |
| Durable events read by a first listing | 2,000, in 40 pages                 |
| Turns indexed per session              | 32, for 64 sessions                |

## Failures

| Shown as                         | When                                                                            |
| -------------------------------- | ------------------------------------------------------------------------------- |
| Outcome unknown, with **Retry**  | The authority did not answer a start. Retry repeats the same request.           |
| Another turn is running          | `active_turn_exists`, or the authority is at capacity.                          |
| The session changed              | Stale revision. It is read again.                                               |
| Not attached to a running Codex  | The gate named the runtime or the attachment. Resume it in Sessions.            |
| No workspace                     | The session has no binding.                                                     |
| Denied                           | The gate denied it and no lease would change that.                              |
| Codex refused                    | The provider rejected the turn.                                                 |
| Could not confirm what Codex did | `external_effect_unknown`. Nothing is sent again by itself.                     |
| Turn ended `lease_expired`       | The turn outlived its time budget.                                              |
| Turn ended `indeterminate`       | Dispatch unknown, an interrupt that was not confirmed, or an authority restart. |
| Could not be read                | A reading failed. What is shown is what was read.                               |

An unknown outcome is drawn as a caution, a refusal as an error, as in ADR 0021.

## Visual design

The view is built from the design system the shell already has: its tokens, its
`action` buttons, pills, status lines and empty states (ADRs 0020, 0021). No
token, typeface, color or motion was added. The transcript is the view's one solid
surface, like the terminal's, so an answer stays legible over any desktop.

No new aesthetic direction was taken, so the direction, engine and style layers of the
UI pipeline were not run again: the direction is the existing one and it is held. The
real-desktop acceptance below is the QA layer.

## Security boundary

- The window is granted four more of this application's own commands and nothing else:
  no filesystem, shell, process, socket, dialog or event permission of the framework.
- No command takes a control method or a payload to pass through.
- No command answers an approval, and no permission for one exists.
- What the user wrote is data in one field, from the textarea to the authority. It is
  not concatenated into anything.
- The window is given no identity it could use against the authority except the
  session's and the turn's.
- Every answer is validated whole before anything is shown; a reading about another
  turn or session, with an unknown field, or with events at or before the cursor, is
  a failed bridge.

## Limits and residual risk

- **Following is polling, in the desktop.** Five looks a second on one connection while
  a turn is followed. A blocking read of the turn stream would be a protocol change.
- **A turn has thirty minutes** when started from the window and is not renewed.
- **The desktop's lease is `turn_start` only.** If Codex proposes a tool action in a
  turn started under it, the authority refuses the action and the turn ends
  `action_rejected`. With sessions activated from the window this does not arise:
  they have no tools.
- **The input of a turn is shown only by the window that sent it, while it is open.**
  Nothing persists it. The same holds for output the authority no longer keeps.
- **The lease of a turn nobody is reading stays until it expires** or until the next
  start for that session.
- **History is what the last two thousand durable events name.** Older turns are not
  listed, and the list says so.
- **Markdown is not rendered.** An answer with code fences shows the fences.
- The non-Linux build of the bridge is a stub that answers `unavailable`. It was not
  compiled here.
- As with ADRs 0019 to 0021, no party other than the session that built this reviewed
  it before this record was written. Its tests were checked with eighty-nine mutation
  controls instead, which is not the same thing.
- **"Cancelling…" was not seen on the real desktop**: the fixture provider confirms an
  interrupt faster than a capture can be taken. It is covered by the window's tests.
- The real-desktop run used a synthetic session with GTK's accessibility bridge
  disabled, as in ADRs 0020 and 0021.

## Deferred

Answering approvals from the window (Phase 3E); starting a local-tools runtime from the
desktop; rendering Markdown; a persisted transcript; renewing a turn's lease; an event
view; other adapters.

## Acceptance evidence

Accepted on Linux (CachyOS, KWin 6.7.5). Every authority ran in an isolated temporary
XDG directory and every workspace was an empty temporary folder. No production state of
the authority was read or written.

**Service, against the real `wyss-control` process**
(`src-tauri/tests/turns_process.rs`, the Codex fixture as the runtime), five tests:

- A turn started from the desktop: the stream read as `started`, the answer, a
  reasoning mark, `message_end`, `completed`, positions one to five, the answer whole
  and once; neither the reasoning summary, nor the prompt, nor a provider, lease or
  runtime identity in any reading. At the authority: a lease audited as `["turn_start"]`
  with 1800 seconds between grant and expiry, then `admitted`, `running`, `completed`,
  `revoked`, in that order, and one `turn/start`. The same intent three more times
  writes nothing and sends nothing; the same intent with another input is refused.
  Replay from a cursor returns what follows it. A second turn, the list of both, and
  nothing of them under another session. After an authority restart: the turns as they
  were, their output gone and said to be, the session detached and a start refused;
  resumed, it runs a third turn.
- One turn at a time: a second start is refused for either session and leaves no lease.
  Cancel at a stale revision is refused; at the shown one the turn ends `interrupted`
  / `user_cancel` with one `turn/interrupt`; the same cancel again is its receipt and
  a new one is refused. The authority killed under a running turn: `indeterminate` /
  `authority_restarted`, no second `turn/start`, and the kept lease given back.
- A turn the authority fails (`inference_isolation_violation`) is shown as that.
- A runtime stopped under a turn: `indeterminate` / `runtime_stopped`, the session
  detached; started and resumed again, the session runs another turn.
- A turn waiting on an approval, in a local-tools runtime under a lease of the
  developer client: shown running with a notice of the kind and the deadline; the
  command, its directory and the approval's identity and fingerprint in no reading;
  the approval still `pending` and undecided after the desktop kept reading; nothing
  answered to the provider and nothing executed; cancelling the turn ends both, and
  the other client's lease is still theirs.

**Tests.** 486 Rust tests and 425 window tests pass. Eighty-nine mutation controls,
one per adversarial focus, are each killed by a test: 55 in the window (duplicate
submit, a new identity after a lost start, a refused start retried, stale revisions,
a wrong cursor, a repeated event, an unsaid gap, a turn of another session, an answer
from a connection that is gone, a cancel shown before the authority said so, unbounded
answer, items, turns, tabs and draft, polling without a floor, two readings at once,
text on any event, unknown fields, markup rendered, an approve button, a generic or an
approval permission) and 34 in Rust (reasoning or action detail crossing, a wider scope
or lifetime, the lease given back at admission or never, given back for another turn,
lent to the next one, a silent turn read as a gap, the tail lost at a turn's end,
following without pace, an unbounded scan, requests that carry more, the prompt in
diagnostics). Five of them found a missing test, and none a defect in the code under test. Two defects were found while the process tests were written, against the real authority, and are described above under "Reading a turn": the marker of a silent turn taken as a cursor, and the tail of a stream lost when a turn ended between two questions.

**Real Codex.** One bounded turn, through the window, with Codex 0.159.0 in the
inference profile and an empty temporary folder as the workspace. The input asked for
one word; the window showed the turn `Completed` with the answer `WYSSOK`. The
authority recorded one `thread/start`, one `turn/start`, no interrupt and no approval,
and three leases granted and revoked (`session_create`, `session_resume`,
`turn_start`). The folder was still empty afterwards. Codex kept one rollout of its
own for the conversation.

**Real desktop**, once, on the release build of `547ae37`, in a nested compositor with
real pointer and keyboard input and nothing sent to the user's session:

1. A session created and activated from the window (native folder dialog) is Ready.
2. **Open agent** shows the Agent view with the session named, Ready, and its path.
3. An objective typed and submitted with Enter starts a turn.
4. The answer appears under it and the turn ends `Completed`.
5. A second submission is a second turn.
6. Enter three times in a row is one `turn/start` at the provider.
7. A turn that stays running shows `Running` with **Cancel turn**; going to the Console
   and to Sessions and back neither cancelled it nor started another.
8. The terminal opened before the turn took input during it: the same PTY.
9. **Cancel turn** ends it `Interrupted`, "Cancelled at your request"; the authority
   recorded `cancel_requested`, `interrupted` and the lease revoked.
10. The authority killed under a running turn: the window says it is offline and keeps
    what it read; after the restart the turn is `Indeterminate` with its reason, the
    later output is marked as not kept, the session is Detached and has to be resumed,
    and the provider was sent no second `turn/start`.
11. A turn that waits on an approval shows "Waiting for approval", the kind of action
    and the deadline, with no control to answer it and no command text. The approval
    was still pending at the authority. Cancelling ended the turn; the lease of the
    developer client stayed active.
12. Three sessions open as three tabs, each with its own turns and its own draft.
13. A turn the window did not start shows no input and says why.
14. At 960×640 nothing overflows; at 600, 380 and 320 pixels the path steps back, the
    sidebar becomes a rail and the composer's row wraps.
15. `_KDE_NET_WM_BLUR_BEHIND_REGION` was set on the composited window; nothing in
    window creation or `desktop/glass.rs` changed.
16. Across eight turns the process stayed at 49 descriptors, 9 sockets and 45 to 47
    threads.

Two visual defects were found in that run before the accepted build and fixed in it:
a workspace path lost its leading slash, and the focus ring was drawn on the field
instead of the composer.

## Later implementation status

Since ADR 0023 the window starts a local-tools runtime when the user chooses Local
Tools for a session, and approvals are read and answered in it. "Approvals are not
answered here", "a local-tools runtime, which the window cannot start", "The desktop's
lease is `turn_start` only" and the notice that "this view cannot answer it" describe
this ADR's phase. A turn of a session attached in Local Tools is now admitted under a
lease that also carries `command_execute` and `file_change`; a turn of an inference
session is admitted as described here. A turn ended `user_cancel` is worded "Stopped at
your request", because the authority records a denied command that way too.
