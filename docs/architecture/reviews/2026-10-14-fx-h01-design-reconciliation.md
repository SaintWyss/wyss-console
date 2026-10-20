# FX-H01 — owner decisions and Codex/Claude design reconciliation

> Integration note (2026-10-19): the dated design and experiment statements below are historical. Only the pure Rust data/validation foundation and test-only filesystem fixtures are being integrated; there is no production B1 launcher, trusted capture, quarantine service or new authority permission. B1 containment and FX-H01 remain NOT_PROVEN. The current owner policy permits ordinary unit fixtures but prohibits disposable HOME, private keyrings/buses, VMs, containers and artificial user installations for development/acceptance. Historical isolated experiment recipes are not current instructions. Accepted authority contracts remain unchanged.

Date: 2026-10-14. **Documentary reconciliation / NOT an independent implementation audit / NOT_ACCEPTED_ADR.**

## 1. Evidence and review provenance

| Source                                                                                                                                                                     | Role / provenance                                                                                         | Evidentiary status                                                                                                                                                                                                                                                                                                                                                                                     |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [Native FX-B1-A..E report at `3ff0d7a`](2026-10-14-fx-b1-composed-native-verification.md)                                                                                  | Codex produced synthetic fixture and verification report                                                  | A **FAIL** via forbidden hardlink, B–E **PASS in explicitly bounded synthetic cases**; no implementation test                                                                                                                                                                                                                                                                                          |
| Original Codex FX-H01 `DECISION_PACKET`, owner-provided on 2026-10-14                                                                                                      | Same experiment author, candidate architecture proponent                                                  | Proposal, **not independent review**; file supplied with SHA-256 `351fce6946b92e923b8a132f67d0b29c4f1e9d004782962e80381d610c13cc10` (as checked against local supplied artifact)                                                                                                                                                                                                                       |
| Claude adversarial `SECURITY_REVIEW_PACKET`, initial pass                                                                                                                  | Different reviewing agent, reported ten F1..F10 candidate risks                                           | First pass lacked original proposal; `INSUFFICIENT_EVIDENCE` about actual candidate, not candidate rejection                                                                                                                                                                                                                                                                                           |
| Claude second `SECURITY_REVIEW_PACKET`, direct candidate comparison                                                                                                        | Reported reading a reconstructed Codex transcript snippet and joined its own architecture recommendations | `REVISE_DESIGN`, recommendation for eight specified revisions, **not tested**. Claude gave reconstructed source hash prefix/suffix `ebc52ba8…f22f`; does not match supplied-artifact identity as reported. **Exact content equality NOT_VERIFIED**, because the reconstructed complete bytes were not provided here; do not claim SHA verification or author independence for the revised architecture |
| Reconciled owner-approved contract [50](../50-B1-STAGED-WORKSPACE-AND-QUARANTINED-RESULT-CONTRACT.md) and plan [51](../51-B1-CONSOLIDATED-ACCEPTANCE-AND-DELIVERY-PLAN.md) | Follow-on synthesis, not an independent reviewer attestation                                              | Planning proposal only; no code, tests, OS or ADR changes                                                                                                                                                                                                                                                                                                                                              |

The exact source file SHA-256 recorded above identifies the full owner-provided Codex artifact. It is not a statement that Claude's reconstructed text was byte-identical; line endings/formatting or substantive differences remain unverified. The major claims cited in Claude's comparison (authorized staging; independent workspace; return broker; AT-04; exclusive auto-apply; deferred auth/egress) occur in the owner-provided packet. **No attempt was made to reconstruct unavailable Claude scratchpad bytes or assert a false digest match.** Another general audit is not needed simply to resolve formatting; independent planning review can use the canonical revised docs in this branch.

## 2. Current security status

- `FX-H01 = OPEN`, legacy composed B1 profile = `FAIL`.
- `FX-B1-A = FAIL`; `FX-B1-B/C/D/E = PASS` only for their bounded synthetic assertions.
- Candidate security design = `REVISE_DESIGN / NOT_PROVEN`, not an accepted ADR.
- Host-global strong approval against unrestricted U/LP4/Docker-root delegation = separately `BLOCKED_ADMIN_DELEGATION + BLOCKED_TRUST_BOUNDARY`.
- No guarantee of real Claude/Codex/Hermes/OpenCode native auth, network or hooks. No provider use, host change or implementation was authorized.

## 3. Frozen owner choices and required nuance

**O-1 ACCEPTED:** B1 results are **quarantined and pending only**. No automatic checkout, Git writeback/merge or working-tree mutation by B1/broker. A later, separate owner U action may integrate after review, but this is outside B1's security assertion.

**O-2 ACCEPTED:** owner explicitly selects repository identity, pinned base commit/OID, exact content inclusion and policy per task. The admission record is the scope of authorization; cryptographic digests/OIDs establish identity, not good intent, and same-UID UX is not protected-human signer A.

**O-3 ACCEPTED, EXTENDED:** permit **staged/unstaged modified tracked files**, deletes, renames and selected untracked inputs **without forcing any commit**. Default extra allowlist is empty; explicit preview/selection of content + typed manifest and snapshot consistency is required. Never silently import all working-tree changes or ignore existing legitimate local modifications.

The B1/U product split was already accepted. U stays native unrestricted, but no host-global containment or nonforgeable managed approval follows.

## 4. F1..F10 disposition against the original candidate and reconciled target

| Finding                                       | Original candidate (Claude's assessment) | Reconciled design treatment / remaining caveat                                                                                                                                                 |
| --------------------------------------------- | ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1 Hardlink writes into other stores          | PARTIALLY_COVERED                        | RV-7: no shared mutable inode with **any** external task/source/cache/toolchain; independently materialize. Must demonstrate CG-03                                                             |
| F2 Broker confused deputy/symlink/Git configs | PARTIALLY_COVERED                        | RV-4/RV-5: frozen restricted exporter, typed bounded output, no traversal from U, no Git with worker-owned config; independent quarantine, delayed-execution classification. CG-05/08/09/10/11 |
| F3 Root cause in admission, not path deny     | COVERED                                  | RV-1/RV-2: authorize content explicitly and eliminate external inode aliases by construction; ownership of admission remains scoped to U-compromise exclusion                                  |
| F4 Shared writable auth                       | OPEN / deferred                          | RV-8: provider-stage gate only (CG-15), no real creds/auto-mount and no real provider compatibility claim                                                                                      |
| F5 Egress/SSRF                                | OPEN / deferred                          | RV-8: provider-stage gate only (CG-16), positive mock egress remains bounded by original C                                                                                                     |
| F6 Worker applies its own Landlock            | COVERED direction                        | RV-6: trusted pre-exec shim, verified ABI/right set/FD closing before untrusted code; CG-07                                                                                                    |
| F7 Overbroad grant                            | PARTIALLY_COVERED                        | RV-6: least privilege and explicit exceptions; prohibit gratuitous FIFO/socket/devices; CG-07/10                                                                                               |
| F8 Freeze/export TOCTOU                       | PARTIALLY_COVERED                        | RV-4: quiesce full process tree; export within its own confinement, no blocking special nodes; CG-10/11                                                                                        |
| F9 Workspace exposed to same-UID watchers     | COVERED for private tmpfs                | RV-7: start with private host-invisible tmpfs; re-review on-disk alternative, including sync/indexer exclusion                                                                                 |
| F10 Inode identity as persistent ID           | COVERED                                  | Hash authenticated snapshot bytes; inode metadata only ephemeral corroboration, never authorization                                                                                            |

Original F1/F2 were adversarial threat scenarios, not observed exploits against an unimplemented candidate. Claim severity/risk separately from confirmation. The single confirmed experimental containment defect remains FX-H01.

## 5. RV-1..RV-8 closure at **documentation specification level only**

| Revision                                                       | Documentary disposition                                                          | Location                       |
| -------------------------------------------------------------- | -------------------------------------------------------------------------------- | ------------------------------ |
| RV-1 Clarify aliases vs copies, rewrite impossible AT-04       | SPECIFIED; no secret-copy oracle claimed                                         | 50 `2; 51 CG-02/04             |
| RV-2 Authoritative owner admission incl. local overlays        | SPECIFIED; implementer must formalize schema and trusted snapshot                | 50 ``1/3; 51 O-2/O-3, CG-04/06 |
| RV-3 Remove auto-apply, refs are not authorization             | SPECIFIED; independent quarantine store, later owner U                           | 50 ``1/3; 51 CG-11             |
| RV-4 Freeze and restricted export; never host Git on B1 `.git` | SPECIFIED; concrete exporter and process supervision still to be designed/tested | 50 `3; 51 CG-08/10             |
| RV-5 Typed paths and delayed-execution inventory               | SPECIFIED; classifier is advisory not approval                                   | 50 `3; 51 CG-09                |
| RV-6 Least privilege, trusted shim/ABI fail closed             | SPECIFIED; exact ABI/final mask remain engineering values                        | 50 `4; 51 CG-07                |
| RV-7 No external mutable inode/caches; private tmpfs candidate | SPECIFIED; actual limits/scale and host proof pending                            | 50 `4; 51 CG-03/14             |
| RV-8 Auth/egress deferred with release gates                   | SPECIFIED, NOT_PROVEN; no real provider use authorized                           | 50 `5; 51 CG-15/16             |

These are **not** “fixed vulnerabilities.” They are completed _documentation corrections_ and open verification obligations. Failure of any required gate keeps the target unproven. No tests were run to make this table.

## 6. Review and authorization handoff

1. Planning owner/architect must turn [50](../50-B1-STAGED-WORKSPACE-AND-QUARANTINED-RESULT-CONTRACT.md) and [51](../51-B1-CONSOLIDATED-ACCEPTANCE-AND-DELIVERY-PLAN.md) into typed interfaces, budgets, negative fixtures and rollout boundaries (P1).
2. A **new independent reviewer** who did not write the FX-B1 fixture, Codex candidate or Claude minimum design must check exact RV-1..8, assumptions and CG-01..14 as a bounded plan review (P2). Separate author and verifier for future implementation.
3. The owner must explicitly authorize coding/privileged testing or real provider usage in later phases; current action covers **documentation only**.
4. Preserve the report `3ff0d7a` unchanged. Do not amend accepted ADRs or `main` as a side effect of this branch.

**Current stop line:** documentation recorded; `FX-H01 OPEN`, `B1 NOT_PROVEN`, `ADR NOT_ACCEPTED`, `implementation NOT_AUTHORIZED`, `provider stage BLOCKED`.
