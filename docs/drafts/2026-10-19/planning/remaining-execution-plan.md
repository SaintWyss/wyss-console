# Remaining product execution plan (proposed)

Status: **Working draft, not an ADR**
Canonical dependency source: `docs/architecture/19-IMPLEMENTATION-ROADMAP.md` (§§2–5).
Basis for current state: owner's reports through local HEAD `5708c09` (2026-10-19); **not inspected on disk here**.

## 1. Project objective and completion semantics

Wyss Console is a native local application with separate **Native** and **Managed** execution planes. Finishing a coding phase is not equivalent to a working product. A phase may be `planned`, `implemented`, `validated`, `owner-accepted` or `integrated`; do not conflate these.

Completion requires a real GUI workflow, provider evidence when applicable, regression tests, honest failure states, reproducible installation/rollback, preserved security and user data. Unverified provider behavior must remain **unsupported/unverified**, never simulated.

## 2. Reported state versus remaining evidence

| Workstream | Owner-reported state | Evidence to reconcile locally |
| --- | --- | --- |
| F1 | Configuration inventory, 227 items | Acceptance and redacted projection |
| F2a | Claude hooks/skills, encrypted undo | ADR 0029 and wallet/UI limits |
| F2b | Codex MCP/skills/rollback | No individual hooks; versioned contract |
| F3a–c | Launcher, registry and profiles | ADR 0030 and isolation |
| F9 | Host-owned PTYs survive window closing | ADR 0031, host crashes/multi-client |
| T1/T2 | Implemented, partially tested | **Next:** GUI walkthrough, renderer search test, full suites, docs |
| P1 | Not started | Reference terminal, OQ-26/27 |
| F1b/G-INF | Not started | E1–E8 and an accepted security ADR |
| F4–F6, F8, F10–F14 | Pending in reported snapshot | Phase-specific contracts and decisions |

**Do not silently mark owner acceptance.**

## 3. Dependencies and execution order

The canonical DAG controls prerequisites, not numerical order.

1. **Close T1/T2 verification**, without redesign: split, drag, focus, shortcut non-interference, scrollback search, close/reopen and exact layout. Add missing renderer search test and reconcile README/CHANGELOG/ADR.
2. **P1** in a separate workstream: measure Wyss on actual hardware. If reference terminal approval is missing, collect Wyss-only measurements and mark comparison pending.
3. **F6a → F6b** after F3: distinguish configured, selected and *observed* model. Native profile flags may choose a start model; managed selection requires its own authority ADR.
4. **F11a** after F3c: a sanitized, reviewed handoff package must never imply lossless transfer of private model context.
5. **F14** builds on ADR 0029's encrypted restoration ideas but needs separate key-injection consent and threat review; no direct authority secrets.
6. **F4a → F4b**: OpenCode versioned adapter before Hermes; inspect/install Hermes only with consent. F12 is a *decision phase*, not implicit implementation of a new agent.
7. **F5** after F2a and DN-12/13: executable-content authoring requires a dedicated confirmation boundary and security review.
8. **F1b → DN-03/ADR → G-INF**: E1–E8 in disposable provider homes; keep managed security controls until proven safe and reviewed.
9. **F8 → F11b** after F6a and DN-08: preserve attestation and fault isolation per generation/managed runtime.
10. **F10a → F10b** after F9 and DN-05: gateway off by default; no public listener or real credentials without owner authorization. Phone follows tested desktop remote.
11. **F13** after first-user acceptance and DN-07: local artifacts may precede publication; updates, signing and public release require separate decisions.

Avoid concurrent edits to the same authority/writer/state store. Independent docs, fixtures and UI contracts can progress in parallel with separate commits.

## 4. Acceptance contract for each deliverable

Each phase handoff states:
- The exact user journey, provider and version verified.
- Commit, migration, capabilities and rollback.
- Focused tests, integrated regression, native GUI evidence and resources.
- Security boundary; information never logged or stored.
- Unsupported/unverified behavior labelled.
- Updated spec, README, CHANGELOG, roadmap status and evidence file.
- Precisely what owner action remains; continue unrelated eligible work meanwhile.

## 5. Decision register — recommendations, not approvals

| Decision | Recommended candidate | Limit before acceptance |
| --- | --- | --- |
| DN-03/G-INF | Based on E1–E8, preserve current block until safe | No Codex hook mutations |
| DN-04/F4a | Comment-preserving JSONC only if required by real OpenCode files | No lost comments |
| DN-05/F10 | Separate long-lived gateway, private overlay preferred | No remote endpoint |
| DN-06/F12 | Compare installed Hermes and original Wyss | No unapproved agent strategy |
| DN-07/F13 | Linux/systemd first | No updater or public release |
| DN-08/F8 | Isolated runtime per managed session if measurements justify | No implicit multi-runtime redesign |
| DN-09/F6a | Transient observed model only when provider reports it | Never guess in-use model |
| DN-12/F5 | Native dialog for executable commands, external editor fallback | No free-form executable command from webview |
| DN-13/F4b/F5 | Closed verified provider commands only | No arbitrary CLI |
| OQ-26/27/P1 | Debug-only timestamp probe, repeatable external latency test | No unsupported performance claims |
| F10b platform | Choose after F10a with physical-device evidence | No mobile release |

Owner already approved DN-01 encrypted secret-bearing restoration (reported ADR 0029) and DN-02 shell/exec launcher (reported ADR 0030). Confirm exact accepted text in local HEAD.

## 6. Risk-focused testing

- Pure UI: targeted component tests and GUI smoke; full suites at milestone.
- Filesystem/writer/keys: byte-identical round-trip, races, crash/undo and secrets corpus.
- Managed authority: all attestations, complete authority suite, independent security review.
- Remote/credentials: revocation, failure injection, leaks and real end-to-end acceptance before deployment.
- Performance: before/after median and p95 under comparable conditions.

A sandbox-induced failure is not a code defect; an unrun test is not a pass.

## 7. Product-level acceptance milestones

- **Local daily driver:** native terminals, profiles, CLI integration, configuration, host persistence, T1/T2 accepted.
- **Integrated workspace:** models, handoff, authoring and credential UX without leakage.
- **Connected workspace:** owner-authorized desktop gateway and then phone pairing/revocation.
- **Distribution:** installation, migration, uninstall, signing/provenance after DN-07, known limitations and security contact.

No early milestone implies the complete roadmap is done.

## 8. Handoff for Sonnet

Resume the **existing** development branch; first validate T1/T2 according to `docs/CHECKPOINT.md`. Reconcile these proposed specs against local ADR 0029–0031 and current code before transferring them to canonical documents. Do not push/merge/publish merely because these drafts exist.
