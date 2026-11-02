# Remaining product roadmap

Only unfinished work belongs here. Current capabilities and evidence live in
[STATUS](STATUS.md); scope in [PRODUCT](PRODUCT.md); open choices in
[DECISIONS](DECISIONS.md#open-decisions). Remove a task when its acceptance is
complete. If implementation exists, retain only its missing validation or change.

## Engineering corrections

These are source-confirmed next slices from the architecture review. READY means specified
for implementation, not implemented or verified. Complete these bounded corrections while
independent qualification proceeds; do not wait for unrelated remote/Claude product work.
Work states and handoff rules are in [EXECUTION](EXECUTION.md).

| ID     | State     | Outcome                                                                            | Depends on                                                      | FP/AC                         |
| ------ | --------- | ---------------------------------------------------------------------------------- | --------------------------------------------------------------- | ----------------------------- |
| ENG-01 | READY     | Explicit main-only permission for the registered orchestration bridge.             | Existing closed DaemonRequest contract.                         | SEC-01, ORC-01/03             |
| ENG-02 | READY     | Prevent resend of uncertain host mutations.                                        | No new provider, service or schema adoption.                    | TERM-01/02, SES-01/03, CTX-04 |
| ENG-03 | READY     | Enforce the existing 4096-byte stdout scratch ceiling and truthful classification. | Existing F8 resource profile; do not change capacity.           | SES-04, NFR-01                |
| ENG-04 | DISCOVERY | Close live DB/witness backup consistency with an explicit procedure.               | Exact witness validation and snapshot-loss semantics.           | NFR-03, SEC-03                |
| ENG-05 | DISCOVERY | Design separate configuration and terminal WebView permission surfaces.            | ENG-01 command inventory; preserve product workflows and focus. | SEC-01/02, CTX-04, NFR-02     |

<a id="eng-01"></a>

### ENG-01 — Explicit orchestration command permission

**Evidence:** `orchestration_commands.rs` registers `orchestration_call`; `build.rs` declares
it and `main` grants its permission. Runtime ACL mediation and the focused checks remain
unverified; existing view mocks do not exercise ACL mediation.

**Chosen change:** add the command to the application manifest and grant its generated
permission to `main` only. Keep the closed `DaemonRequest` allowlist and authority checks.
Do not grant a generic process/file/provider RPC, broaden `w-*`, or enable guarded execution.
Relevant source: `build.rs`, `capabilities/main.json`, `orchestration_commands.rs` and
`src/lib/desktop/orchestration-client.ts`.

**Verification:** extend the existing capability coverage approach to this registered
extension; compare handler/manifest/grant. Test permitted main invocation and actual denied
extra-window invocation using a suitable Tauri harness, rather than relying only on a mocked
`invoke`. Confirm production fixture exclusions and the unchanged closed daemon request shape.
**Done when:** the command has an explicit least-privilege route and the positive/negative
boundary checks pass. Existing F8/V10 release gates remain unchanged.

<a id="eng-02"></a>

### ENG-02 — Host transport uncertainty without resend

**Evidence:** `host/client.rs::Remote::call` maps send and receive errors to the same
`host_unavailable`; `HostLink::call` reconnects and resends any operation. Host protocol 1
has no general intent receipt. `term_create` and `term_write` are effectful consumers.

**Chosen change:** separate pre-dispatch availability from possible-send/no-reply. After
a mutation might have been delivered, return an explicit uncertain result without retry,
host replacement or local fallback for that request. Default unknown operations to mutation
semantics; only explicitly proven pure reads may retry. Initial connection/bootstrap before
the first send remains separate. Propagate truthful outcome to affected terminal/Native
clients; do not retry under a new terminal/session identity. Preserve Projects Owner's
stricter epoch-checked, attach-only path. Durable host-wide deduplication is outside this slice.

**Verification:** owned synthetic host accepts a create/write, drops the reply and stays
alive: one effect, one send, explicit uncertainty. Cover disconnect before send, lost reply,
host epoch replacement, existing-host survival, unknown operation, allowed read retry,
partial terminal input and Projects recovery. Never signal/restart an installed host to test it.
**Done when:** the affected route cannot automatically resend a possibly executed mutation,
and UI/error contracts distinguish known no-send from unknown-after-send.

<a id="eng-03"></a>

### ENG-03 — Provider scratch and classification

**Evidence:** JSON `provider_scratch_bytes=4096`; stdout `ProcessWorker::CHUNK=16*1024`.
The stderr buffer is separate. `budget::classify` currently marks scratch NotProven and
other items vary independently; scheduler source bounds are not hard wall-clock guarantees.

**Chosen change:** retain the existing 4096-byte limit, align the stdout chunk and tie its
numeric definition to the resource contract with a meaningful consistency check. Preserve
bounded partial-frame parsing, total read budget, EOF/malformed handling and control
availability. Update only classifications whose enforcement is actually demonstrated;
do not relabel all NotProven entries or change production cap one.

**Verification:** existing worker/resource tests plus boundary and fragmented/oversized
frame cases, noisy stdout/stderr and slow consumer/control availability. Show source-bound
enforcement separately from real-provider timing/performance qualification.
**Done when:** source, executable contract and classification agree for this ceiling,
without weakening another byte/count bound or losing partial-frame data.

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

| ID     | State     | Required outcome                                                            | Dependency / invariant                                                           |
| ------ | --------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| REQ-01 | DISCOVERY | Define a backend-bound project/configuration context bridge across windows. | Preserve independent drafts and captured targets; no mutable global retargeting. |
| REQ-02 | DISCOVERY | Specify Projects screen attach/commit on its checked Owner transport.       | Preserve epoch/association/reader fences; never substitute generic HostLink.     |
| REQ-03 | DISCOVERY | Specify and implement bounded project input with truthful write outcomes.   | Reuse existing queue limits; define Owner/service/DTO propagation before coding. |

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

### REQ-02 — Projects terminal fidelity

**Source:** `SessionPane.svelte` reads the retained ring and does not use the snapshot
attach/commit route exercised by ordinary terminal tests.

**Chosen direction:** obtain snapshots through the epoch-checked Owner; bind a staged
one-use cursor to association/terminal/epoch/reader, draw before commit and preserve output
on failed draw. Define bounded DTOs and permissions by reusing the existing screen contract.
Unsupported hosts get explicit replay/gap labels, not fabricated full-screen recovery.
Do not use generic HostLink initialization/retry/fallback to attach a project session.
**Closure:** the contract resolves stale token, renderer disposal, helper failure, host change
and project-switch behavior; the resulting READY slice includes source test locations and
real full-screen/IME acceptance still needed after implementation.

<a id="req-03"></a>

### REQ-03 — Projects input bound and outcome

**Source:** project input chains 4096-byte calls with no aggregate queue-byte bound.
Owner/service return generic failures, and the pane can say input was not sent after
earlier chunks were accepted. Current first-chunk refusal tests do not establish that claim.

**Chosen direction:** retain at most 64 KiB including in-flight input, one 4096-byte call
in flight, reject non-fitting new batches before enqueue, and invalidate unsent old-generation
work on blur/detach/claim loss. Already admitted bytes may have reached the PTY.
Specify no-send/known-prefix/unknown propagation through Owner, service, typed DTO and UI;
never reconstruct an accepted count from an error code or replay uncertain bytes.
Known busy refusal before admission may retry within its existing bound.
**Closure:** settle the outcome DTO and claim-generation interaction before implementation;
then define saturation, mid-paste failure, lost reply, blur/reclaim and pane-disposal cases.
No provider or installed-host experiment is needed to prepare that design.

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

- [ ] Resolve and implement REQ-01, REQ-02 and REQ-03 context, project-screen and input gaps before
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
