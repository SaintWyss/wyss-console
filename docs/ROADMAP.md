# Remaining product roadmap

Only unfinished work belongs here. Current capabilities and evidence live in
[STATUS](STATUS.md); scope in [PRODUCT](PRODUCT.md); open choices in
[DECISIONS](DECISIONS.md#open-decisions). Remove a task when its acceptance is
complete. If implementation exists, retain only its missing validation or change.

## Engineering corrections

These are source-confirmed next slices from the architecture review. READY means specified
for implementation, not implemented or verified. Complete these bounded corrections while
independent qualification proceeds; do not wait for unrelated remote/Claude product work.
ENG-01, ENG-02 and ENG-03 are implemented with source tests; see [STATUS](STATUS.md).
Work states and handoff rules are in [EXECUTION](EXECUTION.md).

| ID     | State     | Outcome                                                                 | Depends on                                                      | FP/AC                     |
| ------ | --------- | ----------------------------------------------------------------------- | --------------------------------------------------------------- | ------------------------- |
| ENG-04 | DISCOVERY | Close live DB/witness backup consistency with an explicit procedure.    | Exact witness validation and snapshot-loss semantics.           | NFR-03, SEC-03            |
| ENG-05 | DISCOVERY | Design separate configuration and terminal WebView permission surfaces. | ENG-01 command inventory; preserve product workflows and focus. | SEC-01/02, CTX-04, NFR-02 |

### ENG-04 — DB and witness consistency

Trace `control.anchor`/witness validation and `scripts/f8-activation.sh`: an online DB snapshot
and later independent companion copies are not one atomic snapshot. Choose a supported
point-in-time pairing or a reviewed quiescent authority window, with loss window and restore
oracles; do not stop the Native host or delete a witness to make validation succeed. Deliver
a tested procedure/design first, then a bounded implementation task if required. The current
helper is not certified merely because SQLite's DB backup is consistent.

### ENG-05 — Distinct UI privilege surfaces

Design configuration/credential authoring versus terminal/provider display surfaces from
actual combined main permissions. Specify allowed messages, navigation/focus, native dialog
ownership, project-context binding and negative cross-window calls. Keep strong same-UID
approval as a separate unsatisfied OS boundary. Deliver the selected window/WebView design
and a migration plan before delegating its implementation; do not equate route separation
with permission separation.

## Requirement and journey corrections

The case catalogue refines all 31 FP/AC groups. These source-confirmed missing integrations
remain implementation work; refining their acceptance does not count as delivering them.
They can be reviewed without building or invoking providers.

| ID     | State     | Required outcome                                                            | Dependency / invariant                                                                 |
| ------ | --------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| REQ-01 | DISCOVERY | Define a backend-bound project/configuration context bridge across windows. | Preserve independent drafts and captured targets; no mutable global retargeting.       |
| REQ-02 | VERIFY    | Accept Projects screen recovery on a real host with full-screen programs.   | Implemented on the checked Owner; source tests pass. No installed-host or IME run yet. |
| REQ-03 | VERIFY    | Accept bounded project input and its outcomes against a real host.          | Implemented through Owner, service, DTO and pane; lost-reply cases are synthetic only. |

<a id="req-01"></a>

### REQ-01 — Project and configuration context

**Source:** Projects `selectProject` and configuration's native project selector are separate.
Changing Projects does not invoke configuration `select_project`. The configuration engine
does serialize that operation with apply and clear its own plans.

**Chosen direction:** each scoped editor/plan carries a backend-resolved project/binding/
revision and independent window context. Opening project configuration must identify that
target explicitly; navigation or another window cannot retarget existing drafts. Global
administration remains separate. Resolve a bounded DTO/compatibility design and current
native-editor cancellation semantics, then publish a READY implementation slice.
**Closure:** AC-FP-CTX-04/before-confirmation, after-admission, late-result and two-windows
each have an executable scenario; A operations cannot mutate B. Reuse configuration
plan-invalidation tests and Projects context tests without claiming they cover the missing bridge.

<a id="req-02"></a>

### REQ-02 — Projects terminal fidelity (remaining acceptance)

Implemented: `projects_term_attach` / `projects_term_attach_commit` on the epoch-checked
Owner, a one-use token bound to window, association, terminal and epoch, draw before commit,
and a labelled replay when no usable screen exists or the commit is refused
([contract](contracts/workspace.md#7-journey-semantics-and-implementation-gaps)).
**Remaining:** run it against a real host with the packaged helper: full-screen programs,
Unicode/IME, helper kill, host replacement, project switch and detach/reattach. The source
tests use an Owner double and a renderer double; they are not that acceptance.

<a id="req-03"></a>

### REQ-03 — Projects input bound and outcome (remaining acceptance)

Implemented: 64 KiB retained input including the piece in flight, one 4096-byte call in
flight (also refused in the service), whole-batch refusal before enqueue, generation reset on
blur/detach/release, and `written` / `partial` / `unknown` versus a pre-write refusal carried
through Owner, service, DTO and pane without resend.
**Remaining:** exercise saturation, mid-paste failure and lost reply against a real host and
PTY; the lost-reply and partial cases are covered only with synthetic owners.

## Priority and dependencies

| Priority | Remaining outcome                                                                      | Depends on                                                                                       |
| -------- | -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| 1        | Accept project/workspace adoption and terminal fidelity on the reference installation. | Agreed budgets, compatible host and session-preserving procedure.                                |
| 2        | Close Native/configuration/Arsenal/delegation qualification gaps.                      | Exact provider builds, recorded supported surfaces and real acceptance where required.           |
| 3        | Qualify and accept blocked Managed capabilities.                                       | Security/authentication decisions, credential delivery, protocol/store compatibility and review. |
| 4        | Resolve stronger worker/approval boundaries and qualify higher Managed concurrency.    | Useful provider authentication, containment evidence, trust decision and budgets.                |
| 5        | Accept finite coordination and retained handoff lanes.                                 | Qualified Managed profiles; stronger boundary for automated Native effects.                      |
| 6        | Deliver production PC/Android remote use.                                              | Transport decision, terminal fidelity, keys/presence and permission boundary.                    |
| 7        | Close retained OpenCode/Hermes/Wyss Agent extensions.                                  | Provider qualification; boundary/coordination for planner effects.                               |
| 8        | Complete whole-product acceptance.                                                     | Required capabilities above, both reference PCs and physical Android.                            |

Independent qualification work may proceed while another capability is blocked.
Do not repeat completed engineering or substitute more fixtures for missing real
provider/device evidence.

## 1. Workspace adoption and terminal fidelity

- [ ] Resolve and implement REQ-01, and accept REQ-02 and REQ-03 on a real host, before
      treating the ordinary-terminal/configuration evidence as Projects acceptance.
- [ ] Accept the real project/registry/layout import and rollback procedure;
      preserve existing session identities and prevent duplicate launches across
      project switches, reconnects and multiple windows.
- [ ] Agree measured project-switch/restore and screen/terminal budgets; close
      source/installed-host compatibility gaps before adoption.
- [ ] Close the parser/helper review and remaining real full-screen, Unicode/IME,
      rendering, replay-fallback redraw/resize and lifecycle acceptance. Actual logout, suspend and power-loss
      outcomes need their own evidence; lingering is an explicit owner choice.

**Done when:** reference-installation journeys preserve live work and meet agreed
budgets, with correct missing-process and degraded-fidelity behavior.
Use [workspace](contracts/workspace.md) and
[terminal fidelity](contracts/terminal-fidelity.md) criteria.

## 2. Native, configuration, Arsenal and delegation

- [ ] Complete real GUI/provider consumption evidence for supported profile,
      model/effort, project authoring and imported-template apply/Undo paths; cover
      external writers, scope changes and interrupted recovery.
- [ ] Bound and qualify saved-launch directory enumeration: processing 64 files currently
      follows collection of all TOML candidates. Preserve exact byte/character/count units
      and define truthful overflow behavior before claiming bounded inventory cost.
- [ ] Keep provider-version qualifications current. Reuse the recorded Codex
      0.159.0 and Claude 2.1.316/2.1.319 Arsenal exclusions instead of treating them as
      unimplemented; qualify unproven cases and new builds before claiming support.
- [ ] Close any uncovered real two-computer sync/conflict/recovery journeys and
      Native delegation provider-pair/cancellation/concurrency journeys using their
      current contracts. Record what ran; a harness alone is not a PASS.
- [ ] Resolve restoration policy extension where all-sealed storage conflicts
      with the accepted secret-free fallback; preserve kind-specific recovery and
      stop on diverged outside edits.
- [ ] Complete independent inference/native-hook coexistence evidence, including
      policy precedence, authentication/refresh and existing bindings.

**Done when:** every offered function has build-specific evidence and truthful
configured/observed/unsupported labels. See the
[Native contract](contracts/native.md),
[sync](contracts/arsenal-sync.md) and [delegation](contracts/delegation.md).

## 3. Blocked Managed capabilities

- [ ] Resolve the pending Claude security/authentication and credential-delivery
      decisions and obtain independent review plus real-provider profile evidence.
- [ ] Accept and qualify the provider-aware store preparation/replay and the
      concrete installed migration/backup/recovery procedure. Existing offline V9
      implementation is not an automatic migration or store acceptance.
- [ ] Accept/negotiate the model/effort protocol extension and complete selection,
      attestation, cancellation, reroute and recovery qualification.
- [ ] Resolve the opt-in usage-history privacy/retention policy and verify only
      the accepted metadata is persisted.
- [ ] Close remaining real Codex tool/error/conformance cases; distinguish
      Local Tools from the currently refused production inference-only path.

**Done when:** each enabled profile has an accepted contract and its own source,
fixture and real-provider evidence. Closed `m3/gate.rs` conditions remain closed
until their individual requirements are met. See
[Managed parity](contracts/managed-runtime.md) and
[metrics policy](contracts/usage-history.md).

## 4. Boundaries and Managed concurrency

- [ ] Resolve the worker/approval trust decision with concrete compatibility and
      useful authentication/egress evidence, including same-UID/admin attackers.
- [ ] Qualify useful restricted-worker capture, staging, quarantine and result
      publication; finish independent review. Existing B1 fixtures are not a
      production boundary and must not be planned as one already accepted.
- [ ] Reconcile the specific scratch/timing/classifier/stream/attachment and
      memory gaps in [STATUS](STATUS.md#resource-and-acceptance-gaps).
- [ ] Before raising F8 capacity, demonstrate provider refresh compatibility,
      generation isolation, control fairness, cancellation/reap and measured load
      at the proposed capacity; obtain explicit approval of the increase.

**Done when:** each advertised boundary has positive and negative evidence;
production F8 remains at one until higher capacity is qualified. Native
delegation's separate limits do not satisfy this task. See
[boundary](contracts/security.md) and [concurrency](contracts/concurrency.md).

## 5. Handoff and finite coordination

- [ ] Qualify V10 offline adoption/rollback, coordinator import, pinned root/claim ownership,
      fresh-session proof and verification/settlement. Production guarded execution remains
      false until its review, compatibility and evidence prerequisites pass.
- [ ] Close real daemon disconnect/reconnect, crash-at-dispatch, cancellation,
      failure propagation and ambiguous-result recovery journeys without duplicate
      effects; complete the pending independent review.
- [ ] Qualify supported cross-provider/mode handoff with revision/digest and
      receiving-plan limits; durable payloads require an accepted privacy contract.
- [ ] Accept final effectful lanes only after their provider and trust gates;
      automated Native full-account steps remain dependent on the stronger boundary.

**Done when:** a finite implementation/test/review workflow completes and recovers
truthfully on a real project, preserving ordinary authority confirmations and the
[coordination guard contract](contracts/coordination.md).

## 6. Remote PCs and physical Android

- [ ] Choose transport from concrete private-overlay versus embedded P2P/relay
      evidence, including Android, costs, maintenance and resource footprint.
- [ ] Complete gateway/client transport, host identity/pinning, enrollment,
      grants, revoke, reconnect and partial/unknown-input handling; never replay
      terminal input after a lost acknowledgment.
- [ ] Complete Android device-keystore/presence qualification and real lock,
      background, reinstall and key-invalidation journeys.
- [ ] Qualify LAN/mobile/CGNAT, roaming, outages and long-running use; absent-host
      full-account control remains dependent on the trust decision.

**Done when:** authenticated PC-to-PC and PC-to-Android journeys work on physical
devices with demonstrated permission scope, revocation and recovery.

## 7. Retained extensions

- [ ] Complete real qualification of the supported OpenCode and Hermes Native
      surfaces, including requested versus effective settings.
- [ ] Complete the accepted Wyss policy/effects plus Hermes planning direction:
      useful data-only proposals, containment and mediated coordination integration.

**Done when:** retained extension scope has its own compatibility and boundary
evidence; a Native CLI launch does not establish a safe effectful planner.

## 8. Whole-product acceptance

- [ ] Freeze and meet the remaining performance budgets on both reference PCs:
      input-to-render, resize, terminal idle/load, project navigation, runtime trees
      and long-run pressure; retain failures and outliers.
- [ ] Complete real WebKitGTK, keyboard/IME, TalkBack, themes, accessibility and
      end-to-end screen/error-state journeys on actual devices.
- [ ] Resolve ENG-04 companion-witness consistency, then qualify session-preserving
      mixed-version updates, consistent SQLite/WAL
      backup, restore, rollback and interruption/power-loss recovery.
- [ ] Close all applicable [functional acceptance criteria](REQUIREMENTS.md)
      and unresolved blocking findings with evidence at the required level.

**Done when:** every in-scope criterion passes or has an explicit owner-approved
scope change. Whole-product acceptance stays distinct from integration of code
or a documentation review. Public distribution is outside the personal scope.
