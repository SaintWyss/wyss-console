# ADR 0054 — Versioned Managed model selection and effort

Status: **Proposed; not accepted** · Date: 2026-10-23 · Owner: Santiago

## Context

Accepted ADR0014/0015 do not permit caller-controlled Managed model or reasoning-effort
selection. The current F8 P2 request schema has no such fields. A runtime model catalogue
is an observation, not subscription entitlement. ADR0048 concerns Native model/effort
controls and does not extend the Managed authority contract.

This proposal defines a separate, versioned extension. It must not be added as optional
unknown fields to protocol2. The initial extension is per-turn only; session defaults and
mid-turn changes remain unsupported. Live UI/request admission stays disabled until this
ADR and the corresponding authority extension are explicitly accepted and negotiated.

## Proposed contract

1. Introduce a distinct `ManagedModelSelectionV1` authority capability in a new protocol
   version. An older authority rejects/does not advertise the capability; callers never
   infer it from a model catalogue. Existing P2 Codex behavior and DTOs remain unchanged.
2. A caller may request a canonical provider model identifier and optional provider-defined
   effort value for exactly one new turn. No free-form provider arguments, fallback chain,
   prompt-derived setting or persistent session default is accepted.
3. The authority records the exact request, current runtime identity/generation, current
   model catalogue revision, capability source/freshness, provider, turn and idempotent
   admission receipt. Provider/account entitlement is `unknown` unless an authoritative
   provider mechanism says otherwise; model listing does not imply entitlement.
4. Validate supported model/effort pair at turn admission and again immediately before
   provider dispatch. Reject stale catalogue, unknown or retired model, unsupported effort,
   provider/build mismatch, changed generation, missing feature negotiation and unknown
   capability. No automatic substitution is allowed.
5. Emit four separate facts with source, time, scope and generation: **offered** by current
   catalogue; **configured** by account/project/provider settings; **requested** by the
   user for the turn; **reported in use** by the provider for that turn. Absence is unknown.
   Never infer that effort was honored without provider evidence.
6. A provider reroute is a new observation bound to the same originating turn. It does not
   rewrite the request or catalogue. Any retry/cancel requires a distinct explicit action
   and operation receipt; do not retry at a substitute model automatically.
7. After restart, a request is recoverable only from its durable receipt and exact turn
   identity. A not-yet-dispatched turn can be explicitly revalidated and retried under a new
   receipt. An ambiguous dispatched turn becomes indeterminate and is never replayed.
8. The audit stores model ID, effort enum, source/revision, provider/build, turn/generation,
   outcome and reroute metadata only. It excludes prompt, response, reasoning, secrets and
   arbitrary provider payload.
9. Capability discovery has an explicit source, schema version, observed-at time and
   freshness deadline. Stale catalogue/entitlement distinction is visible. Unknown event
   or DTO fields fail closed; no silent downgrade to protocol2 selection behavior.
10. The first release supports no cross-provider translation and no fallback configuration.
    Each provider adapter publishes its exact supported IDs/efforts and how it observes
    actual use. A provider that cannot confirm actual effort reports `unknown`.

## Required proof before acceptance

- Strict positive/negative DTO fixtures for current and older authority versions.
- Stale catalogue, entitlement-unknown, unsupported effort and provider mismatch refusals.
- Generation change between admission and dispatch; no dispatch after the fence changes.
- Provider reroute, cancellation, interruption, retry and crash recovery with no automatic
  substitution/replay.
- Claude and Codex fixtures prove `offered/configured/requested/reported` remain distinct.
- GUI controls are absent/disabled unless the exact authority negotiates the accepted
  capability; the existing Codex protocol path remains unchanged.
- Independent review confirms no model/effort selection field enters the P2 schema or
  bypasses the authority.

## Consequences and owner decision

This ADR proposes engineering semantics; it does not authorize live selection, change
accepted protocols or claim account entitlement. Santiago must accept or reject this ADR
before any new authority capability is advertised or used. Until then, Managed model and
effort selection remain disabled. The extension and ADR may be revised only through a new
versioned contract; no caller can force compatibility by sending extra fields.
