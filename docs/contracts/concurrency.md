# Managed concurrency and shared-state contract

Production F8 capacity remains **one**, enforced by [resources](f8/resources.json) and its
tests. Native delegation's default two/maximum eight policy is separate. Test-only builds
at two/four and fixture benchmarks do not approve a production increase.

## Invariants

Isolate session/runtime/generation identities. Refuse over-capacity before allocation,
without a hidden queue. A stalled/crashed provider must not block sibling status, cancel,
decline, revoke or stop. Keep a crashed worker's slot until reap; start/stop cycles release
children, descriptors and slots. Flooding output must not starve control work.

After an interrupt is successfully dispatched, `engine::poll` bounds a provider that never
answers using `interrupt_answer_ms` (10,000 ms in the executable contract); clear the
deadline when its turn ends. This source bound is not a measured hard OS scheduling deadline.
Qualify fair stream/attachment allocation and all resource gaps in [STATUS](../STATUS.md).

## Credentials and provider-shared state

Multiple refreshers of one rotating credential lineage can race even in separate processes.
Private credential copies neither coordinate refresh nor meet the ownership policy; do not
copy or re-home account credentials to solve this. The recorded pinned Codex source review
found in-process synchronization and in-place auth writes; it does not prove a safe
cross-process protocol or the real service's token-reuse behavior.

`benchmarks/m5_refresh_race.py` models the failure class with synthetic data. Cross-process
locking across load/exchange/save or a single broker refresher can coordinate the model;
real compatibility requires provider cooperation or its own accepted broker route. Neither
synthetic success nor Claude's proposed broker qualifies shared Codex refresh.

## Before increasing capacity

Obtain provider/build-specific authentication/refresh evidence and real reference-PC runs
at the proposed capacity: readiness, output/backpressure/fairness, generation isolation,
cancel/reap/recovery, descriptors and full owned-tree load. Agree budgets, resolve current
scratch/timing/classifier/stream/attachment/memory gaps and explicitly accept a number.
Change the production contract in its own reviewed change, with a drain/cancel rollback
plan for owned work; reverting capacity to one applies to new admissions, not blind kills.
