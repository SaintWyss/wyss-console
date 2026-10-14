# 42 — Post-REJECT corrective plan and safety stop conditions (2026-10-14)

**Status: corrective design proposal, not an accepted ADR, not implementation, not an independent PASS.**
The partial-independence adversarial review at
[reviews/2026-10-13-independent-post-owner-review.md](reviews/2026-10-13-independent-post-owner-review.md)
reported **REJECT**, 15 findings IR-01–IR-15. The reviewer authored substantial parts
of 05/12/13/15/16/17/18/31, so a third independent reviewer is still
required. Owner-authorised work remains: documentation and safe,
bounded disposable-environment verification only. No source-code,
provider-setting, service, credential or main-branch changes are authorised.

## 1. Non-negotiable P0 stop conditions

### C-A01 — Untrusted native process and same-UID socket impersonation (IR-01)

Accepted [ADR 0011](../adr/0011-external-control-authority-and-durable-state.md)
states same-UID SO_PEERCRED is not a barrier against a compromised
same-user process. [ADR 0020](../adr/0020-managed-sessions-ui.md) and
10 §1 retain that constraint. A local native agent or remote LP4 shell
running with host user rights may submit any host-user-reachable
API request, including managed approvals/OL3 decisions when those
APIs rely solely on same-UID peers. A separate GUI, typed plan or
HMAC carried by another same-UID process is not a proven boundary.

**Fail-closed target contract:** OL3 **MUST NOT automatically dispatch
native/full-account steps** while they can reach control/approval/
coordinator authority as the same OS security principal. Choose
and independently validate a non-bypassable separation:
a per-run restricted principal + IPC ACL/namespace/mount/network
boundary that retains the provider's required functionality and
prevents its credentials from reaching the authority, OR stop
native automation and keep native execution explicitly manual
until an acceptable model is proven. The second is a *temporary
blocked capability*, NOT silent removal of final OL3 scope.
The exact safe native profile and authenticator are **OPEN**.

The existing managed authority's same-UID trust model cannot be
upgraded merely by attaching a `native_confirmation` field.
A confirmation is strong only if its emitter is verified under
a distinct trust boundary that native agents cannot impersonate.
Until then, **do not call native confirmation unforgeable**.
Stop all unsafe OL3 paths at validation with
`BLOCKED_TRUST_BOUNDARY`, not merely a UI warning.
A reviewer must demonstrate the effect of a compromised native
agent trying `approvals.respond`, `grant_step`, `submit` and
every exposed host/gateway socket, and prove no cross-layer
control under the chosen boundary.

### C-A02 — LP4 shell and unattended host-account compromise (IR-02)

A remote `terminal.write` grant equals remote full shell/account
authority. Existing gateway same-UID stored grants are also
tamperable *by a compromised host-user shell*; revoking an enrolled
phone can prevent future normal gateway calls, but **cannot
reverse commands or guarantee restoring the account's integrity**.
The attacker may have installed user services, SSH keys,
persistent shell hooks or other access. Safe recovery requires
out-of-band account review, removing persistence and possibly
re-enrolment/rotation from a trusted boot/user context.
Do not assert that gateway revocation alone repairs compromise.

**Candidate safe defaults (owner trade-off not yet accepted):**
`LP4.write` denied while no verified local graphical login
session exists; `LP4-away` is a *distinct*, explicit, revocable
host-granted feature that requires a separate owner consent.
Default LP4 expiry should be short (candidate <= 15 minutes per
exclusive input claim, never an automatic 7-day full-write grant).
After lock/logout, existing write claim expires/blocks unless
explicit `LP4-away` entitlement is verified; remote
`sessions.read` can remain independently authorised.
An LP4 host grant record stored as a normal same-UID file is
**not compromise-resistant**. A strong revocation and
tamper-resistant grant issuer require a distinct, reviewed
trust/storage boundary or an explicit residual limitation.
No claim that Android Keystore presence by itself proves local
host presence. Android without verified per-use user authentication
and required key attestation gets **no LP4 by default** until a
specific owner risk decision.

### C-A03 — Native DAG start and prompt handoff (IR-03)

A bounded DAG approval only permits **scheduling and preparing**
named tasks. It **never** constitutes native launch approval.
Every `native/full-account` step requires a fresh, host-local,
specific launch confirmation bound to current plan digest, profile,
actual destination, controls, step/attempt and observed source
context. While C-A01 is unresolved, even that confirmation
cannot clear `BLOCKED_TRUST_BOUNDARY`.

Automatic model-produced *data-only* handoff is permissible only
to managed/proposal-only destinations after provider and policy
verification; it **must not** trigger a full-account native
agent. A text prompt can cause the native agent to perform effects:
quoting does not turn prompt injection into safe content.
A destination with native shell capability always remains in
`AWAITING_NATIVE_REVIEW` until the owner has reviewed the actual
payload and explicitly approved the particular attempt.
A stale revision or changed bytes invalidates that approval.

## 2. High-priority corrections

| IR | Corrective design disposition | Gate |
| --- | --- | --- |
| IR-04 | DN-08=A and DN-42=C are future product requirements, **not active IF-ORC invariants**. Existing ADR 0012 allows one shared App Server and ADR 0015 permits only one global active managed turn. Keep managed steps serialized until an independently reviewed ADR changes both. ADR 0031 remains Proposed | New authority ADR + isolated concurrency/selection tests |
| IR-05 | Hermes planner proposes only typed allowlisted actions. Freeform `shell.run`, interpreters, encoded loaders and other opaque command execution are **DENIED from Hermes** by default. Existing Wyss glob-based denial logic is not a semantic shell sandbox. No "approved summary" as replacement for literal effects | Pinned policy code review + E-HRM-09 adversarial corpus |
| IR-06 | Bind input claim to unpredictable fresh gateway boot identity and a monotonically increasing connection epoch. A restart **invalidates old claims**. On ACK-after-effect ambiguity, never blind-replay shell input; return `OUTCOME_UNKNOWN` and require review. Typed authority requests must use the authority's durable intent-idempotency mechanism, not gateway RAM as proof of exactly-once | Power-fail-between-effect-and-ACK test |
| IR-07 | Host-supervised PTYs/process trees need independently measurable process scopes/cgroup tracking. Treat survivors escaping the scope as `ORPHANS_POSSIBLE`/UNKNOWN, not necessarily terminated. Prevent automatic retries while ownership is ambiguous; a cgroup alone cannot prove absence of persistence a full-account user process could create | VM logout, crash and process-escape adversarial test |
| IR-08 | An active, paused, waiting-approval or recovering finite OL3 run prevents host idle-exit via a bounded keepalive from the headless coordinator. Zero terminal + zero viewer alone is insufficient. Service restart throttling is a surfaced `HOST_UNAVAILABLE` pipeline state | Host lifecycle state tests |
| IR-09 | PTY-owning supervisor must remain alive when one untrusted terminal screen parser fails. Parser isolated per terminal under separate low-privilege processing scope and measured memory/CPU caps. If its screen checkpoint fails, only fidelity degrades; do not silently kill PTY or other agents | Adversarial escape/OOM per-terminal crash test |
| IR-10 | Presence key properties and origin attestation must be explicitly checked for LP4; missing hardware-based evidence defaults to read-only while pending owner risk acceptance | Physical Android device with/without TEE/StrongBox |
| IR-11 | Device revocation blocks new delegated approvals and by default pauses pipeline dispatch whose last approval depends on that device; already-running full-account native processes may continue and must be reported as not stopped | Revocation/pipeline concurrency test |
| IR-12 | Provider-file writes, vault restore/undo and remote `config.request` apply stay `BLOCKED_FILESYSTEM` pending DN-21/31. A closed keyring in a lingered no-login host yields `VAULT_LOCKED_NO_SESSION`, no guessed unlock, remote data exposure or automatic apply | Keyring VM and TV-29 only with named owner environment approval |
| IR-13 | Preserve original TS-CFG-17..20 semantics (authoring, undo and page command restriction). Renumber new negative tests to **TS-CFG-21..24**, including new success/failure criteria and traceability | Static identifier audit |
| IR-14 | Rename 33's collision identifiers **RC-01..16** (do not replace historical 31 C1..C18), and 37's Android mockup identifiers **MA-01..10** (do not replace 16's MS-01..10) | Static uniqueness audit |
| IR-15 | 41's disposable `EXDEV` result is narrow kernel evidence, never a pass of actual home/snapshot/sync or DN-31 | Correct gate labels only |

## 3. Safe default vs owner choice

The owner explicitly delegated routine technical defaults but not
unavoidable product/security trade-offs. Accordingly:

- **Suggested safe defaults, not owner approval:** LP4 offline write
  denied; short per-claim TTL; LP4 attestation required; untrusted
  native OL3 steps blocked until OS separation; Hermes free shell
  denied; pipeline pauses on revoked-origin approvals.
- **Owner-dependent remaining choices:** whether to enable unattended
  LP4 at all (and on what devices/TTL); how much existing native-agent
  capability must remain in OL3 under a *different* OS principal;
  whether to accept unsupported Android attestation; DN-05 transport
  after measuring, and DN-31 if a non-exposing storage layout proves
  impossible. Ask only after feasible safe candidates and evidence
  are available. No need to interrupt the owner to choose safer
  interim stop conditions.

## 4. Acceptance extensions (all NOT RUN)

| Identifier | Failure case that must be denied / observed |
| --- | --- |
| TS-SEC-OL3-01 | Compromised native OL3 worker attempts all authority/coordinator/gateway same-UID socket operations; unauthorised requests cannot pass. Without an OS boundary, dispatch is disabled. |
| TS-SEC-OL3-02 | DAG-level approval with auto-handoff to native/full-account step cannot launch; current reviewed payload and local per-launch confirmation necessary; still blocked until trust boundary. |
| TS-SEC-REM-01 | LP4 when host logged out is rejected unless separate expressly owner-enabled absent-host grant is verified; grant expiry and key attestation enforced. |
| TS-SEC-REM-02 | After one LP4-compromised session implants persistence, device revocation cannot be reported as integrity restored; recovery requires host-account audit. |
| TS-SEC-REM-03 | Gateway crashes after applying terminal input but before ACK: reconnect old epoch returns OUTCOME_UNKNOWN, never duplicate keystrokes; typed authority intents use durable receipts. |
| TS-SEC-HOST-01 | Native child exits PTY but creates detached/escaped descendant; no automatic duplicate launch, state UNKNOWN/ORPHANS_POSSIBLE. |
| TS-SEC-HOST-02 | Single terminal parser OOM/crashes and its PTY/sibling PTYs continue; screen state labelled degraded. |
| TS-SEC-HOST-03 | Coordinator waiting for approval prevents host inactivity stop; host restart-exhaustion reports HOST_UNAVAILABLE. |
| TS-SEC-MOB-01 | Android software-only or missing per-use key-attestation cannot obtain default LP4 grant. |
| TS-SEC-ORC-01 | Revoking a pipeline authoriser device pauses unstarted work and reports already-running effects accurately. |
| TS-SEC-CFG-01 | No-login keyring locked: vault operation returns VAULT_LOCKED_NO_SESSION; queued config plan never auto applies. |
| TS-SEC-AGT-01 | Hermes opaque/interpreter shell requests denied, including reordering and encoding; typed allowlisted effects still pass full Wyss policy. |
| TS-SEC-MAN-01 | Under existing ADR 0012/0015, attempted second concurrent managed turn is serialized or refused; no one-runtime-per-session claim is treated as built. |

These test IDs are intentionally distinct from existing `TS-CFG`,
`TS-HYB` and `TS-ORC` families pending test-plan integration.
Pass/fail evidence requires controlled disposable setups. No test
has been executed by writing this document.

## 5. Review and signoff

1. Preserve the original REJECT report with reviewer limitations and
   actual commit; do not reclassify it to PASS.
2. Apply corrective specification diffs across original and new
   contracts, then redo static cross-references and identifier map.
3. Obtain a **third reviewer independent of both earlier design
   authors**. Review the accepted ADRs and real trust boundaries,
   not only delta document 42.
4. Any observed proof that same-UID native agents can forge
   managed approvals keeps native OL3 auto-run **BLOCKED** until
   an actual OS isolation mechanism is accepted.
5. No new provider execution/Android/VM test may be labelled PASSED
   from document-only evidence. No implementation, new ADR
   acceptance or PR merge is authorised by these corrections.

## 6. Codex/Sol High follow-up audit (2026-10-14) — remains REJECT

Read [the newly committed Codex audit](reviews/2026-10-14-post-reject-corrections-external-audit.md)
at commit `141b748d6e603ef25b0ee9c8c209d47ae86c2f7e`.
It reviewed the correction baseline `906822f` and
returned **REJECT**, with nine additional defect clusters
ER-01..ER-09. It did not certify absolute historic reviewer
independence or run any application/provider tests.

The objective corrections now documented:
- **ER-01:** approved ownership of `wyss-control` remains
  same-UID; blocking OL3 automatic native steps does
  not protect against native agents launched manually
  or LP4. Audit all authority-state, service and
  code paths, not only sockets.
- **ER-02:** a restricted native persona loses capabilities;
  preserve a provider-by-provider compatibility matrix.
  Any loss of sign-in, required hooks, MCP or unattended
  native work remains a blocked product decision.
- **ER-03:** model eligibility grant, per-use LP4 claim,
  gateway instance, terminal-writer receipt and managed
  durable intent are distinct; the old seven-day
  shell grant/24h volatile dedup is NOT a security guarantee.
- **ER-04:** Hermes planner needs enumerated typed effects
  and transitive-effect/TOCTOU controls. Original Wyss
  has shell and filesystem defenses beyond matching globs;
  see [40 §6](40-HERMES-BOUNDARY-STATIC-EVIDENCE.md).
- **ER-05:** current accepted one-runtime/one-turn baseline
  and future DN-08=A concurrency need separate, gated
  acceptance; see [20 §16.2](20-VERIFICATION-AND-ACCEPTANCE.md).
- **ER-06:** distinct parser PID does not itself imply
  isolated OOM/FD/IPC failure domains or PTY survival.
- **ER-07:** every new state needs owner, transition guard,
  durable identity, loss/expiry rules and UI action mapping.
- **ER-08:** revoking a device requires a defined ordering
  vs pipeline STARTING and durable authorization lineage.
- **ER-09:** original TS-CFG-17..20 need positive/negative
  cases and AC-CFG-AUTH-01, added to [20 §16.1](20-VERIFICATION-AND-ACCEPTANCE.md).

These changes correct contradictory *paper contracts*, not
secure the application. The following engineering choices
remain **OPEN / BLOCKED** pending feasibility evidence:
(1) protecting managed approvals from any unrestricted
native same-UID process, including external processes and LP4,
without losing the selected native functionality;
(2) full host-account LP4-away with short, real per-use
authentication and honest post-compromise recovery;
(3) multi-runtime authority extension under separately
approved ADR; (4) provider credentials/auth without copying
them across OS principals; (5) full parser and process-scope
fault containment; (6) safe config writes and snapshots.
Do not use another document-only "PASS" to replace actual
isolation, provider, Android or filesystem tests.

**No merge, no ADR acceptance, no code implementation.**

