# Dependency-ordered final-product roadmap

Baseline status: **proposed remaining engineering plan**, 2026-10-22. The Phase1
documentation delivery made no application changes and authorized no credential-backed
experiment, deployment, migration, restart or ADR acceptance. Dated M1/M2 addenda below
record later engineering deliveries without rewriting that baseline. The complete
personal product remains the completion target. Intermediate milestones are increments,
not a reduced MVP scope.

## 1. Priority and dependency graph

| Milestone | Deliverable                                                                | Depends on                                                   | Baseline / priority                                                                                                                                              |
| --------- | -------------------------------------------------------------------------- | ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M0        | Qualification manifest and preservation plan                               | Phase1 consolidated docs                                     | Required evidence foundation; existing tests are inputs                                                                                                          |
| M1        | Project-centered workspace and migration                                   | M0; ADR0052 Accepted (architecture only)                     | Highest UX priority; project records partly exist                                                                                                                |
| M2        | Claude/Codex Native parity, configuration/profile closure                  | M0; M1 for contextual UI                                     | Highest provider priority; finish partial acceptance                                                                                                             |
| M3        | Claude Managed parity and versioned model/effort contract                  | M0, M2 provider evidence; ADR0053 and separate selection ADR | Highest architecture priority; adapter built offline (PR #23), release blocked on ADR0053/0054/0055 and provider evidence                                        |
| M4        | Faithful persistent terminal and bounded parser isolation                  | M0; M1 identity contracts                                    | Existing host preserved; new screen/lifecycle work                                                                                                               |
| M5        | Stronger worker/approval boundaries and qualified concurrency              | M0, M3; owner OD02 where applicable                          | Current F8 cap 1 stays; safety gate for Native OL3/LP4                                                                                                           |
| M6        | Structured handoff and finite OL3 coordination                             | M1, M3, M5 for final effectful lanes                         | Serial Managed scheduling can be built without raising cap 1                                                                                                     |
| M7        | Remote PC gateway and native Android                                       | M1, M4; OD01; M5/OD02 for full-account control               | Partial transport-neutral core and offline Android client; OD01/OD02 and production remote remain blocked ([record](../acceptance/m7-engineering-2026-10-26.md)) |
| M8        | Whole-product efficiency, accessibility, reliability and personal delivery | M1–M7; M9 for retained extension scope                       | Runs throughout; final closure only with evidence                                                                                                                |
| M9        | OpenCode/Hermes/Wyss Agent integration closure                             | M2 contracts; M5/M6 for planner effects                      | Partial version-qualified Native adapters and blocked data-only planner boundary ([record](../acceptance/m9-engineering-2026-10-26.md))                          |

M1, provider feasibility in M2/M3, and M4 design can proceed independently once their
shared identity/capability inputs are fixed. Implementation queues must respect each
security/ADR dependency. Do not block unrelated documentation/fixtures on an unavailable
device, and do not count a blocked final requirement as done. Every milestone has an
independent review of its changed trust surfaces before release.

## 2. Milestone contracts

### M0 — Qualification and preservation foundation

**Eligible M0 work completed, 2026-10-22.** Reproducible source/component inventory,
installed-build capability manifest, every F8 resource-ceiling classification, scoped
read-only preservation observations and selected fixture results are in the
[canonical baseline](implementation-status.md#m0-engineering-baseline) and its linked
provider manifest. Phase1 merged at `d2ee8d2`; application baseline is unchanged.
No installed store, credential, service or session was changed by M0.

Performance procedure is frozen by reference to architecture15; new real-desktop budgets,
provider consumption/refresh and exact resource conformance are explicitly NOT_PROVEN,
not counted as qualified production measurements. M1 release must collect its own
reference performance/preservation evidence; M2/M3 carry G-INF E1–E9 and provider parity;
M5 owns the F8 provider stdout scratch mismatch, stale catalogue classification, service quantum/load and
attachment/retention enforcement proof before any capacity expansion. Current cap1 stays.
No paid/provider/lifecycle experiment is required to begin M1's pure implementation work.
Rollback of M0 is documentation/manifest revert only, with no installed operation.

### M1 — Project-centered workspace

**Engineering complete behind an off gate; release blocked on the owner's adoption and the real-desktop
measurements** ([delivery record](../acceptance/m1-delivery-2026-10-23.md)). FP-CTX-01..04 are implemented per
the [M1 contract](../contracts/m1-workspace.md): closed records/DTOs,
limits, dependency-ordered increments, M1-01..13 tests, deterministic legacy migration,
context/receipt handling, mixed-host compatibility, preservation and rollback. That is
the single detailed handoff; do not duplicate it in a new plan. Global administration and
Scratch stay independent from Managed authority. M1 does not implement Claude Managed,
OL3, new remote transport or concurrency above1. ADR0052 is **Accepted** for the
project/workspace identity architecture only (not inferred from the PR16 merge); its
validation and release gates remain, and release/adoption needs separate authorization.

Verification/DoD: all AC-FP-CTX-01..04 and M1-01..13, including two windows, old host,
no-effect navigation/restore, dispatch/association ambiguity, corrupt/newer stores and
interrupted migration. Native desktop acceptance must show all existing sessions remain
attributable, zero duplicate processes/prompts, no host restart/SQL migration, no project-
switch effects, and measured performance budgets. M1-01..12 pass (M1-02 partial: no foreign-ACL check);
M1-13 has measured costs and **no budget**; the native journey with real providers and the owner's own
registry is the adoption acceptance (see the delivery record).

Risk/rollback: existing registry open mutates and old layout restore can recreate shells;
M1 requires non-starting checked observation and attach-only recovery. Preserve private
legacy snapshot/map, account for post-snapshot work and do not restore over a live writer.
Use GUI/metadata rollback with explicit owner review; installer rollback can close terminals
and is not a substitute. Exact failure states/procedure are in contract sections3–4.

### M2 — Native parity and configuration closure

Close original-feature matrices for real Claude Code/Codex versions, profile exclusions,
model/effort mechanisms and correct consumption labels. Complete F5 imported-template
confirmed apply/Undo and provider consumption slices where supported. Qualify project
authoring, scope changes and absent-aware writes. Keep unsupported Codex per-session
hooks/skills exclusions explicit. Reconcile all-sealed final restoration with ADR0034
secret-free disclosed fallback through a focused policy extension; do not call that
change already accepted. Preserve kind-specific diverged-entry restoration (ADR0041),
not a universal exact-Undo guarantee. G-INF/native-hook coexistence research must include
system/project/plugin/managed policy E9, authentication/refresh and old binding behaviour;
isolated Local Tools production acceptance does not close these automatically.

Verification: AC-FP-SES-01, CFG-01..05, MOD-01; selected fixtures first, then authorized
native reference acceptance preserving provider settings/modes, own sign-in and existing
sessions. No paid-model call is inferred from permission to read source. DoD: both
providers' original workflows and exact limits have evidence, all authoring/Undo paths
have race/failure coverage, GUI says precisely what each session uses or cannot observe.

Risk: unsupported provider versions/format changes and external writers. Rollback:
undo only unchanged postimages through reviewed writer; otherwise stop/replan, retain
sealed recovery material. Never force-restore over outside edits.

**M2 engineering delivery, 2026-10-23:** `ENGINEERING_COMPLETE_RELEASE_BLOCKED` on
`feat/m2-native-parity-2026-10-23`. The frozen 12-oracle contract and actual test,
provider and review evidence are in the [M2 delivery record](../acceptance/m2-engineering-2026-10-23.md).
Native desktop/provider-consumption acceptance and OD-M2-01 remain release conditions;
this delivery does not include M3 Managed model/effort selection or Claude Managed.

### M3 — Managed provider parity and model contract

First freeze Claude feasibility outputs in ADR0053: exact official surface/version,
owned process protocol, supported auth/settings route, complete effect inventory,
no-tool attestation and action mediation. Implement an inference profile and then
Local Tools only when each qualifies. Full milestone includes both; inference-only
increment is not final Claude parity. Reuse existing identity/lease/receipt invariants,
not Codex-specific normalization assumptions. Preserve original Native option.

Separately specify versioned Managed model/effort request fields, current-catalogue
validation, generation binding, pre-admission attestation, per-turn selection/reroute
semantics and recovery. Accepted ADR0014/0015 exclude choice today; SG-12 remains
unchanged and is not itself the model prohibition. Propose a focused ADR when those
exact technical fields/semantics are ready; do not accept one here. Define opt-in
model/usage history schema/privacy/quota, independent from control audit and vault.

Verification: AC-FP-SES-02/03, CFG-05, MOD-01/02, SEC-01/02; unknown events/tools,
callbacks that do not hold an action, hooks/MCP/plugin/subagent escape, cancellation,
resume and crash-at-dispatch. Positive real-provider compatibility follows separate
authorization and required review; no copied credential fixtures with personal content.
DoD: each released Claude profile has source+fixture+real evidence and accepted safety
ADR; both providers' supported Managed controls have equivalent user-visible lifecycle
without pretending identical original capabilities.

Risk: SDK callbacks insufficient as an isolation boundary, auth incompatibility, no
reliable catalogue. Rollback: disable the new profile/feature on fresh launches, preserve
old session history and existing Codex profiles; never auto-convert a session to Native.

The frozen engineering oracles and current status are recorded in the
[M3 contract](../contracts/m3-managed-parity.md),
[acceptance record](../acceptance/m3-engineering-2026-10-23.md), and
[Claude feasibility report](../research/claude-managed-feasibility-2026-10-23.md).
As of 2026-10-23 Claude profiles remain blocked pending no-effect/action-mediation proof;
these records do not accept ADR0053 or authorize real-provider tests.

### M4 — Persistent terminal fidelity and lifecycle

Deliver isolated individually restartable parser helpers, bounded volatile checkpoints,
screen/alternate buffer/cursor/modes, explicit history gaps and one authoritative size.
Keep PTY masters in the host; parser failure cannot kill PTY siblings. Specify helper
IPC/isolation/limits in a focused ADR before release. Implement owner-enabled lingering
and truthful lock/logout/suspend/power-off behaviour; never enable lingering silently.

Verification: AC-FP-TERM-01/02, SES-03 and existing TI/PS/KR/TS-HOST contracts. Real
full-screen/Unicode/IME/keyboard/reference screen corpus, hostile escape/parser fuzz,
slow viewer, crash/OOM and orphan ambiguity tests. DoD: faithful attach on reference
PCs; bounded memory; no claimed survival of dead processes or automatic duplicate launch.

Risk: parser compromise, screen fidelity and PTY partial writes. Rollback: disable
checkpoint feature for new attachments and report explicit limited fidelity; do not
restart host/live PTYs just to revert a viewer. Such a rollback is degraded operation,
not fulfillment of final acceptance.

### M5 — Boundaries, B1 feasibility and concurrency

> **Engineering state (2026-10-24):** the fixtures, the constrained B1 candidate and the contracts exist on
> `feat/m5-boundaries-concurrency` ([record](../acceptance/m5-engineering-2026-10-24.md)); capacity stays 1, B1 and FX-H01 stay
> NOT_PROVEN, nothing is released. What remains is evidence and owner decisions, not more fixtures: provider refresh evidence per
> provider, a trusted capture from a real repository, the `PENDING` publication, a reviewed implementation, and OD02.

Two separate tracks: (a) qualify F8 capacity above1 with per-generation isolation,
fairness, separate staging/catalogues, real provider shared-state/refresh compatibility
and reference-PC resource measurements; (b) resolve useful B1/auth/egress and protected
human approval against the defined U/LP4/admin attackers. Per-session runtime existence
does not close either. Keep cap 1 until track(a) qualifies and Santiago approves increase.

Track(b) carries architecture42–52, FX-H01 and distinct U/B1/M/P/A labels. Test useful
positive workflow plus denial of authority/admin/session IPC, inherited FDs, worktree
injection, network escape and credential misuse. No Docker/user/service privilege
migration is implied. OD02 chooses the owner-dependent threat/absent-host trade-off
only after reversible compatibility options are concrete.

Verification: AC-FP-SES-04, SEC-01, ORC-03, original F8-01..18 and concurrency-specific
adversarial tests at2/4 only after authorization. DoD: each claimed boundary has actual
positive/negative evidence and independent security review; no generic “sandbox PASS”.
Rollback: capacity stays/returns1 for new admissions; drain or explicitly cancel owned
work under approved operational plan. Boundary change rollback preserves metadata;
no arbitrary process-name kill or automatic privilege change.

### M6 — Handoff and finite OL3

Extend reviewed <=16KiB Native brief to structured source/target/revision/digest
handoff and permitted cross-mode recipient admission. Optional durable payload needs
sealed privacy contract. Implement headless coordinator, immutable bounded DAG,
metadata-only run journal, ownership/worktree coordination, per-attempt reconciliation,
fair scheduling and stop. Keep all current authority/tool confirmations and cap 1
admission. Managed-only serial scheduling can ship as an increment; final Native
automated steps remain blocked until M5 boundary and per-launch current-content consent.

Verification: AC-FP-HAND-01 and ORC-01..03, TS-ORC-01..15/TS-HAND-01..05 with updated
F8 interpretation. Crash between spawn/ACK, source mutation, injected handoff,
external editor, dependency failure and revoke mid-run produce no duplicate effects.
DoD: implement/test/review pipeline works on a project with both providers where
qualified, finite limits, precise trust labels and all failures/recovery visible.

Risk: untrusted context as authority, ambiguous spawn and false merge success. Rollback:
stop new scheduling, preserve metadata, reconcile owned work, expose independent sessions;
do not kill the authority or discard evidence to cancel one pipeline.

### M7 — Remote PCs and native Android

> **Engineering slice (2026-10-26):** [M7 record](../acceptance/m7-engineering-2026-10-26.md)
> adds a dual-key P-256 remote core, one-shot input sequencing and an offline native
> Android client shell. OD01 remains unresolved; no transport or remote access is enabled.

Resolve OD01 from H1/H2 comparative evidence, choose standard authenticated encrypted
transport and real asymmetric device/step-up verification. ADR0049 foundations are
not production cryptography. Specify protocol envelope, endpoint pinning, enrollment,
grant/revoke, claim epochs, **accepted byte counts and partial/unknown input results**,
per-instance replay refusal and reconnect checkpoints. No exactly-once terminal
promise across crash. Lock/logout full-account input stays blocked until OD02.

Deliver desktop client, gateway, native Android screens and actual platform-keystore/
presence qualification. Config request remains a host-confirmed proposal, with LP4
account bypass risk disclosed. No remote listener or device enrollment occurs in this
documentation phase; opening exposure is a separate concrete deployment action.

Verification: AC-FP-REM-01..03; LAN/mobile/CGNAT/relay outage/roaming; malformed/replayed
pairing, wrong key/revoke, partial input-before-ACK, real hardware key invalidation,
Android lock/background/reinstall, host logout/wallet lock and 24h/7d stability.
DoD: authenticated second-PC and real-phone user journeys work, scopes preserved across
path changes, no replay and no plaintext relay, owner accepts actual operational risks.

Risk: public network surface, lost device, absent-host account power, Android lifecycle
and relay operations. Rollback: revoke grants/stop gateway under approved plan, leave
local host work and stores intact. Revocation does not undo executed shell commands
or clean a compromised account.

### M8 — Whole-product closure

Run all JF journeys and retained detailed acceptance across both reference PCs and
actual Android, with Claude/Codex Native/Managed capability manifest. Complete M01..M33
measurements and approved budgets, three-theme accessibility, keyboard/IME, long-run
resource pressure, mixed-version updates, backup/recovery and rollback. Existing limited
backend throughput/idle observations are not input-to-glyph acceptance. No technology
rewrite or lower threshold merely to make a benchmark pass.

DoD: every in-scope traceability row passes at its required evidence level or has an
explicit owner-approved product change; all critical/high confirmed issues resolved;
privacy/no-leak corpus passes; no final-product blocker concealed as unavailable UI.
No public release/distribution work is needed under ADR0027. Independent final product
review is distinct from this Phase1 documentation review.

Risk: calling integration coverage “production verified”, unqualified target metrics,
service rollback interrupting sessions. Rollback: tested matching binary/consistent
store snapshot with explicitly approved downtime/loss window; no downgrade/force push.

### M9 — Retained extensions

> **Engineering slice (2026-10-26):** [M9 record](../acceptance/m9-engineering-2026-10-26.md)
> adds version-qualified OpenCode Native launch/profile and separate Hermes Native launch,
> plus a closed data-only Hermes proposal contract. It does not accept a Hermes effectful
> planner boundary or claim real-provider qualification.

Finish OpenCode supported scope and qualify upstream Hermes Native. Wyss Agent policy/
effects plus Hermes planning follows owner DN06=D, not a repeated architecture choice.
Resolve source compatibility, useful planner containment and integration with OL3;
do not import the archived assistant's legacy effect paths into Console authority.
Verification: AC-FP-EXT-01 and existing TS-ADP/AGT plus positive/negative planner tests.
DoD: retained personal scope accounted for, capability evidence and no unauthorized
effects. Rollback disables extension admission; core Claude/Codex work remains usable.

## 3. Outstanding owner decisions and technical discovery

| ID                                   | Alternatives / recommendation                                                                                                                                                                                                     | Unblocks / what is still missing                                                                                             |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| OD01 (DN05)                          | H1 automatically managed private overlay versus H2 embedded P2P/relay. Recommend H2 **if** Android/maintenance/security/footprint comparison passes; otherwise H1 with transparent connection UX                                  | M7 transport/relay/vendor costs and operational choice; no vendor selected here                                              |
| OD02 (DN44 / A boundary)             | Stronger restricted workers + protected approval principal versus account-trusting full Native/LP4 risk model. Recommend first, retain manual unrestricted Native explicitly labelled; keep absent-host LP4 denied until reviewed | Native OL3 and final anywhere full-account control; same-UID/admin capability compatibility and actual human-presence design |
| OD03 (DN31 only if evidence fails)   | Prove actual protected-state/sync/snapshot writer conditions; if impossible, choose concrete constrained write surface versus explicit risk trade-off. Recommend proving/restricting surface first                                | Wider mutation/recovery guarantee; ADR0034 custody is already decided and is not reopened                                    |
| OD04 operational, not Phase1 blocker | Retain P1 beside P2 until owner chooses retirement; recommend no retirement as part of redesign                                                                                                                                   | A later session-preserving service/store retirement plan                                                                     |

Technical work requiring evidence, not another owner product questionnaire: exact Claude
SDK/build/auth route; G-INF E9 policy precedence and old bindings; numeric runtime/
screen/resource budgets; parser implementation/isolation; provider refresh semantics;
Android presence capabilities; final DTO/SQL schemas; per-feature consumption and usage
metadata policy. Return a genuine product trade-off only when these facts rule out a
recommended design. Acceptance of Proposed ADR0053 and any later security ADR is
always Santiago's explicit act, never inferred from WD-01/02 or merging this PR.

## 4. Uniform verification and rollback record

Every implementation milestone produces: requirement/AC IDs, exact source/build and
device/provider/profile versions, commands/procedure, positive/negative results and raw
evidence locator, NOT_RUN/NOT_PROVEN limits, independent findings/resolutions, before/
after preservation, matching backup/binary and a rollback rehearsal with affected live
work and post-backup loss window. Fixture PASS, installed PASS and owner acceptance are
separate columns. Historical blanket private-HOME/VM procedures are not current recipes;
follow the normal owner environment policy, permitted unit fixtures and intrinsic
Managed isolation. Never run blanket `cargo test`/`lifecycle_process` as a shortcut.

**M2 + M3 integration, 2026-10-24:** one integration PR carries M2 (#22) and M3 (#23); `CODE_INTEGRATED`,
`ENGINEERING_TESTS_PASSED`, `SECURITY_REVIEW_PENDING`, `RELEASE_BLOCKED`
([record](../acceptance/integration-m2-m3-2026-10-24.md)). M4 is not started.
