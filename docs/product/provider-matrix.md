# Claude Code and Codex capability matrix

> **On-demand capability reference**, pinned to the Phase1 baseline below. Status cells
> are that dated snapshot; update current implementation/evidence only in the
> [canonical status register](implementation-status.md).

Baseline `e480d479`, reviewed 2026-10-22. `I` = implemented with the cited real/production
observation for that slice; `P` = implemented but only partially verified; `D` = designed,
not implemented; `B` = blocked on evidence/security; `U` = integration behaviour undecided.
These labels describe **Wyss integration**, not whether the provider has the feature.
See [implementation status](implementation-status.md) for precise acceptance limits.

## 1. Product integration, separated by mode

| Capability                                | Claude Code Native                                               | Codex Native                                              | Claude Code Managed                          | Codex Managed                                                           |
| ----------------------------------------- | ---------------------------------------------------------------- | --------------------------------------------------------- | -------------------------------------------- | ----------------------------------------------------------------------- |
| Start own CLI, terminal input/resize/exit | I: F3, 2.1.303                                                   | I: F3, 0.159.0                                            | D/B: no execution adapter                    | Not a terminal CLI; I: App Server 0.159.0                               |
| Original provider features/sign-in        | Native path; P for complete preservation matrix                  | Native path; P for complete preservation matrix           | B: version/auth/settings/tool proof required | Restricted accepted profile, not full Native feature set                |
| Inference                                 | Native provider operation; no blanket model-run acceptance       | Same                                                      | D/B: no attested no-tool profile             | I baseline; F8 production observation specifically Local Tools          |
| Commands/file writes                      | Provider's permissions, full account                             | Provider's permissions, full account                      | D/B: mediate all effect paths first          | I profile; per-action authority approval, no network                    |
| Per-session runtime/generation            | Native host-owned session identity                               | Same                                                      | D: candidate common contract                 | I: F8 P2/V8; runtime cap 1                                              |
| Concurrent agents/turns                   | I: independent native sessions                                   | I: independent native sessions                            | B: provider-specific evidence                | B above1; no release concurrency proof                                  |
| Conversation resume                       | Provider CLI; P Wyss resume UX                                   | Provider CLI; P Wyss resume UX                            | D: structured integration required           | I: explicit bind/resume, no replay after restart                        |
| Model/effort at start                     | P: model/effort profile flags; evidence 2.1.304                  | P: `-m`/`-c` override; evidence 0.159.0                   | D/B: catalogue/selection contract            | B: ADR0014/0015; no P2 request fields                                   |
| Model change while running                | Own `/model`; P GUI guidance only                                | Own CLI capability unverified in Wyss                     | D/B                                          | B pending separate authority extension                                  |
| Actual model/usage                        | Unknown unless provider-reported; no terminal scraping claim     | Same                                                      | D                                            | P: reported thread model/reroute schema; real reroute NOT_PROVEN        |
| Global/project configuration              | I/P: scoped authoring, consumption not generally proven          | I/P: scoped authoring; native trust respected             | D/B: effective-setting isolation not proven  | Fixed isolated profile; not the user's editable native config           |
| Session profiles                          | I/P: supported overlays, not arbitrary independent config copies | I/P: supported flags; hooks/skills exclusions unsupported | D                                            | Fixed security profile; generic native profile cannot relax it          |
| Rules/skills/hooks/MCP/plugins            | Own CLI features; GUI operations per row below                   | Own CLI features; GUI operations per row below            | B until separately profiled and mediated     | Disabled/denied in current Managed profile; not parity delivered        |
| Provider subagents                        | Native provider feature; not Wyss OL3                            | Same                                                      | B                                            | Denied in current profile                                               |
| Manual handoff brief                      | I/P: reviewed new Native session                                 | I/P: reviewed new Native session                          | D                                            | D: final handoff extension, no authority transfer                       |
| OL3 coordinated pipeline                  | B for automatic unrestricted Native dispatch                     | B for same trust reason                                   | D/B                                          | D/B: no coordinator; cap 1 does not prevent design of serial scheduling |
| Remote access/Android                     | D/B: host/gateway/client work remains                            | Same                                                      | D/B                                          | D/B: foundational gateway != remote approval service                    |

The existing Native catalogue has exactly ClaudeCode and Codex (`native/catalogue.rs`).
OpenCode's configuration adapter does not imply it is a launcher entry. Hermes is a
retained future adapter/integration, not an installed capability verified in this phase.

## 2. Configuration/observation capability details

| Feature                    | Claude Code integration at baseline                                        | Codex integration at baseline                                             | Remaining acceptance/design                                                                      |
| -------------------------- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Read real scopes           | `agentconfig/claude.rs`, inventory/redaction                               | `agentconfig/codex.rs`, inventory/redaction                               | Full provider-resolved/session-effective proof per item remains partial                          |
| Individual hooks           | Sealed removal/restore, author/edit/delete via native dialog; ADR0034/0042 | Inventory; individual mutation blocked by G-INF/trust policy              | Never write provider trust records; qualify new version/event/handler separately                 |
| Skills/instructions        | Switches, native Markdown create/edit/delete, exact Undo, sealed templates | Skill switches; account/project AGENTS authoring, account skill authoring | Codex project skill authoring not implemented under0050; generic provider consumption NOT_PROVEN |
| MCP                        | Inventory; not every provider-owned MCP form is editable                   | TOML author/add/edit/delete/switch and encrypted templates                | Unknown/secret-bearing forms unavailable; no connection is made by inventory                     |
| Hook/MCP templates         | Export/import/decrypt/prefill/cancel observed                              | Same for supported MCP forms                                              | Confirmed imported-template application NOT_RUN; execution/consumption NOT_PROVEN                |
| Model/effort               | Parsed config; profile request; CLI-parse evidence for effort              | Config/profile override; read-only resolved effort evidence               | Requested/configured/reported facts distinct; no implied model entitlement                       |
| Native runtime observation | Process/resources/profile metadata                                         | Same                                                                      | No promise to infer model/internal activity by reading terminal output                           |
| Managed observation        | No adapter                                                                 | Thread-reported model; exact reroute notification schema handled          | Real reroute, per-turn observations and durable metrics not generally proven                     |

## 3. Official surfaces and feasibility evidence

Official pages fetched on 2026-10-22 supplement repository evidence. They describe current
upstream surfaces, **not retroactive certification of pinned installed builds**. URLs may
redirect; a future adapter must pin the exact provider/SDK/schema and record a matching
manifest, tests and reviewed authentication route. No provider binary/account was used here.

| Source                                                                                                        | Supported conclusion                                                      | Not established                                                                       |
| ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| [OpenAI App Server](https://developers.openai.com/codex/app-server)                                           | A structured client protocol exposes threads, turns, events and approvals | That Wyss accepts every current method or that 0.159.0 has every latest feature       |
| [Codex security](https://developers.openai.com/codex/security)                                                | Provider sandbox/approval controls exist                                  | Wyss same-UID approval authenticity or a new Managed capability profile               |
| [Codex skills](https://developers.openai.com/codex/skills) and [MCP](https://developers.openai.com/codex/mcp) | Native extensibility surfaces exist                                       | Safe use inside current no-MCP/no-subagent Managed profiles                           |
| [Claude programmatic use](https://code.claude.com/docs/en/headless)                                           | CLI print mode and Agent SDK are official programmatic entry points       | Print mode alone is a Wyss Managed adapter                                            |
| [Claude SDK permissions](https://code.claude.com/docs/en/agent-sdk/permissions)                               | Permission modes/callbacks are available integration surfaces             | Every tool/effect passes one callback, no pre-callback bypass, or process containment |
| [Claude permissions](https://code.claude.com/docs/en/permissions)                                             | Provider permissions and sandboxing have separate roles                   | Equivalence with Wyss lease/action mediation                                          |
| [Claude model configuration](https://code.claude.com/docs/en/model-config)                                    | Provider-native model/effort selection exists                             | A specific account/model is entitled or a requested model answered a particular turn  |
| [Claude hooks](https://code.claude.com/docs/en/hooks)                                                         | Hooks are executable/customizable provider behaviour                      | Auto-enabling them in Managed preserves existing no-external-effects guarantees       |

Recommended Claude Managed feasibility route: evaluate a pinned official Agent SDK in an
owned supervised process, with settings/auth preservation and complete tool mediation as
separate proofs. Compare bounded CLI streaming if SDK dependency cost is excessive.
Do not use terminal scraping, auto-answer native prompts, copy authentication stores or
enable bypass permission modes to obtain integration. This recommendation is an inference
from documented surfaces; feasibility and final adapter transport remain unproven.

## 4. Capability qualification checklist for implementation

Each provider/mode/profile/capability release row must include exact version/schema,
configuration/auth route, positive workflow, adversarial denial, restart/cancellation,
resource results, provider consumption observation where relevant, independent review
and accepted ADR for a security change. Include the limits and evidence paths.

Native fidelity checks must exercise the features the owner actually uses, including
hooks, skills, MCP, original commands and sign-in continuity, without silently replacing
real global/project settings. Managed checks must prove absence of every unsupported
effect route, including provider subagents, hooks, plugins, MCP, background tools and
network. A feature unavailable safely in Managed remains visible with reason and a Native
alternative requiring a **new explicit launch**, never automatic fallback.
