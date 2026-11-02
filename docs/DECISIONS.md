# Current decisions

This is the working summary of current constraints and open choices. Accepted
foundation decisions are consolidated here and in the linked technical contracts;
their original acceptance records remain available in Git. Proposed decisions are explicitly
identified in the current contracts below. Consolidation does not accept a proposal,
change a security decision or open a runtime gate.

## Product and ownership

| Decision                  | Current rule and reason                                                                                                                                                                                                                                       |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Personal complete product | Claude Code and Codex remain first-class Native/Managed targets where technically feasible; project-centered work, remote PCs and native Android remain in scope. Public distribution, teams/multi-tenancy, an IDE, screen sharing and unattended OL4 do not. |
| State owners              | Host owns PTYs/Native processes; each Managed authority owns its own SQLite state; projects/workspaces are a separate projection, not execution permission. Selection/navigation does not start, stop or rebind work.                                         |
| Native versus Managed     | Preserve original provider behavior in Native. Managed admits only its qualified profile; it does not silently fall back to Native or inherit Native permissions.                                                                                             |
| Authority compatibility   | Keep ordinary P1 and explicitly selected F8/P2 separate while both remain in use. Cleanup is not a retirement/migration operation.                                                                                                                            |
| F8 admission              | Production Managed capacity stays one; a larger test build or Native delegation policy does not approve an increase.                                                                                                                                          |
| Secrets and recovery      | Accepted sealed custody uses Secret Service for secret-bearing material. The disclosed secret-free fallback and kind-specific diverged-entry restoration remain applicable; no universal exact-Undo or silent overwrite of outside edits.                     |
| Provider qualifications   | CLI syntax, configuration exclusion, observed settings and actual tool/skill use are distinct evidence. Pin claims to the qualified build; refuse unsupported launches rather than inventing compatibility.                                                   |

Detailed rules: [personal scope](PRODUCT.md),
[restoration custody](contracts/configuration.md),
[kind-specific recovery](contracts/configuration.md),
[per-session runtime](contracts/managed-runtime.md) and
[project identities](contracts/workspace.md).

## Foundation and integration taxonomy

The product remains desktop/Linux/local-first, using Tauri 2, stable Rust, Svelte,
TypeScript, Vite and a mature frontend terminal renderer. Keep explicit typed
IPC/DTO boundaries, a dedicated frontend client and the current single application
crate; do not add speculative services or empty crates. npm is the frontend package
manager, lockfiles are tracked and generated build/runtime data stay ignored.

A model provider supplies inference; an agent runtime supplies its workflow;
an adapter translates a specific runtime surface. Configuration adapters are
distinct from execution adapters. Descriptors, version strings, catalogues and
observations are not authority or account entitlement. Existing WES and the separate
Wyss assistant/daemon are reference systems, not implicit Console dependencies or
authorities; reuse requires an explicit interface rather than copying their state.

The accepted detailed rules now have one technical home:
[event/data contract](contracts/event-envelope.md),
[terminal/host contract](contracts/terminal-host.md) and
[agent adapter/Codex boundary](contracts/agent-adapter.md). Completed sprint limits
such as no tabs, no parser, no persistence or no adapter implementation no longer
describe the product and are not carried forward as open tasks.

## Authority and client lifecycle

Both the Svelte page and the Rust graphical process are clients, not the Managed
security authority. The independent authority owns permission, grants, leases,
canonical transitions and audit. The graphical lifecycle cannot grant, renew,
revoke or expand Managed permission merely by connecting, closing or crashing.
Reconnect reads authority state; it does not restore grants from a UI cache.

Explicit stop/revoke is a separately validated intent. Independent expiry and
security transitions may occur while the GUI is absent. An authority cannot exit
solely because its last graphical client closes while work or obligations remain.
Separate processes alone do not establish an OS security boundary, and a human
terminal is not promoted to a Managed session by a capability declaration.

## Native launches and explicit full access

Per-user TOML launch files name commands or Native sessions with optional profiles
and preparation lines. The desktop rereads the chosen file; profiles and session
starts use the Native path. Preparation runs separately with a bounded process
group and is not injected into an agent's recipe.

`bypass` explicitly enables full access for qualified Claude/Codex Native builds.
The plan, editor and launch display that permission; a compatible host must
advertise `native_permission_bypass_v1`. This is the current rule from
[Native launch contract](contracts/native.md#saved-launch-files). It does not extend Managed permissions.

## Trust and data boundaries

- Same-account confirmation is not a protected approval principal against an
  arbitrary same-UID process or privileged administrative delegation.
- Restricted B1 candidates, Native delegation workers, unrestricted Native,
  Managed execution, untrusted planners and protected human approval each have
  their own contract. A positive fixture for one does not qualify another.
- Accepted Local Tools residual risks include credentials readable by an
  explicitly approved command and pre-existing hardlink effects. Runtime routing
  does not remove those risks.
- Project, workflow and remote journals do not become transcript stores.
  Provider credentials are not copied/re-homed to manufacture compatibility;
  optional usage history needs its own accepted privacy policy.
- Arsenal sync shares a trusted owner's folder. Received content is validated
  and not executed by sync; unsigned records do not establish an independent
  device trust boundary. Excluding a skill does not deny filesystem access.

The [delegation](contracts/delegation.md), [sync](contracts/arsenal-sync.md) and
[architecture](ARCHITECTURE.md) contracts state their actual limits.

## Open decisions

| Choice                               | What must be resolved before changing behavior                                                                                                                                                                     |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Remote transport                     | Compare private overlay with embedded P2P/relay for actual devices, cost, maintenance, security and footprint. No production transport is selected by the current core.                                            |
| Stronger worker/approval boundary    | Prove useful restricted workers and protected approval, or explicitly accept a narrower account-trusting model. Do not infer absent-host full-account permission.                                                  |
| Protected writer/snapshot conditions | Prove actual filesystem/sync/snapshot conditions; if the wider guarantee cannot hold, choose a concrete restricted write surface or explicit risk trade-off. Existing custody is not reopened implicitly.          |
| Fully sealed restoration extension   | Resolve the remaining policy difference from the accepted secret-free fallback before changing it.                                                                                                                 |
| Claude Managed                       | [Security/authentication](contracts/managed-runtime.md), [provider-aware store](contracts/managed-runtime.md) and [credential delivery](contracts/managed-runtime.md) remain proposals with review/evidence gates. |
| Managed selection and history        | Accept the [versioned model/effort extension](contracts/managed-runtime.md) and separate [metrics policy](contracts/usage-history.md) before enabling their gates.                                                 |
| Parser and B1 acceptance             | [Parser helper](contracts/terminal-fidelity.md) and [B1 candidate](contracts/security.md) retain their Proposed status and unresolved release conditions despite implementation.                                   |
| Adoption and capacity                | Agree reference budgets and authorize actual adoption or any F8 capacity increase through their session-preserving operational procedures.                                                                         |

Exact technical discovery belongs to the [roadmap](ROADMAP.md), not another
product-choice questionnaire. A decision becomes accepted only through its
recorded explicit acceptance; merging code or editing this summary is insufficient.
