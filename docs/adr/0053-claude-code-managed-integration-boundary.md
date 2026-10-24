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

## Implementation evidence (2026-10-23, revised after review) — status stays Proposed

The adapter exists, offline and unqualified. It is the evidence package for Santiago's decision; it does
not accept anything. The full E1–E9 table is in the
[feasibility report](../research/claude-managed-feasibility-2026-10-23.md), the measurements and the review
findings in the [acceptance record](../acceptance/m3-engineering-2026-10-23.md).

| Mandatory output        | Engineering result (offline, pinned `claude-agent-sdk==0.2.173` / Claude Code `2.1.309`)                                                                                                                                                                                                                                                                                     | Still needed                                                                                            |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| No-tool inference       | `claude_inference_v1`: tools, settings sources, plugins, skills, slash commands and `@path` expansion closed by a frozen option set whose digest is pinned. The model is offered no tool; that is checked by the bridge's tap **and again by the credential broker outside the sandbox**, on every request.                                                                  | Real-provider run.                                                                                      |
| Local tool mediation    | `claude_local_tools_v1`: only two authority-owned proposal tools; a proposal blocks until the authority decides; only the authority's executor has an effect (bwrap, no network; atomic expected-old-hash writes). Declined, cancelled, failed and indeterminate paths tested; no replay.                                                                                    | Independent review; real-provider run.                                                                  |
| Authentication/settings | **Not decided; see [ADR0056](0056-claude-credential-delivery.md).** This build has no source of a real credential. When one exists it goes only to the broker's stdin; the bridge and the CLI never hold it. Managed policy present → the installation is refused.                                                                                                           | ADR0056.                                                                                                |
| Network                 | The bridge and the CLI run in a network namespace with a loopback only; the one route out is the broker's unix socket. Observed from inside with real sockets and a positive control (host loopback, host abstract sockets and the internet are unreachable; the granted socket works; only `lo` exists). The broker connects to `api.anthropic.com` over verified TLS only. | A reviewer other than the author. Kernel-level escapes and same-user attackers are not claimed against. |
| Lifecycle/cancel        | Owned process handles for the bridge and the broker, bounded frames, queues and time, interrupt-vs-cut-off mapping, retry cap 3, whole-tree reap (0 left in 7 runs), cancel p50 10.4 ms. The environment of the launcher is no longer readable from inside the namespace (found and fixed in the corrective pass).                                                           | —                                                                                                       |
| Resume/recovery         | Authority-minted session id, resume across processes tested, no prompt/approval replay, ambiguous dispatch is indeterminate.                                                                                                                                                                                                                                                 | Durable store integration: **ADR0055**.                                                                 |
| Models/effort           | Offered / configured / requested / reported-in-use kept apart; reroute detected from the provider message model. A model or effort chosen at runtime start is now gated and validated at the start path. Selection stays gated (ADR0054).                                                                                                                                    | ADR0054 acceptance.                                                                                     |
| Admission               | `ClaudeLauncher::start` and `ClaudeWorker::new` each refuse to run without an `Admission`, which a production build cannot make while any gate of `m3::gate` is closed (all are). The fixture and tests have their own, compiled only into test builds and the `m3-fixture` feature.                                                                                         | —                                                                                                       |
| Resource/feature parity | 4 processes (the broker is a fifth; re-measure), about 290–320 MiB PSS, 34 FDs, idle CPU about 3 % (all of it the `claude` process), ready in about 2.2 s. Codex comparison not measured.                                                                                                                                                                                    | Codex arm of the comparison (needs an authorized Codex runtime).                                        |

### Owner decision packet

**Decision.** May the boundary above — two profiles, the model offered no tool, every effect performed by
the authority's own executor after the owner's decision, the exact build pinned, no fallback — be accepted?
It deliberately **does not** decide how a credential arrives.

| Choice                                                                             | What it means                                                                                                                                                                                                                  | Risks                                                                                                                           |
| ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------- |
| **A. Accept the boundary now; take the credential route in ADR0056** (recommended) | Gates stay closed until ADR0055, ADR0056 and an independent review exist. Acceptance names the exact build in `adapters/claude/pins.rs`, the host platform and the two profile digests; a later CLI needs a new qualification. | Accepting a boundary before the credential route can feel premature; nothing becomes reachable, because the gates are separate. |
| B. Accept nothing until everything is decided                                      | One decision later.                                                                                                                                                                                                            | Delays review of the parts that are ready.                                                                                      |
| C. Do not accept                                                                   | Claude Managed stays absent; Native remains the way to use Claude Code.                                                                                                                                                        | Loses the Managed parity goal (M3).                                                                                             |

**Tests it unlocks (after ADR0056):** a real Inference turn and a real Local Tools turn, each paid, each with
a rollback. **Rollback of acceptance:** disabling new Claude admissions; Native sessions, Codex Managed state and
history are untouched.
