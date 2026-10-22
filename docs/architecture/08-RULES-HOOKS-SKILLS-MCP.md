# 08 — Rules, hooks, skills and MCP

> **2026-10-22 consolidation:** this document retains dated design/evidence and detailed
> contracts. Its present-tense status/decision tables describe their stated baseline,
> not necessarily current implementation. Read the [current product set](../product/README.md),
> [verified gaps](../product/implementation-status.md) and [requirement crosswalk](../product/traceability.md)
> first. F8 is active P2/V8, ADR0051 Accepted, cap1; P1 remains available. New product
> direction does not supersede accepted security ADRs or turn proposed tests into PASS.

Domain E of [04](04-TARGET-ARCHITECTURE.md): what every action the user can take on a
configuration entry does, per provider, when it succeeds and when it fails. The machinery is in
[07](07-CONFIGURATION-ENGINE.md). All `[P]` unless marked. The complete capability is designed
here; the phase that delivers each part is given per action.

## 1. Kinds of entry

| Kind       | Claude Code                                       | Codex                                         | OpenCode                   | Hermes               |
| ---------- | ------------------------------------------------- | --------------------------------------------- | -------------------------- | -------------------- |
| Hook       | A handler in a matcher group under an event `[D]` | The same shape; `command` or `mcp_tool` `[D]` | A plugin `[D]`             | `[N]`                |
| Rule       | `CLAUDE.md`, files in `rules/` `[D]`, `[O]`       | `AGENTS.md` `[O]`                             | `instructions` globs `[D]` | `SOUL.md` `[D]`      |
| Skill      | A directory under `skills/` `[O]`                 | A `[[skills.config]]` entry `[O]`             | `[N]`                      | `skills/` `[D]`      |
| MCP server | An entry in `~/.claude.json` or `.mcp.json` `[D]` | `[mcp_servers.<name>]` `[D]`                  | `mcp.<name>` `[D]`         | `mcp_servers` `[D]`  |
| Plugin     | `enabledPlugins` `[O]`                            | `[plugins]` `[O]`                             | `plugin` list `[D]`        | context engine `[D]` |
| Setting    | A known key of the settings file                  | A known key of `config.toml`                  | A known key                | A known key          |

A rule kept as free Markdown is not a structured hook, and a skill's directory is not a switch.
The engine never treats one as the other.

## 2. State of an entry (SM-ENTRY)

Every state but one is read from the provider's file: a computer that has never run the Console
reads the same state from the same file. The exception is `vault_disabled`, which exists only on
the computer whose vault holds the entry (ADR 0029 §3, rung 3, proposed;
[27](27-PROTECTED-RESTORATION-VAULT.md)).

| State             | Meaning                                                                                                                                                                                                                                                          |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `enabled`         | Present in the provider's file and not switched off                                                                                                                                                                                                              |
| `disabled`        | Present and switched off by a state the provider itself has                                                                                                                                                                                                      |
| `kept_disabled`   | Disabled by the Console and kept inside the same file, in a form the provider does not act on. Exists only for a provider version whose compatibility test passed (ADR 0029 §5)                                                                                  |
| `vault_disabled`  | Disabled by the Console on this computer: removed from the provider's file and sealed in the protected vault. The provider's file no longer holds it. Another computer that shares the file sees `removed_outside`                                               |
| `removed_outside` | Gone from the file by an outside change, or disabled on another computer into that computer's vault. This computer knows that it existed and when it went. It cannot restore it, unless the user had chosen to keep a recovery copy of that entry here (27 §3.6) |
| `absent`          | Not in the file; nothing is known about it                                                                                                                                                                                                                       |
| `trust_pending`   | Present, but the provider will skip it until the user reviews it there (Codex)                                                                                                                                                                                   |
| `unavailable`     | The operation that would change it is not offered, with the reason                                                                                                                                                                                               |
| `unreadable`      | Its source cannot be read or parsed                                                                                                                                                                                                                              |

| From              | To                | Initiator      | Guard                                                                                                                                                                | Side effect                                                                                                                 | On failure                                                                                                                                                                                                                                                                 |
| ----------------- | ----------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `enabled`         | `disabled`        | User           | The provider has a native switch; version verified; for Codex hooks, DN-19                                                                                           | Switch written (07 §5.4)                                                                                                    | An outcome of SM-WRITE; file as it was or as the provider left it                                                                                                                                                                                                          |
| `enabled`         | `kept_disabled`   | User           | No native switch; the in-place form is verified for this version; native dialog if the risk class asks                                                               | Entry moved to its kept form in the same file                                                                               | The same                                                                                                                                                                                                                                                                   |
| `enabled`         | `vault_disabled`  | User           | No native switch; no verified in-place form for this entry; consent on record; the vault ready; native dialog if the risk class asks                                 | Record sealed and verified, then the entry removed from the file (27 §8.1)                                                  | The record is purged as unused and the file is as it was or as the provider left it; an outcome of SM-WRITE                                                                                                                                                                |
| `enabled`         | `unavailable`     | —              | No native switch, no verified in-place form, and the vault cannot be used for it yet (its contract not accepted, its key custody not answering, the entry too large) | None: the action is not offered, with the reason and what would enable it                                                   | —                                                                                                                                                                                                                                                                          |
| `disabled`        | `enabled`         | User           | Native switch; native dialog (`executes_code`); for Codex hooks, G-INF                                                                                               | Switch written                                                                                                              | As above                                                                                                                                                                                                                                                                   |
| `kept_disabled`   | `enabled`         | User           | The kept form is intact; nothing else occupies its place; native dialog                                                                                              | Entry put back; file byte-identical to before it was disabled when nothing else changed                                     | As above; `restore_conflict` if its place is taken                                                                                                                                                                                                                         |
| `kept_disabled`   | `absent`          | User (delete)  | Confirmed; a different action from disabling                                                                                                                         | Kept form removed                                                                                                           | As above                                                                                                                                                                                                                                                                   |
| `kept_disabled`   | `kept_disabled`   | Outside change | Someone edited the kept form                                                                                                                                         | Shown as changed outside; restoring shows the current content first                                                         | —                                                                                                                                                                                                                                                                          |
| `vault_disabled`  | `enabled`         | User           | The record authenticates; its source is the same file; native dialog; and the place is exact or unambiguous (27 §3.7)                                                | Entry put back: byte-identical when the file has not changed since; otherwise at its one unambiguous place, shown as a diff | The record stays. `restore_conflict` with its reason (`placement_ambiguous`, `anchor_missing`, `container_missing`, `container_ambiguous`), `already_present`, `source_missing`, `source_identity_mismatch`, or an outcome of SM-WRITE. Nothing is placed by approximation |
| `vault_disabled`  | `absent`          | User (delete)  | Native dialog that says this is the only copy                                                                                                                        | Record removed from the vault                                                                                               | —                                                                                                                                                                                                                                                                          |
| `vault_disabled`  | `enabled`         | Outside change | The same entry reappeared in the file                                                                                                                                | The record becomes redundant and is purged after acknowledgement                                                            | —                                                                                                                                                                                                                                                                          |
| `enabled`         | `removed_outside` | Outside change | The engine had seen it                                                                                                                                               | A journal entry with identifiers; no content                                                                                | —                                                                                                                                                                                                                                                                          |
| `removed_outside` | `enabled`         | Outside change | The same entry reappeared                                                                                                                                            | Journal entry                                                                                                               | —                                                                                                                                                                                                                                                                          |
| `removed_outside` | `enabled`         | User (recover) | A recovery copy of that entry exists on this computer and authenticates; native dialog                                                                               | The copy is inserted at its recorded place, by its neighbours, and shown as a diff; never claimed byte-identical            | The copy stays; `restore_conflict`, `source_missing`, or an outcome of SM-WRITE                                                                                                                                                                                            |
| any               | `trust_pending`   | Provider       | The entry's content differs from what the provider trusted                                                                                                           | None by the Console                                                                                                         | —                                                                                                                                                                                                                                                                          |
| `enabled`         | `absent`          | User (delete)  | F5; confirmed                                                                                                                                                        | Entry removed; not restorable, said before                                                                                  | As above                                                                                                                                                                                                                                                                   |

## 3. Hooks: every action

### 3.1 Inspect (F1)

Shown for each hook: provider, scope, source file, event, matcher, handler type, the command or
target (redacted by the rules of 06 §4.2), timeout, persisted state, effective state, sessions,
and for Codex its trust state as far as it can be read from the file.

Validation reported, never enforced, in F1 (FR-HRS-05): unknown event name; handler type the
provider skips; timeout out of the documented range; a command whose first word is not an
existing executable (checked by `stat`, never by running it); a hook defined twice.

| Outcome                | What the user sees                                                                                      |
| ---------------------- | ------------------------------------------------------------------------------------------------------- |
| Source read            | The list                                                                                                |
| Source malformed       | The source's name, "cannot be parsed at line N", and nothing of its content; other sources still listed |
| Source unreadable      | "Cannot be read (permission)"                                                                           |
| Source too large       | "Larger than the limit; not read"                                                                       |
| Provider not installed | Files listed if present, marked "provider not installed"                                                |

### 3.2 Disable

| Provider               | Mechanism                                                                                                                                                                                                                                                                               | Phase                               | Effect reported                                                                                                                                                                                                        |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude Code, one hook  | No native switch `[D]`. Rung 2, kept in place in the same file: only for the cells where checks V1 to V7 of ADR 0029 §5 passed `[N]`. **Otherwise rung 3: removed from the file and sealed in the protected vault of this computer** (27). Unavailable until one of the two can be used | F2v; rung 2 only after research RS1 | Rung 3: "Disabled. Removed from settings.json and kept, encrypted, on this computer. Your other computer cannot restore it. Claude Code documents that running sessions pick this up; not verified for your sessions." |
| Claude Code, all hooks | Native: `disableAllHooks: true`                                                                                                                                                                                                                                                         | F2a                                 | "All hooks off. Managed hooks, if any, stay on."                                                                                                                                                                       |
| Codex                  | Native switch, once its storage is known (OQ-01). Whether it is offered at all before G-INF is DN-19                                                                                                                                                                                    | F2b                                 | "Switched off in Codex's configuration. Effect on running sessions not confirmed."                                                                                                                                     |
| OpenCode               | A plugin has no switch `[D]`. Unavailable unless a kept form is verified for it                                                                                                                                                                                                         | F4a                                 | —                                                                                                                                                                                                                      |

Risk: disabling a hook is `weakens_security` when the hook is of an event that can block an
action (`PreToolUse`, `PermissionRequest`, `UserPromptSubmit` and their equivalents) or when the
user has marked it as a security control; otherwise `normal`. The classification is the
backend's (11 §4).

| Failure                                         | Result                                                                                                                                                                                                                                                 |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| No way to keep the entry restorable yet         | Not offered, with the reason: "Claude Code has no switch for one hook. Keeping it needs Wyss Console's protected storage, which is not set up" or "is locked" or "is not available in this version". The coarse switch and the file's path are offered |
| Consent to the vault refused                    | Nothing sealed, nothing written; the action stays unavailable until consent is given                                                                                                                                                                   |
| The key's custody does not answer               | The plan is refused before confirmation: `vault_locked`, `vault_no_custody` or `vault_key_missing`                                                                                                                                                     |
| A session of the agent is open                  | `provider_session_active`: refused, with "close your Claude Code sessions to change this" (28 §5.1)                                                                                                                                                    |
| The record cannot be written or does not verify | `storage_failed` or `seal_unverified`; the provider's file is untouched                                                                                                                                                                                |
| The file changed since the plan                 | `not_applied`; new plan offered                                                                                                                                                                                                                        |
| The entry is no longer there                    | `entry_gone`; the inventory refreshes                                                                                                                                                                                                                  |
| The entry is there but differs                  | `entry_changed`; the user is shown the current one and must plan again                                                                                                                                                                                 |
| Native dialog refused                           | Nothing written to the file; the journal records the refusal (item, operation, risk class; no content)                                                                                                                                                 |
| The provider wrote at the same moment           | `lost_race`, `superseded_outside`, `conflict_preserved` or `recovery_incident` (07 §5.4), each with its sentence. A write the Console could not observe has no outcome at all (07 §5.5)                                                                |
| Write failed                                    | The file as it was; error shown                                                                                                                                                                                                                        |
| Read-back differs                               | `unverified`; the user is shown what the file holds now                                                                                                                                                                                                |

### 3.3 Enable and restore

| Case                                                                                          | Behaviour                                                                                                                                                                                                                                                                                                                                                         |
| --------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude Code, kept disabled in the file (by this computer or another)                          | Put back from its kept form to its recorded place: same event, same matcher group, same position, when that place is unambiguous by the rule of 27 §3.7. The file is byte-identical to the one before disabling when nothing else changed. If the place is ambiguous or gone, nothing is written and the user is shown why                                        |
| Claude Code, held in this computer's vault                                                    | Put back from its record by the rule of 27 §3.7: byte-identical to the file before disabling when the file has not changed since; otherwise only at the one unambiguous place, shown as a diff and said to be not identical; otherwise not at all. The record is released only after the entry is seen in the file, and kept a grace period beyond that (27 §8.3) |
| Claude Code, removed by an outside change, or held in another computer's vault                | Offered only if the user had kept a recovery copy of that entry on this computer: it is put back by position, shown as a diff, with the native dialog (27 §3.6; phase F2h). Otherwise not offered: "Removed outside this computer on <date>. This computer holds no copy." The view points to the other computer and to the provider's own backups                |
| The same entry is already present                                                             | Nothing is written                                                                                                                                                                                                                                                                                                                                                |
| A different entry occupies the same event and matcher with the same command but other options | `restore_conflict`: both shown; nothing written                                                                                                                                                                                                                                                                                                                   |
| The kept form was edited outside                                                              | The current content is shown and must be confirmed in the native dialog; it is what will run                                                                                                                                                                                                                                                                      |
| Codex, disabled natively                                                                      | Not offered before G-INF (10 §5): "Enabling a Codex hook would make managed Inference unavailable. It will be offered when managed Inference no longer depends on your hooks."                                                                                                                                                                                    |
| Codex, after G-INF                                                                            | Native switch; if the hook's content changed since Codex trusted it, the result is `trust_pending` and the Console says the review happens in Codex                                                                                                                                                                                                               |

Restoring a hook is `executes_code`: it puts executable content back. It always takes the
native dialog, even though the bytes are ones the user once had.

### 3.4 Create, edit, delete (F5)

Not offered before the contract of DN-12. The design:

| Action | Flow                                                                                                                                | Safeguards                                                                                                                                                                            |
| ------ | ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Create | The user fills a form (event, matcher, handler type, command, timeout). The command is typed in a **native** input, not in the page | The page never carries command text (C-01); the native dialog shows the exact file, scope and resulting entry                                                                         |
| Edit   | The same, starting from the current entry                                                                                           | For Codex the result is `trust_pending`. Undo of an edit would need the previous bytes: an `undo_entry` record in the vault is the proposed design (27 §3.1), adopted or not by DN-12 |
| Delete | Removes the entry                                                                                                                   | Said before confirming whether it can be undone. The same proposed record class, with a short retention stated in the dialog; without it, a delete is final                           |

Until F5 the Console offers "open the file in your editor" for anything it cannot do, and the
watcher (07 §7) shows the result.

### 3.5 Associate with profiles and projects (F3c)

A profile can name hooks that must be absent or present for sessions started with it. For
Claude Code this is done at start with a generated settings file and the exclusion of the user
source (09 §3), because hooks merge across levels and a profile cannot subtract one otherwise.
A hook is never removed from the global file to satisfy a profile.

### 3.6 Externally modified hooks

Every outside change produces a record: added, removed, changed (by identifier, never by
content). Shown in the Configuration view and in history. A removed hook is named by what the
journal may hold: its kind, its event and its position. Its command is not kept and so is not
shown once it is gone.

## 4. Rules and instructions

| Action    | Claude Code                                                                                                                                                                                               | Codex                                                          | Phase          |
| --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- | -------------- |
| Inventory | `CLAUDE.md` at each level, files in `rules/`, what each imports (`@path` lines), size, link or generated                                                                                                  | `AGENTS.md` at each level; generated or not                    | F1             |
| Disable   | **Not offered yet.** Moving a file of `rules/` aside needs a place the provider is known not to read (OQ-23) and a check that no other instruction file imports it. A section of `CLAUDE.md`: not offered | The global file on the owner's machine is generated: read only | After OQ-23    |
| Enable    | Move back, if nothing else took its name                                                                                                                                                                  | —                                                              | With the above |
| Edit      | In a later phase, as a whole-document edit with a diff                                                                                                                                                    | Same, unless generated                                         | F5             |
| Effective | `documented`: the provider loads them at session start `[N]` for reload                                                                                                                                   | `documented`                                                   | —              |

A rule is text for a model, not a control. The interface never presents an instruction as
something that prevents an action (11 §3, T-11).

Disabling a rule file would move the user's own document; nothing of its content would be
copied anywhere. It stays unavailable until, per provider: a destination inside the provider's
tree that the provider does not read is verified (it may read `rules/` recursively `[N]`); the
files that import the target are listed and the move is refused or warned about when one
exists; and the move has its own crash cases written in 14 §4.

## 5. Skills, plugins, MCP servers

| Action           | Claude Code                                                                                        | Codex                     | Phase           | Risk                                                        |
| ---------------- | -------------------------------------------------------------------------------------------------- | ------------------------- | --------------- | ----------------------------------------------------------- |
| Skill inventory  | Directories, names, descriptions from their headers; the override map `[O]`                        | `[[skills.config]]` `[O]` | F1              | —                                                           |
| Skill off / on   | The override map (semantics OQ-15)                                                                 | `enabled` on its entry    | F2a / F2b       | `normal`                                                    |
| Plugin off / on  | `enabledPlugins`                                                                                   | `[plugins]`               | F2a / F2b       | `normal`; on is `executes_code` if the plugin bundles hooks |
| MCP inventory    | Names, transport, non-secret fields; from a bounded projection of `~/.claude.json` and `.mcp.json` | `[mcp_servers]`           | F1              | —                                                           |
| MCP off / on     | No documented switch `[N]`                                                                         | `enabled`                 | F2b             | on is `executes_code` (a server is a program)               |
| MCP add / remove | Through `claude mcp`                                                                               | Through `codex mcp`       | F5; needs DN-13 | `executes_code`                                             |

A skill or MCP server that brings its own hooks (Claude Code plugins and skill frontmatter
`[D]`) is shown with them: switching the parent on switches them on.

## 6. Undo and recovery of prior versions

| Situation                                                   | Undo                                                                                                                        |
| ----------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| The file still has the hash recorded after the change       | Exact: the inverse operation; the result is byte-identical to the file before                                               |
| The file changed since, the entry's surroundings are intact | The inverse operation on the current content; shown as a diff to confirm                                                    |
| The surroundings changed                                    | A proposal the user confirms, or "cannot be undone automatically" with the kept entry shown where it is                     |
| The kept form was removed from the file outside             | Not undoable; history says so. The Console has no copy                                                                      |
| The entry is held in the vault                              | Restoring it is the undo (§3.3). If the record is corrupt or its key is lost, not undoable; the view says the entry is lost |
| The change was reverted from outside already                | Nothing to undo; history says so                                                                                            |

"Recover a prior version" of a whole file is not offered: the vault holds single entries, and a
whole file only when a write conflict would otherwise lose a version (27 §3.1). A provider that keeps its own backups (Claude Code keeps copies of
`~/.claude.json` under `backups/` `[D]`) is pointed to.

## 7. Edge cases the owner named

| Case                                 | Behaviour                                                                                                                                                                                  |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Existing links                       | Followed to a regular file of the user; shown; never replaced (07 §4.3)                                                                                                                    |
| Generated Markdown                   | Read only; origin shown when known                                                                                                                                                         |
| A file rewritten from outside        | Outside change; plans on it are void                                                                                                                                                       |
| A file updated by a replication tool | The same; a conflict copy blocks mutation                                                                                                                                                  |
| Unknown fields                       | Preserved untouched and counted                                                                                                                                                            |
| Comments and ordering                | Preserved, or the file is not edited (E4)                                                                                                                                                  |
| Concurrent editors                   | The protocol of 07 §5.4. Every ordering it covers ends in a named outcome; a writer that does not cooperate cannot be stopped, only seen                                                   |
| A link swapped during an operation   | The file is opened once, relative to a descriptor of its validated directory, and judged by that descriptor; a source reached through a link is read only unless 07 §4.3 allows writing it |
| Corrupt file                         | `malformed`: listed, not edited, not repaired                                                                                                                                              |
| Permission problems                  | `unreadable` for reads; `write_failed` for writes; nothing is chmod-ed                                                                                                                     |
| Partly successful operation          | Not possible for one file: its replacement is atomic (07 §5.4). For a change spanning two files (none in F2a), each is its own plan and the second is not started if the first fails       |

## 8. What the Console never does

It never writes a provider's trust record, never marks a hook as trusted, never edits a system
policy, never changes a file's mode or owner, never repairs a file it cannot parse, never runs
a hook to test it, and never disables or enables anything without a confirmed plan.
