# Development and maintenance

This page owns engineering commands and operational procedures. Read [STATUS](STATUS.md)
for capabilities, [ROADMAP](ROADMAP.md) for remaining work and [DECISIONS](DECISIONS.md)
for the current constraints.

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
Real acceptance uses the owner's normal desktop, HOME/XDG/session bus and wallet.
Old disposable HOME/keyring/bus/VM/container procedures are historical evidence,
not current recipes. Ordinary unit fixtures and intrinsic Managed isolation remain
distinct from those procedures.
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
anchors, applicable requirements/acceptance criteria and the intended Git scope;
application builds/provider experiments are unnecessary. Resource moves require their
consumer checks. Keep formatting limited to files changed by the task. `scan:repo` includes staged new files.

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

The output contains `bin/wyss-console`, `libexec/wyss-console/{wyss-control,wyss-screen,wyss-credential-agent}`
and `share/wyss-console/{package.json,install.sh,icon.png}`. The packaged installer is the
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
Opening the current GUI does not install, restart or replace an authority. Ordinary
service activation is an explicit operator path, separate from opening the window. The installer alone does not migrate a database or enable/start
an F8 service. Optional `--host-service` writes a native-host unit but does not enable
it; that flag is not needed for the ordinary example either.

For an existing F8-enabled installation, do **not** use the ordinary example as an
upgrade recipe: it rewrites the menu entry without `--f8-authority`. Preserve its
reviewed F8 installation settings and use the configured F8 menu entry; a bare binary
or symlink launch does not select F8.

F8 setup uses `install --f8-service --codex ABSOLUTE_PATH` with the pinned Codex 0.159.0;
`--allow-credential` is a separate explicit choice. Those options write an F8 service
unit and a menu entry with `--f8-authority`; they do not complete first-start store
creation/migration or service activation. Use the procedure below only for the separately
authorized operation; documentation maintenance does not execute it.

### F8 backup, activation and rollback

1. Record live host/authority identities, active obligations, installed package/provider
   digests, current P1/P2 selection and recovery loss window. Preserve existing terminals
   with `--keep-host`; do not stop a host to match a GUI version.
2. From the repository root, `bash scripts/f8-activation.sh backup` takes a consistent online
   SQLite backup of P1 `control.sqlite3`, checks schema 7/integrity/foreign keys and records
   row counts/digest in `MANIFEST.json`; it copies companion anchor/layout when present.
   Keep the printed backup path. Live counts may advance while the backup is taken.
   Anchor/layout are copied separately after the DB snapshot; this is not an atomic
   multi-file backup. Resolve the [DB/witness consistency procedure](ROADMAP.md#eng-04--db-and-witness-consistency)
   before relying on that companion set for real recovery.
3. With the exact selected authority binary, use
   `bash scripts/f8-activation.sh rehearse --backup BACKUP_DIR --authority AUTHORITY_BIN`.
   It migrates a copy to a temporary state root, checks V8 integrity and preserved rows,
   then removes that rehearsal image. This is data migration, not a provider/auth experiment.
4. Install the reviewed package with `--keep-host --f8-service --codex ABSOLUTE_CODEX_PATH`;
   add `--allow-credential` only if credential binding is intended. Do not replace an existing
   F8 store: inspect any previous attempt and preserve it separately as an explicit recovery
   action. The migration refuses an existing image; do not delete it to make a retry pass.
5. When the F8 service is stopped and existing work has been accounted for, run
   `bash scripts/f8-activation.sh migrate --backup BACKUP_DIR --authority AUTHORITY_BIN`
   (optional `--state-root DIR`). It creates a separate V8 image, checks row preservation
   and leaves the live P1 store untouched. This does not perform the proposed V9 migration.
6. Explicit activation uses `systemctl --user daemon-reload` and
   `systemctl --user enable --now wyss-console-f8.service`. Open the configured F8 menu
   entry and verify protocol/store/session identities. Real credential/provider turns and
   restart recovery are separate acceptance actions, never implicit smoke tests.

`bash scripts/f8-activation.sh rollback` disables/stops F8 and may invoke package rollback,
which can close terminals. Plan it around owned work; preserve the P2 store. The untouched
P1 store needs no restore. Restoring an actual older backup is a different operation with
its writer stopped, consistent sidecars/witness and an explicit post-snapshot loss window.
There is no SQL downgrade or automatic history merge. Exact options and refusal behavior
are owned by [f8-activation.sh](../scripts/f8-activation.sh) and [install.sh](../scripts/install.sh).

Keep matching binaries and consistent SQLite backups/WAL with an explicit post-snapshot
loss window; no SQL downgrade or arbitrary PID cleanup. Repository documentation checks
require no installation, personal credentials or installed-service changes.

## Engineering handoff

Follow [EXECUTION](EXECUTION.md) for source-based review, work readiness and the division
between design, implementation and verification. Follow the task's focused checks rather
than treating every implementation slice as an invitation to run providers or touch live state.
CI's disposable-runner suites and namespace setup are engineering fixtures; they do not
qualify the owner's normal desktop, provider authentication or physical Android hardware.
Current workflow definitions are in [.github/workflows](../.github/workflows/); this guide's
native-environment rules still apply to local acceptance.

## Documentation maintenance

- Use the six question routes in [README](../README.md); each kind of information has
  one owner. Product owns scope, STATUS capabilities/evidence, ROADMAP remaining work,
  DECISIONS current choices, ARCHITECTURE component contracts and this page commands.
- Update the current explanation in place. Remove completed tasks from ROADMAP; do
  not append delivery diaries, old branch names or historical acceptance counts.
- Preserve applicable functional/acceptance requirements and technical consumers.
  Keep legacy IDs only where a consumer or useful traceability still needs them.
- Consolidate unique decisions, constraints and unresolved findings before deleting
  their original report. Proposals do not become accepted during consolidation.
- Retire replaced drafts, snapshots and reports from the current tree; use Git for
  historical versions. Do not create another archive folder or duplicate inventory.
- Keep specialized contracts and fixtures only where their current role is clear.
  Dated logs/screenshots/results belong in PR or external execution artifacts unless
  they remain necessary evidence or reproducible test inputs.
- Documentation changes do not run provider calls, migrate databases, replace
  services or accept a security decision. Choose checks appropriate to the change.
