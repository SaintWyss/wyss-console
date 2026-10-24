# M5 track (a) — qualified concurrency above one: what is built, what is measured, what is still closed

Status: engineering contract and evidence for [M5](../product/roadmap.md#m5--boundaries-b1-feasibility-and-concurrency).
**Production capacity stays 1.** Nothing here changes `max_active_runtimes` of a shipped build, enables a restricted-worker
privilege, restarts an installed service or runs a paid provider. The fixtures run a _test build_ at 2 and 4 against the
repository's fixture child; the production contract (`docs/contracts/f8/resources.json`) is read by a test that fails if a
non-test build says anything but 1 (`production_capacity_is_one_and_only_a_test_build_can_say_otherwise`).

## 1. What the fixtures establish (`f8::acceptance::concurrency`, real F8 harness, one real fixture child per runtime)

| Property (the M5 brief)                                                               | Test                                                                                     | Result |
| ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ------ |
| per-session independence, per-generation fencing, no identity of one reaches another  | `n_runtimes_run_side_by_side_and_no_identity_of_one_reaches_another` (N = 2, 4)          | PASS   |
| accurate capacity accounting; refusal before any allocation; no hidden queue          | `over_n_is_refused_before_any_allocation_and_nothing_waits_in_a_queue`                   | PASS   |
| a stalled provider must not block sibling status, cancel, decline, revoke or stop     | `a_stalled_provider_holds_no_siblings_status_cancel_decline_revoke_or_stop` (SIGSTOP)    | PASS   |
| a crashed worker keeps its slot until reaped; others unchanged                        | `a_crash_in_one_runtime_changes_nothing_of_the_others_and_retains_its_slot_until_reaped` | PASS   |
| fair admission: a flood from some runtimes cannot starve another's control operations | `a_flood_from_some_runtimes_does_not_starve_the_control_operations_of_another`           | PASS   |
| recovery: cycles of start/stop leave no child, descriptor or slot                     | `start_stop_cycles_at_n_leave_no_child_no_descriptor_and_no_slot_behind`                 | PASS   |
| a provider that never answers a cancel cannot hold the only slot (capacity 1)         | `a_provider_that_never_answers_a_cancel_cannot_hold_the_only_slot`                       | PASS   |
| production capacity is 1                                                              | `production_capacity_is_one_and_only_a_test_build_can_say_otherwise`                     | PASS   |

### The defect these found (fixed here)

After `turn/cancel`, a provider that stops answering (the test stops it with SIGSTOP) kept its turn `Running`, its runtime
`Ready` and its session unstoppable (`TurnActive`) until the 1800 s turn wall clock. At capacity 1 that one slot is the whole
product, so a wedged provider after a cancel locked out every other session for half an hour. **Fix:** the engine records when
the interrupt was dispatched and ends the turn as `budget_exceeded` after `interrupt_answer_ms` (10 000 ms, a new key of
`docs/contracts/f8/resources.json`, enforced in `engine::poll` and classified `Enforced` by `budget.rs`); cleared wherever the turn
ends. The authority calls `engine.interrupt_dispatched(…)` after the interrupt write succeeds. The regression test above fails
without the change.

## 2. Measurements (release build, this machine, 8 runs each, fixture child, idle runtimes)

Raw samples: [evidence](../acceptance/evidence/m5-2026-10-24/concurrency.jsonl). No budget was proposed; nothing below is a pass or a fail.

| Runtimes | all Ready after start (p50 / p95) | sum of children RSS | one turn cancelled, others running (p50 / p95) | application thread CPU, idle (p50 / p95) |
| -------- | --------------------------------- | ------------------- | ---------------------------------------------- | ---------------------------------------- |
| 1        | 208 / 213 ms                      | 14.4 MiB            | 12.5 / 12.9 ms                                 | 0.5 / 1.0 % of a core                    |
| 2        | 358 / 361 ms                      | 28.9 MiB            | 12.5 / 12.9 ms                                 | 0.5 / 1.0 %                              |
| 4        | 687 / 691 ms                      | 57.7 MiB            | 12.7 / 13.1 ms                                 | 0.5 / 1.0 %                              |

Start-up grows linearly (about 170 ms per runtime, the runtimes are started one after another), memory is the child's own
(14 MiB for the fixture, **not** a real provider's), a cancel does not slow down with more siblings, and an idle application
does not cost more with more runtimes. A real provider's memory, a real model turn, descriptors per runtime and fairness under
real output are `PROVIDER_EVIDENCE_REQUIRED`.

## 3. Provider shared state and credential refresh (why the cap stays)

Source reviews already recorded: [Codex 0.159.0](../research/f8-codex-shared-state-2026-10-20.md) (`auth.json` rewritten in
place with in-process semaphores only; a spent refresh token is terminal) and the Claude route of [M3](m3-managed-parity.md)
(the credential never enters the worker; the loopback API tap holds it). The M5 addition is a **synthetic model of the failure
class** under the strictest rotation rule, `benchmarks/m5_refresh_race.py` (20 rounds each, a barrier so the stale reads
overlap; a local stand-in token service; no network, no real credential):

| Protocol                                                  | N = 2: rounds with a reuse failure | N = 4: rounds with a reuse failure |
| --------------------------------------------------------- | ---------------------------------- | ---------------------------------- |
| one shared file, no cross-process lock (Codex's shape)    | 20 / 20                            | 20 / 20 (60 reuses)                |
| one shared file, exclusive `flock` + re-check under lock  | 0 / 20                             | 0 / 20                             |
| a private copy per runtime (the "private home" idea)      | 20 / 20                            | 20 / 20 (60 reuses)                |
| one refresher (a broker); workers hold access tokens only | 0 / 20                             | 0 / 20                             |

Reading: concurrent refreshers of one rotating lineage are unsafe without coordination, **copying the credential into private
homes does not help and is forbidden anyway**, and the two protocols that work (a cross-process lock held across
load–exchange–save, or a single refresher) need either the provider's cooperation or the credential to stay with Wyss, which is
the M3 Claude route. Whether the pinned Codex build takes such a lock, or tolerates reuse, is **`PROVIDER_EVIDENCE_REQUIRED`**:
it needs the provider owner's protocol or an owner-authorized controlled experiment, neither of which was done. The model is not
evidence about any real provider.

## 4. What an increase of capacity still needs (not done)

1. Provider evidence for the credential protocol above, per provider and pinned build.
2. A real-provider run at 2 and 4 on the reference PC with resource, readiness, cancellation and fairness measured (the fixtures
   give the harness; they are not the numbers).
3. The owner's approval of a number, then the production contract changed in its own reviewed commit, with the drain/cancel plan
   for owned work on rollback (capacity returns to 1 for new admissions).

None of these is authorized here. M5's other open items named in the roadmap (stdout scratch mismatch, stale catalogue
classification, attachment/retention enforcement proof) are unchanged by this branch.
