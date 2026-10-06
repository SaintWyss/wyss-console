# Wyss Console

Desktop client for local terminals and, progressively, managed-agent control.
Linux-first, local-first. Current status: **Sprint 0B executable contracts** using Tauri 2,
Rust, Svelte, TypeScript and Vite. The window displays application information
returned by typed IPC. EventEnvelope v1 validates independently of IPC, using a
shared Rust/TypeScript fixture corpus. No Event Bus, storage or terminal exists.
Architecture decisions are in [docs/adr](docs/adr).

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
