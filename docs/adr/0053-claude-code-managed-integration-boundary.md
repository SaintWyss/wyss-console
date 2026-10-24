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

## Implementation evidence (2026-10-23) — status stays Proposed

The adapter exists, offline and unqualified. It is the evidence package for Santiago's decision; it does
not accept anything. Measurements and the full E1–E9 table are in the
[feasibility report](../research/claude-managed-feasibility-2026-10-23.md) and the
[acceptance record](../acceptance/m3-engineering-2026-10-23.md).

| Mandatory output        | Engineering result (offline, pinned `claude-agent-sdk==0.2.173` / Claude Code `2.1.309`)                                                                                                                                                                                                        | Still needed                                                     |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| No-tool inference       | `claude_inference_v1`: tools, settings sources, plugins, skills, slash commands and `@path` expansion closed by a frozen option set whose digest is pinned; three independent legs attest it (init inventory, pre-turn queries, request bodies seen by the tap).                                | Real-provider run; accepted auth route.                          |
| Local tool mediation    | `claude_local_tools_v1`: only two authority-owned proposal tools exist; a proposal blocks until the authority decides; only the authority's executor has an effect (bwrap, no network; atomic expected-old-hash writes). Declined, cancelled, failed and indeterminate paths tested; no replay. | Independent review; real-provider run.                           |
| Authentication/settings | Credential never enters the CLI environment (a synthetic key does; a loopback tap swaps it in). Managed policy present → the installation is refused.                                                                                                                                           | **Santiago: which route is supported** (see choices below).      |
| Lifecycle/cancel        | Owned process handle, bounded frames (64 KiB), queues and time, interrupt-vs-cut-off mapping, retry cap 3, whole-tree reap (0 left in 7 runs), cancel p50 10.4 ms.                                                                                                                              | —                                                                |
| Resume/recovery         | Authority-minted session id, resume across processes tested, no prompt/approval replay, ambiguous dispatch is indeterminate.                                                                                                                                                                    | Durable store integration: **ADR0055**.                          |
| Models/effort           | Offered / configured / requested / reported-in-use kept apart; reroute detected from the provider message model. Selection stays gated (ADR0054).                                                                                                                                               | ADR0054 acceptance.                                              |
| Resource/feature parity | 4 processes, about 290–320 MiB PSS, 34 FDs, idle CPU about 3 % (all of it the `claude` process), ready in about 2.2 s (7 runs). Codex comparison not measured.                                                                                                                                  | Codex arm of the comparison (needs an authorized Codex runtime). |

### Owner decision packet

**Decision.** Which authentication route may a Claude Managed runtime use, and may the exact build above be
accepted for the two profiles?

| Choice                                                                      | What it means                                                                                                                                                                    | Risks                                                                                                                                                                                    |
| --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **A. Console API key** held by Wyss, injected only by the tap (recommended) | The adapter as built. A key is stored in the existing credentials store (ADR0038) and reaches the provider only through the tap; the CLI sees a synthetic key. Billed per token. | Per-token cost; the key lives with the authority; needs a real-provider run to prove the 401/529 handling.                                                                               |
| B. Cloud-provider credentials                                               | Same shape with Bedrock/Vertex variables the tap would have to understand.                                                                                                       | Not built; each provider has its own signing, so the tap check on request bodies would need a new design.                                                                                |
| C. Subscription sign-in (claude.ai login)                                   | Reuse the owner's Native login.                                                                                                                                                  | The official overview says third parties cannot offer claude.ai login without approval; would mean copying or re-homing Native credentials, which this ADR forbids. **Not recommended.** |
| D. Do not accept                                                            | Claude Managed stays absent; Native remains the way to use Claude Code.                                                                                                          | Loses the Managed parity goal (M3).                                                                                                                                                      |

**Recommendation: A**, accepted only for the exact build recorded in `adapters/claude/pins.rs`, host
platform and the two profile digests; a later CLI needs a new qualification. **Tests it unlocks:** a real
Inference turn and a real Local Tools turn with an owner-supplied key (each paid, each with an exact
rollback: stop the runtime, delete the key from the store), plus the real-provider part of the adversarial
matrix. **Install/provider effects of accepting:** none by itself; the live gates (`m3::gate`) stay closed
until a release decision, and admission needs ADR0055 as well (a Claude runtime cannot be persisted today).
**Rollback:** disabling new Claude admissions; Native sessions, Codex Managed state and history are untouched.
