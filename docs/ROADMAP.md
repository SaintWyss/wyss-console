# Remaining product roadmap

Only unfinished work belongs here. Current capabilities and evidence live in
[STATUS](STATUS.md); scope in [PRODUCT](PRODUCT.md); open choices in
[DECISIONS](DECISIONS.md#open-decisions). Remove a task when its acceptance is
complete. If implementation exists, retain only its missing validation or change.

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

- [ ] Accept the real project/registry/layout import and rollback procedure;
      preserve existing session identities and prevent duplicate launches across
      project switches, reconnects and multiple windows.
- [ ] Agree measured project-switch/restore and screen/terminal budgets; close
      source/installed-host compatibility gaps before adoption.
- [ ] Close the parser/helper review and remaining real full-screen, Unicode/IME,
      rendering and lifecycle acceptance. Actual logout, suspend and power-loss
      outcomes need their own evidence; lingering is an explicit owner choice.

**Done when:** reference-installation journeys preserve live work and meet agreed
budgets, with correct missing-process and degraded-fidelity behavior.
Use [workspace](contracts/workspace.md) and
[terminal fidelity](contracts/terminal-fidelity.md) criteria.

## 2. Native, configuration, Arsenal and delegation

- [ ] Complete real GUI/provider consumption evidence for supported profile,
      model/effort, project authoring and imported-template apply/Undo paths; cover
      external writers, scope changes and interrupted recovery.
- [ ] Keep provider-version qualifications current. Reuse the recorded Codex
      0.159.0 and Claude 2.1.316 Arsenal exclusions instead of treating them as
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

- [ ] Close real daemon disconnect/reconnect, crash-at-dispatch, cancellation,
      failure propagation and ambiguous-result recovery journeys without duplicate
      effects; complete the pending independent review.
- [ ] Qualify supported cross-provider/mode handoff with revision/digest and
      receiving-plan limits; durable payloads require an accepted privacy contract.
- [ ] Accept final effectful lanes only after their provider and trust gates;
      automated Native full-account steps remain dependent on the stronger boundary.

**Done when:** a finite implementation/test/review workflow completes and recovers
truthfully on a real project, preserving ordinary authority confirmations.

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
- [ ] Qualify session-preserving mixed-version updates, consistent SQLite/WAL
      backup, restore, rollback and interruption/power-loss recovery.
- [ ] Close all applicable [functional acceptance criteria](REQUIREMENTS.md)
      and unresolved blocking findings with evidence at the required level.

**Done when:** every in-scope criterion passes or has an explicit owner-approved
scope change. Whole-product acceptance stays distinct from integration of code
or a documentation review. Public distribution is outside the personal scope.
