# ADR 0052: Project-centered workspace identities and projections

Status: **Proposed**. Owner WD-02 approves the product direction, not this technical ADR.
Date: 2026-10-22. Extends native project/default/layout semantics of ADR0035/0037;
does not alter Managed authority, leases, database schema or execution permissions.

## Context

Current projects are known native folders/defaults plus a separate configuration selector;
layout is window-centered. Final product needs stable device/host/project/workspace
navigation without changing process ownership or confusing path equality with identity.

## Proposed decision

Introduce opaque ProjectId, RootBindingId and WorkspaceId in a dedicated private
versioned metadata store. A workspace belongs to one project/host; a session keeps its
original host/authority identity. Existing session references are associations, not
execution authority. Multiple workspaces may view a session but share the host's one
input claim. Global configuration/admin and projectless Scratch remain separate.

Backend derives roots from native selection/validated existing binding, captures context
revision on each operation and rejects stale/cross-project mutations. No relink changes
a live session's directory. No imported project/profile implies trust or grants.
Persistence, bounds, migration and failure states are in
[architecture](../product/architecture.md) and the FP-CTX requirements in
[functional specification](../product/functional-specification.md).

## Alternatives and consequences

Keep path-keyed window state: simpler, but aliases/rename/cross-PC grouping remain ambiguous.
Merge everything into Managed SQLite: rejected candidate, as it widens authority data/
responsibility and makes native work depend on it. Use a project projection outside the
authority: recommended; requires consistent association/reconciliation and explicit
mixed-version feature negotiation rather than an assumed cross-owner transaction.

## Acceptance before implementation release

Owner explicitly accepts this ADR after review. Validate exact schema/DTO fixtures,
deterministic legacy mapping, interrupted migration, stale context/project switch,
missing roots, duplicate bindings, protected paths, bounds, newer-store refusal and
rollback preserving live session identities. No source implementation or migration is
performed by proposing this ADR. Product ownership approval is not technical acceptance.
