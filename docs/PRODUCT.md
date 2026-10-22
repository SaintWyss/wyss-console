# Wyss Console: product direction

Current consolidation: 2026-10-22, source baseline `main` at `e480d479`.
This page records owner scope. The [functional specification](product/functional-specification.md)
defines final behaviour; [implementation status](product/implementation-status.md) says
what exists and what recorded evidence proves. A requirement is not a production
capability, and scope approval is not acceptance of a technical ADR.

## Owner decisions

| ID    | Approved product decision                                                                                   | Consequence                                                                                                                                |
| ----- | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| WD-01 | Full hybrid: Claude Code and Codex first-class, Native and Managed where technically feasible               | Preserve original features through each own Native CLI; qualify Managed profiles separately without weakening security or inventing parity |
| WD-02 | Projects are the primary working context, containing terminals, agents, sessions, profiles and coordination | Device/host → project → workspace → sessions; global configuration/administration remains separately accessible                            |

The target is the **complete personal product**, not an MVP. Previously approved
[owner directions](architecture/32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md) remain:
finite supervised OL3, transparent remote PCs, native Android, real configuration,
optional lingering, faithful terminal reattachment, safe model/effort control and
Wyss policy/effects with Hermes planning behind a boundary. These choices are not
reopened. Public distribution and unattended OL4 remain outside personal scope.

## Product promise

Wyss Console is a fast native terminal and development control centre, useful without
agents or network. It changes real provider files through reviewed operations, shows
global/project/session scope, and separates persisted, provider-resolved and session-
effective facts. File success never proves provider consumption.

Every session identifies host/project/provider/mode, profile and known state. Projects
organize work, not permissions. Switching/closing a view does not stop/restart work.
Reconnect queries surviving owners; dead/ambiguous work is shown honestly and restart
is explicit. Handoff is reviewed context for a new recipient, not private model-state
transfer. OL3 coordinates finite work without becoming an authority.

| Plane   | Execution/configuration                                                                    | Boundary                                                         | Baseline                                                               |
| ------- | ------------------------------------------------------------------------------------------ | ---------------------------------------------------------------- | ---------------------------------------------------------------------- |
| Native  | Original Claude Code/Codex CLI in a real terminal, real files or supported session profile | Provider/user controls; full account rights, not Wyss-mediated   | Both launchers/profiles built; full feature/acceptance closure partial |
| Managed | Provider execution owned by `wyss-control` under an exact attested profile                 | Scoped expiring leases, generation fencing, per-action mediation | Codex implemented; Claude designed/feasibility-gated                   |

An unsupported Managed feature remains a product gap. Native is an explicit alternative
launch, never automatic fallback. Hooks/MCP/skills/subagents cannot silently widen an
existing Managed profile.

## Current baseline

F8 is **production-active** with real Codex 0.159.0, protocol 2, SQLite V8 and
[ADR0051 Accepted](adr/0051-per-session-managed-runtimes.md). Runtime capacity is **one**.
Protocol1/V7 remains available with separate service/store. The
[activation result](acceptance/f8-activation-procedure-2026-10-22.md) supersedes earlier
inactive/Proposed statements. Multi-process credential refresh remains NOT_PROVEN.

F8 completion does not imply concurrent Managed work, Claude Managed or full OL3.
Existing project selection/grouping/defaults are not the project-centered workspace.
Native sessions, host, layout, configuration/restoration, manual handoff and personal
installation have evidence with explicit limits. Remote/client/Android, faithful screen
recovery and complete performance/accessibility acceptance remain in the roadmap.

## Documents and authority

1. This page and owner records define product scope.
2. [docs/product](product/README.md) consolidates final behaviour, architecture,
   capabilities, gaps, roadmap and traceability; unchanged detailed contracts remain linked.
3. Accepted [ADRs](adr/) govern technical/security decisions. Product goals cannot
   supersede them. New ADR0052/0053 remain Proposed.
4. [README](../README.md), source and dated acceptance describe actual implementation,
   each at its stated version/slice.
5. Historical architecture/drafts/research and `wyss-agent-legacy/` remain inputs;
   dated present-tense statements are not a second current backlog.

The prior [product snapshot](history/PRODUCT-before-phase-1-2026-10-22.md) is retained.
No technical ADR, service action, migration or credential use is approved by this phase.
Unresolved owner choices have alternatives/recommendations in the
[roadmap](product/roadmap.md#3-outstanding-owner-decisions-and-technical-discovery).
