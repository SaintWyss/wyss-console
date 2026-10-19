# F4a/F4b/F5 — Provider integrations and configuration authoring (proposed)

**Status:** Draft. Based on architecture 07, 18, 19 and 22. F4a follows F1; F4b follows F4a and permission to install/inspect Hermes; F5 follows F2a with DN-12/DN-13. Compiled-in adapters only unless a future security ADR approves executable plugins.

## Provider adapter contract

Report explicit per-version and per-scope capabilities: discovered roots, precedence, format, filesystem trust and evidence states (verified, documented, not_verifiable, unknown, unreadable, stale, conflict, credential_excluded). Separate can_plan / can_apply / can_undo / can_import / can_export. State whether an already-running provider reloads settings, supported by actual observations.

Use the existing bounded reader, closed typed errors, redaction, watcher, plan state machine and atomic writer. A provider-specific feature is not automatically equivalent to a Claude or Codex feature. Provider-owned commands, if approved, come from a closed backend catalogue, never raw webview input.

## F4a — OpenCode

1. Determine installed version and real config roots. If absent, report unverified; no unattended installation.
2. Support read-only inventory and verified scope/precedence first.
3. Preserve JSONC comments, order, whitespace and quoting. Do not use generic parse/stringify when it changes user layout; choose a byte-range or comment-aware editor only if needed and justified under DN-04.
4. Add verified switches through plan/diff, appropriate confirmation, reversible writer, failure recovery and provider check.
5. Treat discovered third-party plugins as untrusted data, never authorize execution by enumeration.

Negative tests: malformed JSONC; delimiter-like text inside comments; duplicate keys; external symlink; credentials; concurrent file replacement; unrecognized semantic keys; unsupported version. Done when real OpenCode inventory, supported switch, exact undo and native GUI pass without regressions.

## F4b — Hermes

Inspect a real installed version and documented behavior after owner consent. Archived comparisons alone do not establish a current API. Prefer provider-owned supported config writers where applicable. DN-13 approval must specify the executable allowlist, typed bounded argument grammar, preview/confirmation and no shell string construction.

Native Hermes CLI support is different from managed Hermes execution (F11b); F4b does not grant authority permissions. If provider inspection cannot be performed, keep status not_verified. Complete only after real detection, native launch and supported configuration state operate on a verified version.

## F5 — Create/edit/delete configuration

Separate toggling an existing field from authoring executable hooks, editing commands or deleting entries. Each needs appropriate risk classification.

`Select provider/type/scope → native editor → static validation → bounded redacted preview → plan/effect → native confirmation where risky → atomic apply → provider verification → history/undo`.

The webview sends structured data; it does not choose executable commands. The native dangerous-action dialog shows exact operation and destination and cannot be autoapproved by forged page messages.

- Change only known byte ranges; reject layouts the editor cannot locate rather than reconstructing entire files.
- Compare source hash/version at plan and apply; preserve concurrent-provider conflict recovery.
- A deleted entry has a restorable record under approved ADR 0029 rules; missing secure storage must refuse secret-bearing restoration.
- After write, report whether provider actually applied the change. If reload unknown: "Written; effect on running sessions not verified".
- Imported templates are untrusted data, never capabilities.

## Acceptance

| Case | Expected |
| --- | --- |
| Shell metacharacters in new hook | No unintended Console command; provider receives intended text |
| Create/edit/delete/undo | Byte-identical restoration; unrelated comments unchanged |
| Concurrent edit | No silent overwrite; deterministic conflict and recovery |
| Credential-shaped values | No secret in projection, logs, journal or diagnostic |
| Risky authoring | Required native confirmation cannot be bypassed |
| Unsupported version/type | Write unavailable with honest explanation |
| Provider writer | Only verified closed command contract |
| Import/export | Versioned schema; no silent grant/activation |

F5 focused external security review is mandatory before real authoring. An unrelated read-only adjustment does not require rerunning the whole security audit.

## Open decisions

DN-04: justified JSONC dependency. DN-12: native editor versus external-editor round-trip. DN-13: specific allowed provider writer commands. Record observed provider reload semantics per version; don't guess.
