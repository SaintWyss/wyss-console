# ADR 0052: Project-centered workspace identities and projections

Status: **Accepted** by Santiago on 2026-10-22, following explicit authorization to complete the pending ADR0052 acceptance in the PR #17 discussion. The earlier WD-02 product direction alone did not accept this ADR.
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

## Acceptance and prerequisites before implementation release

This acceptance is limited to the **architectural decision**: a dedicated versioned
project/workspace metadata projection outside Native and Managed execution authorities,
with stable opaque identities, checked owner association and stale-context refusal. It
does not certify or authorize any migration, installed change, release, service action,
credential access, host/authority restart, runtime-capacity expansion, or claim of
OS-level isolation. ADR0053 remains Proposed. The explicit detailed M1 contract and
13 new acceptance oracles remain proposed engineering details, not executable proof.

Before M1 can be adopted on the owner's desktop, validate exact schema/DTO fixtures,
deterministic legacy mapping, interrupted migration, stale context/project switch,
missing roots, duplicate bindings, protected paths, bounds, newer-store refusal and
rollback preserving live session identities. No source implementation or migration is performed by accepting the architecture.
All M1-01..13 tests remain NOT_RUN until implementation; negative cases, interrupted
migration, rollback, UI/runtime identity preservation and real-desktop resource budgets
must pass before production adoption. Any material alteration to the accepted trust or
ownership boundaries requires a new review/ADR.
