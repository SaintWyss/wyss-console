# Wyss Console — documentation handoff

**Prepared:** 2026-10-19
**Status:** Proposed documentation package on an independent Git branch; **not** an accepted ADR.
**Canonical source read:** `SaintWyss/wyss-console`, Git commit `916521c05cb38c292c0d8b15faee68f5bce8fb03`.
**Latest local development status reported by owner:** `f1-f2a-configuration`, HEAD `5708c09` (not remotely available here).

## Purpose

Complete actionable specification and execution planning for the remaining roadmap without disrupting Sonnet's implementation. The documents expand existing architecture rather than replace it. A statement marked *candidate* or *proposal* is not an approved product decision.

## Contents

1. `docs/planning/remaining-execution-plan.md` — delivery order, dependencies, acceptance gates and open decisions.
2. `docs/spec/credentials-management-F14.md` — protected API Credentials feature and secure storage.
3. `docs/spec/remote-control-F10.md` — gateway, pairing, desktop and phone.
4. `docs/spec/models-and-handoff-F6-F11.md` — model facts and explicit handoff.
5. `docs/spec/providers-and-authoring-F4-F5.md` — OpenCode, Hermes, authoring.
6. `docs/spec/managed-inference-and-wyss-agent-F1b-F8-F12.md` — isolation, scale, Wyss Agent.
7. `docs/spec/performance-and-distribution-P1-F13.md` — measurement, packaging, rollout.
8. `docs/spec/terminal-and-host-acceptance-T1-T2-F9.md` — GUI acceptance for terminal and host.
9. `docs/acceptance/product-completion-matrix.md` — full delivery acceptance matrix.

## Integration instructions for Sonnet

1. Verify local HEAD, `docs/CHECKPOINT.md`, ADR 0029–0031 and current acceptance reports. **Latest local code and accepted ADRs supersede these drafts.**
2. Reconcile each proposal against architecture 07, 11–22, ADR 0028 and later ADRs.
3. Transfer only non-conflicting detail into canonical specifications. These drafts are documentation proposals, not implementation authority.
4. Run formatting, link/reference checks and the repository credential scan before integrating.
5. Do not interpret open decisions as permission to change real provider settings, expose network listeners, collect credentials, publish or alter authority security contracts.

## Evidence and confidence

- **Reported:** progress relayed by Santiago; not independently inspected at local HEAD.
- **Canonical:** requirement present in published architecture.
- **Proposed:** detail requiring engineering review or owner acceptance.
- **Blocked:** explicit requirement/prerequisite not yet satisfied.

## Non-goals

No code changes, no modifications to provider configuration, no new ADR marked accepted and no claims that pending features are implemented. This branch is deliberately separate from the development branch.