# F8 candidate implementation contracts, revision 1

**Proposed, not owner-accepted, not enabled.** These artifacts freeze a bounded candidate for
implementation review; they are not production migration code or installed concurrency acceptance.
No file in this directory is loaded by the authority. Provider shared-state safety and concurrency
at 2/4 remain NOT_PROVEN/NOT_RUN, so active runtime capacity stays 1.

- [Exact V7→V8 SQL](v7-to-v8.sql), evaluated by the contract test against ordinary in-memory
  SQLite unit fixtures. The fixture uses the current V7 schema source and synthetic SQL rows, never
  the live database; it does not prove canonical-record validation.
- [Protocol 2](protocol-2.md): closed inherited method shapes plus explicit deltas, result identity,
  historical references, replay precedence and vocabulary-2 event attribution.
- [Numeric budgets](resources.json): candidate ceilings, not measured performance claims.
- `src-tauri/tests/f8_contracts.rs`: executable SQL, request-delta and routing reference tests.
  Included in `npm run rust:test`; they start no provider, host, socket or authority process.

## SQL application boundary

The future implementation must first run the unchanged supported older-schema path to V7 on
ordinary fixtures, validate exact V7 objects and every canonical record, and prove maintenance
quiescence. Then it opens one IMMEDIATE transaction, executes the SQL statements, validates the
entire expected V8 schema/FKs/canonical records, audits and receipts, and commits. The SQL file
intentionally contains no BEGIN/COMMIT: that validation is part of the same caller transaction.
Error at any statement or pre-commit validation rolls back DDL, data, metadata and user_version.

Quiescence requires no active turns, pending approvals, active leases, or reserved/dispatching
binding operations. Any remain => refusal, not silent cancellation/rewrite by migration. Legacy
indeterminate effects/history remain unassigned and cannot authorize new work. Preserve all
historical rows/receipts/event bytes and stream/head; raise event vocabulary monotonically to 2.
Up to one immutable legacy runtime row is retained separately. No arbitrary session is assigned to it.

Generation allocations are append-only and made in the same transaction as desired state,
receipt and vocabulary-2 desired-state audit. A new attempt consumes generation even if spawn
fails. SQL sidecars are inserted before their base record; their base FK is deferred until commit.
SQL checks ownership of the same session/current desired generation; it cannot attest that an
OS process is live or that a lease/gate is valid. Those remain mandatory core checks at dispatch.
No sidecar may be retrofitted to an existing legacy row.

Global lifetime limits are 128 new runtime-key rows and 4,096 new launch-allocation rows **per
database**, including stopped, failed and crash-reserved attempts. Legacy V7 rows do not count.
The sole writer counts/reserves atomically before mutation/spawn; SQL triggers independently
refuse one extra row. A failed capacity attempt leaves generation/revision/receipt/audit unchanged.
Exact receipt replay is resolved before a new-capacity check and consumes no slot/row. No TTL,
deletion, reset or implicit compaction exists; exhaustion refuses `capacity_exceeded` until a
separately reviewed retention/limit-upgrade decision. The test verifies exhaustion plus rollback.

An offline, verified SQLite-consistent snapshot and matching prior binaries remain mandatory for
an owner-approved installed migration. No tool here migrates or backs up the live authority.
Rollback still requires exclusive writer, complete snapshot restore and acknowledged loss of
post-snapshot writes; never `install.sh rollback` unattended because it stops the terminal host.

## Identity routing and scheduling obligations

The registry is keyed by typed `(adapter,kind,session)`. A launch owns its generation, fresh instance
ID, authority instance, retained process handle, private state/catalogue directory, channels,
pending bindings, attachment, Accept admission, turns, streams, deadlines and approvals.
Provider request/item IDs are scoped to the owning connection and may repeat across A/B. No
lookup by PID, name, selected GUI session or “only runtime” is an execution target.

Every queued item carries allocation identity; the core checks exact identity on dequeue and again
before dispatch, plus the original lease/revision/expiry/workspace/profile/fingerprint guarantees.
Losing A clears/revokes only A's admissions/lease, invalidates its pending approvals and preserves
unknown dispatched effects as indeterminate. Restart loses transient attestation, not allocation
history. Historical reference is distinct from observed live provider version; never fill a missing
version from the installed binary. Explicit-only restart and fresh leases remain owner decisions.

Reserve active capacity before discovery and keep it through starting/initializing/ready/stopping
until owned reap. Stop/reap grace runs outside the core writer. New starts at the bound refuse;
there is no pending-start queue. Existing compatible ready start intents keep the same launch.

The core rotates workers, handles at most four data messages per 50 ms quantum, and services
reserved control channels first. Control messages are bounded to 4 KiB, eight slots per worker.
Data queues have both event and serialized-byte bounds. An overflow never drops an approval,
revocation, violation or terminal event silently: invalidate admission, request owned cleanup,
record failure when storage permits. A slow client cannot block producer/control progress.

250 ms is a candidate control-dispatch acceptance ceiling under specified saturation tests;
it is not an unconditional scheduler/fsync latency promise. If the measured machine cannot meet
it, do not enable concurrency or weaken expiry/revocation. Native-confirmation wait belongs to
the requesting action, never blocks the core scheduler. EOF/TERM grace is 1/2 seconds outside
the writer, followed by owned kill/reap. No discovered PID or arbitrary process-tree signal.

## Resource enforcement semantics

Existing frame/model/stream bounds are retained. The remaining numeric ceilings are conservative
engineering choices for review, not inferred from the earlier swapped-memory sample. They may
cancel a legitimate workload, a residual product risk requiring owner acceptance.

- Counter/time overflow or over-budget before admission => `capacity_exceeded`, zero provider frames.
- Turn output/event/rate/wall limits use monotonic time and checked arithmetic. Rate uses a bounded
  token bucket (256/s, burst 512); no catch-up overflow. Output and events count the whole turn,
  not just retained stream bytes. Stream eviction still reports its explicit gap.
- On an active budget failure, invalidate that runtime's admission and begin owned cancellation.
  Its unknown effects remain indeterminate; other runtimes' gates/approvals remain unchanged.
  Candidate protocol 2 adds fixed diagnostic `budget_exceeded` and nullable `budget_failure` to
  runtime status, containing exactly one of the numeric budget-key words in resources.json.
  No provider text. Turn reason remains the existing `runtime_failed` when terminalized as failed.
- Candidate vocabulary-2 runtime-exited payload adds `budget_failure:null|budget-key`; exit_kind
  additionally permits `budget_exceeded`. No arbitrary diagnostic string or runtime argument.
- RSS ceilings are **soft observations of retained owned process roots**, not hard descendant or
  total-machine containment. Include the attempt's owned discovery/version/main roots; do not
  double count shared roots or infer ownership from process enumeration. Hard tree containment
  and provider auth/cache/session compatibility still require separate proof before concurrency.

Full canonical SQL validation, production protocol serializers/parsers, fair scheduler, owned
cleanup and real provider tests are future implementation requirements. Passing a request/routing
oracle does not prove those implementations or the live authority boundary.

## Independent contract review

The first independent pass identified four issues: historical version reconstruction, missing
event attribution, stale-generation sidecars and lifetime-limit exhaustion semantics. Corrections:
durable RuntimeRef versus transient attestation; vocabulary-2 payload shapes; current-generation
insertion triggers plus live core checks; lifetime bounds with atomic refusal/replay semantics.
The independent follow-up found no new design blockers in the reviewed SQL/protocol/resource/README
surfaces. Its two minor clarifications are incorporated: the legacy row may be absent, and receipt
lookup at exhausted capacity proves historical readability, not the future replay resolver.
Ten passing contract tests exercise candidate SQL and reference oracles; they are not provider tests. Production
security remains unchanged; ADR 0051 still requires Santiago's acceptance and implementation review.
