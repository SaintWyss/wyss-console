# Wyss Console

A native personal development console for CachyOS/KDE: local terminals, Claude Code
and Codex Native sessions, real provider configuration, and authority-managed Codex
work. Tauri2, Svelte5, Rust, xterm6; version0.2.0, private repository and local packages.

This page describes baseline `e480d479` (2026-10-22). The **final redesign is planned**:
full hybrid Claude Code/Codex and project-centered workspaces. Read
[PRODUCT](docs/PRODUCT.md) and the [functional specification](docs/product/functional-specification.md)
for that target, and the [gap register](docs/product/implementation-status.md) for evidence
and limitations. This documentation phase changes no application code or installation.

## Current capabilities

- Plain terminals, tabs/panes, search, shortcuts and saved layout. A persistent native
  host owns terminals independently from the GUI. Faithful screen checkpoint/reattach,
  complete logout/power-loss behaviour and performance budgets remain partly designed.
- Native Claude Code/Codex launch, registry, profiles, known project folders/defaults,
  resource observations and reviewed handoff brief to a new Native recipient. Provider
  CLIs keep their own prompts/controls. This is full user-account execution, not Managed.
- Configuration inventory, supported real-file switches/authoring, native command editors,
  sealed restoration, exact applicable Undo and encrypted templates. Project authoring
  exists; provider consumption/reload is not generally proven. Template decrypt/prefill/
  cancel is observed; confirmed imported-template application remains NOT_RUN.
- Sealed credential management stores/verifies keys without returning plaintext to the
  page or injecting them into providers. Models keeps offered/configured/requested/
  reported facts separate; Native effort has partial evidence. Managed model/effort
  selection and durable usage history are not implemented.
- Managed Codex Inference/Local Tools with expiring scoped leases, generation checks,
  exact action approvals and durable control audit. Native configuration cannot silently
  relax those profiles. Same-UID socket authentication is not protection against every
  unrestricted process of the same account.

### F8 production state

F8 **is active in production** with real Codex 0.159.0, protocol 2, SQLite V8 and
[ADR0051 Accepted](docs/adr/0051-per-session-managed-runtimes.md). The installed
Local Tools Ready→lease→binding→turn Completed and restart→Stop→Close paths were recorded
as verified in the [activation result](docs/acceptance/f8-activation-procedure-2026-10-22.md).
This phase does not repeat that production run.

Runtime identity is per session; active capacity is **one**. Multi-process credential
refresh remains NOT_PROVEN. Protocol1/V7 remains available through its separate authority/
store. P2 selection uses `--f8-authority` and a coordinated installation; there is no
implicit protocol downgrade or conversion of histories. P1 retirement is separate.

F8 is not concurrent Managed execution, Claude Code Managed support or complete OL3.
Remote gateway code is unreachable foundations only, with no transport/listener/device
service. Remote PC/native Android, the coordinator, safe Native automated dispatch and
B1 staged/quarantined worker remain designed/blocked. Existing project grouping is not
already the primary project/workspace redesign.

## Build and repository validation

Build prerequisites: Linux, Rust, Node26+, npm12+ and the
[Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).
Local Tools requires compatible bubblewrap; Managed Codex compatibility is pinned to
0.159.0. Do not replace it with an unverified newer binary.

```sh
npm ci
npm run check
npm run check:rust
npm run scan:repo
npm run desktop:package
```

`npm run check` runs formatting, lint, type checks, frontend tests, owned-process harness
and frontend build. `check:rust` runs the deliberately selected fmt/check/test/clippy
suite. **Do not substitute blanket `cargo test` or `lifecycle_process`**; inherited
private user/systemd procedures are not the current acceptance policy. Unit fixtures
and intrinsic Managed isolation remain distinct from real installed acceptance.

`desktop:package` produces a private directory package with desktop/authority binaries
and digest manifest. It does not install, publish or migrate anything. `desktop:dev`
uses the native IPC host; `npm run dev` alone supplies only the frontend. Installation
and service operation are separate from repository validation.

## Installation, data and live work

The local user installer exists (`scripts/install.sh`, ADR0040); there is no public
AppImage/deb release. Follow the reviewed [F8 activation procedure](docs/acceptance/f8-activation-procedure-2026-10-22.md)
only in a separately authorized operational window; it documents already executed
activation, not an instruction to repeat it. GUI-only `--keep-host` updates preserve
live host terminals and can leave compatible update-pending components.

The native host, P1 authority and F8 authority are independent owners with separate
stores/lifetimes. Closing a view is not stopping a session. Never assume a GUI update
updated the retained host. Never restart a service to collect evidence or clear an old
status. Installer rollback can stop the host and requires accounting for live sessions.
Retain matching binaries and a consistent SQLite backup; there is no SQL downgrade.
User data is not removed automatically with the application.

No credentials are needed for source/docs review or the normal repository checks.
Never run ignored real-provider tests or historical credential/private-environment
procedures merely because they appear in an older report.

## Documentation

- [Current checkpoint](docs/CHECKPOINT.md), [final-product index](docs/product/README.md).
- [Architecture/workflows](docs/product/architecture.md), [provider matrix](docs/product/provider-matrix.md).
- [Implementation/acceptance gaps](docs/product/implementation-status.md), [dependency roadmap](docs/product/roadmap.md).
- [Traceability/consistency](docs/product/traceability.md), [independent review](docs/product/independent-review.md).
- [Complete documentation inventory](docs/product/documentation-inventory.md), [architecture index](docs/architecture/00-INDEX.md).
- [Accepted/proposed ADRs](docs/adr/), [dated acceptance evidence](docs/acceptance/), [CHANGELOG](CHANGELOG.md).

The [prior README](docs/history/README-before-phase-1-2026-10-22.md) retains detailed
protocol 1/development history, all original observations and limitations. Historical
architecture/drafts and `wyss-agent-legacy/` remain accessible; their status statements
do not override later acceptance records or accepted ADRs. Nothing here accepts the
new Proposed ADR0052/0053 or authorizes implementation of the roadmap.
