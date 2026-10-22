# Wyss Console: product vision and requirements

Wyss Console is Santiago's native personal development workspace: a useful terminal
without agents or network, real provider configuration and supervised agent work.
The goal is the **complete personal product**, not an MVP.

## Approved owner decisions

| ID    | Product requirement                                                                                                                                                                                               |
| ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| WD-01 | Full hybrid: Claude Code and Codex are first-class in Native and Managed modes where technically feasible; preserve original capabilities and distinct security boundaries                                        |
| WD-02 | Projects are the primary working context: device/host → project → workspace → sessions, with terminals, agents, profiles and work coordination; global configuration/administration remains separately accessible |

Retained scope includes finite supervised OL3, remote PCs, native Android, optional
lingering, faithful terminal reattachment, model/effort controls and Wyss policy/effects
with Hermes planning behind a boundary. Public distribution and unattended OL4 remain
outside personal scope. An IDE, teams/multi-tenant accounts and desktop screen sharing are also out
of scope. The [original owner record](architecture/32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md)
provides decision provenance, not current implementation status.

## Requirements

The 31 `FP-*` requirements and matching acceptance criteria are defined once in the
[functional specification](product/functional-specification.md); their
[crosswalk](product/traceability.md) preserves all 208 legacy FR/NFR obligations.
Changes to scope must update those definitions and mappings, not add a competing list.

Use [implementation status](product/implementation-status.md) for delivered capability
and evidence, [architecture](product/architecture.md) for technical/security constraints,
and [roadmap](product/roadmap.md#3-outstanding-owner-decisions-and-technical-discovery)
for dependencies and unresolved owner choices. Product approval does not accept an ADR
or authorize weakening its boundary. Navigation and document ownership live in the
[documentation map](product/README.md).
