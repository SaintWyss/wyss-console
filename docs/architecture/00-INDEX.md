# Wyss Console architecture set: index

Status of this set: **design, not implementation, and not accepted**. Written on 2026-10-12 and
merged to `main` the same day by the owner's instruction; revised the same day, in the working
tree, after the owner's policy on secrets, a proposed ADR 0029 and one independent review whose
verdict was **reject** ([25](25-INDEPENDENT-REVIEW.md)). Revised again on 2026-10-13, after the
owner decided that a protected private backup is allowed: ADR 0029 was redrafted, ADR 0030 was
proposed, and documents 27 to 29 were added. Neither revision has been reviewed by anyone but
its author. No phase is authorised by this set, and none is authorised
at all: implementation waits for the owner's word ([22](22-DECISIONS-AND-OPEN-QUESTIONS.md),
DN-16). How far the design really is: [26](26-READINESS-ASSESSMENT.md).

Nothing in it is built unless a document says so with a file reference. No application code,
build or test was run to write it; whatever needs execution to be known is marked NOT VERIFIED
with the procedure that would settle it.

## How to read it

| If you want                                   | Read                                                                        |
| --------------------------------------------- | --------------------------------------------------------------------------- |
| What the product is and for whom              | [01](01-PRODUCT-VISION.md)                                                  |
| What it must do, with identifiers             | [02](02-REQUIREMENTS.md)                                                    |
| What exists today and what is missing         | [03](03-CURRENT-STATE-AND-GAP-ANALYSIS.md)                                  |
| How the whole system is divided               | [04](04-TARGET-ARCHITECTURE.md)                                             |
| The central feature (real configuration)      | [07](07-CONFIGURATION-ENGINE.md), [08](08-RULES-HOOKS-SKILLS-MCP.md)        |
| What may never be weakened                    | [10](10-MANAGED-RUNTIME-AND-SECURITY.md), [11](11-SECURITY-THREAT-MODEL.md) |
| In which order to build, and how to accept it | [19](19-IMPLEMENTATION-ROADMAP.md), [20](20-VERIFICATION-AND-ACCEPTANCE.md) |
| What is still undecided                       | [22](22-DECISIONS-AND-OPEN-QUESTIONS.md)                                    |
| Where this design is weak                     | [24](24-ARCHITECTURE-REVIEW.md)                                             |
| What an independent reader found              | [25](25-INDEPENDENT-REVIEW.md)                                              |
| How ready it is, honestly                     | [26](26-READINESS-ASSESSMENT.md)                                            |

## Documents

| #   | Document                                                                            | Subject                                                                                                                                                                       |
| --- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 00  | This index                                                                          | Map, conventions, baseline                                                                                                                                                    |
| 01  | [Product vision](01-PRODUCT-VISION.md)                                              | Identity, problems solved, users, journeys                                                                                                                                    |
| 02  | [Requirements](02-REQUIREMENTS.md)                                                  | Functional and non-functional requirements, with identifiers                                                                                                                  |
| 03  | [Current state and gap analysis](03-CURRENT-STATE-AND-GAP-ANALYSIS.md)              | What is implemented, tested, documented, missing                                                                                                                              |
| 04  | [Target architecture](04-TARGET-ARCHITECTURE.md)                                    | Subsystems A to Q, boundaries, design decisions                                                                                                                               |
| 05  | [Terminal and native sessions](05-TERMINAL-AND-NATIVE-SESSIONS.md)                  | Terminal engine, agent launcher, supervisor, session registry                                                                                                                 |
| 06  | [Agent adapters](06-AGENT-ADAPTERS.md)                                              | Provider capability specification and adapter contracts                                                                                                                       |
| 07  | [Configuration engine](07-CONFIGURATION-ENGINE.md)                                  | Scopes, states, lifecycle of a change, observation                                                                                                                            |
| 08  | [Rules, hooks, skills and MCP](08-RULES-HOOKS-SKILLS-MCP.md)                        | Every user-visible action on each kind of entry                                                                                                                               |
| 09  | [Profiles and orchestration](09-PROFILES-AND-ORCHESTRATION.md)                      | Profiles, simultaneous sessions, handoff                                                                                                                                      |
| 10  | [Managed runtime and security](10-MANAGED-RUNTIME-AND-SECURITY.md)                  | The authority as built, F1b, future managed extensions                                                                                                                        |
| 11  | [Security threat model](11-SECURITY-THREAT-MODEL.md)                                | Assets, boundaries, threats, restoration storage, secrets                                                                                                                     |
| 12  | [Model management](12-MODEL-MANAGEMENT.md)                                          | Catalogues, configured, observed, selection                                                                                                                                   |
| 13  | [UX and UI specification](13-UX-UI-SPECIFICATION.md)                                | Screens, states, journeys                                                                                                                                                     |
| 14  | [Data, persistence and recovery](14-DATA-PERSISTENCE-AND-RECOVERY.md)               | Ownership of state, crash consistency, rollback                                                                                                                               |
| 15  | [Performance engineering](15-PERFORMANCE-ENGINEERING.md)                            | Metrics, benchmark procedures, proposed budgets                                                                                                                               |
| 16  | [Multi-device and remote](16-MULTI-DEVICE-AND-REMOTE.md)                            | Two computers today, remote control later                                                                                                                                     |
| 17  | [Wyss Agent and Hermes](17-WYSS-AGENT-AND-HERMES.md)                                | Hermes as a provider, the original Wyss, naming                                                                                                                               |
| 18  | [Distribution and extensibility](18-DISTRIBUTION-AND-EXTENSIBILITY.md)              | The product for other developers                                                                                                                                              |
| 19  | [Implementation roadmap](19-IMPLEMENTATION-ROADMAP.md)                              | Phases, dependencies, definitions of done                                                                                                                                     |
| 20  | [Verification and acceptance](20-VERIFICATION-AND-ACCEPTANCE.md)                    | Master test plan and acceptance matrix                                                                                                                                        |
| 21  | [ADR reconciliation](21-ADR-RECONCILIATION.md)                                      | ADRs 0001 to 0028, one by one                                                                                                                                                 |
| 22  | [Decisions and open questions](22-DECISIONS-AND-OPEN-QUESTIONS.md)                  | What is undecided, what evidence decides it, who decides                                                                                                                      |
| 23  | [Traceability matrix](23-TRACEABILITY-MATRIX.md)                                    | Requirement → design → component → phase → test                                                                                                                               |
| 24  | [Architecture review](24-ARCHITECTURE-REVIEW.md)                                    | Self-challenge: contradictions, weaknesses, residual risks                                                                                                                    |
| 25  | [Independent review](25-INDEPENDENT-REVIEW.md)                                      | One cold review by another reader, and what was done about it                                                                                                                 |
| 26  | [Readiness assessment](26-READINESS-ASSESSMENT.md)                                  | What is complete, unverified, partial or blocked; work remaining                                                                                                              |
| 27  | [Protected restoration vault](27-PROTECTED-RESTORATION-VAULT.md)                    | The encrypted store that keeps a disabled entry restorable: records, key, consent, lifecycle, threats                                                                         |
| 28  | [Concurrency and filesystem integrity](28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md)  | What can and cannot be promised when a provider writes the same file; behaviour case by case                                                                                  |
| 29  | [Capability preservation](29-CAPABILITY-PRESERVATION.md)                            | Every capability of the final product against the security decisions: kept, blocked, or not designed enough                                                                   |
| 30  | [Technical validation plan](30-TECHNICAL-VALIDATION-PLAN.md)                        | What must be observed or tested before the vault and the write protocol can be relied on; the results of the one inspection run so far (2026-10-13) and the blockers it found |
| —   | [The reviewer's report](reviews/2026-10-12-independent-review-report.md)            | The independent review, unedited                                                                                                                                              |
| —   | [The focused security review](reviews/2026-10-13-focused-security-review-report.md) | The second independent review, of the vault and the write protocol: **reject**, eleven findings; unedited                                                                     |
| —   | [ADR 0029](../adr/0029-secrets-restoration-data-and-configuration-persistence.md)   | **Proposed**, second draft: secrets, restoration data, the vault, persistence                                                                                                 |
| —   | [ADR 0030](../adr/0030-corrections-to-adr-0028.md)                                  | **Proposed**: corrections to two security statements of ADR 0028                                                                                                              |

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

| Prefix           | Is                                                        | Defined in         |
| ---------------- | --------------------------------------------------------- | ------------------ |
| `FR-<AREA>-nn`   | Functional requirement                                    | 02                 |
| `NFR-<AREA>-nn`  | Non-functional requirement                                | 02                 |
| `AD-nn`          | Architecture design decision of this set                  | 04                 |
| `C-nn`           | Constraint, with the subsystem that enforces it           | 04                 |
| `SG-nn`          | Security guarantee, with boundary and verification        | 10, 11             |
| `T-nn`, `CTL-nn` | Threat, control                                           | 11                 |
| `SM-<name>`      | State machine                                             | 05, 07, 08, 09, 27 |
| `RV-nn`          | Finding of the architecture review                        | 24                 |
| `IR-nn`          | Finding of the independent review                         | 25                 |
| `W1`             | Invariant of the file replacement protocol                | 07                 |
| `VA1`, `VA2`     | Invariants of the restoration vault                       | 27                 |
| `G-D` … `G-N`    | Classes of guarantee of the concurrency contract          | 28                 |
| `L1`…`L5`        | Ways a security control can be lost to a concurrent write | 28                 |
| `TV-nn`          | Item of the technical validation plan                     | 30                 |
| `IF-<name>`      | Interface contract                                        | 05, 06, 07, 09, 27 |
| `OQ-nn`, `DN-nn` | Open question, decision needed                            | 22                 |
| `F…`, `P…`       | Implementation phase, parallel track                      | 19                 |
| `TS-<AREA>-nn`   | Test specification                                        | 20                 |
| `AC-<phase>-nn`  | Acceptance criterion                                      | 19, 20             |
| `D1`…`D10`       | The owner's decisions of 2026-10-12                       | ADR 0028           |

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
ADR 0029 and ADR 0030 stay Proposed.

## What this set does not do

It does not authorise any implementation. A phase described in [19](19-IMPLEMENTATION-ROADMAP.md)
starts only when the owner says so. It changes no accepted ADR: where it finds one that should
change, it says so in [21](21-ADR-RECONCILIATION.md) and [22](22-DECISIONS-AND-OPEN-QUESTIONS.md)
and leaves the ADR as it is.
