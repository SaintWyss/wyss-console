# Wyss Console architecture set: index

Status of this set: **design, not implementation, and not accepted**. Written on 2026-10-12 and
merged to `main` the same day by the owner's instruction; revised the same day, in the working
tree, after the owner's policy on secrets, a proposed ADR 0029 and one independent review whose
verdict was **reject** ([25](25-INDEPENDENT-REVIEW.md)). Revised again on 2026-10-13, after the
owner decided that a protected private backup is allowed: ADR 0029 was redrafted, ADR 0030 was
proposed, and documents 27 to 29 were added. Later that day the interface specification (13)
was re-derived, a planning coverage audit (31) was added, the terminal and native-session
domain (05) was specified in full, and performance engineering (15) was rewritten. None of these revisions has been
reviewed by anyone but its author. No phase is authorised by this set, and none is authorised
at all: implementation waits for the owner's word ([22](22-DECISIONS-AND-OPEN-QUESTIONS.md),
DN-16). How far the design really is: [26](26-READINESS-ASSESSMENT.md).

Nothing in it is built unless a document says so with a file reference. No application code,
build or test was run to write it; whatever needs execution to be known is marked NOT VERIFIED
with the procedure that would settle it.

## How to read it

| If you want                                                              | Read                                                                        |
| ------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| What the product is and for whom                                         | [01](01-PRODUCT-VISION.md)                                                  |
| What it must do, with identifiers                                        | [02](02-REQUIREMENTS.md)                                                    |
| What exists today and what is missing                                    | [03](03-CURRENT-STATE-AND-GAP-ANALYSIS.md)                                  |
| How the whole system is divided                                          | [04](04-TARGET-ARCHITECTURE.md)                                             |
| The central feature (real configuration)                                 | [07](07-CONFIGURATION-ENGINE.md), [08](08-RULES-HOOKS-SKILLS-MCP.md)        |
| What may never be weakened                                               | [10](10-MANAGED-RUNTIME-AND-SECURITY.md), [11](11-SECURITY-THREAT-MODEL.md) |
| In which order to build, and how to accept it                            | [19](19-IMPLEMENTATION-ROADMAP.md), [20](20-VERIFICATION-AND-ACCEPTANCE.md) |
| What is still undecided                                                  | [22](22-DECISIONS-AND-OPEN-QUESTIONS.md)                                    |
| Where this design is weak                                                | [24](24-ARCHITECTURE-REVIEW.md)                                             |
| What an independent reader found                                         | [25](25-INDEPENDENT-REVIEW.md)                                              |
| How ready it is, honestly                                                | [26](26-READINESS-ASSESSMENT.md)                                            |
| How much of the final product is planned, and what documentation remains | [31](31-PLANNING-COVERAGE-AUDIT.md)                                         |

## Documents

| #   | Document                                                                            | Subject                                                                                                                                                                                                                              |
| --- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 00  | This index                                                                          | Map, conventions, baseline                                                                                                                                                                                                           |
| 01  | [Product vision](01-PRODUCT-VISION.md)                                              | Identity, problems solved, users, journeys                                                                                                                                                                                           |
| 02  | [Requirements](02-REQUIREMENTS.md)                                                  | Functional and non-functional requirements, with identifiers                                                                                                                                                                         |
| 03  | [Current state and gap analysis](03-CURRENT-STATE-AND-GAP-ANALYSIS.md)              | What is implemented, tested, documented, missing                                                                                                                                                                                     |
| 04  | [Target architecture](04-TARGET-ARCHITECTURE.md)                                    | Subsystems A to Q, boundaries, design decisions                                                                                                                                                                                      |
| 05  | [Terminal and native sessions](05-TERMINAL-AND-NATIVE-SESSIONS.md)                  | Terminal engine, agent launcher, supervisor, session registry                                                                                                                                                                        |
| 06  | [Agent adapters](06-AGENT-ADAPTERS.md)                                              | Provider capability specification and adapter contracts                                                                                                                                                                              |
| 07  | [Configuration engine](07-CONFIGURATION-ENGINE.md)                                  | Scopes, states, lifecycle of a change, observation                                                                                                                                                                                   |
| 08  | [Rules, hooks, skills and MCP](08-RULES-HOOKS-SKILLS-MCP.md)                        | Every user-visible action on each kind of entry                                                                                                                                                                                      |
| 09  | [Profiles and orchestration](09-PROFILES-AND-ORCHESTRATION.md)                      | Profiles, simultaneous sessions, handoff                                                                                                                                                                                             |
| 10  | [Managed runtime and security](10-MANAGED-RUNTIME-AND-SECURITY.md)                  | The authority as built, F1b, future managed extensions                                                                                                                                                                               |
| 11  | [Security threat model](11-SECURITY-THREAT-MODEL.md)                                | Assets, boundaries, threats, restoration storage, secrets                                                                                                                                                                            |
| 12  | [Model management](12-MODEL-MANAGEMENT.md)                                          | Catalogues, configured, observed, selection                                                                                                                                                                                          |
| 13  | [UX and UI specification](13-UX-UI-SPECIFICATION.md)                                | Screens, states, journeys                                                                                                                                                                                                            |
| 14  | [Data, persistence and recovery](14-DATA-PERSISTENCE-AND-RECOVERY.md)               | Ownership of state, crash consistency, rollback                                                                                                                                                                                      |
| 15  | [Performance engineering](15-PERFORMANCE-ENGINEERING.md)                            | Metrics, benchmark procedures, proposed budgets                                                                                                                                                                                      |
| 16  | [Multi-device and remote](16-MULTI-DEVICE-AND-REMOTE.md)                            | Two computers; the host service (F9); remote access, pairing, grants and their limits, a remote threat model, the phone's screens (F10a, F10b)                                                                                       |
| 17  | [Wyss Agent and Hermes](17-WYSS-AGENT-AND-HERMES.md)                                | Source research of the original Wyss and Hermes, options A to D for Wyss Agent, orchestration levels, naming                                                                                                                         |
| 18  | [Distribution and maintenance](18-DISTRIBUTION-AND-EXTENSIBILITY.md)                | Platforms, packaging, installation, first run, updates and migrations, data removal, licences, security maintenance, extension boundaries                                                                                            |
| 19  | [Implementation roadmap](19-IMPLEMENTATION-ROADMAP.md)                              | Phases, dependencies, definitions of done                                                                                                                                                                                            |
| 20  | [Verification and acceptance](20-VERIFICATION-AND-ACCEPTANCE.md)                    | Master test plan and acceptance matrix                                                                                                                                                                                               |
| 21  | [ADR reconciliation](21-ADR-RECONCILIATION.md)                                      | ADRs 0001 to 0028, one by one                                                                                                                                                                                                        |
| 22  | [Decisions and open questions](22-DECISIONS-AND-OPEN-QUESTIONS.md)                  | What is undecided, what evidence decides it, who decides                                                                                                                                                                             |
| 23  | [Traceability matrix](23-TRACEABILITY-MATRIX.md)                                    | Requirement → design → component → phase → test                                                                                                                                                                                      |
| 24  | [Architecture review](24-ARCHITECTURE-REVIEW.md)                                    | Self-challenge: contradictions, weaknesses, residual risks                                                                                                                                                                           |
| 25  | [Independent review](25-INDEPENDENT-REVIEW.md)                                      | One cold review by another reader, and what was done about it                                                                                                                                                                        |
| 26  | [Readiness assessment](26-READINESS-ASSESSMENT.md)                                  | What is complete, unverified, partial or blocked; work remaining                                                                                                                                                                     |
| 27  | [Protected restoration vault](27-PROTECTED-RESTORATION-VAULT.md)                    | The encrypted store that keeps a disabled entry restorable: records, key, consent, lifecycle, threats                                                                                                                                |
| 28  | [Concurrency and filesystem integrity](28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md)  | What can and cannot be promised when a provider writes the same file; behaviour case by case                                                                                                                                         |
| 29  | [Capability preservation](29-CAPABILITY-PRESERVATION.md)                            | Every capability of the final product against the security decisions: kept, blocked, or not designed enough                                                                                                                          |
| 30  | [Technical validation plan](30-TECHNICAL-VALIDATION-PLAN.md)                        | What must be observed or tested before the vault and the write protocol can be relied on; the results of the one inspection run so far (2026-10-13) and the blockers it found                                                        |
| 31  | [Planning coverage audit](31-PLANNING-COVERAGE-AUDIT.md)                            | The final product's planning, subsystem by subsystem: coverage of requirements, design, decisions, traceability and verification; contradictions found; the ordered documentation backlog. A documentation measure, not a validation |
| —   | [The reviewer's report](reviews/2026-10-12-independent-review-report.md)            | The independent review, unedited                                                                                                                                                                                                     |
| —   | [The focused security review](reviews/2026-10-13-focused-security-review-report.md) | The second independent review, of the vault and the write protocol: **reject**, eleven findings; unedited                                                                                                                            |
| —   | [ADR 0029](../adr/0029-secrets-restoration-data-and-configuration-persistence.md)   | **Proposed**, second draft: secrets, restoration data, the vault, persistence                                                                                                                                                        |
| —   | [ADR 0030](../adr/0030-corrections-to-adr-0028.md)                                  | **Proposed**: corrections to two security statements of ADR 0028                                                                                                                                                                     |
| —   | [ADR 0031](../adr/0031-observed-model-of-managed-sessions.md)                       | **Proposed**: the observed model of managed sessions (DN-09)                                                                                                                                                                         |

## Hierarchy of documents

When two documents disagree, the higher one rules and the lower one is corrected.

1. **Product vision and approved owner requirements**: [`docs/PRODUCT.md`](../PRODUCT.md),
   with [01](01-PRODUCT-VISION.md) and [02](02-REQUIREMENTS.md) as its expansion.
2. **Current functional specification**: `docs/spec/` (does not exist yet; documents 05 to 16
   of this set are its source).
3. **Technical architecture and accepted ADRs**: [`docs/adr/`](../adr/), with
   [04](04-TARGET-ARCHITECTURE.md) as the overall picture.
4. **README and the implementation**: what exists. The README describes reality; it does not
   redefine the product.
5. **Historical research and archived specifications**: `wyss-agent-legacy/`, the base
   specification v0.1, the research report of 2026-10-01 and the vision report of 2026-10-12.
   Inputs, never authority.

One exception runs against the order: a product goal does not by itself override an established
security control. Changing one takes an ADR and proof (decision D10, ADR 0028 §11).

## Evidence classes

Every factual claim in this set carries one of these marks where it matters.

| Mark  | Class                              | Meaning                                                                                                 |
| ----- | ---------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `[R]` | VERIFIED IN REPOSITORY             | Read in this repository, with the file and line                                                         |
| `[D]` | VERIFIED IN OFFICIAL DOCUMENTATION | Read in the provider's own documentation on 2026-10-12                                                  |
| `[O]` | REPORTED                           | Observed on the owner's computer in recorded sessions on 2026-10-12, or stated by the owner; not re-run |
| `[P]` | PROPOSED                           | A design of this set, approved by nobody yet unless the text says which decision approved it            |
| `[N]` | NOT VERIFIED                       | Unknown. The text says what would verify it                                                             |

## Identifiers

| Prefix                                                                                          | Is                                                                                                                                        | Defined in         |
| ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ------------------ |
| `FR-<AREA>-nn`                                                                                  | Functional requirement                                                                                                                    | 02                 |
| `NFR-<AREA>-nn`                                                                                 | Non-functional requirement                                                                                                                | 02                 |
| `AD-nn`                                                                                         | Architecture design decision of this set                                                                                                  | 04                 |
| `C-nn`                                                                                          | Constraint, with the subsystem that enforces it                                                                                           | 04                 |
| `SG-nn`                                                                                         | Security guarantee, with boundary and verification                                                                                        | 10, 11             |
| `T-nn`, `CTL-nn`                                                                                | Threat, control                                                                                                                           | 11                 |
| `SM-<name>`                                                                                     | State machine                                                                                                                             | 05, 07, 08, 09, 27 |
| `RV-nn`                                                                                         | Finding of the architecture review                                                                                                        | 24                 |
| `IR-nn`                                                                                         | Finding of the independent review                                                                                                         | 25                 |
| `W1`                                                                                            | Invariant of the file replacement protocol                                                                                                | 07                 |
| `VA1`, `VA2`                                                                                    | Invariants of the restoration vault                                                                                                       | 27                 |
| `G-D` … `G-N`                                                                                   | Classes of guarantee of the concurrency contract                                                                                          | 28                 |
| `L1`…`L5`                                                                                       | Ways a security control can be lost to a concurrent write                                                                                 | 28                 |
| `TV-nn`                                                                                         | Item of the technical validation plan                                                                                                     | 30                 |
| `IF-<name>`                                                                                     | Interface contract                                                                                                                        | 05, 06, 07, 09, 27 |
| `OQ-nn`, `DN-nn`                                                                                | Open question, decision needed                                                                                                            | 22                 |
| `F…`, `P…`                                                                                      | Implementation phase, parallel track                                                                                                      | 19                 |
| `TS-<AREA>-nn`                                                                                  | Test specification                                                                                                                        | 20                 |
| `AC-<phase>-nn`                                                                                 | Acceptance criterion                                                                                                                      | 19, 20             |
| `D1`…`D10`                                                                                      | The owner's decisions of 2026-10-12                                                                                                       | ADR 0028           |
| `TI1`…`TI9`                                                                                     | Invariant of the split tree of a terminal tab                                                                                             | 05                 |
| `PS1`…`PS6`                                                                                     | Rule of PTY size for panes                                                                                                                | 05                 |
| `KR1`…`KR6`                                                                                     | Rule of the terminal key map                                                                                                              | 05                 |
| `M01`…`M33`                                                                                     | Performance metric                                                                                                                        | 15                 |
| `PP1`…`PP7`                                                                                     | Performance principle                                                                                                                     | 15                 |
| `PR-nn`                                                                                         | Performance risk found in the existing design                                                                                             | 15                 |
| `DG-nn`                                                                                         | Degradation rule under load                                                                                                               | 15                 |
| `BS-…`, `BC-nn`                                                                                 | Benchmark suite, benchmark corpus                                                                                                         | 15                 |
| `U1`…`U15`                                                                                      | Principle of the interface                                                                                                                | 13                 |
| `S1`…`S19`                                                                                      | Screen of the interface                                                                                                                   | 13                 |
| `ND-nn`                                                                                         | Native dialog of the interface                                                                                                            | 13                 |
| `J1`…`J25`                                                                                      | Core user journey                                                                                                                         | 01 (J1 to J15), 13 |
| `C1`…`C18`                                                                                      | Contradiction found by the coverage audit                                                                                                 | 31                 |
| `X1`…`X14`                                                                                      | Cross-domain gap of the documentation backlog                                                                                             | 31                 |
| `M1`…`M5`, `MC1`…`MC10`, `MV-nn`, `SP1`…`SP5`, `UR1`…`UR7`, `UC1`…`UC6`, `SRC-n`                | Model facts, provider capabilities, visibility values, switching paths, usage rules and capabilities, sources                             | 12                 |
| `IO1`…`IO8`, `RZ1`…`RZ5`, `HS1`…`HS5`, `RA0`…`RA5`, `LP1`…`LP4`, `GR1`…`GR5`, `RT-nn`, `MS-nn`  | Host input and resize rules, service shapes, reachability options, remote profiles and grant rules, remote threats, phone screens         | 16                 |
| `OW1`…`OW12`, `IC1`…`IC3`, `OL0`…`OL4`                                                          | Original Wyss inventory items, incompatibilities, orchestration levels                                                                    | 17                 |
| `PT0`…`PT3`, `PF1`…`PF7`, `PK1`…`PK9`, `OB1`…`OB10`, `CP-nn`, `DR-nn`, `IN1`…`IN6`, `EC1`…`EC5` | Platform tiers, package formats and their requirements, first-run steps, component compatibility, data rows, incidents, extension classes | 18                 |

## Baseline when the set was written, as read without running anything

| Fact                                              | Value                                    | How it was read                                     |
| ------------------------------------------------- | ---------------------------------------- | --------------------------------------------------- |
| Checked-out branch                                | `docs/product-direction-0028`            | `.git/HEAD` `[R]`                                   |
| Its tip                                           | `df6b35b`                                | `.git/refs/heads/docs/product-direction-0028` `[R]` |
| `main`                                            | `1ad90ee`                                | `.git/refs/heads/main` `[R]`                        |
| `origin/main`, as last fetched                    | `1ad90ee`                                | `.git/refs/remotes/origin/main` `[R]`               |
| ADR 0028                                          | Accepted; closes phase F0                | `docs/adr/0028-…md:3-6` `[R]`                       |
| Phase F1                                          | Authorised by the owner, **not started** | `[O]`                                               |
| Whether the working tree is otherwise clean       | NOT VERIFIED                             | Needs `git status`, not run                         |
| Whether the remote has moved since the last fetch | NOT VERIFIED                             | Needs a fetch, not run                              |

### The remote baseline moved (2026-10-13)

`origin/main` advanced from `537eabc` to `91180a6` by one commit that renames the archive of the
original Wyss assistant, `docs origins/` → `wyss-agent-legacy/`, and updates the references to
it. From that commit on, the archive's path is `wyss-agent-legacy/`, and any new text of this
set uses it.

The revisions of this set written on 2026-10-13 were committed on the review branch
`docs/console-planning-review-2026-10-13`, on top of `537eabc`, and `origin/main` at `91180a6`
was merged into that branch the same day, with no conflict. On that branch this index and
documents 16 and 17 show the new path. `main` itself is not changed by the branch: nothing is
merged into it, and the branch is published for review only. The planning is not complete, and
ADR 0029, ADR 0030 and ADR 0031 stay Proposed.

## What this set does not do

It does not authorise any implementation. A phase described in [19](19-IMPLEMENTATION-ROADMAP.md)
starts only when the owner says so. It changes no accepted ADR: where it finds one that should
change, it says so in [21](21-ADR-RECONCILIATION.md) and [22](22-DECISIONS-AND-OPEN-QUESTIONS.md)
and leaves the ADR as it is.

## Owner final-product consolidation — 2026-10-13

The owner subsequently selected product scope and delegated routine design defaults
without accepting any security ADR. Read these documents **after** the historical
baseline and audit:

| Document | Purpose | Authority |
| --- | --- | --- |
| [32](32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md) | Owner-selected DN alternatives; delegated planning candidates; explicit open decisions | Product scope only; no implementation |
| [33](33-CROSS-DOMAIN-RECONCILIATION.md) | Conflict register across security, remote, OL3, memory, models and storage | Candidate design; not independently reviewed |
| [34](34-OL3-ORCHESTRATION-CONTRACT.md) | Finite supervised multi-agent DAG contract, state machines and failure modes | Candidate design for DN-15=OL3 |
| [35](35-HOST-REMOTE-AND-MOBILE-DELTA.md) | Native Android/host/screen model; comparison of transparent P2P/overlay/relay choices | DN-05 remains open |
| [36](36-PLANNING-CLOSURE-AND-VERIFICATION-GATES.md) | Hard blockers, validation and independent-review gates | No passing score or authorisation implied |

[02 §4](02-REQUIREMENTS.md), [20 §14](20-VERIFICATION-AND-ACCEPTANCE.md)
and [23 §5](23-TRACEABILITY-MATRIX.md) cross-reference the **new**
final-product targets with acceptance specifications. The historical 64.2%
coverage figure in 31 predates this consolidation; no new score is
claimed without a new scored audit.

This addendum does not make either of the two independent REJECT reviews
pass. DN-16 and DN-24 remain **not authorised**, even if an owner-selected
target is marked decided. PR #2 stays **draft** and must not be merged
before review and explicit owner approval of the full plan.

### Further contract gap closure

[37](37-HANDOFF-ADAPTERS-UX-AND-DATA-CONTRACTS.md) specifies the
explicit handoff state machine, version-sensitive OpenCode/Hermes
adapter contracts, native hook authoring dialog, native Android
wireframe states and durable model/usage event schema. It is
**proposed documentation**; no provider behaviour or UI has been
verified. The detailed test list was added to [20 §15](20-VERIFICATION-AND-ACCEPTANCE.md)
and the crosswalk to [23 §6](23-TRACEABILITY-MATRIX.md).


### Independent-review handoff

[38](38-INDEPENDENT-REVIEW-HANDOFF.md) is a **read-only review brief**
with critical/high threat questions, required 29-subsystem matrix,
finding format and reject criteria. It is **not an independent review**,
does not alter the two earlier REJECT verdicts and authorises no
provider experiment, ADR acceptance or implementation.

### Source-verification and disposable-environment evidence

[39](39-REMOTE-CANDIDATE-SOURCE-EVIDENCE.md) pins iroh Kotlin/Android
and Tailscale source claims. [40](40-HERMES-BOUNDARY-STATIC-EVIDENCE.md)
inspects concrete Hermes source paths concerning code execution
and approval bypasses, without running Hermes.
[41](41-STATIC-TRACEABILITY-AND-ISOLATED-FILESYSTEM-CHECK.md)
records 410 checked relative links across 41 architecture documents,
40 new requirement/acceptance crosswalks and an isolated
`renameat2` same-mount/cross-mount experiment.
These **limited observations** do not establish reference-device
compatibility, authorise configuration writes or overturn earlier
independent REJECT decisions.

### Post-REJECT safety corrections (2026-10-14)

The author-overlapping review
[reviews/2026-10-13-independent-post-owner-review.md](reviews/2026-10-13-independent-post-owner-review.md)
returned **REJECT**, with 15 findings IR-01–IR-15.
[42](42-POST-REJECT-CORRECTIVE-PLAN.md)
lists remediation and hard stop conditions,
linked to expanded adversarial acceptance
in [20 §16](20-VERIFICATION-AND-ACCEPTANCE.md)
and [23 §7](23-TRACEABILITY-MATRIX.md).
Accepted ADR 0011's same-UID limitation
and ADR 0012/0015 managed-turn limits
remain in force. The reviewer was not
independent from all earlier documents;
a third-party review remains mandatory.

### Codex/Sol High audit of post-REJECT corrections (2026-10-14)

[Review at `141b748`](reviews/2026-10-14-post-reject-corrections-external-audit.md)
returns **REJECT** on the revised document set,
with ER-01..ER-09 (same-UID native/manual/LP4,
final native feature compatibility, terminal input
protocol, managed concurrency, parser failure
domains and original hook-authoring cases).
The [correction register 42](42-POST-REJECT-CORRECTIVE-PLAN.md)
now distinguishes fixed contradictions from actual
security design gates. The Claude report from commit
`344143d` is on a separate branch, **not
an additional open PR**. No full-plan acceptance
or implementation approval follows from any
of these documents.


### Authority isolation feasibility verification (2026-10-14)

[43](43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md) compares a
different-UID protected approval/authority service with
restricted native OL3 workers and a hybrid.
It documents two **isolated synthetic Linux fixtures**:
same-UID access to a mode-0600 socket/file succeeds, while
a different unprivileged UID is denied by mode-0700/0600
DAC. This does **not** validate Wyss provider compatibility,
protected service provisioning, human approval signatures
or production security. No ADR accepted, no code built.
The apparent combination of an unrestricted same-UID agent,
same-UID authority and unforgeable human approvals remains
a hard contradiction until a real distinct trust principal
or effect limitation is designed and independently reviewed.


### Executed synthetic fixture report

[44](44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md)
records precise mock results for different-UID Unix socket
protection, mock signed approvals, replay/expiry/scope
denial across restart, rootless namespace access to
same-host-UID mock credentials, precheck/path-write
symlink race, anchored directory descriptor and
`openat2` escape checks. These are **disposable Linux
fixtures only**, not tests of Wyss, provider login,
Android, human approval or replica/snapshot safety.
Their pass results do not change the current REJECT
or authorise implementation.


### Environment blocker for next isolated run

The mock approval and file-safety observations
[44](44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md)
have reached the tool container's practical
limit. A Landlock availability preflight
returned `ENOSYS`; `bwrap`, QEMU, `cargo`,
Android tooling and a user systemd bus are
unavailable **in that container**. Further
actual environment/credential compatibility
requires a named disposable Linux VM/runner.
Nothing in the tests means the owner's
operating systems are missing these tools.


### Next VM evidence handoff

[45](45-DISPOSABLE-VM-VERIFICATION-HANDOFF.md)
defines a fail-closed set of ten test-only checks
for a separate disposable Linux VM. It forbids
running real providers, reading owner credentials,
changing the host, installing a service on the
owner's PC, accepting an ADR or merging PR #2.
If a verifiably disposable VM is unavailable,
the test agent must stop with a concrete blocker.
This is a *handoff*, not a completed test.


### CachyOS-native verification correction

[46](46-CACHYOS-NATIVE-VERIFICATION-HANDOFF.md) **supersedes** the blanket VM requirement previously written in [45](45-DISPOSABLE-VM-VERIFICATION-HANDOFF.md). Wyss Console targets the owner's **native CachyOS KDE**. Read-only host inspection and unprivileged disposable mock tests can take place on CachyOS under strict no-personal-config/no-root/no-service-change guards. Only genuinely privileged/dangerous experiments need a separate disposable environment; no Ubuntu installation or VM is required for ordinary progress.

### CachyOS-specific Docker and sandbox security correction (2026-10-14)

[47](47-CACHYOS-DOCKER-PRIVILEGE-AND-SANDBOX-DECISION.md)
uses the [native CachyOS report](reviews/2026-10-14-cachyos-native-feasibility.md)
to re-evaluate protected approval authority A,
restricted worker B and remote LP4.
**Landlock ABI 7 did not isolate pathname Unix socket
connections by itself**; minimal bubblewrap mount
views did in synthetic fixtures. Rootful Docker
socket access creates `BLOCKED_ADMIN_DELEGATION`
for claims that a different-UID authority protects
against every unrestricted native/LP4 process.
No host Docker, credential, service or OS setting
was changed. This is a new P0 blocker,
not a solved architecture or a review PASS.

