# ADR 0047: Absent-aware creation and deletion of instruction files and skills (F5)

Status: Accepted within Santiago's 2026-10-19 product-completion authorisation
Date: 2026-10-20

Extend ADR 0045/0046 so the user can **create** a supported instruction file or skill where
none exists and **delete** one, with an exact way back. It reuses the one configuration
transaction (plan → native confirmation → lock → intent → sealed record → write → read back →
outcome → journal → recovery). There is no second mutation engine.

## What is supported

Claude Code user `CLAUDE.md`, `rules/*.md` and `skills/<name>/SKILL.md`; Codex user `AGENTS.md`,
`AGENTS.override.md` and, new, skills in `~/.agents/skills/<name>/SKILL.md` (the place Codex's own
`skills/list` reports, checked in F2b; they are a separate source, `codex_user_skills`); OpenCode
user `AGENTS.md`. The page sends a closed kind word (`NewKind`) or an inventory ID. Names and text
are typed in the desktop's own windows. Codex individual hooks stay read-only (G-INF).

## Absence is a state, not an empty file

A plan and the journal hold either a SHA-256 digest or the word `absent` (never valid hex). So
`create` is `absent → digest`, `delete` is `digest → absent`, and an empty file (a real digest)
can never be mistaken for a missing one. The undo of a creation removes exactly the created bytes;
the undo of a deletion recreates exactly the sealed bytes **and mode** and only while the name is
still free.

## Writing without overwriting

- **Create**: the bytes are made complete under a temporary name in the target directory, then
  take their name with `renameat2(RENAME_NOREPLACE)` (hard link on file systems without it).
  Anything that appeared at the name in the meantime—file, link, dangling link, directory—is
  never replaced: `ChangedOutside`, and the temporary file is removed. Mode is `0600`.
- **Delete**: the name is first re-checked against the identity (device, inode, size, mtime, mode,
  owner) read under the lock, then moved with `RENAME_NOREPLACE` to a tombstone in the same
  directory, whose identity is checked again before it is unlinked. If it is not the file that
  was read, it goes back to its name (or stays kept for recovery if a new file took it).
- Both refuse: files not owned by this user, with more than one name, writable by others,
  reached through any link (file or ancestor), outside the approved roots, in credential
  directories, or in a directory not owned by this user. A skill is deleted only when its
  `SKILL.md` is all its folder holds; a creation may make at most the folders the kind brings
  (`skills`/`<name>`, or `rules`), each `0700` and judged, never the provider's own folder.
  Those folders are removed again, empty and deepest first, on undo and on a failed or
  interrupted creation. A folder that gained a file is never removed.

## Recovery

An unfinished apply is closed at start from the journal and the disk. Missing files hash as
`absent`, so a tombstone of the old file, a temporary file of a creation, or folders made by a
creation are resolved by the same rules as the replace path; what cannot be accounted for is kept
(`.recovered-*`), never deleted. If the intent has no record yet, nothing could have been written
(the record is sealed first), so it is closed as `RecoveredNotApplied`.

## Same guarantees as ADR 0045

Always-sealed record (no plaintext fallback), native confirmation with `weakens_security`, the
plan view carries no text, a missing key store refuses before any plan, undo of an undo is not
offered. Capability: one new exact command, `configuration_create_document`; deletion uses the
existing `configuration_plan`. Nothing about the authority, SG-12 or Codex hook blocking changes.

## Not claimed

That a provider loads, reloads or obeys the new or removed file is **not observed**. Project-level
(per-repository) files are not written: the desktop has no project selection for the
configuration engine, and the roots are those of the user's account.
