# Finite coordination, handoff and workspace guards

Source-based contract for `src-tauri/src/orchestration/`, `f8/guard_protocol.rs`,
`f8/workspace_guard.rs`, `f8/v10.rs` and `control/application.rs`.
The coordinator and V10 support exist; production guarded execution remains disabled.
Native delegation is a separate service in [delegation](delegation.md).

## Owners and admission

The `--wyss-orchestrator` process owns its metadata journal and finite scheduler.
The desktop daemon client starts it on demand if the checked socket is absent, including
for read requests; it is not an installed service or an exclusively operator-started process. It has no independent execution permission. `F8Authority` requests the current
authority's capabilities, binds store/epoch and uses its closed guard extension. A V8
authority does not expose the V10 guard. A V10 authority reports guard readiness separately
from `managed_execution_enabled`; `ready` requires both. That execution flag is false in
production constructors and enabled only by a test-only fixture method.

`PreparedApplication::new` validates V8/V9/V10 before serving. V10 adds persistent authority
store identity, coordinator import fence, root bindings, workspace claims, prepared start
receipts, attempts, verification and fresh-session records. Offline `v10::prepare` exists;
the current `wyss-control` CLI exposes V9-copy preparation, not a V10 preparation command.
An install/update/GUI open must not migrate a live store implicitly.

There is no automatic fallback to unguarded starts, P1, Native or a different workspace.
Production parallelism is one, independently of plan bounds and Native delegation limits.
Guard availability does not admit Claude, a new model/effort protocol or protected approval.

## Finite plan and payload

`OrchestrationPlanV1` validates a finite acyclic graph: at most 16 steps, 48 dependencies,
8 KiB prompt per step, 32 KiB plan text, one-hour run, thirty-minute step and at most three
explicitly bounded attempts per step. A retry allowance never authorizes replaying an
uncertain external effect. Typed provider/effect lanes and current admission determine
eligibility; unsupported Native/planner lanes stay blocked.

Review binds the plan revision/digest and workspace observation. Prompt and objective
remain volatile; the journal stores identifiers, closed states, digests and receipts,
not prompt/output, commands, private paths or secrets. An effectful restart cannot recover
a payload or approval from metadata alone. A real provider journey remains separate from
fixture evidence or parsing a valid plan.

## Guarded dispatch and settlement

1. Reconcile the exact coordinator import snapshot with F8 before claiming readiness.
2. Observe a real workspace root and pin its descriptor/identity. Persistent fingerprints
   are not a path that can be reopened as equivalent authority after restart.
3. Acquire a generation/revision-bound claim; detect conflicts/overlap. Bind plan, run,
   step, attempt, intent, provider/profile, prompt digest, lease and current authority epoch.
4. Prepare a short-lived one-use receipt, then recheck the exact tuple at commit. Mark
   the durable attempt before sending; possible dispatch retains its claim and cannot
   become a fresh attempt on a lost reply.
5. Provider completion enters verification. Prepare/commit verification against the current
   reviewed workspace digest and attempt. Release requires a matching settlement and
   coordinator revision; only independently proven never-used claims use release-unused.

The closed method/field allowlist is [guard_protocol.rs](../../src-tauri/src/f8/guard_protocol.rs),
not an open `orchestration/*` permission. Unknown fields/methods and cross-store/epoch/
generation/claim substitution refuse. Ordinary P2's existing envelope/methods remain separate.
Prepared operations do not acquire authority from a UI label or from planner-produced JSON.

## Handoff and uncertainty

The reviewed Native brief and managed/coordinator handoff are different paths.
The envelope is bounded to 16 KiB with a 2 KiB objective; destination preparation verifies
a fresh receiving session and exact reviewed payload/workspace. Freshness is an authority
fact, not inferred from an empty UI transcript or a provider thread name. Unsupported
receiver/source freshness blocks rather than silently changing mode.

Restart creates a new authority epoch, invalidates prepared receipts and loses live root
descriptors. Nonterminal persisted claims are quarantined for reconciliation; imported
claims are identity-unproven until validated. Coordinator recovery retains uncertainty,
pauses/blocks affected work and does not resend prompts. A terminal run state or a provider
exit alone is insufficient proof that its workspace is settled.

## Desktop permission gap and closure

`orchestration_call` forwards a closed `DaemonRequest`; it is not raw arbitrary provider
RPC. It is registered and declared in the Tauri manifest; its permission is granted to
`main` in the capability configuration. Do not claim runtime enforcement from configuration
or a mocked view test. [ENG-01](../ROADMAP.md#eng-01) tracks the required positive and
negative runtime tests.

Before production coordination, qualify the V10 adoption/rollback and immutable import,
real source/root changes, same-root/ancestor/worktree conflicts, lost replies at prepare/
commit/verification, crash at every dispatch boundary, stale/forged freshness, unknown effects,
cancel/revoke and claimant retention/release. Existing tests in `f8/v10.rs`,
`f8/v10_sr_tests.rs`, `f8/workspace_guard.rs` and `orchestration/scheduler.rs` are engineering
oracles; real acceptance, independent review and closed release gates remain explicit.
