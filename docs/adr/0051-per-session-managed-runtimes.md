# ADR 0051: One managed runtime per session, with bounded concurrency (DN-08 = A, F8)

Status: **Proposed.** Independently reviewed design; not owner-accepted, no implementation. Written so that the review the roadmap
requires has something exact to review. Nothing here changes the running authority, its schema (V7) or any
accepted guarantee. Extends ADRs 0012 and 0015; reconciles RC-03 (`33-CROSS-DOMAIN-RECONCILIATION.md`).
Date: 2026-10-20

The owner's direction for DN-08 is **A**: a separate managed runtime per session, with bounded concurrent
supervised work (OL3). That is a direction, not an acceptance of this ADR. A process boundary is not an
isolation proof (32 §2); what follows says what has to be true instead.

## What is true today (the invariants this must not weaken)

- `wyss-control` owns **at most one** Codex App Server; `managed_runtimes` has one row, constrained to
  `adapter_id='codex'` (schema V2); `runtime/start|stop|status|model/list` take no session.
- **One active turn** for the whole authority: `managed_turns_one_active` is a unique index over a constant
  on `state IN ('starting','running')` (V5).
- Identity domains are separate: `ManagedSessionId`, `AgentRuntimeInstanceId`, `CodexThreadId`, PID,
  authority instance. A restarted process gets a fresh instance id, never reused; PID never enters rows.
- Leases, bindings, turns and approvals are per **session**; turns and approvals already carry the
  `runtime_instance_id` they were admitted under. Same-UID peer credentials are not execution authority.
- Cleanup signals only the unreaped `Child` the authority owns; no process enumeration drives a signal.

**Also global today:** `managed_approvals_one_pending`, transient `Turns.active`, `Turns.accepting`,
`Conversations.pending`, worker routing and synchronous cleanup. These must be scoped too.

## 1. Versioned identity

A runtime is named by `RuntimeKey = (adapter_id, runtime_kind, managed_session_id)`: at most one desired-state
row per session. Every launch has `RuntimeIdentityV2`:

| Field                   | Meaning                                                                        |
| ----------------------- | ------------------------------------------------------------------------------ |
| `authority_instance_id` | Authority instance owning the allocation; also fenced in post-hello requests   |
| `managed_session_id`    | The only session this process may serve                                        |
| `runtime_instance_id`   | Fresh per launch, published after spawn, never reused (unchanged from 0012)    |
| `generation`            | Monotone per `RuntimeKey`; a relaunch is `generation + 1`; stored, never reset |
| `observed_version`      | The provider's numeric version at the handshake that created this generation   |
| `schema`                | `runtime_identity` version (2). An unknown version is refused, not interpreted |

The requested session key is validated against canonical storage. Instance and generation are assigned by
the authority; provider version is observed at handshake. PID and client assertions never supply these facts. Two runtimes never share an instance id,
a thread id, a catalogue, a lease, a turn index or an event identity (RC-03).

## 2. Attestation per generation

Everything the core believes about a runtime is true of **one generation** and is stored or kept in memory
with it: the handshake result, the model catalogue, the thread binding, each admitted lease, each admitted turn
and each approval carry `(managed_session_id, runtime_instance_id, generation)`. A check that finds a different
value fails closed with a fixed reason. Consequences, each a test:

- a lease of session A cannot admit a turn on the runtime of session B;
- a pending approval is answered only on the instance that raised it and is invalidated after relaunch;
  accepted/dispatched actions without confirmed outcome become indeterminate; confirmed history is preserved;
- a catalogue or binding from generation _n_ is not read as generation _n+1_;
- `runtime/status` for a session never reports another session's instance, version or catalogue.

## 3. Concurrency and resource budgets

- **Admission bound.** A global limit `MAX_ACTIVE_RUNTIMES` (an integer in the authority's own constants, not
  client-settable, candidate cap **1** pending qualified compatibility/resource measurements) and one active turn **per
  runtime**: `managed_turns_one_active` becomes a unique index on `managed_session_id` over the same predicate
  (candidate V8). The pending-approval index also becomes session-scoped. The global bound is enforced by the core's single writer before a spawn, so a start beyond it is a
  fixed `capacity_exceeded` refusal, not a queue that grows.
- **Per-runtime budgets**, each with a fixed refusal or cancellation reason and none client-supplied: resident
  soft memory observations of retained owned roots, wall-clock per turn, bytes and events per turn and
  per second, pending approvals. They are read from `/proc/<pid>` of **the child the authority owns** (held as
  an unreaped `Child` plus a pidfd, the P0 rule), never from a search by name. A budget exceeded cancels that
  runtime's turn (§4) and marks it `failed` with the budget's name; it never touches another runtime.
- **Back pressure.** Each runtime has its own bounded channels; a slow consumer of A cannot delay B, and the
  event stream remains one ordered stream written by the one core.

## 4. Cancellation

`turn/interrupt` is per turn and per runtime. Escalation for a runtime that does not stop: interrupt, stdin EOF
with a bounded grace, `SIGTERM`, `SIGKILL` and reap, **through the owned handle only**. Stopping or cancelling
session A cannot reach B's child because B's handle is not A's. Authority shutdown cancels every runtime in a
fixed order and records, per runtime, what was in flight as `indeterminate` (existing semantics), never as
completed. `PR_SET_PDEATHSIG` remains the hard-crash mechanism for every child.

## 5. Recovery

- Desired state is durable per `RuntimeKey`; actual state is an observation. Candidate change: after an authority restart nothing
  is launched implicitly: each `desired=running` row is shown as stopped-needing-start, and an explicit
  `runtime/start` for that session makes `generation + 1`. Today `server.rs` calls
  `reconcile(..., true)` and `runtime.rs` permits startup launch: explicit-only restart requires
  owner acceptance; it is not an existing ADR 0012 guarantee.
- A crash of runtime A fails A (and its turn `indeterminate`); B is unaffected. There is no automatic restart
  loop.
- **Migration V7 → V8** (the `managed_runtimes` key, both turn and approval indexes, `generation`): one transaction, exact
  schema and integrity validation of the result, up to one existing row kept unchanged in a separate immutable legacy record of the previous shared runtime (no session
  attached); failure rolls back DDL, data and `user_version`.
  There is no downgrade: **a database snapshot is taken before the first start of the migrated authority**, and
  the previous application stays installed (`wyss-console.previous`) for rollback.

## 6. Protocol

Candidate: Managed protocol 2 with explicit `managed_session_id` on `runtime/*`, and launch identity
where a stale operation could hit a replacement. Unknown fields reject. No arbitrary argv, env, PID, model
or effort is added; existing accepted local-tools profile/workspace exceptions remain narrow. SG-12 stays
closed. Same-UID is not execution authority. An F8 authority rejects a protocol-1 hello with the existing
`protocol_version_unsupported`; it never guesses the session from the GUI or the number of runtimes.
GUI and authority ship together; a compatible old authority stays alive with update pending until an
owner-approved switch. This replaces the original implicit-session compatibility promise. Every new method
stays closed and capability-listed. Test the coordinated upgrade and rollback.

## 7. Required evidence before this can be accepted

1. **Measurements** (one existing runtime sampled read-only; concurrent counts/startup NOT_RUN; no provider quota consumed): qualified idle resident memory (NOT_RUN), start
   latency and CPU of one App Server, and of two and four at once, on the owner's machine; at what count
   contention (disk, CPU, the provider's own limits) shows. The default `MAX_ACTIVE_RUNTIMES` follows from them.
2. **A provider review** of concurrent turns: that two App Server processes under one account do not corrupt
   each other's state (their own session files, auth refresh) — read from the provider's documentation and a
   read-only look at what they write, never by reading credentials.
3. **Adversarial multi-runtime tests** for every consequence in §2, plus: a runtime that exits during another's
   turn, a budget exceeded in A while B is mid-approval, relaunch racing a late approval answer, capacity at the
   bound, and migration failure at each step on a seeded V1–V7 databases.
4. An **independent security review** of the implemented extended core, and the owner's acceptance of this ADR.

## 8. Not decided here

How a coordinator (F11b, OL3 pipelines) asks the authority for several runtimes (it uses the same authority API
and has no private path); how the window lays out several managed sessions (F7 design); whether Hermes or
the original Wyss may ever be one of these runtimes (DN-06 and its own ADR; the answer here is "no"). Nothing in
this ADR installs Hermes, uses a paid credential or exposes a listener.

## 9. Normative corrections from independent design review (2026-10-20)

These refine the candidate above; no production code changed. See the
[evidence, findings and residual risks](../research/f8-runtime-decision-2026-10-20.md) and
[adversarial specifications](../spec/f8-runtime-acceptance.md).

- Reserve a durable checked generation before each attempt, including failed spawn. Never reset, reuse or
  wrap it. Intent replay returns the original receipt without launching. Discovery, starting, initializing,
  ready and stopping workers consume capacity until reaped. The staging ceiling stays **1** until evidence
  supports a higher release limit; this is not F8 concurrency acceptance.
- Scope pending bindings, Accept attestations, admissions, streams, deadlines and observations by
  `(authority instance, session, runtime instance, generation)`. Reject late/cross-runtime work before changing
  state. Revalidate lease/revision/expiry, workspace, safety profile, attachment and fingerprint immediately
  before dispatch. Candidate policy: runtime loss revokes that session's active lease, clears its attachment,
  invalidates pending approvals and requires fresh explicit lease/admission. Unknown accepted effects remain
  indeterminate. Durable session leases are never transferable launch capabilities.
- Current `supervised.rs` fixed catalogue/staging names and `isolation.rs` shared `state/home` cannot serve
  concurrent launches. Use exclusive private per-launch directories, immutable per-launch catalogues and
  separately scoped local-tools state. Never overwrite/clean a file consumed by another live runtime. No
  credential copying or new credential store. Shared provider auth/cache/SQLite/rollout compatibility must
  be established for the pinned version before concurrent launches against that mutable state are enabled.
- `/proc` observations are not hard process-tree limits. No discovered descendant PID becomes a signal target.
  `PDEATHSIG` covers the owned direct child, not grandchildren. Aggregate containment needs its own verified
  mechanism. Refuse unsupported execution instead of broadening cleanup.
- Stop/reap grace runs outside the core writer. Per-worker bounded channels need fair scheduling and reserved
  revocation/expiry/cancellation capacity. Freeze numeric queue, bytes/events/rate, retained-row, aggregate
  memory and control-latency ceilings before release. Missing limits block release.
- Freeze exact V8 SQL and validators before implementing migration. Older supported schemas keep their
  validated path to V7. Preserve stream ID, sequence, events, receipts and historical identity
  bytes; raise vocabulary monotonically to 2 without rewriting historical events. Never invent historical generations. Validate exact schema, all records and foreign keys before commit.
- Backup uses SQLite's consistent backup facility or a verified quiescent DB/WAL pair, private permissions,
  schema/version and checksum manifest, matching prior binaries and exclusive-writer proof. Never copy only
  a live `.db`. Rollback requires owner-approved downtime, stopping the new authority through its owned
  lifecycle and restoring the complete verified snapshot, acknowledging loss of post-snapshot writes.
  No SQL downgrade. `install.sh rollback` stops the terminal host: it is not an unattended authority rollback.

Only Santiago may accept this ADR and its restart/lease/protocol tradeoffs, authorize interruption of live
sessions or installed migration/rollback, approve paid-quota experiments, or expand Managed guarantees.
The [candidate implementation contracts](../contracts/f8/README.md) now specify exact V8 SQL, protocol 2,
identity routing and numeric budgets, with ten executable contract/reference tests and independent
follow-up review. These are implementation inputs, not a production migration or enforcement proof.
Production validators/serializers, scheduler and owned cleanup, provider compatibility, concurrency
measurements and implementation-level adversarial/security review remain required. Active capacity
stays 1 until separate compatibility/resource evidence supports an owner-approved increase.
**Reviewed implementation contract candidate; NOT_PROVEN; not approved for production.** Design review is not review of implemented F8 security;
F8 remains **NOT_PROVEN**.
