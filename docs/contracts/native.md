# Native sessions, profiles and saved launches

Native preserves provider behavior and the account's rights. It is distinct from Managed,
restricted B1 and protected approval. [Delegation](delegation.md) has its own worker policy.

## Launch and profiles

Backend-owned provider catalogue IDs select typed launch recipes, never arbitrary page-supplied
argv. Use separate arguments and the qualified interactive shell exec form for sh/bash/zsh/fish;
an unsupported shell requires an explicit direct-exec fallback. Preserve the normal user
environment except the closed provider-specific session variables. Do not manufacture
compatibility by copying credentials, resetting provider HOME or changing account settings.

Profiles support bounded inheritance (depth eight), pinned versions (20 retained versions),
clear precedence and explicit unavailable-version errors. They contain no secrets or free-form
commands. Requested model/effort is distinct from observed provider state. Do not inject a
selection into an already-running terminal; require a qualified provider mechanism or show
unsupported/unknown. Observations are transient, attachment/generation fenced, not transcript
or account entitlement. Reroute preserves only the accepted typed fields and the originating
turn/generation fence, never arbitrary provider payload.

Arsenal selections are reevaluated at launch against the actual provider build and exclusion
set; changes require reconfirmation. The build-specific [qualifications](provider-capabilities.md)
override older claims that per-session Codex exclusions are impossible. Selection/configuration
does not prove a skill was used or deny reading its files.

## Saved launch files

The accepted launch design (formerly ADR0059) uses per-user TOML files, reread when selected.
At most 64 files, 64 KiB each, validated filename grammar. A file selects either commands
(at most 16, each at most 4096 bytes) or a Native agent and optional profile, never Managed.
The backend parser in `src-tauri/src/launch.rs` owns exact keys and validation.

Optional preparation has at most eight lines, run separately through `/bin/sh`, with a
30-second deadline, process-group cleanup and discarded output. Failure is reported and
the agent can continue; preparation is never silently prepended to the provider recipe.
Resolve the selected profile before preparation. Record the applied Arsenal selection.
Seed the three default launch files only if the directory is absent, never overwrite user files.

`bypass` is explicit full access only for qualified Claude/Codex Native builds. Display it
in plan/editor/launch; require host feature `native_permission_bypass_v1` or refuse. This
accepted amendment replaces the old blanket profile prohibition; it grants nothing to Managed.

## Host lifecycle and handoff

The host owns PTYs and stable terminal/session identities; a PID is not identity. GUI detach
does not kill host-owned work. The local fallback has no equivalent persistence guarantee.
Reboot, power loss and host crash end processes; restoring layout never restarts an agent.
Offer explicit launcher restart for a dead placement. Lingering is separately confirmed.
Use the [host](terminal-host.md) and [screen](terminal-fidelity.md) contracts for exact behavior.

Handoff is an explicitly reviewed, bounded package for a chosen receiver. Bind revisions,
digests and receiving-plan limits; do not infer shared history from switching providers.
Durable prompt/output handoff requires an accepted privacy contract. Pipeline coordination
is finite supervised OL3, metadata-only journaling and authority-mediated effects; a planner
or Native transcript is untrusted data, never an approval or permission to replay dispatch.

## Qualification required for offered controls

For each supported build, verify launch/precedence/inheritance, requested versus observed
model/effort, inventory provenance, scope isolation, version changes and refusal paths.
Cover real GUI edits, native confirmation, imported-template apply/Undo and provider consumption;
external writers, changed roots, interrupted recovery, simultaneous sessions and hook coexistence.
Preserve existing host/session/settings/history/wallet state. Fixture CLI syntax alone does
not qualify extension consumption, real authentication/refresh or a complete user journey.
Measure launch, inventory, supervision and restoration with [acceptance](acceptance.md).
