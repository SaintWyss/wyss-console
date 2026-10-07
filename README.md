# Wyss Console

Desktop client for local terminals and, progressively, managed-agent control.
Linux-first, local-first. Current status: **Sprint 2F execution gates, leases and revocation** using Tauri 2,
Rust, Svelte, TypeScript and Vite. Real local human shells run in bounded PTYs
and render through xterm.js, with native shell input, ANSI colors and resizing. EventEnvelope contracts remain independent
of transient terminal bytes. Architecture decisions are in [docs/adr](docs/adr).

## Development

Requires stable Rust, Node 26+, npm 12+ and the
[Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).

```sh
npm ci
npm run desktop:dev
```

`npm run dev` runs the frontend alone; application IPC requires the desktop host.
Development and native-run commands keep Cargo/XDG caches inside ignored `.cache/`.

## Validation

```sh
npm run check
npm run check:rust
npm run test:pty
```

Frontend commands: `format:check`, `lint`, `typecheck`, `test`, `build`.
Rust commands: `rust:fmt`, `rust:check`, `rust:test`, `rust:clippy`.
`npm run format` applies frontend/config/document formatting.

## Production build

```sh
npm run desktop:build
npm run desktop:run
```

Produces `src-tauri/target/release/wyss-console`. Distribution packaging is deferred.

## Interactive terminal

The application opens one local account-shell terminal automatically. **New Terminal**
creates an independent session/tab, up to 16 retained sessions (the backend also
limits PTY resources to 16). Each tab keeps its shell, screen and scrollback when hidden;
background output continues to be consumed. Type normally; keyboard input is transported
to that session's shell. The active terminal fits the window; hidden terminals retain
previous dimensions until activated.

`exit` retains the exited tab and final screen without respawning. A tab's close control
releases only its local resource; closing the final tab leaves an empty workspace.
New Terminal starts again explicitly. These are local human-terminal sessions, not
managed-agent sessions. Tabs, identities and scrollback are not persisted; restart
creates fresh shells. Tab arrows/Home/End move tab focus; Enter/Space activate.
No global terminal shortcuts are intercepted.

TERM is scoped to each child as `xterm-256color`. Scrollback is 5,000 volatile lines.
Output uses bounded byte batches acknowledged after xterm processing. Input is capped
at 64 KiB queued and 4 KiB per IPC write; an excessive paste fails visibly instead of
buffering indefinitely. Selection/paste use default WebView behavior, without added
clipboard permissions. External terminal hyperlinks are disabled.

Human terminal survival across Console process crash is not guaranteed at this stage.
Graceful application exit closes its local terminals. ADRs 0005–0007 record backend
ownership, renderer flow and local session ownership. No agents, Event Bus or persistence exist.

## Agent adapter contracts

Validated Rust/TypeScript v1 contracts describe agent adapters, capabilities,
version/compatibility evidence, observed managed-session state, models and safe
errors. Shared synthetic JSON fixtures prove classification and semantic round-trip
parity. They do not claim installed-runtime compatibility. See ADR 0008 and
[official runtime-surface research](docs/research/agent-runtime-surfaces-2026-10-06.md).
No agent SDK, authority or agent UI is connected. Existing local
terminal behavior is unchanged; managed-agent execution requires external authority.

## Codex protocol boundary

An offline Rust codec validates and normalizes a bounded subset of Codex App Server
messages using schema-derived synthetic fixtures. It includes initialization ordering,
thread/turn/item lifecycle, approvals and model catalogs. Shared common projections
are checked in Rust and TypeScript. Codex is not usable through the Console UI. See ADR 0009 for codec limits.

## Read-only Codex compatibility probe

The explicit developer harness discovers Codex, reads its version, starts a private
app-server, initializes with experimental API disabled, requests one model catalog
page and reaps its child. **No managed Codex work is executed yet.** No threads,
turns, prompts, approval responses, login or configuration changes are sent.

```sh
node scripts/tool.mjs cargo build --locked --manifest-path src-tauri/Cargo.toml --example codex_live_probe
./src-tauri/target/debug/examples/codex_live_probe
```

Build and execution are separate so the probe inherits your normal environment,
without the Cargo helper's project-local XDG overrides. It prints a safe JSON summary;
no raw stderr/protocol traffic is printed. Missing Codex is a reported unavailable
outcome (exit 0); an installed failed/incompatible probe exits 1. Normal tests do not
require or launch Codex. Linux process tests require standard coreutils.

Current local observation (2026-10-06): Codex 0.159.0 completed the read-only probe,
including legal interleaved notification metadata and a seven-model catalog page
with no next cursor. Compatibility is confirmed only for the exercised protocol
surface; listing is not entitlement. The owned app-server exited cleanly and was
reaped. See ADRs 0009–0010 and the research note. Managed execution still requires
external authority.

## External control authority

`wyss-control` independently owns prepared/closed managed-session identities,
canonical control state, durable intent receipts and a minimal EventEnvelope audit.
**No managed agent work executes yet.** Prepared is not execution permission.
Human terminal tabs remain Console-local; the read-only Codex probe above remains
a separate developer tool. The desktop does not start or connect to the authority.

Build the independent binary and developer client:

```sh
node scripts/tool.mjs cargo build --locked --manifest-path src-tauri/Cargo.toml --bin wyss-control --example control_client
```

Start independently, with a valid private XDG runtime directory:

```sh
./src-tauri/target/debug/wyss-control
```

From another terminal:

```sh
./src-tauri/target/debug/examples/control_client status
./src-tauri/target/debug/examples/control_client prepare
./src-tauri/target/debug/examples/control_client list
./src-tauri/target/debug/examples/control_client audit
```

The client also supports `hello`, `get UUID`, `close UUID revision`, and
`prepare intent-UUID` for durable retry. Close changes only the control record.
Prepared sessions create no Codex thread/turn. Stop authority using Ctrl+C or SIGTERM;
a normal control client cannot shut it down.

Socket: `$XDG_RUNTIME_DIR/wyss-console/control.sock` (0700 directory, 0600 socket).
Missing/unsafe runtime directories fail closed. State: `$XDG_STATE_HOME/wyss-console/control.sqlite3`,
fallback `$HOME/.local/state/wyss-console/control.sqlite3`. Product state is private.
SQLite WAL/FULL commits state, audit and intent receipts atomically. Do not delete
WAL/SHM files or change/copy an active canonical database casually. Client EOF
changes no state. Authority restart preserves committed state/stream/sequence.
Same-UID SO_PEERCRED is a local boundary, not protection from compromised same-user
processes. Maximum eight connected clients; idle connections expire after five
seconds. Session/audit pages are bounded to fifty entries. See ADR 0011.

`npm run rust:test` includes hermetic authority unit tests and actual process tests
using isolated temporary XDG directories. They do not require or launch Codex.
No production state is created by the test suite.

## Authority-owned Codex runtime

`wyss-control` can supervise one shared, long-lived `codex app-server`, independently
of Console clients. Durable desired running/stopped is separate from observed
starting/initializing/ready/stopping/failed. Schema v1/v2 upgrades transactionally to
v3, preserving existing sessions, receipts, stream, audit and runtime desired
state. Migration creates no runtime desired state; startup reconciles an existing
desired-running record. No ManagedSession is bound to this process.

The developer client exposes the new normalized control methods:

```sh
./src-tauri/target/debug/examples/control_client runtime/status
./src-tauri/target/debug/examples/control_client runtime/start
./src-tauri/target/debug/examples/control_client runtime/model/list
./src-tauri/target/debug/examples/control_client runtime/stop
```

Start/stop also accept an intent UUID for durable retries. Each new intent commits
its desired state/revision/audit before reconciliation; retry returns the original
receipt. Status observes readiness asynchronously. The Rust client supports catalog
pages of at most twenty descriptors with instance-scoped cursors. The catalog is
bounded to one fifty-model page, transient and refreshed for each new process;
listing is not provider entitlement. A rejected catalog fails safely without auth
remediation. No raw Codex frames/stderr/auth/environment are exposed or persisted.

The shared 2C handshake sends only initialize, initialized and model/list. Stdout
continues to be validated/drained after readiness and stderr remains bounded.
Server requests are never answered; protocol failure reaps the child. Linux
parent-death SIGKILL plus parent-identity checking prevents an orphan App Server
after authority crash. Graceful authority shutdown reaps the child but preserves
desired running; restart obtains a fresh AgentRuntimeInstanceId and catalog.
Explicit runtime/stop persists stopped and suppresses startup reconciliation.
Unexpected runtime failure does not create an automatic restart loop; a new start
intent admits one recovery attempt.

**Runtime process readiness is not execution authorization.** No Codex threads or
turns are created, no prompts execute, and no approvals/permissions are answered.
Tauri has no runtime command/capability or authority connection. Human terminal
behavior and the standalone read-only probe remain unchanged. See ADR 0012.

Rust tests include seeded v1 migration, idempotency/audit rollback, deterministic
fake App Server IO/failure/allowlist checks, graceful restart, explicit stop and
Linux SIGKILL containment. Normal tests use isolated XDG directories and a fake
executable, never the user's Codex runtime or production authority state.

## Execution leases and internal gate

Execution is denied by default. A prepared ManagedSession has no permission until
an explicit `executionLease/grant` commits a durable lease and audit. Runtime
startup, client reconnection and authority restart never grant or renew a lease.
The authority still creates **zero Codex threads or turns**, submits no prompts
and answers no approvals or permission requests. First managed work remains blocked.

Schema v3 adds only execution leases and a unique active-lease index; migration
preserves sessions, runtime desired state, receipts and exact audit bytes, and
creates zero leases. Each lease has its own ExecutionLeaseId and revision, explicit
nonempty scopes (`session_create`, `session_resume`, `turn_start`), and an
authority-time TTL of 1–3600 seconds. No scope implies another scope.

The typed Rust client and developer client support `executionLease/grant`,
`executionLease/get`, `executionLease/active`, `executionLease/renew` and
`executionLease/revoke`. Grant takes session ID, expected session revision, TTL,
comma-separated scopes and intent UUID. Renew takes lease ID, expected lease
revision, TTL and intent UUID; revoke takes lease ID, revision and intent UUID.
Get/active take the corresponding lease/session ID. Renewal preserves scopes and
uses authority NOW + TTL. Original intent retries return their original receipt,
including the original expiry; UUID case and scope order are normalized.

`executionGate/status` takes session ID and one scope, returning an informational
reason only. It is not an authorization token. Future execution must reevaluate
the internal gate immediately before the authority-owned side effect. It requires
prepared session, active unexpired scoped lease, matching runtime metadata,
desired running, actual ready, live runtime identity, compatible runtime and
healthy canonical storage. A valid lease with stopped runtime still denies;
runtime readiness without a lease also denies.

The single authority writer expires overdue leases at startup and through a
deadline-driven scheduler. Get returns effective state without writing or inventing
a revision; active never returns an overdue lease. Gates compare UTC time even if
expiry persistence is delayed. Storage/audit failure latches execution closed.
Closing a session atomically expires/revokes its lease before closing the session.
Runtime stop leaves leases unchanged. Graceful/hard authority restart preserves
unexpired lease identity, revision and exact deadline, without extending it.
See [ADR 0013](docs/adr/0013-execution-gates-leases-and-revocation.md).
