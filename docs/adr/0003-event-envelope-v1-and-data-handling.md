# 0003 — EventEnvelope v1 and data handling

Status: Accepted

Date: 2026-10-06

## Decision

EventEnvelope v1 is frozen as documentation only in Sprint 0A. No executable
event type, validator, producer, consumer, Event Bus or event persistence exists
in this sprint. Runtime representation starts in Sprint 0B, before the first producer.

All envelope fields are present. Optional values are explicitly `null`.

| Field                    | Contract                                                                                        |
| ------------------------ | ----------------------------------------------------------------------------------------------- |
| `schema_version`         | Integer literal `1`; envelope version.                                                          |
| `event_id`               | UUID; unique event identity preserved on replay/retransmission.                                 |
| `event_type`             | Namespaced string, such as `console.session.created` or `agent.started`.                        |
| `payload_schema_version` | Positive integer; payload version for its event type.                                           |
| `timestamp`              | RFC3339 UTC time recorded by the writer; not ordering authority.                                |
| `stream_id`              | UUID identifying an ordered stream with exactly one writer.                                     |
| `sequence`               | Positive decimal integer string, representable as Rust `u64` without JavaScript precision loss. |
| `session_id`             | Canonical managed-session UUID, or `null` for local/global events.                              |
| `source`                 | Object with `component: string` and `instance_id: UUID`.                                        |
| `correlation_id`         | UUID or `null`; a cross-cutting operation's correlation.                                        |
| `causation_id`           | Causing `event_id` or `null` when unknown/not applicable.                                       |
| `payload`                | JSON object validated against the event type and payload version.                               |
| `persistence`            | `durable` or `ephemeral`.                                                                       |
| `sensitivity`            | `public`, `internal`, `sensitive` or `secret`.                                                  |
| `redaction`              | `none`, `applied` or `content_omitted`; describes emitted content.                              |

Local session events carry `console_session_id` in their payload. Terminal,
process, agent, task and model references belong in the relevant payload, avoiding
hundreds of mandatory identifiers in the envelope. Source identifies declared
provenance; it is not proof of authorization.

## Ordering and versioning

- Ordering is `(stream_id, sequence)`, never timestamp.
- Each stream has one writer and a fixed persistence class. Durable and ephemeral
  streams are distinct. No global total ordering between streams is promised.
- Durable sequence numbers are assigned on confirmed append and continue across
  writer restarts. Replay preserves event IDs and sequences; deduplication uses `event_id`.
- Ephemeral streams can lose data and start new streams on restart.
- Durable events are append-only and versioned; existing records are not rewritten.
- Envelope and payload versions are separate contracts. Incompatible changes
  require explicit versioning and migration decisions.
- Unknown event types or versions never drive control transitions.
- Adapter observations cannot establish grants or approvals. Only the canonical
  authority confirms control-state events.

## Persistence and content channels

Default persistence is **ephemeral**. A durable event type requires an explicit
policy and writer validation. Suitable audit metadata includes lifecycle, control
decisions, requests/grants/denials, routing, tool/test results and consolidated
cost/latency records. Local diagnostics remain separate from control audit.

High-volume terminal bytes and incremental content use separate bounded transient
channels, with backpressure defined before their implementation. They are not
wrapped into a durable event per chunk. Progress and partial counters are ephemeral.

Raw terminal output, raw prompts and arbitrary model content are not durable by
default. Prompt events may record identity, state and explicitly authorized
references without storing prompt text. Content capture is a separate future
decision; this ADR does not authorize it.

Secrets and credentials never enter durable storage. Minimization and redaction
happen before persistence. Sensitivity/redaction labels do not replace content
validation or authorize storage. Events labelled `secret` cannot be persisted.
References must not provide an implicit route to secret or unauthorized content.

Privileged operations fail closed if their required auditable decision cannot be
recorded. Local visual state need not be event-sourced. Database, storage layout,
retention, hash chaining and observability exports are deferred.

## References

- Wyss Console base specification v0.1, section 10.
- Wyss architecture research report, section 16.
- Accepted pre-implementation DECISION_PACKET, 2026-10-06.

## Sprint 0B implementation status

The envelope is now executable in the existing application crate's `contracts`
module and in the frontend's `lib/contracts` modules, independently of Tauri
commands. `EventEnvelopeV1::from_value` / serde deserialization and
`parseEventEnvelope` reject malformed input. Errors contain only a field and a
stable code, never rejected content. Shared JSON fixtures exercise acceptance,
rejection and semantic serialization round-trips in both languages.

Implementation details of the frozen v1 wire representation:

- All 15 fields remain required, including explicit null optional identities.
  Unknown envelope/source fields reject rather than being silently dropped;
  payload objects remain open. No payload registry or control transitions exist.
- Sequence is canonically `[1-9][0-9]*`, bounded by `18446744073709551615`.
  Rust stores a private `u64` with string serde; TypeScript uses a branded string.
  Neither timestamps nor JavaScript numbers determine stream ordering.
- UUIDs use the 36-character hyphenated representation, retain their original
  case, and impose no extra UUID version/variant restrictions. RFC3339 UTC text
  is preserved, including fractions; `Z`, `z` and `+00:00` are UTC, while
  `-00:00` denotes an unknown offset and rejects. Calendar validation uses Rust
  `time` and matching TypeScript checks. Leap-second syntax at the end of a UTC
  month is accepted without claiming to verify historical leap announcements.
- Event types use dot-separated lowercase segments matching `[a-z][a-z0-9_]*`.
  Source component names must be nonblank and contain no control characters.
  Declared provenance grants no authority.
- `durable` + `secret` rejects regardless of redaction metadata. The conceptual
  default is ephemeral, but persistence must still be explicit on the wire.
  `redaction: none` remains valid for inherently minimal metadata; labels never
  certify safety. Validators cannot detect secrets inside arbitrary content or
  certify that minimization occurred. A future writer must enforce the explicit
  durable event policy and content handling before append.
- `ConsoleSessionId` and `TerminalId` are distinct local UUID types, separate
  from the canonical `ManagedSessionId` used by `session_id`. No ID generators,
  process IDs, agent IDs or session lifecycles are implemented.

Event Bus, writer coordination, payload-specific validation, persistence, replay,
content inspection, and bounded transient terminal/content channels remain
**deferred**. Durable/ephemeral classes do not select a transport or database.
The client/authority and crash/restart contracts in ADR 0002 remain unchanged.
