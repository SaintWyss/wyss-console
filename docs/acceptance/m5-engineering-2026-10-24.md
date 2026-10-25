# M5 engineering acceptance record — 2026-10-24

**Branch:** `feat/m5-boundaries-concurrency`, stacked on `feat/m4-terminal-fidelity` (M4 PR, itself on PR #24) ·
**Design:** [ADR 0058](../adr/0058-b1-candidate-launcher-and-approval-boundary.md) (proposed) ·
**Contracts:** [concurrency](../contracts/m5-concurrency.md), [boundaries](../contracts/m5-boundaries.md)

**Result:** track (a) `ENGINEERING_TESTS_PASSED` for the fixtures and the defect they found, capacity stays 1,
`PROVIDER_EVIDENCE_REQUIRED` for anything above it · track (b) `IMPLEMENTED_BUT_UNQUALIFIED` (a constrained candidate with
observed positive and negative evidence; **B1 and FX-H01 stay NOT_PROVEN**) · the one independent review of this branch
returned **FAIL** with ten findings, all addressed here, **the fixes not re-reviewed** · `SECURITY_REVIEW_PENDING` ·
`OWNER_APPROVAL_REQUIRED` (OD02, ADR 0058, any capacity number) · `RELEASE_BLOCKED`.
Nothing here installs, restarts or migrates anything; the installed Console, host, authorities, credentials and stores were not
touched; no provider was run; the `kwin_wayland --virtual` instances that are not this task's were not touched.

## Track (a) — concurrency

| Brief item                                                                        | Evidence                                                                                                     | Result                                                         |
| --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------- |
| keep production capacity one                                                      | `production_capacity_is_one_and_only_a_test_build_can_say_otherwise`; `resources.json` unchanged for the cap | `PASS`                                                         |
| architecture/fixtures for safe higher concurrency; N = 2 and 4                    | eight tests of `f8::acceptance::concurrency` over the real F8 harness with one fixture child per runtime     | `PASS`                                                         |
| per-session independence, generation fencing, lease/approval ownership            | identity of one never reaches another; crash/stop/revoke of one leaves the others                            | `PASS`                                                         |
| accurate accounting, refusal before allocation, no hidden queue                   | over-N refused with nothing allocated or queued                                                              | `PASS`                                                         |
| reservations until owned children are reaped                                      | a crashed runtime keeps its slot until reaped                                                                | `PASS`                                                         |
| stalled provider must not block sibling status/cancel/revoke/stop                 | SIGSTOP on one provider; siblings fully operable                                                             | `PASS`                                                         |
| recovery from crashed or stalled workers                                          | crash test; **a stalled provider after a cancel held the session for 30 minutes: found and fixed** (below)   | `PASS`                                                         |
| fair admission / bounded queues                                                   | a flood from some runtimes does not starve another's control operations                                      | `PASS` for the fixture's lanes; real-provider output `NOT_RUN` |
| provider shared state and credential refresh                                      | Codex source review (existing) + synthetic rotation model (`benchmarks/m5_refresh_race.py`)                  | `PROVIDER_EVIDENCE_REQUIRED`                                   |
| measurements: memory, CPU, readiness, cancellation, fairness, growth              | release table in the concurrency contract; raw samples in `evidence/m5-2026-10-24/`                          | measured, no budget set                                        |
| tests at 2/4 needing special authorization stay disabled; live capacity unchanged | the 2/4 runs are test-build only; no shipped build can say anything but 1                                    | `PASS`                                                         |

### The defect found, and the review of its fix

A provider that stopped answering after `turn/cancel` kept its turn `Running` and its session unstoppable until the 1800 s wall clock
(capacity one: nothing else could start). Fixed with `interrupt_answer_ms` (10 000 ms, a new key of the resource contract, enforced in
`engine::poll`). The independent review found the first form of the fix could fault a provider that had answered in time (the deadline
was judged before the provider's already-sent frames were looked at). **Now** the deadline is judged after the pump and is deferred while
frames from that provider are queued, for at most as long again (`interrupt_unanswered`, truth-table test).

## Track (b) — boundaries

| Brief item                                                                         | Evidence                                                                                                                                                           | Result                                                                                          |
| ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------- |
| lanes U/B1/M/P/A kept distinct                                                     | `b1::boundary::Lane`, contract § intro; B1 not reachable from any product surface (source-scan regression check)                                                   | `PASS` as stated                                                                                |
| constrained B1 candidate with safe primitives                                      | rootless `bwrap` plan, pre-exec shim (fds, rlimits, no-new-privs, seccomp, Landlock ≥ ABI 6), workspace from an admitted snapshot, exporter, quarantine            | `IMPLEMENTED_BUT_UNQUALIFIED`                                                                   |
| user/mount/pid/net namespaces, Landlock, bubblewrap, FD closure                    | 22 process tests with unconfined controls where the attack can be run (contract § 4)                                                                               | `PASS` for what is covered                                                                      |
| network mediation, credentials, IPC exposure                                       | no network; unix-only sockets; synthetic token refreshed in place; inference through a broker that holds the credential                                            | `PASS` (synthetic)                                                                              |
| host/authority socket access, same-UID limits                                      | a same-account client connects to a private authority (U); the worker cannot reach it                                                                              | `PASS`                                                                                          |
| Docker / rootful delegation                                                        | **fact: this account is in the `docker` group** (no daemon socket present now); no path to any container socket from B1; B1 does not change what that group can do | `BLOCKED_ADMIN_DELEGATION` (unchanged)                                                          |
| workspace / worktree poisoning, hardlink to a forbidden signer (FX-H01)            | selected hardlinked file becomes a distinct inode; unselected never enters; hostile links/FIFOs/sockets refused at export                                          | `PASS` for the synthetic fixture; FX-H01 still OPEN (no trusted capture from a real repository) |
| human approval authenticity                                                        | contract + facts: today's channel is weak by its own rules; stronger mode, Native automated steps, production B1 launch, planner effects stay closed               | gates closed                                                                                    |
| root / admin threat                                                                | out of B1's boundary; stated, not claimed                                                                                                                          | `OWNER_APPROVAL_REQUIRED` (OD02)                                                                |
| useful positive workflow                                                           | edit, create, move, delete, `chmod`, run a script, `git init/add/commit` inside the sandbox → exact add/modify/delete record → quarantine                          | `PASS`                                                                                          |
| adversarial positive + negative tests, disposable processes, synthetic credentials | all of the above; every secret is a synthetic string; every process a direct child ended through its handle                                                        | `PASS`                                                                                          |
| environments that cannot run a security test                                       | `NOT_RUN`, recorded in a file, failing under `WYSS_B1_REQUIRE_RUN=1`                                                                                               | in place                                                                                        |

### What was not done (track b)

CG-14 numeric budgets and quotas (needs a size-limited tmpfs: privileged); the `PENDING` publication (`B1QuarantineReceiptV1`);
owner selection and a trusted capture from a real repository; real provider authentication or egress (CG-15/16 are synthetic);
a reviewed implementation beyond the one review below; packaging of any B1 binary (none is packaged by design); a stronger approval
principal (needs privileges, not asked for).

## Independent review

One independent review (Codex, read-only, over `04e862e`) was requested and run. It could not run any test in its sandbox
(`NOT_RUN` there: cargo, probes, x32, races, packaging); findings came from reading. **Verdict: FAIL** as an engineering candidate;
F01–F08 blocking. All ten were confirmed and addressed; **the fixes were not reviewed again**, so nothing below is an independent verdict.

| #   | Sev    | Finding                                                                                                                                    | Fix and evidence                                                                                                                                                                                                                              |
| --- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F01 | High   | `quarantine` wrote every blob under a caller-chosen name: an absolute or `..` key escaped the store                                        | blobs keyed by `Sha256Hex`; `verify()` demands exactly the referenced set, once each, digest and size, **before any write**; test with extra/duplicate/corrupt/missing blobs                                                                  |
| F02 | High   | x32 call numbers passed the seccomp filter (every comparison is native)                                                                    | the filter kills any call with the x32 bit; probe + test; **mutation: with the guard broken an x32 `socket(AF_INET)` succeeded inside the sandbox**                                                                                           |
| F03 | High   | verified directories were re-resolved by path when `bwrap` mounted them (rename of an ancestor)                                            | checks on the open descriptor and on the kernel's path for it; `--bind-fd`; plan bound to the workspace's device+inode (`WorkspaceChanged`); `materialize` via the parent's descriptor                                                        |
| F04 | Medium | the interrupt deadline could fault a provider that had answered in time                                                                    | judged after the pump, deferred while its frames are queued, hard backstop; `interrupt_unanswered` truth-table test                                                                                                                           |
| F05 | Medium | the quarantine followed links in its store and ancestors; the record could be left half-written                                            | store pinned and private, `mkdirat`/`openat` no-follow, apart from workspace/source, record by rename; tests with a link store, a link task directory, a store inside the workspace                                                           |
| F06 | Medium | the exporter enumerated without a bound on nodes or names                                                                                  | every node and name byte counted while a directory is listed; 17 000-directory test refused quickly                                                                                                                                           |
| F07 | Medium | Landlock ABI 6 was documented as required but the launcher defaulted to any                                                                | `MIN_LANDLOCK_ABI = 6` enforced in the shim whatever the profile says; plan default; wording on handled rights corrected                                                                                                                      |
| F08 | Medium | tests could go green without running the attacks (skip on any failure, `\|\| true`, an alias test that tested nothing, claims of controls) | machine capability checked with a bare `bwrap`, a candidate failure now fails; `NOT_RUN` recorded and failing under `WYSS_B1_REQUIRE_RUN=1`; controls added; alias test now refuses unsafe links; docs list exactly which tests have controls |
| F09 | Low    | the gates and `Untrusted<T>` are declarations, not an approval boundary                                                                    | documentation and test names say so; the module states it is not an enforcement point                                                                                                                                                         |
| F10 | Low    | the package marker is a constant that need not survive linking                                                                             | the guard is the feature gate and a test that no packager path enables it (`tests/package.test.ts`); marker kept as defence in depth                                                                                                          |

## Test results (this branch's head)

| Area                     | Command                                                                             | Result                                                                                                                          |
| ------------------------ | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Rust format, clippy      | `cargo fmt --check`; `cargo clippy --all-targets --features b1-fixture -D warnings` | PASS                                                                                                                            |
| Rust tests               | `WYSS_B1_REQUIRE_RUN=1 cargo test --features b1-fixture`                            | PASS: 1,442 lib, 535 bin, 22 B1 process (`WYSS_B1_REQUIRE_RUN=1`), plus the F8, control, approvals, lifecycle and screen suites |
| B1 process tests         | `WYSS_B1_REQUIRE_RUN=1 cargo test --features b1-fixture --test b1_process`          | PASS: 22 of 22, none skipped                                                                                                    |
| Frontend / package guard | `npx vitest run`                                                                    | PASS: 1,055 tests (65 files), format, lint, typecheck, `scan:repo`                                                              |

**Environment:** kernel 6.18.55, Landlock ABI 7, bubblewrap 0.13.1, `kernel.unprivileged_userns_clone=1`, Yama `ptrace_scope=1`; the
account is in the `docker` group.

## Owner decisions pending

ADR 0058 (the candidate, the ABI 6 floor, the deny-list); OD02 (whether a protected approval principal, which needs privileges, is worth
it; this machine's account can already obtain root through the container group); whether B1 may ever launch a real provider
(CG-15/16 with provider evidence); a capacity number above one (needs provider evidence first).

## Preservation and rollback

Nothing installed, restarted or migrated; no credential, store or service touched; every process of the runs a direct child ended through
its handle. **Rollback:** delete `src/b1/`, the two binaries and the feature (nothing references them); set `interrupt_answer_ms` back by
reverting its commit (the cap and every other contract value are untouched).
