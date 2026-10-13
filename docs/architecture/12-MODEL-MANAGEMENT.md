# 12 — Model management

Domain H of [04](04-TARGET-ARCHITECTURE.md). Two phases that must not be merged: **seeing**
models (F6a) and **choosing** them (F6b). Everything is `[P]` unless marked otherwise.

**Revision of 2026-10-13 (fourth planning pass).** This revision replaces the first version, which
was a sketch. It adds:

- the vocabulary (§1);
- a provider capability matrix with dated sources (§3);
- visibility contracts (§4);
- the five paths of selection and switching (§5);
- usage and cost (§6);
- closed words (§7);
- tests and acceptance (§8);
- the proposed [ADR 0031](../adr/0031-observed-model-of-managed-sessions.md) for DN-09 (§9).

It was written by the author of the set and is **unreviewed**. It authorises no implementation,
no provider run and no configuration change. Where it and 13 S7 differ, this document rules for
facts and 13 rules for layout.

## 1. Vocabulary

### 1.1 Five facts about "the model"

| #   | Fact                                     | Example                                                     | Who knows it                       | Layer    |
| --- | ---------------------------------------- | ----------------------------------------------------------- | ---------------------------------- | -------- |
| M1  | The provider                             | Codex, Claude Code                                          | The session's adapter              | Session  |
| M2  | The catalogue a runtime offers           | Seven entries, one marked default                           | The runtime, when asked            | Provider |
| M3  | The model configured at a scope          | `model = …` in a settings file                              | The provider's file                | File     |
| M4  | The model chosen for a session           | A start flag, or a switch typed during the session          | The provider, for that session     | Session  |
| M5  | The model the provider reports as in use | What a thread's start answer or a turn's notification names | The provider, at the grain it uses | Session  |

M5 has a **grain**:

- **Session grain:** the provider names the model of a session or thread. Codex's App Server does
  this in its answer to `thread/start` and `thread/resume` (§3).
- **Turn grain:** the provider names the model of one turn. Codex does this only when it reroutes
  a turn (§3, `model/rerouted`).

A session-grain report is never shown as the model of a particular turn.

### 1.2 Two rules that every screen obeys

1. **Never infer M5 from M2, M3 or M4.** A default in the catalogue, a value in a file, or a flag
   passed at start says what was asked for, not what answered (AD-16). When nothing is
   reported, the screen says "not reported". It does not substitute a default.
2. **A catalogue listing is not entitlement.** That a runtime lists a model does not mean the
   account may use it. This is "listing is not entitlement" (ADR 0012) `[R]`. The adapter already
   sets every catalogue entry's `availability` to `unknown` (`adapters/codex/normalize.rs:163`)
   `[R]`.

### 1.3 Identifiers and display names

| Term                | Meaning                                                                                                                  | Rule                                                                                                                                                           |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Model identifier    | The provider's opaque string, at most 256 bytes (ADR 0008, `AgentModelDescriptorV1.model_id`) `[R]`                      | Compared byte for byte. Never parsed for meaning, never normalised, never completed                                                                            |
| Provider identifier | The model's provider where the runtime names one (`modelProvider` in Codex; the `provider/` part in OpenCode and Hermes) | Nullable `[R]`. Shown beside the model, never guessed from the model identifier                                                                                |
| Display name        | The runtime's label, at most 128 bytes `[R]`                                                                             | Shown with the identifier beside it in the detail view. Two entries with one display name are two entries                                                      |
| Alias               | A name the provider resolves to a model (Claude Code: `default`, `opus`, `sonnet`, `opusplan` and others) `[D]`          | Shown as written, with "alias; <agent> decides which model it means". The Console never resolves an alias: the resolution varies by provider and version `[D]` |

### 1.4 States of a value

Each value on a screen is in exactly one of these states. The state is shown in words, not by
colour alone (13 §5).

| State          | Meaning                                                                                         | Example sentence                                         |
| -------------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| **default**    | The runtime or catalogue marks it as its default                                                | "Default of this runtime"                                |
| **configured** | Written at a scope of the provider's configuration (M3)                                         | "Configured in user settings: X"                         |
| **selected**   | Passed by the Console for this session at start (M4); a switch typed in the session is not seen | "Started with: X"                                        |
| **effective**  | The documented winner among scopes and flags, by the provider's documented order                | "Applies to new sessions, by <agent>'s documented order" |
| **observed**   | Reported by the provider (M5), with its grain                                                   | "Reported for this session: X"                           |
| **unknown**    | None of the above can be said                                                                   | "Not reported"                                           |

- "Effective" is a **documented** conclusion about future sessions, from 06's precedence tables.
  It is never presented as observed.
- "Selected" is what Wyss passed. Whether the provider honoured it is "observed" or "not reported".

### 1.5 Effort settings

"Effort" means a provider's reasoning or thinking setting:

- Claude Code: `effortLevel` and `/effort` `[D]`.
- Codex: `model_reasoning_effort` `[D]`; in the App Server, `supportedReasoningEfforts` and
  `reasoningEffort` `[D]`.
- OpenCode: variants `[D]`.
- Hermes: `agent.reasoning_effort` and `/reasoning` `[D]`.

Values are provider strings, kept as opaque as model identifiers. They follow the same five facts:
offered (catalogue metadata), configured, selected, observed and unknown. The Console never maps
one provider's effort levels onto another's.

### 1.6 Freshness and provenance

Every shown value carries:

- its **source** (which file, runtime or report);
- its **time** (when it was read or reported);
- its **generation** where one exists (the managed runtime's generation for a catalogue; the
  configuration snapshot for a file).

A value whose source has gone (the runtime stopped, the file was removed, the session detached)
is withdrawn or marked stale as §4 says per value. It is never kept on screen as if current.

### 1.7 Unavailable, deprecated, renamed and unrecognised models

| Condition                           | Where it is known                                                                                                        | Shown                                                                                                                                                                       |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Listed, entitlement unknown         | Every catalogue entry `[R]`                                                                                              | "Offered by this runtime; not a guarantee for your account"                                                                                                                 |
| Hidden from the default list        | Codex `hidden`; the authority asks with `includeHidden: false` (ADR 0012) `[R]`                                          | Not listed. An observed model that is not in the list is shown as `model_not_in_catalogue`; that a hidden model may explain it is not claimed                               |
| Marked for replacement              | Codex `upgrade` and `upgradeInfo` (`model`, `retirementAt`, `modelLink`, `upgradeCopy`, `migrationMarkdown`) `[D]`       | "The runtime marks this model for replacement by Y" with the date when given. The provider's own copy and Markdown are **never** rendered (FR-ADP-05). Not read today `[R]` |
| Configured but not in the catalogue | A file names a model that the same agent's readable list does not have; no native agent has one today (MC1)              | `model_not_in_catalogue`, or no remark when no list exists. Never "invalid": the Console cannot know                                                                        |
| Unrecognised by the provider        | Claude Code prints a `[claude-code:unrecognized_model]` diagnostic `[D]`; Hermes `migrate` rewrites retired models `[D]` | Not read: the Console does not parse a terminal's output (05 §6). The provider says it in its own session                                                                   |
| Renamed                             | Not reported by any provider in a form the Console reads `[N]`                                                           | Two identifiers are two models. The Console never links an old name to a new one                                                                                            |
| Rerouted during a turn              | Codex `model/rerouted` `[D]`                                                                                             | `model_rerouted` (§7), when ADR 0031 is accepted                                                                                                                            |

### 1.8 Entitlement

Entitlement is never known to the Console. No screen says "available to you", "included in your
plan" or the like. A start that the provider refuses for entitlement is the provider's own
message, in the session.

### 1.9 Configuration changes while a session runs

A change to M3 affects **new** sessions. What a running session does with it is per provider:

| Provider    | Effect on running sessions                                                                                                                                                                                   |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Claude Code | `model`, `effortLevel` and `modelSettings` are read once at start `[D]` (06 §3.1). `--model` and `ANTHROPIC_MODEL` apply to the launched session only; resumed sessions keep their saved model `[D]` (SRC-1) |
| Codex       | `[N]` (OQ-03)                                                                                                                                                                                                |
| OpenCode    | `[N]` (OQ-04)                                                                                                                                                                                                |
| Hermes      | `[N]`                                                                                                                                                                                                        |

The sentence follows the evidence:

- **With evidence:** "Applies to new sessions. N running sessions keep the previous one." (07 §6.)
- **Without evidence:** "Applies to new sessions. Whether the N running <agent> sessions pick it
  up is not known."

### 1.10 Settings, flags and session commands

Three layers, never shown as one value:

| Layer                    | What it is                                          | Lifetime                             | Who writes it                                                                       |
| ------------------------ | --------------------------------------------------- | ------------------------------------ | ----------------------------------------------------------------------------------- |
| **File** (M3)            | A key in a provider's configuration file at a scope | Until changed                        | The user; the provider's own commands (below); later the Console's engine (§5, SP3) |
| **Launch** (M4)          | A start flag or environment variable                | The session                          | Whoever starts the session; for a Console start, the launch plan (05 §17)           |
| **Session command** (M4) | A command typed in the running session              | The session, or the file too (below) | The user, in the terminal                                                           |

Some session commands also **write the file layer**:

- Claude Code's `/model` "switches immediately and saves as the default" `[D]` (SRC-1).
- Hermes's `/model … --global`, `hermes config set model` and `hermes model` write `config.yaml`
  `[D]` (SRC-7).

To the configuration engine these are outside writers (07 §7; 28). The resulting change is
attributed "changed outside Wyss Console", never to the session. Which file and scope Claude Code
writes is OQ-49.

## 2. What exists today `[R]`

| Fact  | Managed Codex                                                                                                                                                                                                                            | Native sessions                                              |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| M1    | Shown ("Codex")                                                                                                                                                                                                                          | Not shown: the Console does not know what runs in a terminal |
| M2    | One `model/list` per generation (ADR 0012); the authority holds the descriptors in memory and exposes `runtime/model/list` (pages of 1 to 20); the window receives only `model_count` (`authority/view.rs:33`)                           | —                                                            |
| M3    | Not applicable: the managed runtime's model presentation is authority-generated (ADR 0015)                                                                                                                                               | In the provider's files; not read                            |
| M4    | None. Thread start parameters are exactly `{cwd, ephemeral: false}` and turn start parameters `{threadId, input}`; "no model/provider/approval/sandbox/config/instruction overrides" (ADR 0014; ADR 0015; `probe/supervised.rs:602-640`) | —                                                            |
| M5    | The adapter reads `model` and `modelProvider` from the thread's start or resume answer into `observed_model` (`adapters/codex/normalize.rs:197-211`). It is "discarded" before the control plane (ADR 0014)                              | —                                                            |
| Turn  | `model/rerouted` is not in the list of tolerated notifications (`adapters/codex/safety.rs:1203-1218`); by reading, it reaches `_ => violation()` (`:1220`) and fails the turn as `inference_isolation_violation`. Not run                | —                                                            |
| Usage | `thread/tokenUsage/updated` and `account/rateLimits/updated` are tolerated and dropped (`safety.rs:1203-1218`)                                                                                                                           | —                                                            |

Where model selection is excluded: it is excluded by ADR 0014, ADR 0015 and the exact parameter
sets of the codec (`probe/supervised.rs:602-640`; `contracts/control.rs` has no model field, and
unknown fields are rejected).

- **SG-12 (10 §2) does not mention a model.** Its text is: "No client can supply an executable,
  arguments, an environment, a workspace override or a tool."
- Earlier documents attributed the exclusion to SG-12; that is C14 (31 §5). SG-12 is unchanged by
  this document.

## 3. Provider capability matrix

### 3.1 Sources

SRC-1 to SRC-7 were read on 2026-10-13; SRC-0 is the earlier record this set already holds.
Nothing here was run: every behaviour that needs a provider run is `[N]`.

| Source | What                                                                                                                                                                                                                                                                       | Version or date                                                                             | Grade                             |
| ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | --------------------------------- |
| SRC-0  | 06 §3: the provider specifications, from documentation read on 2026-10-12 (AS6) and `--help` of the installed Claude Code 2.1.296 and Codex 0.159.0 `[O]`                                                                                                                  | As recorded there                                                                           | A for `[O]`; as 06 says for `[D]` |
| SRC-1  | Claude Code, "Model configuration" (`code.claude.com/docs/en/model-config`), read in full                                                                                                                                                                                  | Live page; names releases up to v2.1.293                                                    | A                                 |
| SRC-2  | Claude Code, "Status line" (`code.claude.com/docs/en/statusline`), read in full                                                                                                                                                                                            | Live page, same day                                                                         | A                                 |
| SRC-3  | Codex App Server protocol schema and source (`codex-rs/app-server-protocol`) at tag `rust-v0.159.0`: `ClientRequest.json`, `ServerNotification.json`, `v2/ModelListResponse.json`, `v2/ThreadStartResponse.json`, `v2/ThreadResumeResponse.json`, `src/protocol/common.rs` | Pinned tag. The authority's codec is commented "0.159.0 protocol" (`safety.rs:818`) `[R]`   | A                                 |
| SRC-4  | The same schema at commit `0b863c69`, cited by `docs/research/agent-runtime-surfaces-2026-10-06.md`                                                                                                                                                                        | Pinned commit; agrees with SRC-3 on the request, notification and catalogue fields compared | A                                 |
| SRC-5  | Codex CLI pages on `developers.openai.com` (slash commands, configuration, changelog)                                                                                                                                                                                      | **Search excerpts only**: the site does not resolve from this environment                   | C                                 |
| SRC-6  | OpenCode pages on `opencode.ai/docs` (CLI, models, configuration)                                                                                                                                                                                                          | **Search excerpts only**, same reason                                                       | C                                 |
| SRC-7  | Hermes `website/docs` in `NousResearch/hermes-agent` (`configuration.md`, `cli-commands.md`, `cli.md`), read in the parts cited                                                                                                                                            | Branch `main`, **unpinned**; the installed version is unknown (OQ-11)                       | B                                 |

Grade A: a pinned or versioned primary page read in full, or `[O]` output of the installed
version. Grade B: a primary page read, but unpinned. Grade C: wording seen in a search excerpt of
an official page that was not read in full. A grade C fact may be used to name a capability. It is
not enough to build a start recipe or a command hint on (06 §6; DN-23).

### 3.2 Matrix

Codex appears twice:

- **Codex CLI** is the native terminal program.
- **Codex App Server** is the managed runtime the authority drives.

Cells name the mechanism and the source.

| #    | Capability                                | Claude Code                                                                                                                                                                                                                                                | Codex CLI                                                                                                             | Codex App Server (managed)                                                                                                                                                                                                               | OpenCode                                                                                                                                | Hermes                                                                                                                                                                                                           |
| ---- | ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| MC1  | Catalogue a program can read (M2)         | None found. `/model` is an interactive picker; `availableModels` is an allowlist an organisation sets, not a catalogue `[D]` SRC-1                                                                                                                         | `[N]`                                                                                                                 | `model/list` (`cursor`, `includeHidden`, `limit`). Each entry has `id`, `model`, `displayName`, `hidden`, `isDefault`, `supportedReasoningEfforts`, `defaultReasoningEffort`, `upgrade`, `upgradeInfo` and more `[D]` SRC-3; built `[R]` | `opencode models [provider]` lists models of configured providers `[D]` SRC-6, grade C. Running it is running a provider binary (DN-13) | No program-readable list found. `/model` shows options inside a session; `hermes model` is an interactive wizard `[D]` SRC-7                                                                                     |
| MC2  | Configured model (M3) and its order       | Settings `model`; `ANTHROPIC_MODEL`; `ANTHROPIC_DEFAULT_MODEL` (v2.1.236+). Order: `/model`, then `--model`, then `ANTHROPIC_MODEL`, then settings, then `ANTHROPIC_DEFAULT_MODEL` `[D]` SRC-1                                                             | `model`, `model_reasoning_effort` in `config.toml` `[D]` (06 §3.2); profiles `[profiles.<name>]` `[D]` SRC-5, grade C | Not applicable: authority-generated (ADR 0015) `[R]`                                                                                                                                                                                     | `model` `[D]` (06 §3.3); `small_model`, as `provider/model` `[D]` SRC-6, grade C                                                        | `config.yaml` `model` section; `hermes config set model …`. Order: command-line arguments, then `config.yaml`, then `.env` `[D]` SRC-7                                                                           |
| MC3  | Choose at start (M4)                      | `--model <alias or name>` `[D]` SRC-1; `[O]` in `--help` of 2.1.296 (06 §3.1)                                                                                                                                                                              | `-m`/`--model`, `-p`/`--profile` `[O]` in `--help` of 0.159.0 (06 §3.2)                                               | `thread/start` and `thread/resume` accept `model`, `modelProvider` `[D]` SRC-3. **Not sent** (ADR 0014) `[R]`                                                                                                                            | `--model`, `-m` as `provider/model` `[D]` SRC-6, grade C. Order: flag, then configuration, then last used, then built-in default        | `hermes chat -m`/`--model`, `--provider`; `HERMES_INFERENCE_MODEL` for `-z` runs `[D]` SRC-7                                                                                                                     |
| MC4  | Switch in a running session               | `/model <alias or name>`; in the picker, `s` switches for this session only `[D]` SRC-1                                                                                                                                                                    | `/model`, "choose the model for the current chat" `[D]` SRC-5, grade C                                                | `turn/start` accepts `model` and `effort`, "for this turn and subsequent turns" `[D]` SRC-3. **Not sent** (ADR 0015) `[R]`                                                                                                               | `[N]`                                                                                                                                   | `/model <name>` or `/model <provider>:<model>`, among providers already set up `[D]` SRC-7                                                                                                                       |
| MC5  | Does the switch also write the file layer | Yes: `/model` "saves as the default" `[D]` SRC-1. Which file and scope: OQ-49                                                                                                                                                                              | `[N]`                                                                                                                 | Not applicable                                                                                                                                                                                                                           | `[N]`. "Last used" in the order above suggests some memory; not documented further                                                      | No by default. `--global`, or `model.persist_switch_by_default: true`, writes `config.yaml` `[D]` SRC-7                                                                                                          |
| MC6  | Effort or reasoning                       | `/effort`, `--effort`, `CLAUDE_CODE_EFFORT_LEVEL`, `effortLevel`, per-model `modelSettings`; levels include `low`, `medium`, `high`, `xhigh`, `max` `[D]` SRC-1                                                                                            | `model_reasoning_effort` `[D]` (06 §3.2); `/reasoning` for the current chat `[D]` SRC-5, grade C                      | Offered: `supportedReasoningEfforts`, `defaultReasoningEffort` per catalogue entry. Reported: `reasoningEffort` in thread start and resume answers `[D]` SRC-3. Neither reaches the window `[R]`                                         | Variants per model; `opencode run --variant` `[D]` SRC-6, grade C                                                                       | `agent.reasoning_effort`; `/reasoning <level>`; per auxiliary task `reasoning_effort` `[D]` SRC-7                                                                                                                |
| MC7  | Model reported in use (M5)                | `/status` shows it to the user; headless `json` and `stream-json` output carries `modelUsage`; a status-line command receives `model.id` and `model.display_name` `[D]` SRC-1, SRC-2. None reaches the Console without a hook or status line the user adds | `[N]`                                                                                                                 | Session grain: `model`, `modelProvider`, `reasoningEffort`, `serviceTier` in thread start and resume answers. A `Turn` has **no** model field `[D]` SRC-3. Read by the adapter, discarded `[R]`                                          | `[N]`                                                                                                                                   | `stream-json` `system`/`init` carries `model` (only with `-q`); the usage file carries `model` and `provider` (only with `-z`) `[D]` SRC-7                                                                       |
| MC8  | Model change reported during a session    | `fallbackModel` / `--fallback-model` (up to three); `switchModelsOnFlag` `[D]` SRC-1. Hook events `PreModelSwitch`, `PostModelSwitch` `[D]` (06 §3.1), reachable only by a hook the user adds. What a fallback reports: `[N]`                              | `[N]`                                                                                                                 | `model/rerouted`: `threadId`, `turnId`, `fromModel`, `toModel`, `reason` (closed set: `highRiskCyberActivity`). **Not** marked experimental at this tag (`common.rs:1995`) `[D]` SRC-3. Whether and when it is sent: `[N]` (OQ-50)       | `[N]`                                                                                                                                   | Fallback providers (`hermes fallback`) `[D]` SRC-7; how a fallback is reported: `[N]`                                                                                                                            |
| MC9  | Token usage reported                      | Status line `context_window` figures; headless `modelUsage` `[D]` SRC-1, SRC-2                                                                                                                                                                             | `[N]`                                                                                                                 | `thread/tokenUsage/updated`: `threadId`, `turnId`, `tokenUsage.last` and `.total` (input, cached input, cache write, output, reasoning output, total), `modelContextWindow` `[D]` SRC-3. Dropped `[R]`                                   | `[N]`                                                                                                                                   | `stream-json` `result.tokens`; the usage file's counters `[D]` SRC-7                                                                                                                                             |
| MC10 | Money or allowance reported               | `cost.total_cost_usd`, "estimated … computed client-side at list price … may differ from your actual bill"; `rate_limits` (spend limit an estimate, v2.1.284+), through a status line only `[D]` SRC-2                                                     | `[N]`                                                                                                                 | **No money figure.** `account/rateLimits/updated`: windows with `usedPercent`, `windowDurationMins`, `resetsAt`; `credits`, `planType`, `spendControlReached` `[D]` SRC-3. Dropped `[R]`                                                 | `[N]`                                                                                                                                   | `estimated_cost_usd` in the status bar and usage file, from Hermes's bundled price table, the provider's listing, or a public list price; `hermes usage` shows rate-limit windows; `hermes insights` `[D]` SRC-7 |

### 3.3 Reading the matrix

- **Only the managed runtime gives the Console a channel.** In the native plane, each provider
  reports its model and usage inside its own terminal or through output modes the Console does
  not use. The native column of every F6a value is therefore "not reported" (§4). Adding a hook
  or status line to learn it would change the configuration the Console reports on (05 §6). The
  user may add one through the engine as their own decision. The Console never adds one.
- **Grade C rows are not a basis for building.** The Codex CLI's in-session command (MC4) and
  every OpenCode row that F6b would build on need their official pages read in full first
  (OQ-51). The start flags of Claude Code and the Codex CLI are `[O]` from `--help` (06 §3).
- **"Cost" is an estimate in both providers that report one.** Claude Code and Hermes both
  compute it from price tables of their own. Neither is a bill (§6).
- **The managed protocol can do more than the boundary allows.** Codex's protocol accepts a model
  and an effort at thread start and at turn start (SRC-3). The authority sends neither, by
  decision (ADR 0014, 0015). This is a boundary, not a gap of the provider (§5, SP5).

## 4. F6a — Visibility contracts

The page reads models through **IF-MODEL**, a read-only interface of the desktop backend. It owns
no state. Each operation is a projection over an owner listed below:

- `catalogue(agent)`;
- `configured(agent, project)`, a view over IF-CONFIG's inventory;
- `session_models(session)`;
- `start_choices(agent, profile)`;
- `switch_hint(agent)`.

Its failures are the closed words of §7.

| Value | What the user sees                                         | Owner                                                               | Evidence source                                            | Freshness                                                                                                                       | Failure behaviour                                                                                                                                     | Layer    |
| ----- | ---------------------------------------------------------- | ------------------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| MV-01 | Provider of a session (M1)                                 | Native registry (05 §5); authority session record `[R]`             | The launch request; the binding `[R]`                      | Fixed for the session's life                                                                                                    | A hand-started program: "Started by hand" (AC-F3b-03); never a guessed provider                                                                       | Session  |
| MV-02 | Managed catalogue entries, default marked                  | The authority's runtime supervisor `[R]`                            | One `model/list` at the generation's handshake `[R]`       | Per runtime generation. Shown with "listed at <time>"; cleared when the runtime stops (ADR 0012)                                | `catalogue_unavailable` (not running); `catalogue_rejected` (`failed/catalog_rejected` `[R]`); `catalogue_partial` (more entries than were asked for) | Provider |
| MV-03 | Native catalogue                                           | None                                                                | MC1                                                        | —                                                                                                                               | `catalogue_not_readable`. OpenCode's command is not run unless DN-13 allows it                                                                        | Provider |
| MV-04 | Configured model and effort per scope (M3)                 | Configuration engine (07), read-only inventory                      | The provider's files                                       | The watcher (07 §7). Shown "as read at <time>"; a change re-reads                                                               | IF-CONFIG's `source_*` words (13 §9.2); the other scopes stay shown                                                                                   | File     |
| MV-05 | The documented winner among scopes and flags ("effective") | Adapter precedence table (06), per verified version                 | MC2 rows; SRC-1, SRC-7                                     | Per verified provider version (06 §6; DN-23)                                                                                    | `scope_winner_unverified`: every scope is shown, none marked                                                                                          | File     |
| MV-06 | Model and effort a native session was started with (M4)    | Native registry: the launch plan's `overrides` as passed (05 §17.2) | The Console's own launch                                   | Fixed at start. A switch typed later is not seen                                                                                | Absent: "Started without a model choice". Never shown as in use                                                                                       | Session  |
| MV-07 | Model in use, native (M5)                                  | None                                                                | MC7: no channel                                            | —                                                                                                                               | Always `model_not_reported`, with MV-06 or MV-05 beside it, labelled as such                                                                          | Session  |
| MV-08 | Model in use, managed, session grain (M5)                  | The authority, under ADR 0031                                       | `thread/start` and `thread/resume` answers `[D]` SRC-3     | Per attachment. Detached: "Not attached; last reported <time>" only if ADR 0031's durable option is chosen; otherwise withdrawn | Absent or malformed in the answer: `model_not_reported`. Never filled from the catalogue default                                                      | Session  |
| MV-09 | Reroute of a managed turn                                  | The authority, under ADR 0031                                       | `model/rerouted` `[D]` SRC-3                               | Per turn                                                                                                                        | Today the turn fails closed (§2). Under ADR 0031: a malformed or mismatched notice still fails the turn closed; a valid one is `model_rerouted`       | Session  |
| MV-10 | Effort reported for a managed session                      | The authority, under ADR 0031                                       | `reasoningEffort` in the same answers `[D]` SRC-3          | As MV-08                                                                                                                        | As MV-08                                                                                                                                              | Session  |
| MV-11 | Managed effort offered per model                           | The authority `[R]` (already in each descriptor's metadata)         | Catalogue metadata `[D]` SRC-3                             | As MV-02                                                                                                                        | As MV-02                                                                                                                                              | Provider |
| MV-12 | Marked for replacement                                     | The authority, if ADR 0031's catalogue part is accepted             | `upgrade`, `upgradeInfo.model`, `retirementAt` `[D]` SRC-3 | As MV-02                                                                                                                        | Absent: nothing shown. Provider text is never shown                                                                                                   | Provider |
| MV-13 | How to switch in a running session                         | Adapter documentation table                                         | MC4, grade A or B only                                     | Per verified provider version                                                                                                   | `switch_command_unknown` for grade C or `[N]`                                                                                                         | Session  |
| MV-14 | Usage and allowance                                        | §6                                                                  | §6                                                         | §6                                                                                                                              | `usage_not_reported`                                                                                                                                  | Session  |

Rules over the table:

1. **One value, one owner.** The page never combines two rows into one value. "Configured: X.
   Reported for this session: Y" is two values, shown side by side.
2. **File, provider and session never merge.** A row's layer is shown by its label ("in user
   settings", "offered by this runtime", "reported for this session").
3. **Stale is said.** A value past its freshness is withdrawn, or marked with its time. A
   catalogue from a stopped runtime is never shown.
4. **Model history.** History of model changes (FR-MOD-09) depends on the provider reporting them:
   - **Managed:** rows exist only under ADR 0031's durable option (§9). Without it, a reroute is
     visible only while the turn is shown.
   - **Native:** the configuration history (13 S10) holds file-layer changes, including those the
     provider's own commands make. A switch typed in a session is not seen.

## 5. F6b — Selection and switching

### 5.1 Five paths

| Path | What                                                                                                  | Who acts                              | Wyss's role                                                         | Status                                                                                                                                                                                 |
| ---- | ----------------------------------------------------------------------------------------------------- | ------------------------------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| SP1  | **Provider-native command**: the user types `/model` (or the provider's equivalent) in the session    | The user, in the terminal             | Shows which command, from MV-13; nothing else                       | Designed (§5.2). DN-38 (a)                                                                                                                                                             |
| SP2  | **Documented start flag**                                                                             | The launcher, through the launch plan | Passes one identifier as one argument of a documented flag (05 §17) | Designed (§5.3) for flags of grade A or B                                                                                                                                              |
| SP3  | **Wyss configuration-file mutation**: change the default (M3) in the provider's file                  | The configuration engine (07 §5)      | Plans and writes through the write protocol                         | **Blocked**: direct provider-file writes are blocked (L1, L2, L5; DN-21; DN-31). Until then, `default_change_blocked`, naming the file and the provider's own command where one exists |
| SP4  | **Renderer input into a live terminal**: the Console places text in an agent's input line             | The page, writing into a PTY          | —                                                                   | **Not offered.** DN-38 (b), open. Never silent (§5.4)                                                                                                                                  |
| SP5  | **Change to the managed plane**: a model or effort in `thread/start`, `thread/resume` or `turn/start` | The authority                         | —                                                                   | **Not offered.** Excluded by ADR 0014 and 0015. A separate proposal (DN-42), with its own security review                                                                              |

Visual model switching in a running session (FR-MOD-06) remains a **goal of the final product**.
SP1 is what is designed now. Whether SP4 or SP5 ever completes it is the owner's decision (DN-38,
DN-42). This document does not narrow it.

### 5.2 SP1 — Provider-native command

The session detail and the session's tab menu show, for agents with grade A or B evidence:

| Agent       | Shown                                                                                                      |
| ----------- | ---------------------------------------------------------------------------------------------------------- |
| Claude Code | "Type /model in the session to change it. Claude Code saves it as your default too." (MC5)                 |
| Hermes      | "Type /model in the session to change it for this session. Add --global to save it as your default." (MC5) |
| Codex CLI   | `switch_command_unknown` until OQ-51 raises the evidence above grade C                                     |
| OpenCode    | `switch_command_unknown`                                                                                   |

The hint is text. It sends nothing, copies nothing to the clipboard unless the user asks (05 §13),
and never focuses the terminal on its own.

### 5.3 SP2 — Start flag

| Element    | Contract                                                                                                                                                                                                                                                                        |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Input      | `overrides.model` and `overrides.effort` of the start request (05 §17.2), each one choice from `start_choices(agent, profile)`; **never free text** in the request                                                                                                              |
| Choices    | The managed catalogue does not apply to native agents. A native agent's choices are the `model` and `effort` values of the selected profile (09 §1) and its parent, which the user typed when saving the profile                                                                |
| Validation | When a profile is saved: at most 256 bytes; printable, without whitespace or control characters; not starting with `-`. It is opaque otherwise. When a start is planned: the agent and its version have the flag with grade A or B evidence (06 §6), or `start_flag_unverified` |
| Passing    | Exactly one argument after the documented flag, built by the start recipe (05 §17), never by string concatenation into a command line. Hermes gets `--model` only: a profile has no provider field (09 §1), so `--provider` is not passed                                       |
| Labelling  | The launcher and the session detail say "Wyss Console cannot check this name; <agent> reports it in the session if it does not know it" (`model_unchecked`). It is never shown as valid or available                                                                            |
| Effort     | As the model, for agents with a start flag for effort of grade A or B: Claude Code `--effort` (SRC-1). Others: not offered                                                                                                                                                      |
| Record     | MV-06 holds what was passed                                                                                                                                                                                                                                                     |
| Failure    | A start the provider rejects for the model ends as the provider says, in its own terminal; the registry records the exit (05 §7). The Console does not retry with another model                                                                                                 |

### 5.4 SP4 — What "never silent" requires, if DN-38 (b) is ever chosen

These are preconditions for a decision, not a design:

- Every time, one explicit user action.
- Text visible in the agent's input line before anything is sent.
- **Never** a newline or Enter from the Console.
- Only into a pane whose foreground process is the identified agent (05 §6.1).
- Never while a confirmation dialog is open.
- A record of the action in the native-session history.

The path from the page into a terminal is the one ADR 0030 says the native confirmation does not
contain (DN-22; OQ-36). That must be settled first.

### 5.5 SP5 — What a managed selection proposal must contain

To be written only if the owner wants it (DN-42):

- The exact fields admitted (`model`, `effort`) and on which method.
- Validation: the value must be an entry of the **current generation's** catalogue, compared byte
  for byte. No free text.
- What happens when the catalogue changes between plan and send.
- The lease scope it needs (ADR 0013).
- The event it writes.
- How it interacts with the authority-normalised presentation (ADR 0015 pins tool metadata per
  catalogue entry; a selected model must be one of those entries).
- An external security review.

**No automatic or cost-based selection or routing**, in either plane (§6).

## 6. Usage and cost

| Rule | Content                                                                                                                                                                                                                                                                  |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| UR1  | Only figures a provider reports are shown, labelled with the provider and the report's time (ADR 0008: "observe runtime-reported usage/timing; no cost tables, computed prices or trusted audit totals") `[R]`                                                           |
| UR2  | The Console computes no price. It holds no price table, fetches none, and converts no currency                                                                                                                                                                           |
| UR3  | A money figure the provider calls an estimate is shown as "<agent>'s estimate; not a bill". Claude Code's is "computed client-side at list price" and "may differ from your actual bill" (SRC-2). Hermes's comes from its own price table or a public list price (SRC-7) |
| UR4  | **Subscription access is not API billing.** A rate-limit window ("72 % of the 5-hour window used") is an allowance of a plan, not money. It is never converted to money, and a money estimate is never presented as what a subscription user pays                        |
| UR5  | Figures of different providers, planes or sessions are never summed into one total                                                                                                                                                                                       |
| UR6  | No automatic switching, routing, refusal or warning based on cost. A budget or alert is not designed and not approved                                                                                                                                                    |
| UR7  | Usage figures are not durable unless a decision says so. They are counters and identifiers, never prompt or output text                                                                                                                                                  |

What can be shown, by plane:

| Plane   | Today                                                         | Possible, not approved                                                                                                                                                                                                                                                                                            |
| ------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Managed | Nothing (`usage_not_reported`); the reports are dropped `[R]` | UC1: tokens per turn (`last`) and per session (`total`) from `thread/tokenUsage/updated`. UC2: rate-limit windows from `account/rateLimits/updated`, as percentages and reset times. Both are transient by default and need DN-43 (a change to the authority's projection and to the tolerated-notification list) |
| Native  | Nothing (`usage_not_reported`)                                | UC3: a provider's own usage command or file (`hermes usage --json`, `hermes insights`, Claude Code's status line). This runs a provider binary or adds a configuration entry, so it needs DN-13 or the user's own configuration. Not designed                                                                     |
| Either  | —                                                             | UC4: budgets and alerts. UC5: cost-based routing (**rejected** by UR6 unless the owner reopens it). UC6: a durable usage history                                                                                                                                                                                  |

FR-MOD-08 is met by UR1 to UR7 and by `usage_not_reported`. It does not require that a figure
exists.

## 7. Interface states and closed words

### 7.1 States the interface shows

Aligned with 13 S7.

| State                    | Pattern                                                     |
| ------------------------ | ----------------------------------------------------------- |
| Observed, session grain  | "Model: X (reported for this session)"                      |
| Observed, turn grain     | "This turn was moved from X to Y by Codex (reported)"       |
| Configured, not observed | "Configured: X. In use: not reported"                       |
| Selected at start        | "Started with: X. In use: not reported"                     |
| Listed                   | "Offered by this runtime; not a guarantee for your account" |
| Not in the catalogue     | "X (not in this runtime's list)"                            |
| Alias                    | "opus (alias; Claude Code decides which model it means)"    |
| Effective                | "Applies to new sessions, by <agent>'s documented order"    |

### 7.2 Closed words of IF-MODEL

Sentences are in 13 §9.5 (13 §9, rule 1).

| Word                       | Condition                                                              |
| -------------------------- | ---------------------------------------------------------------------- |
| `catalogue_unavailable`    | The managed runtime is not running, so it has no catalogue             |
| `catalogue_rejected`       | The runtime's list failed validation (`failed/catalog_rejected`) `[R]` |
| `catalogue_partial`        | The runtime has more entries than the authority asked for              |
| `catalogue_not_readable`   | The agent has no list the Console can read                             |
| `model_not_reported`       | No report of the model in use exists for this session                  |
| `model_not_in_catalogue`   | A configured, selected or observed model is not in the current list    |
| `model_rerouted`           | The provider reported that a turn ran on another model                 |
| `model_unchecked`          | A start choice that no catalogue confirms                              |
| `model_identifier_invalid` | A profile's model or effort value fails §5.3's validation              |
| `start_flag_unverified`    | The agent's version has no grade A or B evidence for the start flag    |
| `switch_command_unknown`   | No grade A or B evidence of an in-session switch command               |
| `default_change_blocked`   | SP3 is blocked                                                         |
| `scope_winner_unverified`  | The documented order is not verified for this version                  |
| `usage_not_reported`       | No usage figure is reported through a channel the Console reads        |

## 8. Tests and acceptance

Specified in [20 §6](20-VERIFICATION-AND-ACCEPTANCE.md) and §12.2: TS-MOD-01 to 16, AC-F6a-01 to
05, AC-F6b-01 to 04. The levels:

- **Managed:** synthetic App Server answers and notifications, built from SRC-3's schema, through
  the existing codec and safety classifier.
- **Native:** inventory fixtures (07) and launch-plan fixtures (05 §17).
- **Interface:** every state of §7.1 and every word of §7.2 rendered and never confused with
  another (TS-UX-01).

No test runs a provider.

## 9. The managed observation (DN-09)

The proposed [ADR 0031](../adr/0031-observed-model-of-managed-sessions.md) settles where the
observed model of a managed session lives:

- **Option A, transient attachment state:** recommended first.
- **Option B, a durable column:** rejected.
- **Option C, a versioned event:** for history, only with the owner's consent.

The ADR also settles:

- how an unknown or missing report is represented (`null`, never a default);
- how `model/rerouted` stops being a violation without weakening the classifier;
- what changes in the schema, storage and audit under each option.

It keeps SG-12 as written. It leaves managed model selection to DN-42. It is **Proposed**, not
accepted.

## 10. What this document does not establish

- That any provider behaves as its documentation says: nothing was run.
- That the grade C rows (Codex CLI in-session commands; OpenCode beyond `model`) hold.
- That Hermes's `main` documentation describes any installed version.
- That `model/rerouted` is ever sent to a client like the authority (OQ-50).
- Any price, bill or entitlement.
