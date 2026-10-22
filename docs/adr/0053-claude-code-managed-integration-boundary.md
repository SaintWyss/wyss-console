# ADR 0053: Claude Code Managed integration boundary

Status: **Proposed; feasibility-gated**. No adapter/SDK/version or authentication route
is accepted by this record. Date: 2026-10-22. Owner WD-01 makes Claude Code first-class
in the target hybrid product; it does not certify Managed support.

## Context and proposal

Claude Code Native already uses its original CLI. Official Agent SDK/headless interfaces
offer candidate structured integration; neither Native launch nor `claude -p` alone
provides Wyss's Managed contract. Add a separate execution adapter/profile only after
the matrix below is proven. Reuse identity/lease/event vocabulary where compatible,
without pretending Codex's wire schema or safety attestation is provider-neutral.

Recommend evaluating a pinned official Agent SDK in an owned supervised process, then
compare bounded CLI streaming if SDK dependencies/footprint are unsuitable. Exact
version, process protocol and supported account/authentication route are research outputs.
Do not copy/re-home credentials, bypass provider permissions or auto-answer native prompts.

## Mandatory feasibility outputs

| Question                | Required proof / refusal                                                                                                                                      |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No-tool inference       | Complete effective settings/tool inventory, no hidden hooks/MCP/plugins/subagent effects; unsupported action fails closed                                     |
| Local tool mediation    | Every command/file effect is held pending, exact complete action bound to generation/turn/lease/fingerprint, one answer only; callback bypass routes excluded |
| Authentication/settings | Official versioned route preserving sign-in/refresh without secret copying; no whole-HOME bind to obtain a pass                                               |
| Lifecycle/cancel        | Owned process handle, bounded initialization/framing/queues, interruption escalation and reaping; no discovered PID signals                                   |
| Resume/recovery         | Correct provider session binding, crash ambiguity and no prompt/approval replay; immutable operation correlation                                              |
| Models/effort           | Capability discovery/provenance, requested versus reported facts, no assumed entitlement; selection needs separate admitted DTO contract                      |
| Resource/feature parity | Positive owner-useful workflow plus negative containment tests; report unsupported originals explicitly, preserve Native alternative                          |

Unknown/malformed events, unmediated effects, auth ambiguity or inability to attest a
profile keeps that profile unavailable. Inference may qualify separately from Local
Tools, but neither interim slice constitutes final Claude parity. No automatic fallback
to Native or Codex. An adapter-level protocol extension is independently reviewed;
do not silently add Claude fields to protocol2's Codex-only classifier.

## Acceptance and consequences

This decision remains Proposed until pinned source/protocol review, positive/negative
conformance fixtures, authorized real-provider compatibility, recovery/resource evidence,
independent security review and Santiago's explicit ADR acceptance exist. If the official
surface cannot preserve necessary functionality safely, return an evidenced product
trade-off, not fabricated support. See [provider matrix](../product/provider-matrix.md)
for fetched official sources and [roadmap M3](../product/roadmap.md#m3--managed-provider-parity-and-model-contract).
