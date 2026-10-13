# Wyss Console architecture set: index

Status of this set: **design, not implementation**. Written on 2026-10-12 on the branch
`docs/product-direction-0028` and merged to `main` the same day by the owner's instruction.
Being on `main` makes it available, not accepted: no phase is authorised by it and it has had
no independent review ([24 §14](24-ARCHITECTURE-REVIEW.md)). Nothing in it is built unless a
document says so with a file reference. No command, build or test was run to write it;
whatever needs execution to be known is marked NOT VERIFIED with the procedure that would
settle it.

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

## Documents

| #   | Document                                                               | Subject                                                       |
| --- | ---------------------------------------------------------------------- | ------------------------------------------------------------- |
| 00  | This index                                                             | Map, conventions, baseline                                    |
| 01  | [Product vision](01-PRODUCT-VISION.md)                                 | Identity, problems solved, users, journeys                    |
| 02  | [Requirements](02-REQUIREMENTS.md)                                     | Functional and non-functional requirements, with identifiers  |
| 03  | [Current state and gap analysis](03-CURRENT-STATE-AND-GAP-ANALYSIS.md) | What is implemented, tested, documented, missing              |
| 04  | [Target architecture](04-TARGET-ARCHITECTURE.md)                       | Subsystems A to Q, boundaries, design decisions               |
| 05  | [Terminal and native sessions](05-TERMINAL-AND-NATIVE-SESSIONS.md)     | Terminal engine, agent launcher, supervisor, session registry |
| 06  | [Agent adapters](06-AGENT-ADAPTERS.md)                                 | Provider capability specification and adapter contracts       |
| 07  | [Configuration engine](07-CONFIGURATION-ENGINE.md)                     | Scopes, states, lifecycle of a change, observation            |
| 08  | [Rules, hooks, skills and MCP](08-RULES-HOOKS-SKILLS-MCP.md)           | Every user-visible action on each kind of entry               |
| 09  | [Profiles and orchestration](09-PROFILES-AND-ORCHESTRATION.md)         | Profiles, simultaneous sessions, handoff                      |
| 10  | [Managed runtime and security](10-MANAGED-RUNTIME-AND-SECURITY.md)     | The authority as built, F1b, future managed extensions        |
| 11  | [Security threat model](11-SECURITY-THREAT-MODEL.md)                   | Assets, boundaries, threats, restoration storage, secrets     |
| 12  | [Model management](12-MODEL-MANAGEMENT.md)                             | Catalogues, configured, observed, selection                   |
| 13  | [UX and UI specification](13-UX-UI-SPECIFICATION.md)                   | Screens, states, journeys                                     |
| 14  | [Data, persistence and recovery](14-DATA-PERSISTENCE-AND-RECOVERY.md)  | Ownership of state, crash consistency, rollback               |
| 15  | [Performance engineering](15-PERFORMANCE-ENGINEERING.md)               | Metrics, benchmark procedures, proposed budgets               |
| 16  | [Multi-device and remote](16-MULTI-DEVICE-AND-REMOTE.md)               | Two computers today, remote control later                     |
| 17  | [Wyss Agent and Hermes](17-WYSS-AGENT-AND-HERMES.md)                   | Hermes as a provider, the original Wyss, naming               |
| 18  | [Distribution and extensibility](18-DISTRIBUTION-AND-EXTENSIBILITY.md) | The product for other developers                              |
| 19  | [Implementation roadmap](19-IMPLEMENTATION-ROADMAP.md)                 | Phases, dependencies, definitions of done                     |
| 20  | [Verification and acceptance](20-VERIFICATION-AND-ACCEPTANCE.md)       | Master test plan and acceptance matrix                        |
| 21  | [ADR reconciliation](21-ADR-RECONCILIATION.md)                         | ADRs 0001 to 0028, one by one                                 |
| 22  | [Decisions and open questions](22-DECISIONS-AND-OPEN-QUESTIONS.md)     | What is undecided, what evidence decides it, who decides      |
| 23  | [Traceability matrix](23-TRACEABILITY-MATRIX.md)                       | Requirement → design → component → phase → test               |
| 24  | [Architecture review](24-ARCHITECTURE-REVIEW.md)                       | Self-challenge: contradictions, weaknesses, residual risks    |

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

| Prefix           | Is                                                 | Defined in |
| ---------------- | -------------------------------------------------- | ---------- |
| `FR-<AREA>-nn`   | Functional requirement                             | 02         |
| `NFR-<AREA>-nn`  | Non-functional requirement                         | 02         |
| `AD-nn`          | Architecture design decision of this set           | 04         |
| `C-nn`           | Constraint, with the subsystem that enforces it    | 04         |
| `SG-nn`          | Security guarantee, with boundary and verification | 10, 11     |
| `T-nn`, `CTL-nn` | Threat, control                                    | 11         |
| `SM-<name>`      | State machine                                      | 05, 07, 08 |
| `RV-nn`          | Finding of the architecture review                 | 24         |
| `IF-<name>`      | Interface contract                                 | 05, 06, 07 |
| `OQ-nn`, `DN-nn` | Open question, decision needed                     | 22         |
| `F…`, `P…`       | Implementation phase, parallel track               | 19         |
| `TS-<AREA>-nn`   | Test specification                                 | 20         |
| `AC-<phase>-nn`  | Acceptance criterion                               | 19, 20     |
| `D1`…`D10`       | The owner's decisions of 2026-10-12                | ADR 0028   |

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

## What this set does not do

It does not authorise any implementation. A phase described in [19](19-IMPLEMENTATION-ROADMAP.md)
starts only when the owner says so. It changes no accepted ADR: where it finds one that should
change, it says so in [21](21-ADR-RECONCILIATION.md) and [22](22-DECISIONS-AND-OPEN-QUESTIONS.md)
and leaves the ADR as it is.
