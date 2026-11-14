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
unsupported/unknown. Provider reports are distinct from requested Native settings and account entitlement.
Managed attachment/turn/generation observations belong to the separate
[Managed contract](managed-runtime.md#schema-and-observation-compatibility).

Arsenal selections are reevaluated at launch against the actual provider build and exclusion
set; changes require reconfirmation. The build-specific [qualifications](provider-capabilities.md)
override older claims that per-session Codex exclusions are impossible. Selection/configuration
does not prove a skill was used or deny reading its files.

## Saved launch files

The accepted launch design (formerly ADR0059) uses per-user TOML files, reread when selected.
At most 64 sorted TOML candidates are processed, 64 KiB per file, with validated
filename grammar. The current implementation collects all matching directory entries
before that processing limit; this is not a hard directory-enumeration memory bound. A file selects either commands
(at most 16, each at most 4096 Unicode scalar values; one line, no controls) or a Native agent with an optional profile and an optional `preset`, never Managed.
A commands launch may also hold one variant per distribution, `[distro.<os-release ID>]` (at
most 24, each with its own commands and the `options` it was composed from, or `manual = true`),
and a `template` name. The variant whose ID is this computer's `ID`, else the first of its
`ID_LIKE`, is what runs; with none, the file's own `commands`. Workspace edits the variants with
`launch_save_variants`: the window sends commands for commands launches only (never a session's
`before`), each line validated as above, the file re-parsed before it is written, and the
variant this computer runs cannot be removed. The shipped `actualizar-sistema.toml` is replaced
by the variant one only while it is byte-identical to the earlier shipped file.
The backend parser in `src-tauri/src/launch.rs` owns exact keys and validation.

`preset` is the name of an Arsenal preset of that agent's provider. A launch that names one
applies it instead of the preset its folder remembers; a name with no preset for the agent
refuses the start before anything runs. Workspace saves a session launch (`launch_save`: name,
folder, agent, profile, preset; never `commands` or `before`; written through a private temporary
file and a rename, mode `0600`, never through a link). An existing file of that name answers
`exists` until the owner asks to replace it. `launch_delete` removes only a launch with no
`before` line and no commands.

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
Durable prompt/output handoff requires an accepted privacy contract. The [coordinator](coordination.md) implements finite supervised OL3 with metadata-only
journaling, but its production guarded effects remain blocked; a planner
or Native transcript is untrusted data, never an approval or permission to replay dispatch.

## Qualification required for offered controls

For each supported build, verify launch/precedence/inheritance, requested versus observed
model/effort, inventory provenance, scope isolation, version changes and refusal paths.
Cover real GUI edits, native confirmation, imported-template apply/Undo and provider consumption;
external writers, changed roots, interrupted recovery, simultaneous sessions and hook coexistence.
Preserve existing host/session/settings/history/wallet state. Fixture CLI syntax alone does
not qualify extension consumption, real authentication/refresh or a complete user journey.
Measure launch, inventory, supervision and restoration with [acceptance](acceptance.md).
