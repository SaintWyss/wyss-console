# Development and maintenance

This page owns engineering commands and documentation policy. Current product state
belongs only to [implementation status](product/implementation-status.md); work order
belongs to the [roadmap](product/roadmap.md). Start with the relevant item, then follow
its functional requirement and architecture references through the
[documentation map](product/README.md).

## Prerequisites and commands

Linux/CachyOS KDE, Rust, Node26+, npm12+ and the
[Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).
Managed provider/version/isolation requirements are recorded in the status page and
linked contracts, not duplicated here.

```sh
npm ci
npm run desktop:dev
```

`desktop:dev` uses the native IPC host; `npm run dev` alone serves the frontend.
For code changes, select the relevant checks; the full repository suites are:

```sh
npm run check
npm run check:rust
npm run scan:repo
```

`check` covers formatting, lint, types, frontend tests, the owned-process harness and
frontend build. `check:rust` uses the repository's selected fmt/check/test/clippy suite.
**Do not substitute blanket `cargo test` or `lifecycle_process`**: the latter creates
an inherited private user/systemd environment. Do not enable ignored real-provider or
credential-store tests merely because an old report mentions them. Real acceptance
uses the owner's normal native environment; unit fixtures and intrinsic Managed
isolation are distinct from historical disposable HOME/keyring/bus/VM procedures.

For documentation-only changes, check changed Markdown formatting, relative links and
anchors, FP/AC and legacy crosswalk preservation, and documentation-only Git scope;
application builds/provider experiments are unnecessary. Never blanket-format historical
ADRs/evidence to tidy unrelated files. `scan:repo` includes staged new files.

## Packaging and live installation

### Ordinary package installation and launch

From the repository root, build the package after installing the prerequisites and
running `npm ci`. Example for version 0.2.0 on Linux x64; if your build differs, set
`wyss_package` to the exact directory printed by the packaging command:

```sh
npm run desktop:package
wyss_package=".artifacts/package/wyss-console-0.2.0-linux-x64"
bash "$wyss_package/share/wyss-console/install.sh" install --package "$wyss_package" --keep-host
"$HOME/.local/opt/wyss-console/bin/wyss-console"
```

The output contains `bin/wyss-console`, `libexec/wyss-console/wyss-control` and
`share/wyss-console/{package.json,install.sh,icon.png}`. The packaged installer is the
same [installer](../scripts/install.sh) shipped by [package.mjs](../scripts/package.mjs).
It checks binary digests, installs per-user without `sudo` to
`~/.local/opt/wyss-console`, retains the previous package at that path plus `.previous`,
and writes the Wyss Console application-menu entry and `~/.local/bin/wyss-console`
symlink. Launch through the menu or the absolute command above; the symlink also works
when `~/.local/bin` is on PATH. Building alone does not install or launch anything.

**Preserve live terminals:** `--keep-host` skips the installer's host-stop request.
Without it, installation asks the current host to stop, closing its terminals. With
it, the existing host and terminals continue on their running version until it exits;
new GUI features may remain unavailable on that older host. It does not restart or
upgrade a running authority. Close/reopen only the GUI when needed; do not stop the
host or services just to make versions match. Rollback has no `--keep-host` option and
can close terminals, so it needs a separate session-aware operational plan.

### F8 service activation is separate

The ordinary example above selects the normal protocol1 desktop path, **not F8**.
The packaged GUI may bootstrap its ordinary authority lifecycle on launch; that is not
protocol2 activation. The installer alone does not migrate a database or enable/start
an F8 service. Optional `--host-service` writes a native-host unit but does not enable
it; that flag is not needed for the ordinary example either.

For an existing F8-enabled installation, do **not** use the ordinary example as an
upgrade recipe: it rewrites the menu entry without `--f8-authority`. Preserve its
reviewed F8 installation settings and use the configured F8 menu entry; a bare binary
or symlink launch does not select F8.

F8 setup uses `install --f8-service --codex ABSOLUTE_PATH` with the pinned Codex 0.159.0;
`--allow-credential` is a separate explicit choice. Those options write an F8 service
unit and a menu entry with `--f8-authority`; they do not complete first-start store
creation/migration or service activation. Follow the separately authorized
[F8 activation and rollback procedure](acceptance/f8-activation-procedure-2026-10-22.md)
for that operation, including live-session preservation and `--keep-host`. The historical
record is not permission to repeat migration, credential use or service operations.

Keep matching binaries and consistent SQLite backups/WAL with an explicit post-snapshot
loss window; no SQL downgrade or arbitrary PID cleanup. Repository documentation checks
require no installation, personal credentials or installed-service changes.

## Documentation maintenance

- **Update the canonical owner first.** The [six-page map](product/README.md) assigns one
  location per information kind. Change behaviour in the functional specification,
  ownership/security in architecture, delivered evidence in status, and sequencing in
  roadmap; avoid copying those explanations into README or a new checkpoint.
- Create a document only for genuinely separate detailed contracts or dated evidence.
  Link it from its canonical owner and label scope, baseline and status. Keep such
  references out of the routine reading path unless the task needs them.
- Preserve all 31 FP IDs, matching ACs and 208 legacy mappings in
  [traceability](product/traceability.md). When behaviour changes, update its crosswalk
  and acceptance criterion together. Distinguish specification, fixture result and
  production observation; a passed docs check proves no provider capability.
- Accepted ADR decisions and historical findings/evidence remain intact. Extend decisions
  through a separately reviewed ADR; new proposals stay Proposed until the owner accepts.
  Correct current interpretation by reference instead of rewriting historical outcomes.
- README is the short repository entrance; `docs/product/README.md` is the ownership map.
  Store dated validation results in an existing applicable report or PR, not these pages.
