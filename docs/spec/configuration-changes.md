# Changing the configuration (F2a)

What the Console can change in Claude Code's own files, how it does it, what it keeps, and
what it promises. The decision behind the storage is [ADR 0029](../adr/0029-encrypted-restoration-store-and-secret-bearing-entries.md);
the machinery is described in [07 §5](../architecture/07-CONFIGURATION-ENGINE.md) and the
actions in [08](../architecture/08-RULES-HOOKS-SKILLS-MCP.md). Only this user's Claude Code
settings files are ever written: `~/.claude/settings.json` (or `CLAUDE_CONFIG_DIR`), and for
a selected project `.claude/settings.json` and `.claude/settings.local.json`.

## Operations

| Operation           | Effect on the file                                                            | Kept               | Risk (11 §4)                                                                                |
| ------------------- | ----------------------------------------------------------------------------- | ------------------ | ------------------------------------------------------------------------------------------- |
| `park_hook`         | The handler is removed (its group, or its event, if it was the last in it)    | A sealed record    | `weakens_security` for `PreToolUse`, `PermissionRequest`, `UserPromptSubmit`; else `normal` |
| `restore_hook`      | The kept handler is put back                                                  | Record marked used | `executes_code`                                                                             |
| `disable_all_hooks` | `"disableAllHooks": true` is added or set                                     | Nothing            | `weakens_security`                                                                          |
| `enable_all_hooks`  | The key is removed                                                            | Nothing            | `executes_code`                                                                             |
| `skill_off`         | `"skillOverrides": { "<name>": "off" }`                                       | Nothing            | `normal`                                                                                    |
| `skill_on`          | The override is removed. An override with another value is left alone (OQ-15) | Nothing            | `normal`                                                                                    |

Nothing else is changed: not Codex, not MCP servers (they need a provider process), not a
provider-owned file, not a system source, not a file in a directory a group can write.

## How a file is edited

The file is read with a JSON reader that remembers where everything is. A change removes or
inserts exactly the bytes of the change, with the separator that goes with them, so the rest
of the file is the same bytes: layout, key order, spacing and line endings are the user's.
A file with a repeated key, deeper than 64 levels, or not valid JSON is not edited.

Every plan carries the same change made twice, to the bytes and to the parsed value, and is
offered only if they agree. The writer checks them again on what it reads back. Putting a
hook back into a file that is as the removal left it reproduces the original bytes
(AC-F2a-02); into a file that has moved on, it is planned against the file as it is, at the
end of its group, and refused when a different entry is in the way (`restore_conflict`).

## An apply

1. The plan is taken (single use; it lives 60 seconds; at most eight at once).
2. A risk above `normal` asks in a native window the page cannot draw on or answer for. A
   refusal writes nothing and is journaled (item, operation, risk class; no content).
3. The target is locked (advisory, dies with its holder). The file is read again; its hash
   and identity must be the plan's (`changed_outside`); a `*.sync-conflict-*` copy beside it
   stops the change (`conflict_copy_present`).
4. The **intent** is journaled. If that fails, nothing is done (`not_recorded`).
5. For `park_hook`, the removed entry is **sealed and written** (ADR 0029).
6. A new file is written beside the old one with the old one's mode, flushed, checked to be
   still the file that was read, renamed over it, and the directory flushed.
7. The file is read back; its hash must be the plan's (`verify_failed` otherwise, and the
   user is told).
8. The **outcome** is journaled. The inventory is read again.

A write goes to the file a link leads to and never replaces a link. The directory is reached
by descriptors judged on the way down, so a link put in place of a directory after the check
is not followed.

### Repair at start

An intent with no outcome is an apply the application did not finish. The file's hash says
how far it got: the plan's → `recovered_applied`; the old one's → `recovered_not_applied`
(and the spare record is deleted); neither → `recovered_unverified`. Temporary files of an
interrupted write are removed by their exact name pattern. Every step above is covered by a
test that fails it and one that crashes at it.

## What is kept

| Store               | Where (mode 0700 / 0600)                                          | Holds                                                                                                                                                                                                               |
| ------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Journal             | `~/.local/state/wyss-console/configuration/journal/journal.jsonl` | Per change: identifier, time, provider, source, operation, item identifier, risk, hash before and after, outcome, record identifier. No content, no value, no path. 10,000 entries or 180 days                      |
| Restoration records | `…/restoration/<id>.rec`                                          | Clear: identifier, time, state, redacted label, whether the entry may hold a secret, how it is protected. Sealed: the removed entry and where it was. 500 records or 180 days; a parked one is never removed by age |
| Locks               | `…/locks/`                                                        | Empty files                                                                                                                                                                                                         |

The key that seals a record is in the system's Secret Service (service `wyss-console`) and
nowhere else. If it cannot be had, an entry that may hold a secret is not turned off one by
one, and the plan says so; an entry classified free of secrets may be kept as a private file,
and the plan says that before the user confirms. Losing the key makes sealed records
unreadable (`key_lost`); the history still says what was done.

## Window commands

`configuration_plan`, `_apply`, `_history`, `_undo`, `_records`, `_record_delete`, `_export`,
`_import`. Each names something the desktop already holds (an item, a record, a plan, a
change, a source's closed word) and rejects unknown fields; none carries a path or the text
of an entry. A plan shows the entry as redaction shows it. Export and import ask for a
passphrase and a file in native windows; the export is sealed under a key derived from the
passphrase (PBKDF2-HMAC-SHA256, 600,000 rounds, random salt).

## Limits, said plainly

- A provider takes no lock. A write by Claude Code between the last check and the rename is
  either seen by the read-back (it lands after) or lost to the rename (it lands before, in
  milliseconds that were not measured). Narrowing and detection are RV-03 / OQ-31.
- "Applied" means the file holds the change. That Claude Code adopts it in running sessions
  is documented, not verified for the user's sessions (07 §6).
- `enable_all_hooks` removes the key; an explicit `false` that was there before is not
  restored. `skill_on` removes the override; an override with another meaning is not touched.
- A same-user process can ask the Secret Service for the key while it is unlocked. No
  protection against it is claimed (ADR 0029).
- Item identifiers are positions in the file; a plan is bound to the file's hash, so a file
  that moved on makes a new plan necessary, never a wrong edit.
