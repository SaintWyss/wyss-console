# ADR 0051: One managed runtime per session, with bounded concurrency (DN-08 = A, F8)

Status: **Proposed.** Not accepted, not reviewed, no code. Written so that the review the roadmap
requires has something exact to review. Nothing here changes the running authority, its schema (V6) or any
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

## 1. Versioned identity

A runtime is named by `RuntimeKey = (adapter_id, runtime_kind, managed_session_id)`: at most one desired-state
row per session. Every launch has `RuntimeIdentityV2`:

| Field                 | Meaning                                                                        |
| --------------------- | ------------------------------------------------------------------------------ |
| `managed_session_id`  | The only session this process may serve                                        |
| `runtime_instance_id` | Fresh per launch, published after spawn, never reused (unchanged from 0012)    |
| `generation`          | Monotone per `RuntimeKey`; a relaunch is `generation + 1`; stored, never reset |
| `observed_version`    | The provider's numeric version at the handshake that created this generation   |
| `schema`              | `runtime_identity` version (2). An unknown version is refused, not interpreted |

No field is derived from a PID, a path or anything a client sends. Two runtimes never share an instance id,
a thread id, a catalogue, a lease, a turn index or an event identity (RC-03).

## 2. Attestation per generation

Everything the core believes about a runtime is true of **one generation** and is stored or kept in memory
with it: the handshake result, the model catalogue, the thread binding, each admitted lease, each admitted turn
and each approval carry `(managed_session_id, runtime_instance_id, generation)`. A check that finds a different
value fails closed with a fixed reason. Consequences, each a test:

- a lease of session A cannot admit a turn on the runtime of session B;
- an approval is answered only on the runtime instance that raised it; after a relaunch it is `invalidated`;
- a catalogue or binding from generation _n_ is not read as generation _n+1_;
- `runtime/status` for a session never reports another session's instance, version or catalogue.

## 3. Concurrency and resource budgets

- **Admission bound.** A global limit `MAX_ACTIVE_RUNTIMES` (an integer in the authority's own constants, not
  client-settable, default to be set from the measurements below, not guessed) and one active turn **per
  runtime**: `managed_turns_one_active` becomes a unique index on `managed_session_id` over the same predicate
  (V7). The global bound is enforced by the core's single writer before a spawn, so a start beyond it is a
  fixed `capacity` refusal, not a queue that grows.
- **Per-runtime budgets**, each with a fixed refusal or cancellation reason and none client-supplied: resident
  memory of the owned child, number of descendants it has, wall-clock per turn, bytes and events per turn and
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

- Desired state is durable per `RuntimeKey`; actual state is an observation. After an authority restart nothing
  is launched implicitly (0012): each `desired=running` row is shown as stopped-needing-start, and an explicit
  `runtime/start` for that session makes `generation + 1`.
- A crash of runtime A fails A (and its turn `indeterminate`); B is unaffected. There is no automatic restart
  loop.
- **Migration V6 → V7** (the `managed_runtimes` key, the turn index, `generation`): one transaction, exact
  schema and integrity validation of the result, the existing single row kept unchanged as the record of the previous shared runtime (no session
  attached); failure rolls back DDL, data and `user_version`.
  There is no downgrade: **a database snapshot is taken before the first start of the migrated authority**, and
  the previous application stays installed (`wyss-console.previous`) for rollback.

## 6. Protocol

`runtime/*` methods gain a `managed_session_id`; unknown fields still reject; nothing generic (no argv, env,
PID, prompt) appears. The hello and same-UID boundary are unchanged. A client written for the single-runtime
protocol keeps working against an authority with one runtime (the session is then implicit) and is told, in a
fixed word, when it asks for something that needs a session. Each new method is on the closed list the window
and the capability file name.

## 7. Required evidence before this can be accepted

1. **Measurements** (not taken; no provider quota is consumed by this ADR): idle resident memory, start
   latency and CPU of one App Server, and of two and four at once, on the owner's machine; at what count
   contention (disk, CPU, the provider's own limits) shows. The default `MAX_ACTIVE_RUNTIMES` follows from them.
2. **A provider review** of concurrent turns: that two App Server processes under one account do not corrupt
   each other's state (their own session files, auth refresh) — read from the provider's documentation and a
   read-only look at what they write, never by reading credentials.
3. **Adversarial multi-runtime tests** for every consequence in §2, plus: a runtime that exits during another's
   turn, a budget exceeded in A while B is mid-approval, relaunch racing a late approval answer, capacity at the
   bound, and migration failure at each step on a seeded V6 database.
4. An **independent security review** of the extended core, and the owner's acceptance of this ADR.

## 8. Not decided here

How a coordinator (F11b, OL3 pipelines) asks the authority for several runtimes (it uses the same authority API
and has no private path); how the window lays out several managed sessions (F7 design); whether Hermes or
the original Wyss may ever be one of these runtimes (DN-06 and its own ADR; the answer here is "no"). Nothing in
this ADR installs Hermes, uses a paid credential or exposes a listener.
