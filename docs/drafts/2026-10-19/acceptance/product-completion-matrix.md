# Wyss Console — Product completion and acceptance matrix (draft)

Reference: canonical `docs/architecture/19-IMPLEMENTATION-ROADMAP.md`, `20-VERIFICATION-AND-ACCEPTANCE.md`, and owner's 2026-10-19 progress reports.
**This is not proof of tests against local HEAD.** The local branch `f1-f2a-configuration`, last reported `5708c09`, was not remotely accessible during preparation.

## Status definitions

- **Not started:** no implementation reported.
- **Implemented:** code reported committed, acceptance may be incomplete.
- **Validated:** automated and real-provider/GUI evidence reported; not necessarily owner-accepted.
- **Acceptance pending:** owner action, ADR or review outstanding.
- **Accepted:** owner signed off canonical acceptance criteria; never infer from an agent self-rating.

## Roadmap coverage

| Phase | Reported implementation | Acceptance / blocker | Next action |
| --- | --- | --- | --- |
| F0 | Accepted per architecture | Reconcile current index | None unless ADR changes |
| F1 | Inventory, 227 real items | Verify local acceptance artifact | Confirm against HEAD |
| F2a | Hooks/skills Claude, undo, KWallet | Owner wallet/UI acceptance pending | Confirm sign-off |
| F2b | Codex MCP/skills switches, rollback | Hooks excluded; running-session reload unknown | Honest UI |
| F3a/b/c | Native launch, multi-agent, profiles | Reconcile ADR 0030 and GUI evidence | Preserve |
| F9 | Host PTYs survive GUI close | Host crash, multi-client, ADR 0031 | Focused smoke |
| T1/T2 | Implemented at reported `5708c09` | GUI walkthrough, renderer search, final suite/docs | Immediate validation |
| P1 | Not started | Reference terminal, OQ-26/27 | Benchmarks |
| F1b | Not started | E1–E8 experiments | Research upon authorization |
| G-INF | Blocked | F1b, accepted ADR, F2b, external review | Preserve deny |
| F4a | Not started | OpenCode versioned contract, DN-04 | Read adapter first |
| F4b | Not started | F4a, Hermes installed with consent, DN-13 | Inspect provider |
| F5 | Not started | F2a, DN-12/13, security review | Native authoring |
| F6a | Not started | F3b, DN-09 | M1–M5 model facts |
| F6b | Not started | F6a/F3c; ADR for managed selection | Native model profiles first |
| F8 | Not started | F6a, DN-08, managed ADR | Concurrency and attestation |
| F10a | Not started | F9, DN-05, threat review | Gateway off by default |
| F10b | Not started | F10a, platform decision | Physical device |
| F11a | Not started | F3c | Reviewed handoff |
| F11b | Not started | F8, per-agent ADR | Additional managed execution |
| F12 | Decision phase not started | F4b, DN-06 | Compare original Wyss/Hermes |
| F13 | Not started | First-user acceptance, DN-07 | Packaging/install decision |
| F14 | Owner requested/approved as future feature | Wallet contract and security review | Protected API Credentials |

## Evidence template

```text
Phase / requirement IDs:
Commit / source revision:
Provider and OS versions:
GUI journey actually performed:
Automated test commands / result:
Fault-injection / security checks:
Performance baseline or caveat:
Real configuration touched? before/after hash:
User data / secret exposure:
Open P0/P1/P2 with reproductions:
External review:
Owner acceptance (date / scope):
Release state (local / merged / packaged / installed):
```

## Product release blockers and limitations

**Blockers:** silently overwritten restorable data; secret in log or UI; authority attestation bypass; unauthorized listener; remote revocation failure; mutation without confirmation; writing unsupported provider formats; falsely alive sessions after host death; reproducible startup crash.

**Limitations to label honestly:** provider does not report in-use model; unverified Codex hook toggle stays read-only; session may not reload settings; unsupported phone platform; no remote before approval; external input latency unmeasured.

Owner decides release scope. A usable local milestone is narrower than the complete product.

## Latest next action

At the last reported local checkpoint `5708c09`, T1/T2 GUI verification and documentation remain. Reconcile this draft against local HEAD and ADR 0029–0031; do not restart previous builds solely because this draft exists.
