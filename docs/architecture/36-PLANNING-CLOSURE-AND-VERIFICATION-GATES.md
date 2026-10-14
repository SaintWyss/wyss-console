# 36 — Final-product planning closure and evidence gates

Status: **planning integration + remaining-work register**. Updated after the owner
delegated routine recommendations and authorised documentation-only changes on
the existing planning branch on 2026-10-13. Not an acceptance report.
See [31](31-PLANNING-COVERAGE-AUDIT.md) for the historical weighted audit.
The 64.2% figure is a **historical coverage-of-paper score**, not a
current verified percentage, and not percentage of product implemented.

## 1. Gate hierarchy — never conflated

| Gate | Required evidence | Current state |
| --- | --- | --- |
| G0 Owner target scope | Decisions marked owner-selected in 32; remaining genuinely owner-dependent trade-offs explicitly open | Broad product scope set; DN-05 and DN-31 open |
| G1 Paper contracts | Requirement -> stable design/interface/state/failure modes -> phase -> test -> acceptance, with source and evidence label | Partial; expanded in 33–35, still needs cross-document reconciliation |
| G2 Review of paper | Independent reviewer inspects the **entire post-REJECT** design, security claims, migration and product fit; every critical finding resolved or explicitly rejected by owner with evidence | **Not done**; earlier two independent reviews remain REJECT |
| G3 Permission for restricted observations | Owner separately authorises exact experiment, target, data access and rollback; no broad standing permission | **Not granted**, DN-24 and DN-16 remain closed |
| G4 Observed compatibility and safety | Per-version provider tests, filesystem/replication/snapshot behaviour, keyring, concurrency, Android, networking, performance on reference machines | **Not done** for this change; no measured resource bounds |
| G5 Accepted ADRs and final plan | Every extension touching accepted authority/OS boundaries has independent security review, explicit ADR acceptance and final owner's plan approval | **Not done** |
| G6 Implementation | Only after all applicable preceding gates, separate explicit user approval | **Prohibited** |

**Zero** of the new owner decisions in [32] are security ADR acceptances.
A signed-off paper design is not observed software behaviour. This file
does not schedule, run or request G3 without the owner's separate action.

## 2. P0 architectural blockers

### B-P0-01: concurrent writers, provider settings, replicated snapshots

Document 28 identifies L1, L2 and L5; the owner refused silent risk acceptance.
DN-31's layout is not resolved by checking a parent ignore marker, moving into
another mount/subvolume, or calling a copy 'temporary'. Review path and
replication tool's **actual** exclusion rules, snapshot scopes, same-filesystem
rename/exchange preconditions and keyring failure modes. Find the smallest
provider-native mutation surface, disabling direct writes elsewhere.
If no compliant safe write is feasible under the user's real environment,
the honest result is a **product gap/blocker** requiring an owner trade-off,
not permission to silently degrade final scope.
Evidence TV-29, OQ-31/35/40 and accepted follow-up ADR required.

### B-P0-02: full-account remote terminal vs scoped configuration

DN-44=B is a desired capability, not a grant boundary. An LP4 device
can type `rm`, `sed`, launch another provider and write files as the
host Linux user, including the vault's ciphertext and potentially unlocked
key material. A host-only confirmation requirement on `config.request`
protects only that typed API path, **not LP4**. Describe plainly,
require explicit opt-in and revocation, and obtain separate security review.
If genuinely constrained mobile control is also required, it needs a
different OS principal/containment architecture and a distinct decision;
do not imply biometric plus app grants creates it.

### B-P0-03: authority concurrency and managed model selection

DN-08=A and DN-42=C require reviewing one-writer/one-turn invariants,
provider App Server parameters, session/runtime generation, catalog lock,
thread/turn IDs, attestation and lease independence, reroute classifier.
No coordinator may use an authority method not covered by accepted ADRs.
ADR 0031 for observed model remains Proposed; model selection needs its
own ADR and an **external** security review. Unknown vendor behaviour
cannot be validated by reading schemas alone.

### B-P0-04: Wyss Agent effectful tools

DN-06=D requires *demonstrated* OS-level containment of Hermes or a
well-evidenced alternative preventing any direct effectful call. Wyss
Agent policy/execution must own every action; the Console's OL3
coordinator does not grant tools to Hermes. The original source analysis
in 17 is read-only; OQ-55–57 remain unverified. If no confinement
works with required capability, suspend the integration and bring the
real scope/security trade-off to the owner.

### B-P0-05: gateway and local-secret protection

DN-05 hybrid transport unresolved; remote terminal input adds host
account-level power. DN-47 host screen parser introduces an untrusted
escape-sequence interpreter to a long-lived process. The gateway and
parser require independent adversarial review, practical device-loss
recovery, host-key pinning, no plaintext relay and explicit acceptance
on reference hardware.

### B-P0-06: review verdicts

The two previous independent REJECTs (25) are still in force; no
self-review or checklist generated by the same author overturns them.
Obtain a new independent reviewer after major cross-doc updates.
Findings must be fixed and rechecked; 'accepted risks' require informed,
explicit owner approval and must not contradict mandatory safety rules.

## 3. P1 paper planning tasks that need no provider execution

Each row is a *deliverable*, not a claim that it has been completed.

| Work item | Deliverable and acceptance | Primary documents |
| --- | --- | --- |
| P1-01 Owner trace | Each owner decision has an exact alternate/why, requirement and corresponding tests; delegated defaults clearly distinct | 32 -> 22, 02, 23 |
| P1-02 Orchestration | Finite OL3 DAG, states/guards, budgets, cancellation/reconnect, safety boundaries and 15 named tests; define metadata-only state | 34 -> 02, 09, 10, 17, 19, 20, 23 |
| P1-03 Remote host | Transport-neutral host/gateway/API, mobile UI states, security consent, screen snapshot and reconnect; 14 named tests | 35 -> 16, 05, 13, 19, 20, 23 |
| P1-04 Model analytics | Selected != observed, reroute, token provenance, privacy, model/effort feature matrix and historical store contract | 12, ADR 0031, 32, 33 |
| P1-05 Personal deployment | Reduce public-distribution requirements to an *explicit conditional branch*, add private local package/update/backup/recovery acceptance; no published repository | 18, 19, 02, 20, 23 |
| P1-06 Terminal memory | Distinct quotas for screen/history/output queues/renderer, user-visible data-loss labels, OOM behaviour without deleting held data | 05, 15, 16, 35 |
| P1-07 Configuration | User-command authoring through native dialog and validated provider-native writers; exact error/rollback states; no untested parser assumption | 07, 08, 11, 22, ADR 0030 |
| P1-08 UX | Global nav, contextual disabled actions, 3 themes, opt-in desktop events, assisted layout recovery, Android mockup states, a11y | 13, 16, 32 |
| P1-09 Test taxonomy | Test named, level, fixture and failure/assertion criteria; link every requirement, including negative security criteria | 20, 23 |
| P1-10 Threat trace | Attack-by-surface inventory: config webview, terminal webview, PTY parser, gateway, phone, relay, coordinator, Hermes, vault, replicated files | 11, 14, 16, 17, 33 |
| P1-11 Implementation roadmap | Phases with prerequisites and *stop gates*, including no side effects in read-only phases and no public release in personal scope | 19, 26, 29 |
| P1-12 Audit redo | Regenerate 31's 29-row score from evidence, provide denominator/weight, never count owner choice as technical verification | 31 |

A specification row is COMPLETE on paper only if a builder would not invent
a requirement or fail-path decision. A row BLOCKED on evidence is still
BLOCKED even if every fixture to test it is written.

## 4. Conditional acceptance plan for the personal final product

| Phase group | Non-negotiable acceptance |
| --- | --- |
| Terminal/workspaces (T1/T2/F3) | PTY fidelity against a real reference; keyboard/IME; layout restore; no automatic restart; bounded memory; screen reconnection |
| Provider inventory/config (F1–F5) | Real provider data source labelled inferred vs observed; safe per-version adapter; no write where protocol not validated; disable/restore exact without data loss |
| Profiles/model management (F3/F6) | Every profile omission verified before launch; native visual model change safe; managed selected/reported model and per-turn selection correctly separated; historical metadata privacy |
| Multi-agent OL3 (F8/F11) | Independently isolated managed generations, finite user-approved DAG, cancellation, fairness, conflict detection and no forged approval; Hermes only on its own accepted boundary |
| Host+remote+Android (F9/F10) | Survivable window/logout, truthful crash state, device enrolment and revocation, safe LP4 model, connection migration exactly-once input, no plaintext relay, fidelity on Android |
| Vault/replication (F2v/F2h) | Keyring behaviour measured; export/import opt-in authenticated; no unintended disclosure by sync/snapshot/swap; old bytes safely restorable |
| Personal delivery (re-scoped F13) | Installer/update on the owner's reference Linux without changes to unrelated state, verified rollback; no public repository, signing service or automatic update check required by default |
| Observability/security (cross-cutting) | No secret-bearing durable history, logs, notification or usage; failure states explicit; independent security review accepted before code work is authorised |

Each is a target **acceptance obligation**, not a test result.
Public distribution-specific acceptance criteria remain conditional and
are not silently dropped from historical docs.

## 5. Evidence collection protocol for later authorised work

Each technical evidence item must contain: exact question, target host/profile,
source version/commit, pre-run and post-run state checks, command/procedure,
consent scope, expected observable outputs, secrets handling, rollback, pass/fail
condition, actual output provenance and responsible reviewer. Prefer no user
configuration reads; never copy credentials or token values into documents.

Order dependencies: static read and schema review -> independent paper threat
review -> explicitly authorised disposable-environment probes -> reference
hardware measurements -> ADR acceptance -> explicit final-plan signoff ->
implementation. Discovery of a severe contrary fact sends the design back
to paper, not directly into a hotfix on `main`.

## 6. Definition of 'planning complete'

Only report full planning completion when:
1. All owner-relevant scope decisions are captured; DN-05 resolved from
   evidence and DN-31 either safely resolved or presented as a clear
   unavoidable scope trade-off to the owner.
2. Every requirement in 02 (including new ORC/REM/UX/MOD cases) links to a
   concrete design, phase, test and pass/fail acceptance in 23.
3. No contradictory guarantee is claimed across 05/07/09/10/11/12/13/15/16/17/18.
4. All security assertions that *require observations* are explicitly
   verified after authorisation, or are documented as unresolved and
   cannot be advertised as complete.
5. Independent review of the entire amended design changes the
   earlier REJECT outcome on evidence; no author self-certification.
6. All material ADRs get separate explicit acceptance; no implicit
   status transitions or loosening of existing constraints.
7. The owner explicitly approves the **complete** final-product
   architecture and implementation roadmap, not merely individual
   functional preferences.

G0 paper direction alone does not satisfy G3/G4/G5/G6.
No automatic permission is inferred from 'finish everything'.

## 7. Specific further-paper deliverables (2026-10-13)

[37](37-HANDOFF-ADAPTERS-UX-AND-DATA-CONTRACTS.md)
provides the requested missing states for F11a, OpenCode/Hermes
adapter evidence requirements, versioned config command authoring,
Android mobile state sketches and history metadata contracts.
[20 §15](20-VERIFICATION-AND-ACCEPTANCE.md) and
[23 §6](23-TRACEABILITY-MATRIX.md) add the matching planned
acceptance/crosswalk. They do **not** resolve actual
provider compatibility, filesystem semantics, Android security,
user-interface approval or the independent REJECT verdicts.


## 8. Independent-review package prepared, NOT performed

The concrete brief is [38](38-INDEPENDENT-REVIEW-HANDOFF.md):
read-only, adversarial, all 29 subsystems, with traceability
and boundary-specific mandatory questions. It must be handed to an
**independent reviewer** with access to the private PR.
Neither this file nor writing the brief constitutes a
review, a PASS or an owner signoff. Previous REJECTs stand.

## 9. Verification-stage permission clarification (later 2026-10-13)

The owner explicitly authorised advancing from documentation
consolidation into **bounded technical verification on isolated
disposable targets**, as well as an independent review.
This is a *stage authorisation*, not a blanket permission
for provider-account access, real configuration edits, normal
user OS services, Android phone settings, or code implementation.
A limited disposable `renameat2(RENAME_EXCHANGE)` experiment
was performed in the isolated Linux execution container;
see [41](41-STATIC-TRACEABILITY-AND-ISOLATED-FILESYSTEM-CHECK.md).

An accessible isolation environment for full Hermes/Android
integration **is not currently provisioned here**. The sandbox
has no `cargo`, `adb`, Android emulator, Podman or Docker.
Do not report the candidate remote/Hermes E2E tests as PASS.
They remain pending a separate disposable test runner/device
that does not carry the owner's secrets or provider sign-in.
DN-16 **implementation remains prohibited** and this permission
does not accept any ADR.

## 10. New REJECT and added P0 stop states (2026-10-14)

The [post-owner adversarial report](reviews/2026-10-13-independent-post-owner-review.md)
returns **REJECT** and declares limited independence.
Its 15 findings and their corrective states are
enumerated in [42](42-POST-REJECT-CORRECTIVE-PLAN.md).
This extends the earlier P0 set with:

- `BLOCKED_TRUST_BOUNDARY`: same-UID native agent/LP4
  shell can impersonate local authority/gateway callers.
  No native auto-step from OL3 until proven OS separation
  AND per-launch current-context host confirmation.
- `BLOCKED_NATIVE_HANDOFF`: full-DAG consent and
  automated model output cannot trigger a native
  full-account agent.
- `BLOCKED_MANAGED_CONCURRENCY`: accepted ADR 0012
  has one App Server; ADR 0015 has one global active
  managed turn. Serialise until a separately
  reviewed authority extension is accepted.
- `BLOCKED_LP4_ABSENT`: remote terminal input denied
  by default during host logout. A separate explicit
  absent-host risk decision and technically reviewed
  key/host boundary would be necessary.
- `BLOCKED_FILESYSTEM`, `VAULT_LOCKED_NO_SESSION`:
  no provider mutation or vault recovery if DN-31
  unresolved or required keys unavailable.

The 2026-10-13 [isolated EXDEV observation](41-STATIC-TRACEABILITY-AND-ISOLATED-FILESYSTEM-CHECK.md)
is **only a negative cross-mount kernel check**. It
cannot close DN-31, L1/L2/L5, snapshot exclusion,
Syncthing ignore behaviour or the owner's actual
filesystem proof.

**Independent review is still outstanding.** The
present author and earlier author/reviewer cannot
self-convert the new REJECT to PASS. No code,
provider experiment or ADR acceptance follows
from documenting these blocked states.


## 11. Feasibility gate after the two REJECT reviews (2026-10-14)

[43](43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md)
describes distinct-UID authority candidate A vs
restricted automated native worker candidate B,
their effect on normal provider authentication/hooks,
headless LP4 and accepted ADR 0025 no-sudo bootstrap.
Two tiny disposable Linux socket/file fixtures were run:
same UID gained access; different unprivileged UID
received EACCES under private directory/file modes.
These checks are **only proof of synthetic DAC
behaviour**, not Wyss approver integrity, provider
compatibility, trusted UI/device approval or a
working separate-user service. All of TV-ISO-03..10
are NOT RUN; TV-ISO-02 is PARTIAL synthetic only.
A genuine independently reviewed S1/S2 architecture
and an informed owner trade-off remain mandatory.


## 12. 2026-10-14 verification evidence scope update

[44](44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md)
records 19 **synthetic, isolated** fixture assertions,
not product tests. The mock different-UID verifier
accepted one signed dummy action and rejected
forgery/replay/scope/expiry; a rootless user namespace
preserved access to original host-UID mock credentials;
a path-swap analogue redirected a pathname write,
while anchored descriptors and real `openat2`
prevented the test-specific redirect/escape.
All real Wyss, managed Codex, Hermes, Android,
user-service and filesystem snapshot tests remain
**NOT RUN**, and no reference-machine runtime is
claimed. An independent post-correction security
review is still required. The A/B solution and
DN-16 remain **BLOCKED**.


## 13. Explicit environment access blocker

The next owner-authorised test step needs a
**named disposable Linux target** with a
real systemd user manager and suitable
Landlock/bubblewrap isolation, plus mock
provider credentials and synthetic projects.
The current tool container cannot run
Landlock (ENOSYS) or provide QEMU/Android/
`cargo`/`bwrap`. See [44 §8](44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md).

No real provider token, user home, user's
background service, actual phone or production
`wyss-control` should be used just to
satisfy a test. If a clean VM cannot be
identified, mark TV-ISO-03..10
`BLOCKED_ENVIRONMENT`; a review must not
relabel mocks as final-product evidence.


## 14. Evidence handoff for isolated guest runner

The next specific step is
[45](45-DISPOSABLE-VM-VERIFICATION-HANDOFF.md),
a ten-case disposable-VM procedure for protected
authority vs restricted worker. It contains exact
stop conditions, fake credentials, real systemd/
Landlock/bwrap feasibility requirements, negative
tests, evidence classification and docs-only
report location. The current tool container cannot
execute those full VM tests; nothing in that
handoff grants access to the owner's actual PC,
provider tokens or host services.
