# 08 — Rules, hooks, skills and MCP

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

| State         | Meaning                                                                                 |
| ------------- | --------------------------------------------------------------------------------------- |
| enabled       | Present in the provider's file and not switched off                                     |
| disabled      | Present and switched off by a state the provider itself has                             |
| parked        | Taken out of the provider's file by the Console, and kept so that it can be put back    |
| absent_known  | Gone from the file by an outside change; this computer last saw it and could restore it |
| absent        | Not in the file; nothing is known that could restore it                                 |
| trust_pending | Present, but the provider will skip it until the user reviews it there (Codex)          |
| blocked       | The operation that would change it is not offered, with the reason                      |
| unreadable    | Its source cannot be read or parsed                                                     |

| From         | To            | Initiator      | Guard                                                      | Side effect                             | On failure                     |
| ------------ | ------------- | -------------- | ---------------------------------------------------------- | --------------------------------------- | ------------------------------ |
| enabled      | disabled      | User           | Provider has a native switch; version verified             | Switch written                          | File untouched                 |
| enabled      | parked        | User           | No native switch; storage policy allows keeping it (DN-01) | Restoration record; entry removed       | File untouched; record removed |
| disabled     | enabled       | User           | Native switch; not blocked by G-INF                        | Switch written                          | File untouched                 |
| parked       | enabled       | User           | Record valid; no conflict with current content             | Entry put back; record marked used      | File untouched; record kept    |
| enabled      | absent_known  | Outside change | The engine had seen it                                     | Last-seen entry kept (subject to DN-01) | —                              |
| absent_known | enabled       | User           | As parked → enabled                                        | Entry put back                          | As above                       |
| parked       | enabled       | Outside change | The same entry reappeared in the file                      | Record marked "restored elsewhere"      | —                              |
| any          | trust_pending | Provider       | The entry's content differs from what the provider trusted | None by the Console                     | —                              |
| any          | absent        | User (delete)  | F5; confirmed                                              | Entry removed; a record kept for undo   | File untouched                 |

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

| Provider               | Mechanism                                                                                                     | Phase | Effect reported                                                                                                                                                   |
| ---------------------- | ------------------------------------------------------------------------------------------------------------- | ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude Code            | **Park**: remove the handler from its group (and the group or event if left empty), keep a restoration record | F2a   | "Removed from settings.json and kept by Wyss Console on this computer. Claude Code documents that running sessions pick this up; not verified for your sessions." |
| Claude Code, all hooks | Native: `disableAllHooks: true`                                                                               | F2a   | "All hooks off. Managed hooks, if any, stay on."                                                                                                                  |
| Codex                  | Native switch, once its storage is known (OQ-01)                                                              | F2b   | "Switched off in Codex's configuration. Effect on running sessions not confirmed."                                                                                |
| OpenCode               | A plugin has no switch `[D]`: park by removing it from the `plugin` list or moving the file                   | F4a   | —                                                                                                                                                                 |

Risk: disabling a hook is `weakens_security` when the hook is of an event that can block an
action (`PreToolUse`, `PermissionRequest`, `UserPromptSubmit` and their equivalents) or when the
user has marked it as a security control; otherwise `normal`. The classification is the
backend's (11 §4).

| Failure                                                                 | Result                                                                                                                                                                              |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The file changed since the plan                                         | Nothing written; new plan offered                                                                                                                                                   |
| The entry is no longer there                                            | `entry_gone`; the inventory refreshes                                                                                                                                               |
| The entry is there but differs                                          | `entry_changed`; the user is shown the current one and must plan again                                                                                                              |
| The key store is unavailable and the entry may hold a secret (ADR 0029) | Not offered; "this hook holds what looks like a credential; disabling it individually is not available. You can switch all hooks off, or edit the file yourself" (subject to DN-01) |
| Native dialog refused                                                   | Nothing written to the file; the journal records the refusal (item, operation, risk class; no content)                                                                              |
| Write failed                                                            | Old file intact; record removed; error shown                                                                                                                                        |
| Read-back differs                                                       | Journal `unverified`; the user is shown what the file holds now                                                                                                                     |

### 3.3 Enable and restore

| Case                                                                                          | Behaviour                                                                                                                                                                    |
| --------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude Code, parked on this computer                                                          | The entry is inserted at its recorded place: same event, same matcher group, same position if its neighbours are still there, otherwise at the end of the group. Exact bytes |
| Claude Code, gone by an outside change, last seen here                                        | Same, from the last-seen entry                                                                                                                                               |
| Claude Code, gone, never seen here                                                            | Not offered. "Disabled on another computer; no restoration data here." Import is offered                                                                                     |
| The same entry is already present                                                             | Nothing is written; the record is marked "restored elsewhere"                                                                                                                |
| A different entry occupies the same event and matcher with the same command but other options | `restore_conflict`: both shown; nothing written                                                                                                                              |
| Codex, disabled natively                                                                      | **Blocked by G-INF** (10 §5): "Enabling a Codex hook would make managed Inference unavailable. It will be offered when managed Inference no longer depends on your hooks."   |
| Codex, after G-INF                                                                            | Native switch; if the hook's content changed since Codex trusted it, the result is `trust_pending` and the Console says the review happens in Codex                          |

Restoring a hook is `executes_code`: it puts executable content back. It always takes the
native dialog, even though the bytes are ones the user once had.

### 3.4 Create, edit, delete (F5)

Not offered before the contract of DN-12. The design:

| Action | Flow                                                                                                                                | Safeguards                                                                                                    |
| ------ | ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Create | The user fills a form (event, matcher, handler type, command, timeout). The command is typed in a **native** input, not in the page | The page never carries command text (C-01); the native dialog shows the exact file, scope and resulting entry |
| Edit   | The same, starting from the current entry                                                                                           | A restoration record of the previous entry; for Codex the result is `trust_pending`                           |
| Delete | Removes the entry                                                                                                                   | A restoration record, kept for undo within the retention bound                                                |

Until F5 the Console offers "open the file in your editor" for anything it cannot do, and the
watcher (07 §7) shows the result.

### 3.5 Associate with profiles and projects (F3c)

A profile can name hooks that must be absent or present for sessions started with it. For
Claude Code this is done at start with a generated settings file and the exclusion of the user
source (09 §3), because hooks merge across levels and a profile cannot subtract one otherwise.
A hook is never removed from the global file to satisfy a profile.

### 3.6 Externally modified hooks

Every outside change produces a record: added, removed, changed (by identifier). Shown in the
Configuration view and in history. A changed hook that the Console had parked or last seen is
flagged: the kept copy is now older than the file's history and restoring it would bring back
the old content; the restore dialog says so.

## 4. Rules and instructions

| Action    | Claude Code                                                                                              | Codex                                                          | Phase            |
| --------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- | ---------------- |
| Inventory | `CLAUDE.md` at each level, files in `rules/`, what each imports (`@path` lines), size, link or generated | `AGENTS.md` at each level; generated or not                    | F1               |
| Disable   | A file in `rules/`: park by moving it out. A section of `CLAUDE.md`: not offered                         | The global file on the owner's machine is generated: read only | F2a (files only) |
| Enable    | Move back, if nothing else took its name                                                                 | —                                                              | F2a              |
| Edit      | In a later phase, as a whole-document edit with a diff                                                   | Same, unless generated                                         | F5               |
| Effective | `documented`: the provider loads them at session start `[N]` for reload                                  | `documented`                                                   | —                |

A rule is text for a model, not a control. The interface never presents an instruction as
something that prevents an action (11 §3, T-11).

Parking a rule file moves the user's own document. The record keeps its path and hash; the
file itself is moved inside the provider's directory tree to a sibling the provider does not
read `[N]` (which sibling is safe per provider is OQ-23), so that nothing of its content is
copied into the Console's state.

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

| Situation                                                   | Undo                                                                                              |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| The file still has the hash recorded after the change       | Exact: the inverse operation; the result is byte-identical to the file before                     |
| The file changed since, the entry's surroundings are intact | The inverse operation on the current content; shown as a diff to confirm                          |
| The surroundings changed                                    | A merge proposal the user confirms, or "cannot be undone automatically" with the kept entry shown |
| The restoration record expired or was deleted               | Not undoable; history says so                                                                     |

"Recover a prior version" of a whole file is not offered: the Console keeps entries, not files
(ADR 0028 §7 `[R]`). A provider that keeps its own backups (Claude Code keeps copies of
`~/.claude.json` under `backups/` `[D]`) is pointed to.

## 7. Edge cases the owner named

| Case                                 | Behaviour                                                                                                                                                |
| ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Existing links                       | Followed to a regular file of the user; shown; never replaced (07 §4.3)                                                                                  |
| Generated Markdown                   | Read only; origin shown when known                                                                                                                       |
| A file rewritten from outside        | Outside change; plans on it are void                                                                                                                     |
| A file updated by a replication tool | The same; a conflict copy blocks mutation                                                                                                                |
| Unknown fields                       | Preserved untouched and counted                                                                                                                          |
| Comments and ordering                | Preserved, or the file is not edited (E4)                                                                                                                |
| Concurrent editors                   | Hash check before the rename; read-back after                                                                                                            |
| A link swapped during an operation   | The file is opened once and judged by that descriptor; the rename targets the resolved path, whose ancestors were checked                                |
| Corrupt file                         | `malformed`: listed, not edited, not repaired                                                                                                            |
| Permission problems                  | `unreadable` for reads; `write_failed` for writes; nothing is chmod-ed                                                                                   |
| Partly successful operation          | Not possible for one file (rename). For a change spanning two files (none in F2a), each is its own plan and the second is not started if the first fails |

## 8. What the Console never does

It never writes a provider's trust record, never marks a hook as trusted, never edits a system
policy, never changes a file's mode or owner, never repairs a file it cannot parse, never runs
a hook to test it, and never disables or enables anything without a confirmed plan.
