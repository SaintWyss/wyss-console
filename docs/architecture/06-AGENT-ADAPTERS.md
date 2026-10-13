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

| Field       | Values                                                                                                                                                                                                                                                                                                                                                                   |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `kind`      | `rule`, `hook`, `skill`, `mcp_server`, `plugin`, `setting`, `model_preference`                                                                                                                                                                                                                                                                                           |
| `operation` | `inventory`, `enable`, `disable`, `restore`, `create`, `edit`, `delete`, `effective_read`                                                                                                                                                                                                                                                                                |
| `support`   | `native` (the provider has this state), `by_removal` (ADR 0028's word: the entry is disabled without a native switch; under proposed ADR 0029 it is kept inside the same file where that is verified, and otherwise removed and sealed in the protected vault), `by_provider_writer` (done through the provider's own command), `read_only`, `unsupported`, `unverified` |
| `effect`    | `new_sessions`, `running_sessions` (provider reloads), `needs_restart`, `needs_provider_review`, `unknown`                                                                                                                                                                                                                                                               |
| `evidence`  | One of the five marks, with its source                                                                                                                                                                                                                                                                                                                                   |
| `versions`  | The provider versions the evidence was gathered on                                                                                                                                                                                                                                                                                                                       |

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

| Kind · operation            | Support                                                                                                                                                            | Effect                              | Evidence                                                          |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------- | ----------------------------------------------------------------- |
| hook · inventory            | native (read)                                                                                                                                                      | —                                   | `[D]`                                                             |
| hook · disable              | `by_removal`: in place where the compatibility test of ADR 0029 §5 passed; otherwise to the protected vault (27). **Unavailable** until one of the two can be used | `running_sessions`, documented only | `[D]` for "no switch" and for the reload; `[N]` for the kept form |
| hook · restore              | The same                                                                                                                                                           | `running_sessions`, documented only | `[N]`                                                             |
| hook · disable all          | native                                                                                                                                                             | `running_sessions`                  | `[D]`                                                             |
| hook · create, edit, delete | native (file edit)                                                                                                                                                 | `running_sessions`                  | `[D]`; gated to F5                                                |
| rule · inventory            | native (read)                                                                                                                                                      | —                                   | `[D]`, `[O]`                                                      |
| rule · disable, enable      | `unsupported` until a destination the provider does not read is verified (OQ-23); never for a section of `CLAUDE.md`                                               | `unknown`                           | `[N]`                                                             |
| skill · disable, enable     | native (`skillOverrides`)                                                                                                                                          | `unknown`                           | `[O]`, `[N]` (OQ-15)                                              |
| plugin · disable, enable    | native (`enabledPlugins`)                                                                                                                                          | `unknown`                           | `[O]`, `[N]`                                                      |
| mcp_server · inventory      | native (read of `~/.claude.json`, bounded projection)                                                                                                              | —                                   | `[D]`                                                             |
| mcp_server · add, remove    | `by_provider_writer` (`claude mcp`)                                                                                                                                | `unknown`                           | `[O]`; semantics `[N]`                                            |
| setting · model_preference  | native                                                                                                                                                             | `new_sessions`                      | `[D]`                                                             |
| any · effective_read        | `unverified`                                                                                                                                                       | —                                   | `[N]` (OQ-05)                                                     |

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
| `plan(operation, item, content)` | An operation and the current bytes | New bytes, a diff, a risk class, how the entry stays restorable (ADR 0029 §3)                                           | Pure; no I/O                                                                        |
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

| Class                                                             | Examples                                                                                                                                                                                                 |
| ----------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Credential files                                                  | `auth.json`, `.credentials.json`, `.env`: never opened at all                                                                                                                                            |
| Values of environment maps                                        | `env` in settings, `env` of an MCP server: names shown, values never                                                                                                                                     |
| Headers and tokens of HTTP handlers                               | A hook of type `http`, an MCP server reached by URL with credentials                                                                                                                                     |
| Token and credential fields of MCP server entries                 | Bearer-token and similar fields of a server reached over the network. Reported by the independent review from Codex's configuration reference; to be enumerated per provider version with fixtures `[N]` |
| Anything in a provider-owned state file that is not configuration | Sign-in and session state in `~/.claude.json`                                                                                                                                                            |

A hook's command line is configuration and is shown; it can still contain a secret the user
wrote into it. That case is the subject of [11 §5](11-SECURITY-THREAT-MODEL.md).

## 5. Execution adapters

Unchanged by this set. The managed Codex adapter is described in
[10](10-MANAGED-RUNTIME-AND-SECURITY.md). A second execution adapter (F11b) must bring, before
any code: the provider's supported protocol with evidence, a safety contract equivalent to ADR
0015 and 0016, an isolation attestation, and an ADR. Nothing in the native plane is a step
towards it: starting `claude` in a terminal is not managing it.

## 6. Version policy and compatibility evidence

A provider updates itself, independently of the Console. What was verified for one version may
not hold for the next. This section is the design asked for by decision DN-23; nothing in it is
built, and how a row is renewed is still that decision.

### 6.1 By situation

| Situation                                               | Inventory                                                          | Mutation                                                           | Native start        |
| ------------------------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------- |
| Version with recorded evidence                          | Yes                                                                | Per capability                                                     | Yes                 |
| Newer or older version, same major configuration format | Yes, marked unverified                                             | Per tier (§6.3): some refused (`unverified_version`), some allowed | Yes, with a warning |
| Provider not installed                                  | Files are still listed if present, marked "provider not installed" | Refused                                                            | No                  |

### 6.2 The evidence manifest

One row per provider, version, capability and configuration form. A mutation is offered only
where a row allows it.

| Field              | Content                                                                                                                                       |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Provider           | Its identifier                                                                                                                                |
| Version            | One exact version; or a range, only where §6.3 allows one, with its justification written in the row                                          |
| Capability         | One identifier from the **closed set compiled into the build**: a row can switch a known capability on or off, and can never define a new one |
| Form               | The file, and for a hook the event, the handler type and the position (ADR 0029 §5)                                                           |
| Source of evidence | `fixture`, `documentation` with its date, or `real run` with its report                                                                       |
| Verified on, by    | Date; who ran it                                                                                                                              |
| Known limitations  | What the evidence does not cover: a rewrite path not exercised, a second computer not checked                                                 |
| Status             | `valid`, `invalidated`, or `not applicable`                                                                                                   |
| Invalidated by     | The version, date and reason, when a later observation contradicts the row                                                                    |

### 6.3 Tiers, by what a wrong assumption costs

| Tier | Capability                                              | Evidence valid for                                                                                                                                      | Why                                                                                                                                 |
| ---- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| E0   | Reading and inventory                                   | Any version whose files parse; marked unverified outside recorded versions                                                                              | A misreading is shown, not written                                                                                                  |
| E1   | A provider-native switch                                | Exact versions, or a range whose two ends both pass the same fixture and whose documented key did not change                                            | A wrong write of a documented key is visible and reversible                                                                         |
| E2   | Removing an entry to the vault and putting it back      | Any version on which the adapter **recognises** the entry's place and the file passes the round-trip check. Gated by recognition, not by a version list | The file left behind holds only ordinary entries. If the provider moves or reshapes hooks, recognition fails and nothing is offered |
| E3   | Keeping an entry in place, in a form the provider skips | One exact version, per cell. **Never a range**                                                                                                          | A provider that stops tolerating the form can drop every hook of the file (check V1)                                                |
| E4   | Running the provider's own writer                       | One exact version; the command and its arguments are part of the row                                                                                    | The desktop starts a provider's program (DN-13)                                                                                     |

The consequence that matters: the central capability, disabling one hook through the vault, is
tier E2 and does **not** switch off at every provider update. Tier E3 does.

### 6.4 Invalidation and safe degradation

| Event                                                                        | Effect                                                                                                                         |
| ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| The installed version has no row for a capability of tier E1, E3 or E4       | That capability is not offered (`unverified_version`), with the version it was last verified for. Nothing is written           |
| A row is marked `invalidated`                                                | The capability is off for those versions even where it was used before                                                         |
| Entries were kept in place (rung 2) under a version that no longer has a row | They are shown, never touched automatically, and can be restored only by the user's explicit act after a warning (ADR 0029 §5) |
| The adapter no longer recognises where entries live (E2)                     | The source is read only; records already in the vault stay, and their content can be revealed to the user (27 §8.3)            |
| The provider's version cannot be determined without running it               | Treated as unknown: tiers E1, E3 and E4 are off                                                                                |
| The manifest itself cannot be read or verified                               | The manifest compiled into the build stands; a manifest that fails its signature is ignored whole. Never a partial trust       |

Unsafe mutations fail closed. Nothing here ever widens what is allowed because evidence is
missing.

### 6.5 How a row is renewed for a new version (DN-23)

| Option | Mechanism                                                                                                                                                                                                                                                                                                             | Keeps the feature alive | Risk                                                                                                                                           |
| ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| R-a    | A new build of the Console carries the manifest                                                                                                                                                                                                                                                                       | No: off until the build | None new                                                                                                                                       |
| R-b    | A **signed manifest**, separate from the build. Data only: it can mark a pair of provider version and compiled-in capability as valid or invalidated, and nothing else. It cannot add a capability, a command, an argument, a path or a format. Verified against a key pinned in the build; applied by the user's act | Yes, without a build    | A signing key to protect; a channel to deliver it, which belongs to distribution (DN-07). No network path is added to the configuration engine |
| R-c    | A verification the user runs himself on a throwaway home, producing evidence valid for that installation only                                                                                                                                                                                                         | Yes, on that computer   | The Console runs a provider (DN-13, DN-24); the user's run is only as good as its procedure                                                    |
| R-d    | Accepting a range of versions                                                                                                                                                                                                                                                                                         | Yes                     | It assumes what the evidence rule exists to avoid. Allowed for E1 by its own rule; never for E3                                                |

Recommended: R-a now; tier E2 by recognition, so that the vault path survives provider updates;
R-b designed with distribution; R-c studied with the procedures of research RS1; R-d only as
§6.3 allows. A manifest never authorises an operation the build does not already contain, so a
remote file cannot make the Console run something new.
