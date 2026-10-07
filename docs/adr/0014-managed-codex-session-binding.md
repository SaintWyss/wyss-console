# ADR 0014: Managed Codex session lifecycle and runtime binding

Status: Accepted
Date: 2026-10-07

## Decision

A ManagedSessionId has at most one permanent CodexThreadId binding. It remains
prepared or closed; conversation binding does not introduce another control state.
A binding has its own revision (initially 1), canonical workspace and timestamps.
Resume preserves all of these. AgentRuntimeInstanceId identifies only the current
process attachment in memory; PID is never conversation identity. Multiple
ManagedSessions share one authority-owned App Server.

The authority alone admits create/resume through ExecutionGate, using exactly
session_create/session_resume respectively. turn_start implies neither scope.
Readiness, client connection and binding existence grant no permission. Workspace
must be an absolute existing directory, canonicalized without global chdir. The
canonical UTF-8 path is limited to 256 bytes to preserve bounded intents/projections;
symlinks resolve to their target. Resume loads the immutable stored workspace and
thread ID; the client cannot override either.

## Protocol and ownership

Control Protocol v1 adds managedSession/runtime/create, /status, /resume and
/abandon. Mutations accept typed session/intent identities and expected session
revision; create additionally accepts workspace_root. Status distinguishes unbound,
creating, indeterminate, bound_detached and bound_attached. Gate diagnostics remain
informational; production operations evaluate the internal gate themselves.

The existing supervisor remains the only provider IO owner. It accepts bounded
typed conversation commands, with one outstanding request globally and a one-slot
command channel. Request IDs are authority-generated UUID-based strings; responses
must match exact ID type/value. Late, duplicate and unrelated responses fail closed.
Notifications may interleave and are validated; notification arrival is never binding
proof. Any server request receives no response and terminates the owned runtime.
Each conversation operation (including all create durability steps/checkpoints) has
a ten-second total deadline, independent of the control client's
sixteen-second operation wait. No response map or transcript grows with operations.
At most 128 current session attachments are retained; new admission beyond this
bound is rejected. Detached bindings cost no background polling.

Production outbound methods are exactly initialize, initialized, model/list,
thread/start, thread/archive, thread/unarchive and thread/resume. The independent diagnostic probe remains restricted
to its original three methods. experimentalApi remains false.

Thread start parameters are exactly `{cwd: canonical_workspace, ephemeral: false}`.
Internal archive/unarchive parameters are exactly `{threadId: validated_created_id}`.
Thread resume parameters are exactly
`{threadId: stored_id, cwd: stored_workspace, excludeTurns: true}`.
There are no model/provider/approval/sandbox/config/instruction overrides, user
inputs, turn parameters or approval/permission responses. Correlated responses
must validate through the existing codec and return the expected canonical cwd and
thread identity. Resume identity substitution never changes a binding. Provider
model/policy/configuration fields remain observations and are discarded.

## Storage and external effects

Schema v4 migrates v1/v2/v3 explicitly, retaining prior sessions, runtime desired
state, leases, receipts, envelope bytes, stream and sequence. Migration creates no
bindings or provider work. managed_session_bindings stores the immutable provider
identity/workspace. managed_session_operations stores intent, canonical typed
mutation, session, kind, request ID, timestamps and a bounded state:
reserved, dispatching, succeeded, failed or indeterminate. The journal additionally
stores the validated provider thread ID and ordered provider checkpoints (none,
started, archived, unarchived). Checkpoints are acknowledged by the sole authority
writer before the supervisor sends the next step. No provider response body
is stored. Workspace and UUID normalization precede idempotency comparison.

Reservation commits before the dispatch marker, and that marker commits before any
provider command can be written. Start success alone is insufficient: the stable
archive/unarchive persistence barrier and its checkpoints must succeed. Only then
is one transaction committed for binding, operation success, minimal audit and original
receipt. Ordinary successful retries return the original binding and send nothing;
conflicting payload/method returns intent_conflict. Session, lease, binding and
runtime revisions remain independent.

The authority gate is evaluated at admission. The worker verifies the lease's UTC
expiration and an authority-owned cancellation flag immediately before writing,
against the same owned runtime instance. Close/revoke/renew/stop/expiry invalidate
queued admissions. The write is the external admission point: later lease expiry
or revocation cannot undo an already issued provider effect. The admitted create
finishes its durability barrier without admitting a new action. Response correlation
and durable completion continue, but confer no permission for subsequent work.
There is no client-supplied or serialized admission token.

There is **no exactly-once provider creation guarantee**. No verified thread/start
idempotency key exists in the stable schema. Crash before dispatch leaves reserved
work failed and safe for a new intent. A dispatch marker without committed success
recovers as indeterminate for create, never automatically retried. Transport errors,
timeouts, malformed results and post-response commit/audit failures are likewise
uncertain. Explicit abandon is allowed only for an unbound prepared session's
indeterminate create; it atomically audits and receipts abandonment. It does not
claim deletion or unarchival of any possible empty provider thread. A failed
unarchive can leave an archived durable provider orphan. There is no heuristic
thread/list discovery. Resume uncertainty publishes no attachment and becomes a
failed operation, permitting a new explicit, gated resume; no new provider identity
is invented. Storage uncertainty latches the existing execution health closed.

## Empty-thread persistence compatibility

Plain empty thread/start is not sufficient for restart durability in Codex 0.159.0.
The installed stable archive implementation materializes a loaded non-ephemeral
thread before archiving, even when no first turn exists. Wyss therefore uses exactly
thread/start → thread/archive → thread/unarchive as its internal create persistence
barrier. Archive and unarchive are not exposed through Control Protocol and are not
session-close behavior. The exact ID validated from start is used throughout; the
unarchive response must return that ID and the canonical workspace. No binding is
committed on any uncertain intermediate result; no start is retried automatically.

Create returns a durable **bound_detached** binding with no current attachment.
Unarchive restores persistent provider state; it does not authorize or prove a Wyss
attachment. Create never sends thread/resume. Explicit resume separately evaluates
session_resume and only a successful correlated response publishes bound_attached.
session_create permits this internal persistence barrier; turn_start permits none
of the implemented conversation actions. Runtime readiness alone grants nothing.

The narrow real compatibility probe passed on 0.159.0: start, archive, unarchive,
clean App Server exit, new App Server, same-thread resume; zero turns and prompt
bytes. Full real authority acceptance also passed with two independent threads,
explicit resume, graceful restart, runtime stop/start, SIGKILL containment/restart,
scope/expiry denial, historical close, unchanged workspace and unrelated processes.
Future Codex versions may simplify this barrier only after verified compatibility
and a deliberate policy/allowlist change. No name-setting workaround, experimental
historyMode or provider-storage inspection is used.

## Attachment and audit

Runtime exit/stop and authority restart discard current attachment. They retain the
binding and existing lease semantics. Restart never automatically resumes any thread.
Explicit resume with a currently valid lease is required. Lease expiry/revocation
does not delete the binding. Session close keeps its existing atomic lease handling,
retains the historical binding, makes attachment unusable, and sends no archive/delete.

New EventEnvelope v1 event types are authority.managed_session.runtime_bound,
authority.managed_session.runtime_attached and
authority.managed_session.runtime_operation_abandoned. All are durable/internal,
use session_id=ManagedSessionId and correlation_id=intent_id. Payload contains only
necessary adapter/runtime IDs, binding revision or abandoned operation ID. No full
workspace path, PID, provider body, model catalog, stderr or arbitrary user text is
audited. Audits and canonical success receipts commit atomically.

## Verified source and implementation limitation

Stable schema was checked against installed codex-cli 0.159.0 and official source
main revision `5a3140176e668a2f72f3c098490eb7f7052d9d85`
(2026-10-07T04:24:56Z). The installed release tag rust-v0.159.0 resolves to
`687a119f0fcaace47e1f1abcc77cec6c813fd6da` (2026-09-29T06:52:05Z).
Both stable schemas support the exact minimal parameters above. See the
[official App Server documentation](https://learn.chatgpt.com/docs/app-server).

The installed release includes
[pre-first-turn archive persistence](https://github.com/openai/codex/commit/81d5405882d5a8cfba9df39382526d54037f7078)
(`81d5405882d5a8cfba9df39382526d54037f7078`, #48828), exercised by the real probe.
The [naming persistence fix](https://github.com/openai/codex/commit/1f77c0cfa63484e1feee46de8dd5669581cd3adb)
(`1f77c0cfa63484e1feee46de8dd5669581cd3adb`, 2026-10-01, #49785) is later than
0.159.0 and targets empty paginated threads. It is not assumed present or used.
See also the
[installed archive implementation](https://github.com/openai/codex/blob/687a119f0fcaace47e1f1abcc77cec6c813fd6da/codex-rs/app-server/src/request_processors/thread_processor.rs#L1718)
and [deferred recorder](https://github.com/openai/codex/blob/687a119f0fcaace47e1f1abcc77cec6c813fd6da/codex-rs/rollout/src/recorder.rs#L871).

Deterministic tests cover binding/journal migration and rollback, denied gates with
zero frames, strict parameters/correlation, idempotency, provider errors, malformed
responses, timeouts, server requests, two sessions sharing a runtime, explicit
restart/resume, historical close and SIGKILL after writes/responses of start,
archive and unarchive. Recovery tests cover every durable checkpoint, before-start
reservation, before-binding audit rollback and after-binding receipt/reopen. No
crash window automatically duplicates a potentially dispatched start. Fake-provider persistence is not evidence of real Codex persistence.
No managed turn or prompt is enabled.
