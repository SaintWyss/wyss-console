# F5: absent-aware creation and deletion of instruction files and skills

2026-10-20, `saint-acer`, the owner's normal CachyOS/KDE account. ADR 0047. An increment of F5,
not completion of the whole phase or product.

## Delivered

Create and delete for Claude Code user `CLAUDE.md`, `rules/*.md` and `skills/<name>/SKILL.md`,
Codex `AGENTS.md`/`AGENTS.override.md`, **Codex skills in `~/.agents/skills`** (new inventory
source, `codex_user_skills`, edit/create/delete) and OpenCode `AGENTS.md`; an optional start from a
sealed template; a name search in the Configuration view (391 skills made finding one by eye
impractical). All of it rides the one configuration transaction: absence is a journal state that
is not an empty file, creation never replaces (`RENAME_NOREPLACE`), deletion re-checks identity
and moves the file to a tombstone before unlinking, the sealed record holds exact bytes, mode
and the folders the change made, and recovery closes an interrupted create or delete.

## Automated checks

- Rust `cargo test --lib --bins --test authority_boundary`: **622 lib + 309 bin + 1 boundary
  passed**, 12 real-system tests ignored; all-target `clippy -D warnings` and `cargo fmt --check`
  passed. 18 new tests cover: every kind created and undone to true absence (including folders the
  change made); empty versus absent; creation never overwrites or follows a file, dangling link,
  link or directory; a file appearing between plan and apply and in the instant before the entry
  of the name; linked, group/other-writable or foreign places and bad names (`..`, separators,
  hidden, control and bidi characters, other extensions, other sources); skill front matter;
  refusal without a key store; removal keeps exact bytes and mode; removal refused for links,
  extra hard links, skills with other files; change after plan and provider write before the
  swap; undo never overwrites a file that took the name and never removes an edited file or a
  folder that gained a file; native refusal writes nothing; **a failure and a crash at each
  transaction step, for both creation and removal, repaired at the next start**; Codex skills
  inventory, edit and undo.
- Frontend `npm run check`: 48 files, **809 tests** passed (formatting, lint, types, build).
  New: closed kinds only, no text/path from the page, deletion through the ordinary plan,
  history lines whose file was `absent`, name search, authoring offers per source.

## Real installed-style GUI (the owner's own HOME, session bus and wallet)

A release build of this commit (unpinned, so it manages no authority and the running authority and
host were not touched) was run with the real `HOME`, XDG paths and session bus, in an isolated
display so no key or click reached the owner's desktop. Native GTK windows were driven with real
input. The real host PID 1188641 and the owner's GUI PID 1310334 were never touched; my own GUI
was ended by exact PID and the nested display was torn down by its own session.

Only harmless acceptance files were created, each absent before, and each removed again:

| Path (absent before and after)                                   | Result                                                                                                                                                                    |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `~/.claude/rules/wyss-console-acceptance-f5b.md` (34 B, 0600)    | create → native cancel (nothing written) → create → delete → undo delete (same SHA-256, 0600) → undo create (absent)                                                      |
| `~/.agents/skills/wyss-console-acceptance-f5b/` (SKILL.md 195 B) | create (folder 0700, file 0600) → **real Codex `skills/list` lists it** → delete → unlisted → undo → listed, same SHA-256, same modes → undo create → absent, folder gone |

Also in the real window: a name with path separators was refused as "cannot be changed from here",
nothing written; choosing `CLAUDE.md` (a symbolic link in this account) says "Something already has
that name… nothing was written" **before** any text window opens; a hyphenated name was typed through
the clipboard path of the native form.

Unchanged afterwards, with equal owner, mode and SHA-256 prefix: `~/.claude/settings.json`,
`~/.codex/config.toml`, `~/.claude/CLAUDE.md` (link), `~/.codex/AGENTS.md`,
`~/.config/opencode/opencode.jsonc`. No leftover temporary, tombstone or `.recovered-*` file, no
stray folder, `~/.agents/skills` returned to its 178 entries. No content or credential was printed.
The Console's own journal and sealed records keep the acceptance changes (they carry only the
benign test text) until their retention expires.

Defects found **only** in the real window and fixed with tests before this record: the history
page rejected the `absent` digest; a name with a separator was validated only after its text was
typed; the first window's accept button said "Write it"; the detail pane said "read-only" next to
its own Change/Delete buttons.

Local evidence (not committed, no credential values): `.artifacts/acceptance/f5-create-delete-2026-10-20/`.

## Provider evidence and limits

Real Codex 0.159.0 **discovers** a created skill and stops listing a deleted one (read-only
`skills/list`, `scope: user`, `enabled: true`); that is discovery, not consumption. That any
provider loads, reloads or obeys a created or removed instruction file is **NOT_PROVEN**. No model
prompt and no paid credential experiment was made. Claude Code and OpenCode discovery of the new
files was not checked in this increment.

## Still open in F5

Project-level (per-repository) instruction authoring: the desktop does not yet select a project for
the configuration engine, so its roots are the user's account only. Hook/MCP templates. Codex
individual hooks stay read-only until G-INF passes its security requirements.
