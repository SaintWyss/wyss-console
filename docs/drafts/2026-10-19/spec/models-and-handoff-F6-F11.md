# F6a/F6b and F11a — Model observation, selection and handoff (proposed)

Canonical basis: architecture 12, 13, 19, 11 and session profile contract 09. F6a requires F3b; F6b requires F6a/F3c; F11a requires F3c. F11b is a distinct managed phase after F8.

## Five distinct model facts

- **M1:** provider/runtime identity.
- **M2:** runtime catalogue and declared default; **not** entitlement.
- **M3:** model configured in global/project/session sources according to verified precedence.
- **M4:** model requested/selected for a specific session.
- **M5:** model actually reported by the provider for a turn/session, **only when observed**.

Never infer M5 from a profile, M2, binary name or configured default. Every assertion carries source, provider version, freshness and status: reported, configured, listed, predicted, not_verifiable, stale, unknown.

## F6a — Seeing models

Illustrative read model (requires contract review):

```text
ModelObservation {
  provider_id, session_id, source_kind, source_version,
  configured_model?, selected_model?, observed_model?,
  observed_scope: none | session | turn,
  observed_at?, report_freshness?, status,
  effort?, catalogue_reference?
}
```

Managed: catalogue stays authority-owned; any safe bounded observation projection requires DN-09 and an ADR (including transient/durable and event-vocabulary policy). Keep attestations and security per generation.

Native: M3 from inventory, M4 from launcher/profile; show "In use: not reported" unless first-party provider actually supplies M5. Do not add hooks or scrape terminal output to manufacture it.

Usage/cost: only provider-reported numbers with provenance, units and time scope. No price-table estimates represented as charges.

## F6b — Choosing models

Native sessions may pass a verified provider flag as structured backend argv, selected through a per-session profile. Never modify global config when only a session model was chosen. A running session's native model-switch command remains user-entered unless a supported provider API is explicitly reviewed.

Managed choice would change SG-12 and activation surface; a separate accepted authority ADR and catalogue validation are mandatory. Missing entitlement/unsupported effort/stale catalogue means a typed error, not silent fallback.

**Acceptance:** requested, configured and reported model are visibly different. A catalogue model rejected by the account is listed, not usable. Unknown reports never render as the default. Imported profiles with removed model stay readable but cannot launch until resolved.

## F11a — Handoff (explicit and reviewed)

A handoff is a **new context package deliberately reviewed by the user**, not hidden-session continuation and not lossless private model state.

Illustrative v1 envelope:

```text
HandoffEnvelope {
  schema_version, package_id, created_at,
  origin: {provider, plane, session_reference?, project_reference?},
  destination: {provider?, desired_profile_reference?},
  goal, authored_summary, selected_artifacts[],
  constraints[], known_open_questions[], source_refs[],
  provenance_markers[], content_fingerprint
}
```

All fields and artifacts are bounded. References must be normalized against explicitly selected project roots; never arbitrary file-read capabilities. No default raw transcript, environment, credential, authority event or executable command.

**Flow:** choose source and artifacts → construct bounded redacted preview with external/model text marked untrusted → user reviews/edits/confirms → destination starts under its own profile/permissions → send via a supported bounded interface → audit only IDs, hashes and outcomes.

Do not transfer tool grants between Native/Managed planes, auto-execute quoted instructions or persist raw prompts. Export needs a sensitivity warning.

## Security and acceptance

- Seeded secrets never appear in preview, logs, audit or persisted metadata.
- Rejecting handoff causes no destination launch or file mutation.
- Quoted adversarial instructions remain inert content.
- Stale source, missing destination, oversized package and unsupported version fail with typed state.
- Retry must not duplicate a command or other side effect.
- Package fingerprint and version survive export/import.
- Keyboard-only review and visible Native/Managed plane labels.

Definition of done: verified native-to-native handoff through GUI, including cancellation and untrusted-data cases. Managed destination requires separate verified authority API and permissions.
