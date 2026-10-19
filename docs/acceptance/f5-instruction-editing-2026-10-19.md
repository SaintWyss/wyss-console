# F5: native editing of existing instruction files

2026-10-19, `saint-acer`, normal CachyOS/KDE desktop. ADR 0045.
This is an increment of F5, not completion of the whole phase or product.

## Delivered

Native editing of existing Claude Code user skills, `CLAUDE.md` and direct `rules/*.md`,
Codex user `AGENTS.md`/`AGENTS.override.md`, and OpenCode user `AGENTS.md`. The page sends
only an inventory ID. The GTK editor receives the text, the native confirmation names the
file, and the ordinary configuration transaction applies it with an always-sealed exact
inverse. Concurrent changes, unsupported sources, links, hardlinks, conflict copies,
unavailable key storage and invalid/oversized text are refused.

The effect is explicitly “Written and read back. Reload and use by a running provider are
not observed.” Nothing changes the authority, its attestation, SG-12 or Codex hook blocking.

## Automated checks

- Frontend `npm run check`: 47 files, 797 tests passed; formatting, lint, types and release
  frontend build passed. The subsequent installer test addition passed in its six-test suite.
- Seven new Rust tests: all supported document locations and exact UTF-8/CRLF undo;
  editor/plan compare-and-swap; mandatory native consent and sealing; links/hardlinks,
  conflict copies and hidden controls; forged record targets; a failure and crash at each
  of eleven transaction steps; an imported inverse cannot request plaintext storage.
- `cargo clippy --offline --all-targets -- -D warnings`: PASS.
- Policy-compatible Rust regression: **501 library + 288 binary + 1 authority-boundary
  tests passed**, 12 real-system tests remained ignored. `cargo fmt --check` and the
  repository credential scan passed.
- The standard Rust command now selects `--lib --bins --test authority_boundary`.
  The inherited `lifecycle_process` target constructs a disposable user HOME and private
  systemd installation. A blanket suite run was interrupted on discovering this conflict
  with the owner's directive; it is **not** counted as completed acceptance. Its tests
  remain in the source, without weakening their assertions.

## Real installed GUI

Built with `npm run desktop:package`, installed at `~/.local/opt/wyss-console` using
`install --keep-host`. The previous installation remains at `.previous`. The real host
PID 1188641 and its idle fish PID 1188667 survived; no existing native agent was active.
The new installed GUI PID 1271311 used the normal HOME, XDG paths, session bus and wallet.
No private HOME, bus, keyring, container or extra user installation was created for this run.

Only two new, harmless acceptance files were introduced into the real Claude Code roots.
Their prior absence was recorded. Before editing, their owners/modes and SHA-256 were
recorded and verified backups were made. The files were:

- `~/.claude/rules/wyss-console-acceptance-f5.md`: 84 bytes, mode 0600,
  SHA-256 `b5e3d6493a38fd2304212af90073354382226ef159aed6e35b659cd16236bca1`.
- `~/.claude/skills/wyss-console-acceptance-f5/SKILL.md`: 182 bytes, mode 0600,
  SHA-256 `71a8b63747ea3db127a3b2421a6b1c7f016bfd31d09474e4bb95847adfbbd682`.

For each: Configuration → select item → Change it → **native GTK editor** → review plan →
**native confirmation** → Apply → check real bytes → History → Undo → native confirmation →
Apply. Both returned to exactly their original bytes and mode. Cancelling the native editor
also left the skill unchanged. The created files and empty skill directory were then removed
only after comparing them with their backups, restoring the original absence.

The existing real Claude settings, Codex TOML and OpenCode JSONC retained their checksums,
owners and modes. No content or credential was printed. The diagnostic CLI confirms the
normal Secret Service, real installed providers and compatible protocol-1 host.

Local evidence (not committed, contains no credential values):
`.artifacts/acceptance/f5-instruction-editing-2026-10-19/`, with `before.json`, originals,
`result.json` and `native-confirmation.png`.

## Provider evidence and limits

Claude Code 2.1.304 was started in the actual checkout with the normal HOME and environment
(the same session-marker scrub as the native launcher), and only local `/skills` and
`/memory` inputs. No model prompt was sent. This PTY-only attempt did **not** establish
that the provider's local browsers displayed the acceptance files: **NOT_PROVEN**.
It is not used to claim provider consumption or hot reload. No paid credential experiment
was performed. Codex/OpenCode document edits have unit coverage; their GUI/provider
acceptance has not been claimed by this increment.

## Still open

Creation/deletion with absent-versus-empty recovery; document templates; Codex skill
inventory/authoring; project authoring. F5 remains open. Individual Codex hooks still require
G-INF. See `remaining-roadmap-2026-10-19.md` and `../CHECKPOINT.md`.
