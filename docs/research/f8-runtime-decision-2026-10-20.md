# F8 candidate: evidence, independent review and remaining release decisions

2026-10-20. Baseline `fb4a72c`. **Reviewed candidate; NOT_PROVEN; not implementation-ready yet. Not owner-accepted.**
No production Managed code, schema, permission, runtime or provider configuration was changed.
The candidate remains one runtime per managed session, bounded by the authority.

## Read-only observations

The normal desktop, HOME, XDG roots, session bus and wallet were retained. Authority PID 1167285
owns Codex 1167441, version 0.159.0, ready/inference, revision 9. The authority reports seven cached
models and zero thread/start, thread/resume and turn/start writes. Two managed session records
exist. A separate host 1607394 holds one running non-native terminal; GUI 1607295 remains alive.
PIDs here are historical evidence, never future signal targets.

Thirty seconds of `/proc` observation, 31 samples, compared process start ticks at every sample.
No process was started/stopped for this measurement. Background cargo/frontend checks were active;
load at the end was 9.68/4.20/2.15, available memory 8,807,016 KiB, used swap 13,204,408 KiB.

| Existing root       | Median PSS KiB | Median RSS KiB | Median SwapPss KiB | CPU, one core | Threads |
| ------------------- | -------------: | -------------: | -----------------: | ------------: | ------: |
| Authority           |          2,581 |          4,612 |              3,016 |        0.000% |       4 |
| Owned Codex         |          2,855 |          5,148 |             23,812 |        0.033% |      16 |
| Older terminal host |          1,070 |          5,576 |              8,144 |        0.664% |       3 |

These are resident/swapped root observations, not a total peak-memory requirement or qualified idle
benchmark. They omit descendants, GPU, warm-start transients and provider work. Do not extrapolate a
capacity from resident pages while much of the process is swapped out. Startup latency and counts
2/4 are **NOT_RUN**: launching discovery/App Servers could touch shared provider caches or auth while
the existing authority runs; exact-version concurrent state compatibility is not established.
Candidate staging ceiling stays 1. No paid model experiment was run.

Local raw evidence: `.artifacts/acceptance/engineering-2026-10-20/{live-before,resource-live}.json`.

## Provider concurrency/state research

Using OpenAI Docs, the current official documentation was fetched on 2026-10-20:

- [App Server](https://developers.openai.com/codex/app-server) documents thread creation/resumption
  and provider thread identity. It does not establish that two 0.159.0 processes can safely mutate
  the same stored thread. Authority admission must enforce one owning session/launch per thread.
- [Authentication](https://developers.openai.com/codex/auth) documents automatic token refresh
  and file/keyring credential stores. Concurrent refresh safety for the pinned binary remains
  **NOT_PROVEN**. No credential contents or keyring secrets were inspected.
- [Configuration reference](https://developers.openai.com/codex/config-reference) identifies
  SQLite runtime state and configurable log locations. These are potential shared-state surfaces,
  not evidence that setting a different path preserves Managed security.

Current docs can differ from pinned 0.159.0. Production restrictions in ADRs 0015/0018 remain the
version-specific contract. Read-only source inspection in this repository confirms fixed catalogue
staging/final paths in `adapters/codex/probe/supervised.rs` and shared `state/home` in
`probe/isolation.rs`. A per-session worker registry alone cannot provide separation.

Before concurrency is enabled, inventory exact-version writes to model cache, rollouts, SQLite/WAL,
logs and auth using source/metadata, classify each as private, immutable-shared or serialized-shared,
and prove the relevant locking/refresh protocol. Never copy auth into another HOME or silently
disable mandatory user controls. Do not test refresh, inference or same-thread concurrent writes
against the owner's live state to fill this gap.

## Independent security review and dispositions

A separate reviewer examined the original candidate and actual Managed sources read-only, without
provider execution or edits. This is a design review, not an implemented-runtime security verdict.

| ID  | Severity    | Finding and source                                                                                     | Candidate correction                                                                                                   |
| --- | ----------- | ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------- |
| R1  | High        | Migration claims V6→V7, but `control/schema.rs` already validates V7; pending-approval index is global | V7→V8; change both partial indexes; freeze exact SQL and preserve transitions/history                                  |
| R2  | High        | Fixed catalogue/staging paths (`probe/supervised.rs`), shared local home (`probe/isolation.rs`)        | Private per-launch state and immutable catalogue; shared provider compatibility still blocks concurrency               |
| R3  | High        | Implicit legacy session can retarget after A disappears and B becomes the only runtime                 | Protocol 2 with explicit session/launch; reject v1 hello; coordinated owner-approved update                            |
| R4  | High        | Global `Turns.active/accepting`, `Conversations.pending` and server routing                            | Typed identity-scoped registry for every admission, binding, stream, deadline, event and approval                      |
| R5  | Medium/High | Parent RSS/descendants are not effective tree limits; synchronous `worker.stop()` stalls core          | Distinguish observation/containment; reserve full lifecycle capacity; fair bounded scheduling and asynchronous cleanup |
| R6  | Medium      | Candidate misdescribes existing autostart (`server.rs` startup reconcile)                              | Explicit-only recovery identified as a proposed behaviour change; failed attempts consume generation                   |
| R7  | Medium      | Snapshot/previous binary alone do not establish DB/WAL-consistent rollback                             | Verified consistent backup, exclusive writer, matching binaries, owner downtime and acknowledged data-loss boundary    |

All seven findings resulted in normative corrections in ADR 0051 §9 and the linked test plan.
No finding was treated as permission to modify production authority guarantees. An independent second pass confirmed the seven design dispositions and requested five wording
corrections (identity provenance, pending versus dispatched approvals, unqualified memory sample,
evidence versus tests, and implementation readiness). All are incorporated. Implementation review
is still required after code exists.

## Concrete implementation sequence and residual risks

1. Freeze protocol-2 request/response shapes, RuntimeKey/launch identity, V8 exact SQL and numeric
   queue/storage/latency ceilings as reviewable contracts. Keep staging capacity 1.
2. Implement/test migration on ordinary seeded unit fixtures, typed registry and identity routing;
   then per-launch catalogue/local-tools state and admission/approval scoping. No installed rollout.
3. Implement asynchronous owned cleanup and fair control scheduling; adversarial tests in
   [the specification](../spec/f8-runtime-acceptance.md) must fail the single-runtime assumptions.
4. Independently review the implementation against ADRs 0012–0018 and SG-12; verify provider shared
   state compatibility before owner-authorized real concurrency measurements at 1/2/4.
5. Choose a measured release capacity, complete real provider acceptance, then request Santiago's
   acceptance of the exact candidate and an explicit migration/rollback maintenance window.

Residual risks: provider state/auth races; unproven descendant containment; starvation or aggregate
memory pressure; late effects after cancellation; schema/receipt migration mistakes; incompatible
older clients; post-snapshot data loss on rollback. Each has adversarial tests or evidence requirements specified; none is
discharged by separate processes or a green single-runtime regression suite.

**Owner-only decisions:** accepting ADR 0051 and its protocol/restart/lease invalidation changes;
interrupting installed authority/host sessions; installed migration/rollback and its loss boundary;
paid-quota/provider-auth experiments; changing SG-12 or any execution guarantee. No blanket request
for planning authorization is necessary to continue the eligible preparation above.
