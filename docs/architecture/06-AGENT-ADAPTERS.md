# 06 — Agent adapters

Domain F of [04](04-TARGET-ARCHITECTURE.md). A capability specification per provider, with the
evidence behind every claim, and the contract an adapter must implement. No adapter exists for
configuration yet; one execution adapter exists, for managed Codex `[R]`.

Evidence marks: `[R]` repository, `[D]` official documentation read on 2026-10-12, `[O]`
observed on the owner's machine on 2026-10-12, `[P]` proposed, `[N]` not verified.

## 1. Two families

| Family                    | Lives in                                       | Does                                                     | Contract                      | Exists      |
| ------------------------- | ---------------------------------------------- | -------------------------------------------------------- | ----------------------------- | ----------- |
| **Execution adapter**     | The authority (`src-tauri/src/adapters/`)      | Speaks a runtime's protocol for managed work             | ADR 0008 `[R]`                | Codex `[R]` |
| **Configuration adapter** | The desktop backend (`agentconfig/providers/`) | Discovers, reads and plans changes to a provider's files | `IF-CONFIG-ADAPTER`, §4 `[P]` | No          |

They share words (ADR 0008's capability identifiers, e.g. `hooks`, `mcp_servers`, `skills`,
`model_list`) and nothing else. ADR 0008 allows only `observe` on `hooks` and `mcp_servers`
(`0008:91`) `[R]`; a configuration adapter's write capabilities are a separate declaration and
add no `request` to ADR 0008.

## 2. How a capability is declared

Every capability of a configuration adapter is a record:

| Field       | Values                                                                                                                                                                                           |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `kind`      | `rule`, `hook`, `skill`, `mcp_server`, `plugin`, `setting`, `model_preference`                                                                                                                   |
| `operation` | `inventory`, `enable`, `disable`, `restore`, `create`, `edit`, `delete`, `effective_read`                                                                                                        |
| `support`   | `native` (the provider has this state), `by_removal` (the entry is taken out and kept), `by_provider_writer` (done through the provider's own command), `read_only`, `unsupported`, `unverified` |
| `effect`    | `new_sessions`, `running_sessions` (provider reloads), `needs_restart`, `needs_provider_review`, `unknown`                                                                                       |
| `evidence`  | One of the five marks, with its source                                                                                                                                                           |
| `versions`  | The provider versions the evidence was gathered on                                                                                                                                               |

Rules: a missing record means unknown, and unknown is never offered (ADR 0008 `:13-15` `[R]`).
A capability whose evidence is `[N]` may be shown as an inventory fact and never as an action.

## 3. Provider specifications

### 3.1 Claude Code

Installed version observed: 2.1.296 `[O]`.

| Topic                  | Finding                                                                                                                                                                                                                                                                                       | Evidence                                                    |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| Native CLI             | `claude`; flags `--settings`, `--setting-sources`, `--mcp-config`, `--strict-mcp-config`, `--append-system-prompt`, `--system-prompt`, `--agents`, `--plugin-dir`, `--model`, `--permission-mode`, `--allowed-tools`, `--disallowed-tools`, `--add-dir`, `--disable-slash-commands`, `--bare` | `[O]` (`--help`)                                            |
| SDK or protocol        | An agent SDK and a headless mode exist; which surface suits supervision                                                                                                                                                                                                                       | `[N]` (OQ-05)                                               |
| Global configuration   | `~/.claude/settings.json`; relocatable with `CLAUDE_CONFIG_DIR`                                                                                                                                                                                                                               | `[D]`                                                       |
| Project configuration  | `.claude/settings.json` (shared), `.claude/settings.local.json` (personal)                                                                                                                                                                                                                    | `[D]`                                                       |
| System policy          | `managed-settings.json` in a system directory, MDM, or server-managed                                                                                                                                                                                                                         | `[D]`; none on the owner's machine `[O]`                    |
| Precedence             | managed > `--settings` > project local > shared project > user                                                                                                                                                                                                                                | `[D]`                                                       |
| Format                 | JSON                                                                                                                                                                                                                                                                                          | `[D]`                                                       |
| Provider-owned file    | `~/.claude.json`: sign-in, MCP servers, per-project trust; "you don't need to edit it"                                                                                                                                                                                                        | `[D]`; 112 KB on the owner's machine `[O]`                  |
| Hooks: where           | `hooks` in any settings file; plugin `hooks/hooks.json`; skill and subagent frontmatter                                                                                                                                                                                                       | `[D]`                                                       |
| Hooks: structure       | event → matcher group → handlers; handler `type` is `command`, `http`, `mcp_tool`, `prompt` or `agent`; fields `if`, `timeout`, `statusMessage`, `once`                                                                                                                                       | `[D]`                                                       |
| Hooks: events          | 33 event names, among them `SessionStart`, `PreToolUse`, `PostToolUse`, `ConfigChange`, `PreModelSwitch`, `PostModelSwitch`, `SessionEnd`                                                                                                                                                     | `[D]`                                                       |
| Hooks: merging         | "Hook entries merge across levels rather than replacing each other"                                                                                                                                                                                                                           | `[D]`                                                       |
| Hooks: per-hook switch | **None.** "There is no way to disable an individual hook while keeping it in the configuration"                                                                                                                                                                                               | `[D]`                                                       |
| Hooks: all off         | `disableAllHooks: true`; cannot disable managed hooks                                                                                                                                                                                                                                         | `[D]`                                                       |
| Rules and instructions | `CLAUDE.md` at user and project level; `~/.claude/rules/`                                                                                                                                                                                                                                     | `[D]` for `CLAUDE.md`; the directory `[O]`                  |
| Skills                 | `~/.claude/skills/<name>/`; a `skillOverrides` map with value `off` switches one off                                                                                                                                                                                                          | Directory and key `[O]`; semantics of the key `[N]` (OQ-15) |
| Plugins                | `enabledPlugins: { id: true or false }`                                                                                                                                                                                                                                                       | `[O]`; `[N]` in the page read                               |
| MCP                    | User and local scope in `~/.claude.json`, project scope in `.mcp.json`; managed with `claude mcp`                                                                                                                                                                                             | `[D]` for location; the command `[O]`                       |
| Model                  | `model`, `effortLevel`, `modelSettings` in settings; `/model` and `/effort` in a session                                                                                                                                                                                                      | `[D]`                                                       |
| Session persistence    | Session history under the configuration directory                                                                                                                                                                                                                                             | `[D]` (mentioned); format `[N]`                             |
| Resume                 | Provider feature; not specified here                                                                                                                                                                                                                                                          | `[N]`                                                       |
| Runtime observability  | `/status` shows loaded setting sources, interactively                                                                                                                                                                                                                                         | `[D]`; programmatic equivalent `[N]` (OQ-05)                |
| Environment isolation  | `CLAUDE_CONFIG_DIR` relocates settings, history and plugins; also the sign-in                                                                                                                                                                                                                 | `[D]`; effect on credentials `[N]`                          |
| Live reload            | "Claude Code watches your settings files and reloads them when they change", including `permissions` and `hooks`; `model`, `effortLevel` and `modelSettings` are read once at start                                                                                                           | `[D]`                                                       |
| Security and trust     | Per-project trust in `~/.claude.json`; some security keys keep the stricter value from any level                                                                                                                                                                                              | `[D]`                                                       |
| Version compatibility  | Evidence is for the documentation as of 2026-10-12 and the binary 2.1.296                                                                                                                                                                                                                     | —                                                           |

Capability table for the configuration adapter:

| Kind · operation            | Support                                                                    | Effect             | Evidence                  |
| --------------------------- | -------------------------------------------------------------------------- | ------------------ | ------------------------- |
| hook · inventory            | native (read)                                                              | —                  | `[D]`                     |
| hook · disable              | `by_removal`                                                               | `running_sessions` | `[D]` (no switch; reload) |
| hook · restore              | `by_removal`                                                               | `running_sessions` | `[D]`                     |
| hook · disable all          | native                                                                     | `running_sessions` | `[D]`                     |
| hook · create, edit, delete | native (file edit)                                                         | `running_sessions` | `[D]`; gated to F5        |
| rule · inventory            | native (read)                                                              | —                  | `[D]`, `[O]`              |
| rule · disable, enable      | `by_removal` (move the file) or `unsupported` for a section of `CLAUDE.md` | `unknown`          | `[N]`                     |
| skill · disable, enable     | native (`skillOverrides`)                                                  | `unknown`          | `[O]`, `[N]` (OQ-15)      |
| plugin · disable, enable    | native (`enabledPlugins`)                                                  | `unknown`          | `[O]`, `[N]`              |
| mcp_server · inventory      | native (read of `~/.claude.json`, bounded projection)                      | —                  | `[D]`                     |
| mcp_server · add, remove    | `by_provider_writer` (`claude mcp`)                                        | `unknown`          | `[O]`; semantics `[N]`    |
| setting · model_preference  | native                                                                     | `new_sessions`     | `[D]`                     |
| any · effective_read        | `unverified`                                                               | —                  | `[N]` (OQ-05)             |

### 3.2 Codex

Installed version observed: 0.159.0 `[O]`; it is also the only version the managed plane
accepts (ADR 0015 `:40`) `[R]`.

| Topic                             | Finding                                                                                                                                                                                                                          | Evidence                                                                                       |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Native CLI                        | `codex`; `-c key=value`, `-p/--profile`, `-m/--model`, `-s/--sandbox`, `-a/--ask-for-approval`, `-C/--cd`, `--add-dir`, `--enable`, `--disable`; subcommands `mcp`, `app-server`, `agents`, `remote-control`                     | `[O]` (`--help`)                                                                               |
| Protocol                          | App Server: JSON-RPC over stdio; methods the repository already uses include `initialize`, `model/list`, `config/read`, `configRequirements/read`, `hooks/list`, `thread/start`, `thread/resume`, `turn/start`, `turn/interrupt` | `[R]` (`adapters/codex/safety.rs:38-57`; `docs/research/agent-runtime-surfaces-2026-10-06.md`) |
| Global configuration              | `~/.codex/config.toml`; state under `CODEX_HOME` (default `~/.codex`)                                                                                                                                                            | `[D]`                                                                                          |
| Profiles                          | `--profile name` overlays `~/.codex/name.config.toml`; the `[profiles.name]` table is gone since 0.134.0                                                                                                                         | `[D]`                                                                                          |
| Project configuration             | `.codex/config.toml` from the project root down; loaded only when the project is trusted                                                                                                                                         | `[D]`                                                                                          |
| System policy                     | `/etc/codex/requirements.toml`; can pin features and define managed hooks                                                                                                                                                        | `[D]`; present on the owner's machine with `[features] hooks = true` `[O]`                     |
| Precedence                        | user < profile < project < CLI                                                                                                                                                                                                   | `[D]`                                                                                          |
| Format                            | TOML with comments                                                                                                                                                                                                               | `[D]`; 631 lines, 34 comment lines `[O]`                                                       |
| Hooks: where                      | `hooks.json` or inline `[hooks]` in `config.toml`, at user and project level; plugins may bundle them                                                                                                                            | `[D]`                                                                                          |
| Hooks: events                     | `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `UserPromptSubmit`, `SubagentStop`, `Stop`, `Interrupt`, `SessionStart`, `SubagentStart`, `SessionEnd`                                            | `[D]`                                                                                          |
| Hooks: handler types              | `command`, `mcp_tool`; `prompt` and `agent` are parsed but skipped                                                                                                                                                               | `[D]`                                                                                          |
| Hooks: per-hook switch            | Yes: `hooks/list` reports `enabled` per hook; the `/hooks` browser disables individual non-managed hooks                                                                                                                         | `[R]` (`safety.rs:344`), `[D]`                                                                 |
| Hooks: where the switch is stored | Not documented in what was read                                                                                                                                                                                                  | `[N]` (OQ-01); `[hooks.state]` with `trusted_hash` exists `[O]`                                |
| Hooks: trust                      | "Codex records trust against the hook's current hash, so new or changed hooks are marked for review and skipped until trusted"                                                                                                   | `[D]`                                                                                          |
| Hooks: engine switch              | `[features] hooks = false`; an administrator can pin it either way in `requirements.toml`; `allow_managed_hooks_only`                                                                                                            | `[D]`                                                                                          |
| Rules and instructions            | `AGENTS.md`, global and per project                                                                                                                                                                                              | `[O]`; on the owner's machine the global one is generated                                      |
| Skills                            | `[[skills.config]]` entries with `path` and `enabled`                                                                                                                                                                            | `[O]` (93 entries); documentation `[N]`                                                        |
| Plugins                           | `[plugins]` table                                                                                                                                                                                                                | `[O]`                                                                                          |
| MCP                               | `[mcp_servers.<name>]` with an `enabled` key; `codex mcp`                                                                                                                                                                        | `[D]` (override example), `[O]`                                                                |
| Model                             | `model`, `model_reasoning_effort`; catalogue through `model/list` with `isDefault` and reasoning metadata                                                                                                                        | `[D]`, `[R]`                                                                                   |
| Session persistence               | Threads kept by Codex; the managed plane stores only a thread identifier                                                                                                                                                         | `[R]` (ADR 0014)                                                                               |
| Resume                            | `thread/resume`                                                                                                                                                                                                                  | `[R]`                                                                                          |
| Remote                            | `remote-control`, experimental                                                                                                                                                                                                   | `[O]` (listed); behaviour `[N]`                                                                |
| Environment isolation             | `CODEX_HOME`; the managed Local Tools profile uses a private one inside a namespace                                                                                                                                              | `[R]` (`probe/isolation.rs:505`)                                                               |
| Live reload                       | Not documented in what was read                                                                                                                                                                                                  | `[N]` (OQ-03)                                                                                  |
| Version compatibility             | Managed: 0.159.0 only `[R]`. Configuration adapter: evidence for 0.159.0 `[O]`                                                                                                                                                   | —                                                                                              |

Capability table for the configuration adapter:

| Kind · operation             | Support                                                                                                                              | Effect                  | Evidence                     |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | ----------------------- | ---------------------------- |
| hook · inventory             | native (read of files)                                                                                                               | —                       | `[D]`                        |
| hook · disable               | native, storage unknown                                                                                                              | `unknown`               | `[N]` (OQ-01)                |
| hook · enable                | native, **blocked** by G-INF (10 §5)                                                                                                 | `unknown`               | `[R]` for the block          |
| hook · create, edit          | native (file edit)                                                                                                                   | `needs_provider_review` | `[D]`; gated to F5           |
| hook trust                   | `read_only`, forever                                                                                                                 | —                       | C-08                         |
| rule · inventory             | native (read)                                                                                                                        | —                       | `[O]`                        |
| skill · disable, enable      | native (`enabled`)                                                                                                                   | `unknown`               | `[O]`, `[N]`                 |
| mcp_server · disable, enable | native (`enabled`)                                                                                                                   | `unknown`               | `[D]`, `[O]`                 |
| mcp_server · add, remove     | `by_provider_writer` (`codex mcp`)                                                                                                   | `unknown`               | `[O]`; `[N]`                 |
| setting · model_preference   | native                                                                                                                               | `new_sessions`          | `[D]`                        |
| any · effective_read         | Possible in principle through `config/read` and `hooks/list`; starting a Codex process from the desktop for it is **not** part of F1 | —                       | `[R]` for the methods; DN-10 |
| system policy                | `read_only`                                                                                                                          | —                       | C-09                         |

### 3.3 OpenCode

Installed version observed: 1.19.0 `[O]`.

| Topic                 | Finding                                                                                                                                                                                                                      | Evidence                                             |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| Configuration sources | Merged in order: remote `.well-known/opencode`, global `~/.config/opencode/opencode.json`, `OPENCODE_CONFIG`, project `opencode.json`, `.opencode` directories, `OPENCODE_CONFIG_CONTENT`, managed files in `/etc/opencode/` | `[D]`                                                |
| Format                | JSON or JSONC                                                                                                                                                                                                                | `[D]`; `opencode.jsonc` on the owner's machine `[O]` |
| Rules                 | `instructions`: paths and globs                                                                                                                                                                                              | `[D]`                                                |
| Hooks                 | Plugins, in `plugins/` directories or by the `plugin` option; no per-plugin switch documented                                                                                                                                | `[D]`; two plugins `[O]`                             |
| Agents, commands      | The `agent` and `command` options, or markdown files                                                                                                                                                                         | `[D]`                                                |
| Skills                | A `skills/` directory is named; nothing more                                                                                                                                                                                 | `[N]` (OQ-12)                                        |
| MCP                   | `mcp.<name>` with `enabled`                                                                                                                                                                                                  | `[D]`                                                |
| Tools and permissions | `tools.<name>: false`; `permission` per tool                                                                                                                                                                                 | `[D]`                                                |
| Model                 | `model`; providers under `provider`                                                                                                                                                                                          | `[D]`, `[O]`                                         |
| Environment isolation | `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, `OPENCODE_CONFIG_CONTENT`                                                                                                                                                          | `[D]`                                                |
| Protocol              | A local server (`opencode serve`) and ACP are reported by earlier research                                                                                                                                                   | `[N]` here (OQ-12)                                   |
| Live reload           | Not stated                                                                                                                                                                                                                   | `[N]` (OQ-04)                                        |

Everything beyond inventory is `unverified` until F4a.

### 3.4 Hermes Agent

Not installed `[O]`. Everything is from one documentation page and is to be re-verified when
it is installed, with the owner's consent (F4b).

| Topic         | Finding                                                                                                                                                    | Evidence                                      |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| Configuration | `~/.hermes/`: `config.yaml` (settings), `.env` (keys and secrets), `auth.json` (OAuth), `SOUL.md` (identity), `memories/`, `skills/`, `cron/`, `sessions/` | `[D]`                                         |
| Format        | YAML; written with `hermes config set`                                                                                                                     | `[D]`                                         |
| Profiles      | State is profile-scoped through `HERMES_HOME`                                                                                                              | `[D]`; relocation of the base directory `[N]` |
| Skills        | `skills.config`, `skills.auto_load`; write approval optional                                                                                               | `[D]`                                         |
| Tools         | `agent.disabled_toolsets`                                                                                                                                  | `[D]`                                         |
| MCP           | `mcp_servers`; switch not stated                                                                                                                           | `[N]`                                         |
| Hooks         | Context-engine plugins; registration not stated                                                                                                            | `[N]`                                         |
| Memory        | `memory.memory_enabled`, limits, provider plugins                                                                                                          | `[D]`                                         |
| Approval      | Dangerous-command approval exists; keys not stated                                                                                                         | `[N]`                                         |
| Interfaces    | ACP helper processes, a gateway for messaging platforms, `hermes serve`                                                                                    | `[D]` (mentioned); `[N]` in detail            |
| Licence       | Reported as MIT by the vision report                                                                                                                       | `[N]` (OQ-11): to be read in the repository   |

Because its file is YAML and the product has a writer of its own, the adapter writes through
`hermes config set` (`by_provider_writer`) and never edits the YAML directly `[P]`.

## 4. The configuration adapter contract (IF-CONFIG-ADAPTER)

An adapter is a set of pure and bounded functions. It never runs a provider and never receives
a path from its caller. `[P]`

| Function                         | Input                              | Output                                                                                                                  | Bounds and rules                                                                    |
| -------------------------------- | ---------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `identity()`                     | —                                  | Provider id, display name, executable name                                                                              | Compiled in                                                                         |
| `detect()`                       | The environment                    | Installed or not, version text (bounded, opaque), evidence state                                                        | Reuses the discovery rule of ADR 0010 `[R]`                                         |
| `sources(project?)`              | A project the user chose, or none  | The closed list of files and directories this provider reads, each with scope, format, owner class and write capability | Derived from the home directory, documented variables and the project; nothing else |
| `read(source)`                   | One of its own sources             | Parsed items, or a typed failure                                                                                        | Size, count, depth and link bounds (C-11); the path rules of `control/paths.rs`     |
| `capabilities(version)`          | A version                          | The records of §2                                                                                                       | Unknown version → every mutation `unverified`                                       |
| `redact(item)`                   | A parsed item                      | Its projection for the page                                                                                             | Secret classes removed before anything is serialised (C-07)                         |
| `plan(operation, item, content)` | An operation and the current bytes | New bytes, a diff, a risk class, the restoration record it needs                                                        | Pure; no I/O                                                                        |
| `verify(operation, content)`     | The bytes read back                | Whether the item is in the intended state                                                                               | Pure                                                                                |
| `managed_effect(plan)`           | A plan                             | Whether the result is compatible with managed execution, and why                                                        | Only Codex has one today (10 §5)                                                    |
| `reload_rule(kind)`              | A kind                             | The `effect` of §2                                                                                                      | From documentation, never guessed                                                   |

### 4.1 Source classes

| Class            | Meaning                                             | Written by the Console                                         | Example `[O]`                  |
| ---------------- | --------------------------------------------------- | -------------------------------------------------------------- | ------------------------------ |
| `user`           | A file the provider documents as the user's to edit | Yes, by a plan                                                 | `~/.claude/settings.json`      |
| `project`        | The same, inside a project                          | Yes, by a plan                                                 | `.claude/settings.json`        |
| `system`         | Root-owned or organisation policy                   | Never (C-09)                                                   | `/etc/codex/requirements.toml` |
| `provider_owned` | A file the provider rewrites for itself             | Never directly; through the provider's writer where one exists | `~/.claude.json`               |
| `generated`      | Produced by another tool from another source        | Never; shown with what is known of its origin                  | `~/.codex/AGENTS.md`           |
| `link`           | A symbolic link                                     | The link never; its target according to the target's own class | `~/.claude/CLAUDE.md`          |

How a file is known to be generated cannot be discovered in general. The adapter recognises a
generator's marker when the file carries one, and the user can mark a file as protected; a
protected file is read only in the Console `[P]` (OQ-21).

### 4.2 Secret classes

What an adapter must never let through to the page or to any record:

| Class                                                             | Examples                                                             |
| ----------------------------------------------------------------- | -------------------------------------------------------------------- |
| Credential files                                                  | `auth.json`, `.credentials.json`, `.env`: never opened at all        |
| Values of environment maps                                        | `env` in settings, `env` of an MCP server: names shown, values never |
| Headers and tokens of HTTP handlers                               | A hook of type `http`, an MCP server reached by URL with credentials |
| Anything in a provider-owned state file that is not configuration | Sign-in and session state in `~/.claude.json`                        |

A hook's command line is configuration and is shown; it can still contain a secret the user
wrote into it. That case is the subject of [11 §5](11-SECURITY-THREAT-MODEL.md).

## 5. Execution adapters

Unchanged by this set. The managed Codex adapter is described in
[10](10-MANAGED-RUNTIME-AND-SECURITY.md). A second execution adapter (F11b) must bring, before
any code: the provider's supported protocol with evidence, a safety contract equivalent to ADR
0015 and 0016, an isolation attestation, and an ADR. Nothing in the native plane is a step
towards it: starting `claude` in a terminal is not managing it.

## 6. Version policy

| Situation                                               | Inventory                                                          | Mutation                                               | Native start        |
| ------------------------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------ | ------------------- |
| Version with recorded evidence                          | Yes                                                                | Per capability                                         | Yes                 |
| Newer or older version, same major configuration format | Yes, marked unverified                                             | Refused (`unverified_version`) until evidence is added | Yes, with a warning |
| Provider not installed                                  | Files are still listed if present, marked "provider not installed" | Refused                                                | No                  |

Evidence is added by a fixture test that reproduces the provider's documented behaviour for
that version, and a note in the adapter's version table (NFR-MNT-01).
