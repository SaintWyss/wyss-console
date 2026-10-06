# 0004 — Existing systems and provider taxonomy

Status: Accepted

Date: 2026-10-06

## Existing systems

| System                                                                  | Responsibility                                                                                          | Relationship to Sprint 0                      |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| WES (`~/.claude/bin/workflow-control.py` and routing/handoff machinery) | Development/control workflow: classification, profiles, tasks, receipts, handoffs and phase closure.    | Reference / future integration candidate.     |
| Existing Wyss daemon                                                    | Wyss-specific plans/actions, A–D policy, pending operations and audit over local JSON-RPC/Unix sockets. | Reference / future integration candidate.     |
| Wyss Console                                                            | Desktop client, progressively a terminal and visual cockpit.                                            | Independent foundation.                       |
| Future external control authority                                       | Canonical control of managed agents with a lifecycle independent of Console.                            | Conceptual boundary; implementation deferred. |

WES and the existing daemon are separate systems. Neither is automatically the
universal agent host for Console. Sprint 0A does not import their runtime, copy
their authority, read their internal state as its backend or depend on them.
Potential reuse must be evaluated through explicit contracts in a later phase.

Manual model routing currently governs the development workflow. It does not
implement or select a Console product routing engine. Risk/security and model
capability remain separate dimensions.

## Provider taxonomy

**Model provider:** access to model inference, responses, usage and costs.
Examples: OpenAI, an OpenAI-compatible gateway such as LiteLLM, Anthropic and Ollama.

**Agent adapter:** integration with an agent runtime's lifecycle, sessions,
observations and supported operations. Examples: Codex, Claude Code and OpenCode.

The existing Wyss model gateway does not constitute a Console agent adapter.
Neither category is implemented in Sprint 0A.

Future adapters declare capabilities for model visibility, model switching,
structured events, subagents, prompts/handoffs, tools, skills, permissions and
terminal fallback. Declarations include support, validated version and coverage.
Unknown does not mean supported; observed does not mean enforced. Capability
declarations do not grant authority. Adapters translate authorized requests and
report facts; policy belongs to the external deterministic authority.

Adapter interfaces, registries, provider protocols, configuration projection,
authentication handling and integration code are deferred. Terminal fallback does
not relax the authority milestone described in ADR 0002.

## References

- Wyss Engineering System v1, section 1.
- Wyss architecture research report, section 9.
- Accepted pre-implementation DECISION_PACKET, 2026-10-06.
