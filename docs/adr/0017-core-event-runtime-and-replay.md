# ADR 0017: Core event runtime and replay

Status: Accepted
Date: 2026-10-08

## Decision

The authority's durable `EventEnvelope` stream becomes a complete event runtime: one
transactional append path, bounded replay from a client cursor through the existing
`audit/read`, a live wait for the next durable commit through `audit/watch`, and
consistency checks that hold every canonical row to the event that recorded it.

The stream is audit, history, replay, catch-up and future UI observation. It is not
the authority. Canonical control state stays in the SQLite control tables and the
authority's state machines (ADRs 0011 to 0016); nothing is derived from an event, and
reading the stream cannot start, grant, answer or repeat anything.

EventEnvelope v1 (ADR 0003) is unchanged. There is no envelope v2 and no schema bump:
storage stays at version 6.

## EventEnvelope v1 stays frozen

`schema_version` 1, `event_id` UUID, namespaced `event_type`, positive
`payload_schema_version`, RFC 3339 UTC `timestamp`, `stream_id` UUID, `sequence` as a
positive decimal string, nullable `session_id`, `source.component` and
`source.instance_id`, nullable `correlation_id` and `causation_id`, object `payload`,
`persistence`, `sensitivity` and `redaction`. Ordering exists only within the stream.
`sequence` is a string on the wire and in storage and is never a JSON number.

One event type was added to the vocabulary, which the envelope allows:
`authority.managed_session.runtime_operation_indeterminate` (see Coverage).

## Canonical state is not event sourced

| Canonical authority                    | Durable stream                                |
| -------------------------------------- | --------------------------------------------- |
| control tables and state machines      | audit, history, replay, catch-up, observation |
| decides every gate and transition      | describes transitions that already committed  |
| written first, in the same transaction | appended last, in that transaction            |

No code path reads an event to decide anything. The only consumer of events inside the
authority is the open-time consistency check, which compares and refuses; it never
writes and never repairs.

## One append path

Six producers each built, validated, numbered and inserted their own envelope. Two of
them (managed turns and approvals) did not validate the envelope itself before
inserting. All of them now call one primitive, `control/events.rs::append`, inside the
transaction of the canonical mutation they describe:

```text
canonical mutation transaction
  -> append(Draft { kind, session, correlation, timestamp, payload })
       refuse a type outside this build's registry
       allocate head + 1 with checked arithmetic
       enforce the known-event payload contract
       validate EventEnvelope v1
       enforce the encoded size bound
       INSERT control_events
       UPDATE authority_metadata.last_sequence
  -> commit
```

There is one sequence allocator, one envelope validation boundary, one encoded-size
check and one insert. The head moves only in the transaction that inserts the row it
numbers, so a rollback leaves neither an event nor a gap: the next commit takes the
same number. The state machines themselves were not merged or redesigned.

A required event that cannot be built, validated or appended fails its canonical
mutation. That is unchanged and is now uniform.

## Event size bound

A durable envelope is at most 2048 encoded bytes (`MAX_EVENT_BYTES`). The number is
the schema's own `length(envelope) <= 2048` check read as bytes, and it is twice the
largest event the schema v6 corpus contains: 1024 bytes for an accepted approval,
measured over the 437 events of the Sprint 2I real acceptance databases. An event over
the bound fails its mutation with `capacity_exceeded` before commit. Durable security
metadata is never truncated.

No event this build can write reaches the bound: every payload field is an identifier,
an enumeration, a revision, a timestamp, a fingerprint or a string the contract already
limits (the runtime version, 128 bytes). The check is enforced anyway.

## Sequence, cursor and page semantics

`audit/read` is the replay primitive. No alias was added.

- `after_sequence` returns events with `sequence > after_sequence`, ascending. `null`
  means from the beginning.
- A `stream_id` that is not the authority's stream is refused.
- A cursor greater than the durable head is refused: it names history that does not
  exist. A cursor equal to the head returns an empty page.
- A page holds at most 50 events (`limit` 1 to 50) and at most `MAX_FRAME - 1024`
  encoded bytes of events. It can therefore stop before `limit`. It always holds at
  least one event when one exists, and at least thirty of maximal size.
- `next` is the sequence of the last event returned, and it is present only while a
  later committed event exists. A client that follows `next` until it is absent has
  read everything committed up to that moment, with no duplicate and no skipped event.
- Timestamps are never cursors.

Reading is deterministic: over an unchanged log the same cursor and limit return the
same bytes, and pages of any size concatenate to the same bytes. A page is checked to
be contiguous from the cursor as it is read, so damage that appears while the
authority runs is refused by the read that would have crossed it.

No server response exceeds the 64 KiB frame.

## audit/watch

```text
audit/watch { stream_id, after_sequence, limit, wait_ms }
  limit   1..50
  wait_ms 0..30000
```

The result has the shape of `audit/read`.

1. The authority reads durable events after the cursor.
2. If any exist it answers immediately.
3. If none exist and `wait_ms` is 0 it answers empty immediately.
4. Otherwise it waits for a durable commit to move the head.
5. When woken it reads SQLite again by the client's cursor and answers.
6. If `wait_ms` elapses first it answers empty.

It is the same stream, the same cursor rules and the same page bounds. It is not a
general publish/subscribe mechanism and it does not merge with the turn stream.

### Live notification

The notification is a wakeup hint and carries no event. The core thread, which is the
only SQLite writer, announces the committed head at the top of its loop, the single
point every path returns to after its transactions have committed or rolled back. It
reads the head outside any transaction, so what it announces is committed by
construction, and a rolled back mutation announces nothing.

A woken watcher never trusts the hint: it asks the core for the events by cursor. A
lost or spurious wakeup can delay an answer; it cannot lose, invent or reorder history.
If the process dies between commit and announcement, the event is in SQLite and the
next read or reconnect returns it.

### No lost wakeup

A watcher registers under a mutex and only then compares the announced head with its
cursor. `publish` moves the head and writes one wake byte to every registered watcher
under that same mutex.

- Read head, event commits, register waiter: the comparison after registration sees
  the new head and the watcher does not sleep.
- Register waiter, event commits, sleep: the commit left a byte in the watcher's wake
  channel, so the sleep returns at once. The byte is drained only before the next
  comparison, which then sees the head that wrote it.

The sleep is a `poll` on the wake channel and the client socket with the remaining
wait as its timeout. There is no periodic polling of SQLite, of the head or of time.

### Watcher capacity

The authority serves at most eight connections. At most four of them may be waiting in
`audit/watch` at once (`MAX_WATCHERS`, half of `MAX_CLIENTS`), so four connections are
always left for approval responses, cancels, revocations, closes and runtime
operations however long watchers wait. A watch that would have to wait when every slot
is taken fails immediately with `capacity_exceeded`; it is not queued. A read, or a
watch that can answer without waiting, needs no slot.

### Writer isolation and backpressure

A waiting watcher holds no transaction, no core queue slot and no store lock. Both of
its reads are ordinary jobs in the core that run to completion before anything is
written to a socket; socket writes happen on the connection's own thread with the
existing three second write timeout. A client that never reads what it is sent stalls
only its own connection thread and is dropped by that timeout.

`publish` never blocks: a wake channel that is already full is already readable, so
the failed write is ignored. Nothing is queued per watcher except that one pending
byte, and nothing grows with the number of commits.

### Shutdown and disconnect

SIGINT or SIGTERM closes the signal, which wakes every watcher at once; no watcher
delays shutdown for the rest of its wait, and no answer is written for it. A hard kill
needs no cleanup because there is no watcher state to clean.

A watcher whose client hangs up is noticed by the same `poll` and releases its slot
immediately. Disconnecting creates no event, changes no canonical state, consumes
nothing and moves no cursor, because the authority has no cursor to move.

## Restart and resume

There is no durable subscriber state, no acknowledgement table and no consumer offset.
The cursor belongs to the client. After a reconnect or an authority restart the client
sends the last sequence it processed and receives everything committed after it.

A graceful or a hard restart preserves the stream id, every committed event and the
head; the next committed event is head + 1. The process identity in
`source.instance_id` changes, as before.

## Replay has no side effects

Replay delivers envelopes that already committed. It performs no canonical mutation,
writes no row, sends no provider frame, starts no runtime, resumes no thread, sends no
Accept, runs no command, changes no file and renews or revokes no lease. A replay
consumer has no way to cause an authority transition: the only operations that mutate
are the ones that always did, each behind its own gate.

## Known, unknown and malformed events

| Class                 | Meaning                                                                                   | Treatment                                                                                |
| --------------------- | ----------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| known authority event | one of the 28 types this build writes                                                     | payload contract enforced on append and on open; folded into the consistency check       |
| unknown future event  | a valid EventEnvelope v1 of any other type                                                | replayed exactly as stored; counted; never interpreted, never folded, never a transition |
| malformed envelope    | invalid JSON, duplicate member, invalid v1 envelope, non-durable event in the durable log | corruption: the store is refused                                                         |

An unknown event is never coerced into a known one, whatever its payload says. The
authority itself cannot append a type outside its registry. Unknown events are held to
the stream invariants like any other row: contiguous sequence, the authority's stream
id, columns equal to the envelope.

## Event inventory

Every row is written by `append` in the same `IMMEDIATE` or deferred transaction as its
canonical mutation. Every event is `durable`, `internal`, `redaction: none`, from
`wyss_control`, with a null `causation_id`.

| Event type                                                                        | Producer                                     | Canonical mutation                                              | session_id | correlation_id | Payload                                                                                                                      |
| --------------------------------------------------------------------------------- | -------------------------------------------- | --------------------------------------------------------------- | ---------- | -------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `managed_session.prepared`                                                        | `Store::mutate`                              | insert `managed_sessions`, receipt                              | session    | intent         | adapter, runtime kind, control state, revision                                                                               |
| `managed_session.closed`                                                          | `Store::mutate`                              | close session, revoke its lease, receipt                        | session    | intent         | same                                                                                                                         |
| `managed_session.runtime_bound`                                                   | `commit_binding`                             | insert `managed_session_bindings`, operation succeeded, receipt | session    | operation      | adapter, runtime kind, provider thread id, binding revision                                                                  |
| `managed_session.runtime_attached`                                                | `commit_binding`                             | resume operation succeeded, receipt                             | session    | operation      | provider thread id, runtime instance                                                                                         |
| `managed_session.runtime_operation_indeterminate`                                 | `fail_binding`, `recover_binding_operations` | create operation becomes `indeterminate`                        | session    | operation      | operation id                                                                                                                 |
| `managed_session.runtime_operation_abandoned`                                     | `abandon_binding`                            | indeterminate operation becomes `failed`, receipt               | session    | intent         | operation id                                                                                                                 |
| `runtime.desired_state_changed`                                                   | `Store::mutate`                              | upsert `managed_runtimes`, receipt                              | null       | intent         | adapter, runtime kind, desired state, revision                                                                               |
| `runtime.instance_ready`                                                          | `Store::runtime_event`                       | none: observed lifecycle of the owned process                   | null       | null           | adapter, runtime kind, runtime instance, version, compatibility                                                              |
| `runtime.instance_exited`                                                         | `Store::runtime_event`                       | none: observed lifecycle of the owned process                   | null       | null           | adapter, runtime kind, runtime instance, exit kind, expected, desired state                                                  |
| `execution_lease.granted`                                                         | `leases::mutate`                             | insert `execution_leases`, receipt                              | session    | intent         | lease id, scopes, state, revision, expiry                                                                                    |
| `execution_lease.renewed`                                                         | `leases::mutate`                             | new expiry and revision, receipt                                | session    | intent         | same                                                                                                                         |
| `execution_lease.revoked`                                                         | `leases::mutate`, `leases::close`            | state revoked, receipt                                          | session    | intent         | same and revocation reason                                                                                                   |
| `execution_lease.expired`                                                         | `expire_due`, grant, close                   | state expired                                                   | session    | null           | same                                                                                                                         |
| `managed_turn.admitted`                                                           | `reserve_turn`                               | insert `managed_turns`, receipt                                 | session    | intent         | turn, provider turn id, lease id and revision, runtime instance, state, revision, reason                                     |
| `managed_turn.running`, `.completed`, `.interrupted`, `.failed`, `.indeterminate` | `transition_turn`                            | state and revision of the turn                                  | session    | null           | same                                                                                                                         |
| `managed_turn.cancel_requested`                                                   | `cancel_turn_receipt`                        | receipt of the cancel intent                                    | session    | intent         | same, with the cancel reason                                                                                                 |
| `managed_approval.requested`                                                      | `open_approval`                              | insert `managed_approvals`                                      | session    | null           | approval, turn, kind, lease id and revision, runtime instance, state, revision, decision, effect, reason, action fingerprint |
| `managed_approval.accepted`, `.declined`, `.cancelled`                            | `decide_approval`                            | decision, dispatch marker, receipt                              | session    | intent         | same                                                                                                                         |
| `managed_approval.confirmed`                                                      | `confirm_approval`                           | effect confirmed                                                | session    | null           | same                                                                                                                         |
| `managed_approval.expired`, `.invalidated`, `.indeterminate`                      | `settle_approvals`                           | terminal state without a client decision                        | session    | null           | same                                                                                                                         |

All types carry the `authority.` prefix. The payloads hold identifiers, enumerations,
revisions, timestamps, counts and SHA-256 fingerprints, as accepted by ADRs 0011 to 0016. The provider thread id in the binding events is the opaque identifier ADR 0014
already made durable.

### Coverage

Every canonical durable mutation through Sprint 2I has an event in its own
transaction, with these intentional exceptions:

| Mutation                                                                        | Why it has no event                                                                                                                        |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `managed_session_operations` reserved, dispatching, materialization checkpoints | The write-ahead journal of one provider attempt. It grants nothing and is only meaningful until the attempt ends; its outcomes are events. |
| operation becomes `failed` without uncertainty                                  | A definite failure, or a resume, leaves the authority exactly as before the attempt. The client received the error.                        |
| `processed_intents`                                                             | The idempotency receipt of a mutation that has its own event in the same transaction.                                                      |
| the expiry check that finds nothing due, every read                             | Not a mutation.                                                                                                                            |

One gap was closed. A binding create whose provider outcome is unknown blocks its
session until it is explicitly abandoned, and that transition had no event; the later
`runtime_operation_abandoned` referred to an operation the log had never mentioned. It
is now `runtime_operation_indeterminate`, committed with the transition, at the moment
of failure and at startup recovery. If its audit cannot be written the operation stays
open, which blocks the session in the same way.

The turn validator listed a type, `managed_turn.isolation_violation`, that no build
ever wrote; it is no longer accepted as a known turn event.

### Consistency on open

Opening the store replays the log from sequence 1 into a read-only in-memory view and
compares it with the tables. The store is refused, never repaired, when:

- an envelope is invalid, has a duplicate member, is not durable, or disagrees with its
  `event_id`, `stream_id`, `sequence` or `session_id` column;
- the sequence has a gap, a repeat or a different stream, or the head is not the number
  of events;
- a known event violates its payload contract;
- an entity does not advance by exactly one revision per event from revision 1;
- a session, lease, turn or approval row is not the latest state and revision the log
  recorded for it, or a row has no event, or an event has no row;
- a binding row is not the one its `runtime_bound` event recorded;
- the desired runtime state is not the last one the log recorded;
- an operation the log declared uncertain is neither still uncertain nor abandoned.

The view holds heads only: state and revision per entity. It grants nothing and is
discarded after the comparison. There is no second projection database.

## Durable and transient

| Durable EventEnvelope                                                | Transient                                                                                                 |
| -------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| session, runtime, lease, binding, turn and approval control metadata | assistant text, reasoning summaries, command output, file diffs, prompts, provider frames, terminal bytes |

`managedSession/turn/events` (ADR 0015, extended by ADR 0016) stays memory-only, with
its own bounds, and is not merged with `audit/read` or `audit/watch`. After an
authority restart the durable history is intact and the transient stream of earlier
turns is gone.

## Terminal boundary

Human PTY traffic never reaches the authority. No terminal operation exists in the
control protocol, no event is written per terminal read or write, and no terminal
lifecycle event exists because no canonical durable terminal state exists. ADRs 0005
to 0007 are unchanged.

## Privacy

Replay returns exactly what was already durable under ADRs 0011 to 0016 and nothing
more. Prompts, assistant output, reasoning, commands, command output, file paths,
contents and diffs, terminal bytes and provider credentials do not occur in
`control_events`, in the database, its write-ahead log or its shared memory, in the
intent receipts, or in any replay response.

## Failure semantics

| Failure                                                            | Effect                                                                     |
| ------------------------------------------------------------------ | -------------------------------------------------------------------------- |
| a required event cannot be built, validated, sized or inserted     | the canonical mutation rolls back; no event, no gap                        |
| the announcement after a commit is lost or the head cannot be read | watchers answer later or empty; the event is in SQLite                     |
| every watcher slot is taken                                        | the new watch fails with `capacity_exceeded`; nothing else is affected     |
| a watcher's client stalls or disappears                            | its connection ends; no state changes                                      |
| the log is damaged                                                 | the store is refused on open, or the read that meets the damage is refused |

Durable append is a transactional requirement. Live notification is a best-effort
optimization.

## Limits and residual risk

- The log is integrity-checked, not authenticated. A same-user process that can rewrite
  the database consistently can forge history; ADR 0011's same-UID boundary applies.
- An unknown future event is tolerated so that the vocabulary can grow. Retyping a
  known event as an unknown one is caught only where a canonical row depends on it;
  `runtime.instance_ready`, `runtime.instance_exited`, `runtime_attached` and
  `managed_turn.cancel_requested` have no row and could be hidden that way.
- An uncertain operation recorded before this ADR has no `runtime_operation_indeterminate`
  event. Abandoning it is still accepted.
- Watchers are woken at the top of the core loop, after the client that committed has
  been answered. The delay is one loop iteration.
- Replay re-encodes a stored envelope through the typed contract. The JSON value is
  preserved; byte formatting of a foreign envelope (whitespace, escapes) is not.
- A watch reports progress only for the single authority stream. There is no filter by
  session or type; clients filter.
- `managed_turn.cancel_requested` carries the turn's last update as its timestamp, as it
  always did. Order within the stream is the sequence, never the timestamp.

## Acceptance evidence

Measured on 2026-10-08 with the release `wyss-control` built from the code of this
decision, the developer control client and raw control-protocol sockets, in isolated
authority state with a temporary workspace outside this repository. The provider cases
used Codex 0.159.0 and the real authenticated provider.

| Case                  | Measured                                                                                                                                                                                                                                                                                                                                                   |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A history replay      | 12 events replayed from sequence 1 in pages of 1, 2 and 50: identical bytes, exact order, continuation cursor `2` after the first page of 2                                                                                                                                                                                                                |
| B catch-up watch      | a watch parked at the head answered 13 ms after the commit with that event; caught up with no wait: empty in 1 ms; with a 400 ms wait: empty after 402 ms                                                                                                                                                                                                  |
| C race window         | 80 rounds, 40 with the watch first and 40 with the commit first: 0 lost events; slowest watch 21 ms                                                                                                                                                                                                                                                        |
| D multiple watchers   | 4 watchers at one cursor each returned the one committed event; the same cursor still returns it; a fifth waiting watch: `capacity_exceeded` in 1 ms                                                                                                                                                                                                       |
| E slow watcher        | 3 woken watchers that never read and one client with 168 unread maximal-page requests queued; 25 lease and close mutations, slowest 3 ms                                                                                                                                                                                                                   |
| F disconnect          | 4 parked watchers dropped: 0 events, 0 state changes, identical tables; 4 new watchers admitted at once                                                                                                                                                                                                                                                    |
| G graceful restart    | shutdown in 15 ms with 4 parked watchers; same stream and head; the exact missed suffix from the client's cursor                                                                                                                                                                                                                                           |
| H hard kill           | same stream, head and history; next committed sequence = head + 1                                                                                                                                                                                                                                                                                          |
| I rollback            | a fault on the audit insert of the live database: prepare and close `storage_unavailable`; canonical mutation 0, new event 0, sequence gap 0; four parked watchers slept their whole wait; the retried intent took head + 1                                                                                                                                |
| J replay side effects | a 176 event history with an accepted command, an accepted file change, a cancelled turn and a revoked lease, replayed in pages of 1, 7 and 50 and through watch: new provider frames 0, new commands 0, new file writes 0, new lease transitions 0, new approvals 0; the accepted file, deleted first, was not recreated; again after an authority restart |
| K privacy             | prompt, assistant output, command, command output, file path, file content and diff, every streamed text segment and the provider credential values: 0 occurrences in the 7 authority-owned files scanned live (database, WAL and SHM included), 0 after shutdown, 0 in the replay responses, 0 in `control_events` and `processed_intents`                |

The inference-only regression ran in the same authority: one traced model request, no
model-visible tool, no command, no file and no answer. No reasoning summary was streamed
by the provider in these turns; reasoning sentinels are covered by the process tests,
which also repeat J and K against the test provider with a live WAL.

Deterministic coverage: 283 unit tests, 19 control process tests and 46 runtime process
tests. A mutation round removed 46 defenses one at a time; 44 were detected. The two
that survive are equivalent: an internal assertion that cannot be reached and an answer
written to a socket that is already closed.
