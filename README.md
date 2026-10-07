# Wyss Console

Desktop client for local terminals and, progressively, managed-agent control.
Linux-first, local-first. Current status: **Sprint 2D external authority foundation** using Tauri 2,
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
No Codex runtime/thread/turn is created. Stop authority using Ctrl+C or SIGTERM;
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
