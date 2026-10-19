# Decision closure pack — design defaults, evidence and approval boundaries

**Status:** design proposals; **not** accepted ADRs. Consult canonical [architecture 22](../../../architecture/22-DECISIONS-AND-OPEN-QUESTIONS.md) for complete options, owner rights and OQ IDs. This pack makes the implementation choices explicit so routine work can continue without redesigning them. Local HEAD `5708c09` was reported, **not read** here.

## Hierarchy and reconciliation

Owner product intent/requirements → accepted security ADRs → implementation contracts and observed behavior → these proposals. Do not treat being pushed to a docs branch as a security approval. Current branch from public GitHub base `916521c`; local ADR 0029, 0030, 0031 and checkpoints supersede older baseline status.

## Decisions already reported by the owner

| Decision | Reported resolution | Required local evidence |
| --- | --- | --- |
| DN-01 | Encrypted-at-rest restoration with OS key storage, refuse unsafe persistence | ADR 0029, wallet behavior |
| DN-02 | Native shell launcher with exec replacement | ADR 0030, real Claude/Codex smoke |
| DN-05 **host part** | Separate host process owns PTYs/sessions | ADR 0031, F9 GUI close/reopen |
| DN-19 | Codex individual hooks remain read-only until G-INF | F2b adapter/UI and accepted ADR |
| DN-20 (profile controls) | First-use native confirmation for risk controls per canonical recommendation | Check actual ADR 0030, not inferred as accepted |

These are **reported**, not independently inspected on the local branch; do not recast any unverified row as approved.

## Recommended decision set for remaining phases

| ID | Suggested decision for implementation proposal | Evidence and acceptance condition | If unresolved |
| --- | --- | --- | --- |
| DN-03 | Prefer I4 (private namespace + host policy visible); only if E1–E8 prove safety | Canonical 10 §5.6/5.7 + independent review; owner ADR | Keep managed Inference hook block |
| DN-04 | Use existing bounded own-search implementation for T1 if GUI verifies; JSONC editing library only if needed, preserving comments | T1 renderer tests; OpenCode real file fixture, licensing | Don't add dependency on speculation |
| DN-05 **remote part** | Separate gateway on owner-approved private overlay; restricted device capabilities | Threat model, device revocation and real two-PC smoke; owner ADR | Gateway stays off |
| DN-06 | No choice between Hermes/original Wyss/fork before live comparison | Hermes installed only with consent, original Wyss code + threat comparison | F12 remains decision, no invented agent |
| DN-07 | First support Linux/systemd, user-level install and offline-by-default | License/platform/update support, signed provenance and fresh install acceptance | Local build only, no publication |
| DN-08 | Prefer isolation per managed runtime generation; optimize based on measured concurrency | Provider protocol, memory/CPU, security ADR and fault tests | No F8 change inside authority |
| DN-09 | Observed model transient unless durable per-turn record has clear need | Actual provider report, no invented in-use model; owner ADR | Present configured/selected only |
| DN-10 | Static Codex evidence from files by default | Any live provider-effective read requires explicit ADR | Predictive label only |
| DN-11 | Wyss Console product name; Wyss Agent remains separate | Canonical 17 §1 / owner naming | Don't conflate authority and agent |
| DN-12 | Dedicated native editing dialog for executable hooks; no webview-authored free command | Threat review, keyboard/accessibility, confirm transcript | Offer read-only control |
| DN-13 | Closed, fixed-argument provider own-writer commands after version verification | Provider behavior fixture, no arbitrary shell strings, own ADR | Manual provider edit outside Console |
| DN-14 | Preserve native provider sign-in via flags/overlays, no credential-home copy | F3c real acceptance | Refuse unsupported isolated profile |
| DN-15 | No automatic orchestration until reviewed manual F11a handoff is useful | Multi-agent cost and safety measures | Manual handoff only |
| DN-16 | F1 already owner-authorized and reported built | Local evidence | No reopening |
| DN-17 | Canonical architecture merged to main by owner; this branch adds drafts only | Base Git history | No automatic merge |
| DN-18 | Last-seen index stores hashes/identifiers; not undisabled content | Real schema, owner acceptance | No implicit restore for outside deletion |
| DN-19 | Neither direction for Codex individual hooks prior to G-INF | Real provider/version | Read-only |
| DN-20 | Native dialog on first risky profile version, generated overlays in secure runtime state | ADR 0030, real profile smoke | Block risky launch |
| F10b platform | Android candidate based on owner's prior Wyss ecosystem; product needs explicit platform choice | Current supported environment, physical device, store constraints | Desktop F10a first |
| F14 key storage | OS secret store + AEAD sealed metadata; locked wallet fails closed | ADR 0029 extension, threat review, import/rotation tests | No raw credential store |

## Owner-only gates versus routine implementation

**Autonomous engineering:** tests, schema drafts, feature-limited UI, reversible fixture migration, perf scripts, formatters, closed error wording and routine dependency fixes.

**Owner decision/action:** irreversible real-data deletion; new trust policy; accepting authority G-INF/F8 changes; opening remote network services and enrolling real devices; accessing actual API credentials; installing third-party binaries; choosing public license/signing/platform/release; publishing packages.

Previous owner authorization to build the entire roadmap permits work but **does not assert that a security experiment passed** or that real network/credential interactions were approved. Continue independent tasks rather than inventing approval.

## Decision record template

```
ID:
Status: proposed | accepted | rejected | superseded
Date and author:
Context and product requirement:
Options with operational / security / performance tradeoffs:
Chosen option and why:
Affected trust boundary / authority guarantee:
Schema and compatibility implications:
Evidence (version / test / sample / source commit):
Rollback and failure behavior:
Owner approval evidence, if required:
ADR and phase links:
```

## Reconciliation to-do

- Fetch local ADR 0029–0031, `docs/CHECKPOINT.md`, F9/T1/T2 acceptance and actual roadmap updates.
- Close differences in DN-05 host vs remote, DN-20 and storage key management **only using accepted local evidence**.
- Mark each accepted decision in canonical 22 with a link to its actual ADR, not this pack.
- Keep this draft branch independent; Sonnet integrates selected documents after review.
