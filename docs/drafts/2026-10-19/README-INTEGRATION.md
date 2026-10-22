# Wyss Console — consolidated design and documentation package

> **Historical draft pack.** Current scope/status/dependencies are consolidated in
> [docs/product](../../product/README.md). Proposed ADR numbers and “not built” statements
> below are dated; use the [crosswalk/resolutions](../../product/traceability.md).
> Preserve the contracts as inputs, not a second current execution roadmap.

**Prepared:** 2026-10-19
**Git branch:** `docs/wyss-console-planning-2026-10-19`
**Source base:** `main` at `916521c05cb38c292c0d8b15faee68f5bce8fb03`
**Last locally reported development HEAD:** `f1-f2a-configuration` at `5708c09` (not pushed and not accessible from this branch).

## What is done

This branch contains a cohesive, implementation-oriented supplement to the existing 25-document architecture set: remaining-phase specifications, decisions/options, UI journeys, threat model, IPC/data ownership design, migration and rollback, user/operator manuals, performance/distribution, and full acceptance scenarios.

**Design documentation is substantially complete as a proposed package.** It is **not** a claim that the full product is implemented, that local ADR 0029–0031 have been reviewed here, that open owner decisions have been approved or that real-device tests passed. All draft contents are explicitly separate from the accepted ADRs and executable source code.

## Read in this order

| Audience                     | Start with                                                                                                                                                                                                  |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Sonnet merging documentation | [Integration playbook](INTEGRATION-PLAYBOOK.md), [Execution plan](planning/remaining-execution-plan.md), [Acceptance matrix](acceptance/product-completion-matrix.md)                                       |
| Architect                    | [Decision closure pack](architecture/decision-closure-pack.md), [Interface contracts](architecture/cross-phase-interface-contracts.md), [State and recovery](architecture/state-recovery-and-migrations.md) |
| Security reviewer            | [Threat-control matrix](architecture/security-assurance-matrix.md), [E2E test runbook](acceptance/end-to-end-test-runbook.md)                                                                               |
| Product/UI engineer          | [Journeys](ux/target-user-journeys.md), [User guide](manual/user-guide.md)                                                                                                                                  |
| Operations/release           | [Recovery handbook](manual/operator-recovery-handbook.md), [Release guide](manual/installation-release-support.md)                                                                                          |

## File index

### Product planning and acceptance

- [Remaining execution plan](planning/remaining-execution-plan.md) — dependency order, evidence, independent work, decision gates.
- [Product completion matrix](acceptance/product-completion-matrix.md) — reported status and what needs verification.
- [End-to-end tests](acceptance/end-to-end-test-runbook.md) — physical/GUI/provider/fault test procedures and severity.

### Cross-cutting architecture

- [Decisions](architecture/decision-closure-pack.md) — canonical DN-01–20 and owner-only gates, with proposed defaults.
- [Interface contracts](architecture/cross-phase-interface-contracts.md) — host, authority, configuration, model, remote and vault trust ownership.
- [State and migration](architecture/state-recovery-and-migrations.md) — persistence, key loss, conflicts and version compatibility.
- [Threat-control assurance](architecture/security-assurance-matrix.md) — attack classes, mitigation and verification.

### Phase-specific specifications

- [T1/T2/F9](spec/terminal-and-host-acceptance-T1-T2-F9.md) — terminal splits, restore, host persistence and GUI verification.
- [F4/F5](spec/providers-and-authoring-F4-F5.md) — OpenCode, Hermes and executable-content authoring.
- [Provider/version support](spec/provider-compatibility-and-version-policy.md) — verified Claude/Codex versions and capability status.
- [F6/F11a](spec/models-and-handoff-F6-F11.md) — model observations/choices and reviewed handoff.
- [F14](spec/credentials-management-F14.md) — encrypted API key handling and wallet.
- [F10a/b](spec/remote-control-F10.md) — remote **agent/session workspace** and physical phone, not graphical desktop sharing.
- [F1b/G-INF/F8/F11b/F12](spec/managed-inference-and-wyss-agent-F1b-F8-F12.md) — managed isolation and Wyss Agent strategic decision.
- [P1/F13](spec/performance-and-distribution-P1-F13.md) — benchmarks, installer, upgrades, public-release constraints.

### User and operations

- [Target GUI journeys](ux/target-user-journeys.md) — navigable screens, interaction flows, error copy and accessibility.
- [User manual](manual/user-guide.md) — real versus pending GUI behavior and troubleshooting.
- [Operator/recovery handbook](manual/operator-recovery-handbook.md) — lifecycle, backup and failure recovery.
- [Installation/release/support](manual/installation-release-support.md) — reproducibility, migrations, provenance, and rollout.

### Integration

- [Import and reconcile](INTEGRATION-PLAYBOOK.md) — copy the docs into local development, inspect local ADRs first, update canonical docs after acceptance.

## Core rules

1. Canonical architecture, accepted owner requirements, existing security guarantees and later **accepted ADRs** override these drafts.
2. This branch is documentation-only; it must not alter `main`, Sonnet's local branch, provider configs, wallets, host/authority state or network services.
3. Statements based only on owner's report are marked reported, not independently re-run. No invented local commits or test passes.
4. F12's current roadmap deliverable is **a strategy decision**, not an automatically authorized new Wyss Agent implementation.
5. Remote F10 is **control of the remote agent workspace**, not remote graphical desktop access (canonical 16 §3).
6. There is no need to restart the architecture phase: use the existing roadmap to implement, and reconcile the specific known blockers.
7. Public release, remote exposure, actual credentials, irreversible migrations and changing managed authority trust require their specified explicit acceptance.

## What prevents "final accepted documentation" today

- The development branch and ADR 0029–0031 remain local and have not been inspected here.
- T1/T2 are reported coded but still need native GUI acceptance and final README/CHANGELOG/ADR verification.
- Future provider/remote/credential contracts cannot be described as "implemented" before their code and physical/device tests exist.
- Product-owner decisions DN-03/05 remote/06/07/08/09/12/13 remain unaccepted where relevant, and F12 requires live Hermes comparison.
- The final user/installer manuals need exact interface labels, screenshots and tested package/service commands from the shipped build.

These are **verification and decision dependencies**, not gaps requiring a new product redesign. Sonnet should keep implementing while closing them incrementally.

## Next action for local development

Fetch this branch and follow [INTEGRATION-PLAYBOOK.md](INTEGRATION-PLAYBOOK.md). First close T1/T2 GUI acceptance from local `docs/CHECKPOINT.md`, reconcile ADR 0029–0031, then incorporate relevant drafts without re-running the planning process. Do not push/merge local code without the owner's authorization.
