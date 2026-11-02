# Current architecture

The [status page](STATUS.md) owns evidence and delivery limits; [PRODUCT](PRODUCT.md)
owns target behavior. This page describes components present in source and their
boundaries. [Remaining work](ROADMAP.md) is separate from the component inventory.

## Components and state owners

Rust paths are relative to `src-tauri/src/`.

| Component              | Source                                                              | Responsibility                                                                                                                                                                                              |
| ---------------------- | ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Desktop                | `main.rs`, `desktop/`, `src/App.svelte`, `src/lib/`                 | Tauri command composition, Svelte views and client controllers. The configuration engine is desktop-only, outside the authority library.                                                                    |
| Terminal host          | `host/`, `terminal/`                                                | PTY masters, Native process ownership, bounded byte history, layout and versioned host IPC. It can outlive a GUI window.                                                                                    |
| Screen helper          | `screen/`, `bin/wyss-screen.rs`                                     | A separate bounded parser/repaint process; the host verifies/starts a helper when available and retains PTY ownership. Missing or failed helpers are degraded fidelity, not a reason to duplicate sessions. |
| Native launches        | `native/`, `launch.rs`                                              | Provider catalogue, registry/profiles, controlled shell recipes and user TOML launches. Feature negotiation rejects unsupported profile words.                                                              |
| Configuration          | `agentconfig/`, `configuration.rs`, `credentials/`                  | Source discovery, supported authoring, context-bound plan/apply/Undo, templates, sealed recovery and credential custody.                                                                                    |
| Arsenal                | `arsenal/`, `native/arsenal.rs`                                     | Offline SQLite capability library, revisioned presets/selections, build-qualified per-launch exclusions and trusted-folder sync.                                                                            |
| Native delegation      | `delegate/`                                                         | Per-parent rights, MCP forwarding, provider-specific workers, limits, isolated workspaces, verification records and cancellation.                                                                           |
| Projects               | `projects/`, `contracts/m1/`                                        | Separate private projection store, stable identities, root binding, workspace layout/association, CAS, migration and cooperative input claims.                                                              |
| Ordinary authority     | `control/`, `sessions/`, `turns/`, `events/`                        | Canonical Managed state, receipts, lifecycle, turns, approvals and durable audit for its protocol/store.                                                                                                    |
| F8 authority           | `f8/`, `desktop/f8/`                                                | Explicit P2 per-session allocation, generation fences, leases, worker supervision, resource policy and Codex Local Tools production spawner.                                                                |
| Claude Managed support | `adapters/claude/`, `f8/claude/`, `m3/`, `src-tauri/claude-bridge/` | Adapter/SDK bridge and model/history support. Closed admission gates separate implementation from accepted production use.                                                                                  |
| Coordinator            | `orchestration/`, `orchestration_commands.rs`                       | Finite DAG scheduling, metadata-only journal and ordinary typed authority requests. It is not an execution authority.                                                                                       |
| Remote core            | `remote/`, `android/`                                               | Grant/pairing/request rules, verification and Android shell/input state. No production transport/listener connects these pieces yet.                                                                        |

The packager includes the desktop, authority, screen helper and credential agent.
B1 executables remain fixture-feature-only and excluded from production packaging.

## Identity and routing

Project/workspace identity is independent of a directory name, title, Git URL or
PID. Root binding includes host identity and revision. Native session and terminal
IDs remain owned by the host; Managed IDs include the authority/store and runtime
generation. A workspace can reference a session without owning it again.

Operations capture context before asynchronous work. Reads and navigation do not
grant an execution, lease or approval. Cross-owner associations are reconciled
explicitly; they are not a distributed transaction or a reason to replay a start.
The [workspace contract](contracts/workspace.md) defines the exact records,
limits, commands and migration/rollback rules.

P1 and F8/P2 retain distinct writers, stores and connection status. F8 is selected
by `--f8-authority`; the ordinary startup does not select it. No window command
can merge these histories or silently change an authority's bound workspace.

## Execution boundaries

Native runs the original CLI with the permission explicitly selected for that
launch. Wyss does not mediate its original provider prompts as Managed approvals.
The full-access profile remains explicitly labelled.

Native delegation uses parent rights and its own policy. Workers have separate
workspaces/process groups and Bubblewrap PID namespaces; Wyss control directories
are hidden/read-only according to the [delegation contract](contracts/delegation.md).
Provider homes remain writable and the rest of the filesystem retains its host
permissions. This is not the stronger B1 or protected-approval guarantee.

Managed adapters use closed protocol/tool surfaces and attestation, runtime
generation, leases and per-action approval. Unknown or stale actions are refused.
The current production F8 spawner admits its pinned Codex Local Tools profile;
Claude/model/history admission remains independently gated in `m3/gate.rs`.

The coordinator schedules admitted work through its existing owner. Untrusted
Hermes proposals are data, not effects or approval. Automated Native and remote
full-account lanes remain subject to the unresolved trust decision.

## Persistence and recovery

| Data                                | Owner and recovery rule                                                                                                                                                                            |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Managed SQLite state/audit          | One authority writer per store; consistent backup/replay rules, no SQL downgrade or reconstruction of execution from audit. Offline V9 preparation does not automatically migrate an installation. |
| Project metadata                    | Separate locked/revisioned store. Migration is read-only-first; preserve conflict/newer/corrupt states and distinguish metadata restoration from restarting processes.                             |
| Native registry/profiles/layout     | Host-owned records. Observe existing sessions without mutating their ownership or interpreting absent processes as permission to relaunch.                                                         |
| Configuration and recovery material | Real provider files plus kind-specific sealed restoration/templates. Apply/Undo revalidate postimages and outside edits; custody/fallback follows accepted decisions.                              |
| Arsenal and sync                    | Local SQLite library; immutable content revisions and shared per-device journals. Sync validates records, tracks conflicts and acknowledges actual application, never executes received content.   |
| Delegation records                  | Per-worker lifecycle/configuration/result records; live-process ownership and the finished history are distinct. A parent right is revoked when its session ends.                                  |
| Screen/stream/handoff content       | Bounded volatile state with disclosed gaps; no durable transcript added merely to correlate UI state.                                                                                              |
| Coordination journal                | Bounded metadata and digests; ambiguous recovery pauses rather than duplicating effects.                                                                                                           |
| Usage and remote state              | Proposed/blocked policy and transport paths remain separate from accepted audit and recovery stores.                                                                                               |

Provider authentication/history remain provider-owned. No project switch, handoff
or sync copies credentials to simulate compatible login. Source paths, labels and
associations are private metadata, not ordinary telemetry.

## Failure behavior and detailed contracts

Host failure, authority failure, wallet lock, a parser failure and a remote outage
affect their own regions. A stale receipt or lost input acknowledgment is not a
new action. Report partial/unknown input; do not replay it. Power-off does not
mean continued execution.

Use the [event/data contract](contracts/event-envelope.md),
[terminal/host byte flow](contracts/terminal-host.md),
[adapter/Codex boundary](contracts/agent-adapter.md),
[F8 wire contract](contracts/f8/protocol-2.md),
[terminal fidelity](contracts/terminal-fidelity.md),
[configuration changes](contracts/configuration.md),
[Arsenal sync](contracts/arsenal-sync.md) and
[delegation](contracts/delegation.md) for exact interfaces. Accepted residual
risks and open trust choices are summarized in [DECISIONS](DECISIONS.md).

## Remote foundations and pending transport

The accepted local core validates grants, device states, per-request scope and replay
rules. QR enrollment uses a 128-bit secret and transcript MAC; P-256 device presence
signatures do not supply an encrypted channel or authenticated/persisted host identity.
Short-code pairing still needs a PAKE. No production constructor/listener, platform
keystore integration or actual hardware qualification follows from the tested library.
Transport/host-key lifecycle and absent-host account-control approval remain open choices.
Remote membership never grants direct authority access; configuration proposals require
host confirmation and ambiguous terminal input is never replayed. See the
[remote requirements](REQUIREMENTS.md#fp-rem-01) and [remaining roadmap](ROADMAP.md#6-remote-pcs-and-physical-android).
