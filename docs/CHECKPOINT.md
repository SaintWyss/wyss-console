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

`npm run desktop:package` builds a private directory package and digest manifest; it
installs, publishes and migrates nothing. Installation, service operation and rollback
are separate actions. Follow the applicable [acceptance/runbook](acceptance/) only under
its actual authorization; a historical procedure is not current permission to repeat it.

Preserve running sessions. GUI-only `--keep-host` updates can retain older host/authority
components; never assume an updated window updated them. Installer rollback may stop the
host. Account for live work, matching binaries, consistent SQLite backups/WAL and the
post-snapshot loss window; no SQL downgrade or arbitrary PID cleanup. Repository checks
need no personal credentials or installed-service changes.

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
