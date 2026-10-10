# ADR 0024: Events and history in the window

Status: Proposed
Date: 2026-10-09

## Decision

The window has an **Events** view: what `wyss-control` recorded, newest first. It answers
what happened, when, to which session, turn or approval, why something stopped, and
whether the runtime went away, without SQLite, a log file, the developer client or
anything of the provider.

```text
Events  ->  the newest fifty events of the stream the authority holds
        ->  Load older, a window at a time, down to the first event
        ->  a row  ->  what the record says, and where the thing it is about lives now
```

```text
Svelte            a view of rows; filters over what is loaded; nothing that changes anything
  -> Tauri        two read commands: the newest window, and the window before a position
  -> EventService
  -> ControlClient   hello (stream, head) + audit/read
  -> wyss-control
```

Control Protocol v1 is unchanged: no method, field, schema or storage version was added,
and nothing in `src-tauri/src/control` was touched. The phase adds **no execution
authority**: both commands are reads, the capability grows by those two reads, and no
control in the view performs, repeats or undoes what an event describes.

## The product history model

History is **evidence of what happened**. What is true now is a **canonical read**.

A row saying an approval was requested does not say it is pending; a row saying a turn
was admitted does not say it runs; a row saying a session was prepared does not say it
is open. The Events view never states a present fact from a record, and no other view
takes its state from one:

- Sessions, the runtime, turns, approvals and leases are still read where ADRs 0019 to
  0023 read them, from the authority's canonical answers. Nothing in those views
  imports the Events state.
- A row's details say it: _"This is a record of what happened then. What it is about
  may have changed since."_
- Going from a record to its subject is navigation followed by that view's own read
  (see Correlation).

## The event contract, as it was found

Read from the code and from a running authority before anything was built
(ADRs 0017, 0018, 0019).

| Fact                  | What it is                                                                                                                                                                                    |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Envelope              | `EventEnvelope` v1: identity, `event_type`, timestamp, `stream_id`, `sequence`, an optional `session_id`, a payload, and bookkeeping (correlation, causation, sensitivity, redaction).        |
| Stream                | **One durable stream per state store.** Its identity and its sequences are stored with it.                                                                                                    |
| Sequences             | Contiguous from 1. Decimal strings on the wire; a `u64` in the store.                                                                                                                         |
| Reading               | `audit/read {stream_id, after_sequence, limit}`: forward only, at most fifty events and at most one frame; a cursor past the head or of another stream is refused.                            |
| Retention             | **None.** Nothing is compacted or deleted: a stream that exists can be read from its first event.                                                                                             |
| An authority restart  | **The same stream continues.** A new process on the same store greets with the same `stream_id` and goes on from the next sequence.                                                           |
| A restart as an event | **There is none.** No event says the authority started or stopped. What a restart did is recorded: the runtime's exit, a turn made `indeterminate`, an approval `invalidated`.                |
| A refusal as an event | **There is none.** A refused `runtime/stop`, a denied gate check, a stale revision: each is an answer to its caller and writes nothing.                                                       |
| What a payload holds  | Facts and identities, never content: no prompt, command, diff, output or reasoning. It does hold the provider thread and turn, lease and runtime-instance identities and action fingerprints. |
| Kinds                 | Twenty-eight, in five families: managed session (6), runtime (3), managed turn (7), managed approval (8), execution lease (4).                                                                |

Three consequences shaped the view:

1. A restart is **not** a stream boundary and is not shown as one.
2. Refusals cannot appear in Events. The view shows what was recorded, and says nothing
   about what was refused.
3. A gap in history cannot happen with this authority. It is handled, and proved with a
   scripted one, because the desktop does not get to assume how long that stays true.

## EventService

`src-tauri/src/events` owns history reads. It holds the `SessionService` for its
connection policy and nothing else: no cache, no thread, no watcher, no state.

- **`latest()`** greets the authority, which names the stream and its head, and reads
  the window `(head - 50, head]`.
- **`older(stream, before)`** greets, refuses with `changed` unless `stream` is the
  stream the authority holds now and `before` is a position of it, and reads the window
  `[before - 50, before)`.

`audit/read` only goes forward and may answer fewer events than asked for, so a window
is read in as many requests as it takes: never more than fifty, each for at most fifty
events. What comes back has to be the stream. Every event is of the named stream and
one position after the one before; an empty or oversized answer, an event that does not
go forward, a hole inside a window and a window that ends short are all a failed read,
not a shorter history. The one hole that is said and not refused is at the beginning of
a window: history before its first event is unavailable, the page says `gap`, and it
hands back no position to read before.

The desktop commands (`src-tauri/src/desktop/events.rs`) are `managed_events {}` and
`managed_events_older {stream_id, before}`. Both reject unknown fields. There is no
`audit_read`, no `control_request`, no method or parameter passed through, no limit or
filter of the authority's, and no command that returns an envelope. They have an
admission pool of their own, two at once: a history read never takes the place of a
session call, a turn read, a terminal or the authority view.

## The projection

The envelope stops in Rust. `project()` builds a row from a closed vocabulary, field by
field, and the window's parser accepts exactly that shape and refuses any other key.
`serde_json::Value` does not cross, and the window is never the one deciding what is
safe.

| Field         | From                                                                 |
| ------------- | -------------------------------------------------------------------- |
| `sequence`    | the envelope's position, as decimal text                             |
| `at`          | the envelope's timestamp                                             |
| `category`    | `session`, `runtime`, `turn`, `approval`, `lease`, `authority`       |
| `kind`        | one of the twenty-eight known kinds, or `other`                      |
| `disposition` | `normal`, `attention`, `failure`, `security`                         |
| `security`    | it is a lease event, or isolation or safety is what it records       |
| `session`     | the envelope's managed session identity                              |
| `turn`        | the managed turn identity, for turn and approval events              |
| `approval`    | the managed approval identity, for approval events                   |
| `facts`       | eleven optional facts, each an enumeration, a boolean or a timestamp |

The facts: why a turn ended; what kind of action an approval was about, why it ended
and what effect was recorded; what the runtime was asked to be, how it exited and
whether that was expected; the runtime's version (only as `[0-9A-Za-z.+-]{1,32}`); a
lease's scopes, deadline and why it was revoked.

**Never projected**: the payload itself; the provider's thread and turn identities; the
runtime instance; lease identities; action fingerprints; operation identities; event,
correlation and causation identities; source, sensitivity and redaction; and any field
this build does not name. Events carry no prompt, command, diff, output, reasoning,
credential or environment to begin with, and none is recreated from another source: a
row about an approval says a command was proposed, never which.

## Categories and what a row says

A row's title comes from its kind and, where the kind has one, the authority's recorded
reason. Nothing is inferred about what led to what.

| Category  | Rows                                                                                                                                                                                                         |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Session   | Managed session prepared · closed · Session bound to a workspace · attached to Codex · Workspace binding outcome unknown · Uncertain binding attempt discarded                                               |
| Runtime   | Codex runtime start accepted · stop accepted · ready · stopped · ended unexpectedly · contained                                                                                                              |
| Turn      | Managed turn admitted · running · Turn cancellation requested · Managed turn completed · interrupted · failed · outcome unknown                                                                              |
| Approval  | Command approval requested · File change approval requested · Approval accepted · Approved action confirmed by Codex · Approval denied · cancelled · expired undecided · no longer applies · outcome unknown |
| Lease     | Execution lease granted · renewed · revoked · expired                                                                                                                                                        |
| Authority | Authority event (a kind this build does not know)                                                                                                                                                            |

`disposition` says how the thing ended, from the record alone: `failure` for a failed
turn and an unexpected runtime exit; `attention` for an outcome the authority could not
determine, an interruption nobody asked for, and an approval that ended undecided;
`security` only where the recorded reason is an isolation violation or unproven safety.
There is no severity scale and no alarm level: the labels are _Failed_, _Needs a look_
and _Isolation_.

Security and authority decisions are shown where a durable event exists for them: every
lease grant, renewal, revocation and expiry, a runtime the authority contained, a turn
or an approval it ended for isolation. A decision that left no event is not shown.

## Sequences

A position is opaque decimal text from the store to the screen. The window never calls
`Number` or `parseInt` on one and never makes one up: it hands back the position a page
gave it. The only arithmetic is exact, on `BigInt`, and only to verify: that a page is
one unbroken descending run, that nothing is newer than the head it was read under,
that an older window ends exactly one before the position asked for, and that the head
moved. Two positions past 2^53 that a float cannot tell apart are told apart.

## Pagination and bounds

| Bound                                     | Value                                  |
| ----------------------------------------- | -------------------------------------- |
| Events in one window                      | 50                                     |
| Requests to the authority for a window    | at most 50, each for at most 50 events |
| Events kept of the stream shown           | 400                                    |
| Events kept of a stream that was replaced | 100                                    |
| History reads admitted at once            | 2                                      |
| Reads in flight in the view               | one newest read and one older read     |
| Timers, watchers, threads                 | none                                   |

Opening the view reads the newest window. **Load older** reads the window before the
oldest event shown. Past 400 events the far end is let go: loading older drops the
newest, and the view then says new events exist instead of following them.

The end of the list says which of three things it is:

- _"Older events exist and are not loaded."_ with **Load older**;
- _"This is the beginning of the event stream."_ when the first event is shown;
- _"History before this point is unavailable."_ when a window began at a hole.

Loaded history is never described as all of history: the count reads _"N loaded"_.

## Live refresh

The view is told that history moved by the shell's one authority follower, the same
observation the top bar and the other views use (ADR 0019): its `last_sequence` is the
durable head. **No second `audit/watch` exists**, and the view has no timer.

- It reads only while it is shown. Hidden, the head may move any number of times and
  nothing is read; shown again, it reads once.
- One read at a time. A head that moves during a read is looked at when the read lands.
  A read that failed is not repeated by itself until the head or the connection changes.
- At the newest end, new events are read and added above the ones shown.
- Anywhere else, nothing moves under the reader: the view says **New events available**
  and reads when asked.

## Stream identity and authority restart

Every page names its stream, every older read names the stream its position came from,
and a selection is a stream and a position. Two streams are never merged, compared or
deduplicated against each other.

- **The authority restarts on its store.** The stream is the same and its positions go
  on. The follower's `restarts` counter proves another process answered, so the view
  reads once and says: _"wyss-control restarted. It kept the same event stream: the
  events below continue where they were."_ No boundary is drawn, because there is none.
  The restart is not a row: the authority records none.
- **Another stream answers on the same socket** (another state store). The view shows
  the new stream's newest window, then an explicit boundary, **New authority event
  stream**, then up to 100 of the rows it was showing, labelled as read from another
  stream whose positions are unrelated. Nothing is asked of the old stream again, and
  the word "restarted" is not used: the protocol does not prove that.
- A position of a stream the authority no longer holds is answered `changed` by the
  desktop, and the view reads where history is now.

## Gaps, retention and duplicates

No retention exists, so no history is missing today. The handling is in place and was
proved against a scripted authority: a window that begins at a hole is marked, the view
says _"History before this point is unavailable."_ and offers nothing older; a hole
inside a window is a failed read. A gap is never skipped in silence and never presented
as continuous.

An event is identified by its stream and its position. A page that overlaps what is
shown adds each event once; rows are keyed by position within their stream's list.
Order is the stream's own, newest first; the clock is shown and never used to sort.

## Filters

All, Sessions, Runtime, Turns, Approvals, Security. A filter is applied in the window
to the events that are loaded, and the view says so: _"Filters apply to the events
loaded here, not to all of history."_ Choosing one reads nothing. Security is the
`security` flag: leases, and whatever the record says was about isolation or safety.

A scope narrows the list to one session, or one turn of it, from a row's details or
from a session's **Events** action in Sessions. It is a filter over the same loaded
rows and can be cleared. Each stream's list is filtered by itself.

## Details and correlation

A row opens a details surface: its title, a sentence from the record, the time, the
kind, the session, turn and approval identities, a lease's deadline, its position and
stream, and the note that it is a record. It shows no JSON and offers no action on the
event.

Its links navigate and let the destination read:

- **Open session** selects the session in Sessions, which shows what the authority says
  of it now.
- **Open in Agents** opens the session there, which reads its turns.
- **Show events of this session / turn** scopes the list.
- **Check what it is now**, for an approval, asks the authority for that approval. The
  answer is shown as _"Now: …"_. An approval the authority no longer holds answers
  _"Approval detail is no longer available. The record above is still what happened."_
  Nothing about the approval is filled in from the event. **Open Approvals** goes to the
  view that decides.

## Offline

When the authority goes away the loaded rows stay and are said to be from before:
_"Authority offline. Showing previously loaded history: it was read before the
connection was lost and nothing newer is known."_ Refresh and Load older are disabled,
and answers in flight are dropped. On reconnection the view reads once, under whatever
stream the authority holds then.

## Unknown events

An event type this build does not know is a row titled **Authority event** with its
time and position, in the Authority category. Its payload is not looked at: no fact, no
turn, no approval and no mark is derived from it. An authority newer than the desktop
does not break the view and does not leak through it.

## Security boundary

- Two reads. The capability has 26 permissions, 24 before and these two; the window
  cannot name a control method, a limit, a filter or a payload.
- No mutation: the view has no Approve, Deny, Retry, Cancel, Start, Stop, Resume,
  Close, Revoke, Run again, Replay or Undo, and no command behind it could do any.
- No file system: no export, no save, no folder.
- The projection is closed on both sides of the bridge: Rust builds it from named
  fields, and the window refuses a row with any other key.

## Limits and residual risk

- **Refusals are not history.** A rejected runtime stop, a denied gate check and a stale
  revision write no event, so Events cannot show them. This is the authority's contract,
  not a choice of the view.
- **A restart of the authority is not a row.** It is said once as a notice while the
  window was open across it; afterwards it is visible only through what it did to the
  runtime, a turn or an approval.
- **A record names no folder.** The binding event carries none, so a row says a session
  was bound and not where; Sessions says where.
- **Filters and scopes cover loaded rows only.** Finding every event of a session means
  loading history until it is reached.
- **The start of history is the store's.** A new state store is a new stream; the rows
  of a replaced one stay only while the window is open, 100 at most.
- **Gap handling is proved with a scripted authority**, because the real one keeps
  everything.
- **A window costs a connection.** Each read greets the authority again, so that the
  stream and head it reads under are the current ones.
- The accepted low residual of ADR 0023 (a replayed `lease/close` receipt may withdraw a
  binding admission that happens to be in flight) is unchanged: this phase does not
  expose it and did not touch it.
- The non-Linux build of the bridge is a stub that answers `unavailable`. It was not
  compiled here.
- As with ADRs 0019 to 0023, no party other than the session that built this reviewed
  it before this record was written. Its tests were checked with mutation controls
  instead, which is not the same thing.

## Deferred

Searching or filtering history at the authority; exporting it; a durable record of
refusals or of the authority's own lifecycle (both would be core changes); retention;
Settings.
