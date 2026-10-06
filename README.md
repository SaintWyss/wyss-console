# Wyss Console

Desktop client for local terminals and, progressively, managed-agent control.
Linux-first, local-first. Current status: **Sprint 1A PTY foundation** using Tauri 2,
Rust, Svelte, TypeScript and Vite. Real local human shells run in bounded PTYs;
there is no terminal renderer yet. EventEnvelope contracts remain independent
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

## PTY diagnostic

In the desktop window, open **PTY diagnostic — Sprint 1A** and select **Run PTY
smoke**. It creates the account shell, writes `printf 'WYSS_PTY_OK\n'` and `pwd`,
checks returned bytes, resizes to 32×100, checks `stty size`, and closes the terminal.
The text preview is transient and capped; it is not a terminal emulator. Remove
or replace `src/lib/features/pty-diagnostic` when the Sprint 1B renderer is ready.

Sprint 1A uses `TERM=dumb` because no emulator answers terminal capability queries.
Human terminal survival across Console process crash is not guaranteed in Sprint 1A.
Graceful application exit closes its local terminals. PTY ownership, limits,
acknowledgements and failure semantics are recorded in ADR 0005.
