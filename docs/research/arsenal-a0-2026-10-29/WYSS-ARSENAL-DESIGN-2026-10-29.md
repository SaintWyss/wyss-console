# Wyss Arsenal — product architecture and implementation plan

Status: **DESIGN_READY_FOR_OWNER_REVIEW** · 2026-10-29 · base `main` @ `e9210d90f14ec826cd8cc6f2ad20148a30281867`
Scope: design and roadmap only. No code, dependency, provider-config, `main`, merge or push change was made.
Machine: Acer (Santiago's laptop). The second computer was not accessed.

## 0. Evidence legend and headline findings

| Tag          | Meaning                                                                                         |
| ------------ | ----------------------------------------------------------------------------------------------- |
| `[OBS]`      | Observed today on this machine (read-only command or file read)                                 |
| `[REPO]`     | Wyss Console code or committed doc at `e9210d9`                                                 |
| `[REPO-EV]`  | Wyss committed acceptance evidence (may be for an older provider build)                         |
| `[DOC]`      | Provider's upstream documentation or upstream source; **not** qualified for the installed build |
| `[PROPOSED]` | This design; nothing exists yet                                                                 |
| `NOT_PROVEN` | Mechanism plausible or documented, no evidence on the installed build                           |

Headline findings that shape the design:

1. **Wyss already owns the right seam.** Native profiles compile to a `LaunchPlan` (`src-tauri/src/native/profile.rs:221`), write a per-session Claude `--settings` overlay into `~/.local/share/wyss-console/native/sessions/<id>/settings.json` (`native/service.rs:810`, `:838-847`), bind the start to a preview fingerprint (`plan_token`, `service.rs:771-779`) and to an exact CLI build (`version_guard`, `service.rs:788-806`). Arsenal is an extension of this pipeline, not a new launcher. `[REPO]`
2. **Installed Claude Code drifted:** `claude --version` = `2.1.312` `[OBS]`, but Native override syntax is qualified only for exactly `2.1.309` (`native/capabilities.rs:32`) `[REPO]`. Today every profile launch with overrides is refused with `provider_version_unverified`. Any Arsenal slice needs a requalification step first.
3. **Claude per-session skill hiding is proven on an older build:** `skillOverrides: off` through the `--settings` overlay was observed on 2.1.303 (`capabilities.rs:39`, F1/F2a record) `[REPO-EV]`. Upstream docs add `name-only` and `user-invocable-only` states, a 1 % context listing budget, and state that **plugin skills are not affected by `skillOverrides`** `[DOC]`.
4. **Claude can add per-session capability without touching global files:** `--plugin-dir` ("for this session only"), `--agents <json>`, `--mcp-config` + `--strict-mcp-config`, `--append-system-prompt-file` are in the 2.1.312 help `[OBS]`. None is qualified by Wyss yet.
5. **Codex per-session skill exclusion is likely possible, contrary to the repo's current claim.** The repo says "Codex skills cannot be switched off for one session" (`profile.rs:334`, provider-matrix). Upstream Codex source collects `[[skills.config]]` rules from both the `User` and `SessionFlags` layers, rule-by-rule (later layer wins per selector), with `path` **or** `name` selectors, plus `skills.max_context_tokens` and `skills.include_instructions` `[DOC]`. `-c` writes the `SessionFlags` layer `[OBS help]`. This is `NOT_PROVEN` on 0.159.0 and must be probed (A0). Codex 0.159.0 also runs a **shared app-server daemon** by default (`--no-daemon` exists) `[OBS]`; whether launch overrides stay per thread is `NOT_PROVEN`.
6. **Why GSD children "lost" skills is now explained by provider semantics:** Claude subagents start with a fresh context and receive skill _content_ only via the agent's `skills:` frontmatter; they can still _invoke_ any skill available to the session through the Skill tool `[DOC]`. Codex children clone the parent's config (incl. `SessionFlags`) and inherit skill roots, not loaded bodies `[DOC]`. Availability propagates; loaded content does not.
7. **The installed GSD is GSD Core 1.15.0, not the patched 1.42.30.** It is a Wyss-managed additive payload at `~/.local/share/wyss/gsd-core/1.15.0/` with a manifest of 1 950 hashed artifacts and per-runtime projections (`claude`, `codex`, `opencode`) `[OBS]`. The historical `gsd-arsenal-patch`, `gsd-speckit-patch` and `gsd-crosscheck-patch` target the residual 1.42.30 trees (`~/.claude/get-shit-done`, `~/.codex/gsd-legacy-1.42.3`); **none of their markers exist in GSD Core 1.15.0** (`global arsenal`: 0 hits, `CONSTITUTION`: 0 hits) `[OBS]`. `codex-gsd-exec`, `codex-gsd-check`, `codex-gsd-bulk`, `claude-build`, `codex-ux` are live wrappers in `~/.local/bin` `[OBS]`.
8. **GSD Core already has the supported per-agent skill seam:** `.planning/config.json` → `agent_skills.<agent-type>` (orchestrator-side `<agent_skills>` injection plus agent self-load with a dedup guard, `references/agent-skills-bootstrap.md`), `model_overrides.<agent-type>`, and `agent_skills_security.trusted_global_roots` `[OBS]`. It is **project-scoped** (no session overlay env var exists; `GSD_HOME` only changes `~/.gsd/defaults.json`, which is ignored when a project config exists, `config-loader.cjs:933-1001`) `[OBS]`. Note: the GSD config loader may **write back** a normalized `config.json` (`persist` default true, `config-loader.cjs:701`) `[OBS]`.

Measured context baseline (estimate = characters / 4; deterministic script `catalog_cost.py` in the session scratchpad) `[OBS]`:

| Surface                                | Items                                                   | Catalog text exposed (est. tokens)                                       | All bodies (est.) |
| -------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------------------------ | ----------------- |
| `~/.claude/skills`                     | 118 (117 model-listed; 72 are GSD)                      | ≈ 5 840 (GSD share ≈ 1 760)                                              | ≈ 215 k           |
| Claude plugin skills (`plugins/cache`) | 46 SKILL.md files                                       | not measured (plugin-dependent)                                          | —                 |
| `~/.agents/skills` (Codex)             | 178                                                     | ≈ 13 000 before the 91 `enabled = false` rules in `~/.codex/config.toml` | ≈ 1.1 M           |
| Claude user MCP servers                | 3 (`context7`, `playwright`, `notebooklm`)              | tool schemas `UNKNOWN` offline                                           | —                 |
| Claude hooks (user settings)           | 9 handlers: PreToolUse 6, SessionStart 2, PostToolUse 1 | 0 (hooks are not context)                                                | —                 |

Claude truncates its listing to 1 % of context and Codex caps its catalog at 2 % / ≤ 10 k tokens `[DOC]`, so the real cost of a large library is **truncation and mis-triggering**, not only tokens. That is the problem Arsenal solves.

---

## 1. Current-state assessment

| Area                  | What exists (reuse)                                                                                                                                                                                                                                                                                                     | Gap for Arsenal                                                                                                                                                        |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Provider inventory    | `agentconfig/inventory.rs`: closed `SourceKey` list (Claude user/project skills, rules, MCP; Codex config/skills/hooks/rules; OpenCode), redacted `Item` with `ItemKind {Hook, RuleFile, Skill, McpServer, ModelPreference, Plugin}`, three-state `Persisted / Effective / SessionUse` (`agentconfig/model.rs:190-236`) | No agent definitions, commands, composite packages, plugin _contents_, or skill trees (resources/scripts). Items carry no stable identity across files.                |
| Safe reading          | `agentconfig/reader.rs`, `roots.rs` (path trust, bounded reads, link following), `redact.rs` (`may_hold_secret`)                                                                                                                                                                                                        | Reused as-is for import scanning.                                                                                                                                      |
| Config mutation       | `agentconfig/changes.rs`, `writer.rs`, `jsonspan.rs`, `tomlspan.rs`, journal, exact Undo, sync-conflict refusal                                                                                                                                                                                                         | Reused for the one Arsenal write to a non-Wyss file: GSD project bindings (§5).                                                                                        |
| Per-session launch    | `native/profile.rs` (`Body`, inheritance `merge`, `plan` → `LaunchPlan {words, overlay, applied, with_cost, not_applicable}`), `native/service.rs` (`preview`, `start`, session dir, env scrub of `SESSION_MARKERS`), `native/recipe.rs` (exec through the user shell)                                                  | `Body.skills_off` is a flat name list, capped at 64 (`profile.rs:13`), Claude-only. No additions, no plugin dir, no MCP scoping, no agent overlays, no token estimate. |
| Versioned persistence | `native/profiles.rs`: one JSON file per object, ≤ 20 versions, current pointer, export/import with no secrets; `native/store.rs` (`private_dir` 0700, `write_atomic`)                                                                                                                                                   | Same model reused for presets and selections. Content-addressed blobs are new.                                                                                         |
| Exact-build gating    | `native/capabilities.rs` `Evidence` records per provider build                                                                                                                                                                                                                                                          | Needs Arsenal operations and a 2.1.312 record.                                                                                                                         |
| Session registry      | `native/registry.rs`, `extended_registry.rs` (session id, profile ref, project)                                                                                                                                                                                                                                         | Needs the compiled selection hash per session.                                                                                                                         |
| Frontend              | `src/lib/shell/nav.ts` `ViewId` (has `profiles`, `configuration`, no `arsenal`), `features/native/ProfilesView.svelte`, `WorkspaceView.svelte`, `features/configuration/`                                                                                                                                               | New Arsenal view and a per-terminal drawer.                                                                                                                            |
| Managed plane         | F8 `codex_local_tools_v1` denies hooks/MCP/plugins/subagents (provider-matrix) `[REPO]`                                                                                                                                                                                                                                 | Arsenal must show "fixed security profile — not applicable" and never relax it.                                                                                        |
| Observation           | Native sessions show usage/PID; no transcript parsing of skill use                                                                                                                                                                                                                                                      | New: read-only transcript observer (Claude `projects/<p>/<session>.jsonl` and `<session>/subagents/*.jsonl` exist `[OBS]`).                                            |

Existing conflict to surface: the shipped template **"Exploration, fewer hooks"** (`profile.rs:373`) sets `disableAllHooks`, which also switches off the owner's protective hooks (`r3-gate.py`, `model-routing-gate.py`, `block-destructive.py` are registered in user settings) `[OBS]`. Arsenal will never offer hooks-off; the template's fate is an owner decision (OD-AR-08).

---

## 2. Domain model `[PROPOSED]`

New Rust module `src-tauri/src/arsenal/` (no dependency on `f8/`, `authority/`, `b1/`). Store root: `~/.local/share/wyss-console/arsenal/` (sibling of the existing `native/`, resolved like `native/mod.rs:33`).

### 2.1 Entities

```text
Capability            stable identity of one thing the owner can select
  id                  uuid (stable across revisions)
  kind                skill | rule | hook | mcp_server | agent | plugin | command | reference | delegation_wrapper
  name                provider-facing name (validated like profile::skill_ok, ≤64, [A-Za-z0-9._:-])
  portability         portable_skill | provider_specific(provider) | composite_member(composite_id)
  risk                text_only | executes_code | network | needs_secret   (needs_secret = not importable in v1)
  role                controlling_methodology | specialist | style | tool | mandatory
  facets              tags (domain: frontend, rust…; owner-defined)
  current_revision    n
  state               active | tombstoned

CapabilityRevision    immutable
  n, created_at, parent_n
  tree_hash           sha256 over sorted (relpath, mode, sha256(bytes))
  files[]             relpath, size, sha256, executable(bool)
  frontmatter         name, description, when_to_use, disable_model_invocation, declared tools
  provenance          source_provider, source_key (agentconfig::SourceKey or new), source_path_class,
                      import_mode copy|supervised_link, source_hash_at_import
  estimates           catalog_tokens_est, body_tokens_est   (chars/4, labelled ESTIMATED)
  consent             executes_code accepted? (per revision hash, owner-confirmed)

Dependency            from_capability → to (capability | binary | mcp_server | tool | provider_feature)
  kind                requires | recommends | provided_by_composite
  source              declared (frontmatter/manifest) | detected (static scan) | owner

ProviderBinding       how one revision reaches one exact provider build
  provider, build     e.g. claude-code "2.1.312 (Claude Code)"
  materialization     claude_session_plugin_skill | claude_skill_override | claude_agents_overlay |
                      claude_mcp_config | codex_skill_rule | gsd_agent_skills_entry | none
  enforcement         ENFORCED | ADVISORY | UNAVAILABLE
  evidence_ref        capabilities.rs record id

CompositeTool         GSD, Superpowers, gstack…
  id, name, package   {name, version, manifest_sha256, source root}
  members[]           capability ids with member_role (orchestrator_skill, agent, workflow, hook,
                      template, reference, support_script, delegation_wrapper, owner_patch)
  stages[]            Stage
  controlling         true when it owns the methodology (GSD, Superpowers)
  patches[]           OwnerPatch {target_version, markers[], status applied|absent|unknown}

Stage                 one agent launch boundary inside a composite
  stage_id            e.g. gsd:execute-phase/gsd-executor
  agent_type          provider agent name (frontmatter name:)
  optional            bool, with toggle_key only when the composite's schema has one
  binding             model?, effort?, attached capability ids (Wyss-selected)
  required_outputs    read-only, copied from composite (never overridable)

Preset                versioned like native/profiles.rs (≤20 versions, current pointer)
  name, applies_to providers
  items[]             capability id + pinned revision | "current"
  catalog_mode        per item: full | name_only (Claude only)
  exposure            strict | additive
  methodology         ≤1 controlling composite

Selection             what one scope uses
  scope               terminal | session | task | stage
  scope_ref, preset ref + deltas (+/- capabilities), revision pins
  timing              next_launch | next_gsd_spawn   (see §3.4)

EffectiveConfiguration   compiled, immutable, hashed (extends profile::LaunchPlan)
  selection_hash, provider, exact build
  words[], generated files[] (overlay, session plugin, mcp json, agents json) with hashes
  ladder[]            per capability: L0..L5 (§2.2) + evidence tag
  estimate            catalog tokens before/after, preload tokens, MCP servers count, hooks count
  conflicts[], warnings[], not_applicable[]
  plan_token          same contract as PreviewView.plan_token

Observation           read-only facts from provider transcripts
  session_id, child (agent_type, transcript ref), capability id, level exposed|loaded|invoked,
  source line hash, observed_at
```

### 2.2 The capability ladder (one vocabulary everywhere)

| Level               | Meaning                                                                          | Source of truth                        | Maps to existing                      |
| ------------------- | -------------------------------------------------------------------------------- | -------------------------------------- | ------------------------------------- |
| L0 Installed        | Present in a provider's files                                                    | `agentconfig::inventory`               | `Persisted::PresentEnabled/Disabled`  |
| L1 Imported         | A Wyss revision exists                                                           | Arsenal store                          | new                                   |
| L2 Available        | A `ProviderBinding` exists for this exact build (ENFORCED or ADVISORY)           | `capabilities.rs`                      | `Effective::Documented / Verified*`   |
| L3 Selected         | In the scope's `Selection`                                                       | Arsenal store                          | new                                   |
| L4 Exposed          | Launched in the compiled config **and** (when possible) seen by the provider     | launch record + provider listing probe | `SessionUse::DocumentedUsing / Using` |
| L5 Loaded / Invoked | Body entered a context (preload, Skill tool call, `<agent_skills>` include read) | transcript observer                    | `SessionUse::Using`                   |

Rules: importing never raises L0→L3 (import ≠ enable); a parent at L4/L5 says nothing about a child (each child gets its own ladder row); L4 without a provider observation is drawn as "documented", never as "verified" (FR-UX-03 convention).

### 2.3 Invariants

- I-1 No global provider file is written by Arsenal. Gate: hashes of `~/.claude/settings.json`, `~/.claude.json` (bounded projection), `~/.codex/config.toml`, `~/.codex/hooks.json`, `~/.claude/AGENTS.md`, `~/.codex/AGENTS.md` identical before/after every Arsenal operation and launch.
- I-2 Revisions are immutable; content-addressed blobs are never edited in place.
- I-3 `role = mandatory` capabilities (owner security hooks, kernel instructions) cannot be deselected; Arsenal never emits `disableAllHooks`, `--bare`, `--setting-sources` without `user`, `--dangerously-*`.
- I-4 Every launch with Arsenal content is bound to an exact build and a preview `plan_token`.
- I-5 Nothing imported is executed by Wyss.

---

## 3. User journeys and Arsenal tab UX `[PROPOSED]`

### 3.1 Journeys

**J1 — Focused Codex frontend session (the owner's example).**
Workspace → Codex → project X → task text "frontend of project X" → _Arsenal_ button on the pending terminal → drawer opens scoped to this terminal → owner picks preset "Frontend TS" (React, TypeScript, Vitest, concise) → preview shows: _Enforced_ (skill rules for this Codex build), _Advisory_ (anything Codex cannot enforce), _Unavailable_ (with reason), catalog estimate "≈ 13 000 → ≈ 900 tokens", conflicts none → **Start** (existing `start` with `plan_token` + `expected_cli_version`). If A0 shows Codex 0.159.0 ignores session skill rules, the same screen says so and offers only _Advisory_ — it never claims isolation.

**J2 — Claude Code same flow.** Identical UI; enforcement uses `skillOverrides` (off / name-only) for user skills not selected, a Wyss session plugin for imported skills, session plugin disables if proven.

**J3 — Revisit while working.** Drawer on a running terminal shows the _launched_ configuration (frozen) and an _edited_ selection side by side. Each change carries a timing badge (§3.4). "Apply" for a restart-only change offers **Restart as new conversation** (guaranteed) or **Resume** (labelled `NOT_PROVEN`: Claude records the rendered system prompt and re-sends it on resume until compaction `[OBS help --system-prompt-snapshot]`).

**J4 — Import an existing setup.** First Arsenal visit → "Found 118 Claude skills, 178 Codex skills, 35 Claude agents, 59 Codex agents, 6 Claude plugins, GSD Core 1.15.0 (composite)…" → review → import selected → nothing becomes active.

**J5 — GSD with per-stage skills.** Arsenal → Composites → GSD Core 1.15.0 → stage board (researcher, planner, plan-checker, executor, verifier, ui-*, …) → attach "Frontend TS" to `gsd-executor`, "concise" to all → model per stage → project-scoped write preview (`.planning/config.json` diff) → confirm → verification "`gsd_run query agent-skills gsd-executor` returns exactly these includes".

**J6 — Inspect a session.** Sessions → session → Arsenal tab: ladder table per capability and per child agent, with transcript evidence links ("gsd-executor child #2 read `…/SKILL.md` at 14:03").

### 3.2 Arsenal view layout (new `ViewId 'arsenal'`)

```
┌ Arsenal ─────────────────────────────────────────────────────────────────────┐
│ [Library] [Presets] [Composites] [Sessions] [Import]        search ⌘K        │
├────────────┬──────────────────────────────────────────┬──────────────────────┤
│ Facets     │ Results (virtualized)                     │ Detail               │
│ Kind       │ name · kind · role · providers ● ◐ ○      │ SKILL.md (read-only) │
│ Provider   │ est. catalog tok · body tok · risk        │ Revisions r1 r2 →    │
│ Source     │ dup/conflict badges                       │ Provenance & hashes  │
│ Composite  │                                           │ Dependencies         │
│ Role       │                                           │ Provider bindings    │
│ Risk       │                                           │  Claude 2.1.312 ◐    │
│ Tags       │                                           │  Codex 0.159.0 ○     │
│ Ladder lvl │                                           │ Used by presets/sess │
└────────────┴──────────────────────────────────────────┴──────────────────────┘
● enforced on this exact build   ◐ documented / advisory   ○ unavailable (reason on hover and in text)
```

- Search: deterministic substring + facet filter over name/description/tags (no LLM, no embedding).
- Badges are text + icon (not colour-only); evidence tags are always readable.
- Presets tab: editor with methodology slot (≤ 1 controlling), item list with `full / name-only`, live estimate, version history and restore (same UI pattern as `ProfilesView.svelte`).
- Composites tab: package identity, member counts by role, owner patches with status, stage board.
- Sessions tab: per running/ended Native session, the frozen `EffectiveConfiguration` and observations.
- Import tab: §4.

### 3.3 Terminal drawer (Workspace)

Opened from a pending or running Native terminal; scoped to that terminal only. Shows provider + exact build, selection chips, the three existing preview lists (`applied`, `with_cost`, `not_applicable`) extended with _Advisory_, a token meter (catalog before → after; preload; MCP servers; "MCP tool schemas: unknown until connected"), conflicts, and the timing of each pending change.

### 3.4 When a change takes effect (shown per change, never implied)

| Change                  | Claude Native                                                   | Codex Native                               | GSD stage binding                                                                                 |
| ----------------------- | --------------------------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------------------------------- |
| Select / deselect skill | **Restart required** (overlay, plugin dir are launch-time)      | **Restart required** (`-c` is launch-time) | **Next GSD spawn** of that agent type (GSD queries `agent_skills` at spawn) `[OBS bootstrap doc]` |
| Model / effort          | Restart (existing)                                              | Restart (existing)                         | Next spawn (`model_overrides`)                                                                    |
| Next turn               | Not offered in v1: no proven per-turn reload for these surfaces | Not offered                                | —                                                                                                 |

---

## 4. Import flow `[PROPOSED]`

1. **Discover (read-only).** Reuse `inventory::Environment` and `reader`. New scanners (bounded, no execution): Claude `skills/*/SKILL.md` + resource trees, `agents/*.md`, `plugins/installed_plugins.json` + plugin manifests and their skills/agents/hooks, MCP (bounded projection already in F1); Codex `~/.agents/skills`, `$CODEX_HOME/skills`, `$CODEX_HOME/agents/*.toml`, `[mcp_servers]`, `[[skills.config]]`; project `.claude/skills`, `.agents/skills`. Links followed only through `roots.rs` rules, recorded as link + target class.
2. **Classify.**
   - _Portable skill_: `SKILL.md` with frontmatter, text resources only.
   - _Provider-specific_: Claude plugin, hook, agent `.md`; Codex agent `.toml`, hook; MCP server.
   - _Composite_: detected by manifest, not by name: GSD Core `manifest.json` (`schema wyss.gsd-core-additive-payload-manifest.v1`, artifacts with category + sha256, `projection_ownership`) → exact member list; Claude plugin manifest (`.claude-plugin/plugin.json`) → plugin composite (Superpowers); gstack → a skill family sharing a support tree (detected by shared relative script references; marked `heuristic`).
   - _Residue_: inactive trees (e.g. GSD 1.42.30 in `~/.claude/get-shit-done`, `~/.codex/gsd-legacy-1.42.3`) shown as inactive, not offered as active capabilities.
3. **Analyze.** Dependencies (frontmatter tools, referenced scripts/binaries, MCP names mentioned, composite membership), provider compatibility (§6), duplicates (§7.1), risk (`executes_code` when any file is executable or a script is referenced; `needs_secret` when `redact::may_hold_secret` fires or an MCP entry has env/headers).
4. **Review.** Per item: file list with sizes and hashes, risk, duplicates, what will _not_ be imported and why. Bulk actions by facet.
5. **Import.** Copy-based by default: blobs into `arsenal/blobs/sha256/…` (0400), revision 1, provenance with source hash. `supervised_link` mode (later PR): store path + hash only; every materialization re-hashes the source and refuses on drift ("changed outside — review new revision").
6. **Never.** Execute anything; import `auth.json`, `.credentials.json`, OAuth state, `~/.claude.json` beyond the existing MCP projection, MCP env/header values, Codex trust records; modify the source.
7. **Updates and rollback.** Re-scan shows "source changed" per item → new revision on owner confirmation; current pointer moves; previous revisions stay selectable; sessions keep the revision they launched with.

---

## 5. GSD integration architecture

### 5.1 What is actually installed `[OBS]`

| Part                                                                               | Location                                                                                                                          | Count                          |
| ---------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| Payload (engine, workflows, references, templates, `bin/gsd-tools.cjs`, `gsd_run`) | `~/.local/share/wyss/gsd-core/1.15.0/payload/package/gsd-core` (symlinked as `~/.claude/gsd-core`, `~/.codex/gsd-core`)           | 109 workflows, ~110 references |
| Claude projection                                                                  | `…/1.15.0/claude/.claude/{agents,skills}` → 35 agents, 72 skills symlinked into `~/.claude`                                       | 35 + 72                        |
| Codex projection                                                                   | `~/.codex/agents/gsd-*.toml` (59 agent files incl. non-GSD), skills under `~/.agents/skills`                                      | —                              |
| Hooks                                                                              | `~/.claude/hooks/gsd-*.{js,sh}` (context monitor, prompt guard, read guard, workflow guard, phase boundary, validate commit…)     | —                              |
| Project state                                                                      | `<project>/.planning/` (config.json, STATE, ROADMAP, phases)                                                                      | per project                    |
| Delegation wrappers                                                                | `~/.local/bin/codex-gsd-{exec,check,bulk}`, `claude-build`, `codex-ux` (read `CODEX_MODEL/EFFORT`, consult `workflow-control.py`) | 5                              |
| Owner patches                                                                      | `~/.claude/gsd-{arsenal,speckit,crosscheck}-patch/` target 1.42.30; **absent from Core 1.15.0**                                   | 3 (inactive)                   |

### 5.2 Model

GSD becomes a `CompositeTool` built from its manifest (identity = package version + `manifest.json` sha256). Stages are derived, not hand-written: one `Stage` per agent definition in the projection (frontmatter `name:`), linked to the workflows that spawn it (static reference scan of `workflows/*.md`). Optional stages are offered only where `bin/shared/config-schema.manifest.json` declares a toggle key (e.g. workflow research/plan-check/verifier switches); otherwise the stage is shown as _required by GSD_. Required outputs (PLAN.md, SUMMARY.md, VERIFICATION.md…) are displayed read-only.

### 5.3 Per-stage capability and model binding — the supported seam

Wyss writes, through the existing reviewed writer (`agentconfig/changes.rs` + `jsonspan.rs`, plan → preview → confirm → exact Undo), **only these keys** of `<project>/.planning/config.json`:

```json
{
  "agent_skills": {
    "gsd-executor": [
      ".wyss/arsenal/9f3c…/frontend-ts",
      ".wyss/arsenal/9f3c…/concise"
    ],
    "gsd-planner": [".wyss/arsenal/9f3c…/concise"]
  },
  "model_overrides": { "gsd-executor": "<model-id>" }
}
```

- Imported skills are materialized **project-locally** at `<project>/.wyss/arsenal/<preset-rev-hash>/<skill>/` (read-only files) because GSD only accepts project-confined paths or `global:<name>` resolved inside the runtime's global skills dir (`init.cjs:3596-3645`) `[OBS]`. `.wyss/` is excluded through `.git/info/exclude` (local, not shared) — never `.gitignore`. (Owner decision OD-AR-03.)
- A skill that is already installed globally at the _same tree hash_ may be referenced as `global:<name>`; a different hash is refused (it would load a different text than the one selected).
- `model_overrides` is GSD's own per-agent model key `[OBS planning-config.md:407]`; Wyss shows it as _selected_, never as _observed_.
- This is **project-scoped**, by GSD's design. Two simultaneous sessions in the same project share it; the UI says so. Session-scoped GSD stage overrides are not offered in v1 (see alternative ALT-6).

### 5.4 Child-agent propagation and launch-boundary verification

| Boundary                                         | What reaches the child                                                                                                                                                                     | How Wyss verifies                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wyss → provider session                          | Compiled `EffectiveConfiguration`                                                                                                                                                          | Launch record + provider listing probe (A0)                                                                                                                                                                                                                                                                                                                                                           |
| GSD orchestrator → `gsd-*` agent (Claude)        | `<agent_skills>` includes for that agent type; plus every skill _available_ to the session via the Skill tool `[DOC]`                                                                      | **Before launch:** run `gsd_run query agent-skills <agent>` for each bound agent (read-only per GSD docs, deterministic Node, no model) and diff against the expected list; hash `config.json` before/after to catch GSD's normalization write-back. **After spawn:** transcript observer reads `<session>/subagents/*.jsonl` for the include block and the `Read` of each `SKILL.md` → L5 per child. |
| GSD orchestrator → `gsd-*` agent (Codex runtime) | Child clones parent config incl. `SessionFlags` skill rules `[DOC]`; filesystem includes self-loaded; plugin-namespaced skills skipped by GSD on non-Claude runtimes `[OBS bootstrap doc]` | Same pre-launch query; post-spawn Codex rollout parsing is `NOT_PROVEN` (A5 research)                                                                                                                                                                                                                                                                                                                 |
| GSD → `codex-gsd-*` wrapper → Codex process      | Whatever the wrapper passes (model/effort env, no Arsenal manifest today)                                                                                                                  | Shown as `delegation_wrapper` members, enforcement **UNAVAILABLE** until the wrapper accepts an explicit manifest (owner-owned tool, outside Wyss)                                                                                                                                                                                                                                                    |

Display rule: the session view lists **each child separately**. A parent having a skill is never rendered as the child having it. "Available via Skill tool" and "preloaded" are separate columns.

### 5.5 Updates

- Composite identity change (new version dir or manifest hash) → bindings re-validated: agent types that disappeared → _orphaned_ (launch warns, binding kept); `agent_skills` / `model_overrides` semantics re-checked against the new config-schema manifest; owner patches never re-applied automatically, only their markers checked.
- GSD's own `/gsd-update` cannot delete Wyss bindings: they live in project config (which GSD preserves) and in `.wyss/` (which GSD does not own).
- The old `references/arsenal.md` "Step 0" approach is retired by design: no global router text is injected anywhere.

---

## 6. Provider compatibility and enforcement matrix

| Mechanism                             | Claude Code 2.1.312                                                                                | Codex 0.159.0                                                                                                                         | OpenCode 1.19.4 / Hermes |
| ------------------------------------- | -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| Hide a user skill for one session     | `--settings` overlay `skillOverrides: off` — proven 2.1.303 `[REPO-EV]`; 2.1.312 `NOT_PROVEN` → A0 | `-c 'skills.config=[{name=…,enabled=false}]'` SessionFlags layer `[DOC]`; 0.159.0 `NOT_PROVEN` (repo currently says unsupported) → A0 | UNAVAILABLE              |
| List without description              | `skillOverrides: name-only` `[DOC]` → A0                                                           | none known                                                                                                                            | —                        |
| Hide plugin skills                    | not affected by `skillOverrides` `[DOC]`; `enabledPlugins:false` in overlay `NOT_PROVEN` → A0      | `[plugins]` per-session `NOT_PROVEN`                                                                                                  | —                        |
| Add an imported skill for one session | `--plugin-dir <session plugin>` `[OBS help]`, skills namespaced `plugin:skill` `[DOC]` → A0        | **UNAVAILABLE** without writing a provider/project skills dir; option: project-local `.agents/skills` materialization (OD-AR-03)      | —                        |
| Catalog budget                        | `skillListingBudgetFraction` (overlay) `[DOC]`                                                     | `skills.max_context_tokens` (`-c`) `[DOC]`                                                                                            | —                        |
| MCP for one session                   | `--mcp-config <file> --strict-mcp-config` `[OBS help]`; secret-bearing servers excluded            | `-c mcp_servers.<n>.enabled=false` `NOT_PROVEN`                                                                                       | —                        |
| Custom agents for one session         | `--agents <json>` `[OBS help]`                                                                     | role files only in `$CODEX_HOME/agents` / project `.codex/agents` (global/project) `[DOC]`                                            | —                        |
| Hooks                                 | additive only; never off (I-3)                                                                     | no per-session off `[REPO]`                                                                                                           | —                        |
| Extra instructions                    | `--append-system-prompt-file` `[OBS help]`; `AGENTS.md`/`CLAUDE.md` (`@AGENTS.md`) untouched       | none per session besides the first message                                                                                            | —                        |
| Children inherit availability         | yes, via Skill tool; content only via `skills:` `[DOC]`                                            | yes, config clone incl. SessionFlags `[DOC]`                                                                                          | —                        |
| Resume after selection change         | system-prompt snapshot re-sent `[OBS help]` → "new conversation"                                   | `NOT_PROVEN`                                                                                                                          | —                        |
| Shared daemon interference            | n/a                                                                                                | default shared app-server; per-thread scope `NOT_PROVEN` → A0; `--no-daemon` fallback (OD-AR-06)                                      | —                        |
| Managed plane                         | Claude Managed admission closed `[REPO]`                                                           | F8 fixed profile denies skills/MCP/plugins/subagents `[REPO]`                                                                         | —                        |
| Observation                           | `projects/<p>/<session>.jsonl` + `subagents/*.jsonl` exist `[OBS]`; parser `NOT_PROVEN`            | `sessions/**/rollout-*.jsonl` exist `[OBS]`; skill events `NOT_PROVEN`                                                                | —                        |

Enforcement labels shown to the owner: **ENFORCED** (exact-build evidence that the provider honours it), **ADVISORY** (included or excluded, but the provider can still see/do otherwise), **UNAVAILABLE** (with the unlocking proof named).

---

## 7. Conflict detection and context efficiency

### 7.1 Deterministic conflict rules (no LLM)

| Rule                   | Detection                                                                                                             | Severity                                             |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| C1 Exact duplicate     | same `tree_hash` from two sources (e.g. a skill present in both `~/.claude/skills` and `~/.agents/skills` `[OBS]`)    | info; dedup into one capability with two provenances |
| C2 Divergent duplicate | same name, different hash                                                                                             | warn; selection must pin one                         |
| C3 Methodology         | > 1 `controlling_methodology` in one selection (e.g. GSD + Superpowers)                                               | **block** until owner chooses                        |
| C4 Style exclusivity   | > 1 `style` capability with `exclusive` tag (e.g. caveman vs humanizer voice)                                         | warn                                                 |
| C5 Declared conflict   | `conflicts_with` annotations (owner or package)                                                                       | block                                                |
| C6 Shadowing           | provider precedence makes one source hide another (project vs user skill, local plugin vs marketplace plugin `[DOC]`) | warn, show which wins                                |
| C7 Trigger overlap     | token-set Jaccard of name+description over a fixed threshold                                                          | info                                                 |
| C8 Mandatory           | attempt to deselect `mandatory`                                                                                       | refused                                              |
| C9 Budget              | catalog estimate > provider budget (1 % / 2 %) → truncation predicted                                                 | warn with the items that would lose descriptions     |

A composite may declare compatible specialists: one controlling methodology plus any number of `specialist` / `style` / `tool` capabilities is the normal shape.

### 7.2 Efficiency strategy

- Default exposure **strict**: only selected + mandatory are listed; "maybe" items as `name-only` on Claude.
- Preload (`skills:` / `agent_skills`) only per stage, never for the whole session: preload injects full bodies, listing does not.
- Arsenal adds **no** prose of its own to any context (no router skill, no manifest paragraph). The compiled config is the manifest.
- Metrics, each labelled: catalog tokens (ESTIMATED, chars/4), preload tokens (ESTIMATED), MCP servers (COUNT; schemas UNKNOWN until connected), first-request input tokens (MEASURED from transcript `usage`, only for sessions the owner actually ran).
- Success is measured as: predicted truncation eliminated, catalog estimate reduced, observed invocations match selection — not as "fewer installed skills".

---

## 8. Security, isolation, persistence, rollback

- **Store**: `~/.local/share/wyss-console/arsenal/` 0700 (`store::private_dir`), metadata JSON written with `store::write_atomic`, blobs 0400 content-addressed, append-only journal (pattern of `agentconfig/journal.rs`), caps like `profiles.rs` (objects, versions, file sizes).
- **Materialization**: per session under `native/sessions/<id>/arsenal/` (session plugin, MCP json, agents json), 0500/0400, hashed into the `plan_token`; re-hashed at start (same-UID tampering cannot be prevented, only detected — same blocker class as `implementation-status.md` §4).
- **Executable content**: copied without exec bit unless the revision carries owner consent for `executes_code`; Wyss never runs it. Plugins with hooks are `executes_code` (08 §5).
- **Credentials**: never imported; MCP servers needing secrets are not materialized in v1 (no ADR0038 provider injection).
- **Boundaries**: Arsenal acts only on the Native plane. Managed profiles render "fixed security profile". No change to `f8/`, `authority/`, approvals, `r3-gate`, routing gate or any hook. Native env scrub (`SESSION_MARKERS`) unchanged.
- **Writes outside Wyss**: only (a) `.planning/config.json` keys `agent_skills`, `model_overrides`, `agent_skills_security` via the reviewed writer with exact Undo; (b) `<project>/.wyss/arsenal/…`; (c) one line in `.git/info/exclude`. All opt-in per project.
- **Rollback**: preset/selection = pointer to previous version; capability = previous revision; GSD binding = existing exact Undo (refused if the file diverged); delete = tombstone, blob GC only when no revision is referenced by a session record.

---

## 9. Alternatives considered and decisions for the owner

| Alt    | Idea                                                                           | Verdict                                                                       |
| ------ | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------- |
| ALT-1  | Per-session `CLAUDE_CONFIG_DIR` / `CODEX_HOME` clone                           | Rejected: requires copying auth (AUTH-MODIFYING), forks history/state         |
| ALT-2  | Rewrite global config before launch, restore after                             | Rejected: global mutation, races with other sessions and the second PC's sync |
| ALT-3  | One global "arsenal" router skill / reference (old `arsenal.md`)               | Rejected: context cost, advisory, not inherited by children                   |
| ALT-4  | Symlink farm into `~/.claude/skills` / `~/.agents/skills`                      | Rejected: global                                                              |
| ALT-5  | Claude `--add-dir <store>` (loads `.claude/skills`) `[DOC]`                    | Rejected: grants the agent file access to the store                           |
| ALT-6  | Claude `--agents` JSON re-defining `gsd-*` with `skills:` (session-scoped GSD) | Deferred: copies GSD agent bodies → drift on every update; revisit after A8   |
| ALT-7  | `--bare` / `--setting-sources project,local`                                   | Rejected: drops owner security hooks / changes auth                           |
| ALT-8  | Codex `-p <name>` profile file in `$CODEX_HOME`                                | Owner choice only if `-c` fails (writes a new file in provider home)          |
| ALT-9  | SQLite store                                                                   | Not now: JSON + blobs matches `profiles.rs`, no new dependency                |
| ALT-10 | Exact token counts via provider API                                            | Not now: needs credentials/network; estimates + transcript usage instead      |

| ID       | Decision                                                                                           | Recommendation                                                                                   |
| -------- | -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| OD-AR-01 | Default exposure strict vs additive                                                                | **Strict**, with `name-only` for Claude "maybe" items                                            |
| OD-AR-02 | Import mode for the first slice                                                                    | **Copy-only**; supervised links in A9                                                            |
| OD-AR-03 | Allow project-scoped GSD bindings + `<project>/.wyss/arsenal` + `.git/info/exclude`                | **Yes, opt-in per project**                                                                      |
| OD-AR-04 | Authorize A0 probes (provider CLIs run without model calls; Claude TUI in the nested KWin display) | **Yes**                                                                                          |
| OD-AR-05 | Codex fallback if 0.159.0 ignores session skill rules                                              | **Advisory only** until a newer build proves it; ALT-8 only if you want enforcement now          |
| OD-AR-06 | `--no-daemon` for Arsenal Codex sessions if the probe shows daemon-wide config                     | Decide after A0                                                                                  |
| OD-AR-07 | Mandatory set                                                                                      | All user-settings hooks + global `AGENTS.md`/`CLAUDE.md` always load; Arsenal never touches them |
| OD-AR-08 | Template "Exploration, fewer hooks" disables protective hooks                                      | Keep with an explicit warning, unreachable from Arsenal; or retire                               |
| OD-AR-09 | Superpowers (installed, disabled)                                                                  | Import as composite, `controlling_methodology`, C3 against GSD                                   |
| OD-AR-10 | Requalify Claude 2.1.312 (today all override launches are refused)                                 | **Yes**, inside A0                                                                               |

---

## 10. Implementation roadmap (PR boundaries)

Every PR: branch from `main`, no merge/push without owner, targeted tests only (repo rule: never blanket `cargo test` / `lifecycle_process`), I-1 hash gate, `npm run check` + `svelte-check` for frontend PRs, no path under `src-tauri/src/{f8,authority,b1,remote}` touched (diff path guard).

| PR                                  | Content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Depends                     | Acceptance tests                                                                                                                                                                                                  | Measurable outcome                                                                                           |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| **A0 Evidence**                     | Research doc + `capabilities.rs` records (data only). Probes: P-C1 2.1.312 `skillOverrides off/name-only` via overlay; P-C2 `--plugin-dir` session plugin skills listed and namespaced; P-C3 `enabledPlugins:false` in overlay; P-C4 subagent sees session-hidden skill? (Skill tool listing); P-C5 `--mcp-config --strict-mcp-config`; P-X1 Codex 0.159.0 `-c skills.config` by name and by path via read-only `skills/list`; P-X2 daemon vs `--no-daemon` scope; P-X3 `-c skills.max_context_tokens`; P-G1 `gsd_run query agent-skills` on a fixture project (and config write-back check). No model call in any probe. | owner OK (OD-AR-04)         | Each probe: positive + negative control, raw output stored outside repo, global hashes unchanged                                                                                                                  | Each mechanism labelled ENFORCED / ADVISORY / UNAVAILABLE for the exact build                                |
| **A1 Store**                        | `arsenal/` module: entities §2, revisions, blobs, journal, caps, tombstones                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | —                           | crash/atomic write, cap overflow, tombstone GC, hash verification, concurrent writer refusal                                                                                                                      | 0 partial files after injected crash                                                                         |
| **A2 Discover + import**            | Scanners §4, composite detection (GSD manifest, plugin manifest), dedup C1/C2, risk classification, import commit                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | A1                          | fixture HOME trees: links, link escape, secrets, sync-conflict copies, huge files, malformed frontmatter, residue trees; read-only acceptance on the real HOME (no writes)                                        | Real HOME: 118 + 178 skills, 35 + 59 agents, GSD 1.15.0 detected; 0 secrets imported; 0 source bytes changed |
| **A3 Arsenal view (read + import)** | `ViewId 'arsenal'`, Library/Detail/Import tabs, facets, search, badges, estimates; UI contract (tokens, states, dark mode, a11y) written first                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | A2                          | component tests, keyboard nav, empty/error/loading states, 300+ items virtualized                                                                                                                                 | Search < 50 ms on 300 items; screenshot evidence light/dark                                                  |
| **A4 Claude enforcement slice**     | Selection → `EffectiveConfiguration` (extends `profile::plan`): overlay `skillOverrides` (off/name-only) for every non-selected user skill, session plugin with selected imported skills, mandatory pins, estimate before/after, `plan_token`; Workspace drawer (minimal); session record stores selection hash                                                                                                                                                                                                                                                                                                           | A0 (P-C1, P-C2), A1, A2     | unit: compiler golden files; service: start refused on stale plan / unverified build / tampered plugin; native acceptance (owner-authorized, nested display): `/skills` set equals compiled expectation; I-1 gate | Listed user skills 117 → selected + mandatory; catalog estimate ≈ 5 840 → reported value; 0 global writes    |
| **A5 Inspector + observation**      | Sessions tab, per-child ladder, Claude transcript parser (Skill invocations, `<agent_skills>` blocks, subagent `Read` of `SKILL.md`, first-request `usage`)                                                                                                                                                                                                                                                                                                                                                                                                                                                               | A4                          | parser fixtures from redacted real transcripts; unknown line kinds ignored and counted                                                                                                                            | Observed L5 per child; measured input tokens shown only where a real session exists                          |
| **A6 Codex slice**                  | `-c skills.config` rules (+ budget) if P-X1 passes; otherwise ADVISORY UI; daemon policy per OD-AR-06                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | A0 (P-X1..3), A4            | golden words; start refusal paths; native acceptance via `skills/list` read-only                                                                                                                                  | Codex catalog estimate ≈ 13 000 → reported value, or explicit ADVISORY                                       |
| **A7 Presets + conflicts**          | Preset store (profiles.rs model), C1–C9 engine, methodology slot                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | A3, A4                      | rule fixtures per C-rule; preset version restore                                                                                                                                                                  | C3 blocks GSD + Superpowers; C9 predicts truncation correctly vs provider budget                             |
| **A8 GSD composite**                | Stage board, project binding writer (reviewed writer, exact Undo), project-local materialization + `.git/info/exclude`, pre-launch `gsd_run query` verification, update drift handling                                                                                                                                                                                                                                                                                                                                                                                                                                    | A0 (P-G1), A5, A7, OD-AR-03 | fixture project: query output equals expected; diverged config refuses Undo; GSD version change → orphan detection; transcript shows child loading                                                                | Bound skills observed in ≥ 1 real `gsd-executor` child transcript (owner-run)                                |
| **A9 More composites + links**      | Superpowers plugin composite, gstack family, supervised links, re-import/update UX                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | A2, A7                      | drift refusal; heuristic composite labelled                                                                                                                                                                       | —                                                                                                            |

---

## 11. Recommended first milestone: **AM1 = A0 + A1 + A2 (Claude user skills) + A4**

A complete vertical slice: import a few Claude skills → build a selection → preview with enforcement labels and token estimate → launch Claude Code Native → verify the provider lists exactly that set → global files untouched.

Why this one:

- It exercises the whole promise (import ≠ enable, scoped selection, verified exposure, no global writes) on the provider with the most existing evidence (`skillOverrides` proven on 2.1.303) and the most reusable code (overlay, session dir, `plan_token`, version guard already shipped).
- It forces the 2.1.312 requalification, which is needed anyway: today no profile override can launch.
- It needs no project writes, no GSD coupling and no owner product decision beyond OD-AR-01/02/04/10.
- Codex (A6) follows immediately and reuses the compiler; doing it first would build on an unproven mechanism (P-X1).
- GSD (A8) depends on observation (A5) to prove child loading, which is the original pain point; shipping it before observation would repeat the `arsenal.md` mistake of unverified propagation.

---

## 12. Unresolved technical constraints

1. Claude 2.1.312 is unqualified; all Native overrides are refused until A0 records evidence.
2. Codex 0.159.0 session skill rules (`-c skills.config`) and daemon scope are unproven; the current repo claim "unsupported" is also unproven.
3. Claude plugin skills cannot be hidden via `skillOverrides` `[DOC]`; per-session `enabledPlugins` is unproven.
4. Codex has no known per-session _add-skill_ mechanism; additions require project or provider directories.
5. GSD per-agent skills are project-scoped by GSD's design; two sessions in one project share bindings.
6. GSD's config loader can write back a normalized `config.json`; Wyss can detect it, not prevent it.
7. Same-UID agents can modify materialized files; Wyss detects by hash, cannot prevent (existing blocker class).
8. MCP tool-schema token cost is unknown without connecting to servers; v1 reports server counts only.
9. `codex-gsd-*` wrappers do not accept an explicit capability manifest; Codex delegations from GSD stay UNAVAILABLE for Arsenal enforcement.
10. Resume semantics after a selection change are unproven for both providers; v1 offers "new conversation" as the guaranteed path.
11. The second PC (`.18`) was not inspected; its GSD Core install is recorded as pending in memory and must be inventoried before any cross-device claim.

DESIGN_READY_FOR_OWNER_REVIEW
