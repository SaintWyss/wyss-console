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

Creation/deletion with absent-versus-empty recovery; Codex skill
inventory/authoring; project authoring. F5 remains open. Individual Codex hooks still require
G-INF. See `remaining-roadmap-2026-10-19.md` and `../CHECKPOINT.md`.

## Subsequent increment: instruction templates (ADR 0046)

Existing instruction files now support **Export template…** and **Apply template…**.
The template is versioned and always passphrase-sealed in its own authentication domain.
It contains a kind and text, never a provider destination or a permission grant. Importing
opens an untrusted-text review in GTK; it does not write, activate or grant anything.
A separate ordinary plan and native confirmation are required.

Automated verification: frontend **800 tests** passed with formatting, lint, types and
build; Rust **501 library + 291 binary + 1 authority-boundary** tests passed, 12 remained
ignored; all-target clippy with `-D warnings` passed. The three additional Rust tests cover
byte-preserving encrypted round trips, cross-domain and cross-kind rejection, unknown
schemas/fields, oversized/tampered input, wrong passphrases, no write/grant on import,
editor compare-and-swap and refusal of native consent. Two additional frontend tests
verify ID-only export and import without automatic apply. Capability exceptions name the
single new export command exactly; generic filesystem/process/shell grants remain absent.

In the **normal native release GUI**, using the same real host, HOME, bus, state and wallet:

1. A new real Claude user rule `rules/wyss-console-acceptance-template.md` (75 bytes,
   mode 0600, SHA-256 prefix `377d159b850cf95d`) was backed up and selected.
2. Export opened the native masked passphrase dialog (two entries) and native Save chooser.
   The chosen `.wyss-template` was 511 bytes, mode 0600, contained the versioned encrypted
   envelope and neither the source instruction text nor the passphrase.
3. Apply template opened the native Open chooser and masked passphrase dialog. The native
   review displayed the exact original text and explicitly called it untrusted. At this
   point the real destination was still unchanged.
4. A harmless comment was added **in that native editor**, then the plan and native
   confirmation were accepted. The real file became 121 bytes. History → Undo → native
   confirmation returned it to the exact original bytes and mode.
5. After checking its backup, the newly created acceptance rule was removed, restoring
   its prior absence. Claude settings, Codex TOML and OpenCode JSONC kept their original
   checksums, owners and modes. No model or paid credential experiment was involved.

Native evidence: `.artifacts/acceptance/f5-template-2026-10-19/` (`before.json`,
`original.md`, encrypted template, native review screenshot, `result.json`). This was a
release package run against the actual environment, with no alternative installation or
HOME. It is not provider hot-reload evidence.

Remaining F5 scope: absent-aware creation/deletion, Codex skill inventory/authoring,
project authoring and hook/MCP templates. **The full product remains unfinished.**
