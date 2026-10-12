# 12 — Model management

Domain H of [04](04-TARGET-ARCHITECTURE.md). Two phases that must not be merged: **seeing**
models (F6a) and **choosing** them (F6b). All `[P]` unless marked.

## 1. Five different facts about "the model"

| #   | Fact                            | Example                                      | Who knows it                   |
| --- | ------------------------------- | -------------------------------------------- | ------------------------------ |
| M1  | The provider                    | Codex, Claude Code                           | The session's adapter          |
| M2  | The catalogue a runtime offers  | Seven entries, one marked default            | The runtime, when asked        |
| M3  | The model configured at a scope | `model = …` in a settings file               | The provider's file            |
| M4  | The model chosen for a session  | A start flag, or a switch during the session | The provider, for that session |
| M5  | The model that answered a turn  | What the provider reports for that turn      | The provider, per turn         |

The interface never shows M3 or M4 as M5 (AD-16). A catalogue entry (M2) is never shown as
something the account may use: "listing is not entitlement" (ADR 0012; README `:663`) `[R]`.

## 2. What exists today

| Fact | Managed Codex                                                                                                                                                         | Native sessions                                              |
| ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| M1   | Shown ("Codex") `[R]`                                                                                                                                                 | Not shown: the Console does not know what runs in a terminal |
| M2   | The authority holds a bounded catalogue per generation and exposes `runtime/model/list` on the control socket; the window shows only its **count**                    | —                                                            |
| M3   | —                                                                                                                                                                     | In the provider's files; not read                            |
| M4   | The managed runtime selects nothing: no client can supply a model (SG-12)                                                                                             | —                                                            |
| M5   | The adapter normalises an observed model from the thread's start or resume answer (`adapters/codex/normalize.rs:197-211`), and it is dropped before the control plane | —                                                            |

Evidence `[R]`: `control/runtime.rs:35`, `:136` (catalogue, count); `authority/view.rs:33`,
`:98` (the count is all that crosses); the developer client's `runtime/model/list`
(`examples/control_client.rs`). The catalogue carries identifier, display name, a default flag
and reasoning-effort metadata (`docs/research/agent-runtime-surfaces-2026-10-06.md`).

## 3. Per provider

| Topic                 | Claude Code                                                                                          | Codex                                                    | OpenCode      | Hermes          |
| --------------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------- | ------------- | --------------- |
| Configured model (M3) | `model` in settings `[D]`                                                                            | `model` in `config.toml` `[D]`                           | `model` `[D]` | `model.*` `[D]` |
| Effort or reasoning   | `effortLevel`, `modelSettings` `[D]`                                                                 | `model_reasoning_effort` `[D]`; per-model metadata `[R]` | `[N]`         | `[N]`           |
| Choose at start (M4)  | `--model` `[O]`                                                                                      | `-m` `[O]`; a profile `[D]`                              | `[N]`         | `[N]`           |
| Switch while running  | `/model`, typed by the user `[D]`; settings changes are not reloaded for the model `[D]`             | `[N]`                                                    | `[N]`         | `[N]`           |
| Catalogue (M2)        | `[N]` for a programmatic list                                                                        | `model/list` on the App Server `[R]`                     | `[N]`         | `[N]`           |
| Model of a turn (M5)  | Hook events `PreModelSwitch` and `PostModelSwitch` exist `[D]`; no report read without adding a hook | Thread-level report `[R]`; per-turn report `[N]` (OQ-10) | `[N]`         | `[N]`           |
| Usage and cost        | `[N]`                                                                                                | `[N]` for what the App Server reports per turn           | `[N]`         | `[N]`           |

## 4. F6a — Seeing

| Element                      | Managed Codex                                                                                                                                                                                                 | Native session                                                                                     |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Catalogue                    | The existing `runtime/model/list`, paged, shown in the Agents view with the default marked and "listed, not guaranteed for your account"                                                                      | "Not available" unless the provider has a supported list                                           |
| Configured model             | Not applicable (the managed runtime takes none)                                                                                                                                                               | From the configuration inventory (07): value per scope, the documented winner marked               |
| Model of the session or turn | The observed model, carried to the window. **Needs an ADR**: a new field in the authority's projection (ADR 0019's `AuthorityView` or the session page) and whether it is kept durably or only while attached | "Configured: X. In use: not reported"                                                              |
| Effort                       | From the catalogue's metadata                                                                                                                                                                                 | From the inventory                                                                                 |
| History of changes           | An event when the observed model of a session changes, if the ADR admits one                                                                                                                                  | Configuration changes are in the configuration history; switches typed in the session are not seen |

The ADR for the managed part (DN-09) has to settle: where the observed model lives (transient
attachment state, like the mode, or a durable fact of a turn); whether it enters the event
vocabulary (schema and vocabulary version, ADR 0017); how an unknown or missing report is
represented (never as the default).

For native sessions the Console does not install a hook to learn the model. That would change
the configuration it reports on (05 §6). If the user wants that observation he adds the hook
himself, through the configuration engine, as his own decision.

## 5. F6b — Choosing

| Action                      | Native                                                                                          | Managed                                                                                                                                            |
| --------------------------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| Model for a new session     | A field of the session profile, applied by the provider's start flag (09 §3)                    | Would need the authority to accept a model in a session's activation: a change to SG-12's surface, by its own ADR, validated against the catalogue |
| Change the default          | A configuration change to the provider's file, through the engine (07 §5); affects new sessions | Not applicable                                                                                                                                     |
| Switch in a running session | The user types the provider's own command in the terminal. The Console does not type for him    | Not designed                                                                                                                                       |
| Effort                      | As the model                                                                                    | As the model                                                                                                                                       |

Model selection is never a preference of the Console (ADR 0026: a preference decides nothing)
`[R]`.

## 6. Cost and usage

Shown only where the provider reports figures for the turn or session, labelled as the
provider's, never computed from a price table (ADR 0008 `:92`: "no cost tables, computed prices
or trusted audit totals") `[R]`. No figure exists to show today `[N]`.

## 7. States the interface must be able to show

| State                      | Text pattern                                                                     |
| -------------------------- | -------------------------------------------------------------------------------- |
| Reported by the provider   | "Model: X (reported for this turn)"                                              |
| Configured, not observed   | "Configured: X. In use: not reported"                                            |
| Listed in the catalogue    | "Offered by this runtime; not a guarantee for your account"                      |
| Unknown identifier         | "X (not in this runtime's catalogue)"                                            |
| Catalogue unavailable      | "The runtime is not running" or "This provider has no list the Console can read" |
| Changed during the session | "Changed from X to Y at …", when reported                                        |

## 8. Testing

Managed: fixtures of thread answers with a model, without one and with an unknown one; the
projection carries exactly what was reported. Native: inventory fixtures with a model at two
scopes. Interface: none of the states of §7 is ever rendered as another.
