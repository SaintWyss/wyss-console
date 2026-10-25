# M1–M5 production candidate — release qualification

> Historical package qualification for `48396c4`. Later V9, credential-agent and GUI observer changes supersede its implementation status; see PR #27 and the current rollout runbook. The package hashes below do not qualify the current source.

Date: 2026-10-24. Owner: Santiago. **Deployment verdict: NO-GO.**

This is a release branch carrying the complete stacked history, not an acceptance of the proposed
ADRs. Engineering checks, independent security qualification and deployment permission are separate.
No M6 work, installation, merge, service change, lingering change or live database migration occurred.

## Verified baseline

GitHub and local worktrees agreed at the start of execution:

| PR  | State          | Exact HEAD                                 | Base                        |
| --- | -------------- | ------------------------------------------ | --------------------------- |
| #24 | Open, unmerged | `6c944dd2474da1c2e0805d3123b50b140638b310` | `main`                      |
| #25 | Open, unmerged | `0a8fb60744eaccafed0cfe0c98d551b829e4014c` | `integration/m2-m3`         |
| #26 | Open, unmerged | `42c73aa43b9195f4db20d4f3dbeac260df8b97c3` | `feat/m4-terminal-fidelity` |

Both ancestry checks passed. `release/m1-m5-production-candidate` was created directly from the full
M5 SHA in a separate `wyss-console-release` worktree. The principal checkout's pre-existing untracked
`src-tauri/src/contracts/b1.rs` was left alone. Source PR branches were not edited or rewritten.

## Release corrections

- `01f48fe`: **confirmed security defect** in the terminal helper. Its seccomp filter checked the
  x86_64 architecture and native syscall numbers but allowed x32 numbers. An independent process
  reproduction created an IPv4 socket under the actual filter. The new regression failed before
  the fix and passed after it: x32 is killed with SIGSYS before native comparisons. All 16 screen
  process tests passed. B1's separate x32 guard was already present and was preserved.
- `2e00b8c`: acceptance tools signal pinned pidfds after proving ownership, rather than checking and
  later signalling a bare PID. The process launcher tolerates the bounded posix_spawn/exec visibility
  interval. Private GUI Secret Service startup waits for registration without auto-activating a
  competing daemon. The 16 ownership tests pass; the failed GUI scenario passed on retry.
- `55538cc`: disabled desktop-side `SealedCredentialSource` candidate. It requests native consent
  for one selected Anthropic record/profile, revalidates the exact metadata after consent, opens
  only the sealed record and consumes the grant once. Replacement, deletion, refusal and wrong
  provider are refused. There is no new Tauri command or credential-returning DTO. The authority
  still links neither the configuration engine nor the credential vault and still uses
  `NoCredential`. **Cross-process delivery is not implemented or qualified.**
- `3d2da65`: disposable on-disk V9 rehearsal, actual child-process crash during table rebuild,
  SQLite recovery, old-validator refusal and V8 snapshot restoration. Added an executable check
  exposing the existing Claude kind incompatibility. **This does not implement full V9.**

The first production package attempt was correctly rejected: commits moved HEAD between its two
build stages, changing the embedded build revision and the authority digest. The packager now
requires a clean tree, freezes `WYSS_BUILD_REVISION` for both stages, verifies the source stayed
unchanged and records `source_revision` in the package manifest. A disposable Git regression
checks dirty/staged sources and a moving HEAD. Rejected attempts are retained as evidence.

## Independent security review

The reviewer of the prior authors' corrections was this release session. Reviewed corrections:
M3 `5703054`, M4 `7dd8ff7`, M5 `42c73aa`, and their final integration. New release changes above are
not represented as independently reviewed by a second reviewer. No security gate was opened.

PASS below is limited to the stated property and executed evidence, not a blanket product attestation.
An unavailable property is FAIL for production qualification; its evidence may be NOT_PROVEN.

| Boundary                                                           | Verdict                                            | Executed evidence and qualification limit                                                                                                                                                                                          |
| ------------------------------------------------------------------ | -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Broker origin, request policy, framing and verified TLS            | PASS for synthetic/local TLS tests                 | 77 Python bridge/tap/broker/guard tests; fixed-origin and no-plaintext-key paths. No live Anthropic authentication qualification.                                                                                                  |
| Claude bridge/CLI network namespace and empty environment          | PASS for tested process boundary                   | 16 pinned real SDK/CLI tests against mock provider, including environment, egress, cancellation and whole-tree reap; sandbox tests in full Rust run. No kernel-escape or hostile same-UID guarantee.                               |
| Approved file effects and TOCTOU                                   | FAIL for an unconditional race-free claim          | Descriptor-based traversal, bounded nonblocking reads, held replacement descriptor, hash/identity rechecks and their regressions pass. The last-check/rename race remains stated in `exec.rs`; Claude Local Tools remains blocked. |
| Helper executable trust and lifecycle                              | PASS for covered corrections                       | Open-descriptor execution, permission/path-swap regressions, helper-flood shutdown regression and process tests executed. Same-user modification is outside the declared boundary.                                                 |
| Terminal helper seccomp/x32                                        | FAIL at baseline; regression PASS after correction | Real x32 socket escaped before `01f48fe`; corrected helper dies with SIGSYS. Final correction still needs another reviewer before an independent final sign-off.                                                                   |
| Screen IPC, resource/repaint bounds and control stripping          | PASS for covered limits                            | Oversize/malformed frame, overflow, hostile output, 512×512 repaint/resource and C1/OSC regressions; screen corpus and helper suites executed.                                                                                     |
| Attach/replay, stream offsets and resize ordering                  | PASS for exercised flows                           | Rust/frontend regressions plus isolated desktop: less alternate buffer, window reopen, helper replacement, restart-budget exhaustion and working PTY fallback.                                                                     |
| B1 quarantine and namespace boundaries                             | PASS for synthetic candidate properties            | 22 process tests with `WYSS_B1_REQUIRE_RUN=1`, plus validation/quarantine tests. No skipped attack counted as PASS. B1 is not shipped or reachable from production.                                                                |
| B1 x32, ABI floor and pinned mounts                                | PASS for tested candidate                          | Real process probes and unconfined controls; minimum ABI 6 enforced; directory FD mounting and workspace identity regressions.                                                                                                     |
| F8 concurrency/cancellation and admission                          | PASS for fixtures                                  | Full F8 acceptance suite and capacity-2/4 fixtures; cancel deadline runs after pump with bounded queued-frame deferral. Production capacity stays one. No real-provider concurrency evidence.                                      |
| Human approval authenticity against hostile same-account processes | FAIL                                               | Existing same-UID channel is not a protected approval principal. OD02 remains pending; stronger-mode/B1/planner effect gates stay closed.                                                                                          |
| Protected store → production authority-owned broker                | FAIL / NOT_PROVEN end-to-end                       | Desktop candidate unit tests and existing broker tests pass separately. No authenticated cross-process handoff exists; no real credential was loaded.                                                                              |
| V9 multiprovider store and replay                                  | FAIL                                               | Existing validator calls the V8 view and refuses any non-Codex rebuilt row. Rehearsal uses `claude`, while typed wire kind is `claude_code`. Full writers/replay/admission remain incomplete.                                      |

M3 F2–F7 corrections were inspected together with raw-header/deadline/closed-event tests and async
history regressions. M4 F01–F09 and M5 F01–F10 regressions were executed in the combined Rust/frontend
suites. This review does not erase the earlier FAIL records or change their historical wording.

## Integrated acceptance matrix

| Stage                                                        | Result                                   | Evidence / limits                                                                                                                                                                                          |
| ------------------------------------------------------------ | ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Rust core, commands, persistence, supervision, ACL contracts | PASS for automated suites                | Full all-feature run: 2,191 passed, zero failed, 48 ignored. Includes 1,442 library and 537 desktop-bin tests, process/contract suites and doctests. Ignored tests are not included in PASS counts.        |
| Clippy                                                       | PASS                                     | All targets, b1/f8/m3 fixture features, warnings denied; repeated after the V9 test changes.                                                                                                               |
| Frontend, formatting, types and lint                         | PASS                                     | 1,055 frontend tests; Svelte/TypeScript checks; prettier/eslint; complete npm check.                                                                                                                       |
| Python bridge and process harness                            | PASS                                     | 77 broker/bridge/tap/guard tests; 16 ownership tests.                                                                                                                                                      |
| Native Claude/Codex UI and M2 configuration                  | PASS with fixture CLIs                   | 13 integration desktop scenarios: 12/13 initial, 1/1 corrected retry, all final scenarios with no leftovers. Includes configuration edit/undo/import/export and Native plus Managed synthetic coexistence. |
| Claude Managed SDK/CLI                                       | PASS offline                             | 16 actual SDK/CLI tests against mock provider, synthetic credentials only; no real-provider result inferred.                                                                                               |
| Codex Managed production process adapter                     | PASS for automated fixture process flows | 11 process tests; real Codex 0.159.0 additionally reached ready and listed seven models without a credential. Its unauthenticated turn failed as expected; no paid inference is qualified here.            |
| M4 desktop/full-screen restoration                           | PASS for scripted scenario               | less end screen survives reopen/helper replacement; PTY identity survives exhausted helper restart budget; subsequent shell output works. No IME/accessibility/logout/power-cycle qualification.           |
| M5 concurrency                                               | PASS for fixtures                        | Capacity 2/4 attacks, fencing, stalled sibling, reap-before-release and cancellation regressions; production remains one.                                                                                  |
| V9 persistence/recovery                                      | PASS for rehearsal only                  | 13 tests passed, one internal child entry ignored in normal discovery and invoked explicitly by its parent crash test. Full Claude replay remains FAIL.                                                    |
| Production package desktop and rollback                      | See release artifact summary             | Built package must be tested separately from fixture GUI; package/install qualification does not authorize deployment.                                                                                     |

Raw logs, screenshots, initial failures and corrected retries are retained locally under
`.artifacts/release-evidence/`. The release artifact summary records package digests, exact toolchain,
dependency-lock digests, package checks and measurements. Raw workstation status/cache data is not
published as source evidence.

## V9 recommendation and rollback constraint

**Recommend amending ADR0055 before acceptance** to use the closed identity pairs
`('codex','codex')` and `('claude','claude_code')`, matching the established Rust enum. Implement a
provider-aware replay verifier and writers using those identities; preserve the frozen V7 and V8
boundaries and compare all existing rows/events/receipts/leases/generations. Do not normalize Claude
history into Codex or call schema-only widening sufficient. Keep `STORE_HOLDS_CLAUDE=false` until
the complete path and mixed-provider tamper/replay cases pass. ADR0055 remains Proposed.

The disposable disk test proves the original V8 file is byte-identical after preparing V9, V8
validators refuse the V9 candidate, and restoring the original V8 snapshot recovers all rows. A real
crash before transaction commit recovers V8 through SQLite's journal. This is a rehearsal, not an
implemented operational migration tool. Once new V9-only history exists, binary replacement cannot
roll back the store: restoring V8 loses post-snapshot work unless separately preserved. That policy
and a qualified operational tool are required before a live migration is even proposed.

## Credential recommendation

**Recommend ADR0056 option A with explicit cross-process ownership:** native desktop consent/unseal
remains outside the authority's configuration-free binary. Define a reviewed, one-runtime handoff
to the authority-owned broker bound to runtime identity, selected record revision/fingerprint and
profile. A page must never supply a credential path, get the key, mint the grant or replay it.
The present same-user control socket is insufficient evidence for a protected principal; reconcile
ADR0056 with OD02 rather than importing the vault/configuration engine into the authority.

The disabled source validates record identity after the native dialog, refuses stale/deleted records,
wrong providers and repeated use, and persists only the existing encrypted record. Revocation policy
must say explicitly: deletion revokes future starts; stopping the owned runtime/broker revokes its
current spending capability. Replacement requires a new runtime. No memory zeroization guarantee is
claimed for `ApiKey`, Python strings or broker frame buffers. No owner API key was loaded.

## Read-only installation findings

Installed desktop and package authority are both version 0.2.0; version alone does not identify the
source commit. Installed GUI SHA-256 is
`56e201912514f45078a2f3a5ddc6bd18bfb5d77b5a4aaa9562dbe85a9ec2b9fa`; installed package authority is
`593c28d55513187b9666eb7d40288627575001209e149856df079c7a28f81828`.

Read-only `authority/hello` over the existing sockets reported protocol 1/schema 7 and protocol
2/schema 8, both service version 0.2.0. No database file was opened. Both services were active.
The running protocol-2 executable was under `wyss-console.pre-m1`, rather than the current unit's
configured package path: installed files and running processes are distinct versions. Running
authority hashes were `29beda6732022cc8b98cc2b177e43573fb914ef2e6b741539355b0640ca4bdf3`
(v1) and `fbd0e793b95575724716de250a7b46817f35e7d839f79ffd700907c684fddaab` (v2).
A Codex child remained under the v1 authority. No process was stopped to inspect it.

Lingering was `no`. The account still belongs to `docker`; B1 does not remove that administrative
delegation. No service configuration, group, privilege or lingering setting was changed.

| Combination                                                  | Compatibility / action                                                                                                                                                                                                    |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Candidate GUI + existing protocol-2 authority 0.2.0/schema 8 | Wire version checks agree; existing service is not replaced by copying a package. GUI startup may also run the v1 bootstrap: do not launch in production without that effect being authorized.                            |
| Candidate host + old host                                    | Host wire protocol remains 1; older-host replay fallback is tested. A running host retains its own executable and PTYs. A helper is used by the host that launches it; copying a new GUI does not upgrade a running host. |
| Candidate authority + disposable V8                          | Automated authority/process tests pass. Actual production restart/upgrade not executed.                                                                                                                                   |
| Current binaries + V9 candidate                              | Refused; restoring an approved V8 snapshot is required. No live V9 upgrade is allowed.                                                                                                                                    |
| Optional Claude Managed/B1/metrics/model selection disabled  | Native/terminal/Codex paths retain their existing behavior; no paid-provider admission is enabled.                                                                                                                        |

## Pending owner decisions and minimal blockers

No decision in this table was accepted by this session.

| Decision                        | Exact recommendation / retained state                                                                                                                                                     |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ADR0053                         | Accept only the documented provider/network/same-UID boundary after final review; real admission stays off.                                                                               |
| ADR0054                         | Model per turn, effort at runtime start; selection gate remains off.                                                                                                                      |
| ADR0055                         | Amend to the typed identity pairs above; complete mixed-provider writer/replay engineering before acceptance enables anything.                                                            |
| ADR0056                         | Native one-shot consent/unseal plus reviewed runtime-bound broker handoff; current candidate remains disconnected.                                                                        |
| ADR0057                         | Decide helper shipping, ABI/fallback policy and budgets after reviewing the x32 correction; helper failure must preserve PTYs/replay.                                                     |
| ADR0058 / OD02                  | Keep B1 absent from the package; decide whether a protected principal is worth its privileges. No privilege is granted here.                                                              |
| Metrics history                 | Off by default; explicitly decide retention/size/privacy before opening its gate.                                                                                                         |
| OD-M2-01                        | Decide whether to remove ADR0034's private plaintext fallback for secret-free restoration records when Secret Service is unavailable; preserve the accepted ADR until separately amended. |
| Managed concurrency >1          | Keep one; real-provider refresh/isolation/fairness evidence is required first.                                                                                                            |
| Real credentials/provider turns | Separate owner authorization and synthetic-to-real qualification; none granted or executed here.                                                                                          |
| Lingering                       | Leave unchanged/off.                                                                                                                                                                      |
| Live migration and installation | Separate approvals, each naming exact binaries, services, stores, backups, downtime and rollback.                                                                                         |

Minimal blockers to the requested complete release: full V9 provider-aware writers/replay and identity
contract; reviewed cross-process credential delivery; independent sign-off of the new release
security/credential corrections; required owner decisions and real-provider qualification. The package
with these capabilities disabled is an engineering candidate, not a completed M1–M5 production release.

See [the controlled rollout/rollback runbook](m1-m5-rollout-runbook-2026-10-24.md). No production
authorization request is issued while these engineering and qualification blockers remain.
