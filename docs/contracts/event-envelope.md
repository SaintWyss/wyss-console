# Event envelope and data handling

Accepted EventEnvelope v1 rules, consolidated without the completed sprint history.
The executable definitions and shared fixtures remain authoritative for exact
validation: [Rust envelope](../../src-tauri/src/contracts/event_envelope.rs),
[TypeScript envelope](../../src/lib/contracts/event-envelope.ts) and
[identity types](../../src-tauri/src/contracts/identities.rs).

## Wire fields

All fields are required. Nullable values are explicit `null`, not omitted.

| Field                    | Rule                                                                                               |
| ------------------------ | -------------------------------------------------------------------------------------------------- |
| `schema_version`         | Integer literal 1.                                                                                 |
| `event_id`               | UUID preserved on replay/retransmission.                                                           |
| `event_type`             | Namespaced dot-separated lowercase segments, each matching `[a-z][a-z0-9_]*`.                      |
| `payload_schema_version` | Positive integer, independently versioned from the envelope.                                       |
| `timestamp`              | Writer-recorded RFC3339 UTC, not ordering authority.                                               |
| `stream_id`              | UUID of a stream with one writer.                                                                  |
| `sequence`               | Canonical decimal string `[1-9][0-9]*`, at most `18446744073709551615`; never a JavaScript number. |
| `session_id`             | Canonical ManagedSessionId, or null for local/global events.                                       |
| `source`                 | Nonblank/control-free component and writer instance UUID; declared provenance, not authorization.  |
| `correlation_id`         | Operation UUID or null.                                                                            |
| `causation_id`           | Causing event UUID or null.                                                                        |
| `payload`                | JSON object; the producer/writer validates the event-type payload contract.                        |
| `persistence`            | Explicit `durable` or `ephemeral`; policy defaults to ephemeral.                                   |
| `sensitivity`            | `public`, `internal`, `sensitive` or `secret`.                                                     |
| `redaction`              | `none`, `applied` or `content_omitted`; a description, not proof of safe content.                  |

Unknown envelope/source fields reject. The generic envelope permits an open
payload object; this does not waive an authority producer's closed payload policy.
UUID text is 36-character hyphenated, case-preserving, with no extra version/variant
restriction. UTC text preserves fractions and accepts Z/z/+00:00; -00:00 is unknown
offset and rejects. Calendar/leap-second validation follows the shared fixtures.

Local console session, terminal, runtime, task and provider references belong in
their appropriate payloads. Local and Managed identity domains are distinct even
though their UUID wire shapes match. Writer identity is not runtime identity.

## Ordering, replay and authority

- Order is `(stream_id, sequence)`, not timestamp; there is no global causal
  ordering across independent streams. A stream has one fixed persistence class.
- Durable append assigns sequence transactionally, continues across restarts and
  preserves event IDs/sequences on replay. Deduplicate by event_id. Existing
  durable records are not rewritten; incompatible changes require versioning.
- Ephemeral streams can lose data and use new stream identities after restart.
- Unknown event types/versions do not drive control transitions. Adapter reports,
  provenance, capability claims and UI events cannot grant approval or permission.
- Canonical authority state lives in its control tables. Audit reads/watches are
  observation; they do not reconstruct or replay execution. Required audit append
  failure rolls back the privileged mutation and keeps execution closed.

The authority's exact append/replay vocabulary and storage rules remain in the
[control audit decision](managed-runtime.md) and
[store hardening](managed-runtime.md).

## Content policy

Durable events need explicit writer policy and contain minimized metadata. Raw
prompts, arbitrary model content, terminal bytes and incremental output are not
durable by default. High-volume content uses separate bounded transient channels
with backpressure, not one durable event per chunk. Local diagnostics stay
separate from canonical audit. Visual UI state need not be event-sourced.

`durable` plus `secret` rejects regardless of redaction. Minimize/redact before
append; labels and generic validators do not detect secrets in arbitrary strings
or authorize content capture. References cannot bypass the same content policy.

The [accepted sealed-restoration custody](configuration.md)
is a specific exception for protected restoration material, not permission to put
credentials in audit, transcripts, event payloads or ordinary metadata stores.
