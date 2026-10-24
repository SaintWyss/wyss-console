# Claude Code and Codex capability matrix

> **On-demand capability reference**, pinned to the Phase1 baseline below. Status cells
> are that dated snapshot; the M0 manifest below adds scoped installed-build observations.
> Update current implementation/evidence only in the
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

## M0 installed-build manifest

Dated observation 2026-10-22, source main `d2ee8d2` (application trees equal `e480d479`).
This augments the historical matrix, not its acceptance scope. Probes used only PATH
`claude --version`, `codex --version`, existing host hello and existing F8 authority/hello;
no help/model-list refresh, sign-in, auth/config reads, inference or paid calls.

| Provider / mode / profile                      | Installed observation / compatibility                                                                    | Evidence, test and remaining qualification                                                                                                                                                                                                                                                                              |
| ---------------------------------------------- | -------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude Native / original CLI + Native profiles | PATH2.1.309; host catalogue supports Claude. No exact2.1.309 original-feature qualification              | F3 real2.1.303 launch; F6 parse2.1.304 flags. New version probe proves version only. Hooks/skills/MCP/sign-in/profile/model/effort consumption fidelity on2.1.309 remains partial/NOT_PROVEN; M2 positive original workflow + denied unsupported override + restart/continuity matrix                                   |
| Codex Native / original CLI + Native profiles  | PATH0.159.0, matches recorded launch/config evidence                                                     | F3/F6; model/config override partial, original commands/auth/hook/skill/MCP use remains provider-owned. Per-session Codex hooks-off/skills exclusions unsupported. M2 must test supported profile flags without editing original settings                                                                               |
| Codex Managed / inference_only_v1              | P1/V7 separately active; P2 supports this attested profile, real F8 observation specifically Local Tools | Existing ADR0014/0015 and protocol fixtures; no claim that M0 ran an inference turn, refreshed auth or resolved G-INF. Real per-profile/version consumption/refresh/error matrix remains required                                                                                                                       |
| Codex Managed / codex_local_tools_v1           | P2/V8 active, service0.2.0; production adapter pins0.159.0, cap1                                         | Recorded real0.159.0 F8 activation + accepted ADR0051; M0 hello confirms2/8, registry/engine/transport fixtures confirm narrow contracts. No new runtime launch, real overload, refresh, reroute or full adversarial provider campaign. Model/effort selection fields remain absent; hooks/MCP/plugins/subagents denied |
| Claude Managed / any profile                   | No released adapter/compatible build                                                                     | ADR0053 Proposed; unsupported. M3 pins official surface/auth/settings route and mediates every effect before even no-tool acceptance; Native is a separately explicit alternative                                                                                                                                       |
| OpenCode/Hermes                                | No Native catalogue entry/Managed execution adapter qualified                                            | Retained config/extension work only; not another installed provider qualification or M1 dependency                                                                                                                                                                                                                      |

## M2 Native profile evidence — 2026-10-23

Current PATH observations are Claude Code `2.1.309 (Claude Code)` and Codex
`codex-cli 0.159.0`. Read-only `--help` output confirms Claude's `--model`, `--effort`,
`--permission-mode` and `--settings` session interfaces, and Codex's `-m`, `-c
KEY=VALUE`, `-s` and `-a` interfaces. The exact-build Native profile manifest and its
limits are in [`native/capabilities.rs`](../../src-tauri/src/native/capabilities.rs)
and the [M2 acceptance record](../acceptance/m2-engineering-2026-10-23.md).

These probes establish installed version and accepted command-line surface only.
Claude settings-overlay behavior was exercised on 2.1.303 in the cited F1/F2a and F3
records; Codex `model_reasoning_effort` was read from the 0.159.0 config surface in F6.
Neither record proves that a Native turn on the current exact build resolved, was
entitled to, or effectively used the requested model/effort. No prompt, sign-in,
credential/config mutation or paid inference was performed for this M2 record.

The updated Native host binds profile previews to the resolved plan and checks the
shell-resolved CLI build before passing profile values. A legacy host remains able to
start non-effort profiles with a visible warning, but cannot bind the displayed plan
revision; effectful M2 acceptance therefore requires the updated host. Unsupported
Codex per-session hooks/skills exclusions, Codex project skills, Codex hook mutation,
and project trust writes remain refused. Claude Managed, Managed model/effort
selection, and authority extensions remain M3 work.

Host hello0.2.0 exposes `profile_effort`; this changes the old-host compatibility
observation, not the recorded limits of real GUI/provider consumption. The host's protocol1
is its **terminal transport**, not proof that the selected Managed authority is P1.
Installed GUI/authority binary fingerprints and remaining source correspondence are in
[status](implementation-status.md#m0-engineering-baseline). Unknown/new build => Native
inspection may remain available with unqualified label; Managed attestation must refuse
an unsupported exact build, never fall back automatically or reuse Native trust.

### G-INF carry-forward: E1–E9 scope

[Historical E1–E8](../research/inference-isolation-2026-10-19.md) used procedures now
prohibited for real acceptance; retain observations, do not rerun disposable HOME/keyring
experiments. E1 user-hook hiding, E2 policy pin visibility, E4 project trust, E8 visible
policy/namespace had bounded controls; E3 synthetic link write was **not real refresh**;
E5 full isolated authority attestation and E6 turn/resume were NOT_RUN; E7 old thread-store
resume needs an explicit compatibility plan. F8 Local Tools acceptance does not close these.

E9 is a **required future matrix, NOT_RUN**, not an existing experiment result:

| Source / behaviour               | Must establish before G-INF/native-hooks coexistence release                                                                                                               |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Account and project config/trust | Isolated inference cannot inherit external hooks/tools; original Native settings remain intact; effective values, not only discovery counts                                |
| Administrator/managed policy     | Host policy remains visible/authoritative; pinned-on features and executable definitions distinguished; fail closed on incompatible mandated effect                        |
| Plugin/skill/subagent surfaces   | Enumerate inherited/generated actions and deny unsupported paths before dispatch; no callback-only isolation claim                                                         |
| MCP account/project/policy       | Enumerate servers/tools/transport and policy precedence; no inherited connection or executable server merely because hooks list is empty                                   |
| Auth and old bindings            | Supported credential route, real refresh/multi-process safety and resume/old binding behaviour with explicit authorization; no credential copying or shared-store shortcut |

M0 carries this matrix without new experiments. M2/M3 own the unlocking evidence; M1
project registration does not enable hooks, trust a project or weaken Managed attestation.
