# Wyss Console

Desktop client for local terminals and, progressively, managed-agent control.
Linux-first, local-first. Current status: **Sprint 2A agent adapter contracts** using Tauri 2,
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
No agent runtime, SDK, network, authority or agent UI is connected. Existing local
terminal behavior is unchanged; managed-agent execution requires external authority.
