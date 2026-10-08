# ADR 0013 — Execution gates, leases and revocation

- Status: Accepted
- Date: 2026-10-07

## Decision

Execution is denied by default. Runtime process readiness is one necessary
condition, never permission to perform agent work. Sprint 2F introduces durable
permission facts and an internal gate, and executes zero Codex work. The runtime
outbound allowlist remains exactly initialize, initialized and model/list.
There is no thread, turn, prompt, tool, approval or permission-response operation.
No Tauri or human-terminal integration changes.

## Durable lease model

ExecutionLeaseId is a distinct strong UUID domain, independent of ManagedSessionId,
AgentRuntimeInstanceId, intent and request identity. Each lease belongs to one
ManagedSession. States are active, revoked and expired. History is retained;
a new grant after a terminal lease allocates a new ID. A partial unique index
allows at most one stored active lease per session. Overdue active rows are
reconciled before a new grant can commit.

Each lease contains an explicit nonempty set of at most three exact scopes:
session_create, session_resume and turn_start. There is no hierarchy, wildcard,
implicit tool permission or approval authority. Duplicate scopes reject. Storage
and intent comparison use this canonical order; different input order and UUID
case represent the same validated intent. TTL is an integer 1–3600 seconds;
permanent permission is impossible. The authority assigns UTC RFC3339 issued_at,
expires_at and updated_at. Lease revision starts at one and advances on renewal,
revocation or canonical expiration independently of session/runtime revisions.

Schema v3 adds execution_leases and its unique active index. The versioned v2→v3
transaction rebuilds only the metadata version constraint and creates the new
objects. Existing sessions, runtime desired state, processed intents, exact event
bytes, stream ID and sequence head survive unchanged. Fresh v3 and direct v1→v3
creation/upgrade are supported. Migration creates no leases and grants no authority.
Unknown, altered or corrupt schemas reject; migration failures roll back DDL,
data and user_version. No provider data or runtime catalog is added to storage.

## Control intents

- executionLease/grant: intent_id, session_id, expected_session_revision, scopes,
  ttl_seconds. Requires prepared session, matching revision and no effective active
  lease. Runtime readiness is unnecessary. Expire an overdue predecessor, allocate
  the new active revision-one lease, append audit and save receipt in one transaction.
- executionLease/get: lease_id. Read-only effective state with stored revision and
  timestamps. An overdue stored active row projects expired without manufacturing
  a canonical revision or event. It cannot be used as authorization.
- executionLease/active: session_id. Read-only effective active lease or null.
- executionLease/renew: intent_id, lease_id, expected_revision, ttl_seconds.
  Requires effectively active lease and prepared session. Preserve ID/scopes,
  increment revision, set expiration to authority NOW + TTL, audit and receipt.
  No scope field is accepted. Terminal permission requires a new grant.
- executionLease/revoke: intent_id, lease_id, expected_revision. Active→revoked,
  increment revision, fixed user_request reason, audit and receipt. Arbitrary
  reason text is not accepted. A new terminal-state intent returns invalid_transition.

Each mutation uses the existing single-writer immediate transaction and durable
processed-intent mechanism. Same intent/canonical payload returns the original
committed receipt even after expiration/restart; no extra revision, event or TTL
extension occurs. Conflicting method/payload returns intent_conflict. Receipts
are historical results, never execution authority. Read responses remain bounded
by the existing 64 KiB protocol limit. There is no historical-list endpoint.

## Expiry, clock and restart

The authority core reconciles overdue active rows before accepting client work at
startup. It caches only the next expiration deadline, not permission state, and
waits on its existing bounded channel until that deadline or a mutation/shutdown
wakeup. Grant/renew/revoke/session close recalculate the earliest active deadline.
No leases means an indefinite channel wait. Future deadlines use at most a
30-second clock recheck without querying SQLite. Canonical expiry transactions
run only on startup, relevant mutations or when an active deadline is reached.

UTC wall time is authoritative because deadlines survive process restart. A forward
clock jump makes a lease immediately ineffective at the gate and is reflected by
the scheduler within its bounded recheck. A backward jump may lengthen the remaining
wall-time window; it never renews or changes the recorded deadline. No monotonic
clock value is persisted. Normal elapsed-time waiting uses bounded process waits.

At now >= expires_at, authorization denies even if storage still says active.
Canonical expiry changes active→expired, increments revision and atomically appends
one expiry audit, without a client receipt/correlation. Repeated reconciliation
and restart do not append duplicates. A failed expiry write rolls back and latches
execution unavailable for that authority lifetime. Gate diagnostics report
`authority_unavailable`; no unaudited permission is created. Other canonical
storage/audit failures also latch execution closed. There is no busy retry loop;
restore storage health and restart the authority to restore execution availability.

A still-valid lease survives client disappearance, graceful authority restart and
SIGKILL unchanged: same identity, revision, scopes and expiration, with no extension.
Overdue leases are reconciled on restart. Runtime restart may allocate a fresh
runtime instance independently. Runtime stop does not revoke a lease; it continues
aging. Runtime startup does not create or renew one.

## Session close and audit

Session close handles its stored active lease in the same immediate transaction:
first expire an overdue lease (not a false revocation), otherwise revoke it with
session_closed; then close the session and save the close receipt. Event order is
lease expiry/revocation before session close. Internal close revocation correlates
to the close intent; expiry correlation remains null. Any required write/audit
failure rolls back all lease/session/events/receipt changes. Closed sessions can
never receive a grant, renew a lease or pass the gate.

EventEnvelope v1 remains unchanged. New event types are exactly:
authority.execution_lease.granted, authority.execution_lease.renewed,
authority.execution_lease.revoked and authority.execution_lease.expired.
Events are durable/internal, use session_id=ManagedSessionId and correlate to the
client intent except autonomous expiry. Payload contains only lease_id, canonical
scopes, lease_state, revision, expires_at and revocation_reason when revoked.
No prompt, arbitrary user text, model response/catalog, runtime frame, PID,
environment or authentication data is included. Audit and mutation commit together.
Read-only gate evaluations are not durably audited in this sprint.

## Internal gate

ExecutionGate returns a non-serializable typed Allowed/Denied decision. Future
authority-owned side effects must evaluate it NOW, without trusting a previous
client diagnostic or lease receipt. It checks, in deterministic order:

1. Authority canonical storage/audit health: authority_unavailable.
2. Session exists: session_not_found; prepared: session_not_prepared.
3. Current active lease, otherwise latest terminal history: lease_missing,
   lease_not_active (revoked), lease_expired (expired).
4. Authority now strictly before expiration: lease_expired.
5. Exact requested scope: scope_not_granted.
6. Session adapter/runtime matches runtime: runtime_mismatch.
7. Desired running: runtime_not_running.
8. Actual ready: runtime_not_ready.
9. Live AgentRuntimeInstanceId: runtime_identity_missing.
10. Compatible runtime: runtime_incompatible. Unknown/degraded also deny.

Only all conditions together return allowed. executionGate/status projects only
session, action and reason for diagnostics; no serialized capability token exists
and no operation accepts a gate result for replay. Same UID, client connection,
model availability and adapter capability support grant no permission.

## Validation

Clock-controlled unit tests cover exact deadline boundaries, idempotency,
canonicalization, scope isolation, TTL bounds, independent identities/revisions,
each gate denial, audit failures and atomic close rollback. Real seeded SQLite v2
migration tests compare prior data/events byte-for-byte, including receipts,
runtime desired state and metadata, and test rollback/corruption/future versions.
Process tests exercise concurrent grants, autonomous expiry, disconnects,
graceful/SIGKILL restart, parent-death containment and the unchanged Codex method
allowlist. Isolated real-Codex acceptance observes permission/readiness separation
without creating threads or turns. Sprint 2G remains unimplemented.

## Later implementation status

Sprints 2G to 2K are implemented (ADRs 0014 to 0018). A lease now holds up to five
exact scopes: `command_execute` and `file_change` were added by ADR 0016 with schema
v6, with the same rule that no scope implies another. The gate additionally requires
the profile and attachment facts of ADRs 0015 and 0016. A turn and every approval of
it stay bound to the lease the turn was admitted under: renewing that lease extends
it, a replacement never transfers authority. Since ADR 0018 the admission of a turn
that was not yet dispatched is withdrawn before a revocation of its lease or a close
of its session commits. The schema is at version 7.
