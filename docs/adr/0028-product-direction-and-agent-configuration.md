# 0028 — Product direction: two execution planes and real agent configuration

Status: **Proposed.** Not accepted: seven of its ten decisions are still with the owner (see
[Decisions put to the owner](#decisions-put-to-the-owner)). Nothing in it is implemented.

Date: 2026-10-12

## Context

Four documents described Wyss Console and they did not agree: an architecture research report
(2026-10-01: a control kernel, the terminal last), the base specification v0.1 (2026-10-05:
terminal first, a process supervisor and three agents early), ADRs 0001 to 0027 (what was
built: one deep vertical of securely managed Codex) and a vision report (2026-10-12: a terminal
plus visual control of rules, hooks and several agents). None said which one rules.

What exists at `1ad90ee`:

- Human terminals whose backend starts the account's shell and nothing else
  (`src-tauri/src/terminal/worker.rs:79-82`). An agent's CLI can be started in one by hand;
  the Console neither starts it nor knows it is there.
- One managed Codex runtime, in one profile and for one workspace per generation
  (`src-tauri/src/control/runtime.rs:84-98`), with one managed turn at a time
  (`src-tauri/src/control/turn_store.rs:204-206`).
- Managed Codex that deliberately does not run the user's methodology. Local Tools hides the
  user's Codex home behind a private one
  (`src-tauri/src/adapters/codex/probe/isolation.rs:428-505`). Inference reads the user's real
  configuration and refuses to start unless every external hook is disabled
  (`src-tauri/src/adapters/codex/safety.rs:328-348`).

The owner's decision of 2026-10-12: Wyss Console must read and change the real global
configuration files of the installed agents, visibly and in both directions, and must keep the
managed execution boundaries exactly as they are. Global editing and independent per-session
profiles are two features and both are wanted. Security controls stay under his control and
never change silently.

One fact makes this more than a new view. ADR 0015 records that the owner disabled his four
Codex hooks outside Wyss because managed Inference rejects any enabled hook
(`0015-first-managed-turn-and-streaming.md:142-147`). His Codex hook list is empty today for
that reason. Controlling Codex hooks from the Console and running managed Inference are, as
built, in conflict.

## Decision

### 1. Two execution planes, never presented as one

- **Native plane.** An agent's own CLI in a Console terminal, with the user's real global and
  project configuration or with a session profile. The agent's own permissions and hooks decide
  what it does. Wyss Console configures and observes it; it does not mediate it.
- **Managed plane.** Codex under `wyss-control`, unchanged.

Every session shows its plane and the configuration it actually uses. Editing the global
configuration never makes a managed session claim to use it.

### 2. A configuration subsystem outside the authority

A Rust library (`src-tauri/src/agentconfig/`) hosted by the desktop backend, with no dependency
on Tauri or on `wyss-control`. The authority gains no access to user configuration; the
subsystem has no access to the control socket or to the authority's state. The window reaches
it through narrow commands that carry an item identifier and an operation, and never a path,
file content or command text.

### 3. Provider adapters that state what they can really do

One configuration adapter per agent. For each operation it declares one of `native`,
`by_removal`, `restart_required`, `read_only`, `unverifiable`. What was read on 2026-10-12 in
the providers' own documentation and in the installed binaries:

| Fact                                        | Claude Code 2.1.296                            | Codex 0.159.0                                             |
| ------------------------------------------- | ---------------------------------------------- | --------------------------------------------------------- |
| Per-hook switch                             | None: an entry is deleted or all hooks are off | Yes: `hooks/list` reports `enabled` for each hook         |
| A changed hook                              | Reloaded                                       | Skipped until the user trusts it again (trust is by hash) |
| Settings edited while a session is open     | Reloaded, except `model` and effort            | Not documented                                            |
| Per-session configuration without a rewrite | `--settings`, `--setting-sources`              | `--profile` (`~/.codex/<name>.config.toml`), `-c`         |

Consequences: disabling one Claude Code hook means removing its entry and keeping it elsewhere
so that it can be put back; Wyss Console never writes Codex's hook trust; only for Claude Code
may a change be reported as adopted by an open session.

### 4. Four scopes and three states

Scopes: system (read only), global, project, session. A session profile is applied when the
session is started, through the provider's own mechanism, and never writes a global file.

States, always shown apart: **declared** (what the file says), **effective** (what the provider
resolved, where it can be asked) and **running** (what each open session uses). An item is
never shown as active because a file says so.

### 5. One mutation pipeline

plan (pure) → show scope, redacted diff and affected sessions → confirm → check the file is
still the one planned against → back up → write a new file and rename it over the old one →
read back and verify → journal → report per session.

- **Round-trip precondition.** Before a file is edited, parsing and writing it back unchanged
  must reproduce it byte for byte; otherwise it is not edited. The diff of a change is then the
  change and nothing else. (Checked by reading, on 2026-10-12: both of the owner's Claude Code
  settings files round-trip exactly with key order preserved, two-space indentation and a final
  newline.)
- A change that weakens a security control, or that could introduce executable content, is
  confirmed in a native dialog that the page cannot answer, as the project folder already is
  (ADR 0021).
- The first mutations only switch, remove and restore entries that already existed, byte for
  byte. A hook runs commands: letting the page author one would turn a compromised page into
  command execution.
- Where the provider has its own writer (`claude mcp`, `codex mcp`) it is used. `~/.claude.json`,
  which Claude Code rewrites for itself, is never edited directly.
- Files are classified by owner: user, system, provider-owned, generated, link. A link is
  followed to a regular file of this user and never replaced; a generated file is read only.

### 6. No change to global hooks may break managed Inference silently

Managed Inference requires zero enabled external Codex hooks (ADR 0015). Therefore:

1. **Shown at rest.** The Configuration view states, on the Codex hooks themselves, that
   managed Inference needs all of them disabled, and whether that holds now.
2. **Predicted before applying.** Every plan against Codex's global or project configuration is
   evaluated for its effect on managed Inference. The prediction is conservative: a plan that
   leaves any external hook declared and enabled is reported as making Inference unavailable.
3. **Refused until resolved.** Until gate **G-INF** below is met, an operation predicted to make
   Inference unavailable is not offered: the adapter reports it as blocked, with that reason.
4. **Named afterwards.** When Inference is unavailable and enabled hooks are declared, the
   window says so in those words. Today the authority answers one generic code
   (`inference_safety_unproven`, `safety.rs:86`); the cause is added by the desktop from what
   the configuration subsystem knows, with no change to the authority.

**Gate G-INF.** Enabling a Codex hook from the Console ships only when points 1, 2 and 4 are
implemented and tested against the same fixtures as `zero_external_hooks`, and the owner has
chosen how the conflict is resolved (decision D7):

- **A. Explicit trade.** The owner enables hooks knowing that managed Inference is unavailable
  until they are disabled again; one confirmed action, one action to undo.
- **B. Decoupling.** Inference runs on a private Codex home, as Local Tools does, so that the
  user's hooks no longer reach it. This changes a security contract and needs its own ADR,
  compatibility tests and the owner's approval before any implementation.

Claude Code's hooks have no such coupling: no managed runtime reads them.

### 7. Two computers that share configuration

The owner's `~/.claude/` is replicated between two computers; Wyss Console's private state is
not. A hook removed on one computer disappears on the other, which has no parked copy.

- **Parked entries stay in the private state of the computer that parked them** (decision D5),
  with a backup of the whole previous file.
- **Every computer keeps its own last-seen copies.** The configuration subsystem keeps, per
  file, the last contents it observed. When a file changes from outside and an entry is gone,
  that computer can restore it from its own last-seen copy, without anything replicated.
- **When there is no local copy** (the Console never saw that file there), the view says:
  disabled on another computer, no local backup. The entry can be carried over by an explicit
  export on one computer and an import on the other; nothing is replicated by itself.
- **Reconciliation.** An entry restored on one computer reappears on the other as an outside
  change; the parked record there is marked restored elsewhere and is not applied again.
- A Syncthing conflict copy of a file blocks changes to that file until the user resolves it.
- What a change does to sessions open on the other computer cannot be known and is said so.

### 8. Honest concurrency

A plan is bound to the hash of the content it was computed from; applying it to changed content
fails without writing. A per-target lock serialises the Console's own writers. A provider
writing between the last check and the rename cannot be excluded: it is detected on read-back
and recoverable from the backup.

### 9. Dependencies (decision D6)

| Need                                  | Chosen                                               | New crates | Why                                                                                             |
| ------------------------------------- | ---------------------------------------------------- | ---------- | ----------------------------------------------------------------------------------------------- |
| Edit JSON keeping key order           | `serde_json`, feature `preserve_order`               | 0          | `indexmap` is already in `Cargo.lock`; exact round trip verified on the real files              |
| Edit TOML keeping comments and layout | `toml_edit` as a direct dependency                   | 0          | Already in `Cargo.lock` at 0.25.15 as a transitive dependency; maintained by the `toml` authors |
| Notice outside changes                | `nix`, feature `inotify`                             | 0          | `nix` 0.28 is already a direct dependency; avoids the `notify` crate and its tree               |
| Edit JSONC keeping comments           | Deferred to the OpenCode phase (`jsonc-parser` then) | 1, later   | Only OpenCode needs it; its file here has no comments today                                     |

Nothing is added before the phase that uses it.

## Compatibility with ADRs 0001 to 0027

Read for this matrix: the decision section of every ADR, and every clause in them about user
configuration, hooks, process start, the window's permissions and audit. **Stands** means
unchanged. **Extended** means 0028 adds beside it. **Constrains** means it limits how 0028 may
be built. Nothing is superseded.

| ADR  | Clause that matters here                                                                                   | Effect     | What follows                                                                                           |
| ---- | ---------------------------------------------------------------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------------------ |
| 0001 | Skills, permissions and presets deferred (`:43-47`)                                                        | Extended   | The deferral ends for configuration in the native plane                                                |
| 0002 | The Console is a client, not the security authority (`:9`)                                                 | Constrains | The configuration subsystem is Console-local and grants no authority                                   |
| 0003 | EventEnvelope v1                                                                                           | Stands     | The configuration journal is not an envelope stream (D8)                                               |
| 0004 | "Configuration projection" deferred (`:43`)                                                                | Extended   | A third category beside model provider and agent adapter: configuration adapter                        |
| 0005 | The frontend supplies no executable, argv or environment (`:49-51`); no arbitrary spawn (`:141`)           | Constrains | Starting an agent in a terminal needs its own ADR: a closed catalogue resolved in the backend          |
| 0006 | The frontend cannot choose an executable, argv or environment (`:86-87`)                                   | Constrains | Same                                                                                                   |
| 0007 | Profiles, settings and persistent sessions deferred (`:86-88`)                                             | Extended   | A registry of native agent sessions comes with that later ADR                                          |
| 0008 | `hooks` and `mcp_servers` are observe-only capabilities (`:91`); capability is not authority               | Stands     | Configuration capabilities are a separate declaration; 0008 gains no `request` on them                 |
| 0009 | The Codex codec is offline                                                                                 | Stands     | Its decoding may be reused to read; it writes nothing                                                  |
| 0010 | The probe reads and changes no configuration (`:14-16`)                                                    | Stands     | True of the probe. A live read of effective Codex state from the desktop needs its own ADR             |
| 0011 | Only the authority commits control state                                                                   | Stands     | No control method is added                                                                             |
| 0012 | One shared runtime (`:8`); Tauri neither starts nor connects to it (`:11`)                                 | Stands     | Several runtimes are not decided here                                                                  |
| 0013 | Execution is denied by default                                                                             | Stands     | —                                                                                                      |
| 0014 | One permanent binding per session                                                                          | Stands     | —                                                                                                      |
| 0015 | One turn at a time (`:9`); zero enabled hooks (`:138-139`); "Wyss does not edit the user's hooks" (`:147`) | Constrains | The guarantee stands. The sentence is true of the authority and is narrowed to it. Section 6 and G-INF |
| 0016 | Local Tools on a private Codex home (`:43-45`)                                                             | Stands     | The view says managed Local Tools does not use the global configuration                                |
| 0017 | The event stream is audit, not authority                                                                   | Stands     | —                                                                                                      |
| 0018 | The managed core is complete                                                                               | Stands     | No part of it is redesigned                                                                            |
| 0019 | The desktop only reads the authority (`:8`)                                                                | Stands     | The Configuration view does not use `AuthorityService`                                                 |
| 0020 | Session intents are "the only" mutations the desktop sends (`:10`)                                         | Stands     | True of control intents. A configuration write is not one and never reaches the authority              |
| 0021 | A folder is chosen in a native dialog the page cannot answer                                               | Extended   | The same principle confirms a change that weakens security                                             |
| 0022 | Managed turns from the window                                                                              | Stands     | —                                                                                                      |
| 0023 | Local Tools and approvals from the window                                                                  | Stands     | —                                                                                                      |
| 0024 | Events shows what the authority recorded                                                                   | Stands     | Configuration history is its own list (D8)                                                             |
| 0025 | The desktop installs files for the user under a provenance rule                                            | Stands     | Precedent for writing outside its own directory with strict checks                                     |
| 0026 | Settings never change what the product may do (`:18-20`)                                                   | Constrains | Configuration is a different view with a different contract and is not merged into Settings            |
| 0027 | No distribution packaging                                                                                  | Stands     | No path, user name or host is fixed in code                                                            |

`README.md:848` ("Wyss never edits them") is corrected to speak of the authority when the first
mutation ships, not before.

## Roadmap

Each phase leaves the terminal and managed Codex exactly as they are and can be the last.

| Phase | Delivers                                                                                                          | Before it                            | Accepted when                                                                                                       |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------- |
| F0    | This ADR, `docs/PRODUCT.md`, the matrix above                                                                     | —                                    | The owner has answered D1 to D10                                                                                    |
| F1    | Read-only inventory for Claude Code and Codex; the Configuration view; outside changes noticed; section 6 point 1 | F0 accepted                          | Fixture inventories match exactly; no secret value reaches the page; **no file is written**                         |
| F1b   | Study only: can Inference run on a private Codex home? A written contract and its compatibility tests             | F1                                   | A report and a draft ADR. No guarantee is changed                                                                   |
| F2a   | Mutations with no managed coupling: Claude Code skills, plugins, all-hooks switch, hook parking and restore       | F1; D3, D4, D5 answered              | A file broken on purpose, an outside change mid-way and a simulated power cut all leave a valid file; undo is exact |
| F2b   | Mutations of Codex configuration, each with its effect on managed Inference predicted                             | F2a; section 6 points 2 and 4        | Enabling a hook is blocked with its reason                                                                          |
| G-INF | Enabling a Codex hook                                                                                             | F2b; D7 answered (A, or B after F1b) | The conflict is shown before, named after and undone in one action                                                  |
| F3    | Starting an agent in a terminal, a registry of native sessions, session profiles                                  | Its own ADR (0005 to 0007)           | Two sessions with different profiles at once and no global file changed                                             |
| F4    | OpenCode and Hermes adapters, read only first                                                                     | F1                                   | As F1, per agent                                                                                                    |

Real integration on the owner's configuration happens only with his explicit authorisation, per
phase. Everything else runs on synthetic fixtures and private homes.

## Decisions put to the owner

| #   | Decision                                     | Recommended                                              | State                                                                                       |
| --- | -------------------------------------------- | -------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| D1  | Two planes as the definition of the product  | Yes                                                      | General direction approved; not confirmed as D1                                             |
| D2  | Where the configuration subsystem lives      | A library in the desktop backend, outside the authority  | General direction approved; not confirmed as D2                                             |
| D3  | Scope of the first mutations                 | Switch, remove and restore existing entries only         | Open                                                                                        |
| D4  | Confirming a change that weakens security    | A native dialog, every time                              | Open                                                                                        |
| D5  | Where parked hooks are kept                  | The Console's private state, per computer                | **Answered, preliminary**: private state, with backup and restore; section 7                |
| D6  | New dependencies                             | The set in section 9: no new crate until OpenCode        | **Answered, preliminary**: only what is needed, each justified; the set awaits confirmation |
| D7  | Managed Inference and the user's Codex hooks | Study B (F1b); allow A meanwhile                         | **Answered, preliminary**: investigate only; no guarantee altered; A or B still to choose   |
| D8  | Where configuration history is recorded      | Its own local journal, apart from the authority's stream | Open                                                                                        |
| D9  | Order of agents                              | Claude Code, Codex, OpenCode, Hermes                     | Open                                                                                        |
| D10 | Document hierarchy                           | `docs/PRODUCT.md`, `docs/spec/`, ADRs, README            | Open                                                                                        |

## Not decided here

Several managed runtimes or turns at once; a managed profile that admits user rules; handing
work between agents; showing the model of a session; remote access; a personal assistant and
the relation between the original Wyss assistant and Hermes; distribution to other users.

## To be measured before acceptance of each phase

- Where Codex persists a hook disabled from its `/hooks` browser.
- The exact effect of `--setting-sources` on hooks, which Claude Code merges across levels.
- Whether Codex and OpenCode apply a configuration change to a running session.
- Whether Claude Code's effective settings can be read without an interactive session.
- Everything about Hermes, which is not installed.

## References

- [`docs/PRODUCT.md`](../PRODUCT.md).
- Claude Code settings and hooks documentation, Codex configuration and hooks documentation,
  OpenCode configuration documentation, read on 2026-10-12.
- ADRs 0001 to 0027.
