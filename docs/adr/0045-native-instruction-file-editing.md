# ADR 0045: Native editing of provider instruction files (F5)

Status: Accepted within Santiago's 2026-10-19 product-completion authorisation
Date: 2026-10-19

## Decision

Extend the existing configuration transaction, native editor, journal and encrypted
restoration store. No authority or Managed protocol changes.

- Existing user files: Claude Code `skills/<name>/SKILL.md`, `CLAUDE.md` and direct
  `rules/*.md`; Codex `AGENTS.md` and `AGENTS.override.md`; OpenCode `AGENTS.md`.
  The provider's configured root is used. Project/system files, provider controls and
  hook files cannot be targets. Codex individual hooks remain blocked until G-INF.
- The webview sends an opaque inventory item identifier. The backend resolves raw names
  against bounded trusted roots; a redacted label is never a path. Only the native GTK
  editor receives instruction text. The page sees the destination and byte counts.
- Require an existing regular UTF-8 file of at most 64 KiB, without hidden direction or
  formatting controls. Reject symlinks, linked ancestors, hardlinks, foreign ownership,
  untrusted places and sync-conflict copies. Linked/shared skill installations are not
  edited through their aliases.
- Native confirmation is mandatory (`weakens_security`): editing instructions can change
  agent safeguards. Compare the bytes shown in the editor before making the plan, and
  recheck the plan digest under the target lock at apply. Preserve the existing exchange,
  displaced-provider-write protection, flushes and read-back checks.
- Keep the minimal changed byte range and its exact inverse using ADR 0042's UTF-8-safe
  common prefix/suffix. The inverse is **always sealed**. An unavailable wallet refuses
  the plan; there is no classifier-dependent plaintext fallback. Importing such a record
  cannot relax this by changing its untrusted outer `secret_bearing` flag.
- Undo requires the exact post-edit digest and reconstructs the exact prior digest.
  Imported restoration records must resolve to the same closed source/name grammar;
  they cannot reach hooks, trust, credentials or provider settings.
- Recover unfinished instruction transactions using their sealed target locator, with
  per-file intent matching even when several files share one source. Older builds ignore
  the new operation and cannot undo it; rollback leaves provider-valid Markdown and the
  sealed record intact for a newer build.
- Report only `document_written`: bytes written and checked. Provider reload/consumption
  is separate evidence and is never inferred from a successful write.

## Remaining F5 work

Creation and deletion of instruction files, document templates, Codex skill-directory
inventory/authoring and project authoring are not delivered by this increment. They must
preserve absent-versus-empty state and concurrent-writer recovery before being offered.
No security guarantee is relaxed to make an unsupported operation look available.

## Verification

See `../acceptance/f5-instruction-editing-2026-10-19.md` for the current test and real-GUI
record. Unit fixtures are not real-provider acceptance.
