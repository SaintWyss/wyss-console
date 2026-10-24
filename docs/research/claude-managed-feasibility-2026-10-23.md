# Claude Managed feasibility — 2026-10-23

**Scope of the first pass:** read-only feasibility; no SDK installation, provider operation,
credential/config inspection, sign-in, provider turn or state mutation. A separate
read-only local `--version` check was run after the research pass. **Repository baseline:**
`def69fe35ddfe8b696b65a12d2f2babe296c8247`. **Verdict of that pass:** official local SDK is a
viable candidate, but neither Claude Inference nor Local Tools is qualified for Wyss today.

> **Update 2026-10-23 (implementation pass).** The experiments below were then run for real, offline,
> against the pinned SDK and CLI; see [Results of E1–E9](#results-of-e1e9-offline-pinned-build). The
> verdict changed from "feasible candidate" to "implemented and tested offline, unqualified against
> the real provider". Nothing in this document is provider evidence.

## Candidate pin and process model

The current official Python release is `claude-agent-sdk==0.2.173`, released 2026-10-22,
and bundles Claude Code CLI `2.1.309`. The current official TypeScript release is
`@anthropic-ai/claude-agent-sdk@0.3.308` and announces CLI parity with `2.1.308`.
These are package publication facts only, not Wyss compatibility evidence.

The Agent SDK runs Claude Code as a subprocess and provides its agent loop, built-in
tools, permissions, hooks and sessions. A Wyss adapter must supervise the full child tree,
not treat the Python/TypeScript SDK object as the provider process. Python's published
manifest requires Python >=3.10 and includes `anyio`, `sniffio`, `mcp` and `jsonschema`;
dependency footprint, distribution method, process cleanup and reference CachyOS cost are
not measured here. Do not add either SDK to production dependencies until reviewed.

Sources checked on 2026-10-23:

- [Official Python SDK v0.2.173 release](https://github.com/anthropics/claude-agent-sdk-python/releases/tag/v0.2.173)
- [Official Python SDK v0.2.173 manifest](https://github.com/anthropics/claude-agent-sdk-python/blob/v0.2.173/pyproject.toml)
- [Official TypeScript SDK v0.3.308 release](https://github.com/anthropics/claude-agent-sdk-typescript/releases/tag/v0.3.308)
- [Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview)
- [Agent SDK configuration](https://code.claude.com/docs/en/agent-sdk/configuration)
- [Agent SDK permission evaluation](https://code.claude.com/docs/en/agent-sdk/permissions)
- [Agent SDK hooks](https://code.claude.com/docs/en/agent-sdk/hooks)
- [Agent SDK MCP](https://code.claude.com/docs/en/agent-sdk/mcp)
- [Agent SDK plugins](https://code.claude.com/docs/en/agent-sdk/plugins)
- [Agent SDK sessions](https://code.claude.com/docs/en/agent-sdk/sessions)
- [Agent SDK usage and cost reporting](https://code.claude.com/docs/en/agent-sdk/cost-tracking)
- [Official Anthropic authentication](https://code.claude.com/docs/en/agent-sdk/quickstart#authentication)

## Documented behavior and Wyss implications

| Area                        | Officially documented                                                                                                                                                                                                                  | Current conclusion                                                                                                                                                                                      |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Process                     | Agent SDK operates Claude Code as a local subprocess; SDK is intended for an application operating the process                                                                                                                         | Candidate boundary only. Wyss ownership/reaping, startup, exact child identity and capacity integration remain untested.                                                                                |
| Authentication              | Official quickstart documents API-key and cloud-provider credential routes. The overview says third-party developers cannot offer claude.ai login/rate limits without prior approval. SDK consumes environment/configured credentials. | No accepted Wyss route for auth, refresh, quota or credential injection. Do not copy Native credentials, inspect values or assume shared login. Santiago must decide supported route.                   |
| No-tool Inference           | `disallowed_tools: ["*"]` removes every tool definition. `setting_sources: []` disables user/project/local settings loading.                                                                                                           | Makes no-tool mode plausible, but not attested. Managed policy, `~/.claude.json`, connectors, auto-memory, hooks and other effects require exact-version inventory/experiments.                         |
| Permission callback         | Rules/modes/hooks can resolve actions before `canUseTool`; `allowed_tools` is not a complete allowlist; `bypassPermissions` approves broadly; hooks run before the later rule/callback stages.                                         | Callback-only Local Tools mediation is insufficient.                                                                                                                                                    |
| Hooks/MCP/plugins/subagents | SDK explicitly exposes these capabilities. Session-start hooks may act before first model response; user/project/local source selection does not alone prove managed policy and connector state are absent.                            | Enumerate and deny/exclude every independent effect. Unknown policy/capability means unavailable.                                                                                                       |
| Local Tools candidate       | A custom authority-owned tool could hold a complete action and return an authority result, while provider built-in effectful tools are removed.                                                                                        | This is a design hypothesis only. Prove there is no provider-local execution path, hook, plugin, connector, MCP server or subagent escape. Exact tool call must be fenced and admitted before dispatch. |
| Streaming                   | SDK offers typed streaming input/output and incremental events. Buffer/backpressure defaults are SDK details, not Wyss guarantees.                                                                                                     | Need strict frame/event parsing, fixed queue/size/time limits and malformed/unknown fail-closed fixtures.                                                                                               |
| Cancellation/resume         | Streaming client supports interrupt and sessions can resume by provider session ID. Session storage is local unless a store is explicitly configured; after interrupt, buffered messages can remain.                                   | Wyss operation receipts remain authoritative. On crash or ambiguous acceptance, mark indeterminate and never replay prompt/approval. No automatic Native session migration.                             |
| Models/effort               | SDK accepts model/effort options, offers fallback-model configuration, and exposes reported model/usage details. Effort support varies; no reliable documented observation of effective effort was identified.                         | No fallback/substitution without accepted policy. Offered catalogue is not account entitlement; effort is unknown unless provider reports it.                                                           |
| Usage                       | SDK reports token usage at step/result levels and cost figures; docs direct authoritative billing to Usage & Cost API/Console and warn against billing decisions from SDK cost.                                                        | Display provider-reported counts with provenance/completeness. Never call SDK estimates subscription quota or authoritative charges.                                                                    |
| Existing architecture       | F8 P2/V8 is active for Codex; F8 model DTOs, profile enums and production spawner are Codex-shaped; global runtime capacity is one.                                                                                                    | Add provider-specific adapter contracts, preserve Codex strictness and cap 1. Existing Codex evidence does not qualify Claude.                                                                          |

## E1–E9 carry-forward and required experiments

Historical E1–E8 in `inference-isolation-2026-10-19.md` are Codex observations, not
Claude evidence. Do not repeat the repository-prohibited disposable HOME/keyring provider
experiments. E9 is required and **NOT_RUN**. The following exact matrix must be completed
with a version-pinned isolated fixture first; real-provider portions require Santiago's
separate authorization.

| Experiment                       | Positive evidence required                                                                                                                     | Negative control / refusal oracle                                               | Current status                                      |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | --------------------------------------------------- |
| E1 user/project/local hooks      | Effective settings and hook inventory match the qualified profile                                                                              | Inject hook in each source; no unapproved hook starts or changes state          | NOT_RUN for Claude                                  |
| E2 system/managed policy         | Effective mandated policy is visible and reported with source/revision                                                                         | Unknown/conflicting pinned executable capability blocks profile                 | NOT_RUN for Claude                                  |
| E3 authentication/refresh        | Approved API/cloud-provider credential route and supported refresh semantics                                                                   | Expired/malformed credential fails closed; no copied Native secret/store        | NOT_RUN                                             |
| E4 project trust/config          | Project-specific source and trust are visible without modifying Native configuration                                                           | Untrusted/wrong project cannot inherit another project's settings or permission | NOT_RUN for Claude                                  |
| E5 complete attestation          | Full authority preflight covers executable/version, config sources, tools, hooks, MCP, plugins, subagents, connectors, identity and child tree | Omit/forge one attestation item; runtime never reaches Available/Ready          | NOT_RUN                                             |
| E6 turn/resume                   | Explicitly admitted turn streams once; a user-selected resume is bound to same session/runtime generation                                      | Crash/ambiguous acceptance never replays prompt or old approval                 | NOT_RUN; provider turn needs separate authorization |
| E7 prior session bindings        | Clear compatibility policy for Native/old SDK session IDs                                                                                      | No automatic resume, transcript migration or shared-store shortcut              | NOT_RUN                                             |
| E8 namespace/policy              | Isolation and visible system policy coexist in the exact supervised process                                                                    | Hide/mismatch policy or namespace invalidates attestation                       | NOT_RUN for Claude                                  |
| E9 policy/auth/plugin precedence | Account/project/admin policy, plugins/skills/subagents, MCP/connectors, auth/refresh and old binding behavior inventoried                      | Any hidden or unmediated effect makes corresponding profile unavailable         | NOT_RUN                                             |

## Qualification decision

**Inference:** BLOCKED / NOT_PROVEN. The documented all-tools deny is a promising control,
not proof of no executable effects. Qualify only after exact-build effective policy and
hook/MCP/connector/auto-memory inventory plus negative controls establish no non-model
effect, and the supported auth/refresh route is accepted.

**Local Tools:** BLOCKED / NOT_PROVEN. A callback alone cannot hold every provider action.
The only candidate worth testing is an adapter that removes effectful built-ins and exposes
only authority-owned action tools. This remains blocked until every action is complete,
held for Wyss admission and dispatched at most once, with all bypass paths experimentally
excluded.

**No Claude adapter or provider state changed.** No SDK was installed. On 2026-10-23, read-only
`claude --version` returned `2.1.309 (Claude Code)` and `codex --version` returned
`codex-cli 0.159.0`; no CLI `--help`, credential access, account sign-in, turn, refresh,
configuration read/write or performance run occurred. Version output establishes binary
identity only, not account entitlement, authentication, Managed compatibility or safety.

## Proposed ADR0053 acceptance text (owner review required)

> **Status: Accepted only for the exact qualified adapter/profile set recorded by version,
> host platform, authentication route and effective configuration inventory. This decision
> is not a guarantee for an SDK family or later provider build.** The authority owns the
> supervised process tree and preserves F8's global runtime capacity of one, per-session
> identities, generation fences, leases, durable receipts and indeterminate recovery.
>
> **Inference is Available only** when the exact runtime build has no provider-executable
> tools and effective configuration proves no unapproved hooks, MCP servers/connectors,
> plugins, subagents, auto-memory effects or other executable capabilities. Managed policy
> remains authoritative; unknown or incompatible policy leaves the profile unavailable.
> No Native configuration or credentials are copied, rewritten or implicitly reused.
>
> **Local Tools remains Blocked** until provider-specific adversarial evidence proves every
> command/file action is complete, bounded, held for exact authority admission, bound to
> generation/turn/lease and dispatched at most once. `canUseTool` alone is not mediation.
> Any bypass, truncation, unknown event, unsupported version or ambiguous effect fails
> closed. No model fallback/substitution is enabled; effort remains unknown unless reported.
> SDK cost estimates are not billing authority.
>
> Release requires authorized real-provider compatibility evidence, independent security
> review, measured resource bounds and Santiago's explicit acceptance. Rollback disables
> new Claude admissions without altering Native sessions, Codex Managed state or existing
> history.

This text is a proposal only. Only Santiago can accept ADR0053.

## Results of E1–E9 (offline, pinned build)

**Build under test:** Python `claude-agent-sdk==0.2.173` with its bundled Claude Code `2.1.309`
(bundled binary sha256 `4d704a8aecbe038affa3000f498ae17c1b4f753add8facb8764120affc99b658`, identical to
the installed `claude`). The TypeScript SDK `0.3.308` (CLI `2.1.308`) was evaluated on paper only; Python
was chosen because its closure is 30 hash-pinned wheels (the TypeScript tree had 105 packages) and its CLI
equals the installed one.

**Method.** Every experiment ran the real SDK and the real CLI inside `bwrap --unshare-all` with a
disposable HOME, a synthetic credential and a mock Messages API on loopback. **The ground truth is the
mock's log of the request bodies the CLI actually sent** (tool definitions, system prompt, model, effort),
never what the SDK says about itself. No network, no real credential, no Native configuration and no
provider account was involved. The probes are in `src-tauri/claude-bridge/probe/`; the captured frames that
the tests replay are in `src-tauri/claude-bridge/fixtures/`.

| Experiment                       | Result on the pinned build                                                                                                                                                                                                                                                                                                                        | Consequence in the adapter                                                                                                                                                                        |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| E1 user/project/local hooks      | `disallowed_tools=["*"]` alone does **not** stop user/project/local hooks, MCP launch, `apiKeyHelper` or memory loading; they all ran. `tools=[]` with `setting_sources=[]` does stop them.                                                                                                                                                       | The frozen option set (`profiles.py`) uses both, plus `--bare`, `--safe-mode`, no plugins/skills. Its digest is pinned in Rust (`adapters/claude/pins.rs`).                                       |
| E2 system/managed policy         | `/etc/claude-code/managed-settings.json` and `managed-mcp.json` still apply after the above, and `--strict-mcp-config` fails when an enterprise MCP list exists.                                                                                                                                                                                  | The installation check refuses to start when a managed policy exists (`launch.rs`). It is never overridden or edited.                                                                             |
| E3 authentication/refresh        | Only a synthetic key was used. The CLI retries 401/529 without a bound.                                                                                                                                                                                                                                                                           | The CLI environment holds only a synthetic key; the loopback tap swaps in the real one. The authority cuts a failing turn off after 3 retries. **The real auth route is not exercised: ADR0053.** |
| E4 project trust/config          | With `setting_sources=[]` project settings are not read. A prompt containing `@/path` makes the CLI read that local file with **no tool involved**.                                                                                                                                                                                               | `verbatim_prompts=True` closes `@path` expansion (tested with a hostile prompt).                                                                                                                  |
| E5 complete attestation          | Three independent legs agree or the runtime is refused: the `init` inventory of the turn, the pre-turn `get_server_info` / `get_mcp_status` / `get_context_usage` answers, and the request bodies seen by the loopback tap. About 50 `system` message subtypes exist; unknown ones fail closed.                                                   | `policy.rs` (`verify_ready`, `verify_turn_started`, `verify_tap`, `classify_notice`).                                                                                                             |
| E6 turn/resume                   | An authority-minted `session_id` is honoured; resuming in a new process puts the earlier turn's content into the second request; a prompt is never replayed. `DeferredToolPlaceholder` is inert.                                                                                                                                                  | Operation receipts stay authoritative; an ambiguous dispatch is indeterminate and never re-sent.                                                                                                  |
| E7 prior session bindings        | Native sessions are not visible to a Wyss-owned home.                                                                                                                                                                                                                                                                                             | Provider homes live under Wyss state, keyed by managed session; no Native session is read or migrated.                                                                                            |
| E8 namespace/policy              | The namespace and the visible policy coexist in the supervised tree; a hidden/mismatched policy invalidates attestation.                                                                                                                                                                                                                          | `bwrap --unshare-pid --die-with-parent`; stop reaps the whole tree (0 left in 7 measured runs).                                                                                                   |
| E9 policy/auth/plugin precedence | Plugins, skills, slash commands, subagents, MCP and connectors are all inventoried and closed by flags; effort can be set **only** when the CLI starts (`--effort`), the model can change per turn (`set_model`). `ResultMessage.modelUsage` is keyed by the _requested_ model, so only `AssistantMessage.model` is evidence of the model in use. | Model "reported in use" comes from the provider message; a different echoed model is a reroute, never a silent substitution.                                                                      |

**What these results do not show.** They do not show that a real account, entitlement or refresh
route works, that a future CLI keeps these properties, or that the 50 observed notice subtypes are the
complete set a real provider can produce. That is the real-provider evidence the owner has to authorize.

## Qualification decision (updated)

**Inference:** `IMPLEMENTED_BUT_UNQUALIFIED`. Offline, with the exact build, the model is offered no
tools in every request (checked in 3 places), no hook/MCP/plugin/subagent/memory effect ran against
hostile configuration, and nothing outside the workspace changed. Qualification needs the supported auth
route (ADR0053) and a real-provider run the owner authorizes.

**Local Tools:** `IMPLEMENTED_BUT_UNQUALIFIED`. The only tools the model is offered are the two
authority-owned proposal tools; a proposal blocks until the authority has decided, and only the authority's
own executor (bwrap, no network, expected-old-hash atomic file writes) ever has an effect. A model
that demands a built-in tool gets nothing and the workspace is untouched (test
`a_model_demanding_a_builtin_tool_gets_nothing_and_the_workspace_is_untouched`).
