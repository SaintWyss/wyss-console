# 0001 — Product foundation and sprint boundaries

Status: Accepted

Date: 2026-10-06

## Decision

The product is Wyss Console: a desktop-first, Linux-first, local-first application.
Development is terminal-first and incremental. The approved stack is Tauri 2,
stable Rust, Svelte, TypeScript and Vite. A mature terminal component will be reused;
Wyss Console will not build a terminal emulator from scratch.

Sprint 0A establishes a single desktop application crate, a minimal frontend,
explicit DTO boundaries, one harmless typed IPC command, deterministic validation,
and the four accepted ADRs. The only application DTO is AppInfo v1:
`schema_version: 1`, `name: "Wyss Console"`, `version: string`.

The frontend has an app shell, a foundation component, a dedicated IPC client and
shared types. Rust separates bootstrap, the Tauri adapter, local application facts
and serialized contracts. These boundaries do not require speculative services or
empty crates. npm is the single frontend package manager; both lockfiles are tracked.
Build caches, runtime scratch data and generated outputs stay ignored inside the repository.

The minimal UI has a title and loading, success and error states. Readiness appears
only after IPC resolves. It displays the returned name, version and schema. It uses
system fonts, restrained spacing, OS light/dark preference and no motion or final
product navigation. This is a foundation screen, not the final product design.

## Sprint boundaries

Sprint 0A includes build, format, lint, typecheck and focused test commands, plus an
observed Linux desktop launch. The production build creates the native executable;
distribution packages and installers are deferred.

Sprint 0B may represent and validate EventEnvelope v1, introduce compatible
Rust/TypeScript fixtures and the minimal local identities needed by the next unit.
It must define transient content channels and backpressure before terminal output
is introduced. This preview does not authorize starting Sprint 0B in Sprint 0A.

## Exclusions and deferred mechanisms

Deferred: PTY, xterm.js, terminal emulation, terminal sessions, EventEnvelope runtime
types or validators, Event Bus, SQLite, persistence, external authority, daemon,
providers, agent adapters, managed agents/subagents, prompts, skills, permissions,
presets, routing, R0–R10, Decision Engine, telemetry, analytics, MCP, remote access,
plugins and auto updater. No speculative implementation accompanies these decisions.

System package installation is outside implementation authorization. Tauri exposes
only the main window's read-only application-information command. No broad core,
shell, filesystem, process, network or plugin capability is granted.

## Consequences

Later features consume explicit boundaries rather than frontend internals.
Sprint 0A deliberately has no session restoration or persistence demo. Existing
research's broader Sprint 0 deliverables are split into later units.

## References

- Wyss Console base specification v0.1, sections 7, 27 and Sprint 0/1.
- Accepted pre-implementation DECISION_PACKET, 2026-10-06.
