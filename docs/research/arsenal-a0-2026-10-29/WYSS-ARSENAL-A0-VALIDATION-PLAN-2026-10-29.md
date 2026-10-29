# Wyss Arsenal — A0 compatibility and isolation validation plan

Status: **A0_PLAN_READY_FOR_OWNER_APPROVAL** · 2026-10-29 · protected `main` `e9210d90f14ec826cd8cc6f2ad20148a30281867` (clean, unchanged)
Companion of: `WYSS-ARSENAL-DESIGN-2026-10-29.md` (same folder). Plan only: nothing here was executed.

## 0. What was and was not run

- This turn: static reads only (repository source/docs, file listings, `sha256sum` and `grep -a -c` string counts over the installed provider binaries, root-owned `/etc/codex/managed_config.toml`). **No provider binary was executed.**
- Disclosure: in the design turn (before this restriction), `claude --version`, `claude --help`, `codex --version` and `codex --help` were executed against the real HOME. No session, prompt, model call or config write resulted; hooks are not known to run for `--help`/`--version`, but that is `ASSUMED`. A0 never repeats provider execution outside the sandbox.
- No credential file was opened. Credential files appear below only as `stat` targets.

Finding classes used throughout:

| Class                | Meaning                                                                                                    |
| -------------------- | ---------------------------------------------------------------------------------------------------------- |
| **VERIFIED**         | Observed on this machine's installed artefacts, or recorded Wyss evidence on the _same_ build              |
| **SOURCE-SUPPORTED** | Upstream docs/source, or the feature's strings are present in the installed binary; behaviour not observed |
| **ASSUMED**          | Reasoned, no artefact checked                                                                              |
| **NOT_PROVEN**       | No evidence for the installed build; A0 must decide it                                                     |

---

## 1. Verified baseline and discrepancies

### 1.1 Baseline

| Item                      | Value                                                                                                                                                                                                                                                                                                                                                                        | Class                                                          |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| Claude Code               | `~/.local/bin/claude` → `~/.local/share/claude/versions/2.1.312`, sha256 `4f4178ce41d83e23341ebfbaecdbcf8ea6bf7baea0690025abe49eeec3a5d01d`; builds 2.1.310 and 2.1.311 also on disk                                                                                                                                                                                         | VERIFIED                                                       |
| Codex CLI                 | `~/.local/bin/codex` → `~/.codex/packages/standalone/releases/0.159.0-x86_64-unknown-linux-musl/bin/codex`, sha256 `d2752c52353401f7f6efbfcea68796f4f7a3d3e4769f5d1da53fa49d4856b72f` (286 750 056 B; release dir also holds `codex-resources`, `codex-code-mode-host`)                                                                                                      | VERIFIED                                                       |
| GSD Core                  | `@opengsd/gsd-core 1.15.0`, commit `b10ab3fdeb62…`, manifest at `~/.local/share/wyss/gsd-core/1.15.0/manifest.json`                                                                                                                                                                                                                                                          | VERIFIED                                                       |
| GSD 1.42.30               | residue only (`~/.claude/get-shit-done`, `~/.codex/gsd-legacy-1.42.3`); owner patches target it; not active                                                                                                                                                                                                                                                                  | VERIFIED                                                       |
| Sandbox tool              | `/usr/bin/bwrap`, bubblewrap 0.13.1                                                                                                                                                                                                                                                                                                                                          | VERIFIED (presence); user-namespace operation NOT_PROVEN today |
| Existing isolated harness | `src-tauri/claude-bridge/probe/sandbox.py` (`bwrap --unshare-all --die-with-parent --new-session --clearenv`, private `/tmp`, synthetic key, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, `DISABLE_AUTOUPDATER`), `mock_anthropic.py` (loopback Messages API that logs every request body), `in_sandbox.py`; `scripts/research/f1b/h.py` (`codex app-server` JSON-RPC client) | VERIFIED (source)                                              |
| Claude managed policy     | `/etc/claude-code/` exists and is empty                                                                                                                                                                                                                                                                                                                                      | VERIFIED                                                       |
| Codex managed policy      | `/etc/codex/managed_config.toml`: `check_for_update_on_startup = false`, `[features] hooks = true` (managed layer ranks above CLI `-c`)                                                                                                                                                                                                                                      | VERIFIED                                                       |
| Native gate               | `capabilities.rs:32` qualifies only `2.1.309 (Claude Code)` and `codex-cli 0.159.0`; `allows_start_with_profile` (`:130`) refuses overrides for any other build                                                                                                                                                                                                              | VERIFIED (source)                                              |
| Plan token                | `service.rs:685-687`: sha256 of `(agent, profile id, profile ref, resolved body, plan)`; plan includes overlay text; CLI version bound separately via `expected_cli_version`                                                                                                                                                                                                 | VERIFIED (source)                                              |

### 1.2 Discrepancies to resolve

| #   | Discrepancy                                                                                                                                                                                                                                                  | Consequence                                                                                                                                                  |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| D1  | Installed Claude 2.1.312 ≠ qualified 2.1.309                                                                                                                                                                                                                 | Every Native override launch is refused today. A0 group B requalifies; a newer build never inherits evidence.                                                |
| D2  | Claude auto-updates: three builds (2.1.310–312) present                                                                                                                                                                                                      | Exact-build gating will churn. AM1 needs a re-runnable A0 suite per build (§7).                                                                              |
| D3  | Repo policy (`docs/CHECKPOINT.md:33-36`, `docs/research/inference-isolation-2026-10-19.md:3-8`, owner 2026-10-19): "no disposable HOME … older isolated procedures must not be repeated" vs this mission: disposable roots mandatory                         | The current explicit instruction governs A0, but the repo must record the exception or the docs contradict each other. **Approval AP-0.**                    |
| D4  | Repo says Codex skills cannot be excluded per session (`profile.rs:334`, provider-matrix); the 0.159.0 binary contains `skills.config`, `SessionFlags`, and the name-selector warnings `ignoring empty skills.config name override` / `…both path and name…` | SOURCE-SUPPORTED that name and path selectors exist in 0.159.0; which layers are read is NOT_PROVEN. Group E decides.                                        |
| D5  | Codex daemon socket is `/tmp/codex-daemon-1000/<hash>` (link from `~/.codex/app-server-daemon/daemon-updater.sock`), i.e. **outside `CODEX_HOME`, shared per UID**                                                                                           | A test that only swaps `CODEX_HOME` (as `f1b/h.py` and `codex_plan_tests.rs:211` do) may reach the owner's live daemon. Private `/tmp` (bwrap) is mandatory. |
| D6  | Existing ignored real-provider tests use the real HOME/shell (`changes_tests.rs:1097`, `codex_plan_tests.rs:211`, `service_tests.rs:1807`)                                                                                                                   | They would run the owner's real hooks / see real `~/.agents/skills`. Not reusable for A0 as-is.                                                              |
| D7  | Claude docs name `maxSkillDescriptionChars`; 2.1.312 binary has 0 hits for it and contains `skillListingMaxDescChars`                                                                                                                                        | Use binary names; verify in A-9.                                                                                                                             |
| D8  | Claude has a background daemon (`~/.claude/daemon/{roster.json,control.key,dispatch,attach-journal}`, `daemon.log` last entries 2026-10-08, bg/fleet sessions)                                                                                               | Whether a foreground interactive session touches it is ASSUMED no; group D checks.                                                                           |
| D9  | 2.1.303 evidence (`f1-f2a-2026-10-19.md:137`): an invalid `skillOverrides` value makes Claude ignore the **whole** settings file ("did not load")                                                                                                            | Silent fail-open for strict exclusion. Group H must show it is detectable or prevented.                                                                      |
| D10 | 2.1.303 evidence: plugin skills are not hidden by `skillOverrides` (`--plugin-dir` case)                                                                                                                                                                     | The owner has 5 plugins enabled (codex, caveman, ponytail, rust-analyzer-lsp, warp). Strict exclusion of plugin skills depends on A-6.                       |
| D11 | Launcher maps `Permission::Ask` → `--permission-mode manual` (`profile.rs`); `"manual"` occurs 60× in the binary but is not in the documented mode list                                                                                                      | Group B-3 checks it is accepted.                                                                                                                             |

---

## 2. Shared isolated harness (used by every active group)

### 2.1 Layout

```
A0_ROOT = ~/.local/state/wyss-a0/<run-id>/        (0700; on disk, not /tmp: tmpfs filled before)
  harness/      copied probe code (sandbox.py, mock_anthropic.py, new mock_responses.py, pty_driver.py,
                fixtures.py, oracle.py, integrity.py) — never run from inside a fixture dir
  homes/<case>/ disposable HOME per case (bound at /home/probe)
  work/<case>/  disposable project (bound at /work), git-initialised, with AGENTS.md + CLAUDE.md (@AGENTS.md)
  evidence/<group>/<case>/  results.json, requests.jsonl, stdout/stderr, screen captures (text), file-tree hashes
```

The harness lives outside the repository for A0. If adopted, it lands later in a reviewed PR under `scripts/research/a0/`.

### 2.2 Sandbox contract

- `bwrap --unshare-all --die-with-parent --new-session --clearenv` (network, PID, IPC, UTS, user, cgroup namespaces); `--proc /proc --dev /dev --tmpfs /tmp`; system dirs read-only as in `sandbox.py:14-15`; **the real HOME is not mounted**; no `XDG_RUNTIME_DIR`, D-Bus, `DISPLAY`, `WAYLAND_DISPLAY`, SSH agent, keyring.
- Provider binaries: `--ro-bind` of the exact versioned file (Claude) or release directory (Codex) at `/opt/a0/…`; sha256 checked before and after each group.
- `/etc/codex` visible read-only by default (fidelity with the owner's machine); every Codex group has one control run with `--tmpfs /etc/codex`.
- Environment: `HOME=/home/probe`, `XDG_{CONFIG,DATA,STATE,CACHE}_HOME` under it, `PATH=/usr/bin:/opt/a0/bin`, `LANG=C.UTF-8`, `DISABLE_AUTOUPDATER=1`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, `DISABLE_TELEMETRY=1`, `DISABLE_ERROR_REPORTING=1`.
- Synthetic credentials only: Claude `ANTHROPIC_API_KEY=sk-ant-api03-synthetic-wyss-a0-…0` + `ANTHROPIC_BASE_URL=http://127.0.0.1:18080`; Codex disposable `config.toml` with `model_provider = "a0mock"`, `[model_providers.a0mock] base_url = "http://127.0.0.1:18081/v1"`, `wire_api = "responses"`, `env_key = "A0_SYNTH_KEY"`; no `auth.json`, no `.credentials.json`.
- Mocks run inside the namespace (loopback only). `mock_anthropic.py` is reused; `mock_responses.py` (Responses API, SSE) is new and gets its own negative control (malformed SSE → CLI reports an error).
- Interactive variants use a PTY driver inside the sandbox (no display). First-run dialogs are pre-empted by synthetic state (Claude `~/.claude.json` onboarding/trust for `/work`; Codex `[projects."/work"] trust_level = "trusted"`); if a dialog still appears, the driver records the screen and the case is `INCONCLUSIVE`, never auto-answered with a permission-granting key.

### 2.3 Deterministic oracles

- **Canaries:** every fixture skill `a0-<x>` has `description: … CANARY-DESC-<x>-<run>` and a body line `CANARY-BODY-<x>-<run>`; instruction files carry `CANARY-AGENTS-<run>`.
  - _Listed_ = DESC canary appears in a captured request; _loaded_ = BODY canary appears; _instructions kept_ = AGENTS canary appears.
- **Markers:** synthetic hooks `touch /markers/<event>-<case>`; the PreToolUse guard denies any Bash command containing `A0-FORBIDDEN` and touches `/markers/guard-denied-<case>`.
- **Scripted mock responses** drive tool calls deterministically: `Skill`, `Agent`/`Task`, `Bash`, Codex `spawn_agent`/shell. No model decides anything.
- `oracle.py` computes PASS/FAIL from files only; no result is judged by eye.

### 2.4 Real-system integrity snapshot (before and after every group)

`integrity.py`, read-only:

- sha256 of `~/.claude/settings.json`, `~/.claude/settings.local.json`, `~/.claude/AGENTS.md`, `~/.claude.json`, `~/.codex/config.toml`, `~/.codex/hooks.json`, `~/.codex/AGENTS.md`, `/etc/codex/managed_config.toml`, both provider binaries.
- `stat` only (size, mtime, inode — never read) of `~/.claude/.credentials.json`, `~/.codex/auth.json`, `~/.claude/daemon/control.key`.
- Content hash of `~/.codex/app-server-daemon/daemon.pid` and inode of `/tmp/codex-daemon-1000/*`.
- Set of PIDs whose exe is a provider binary (from `/proc/*/exe`, read-only).

PASS = identical, except PIDs the owner started or ended (recorded as owner activity, never acted on).

### 2.5 Global immediate-stop conditions

Abort the run, keep evidence, report `BLOCKED` when:

- **S1** Any integrity value in 2.4 changes in a way the harness could have caused.
- **S2** A process with an exe under `/opt/a0` or `A0_ROOT` exists after the sandbox exits.
- **S3** The mock receives a request carrying a non-synthetic credential, or the sandbox has a non-loopback route.
- **S4** bwrap cannot create the namespaces. There is no unsandboxed fallback.
- **S5** A provider prompts for login/OAuth, opens a browser, or tries keyring access.
- **S6** Disk use of `A0_ROOT` > 2 GiB.
- **S7** A provider binary hash changes.

---

## 3. Test matrix

**Per-group conventions** (unless stated otherwise):

- _Preconditions:_ §2 harness ready; S0 integrity snapshot taken; the group's approval recorded.
- _Cleanup:_ the sandbox exits (PID namespace dies); delete `A0_ROOT/homes/<case>` and `A0_ROOT/work/<case>` only (paths created by the harness); keep `evidence/`.
- _Rollback:_ nothing outside `A0_ROOT` is written, so rollback = deleting `A0_ROOT`.
- _Stop:_ §2.5.
- _Modes:_ **H** = headless `-p`/`exec`; **I** = interactive PTY (how Native launches).

### A. Claude per-session overlays and skill exclusion (2.1.312) — approval AP-CL

Fixtures: user skills `a0-alpha/beta/gamma` in `~/.claude/skills`; project skill `a0-proj` in `/work/.claude/skills`; session plugin `a0sess` (`.claude-plugin/plugin.json` + `skills/a0-delta`) at `/a0/session-plugin`; overlay at `/a0/overlay.json` passed as `--settings`.

| ID   | Purpose                                 | Setup / command                                                                                                                                                                      | Positive expectation                                                                                      | Negative control                                                                | Evidence                                                 |
| ---- | --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | -------------------------------------------------------- |
| A-1  | Oracle sensitivity                      | No overlay; `claude -p ping` (H) and PTY `ping` (I)                                                                                                                                  | DESC α β γ proj listed; no BODY canary                                                                    | Skill without frontmatter `a0-bad` → not listed                                 | requests.jsonl, canary table                             |
| A-2  | `off` hides and refuses                 | overlay `{"skillOverrides":{"a0-beta":"off"}}`; `-p ping`; `-p "/a0-beta"`                                                                                                           | β DESC absent, α γ present; `/a0-beta` refused with "disabled via skillOverrides" and **0** mock requests | Same run without `--settings` → β present                                       | requests, stderr, exit codes                             |
| A-3  | `name-only`                             | `{"a0-beta":"name-only"}`                                                                                                                                                            | `a0-beta` name present, β DESC absent                                                                     | value `"on"` → DESC present                                                     | requests                                                 |
| A-4  | `user-invocable-only`                   | `{"a0-beta":"user-invocable-only"}`                                                                                                                                                  | not listed; `/a0-beta` accepted (reaches mock)                                                            | —                                                                               | requests, exit                                           |
| A-5  | Project skill exclusion                 | `{"a0-proj":"off"}`                                                                                                                                                                  | proj DESC absent                                                                                          | without overlay → present                                                       | requests                                                 |
| A-6  | Plugin skill exclusion                  | Disposable marketplace-style install of plugin `a0plug` (installed_plugins.json + cache + `enabledPlugins:{"a0plug@a0mk":true}`); overlay `{"enabledPlugins":{"a0plug@a0mk":false}}` | plugin skill DESC absent (if this fails: plugin skills are **ADVISORY**)                                  | overlay `skillOverrides` on the plugin skill → still listed (2.1.303 behaviour) | requests; fixture-fidelity note                          |
| A-7  | Session plugin adds skills              | `--plugin-dir /a0/session-plugin`                                                                                                                                                    | `a0sess:a0-delta` listed; `/a0sess:a0-delta` invocable                                                    | dir absent → not listed, no crash                                               | requests, stderr                                         |
| A-8  | **Exact selected set** (core AM1 claim) | Overlay `off` for every user/project skill except the selection + session plugin with copied selected skills; H and I; 3 repetitions                                                 | Listed skill set == expected set ∪ provider built-ins (built-ins recorded separately from A-1 diff)       | Remove one overlay entry → that skill reappears                                 | set diff per repetition                                  |
| A-9  | Budget / truncation                     | 150 synthetic user skills; baseline vs strict selection; variant overlay `skillListingBudgetFraction`, `skillListingMaxDescChars`                                                    | Baseline shows truncation (some DESC missing); strict selection shows all selected DESC                   | —                                                                               | counts of DESC canaries                                  |
| A-10 | Child agents                            | Mock scripts `Agent` tool_use (general-purpose); agent fixture `a0-child` with `skills: [a0-alpha]`                                                                                  | Child request: excluded skills absent from its listing; `a0-child` request contains BODY α (preload)      | Agent without `skills:` → no BODY α                                             | parent/child requests (by request order + system prompt) |

### B. Claude 2.1.312 and the currently allowlisted launch words — approval AP-CL

Words emitted by `profile::plan`: `--model`, `--effort {low,medium,high,xhigh,max}`, `--permission-mode {manual,plan,acceptEdits}`, `--settings <file>`, plus the first-message brief word.

| ID  | Purpose                             | Command                                                                             | Positive                                                                                  | Negative                                      | Evidence               |
| --- | ----------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | --------------------------------------------- | ---------------------- |
| B-1 | `--model`                           | `--model a0-model-x -p ping`                                                        | request `model` == `a0-model-x`                                                           | —                                             | request                |
| B-2 | `--effort`                          | each of the 5 levels                                                                | each accepted; request reflects the level (record the field)                              | `--effort bogus` → error exit, 0 requests     | requests, exit         |
| B-3 | `--permission-mode`                 | `manual`, `plan`, `acceptEdits`; mock scripts an `Edit` tool_use                    | `plan` blocks the edit; `manual` accepted as a mode (else **D11 becomes a launcher bug**) | `--permission-mode bogus` → error             | tool_result text, exit |
| B-4 | `--settings` file form + brief word | overlay path + `"# brief…"` as positional prompt (I)                                | overlay applied (A-2 oracle) and first request carries the brief                          | —                                             | requests               |
| B-5 | Version-guard recipe                | Wyss `recipe_with_version_guard` words run inside the sandbox with `/opt/a0` binary | guard passes for `2.1.312 (Claude Code)` string                                           | expected `2.1.309` → guard aborts before exec | exit, stderr           |

### C. Mandatory security hooks and permission controls — approval AP-CL (C-6/C-7: AP-CX)

Fixture user `settings.json`: SessionStart + UserPromptSubmit marker hooks, PreToolUse guard, `permissions.deny: ["Bash(rm:*)"]`.

| ID  | Purpose                                     | Setup                                                                                | Positive                                                                                   | Negative control                                                                           | Evidence                                                                                        |
| --- | ------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------- |
| C-1 | Hooks survive Arsenal overlay               | A-8 overlay; H and I                                                                 | all markers present                                                                        | overlay `disableAllHooks:true` → markers absent (proves detection; Arsenal never emits it) | markers                                                                                         |
| C-2 | Guard still denies                          | mock scripts `Bash "echo A0-FORBIDDEN > /markers/ran"`                               | `guard-denied` marker present, `/markers/ran` absent, denial in next request's tool_result | guard removed → `/markers/ran` present                                                     | markers, request                                                                                |
| C-3 | `permissions.deny` kept                     | mock scripts `Bash "rm /work/x"`                                                     | denied                                                                                     | —                                                                                          | tool_result, file exists                                                                        |
| C-4 | Overlay hooks add, not replace              | overlay with its own SessionStart marker                                             | both user and overlay markers                                                              | —                                                                                          | markers (result decides whether the compiler forbids `hooks` keys outright; it does regardless) |
| C-5 | No escalation by overlay                    | `--permission-mode plan` + A-8 overlay                                               | edit still blocked                                                                         | —                                                                                          | tool_result                                                                                     |
| C-6 | Codex hooks unchanged by `-c skills.config` | disposable `hooks.json`; `app-server` `hooks/list` with and without the E-2 override | identical list (event, enabled, trustStatus)                                               | `--tmpfs /etc/codex` control shows the managed layer's effect                              | JSON diff                                                                                       |
| C-7 | Codex sandbox/approval unchanged            | `-s read-only -a on-request` + skills override; `config/read`                        | sandbox/approval values equal to the run without the override                              | —                                                                                          | config/read JSON                                                                                |

Limitation: Codex hook _execution_ needs persisted hook trust. A0 does not seed trust and never uses `--dangerously-bypass-hook-trust`, so C-6 proves definitions and enablement, not execution (NOT_PROVEN, stated).

### D. Two concurrent Claude sessions, different selections — approval AP-CL-D

Same disposable HOME (as in the product: one user, one HOME). Two PTY sessions S1 (`{α}`) and S2 (`{β}`), each with its own overlay, session plugin and mock port (18080 / 18082), started 2 s apart; 3 interleaved turns each; variants: same `/work` vs different projects.

| ID  | Purpose                | Positive                                                                                                                     | Negative / stop                                         | Evidence                 |
| --- | ---------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- | ------------------------ |
| D-1 | No selection crossover | every S1 request lists only S1's set, S2 only S2's                                                                           | any crossover → AM1 is single-session only (blocker B6) | per-port requests        |
| D-2 | Shared-state writes    | user `settings.json` byte-identical; record every file both sessions write (`~/.claude.json`, history, sessions/, projects/) | user settings changed → stop                            | tree hashes before/after |
| D-3 | Overlay independence   | rewrite S1's overlay mid-run; S2's next request unchanged                                                                    | S2 changes → stop                                       | requests                 |
| D-4 | Background daemon      | no process other than the two CLIs and their children in the PID namespace; `~/.claude/daemon` not created                   | daemon appears → record and re-run D-1 against it       | `/proc` listing, tree    |

### E. Codex `-c` skill overrides (0.159.0) — approval AP-CX

Fixtures: `~/.agents/skills/a0-{alpha,beta,gamma}` and `/work/.agents/skills/a0-proj`; disposable `config.toml` (mock provider, trusted `/work`, `[features] multi_agent = true`).

Three observation channels per case:

1. `app-server` `skills/list` (resolution, read-only).
2. `codex debug prompt-input` (present in the 0.159.0 binary; renders the model input — whether it needs auth or applies `-c` is itself checked in E-1).
3. `codex exec ping` against the mock (the actual request).

| ID   | Purpose                      | Override                                                                       | Positive                                                                               | Negative control                                                                              |
| ---- | ---------------------------- | ------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| E-1  | Baseline + channel agreement | none                                                                           | all skills enabled; DESC α β γ proj in channels 2 and 3                                | skill without frontmatter not listed                                                          |
| E-2  | Name selector                | `-c 'skills.config=[{name="a0-beta",enabled=false}]'`                          | β disabled (1), absent (2,3)                                                           | override omitted → present                                                                    |
| E-3  | Path selector                | `[{path="/home/probe/.agents/skills/a0-beta/SKILL.md",enabled=false}]`         | same as E-2                                                                            | wrong path → β present                                                                        |
| E-4  | Non-canonical path           | path through a symlinked dir                                                   | disabled (canonicalized) or documented miss                                            | —                                                                                             |
| E-5  | **Merge with user layer**    | user `config.toml` disables γ; session disables β                              | β and γ both disabled                                                                  | if γ re-appears, `-c` replaces the user array → Arsenal must re-emit owner disables (finding) |
| E-6  | Re-enable precedence         | user disables γ; session `{name="a0-gamma",enabled=true}`                      | records which wins (policy: Arsenal never re-enables an owner-disabled skill silently) | —                                                                                             |
| E-7  | Both selectors               | `{path=…,name=…,enabled=false}`                                                | warning "both path and name" on stderr, entry ignored                                  | —                                                                                             |
| E-8  | Malformed value              | `-c 'skills.config=[{name="a0-beta",enabled=fals]'` (falls back to raw string) | error exit **or** detectable warning; never a silent "applied"                         | fail-open without signal → blocker B2-Codex                                                   |
| E-9  | Unknown name                 | `{name="a0-nope",enabled=false}`                                               | no effect on others; record whether it warns                                           | —                                                                                             |
| E-10 | Catalog budget               | 150 skills; `-c skills.max_context_tokens=500`                                 | truncation visible in (2,3)                                                            | without override → larger catalog                                                             |
| E-11 | Instructions block           | `-c skills.include_instructions=false`                                         | skills instruction block absent                                                        | —                                                                                             |
| E-12 | Child inheritance            | role file `~/.codex/agents/a0-child.toml`; mock scripts `spawn_agent`          | child request lacks β (inherits session flags)                                         | role layer `{name="a0-alpha",enabled=false}` → child also lacks α                             |
| E-13 | Project skills               | `{name="a0-proj",enabled=false}`                                               | proj absent                                                                            | —                                                                                             |
| E-14 | Managed layer interaction    | repeat E-2 with `--tmpfs /etc/codex`                                           | same result                                                                            | difference → managed layer matters, record                                                    |

Evidence: JSON-RPC transcripts, prompt-input output, requests, stderr, exit codes.

### F. Codex shared background server — approval AP-CX-F (interactive TUI, daemon)

| ID  | Purpose                              | Procedure                                                                                                          | Positive                                                                                                                                                     | Negative / stop                                                                 | Evidence                                |
| --- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------- | --------------------------------------- |
| F-1 | Where the daemon lives               | PTY TUI A in sandbox (private `/tmp`)                                                                              | `/tmp/codex-daemon-<uid>/<hash>` and `CODEX_HOME/app-server-daemon/daemon.pid` created **inside** the sandbox; hash recorded for two different `CODEX_HOME`s | real socket inode/`daemon.pid` changes → S1                                     | tree listing, integrity                 |
| F-2 | **Cross-session contamination**      | A with `-c` disable β, then B (same `CODEX_HOME`) with `-c` disable γ; one prompt each; both orders; 3 repetitions | A's requests lack only β, B's only γ                                                                                                                         | any thread showing the other's config → daemon contaminates (blocker; OD-AR-06) | per-thread requests (thread id in body) |
| F-3 | Daemon pre-started without overrides | start `codex app-server`/`codex agents` first, then TUI with `-c`                                                  | TUI thread honours its `-c`                                                                                                                                  | ignored → record                                                                | requests                                |
| F-4 | `--no-daemon` control                | F-2 with `--no-daemon` on both                                                                                     | isolation holds                                                                                                                                              | —                                                                               | requests                                |
| F-5 | Lifetime                             | after both TUIs exit, list daemon processes before teardown                                                        | recorded (survives or not)                                                                                                                                   | —                                                                               | `/proc` listing                         |
| F-6 | Updater                              | daemon-updater behaviour with `/etc/codex` visible and hidden                                                      | no network attempt succeeds; `check_for_update_on_startup=false` respected when visible                                                                      | —                                                                               | stderr logs                             |

### G. Provider-visible vs actually loaded / invoked; change timing — approval AP-CL (G-5, G-9: AP-CX)

| ID   | Purpose                                           | Procedure                                                                                                         | Positive                                                                                                       | Negative                                  |
| ---- | ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| G-1  | Listing ≠ loading (Claude)                        | A-1                                                                                                               | DESC present, BODY absent                                                                                      | —                                         |
| G-2  | Skill invocation loads body                       | mock scripts `Skill{skill:"a0-alpha"}`                                                                            | next request contains BODY α                                                                                   | —                                         |
| G-3  | Excluded skill cannot be invoked via Skill tool   | `Skill{skill:"a0-beta"}` with β `off`                                                                             | error tool_result, no BODY β                                                                                   | β `on` → BODY β                           |
| G-4  | **Exclusion is not access control**               | mock scripts `Read ~/.claude/skills/a0-beta/SKILL.md` with β `off`                                                | records whether the file is readable (expected readable) → UI must say "hidden from the list", never "blocked" | —                                         |
| G-5  | Codex: same for catalog vs file read              | E-2 + scripted shell `cat …/a0-beta/SKILL.md`                                                                     | records readability; catalog-only exclusion                                                                    | —                                         |
| G-6  | Transcript as observation source                  | after G-2/A-10, inspect disposable `~/.claude/projects/<p>/<id>.jsonl` and `<id>/subagents/*.jsonl`               | Skill tool_use, preload, child records present → AM1 observer fixtures (synthetic content only)                | —                                         |
| G-7  | **Next-turn change** (Claude)                     | I-mode session; after turn 1, rewrite the overlay to hide α; edit the session-plugin SKILL.md DESC canary; turn 2 | records whether turn 2's listing changed (→ "next turn" or "restart required")                                 | —                                         |
| G-8  | Resume semantics                                  | end session; `--resume <id>` with a different overlay; variant `--system-prompt-snapshot off`                     | records whether the first resumed request shows the new or old listing                                         | —                                         |
| G-9  | Codex mid-session                                 | change skill files on disk; `skills/list` with `forceReload`                                                      | records reload behaviour; `-c` cannot change in-process (ASSUMED, checked)                                     | —                                         |
| G-10 | **Listing in transcript** (AM1 verification path) | after A-8, search the disposable transcript for DESC canaries / listing attachment                                | if present: AM1 can verify provider-visible skills read-only from real transcripts                             | absent: AM1 needs a pre-launch probe (§7) |

### H. Failure behaviour — approval AP-CL / AP-CX

| ID   | Case                                                                                 | Expected safe behaviour                                                        | Fail-open handling                                                                          |
| ---- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| H-1  | `skillOverrides` value `bogus`                                                       | (2.1.303) whole overlay ignored with "did not load"                            | Record where the warning appears (stderr in H, screen text in I). Undetectable → blocker B2 |
| H-2  | Malformed JSON overlay                                                               | error exit or explicit warning                                                 | same                                                                                        |
| H-3  | Overlay path missing / mode 000                                                      | error exit                                                                     | same                                                                                        |
| H-4  | Override for non-existent skill                                                      | ignored, others unaffected                                                     | —                                                                                           |
| H-5  | Session plugin: missing `plugin.json`, SKILL.md without frontmatter, duplicate names | plugin or skill skipped with warning; session starts                           | record                                                                                      |
| H-6  | Same name in session plugin and user dir, user one `off`                             | only `a0sess:<name>` listed                                                    | record shadowing                                                                            |
| H-7  | Different build (optional, AP-H7)                                                    | run A-2/A-8 against on-disk 2.1.311                                            | results recorded as 2.1.311 evidence only; proves the gate is per build                     |
| H-8  | Codex malformed `-c` (E-8), `--strict-config` with unknown `-c` key                  | error or warning                                                               | same                                                                                        |
| H-9  | Codex selector to missing path                                                       | warning, no effect                                                             | record                                                                                      |
| H-10 | Conflicting capability names across user/project (both providers)                    | provider's precedence recorded (feeds conflict rule C6)                        | —                                                                                           |
| H-11 | Wyss side (unit, no provider)                                                        | stale plan token, unverified build, tampered materialized file → start refused | see J                                                                                       |

### I. Preservation — no extra approval (it runs inside every group)

| ID                    | Check                                                                                                               | Pass                                                                                                                                                                                                                                     |
| --------------------- | ------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I-1                   | Real-system integrity (§2.4), before/after each group                                                               | identical                                                                                                                                                                                                                                |
| I-2                   | Disposable user config untouched by providers when overlays are used (`settings.json`, `config.toml`, `hooks.json`) | byte-identical; provider state files (`~/.claude.json`, sqlite) listed as expected writes                                                                                                                                                |
| I-3                   | Project instructions                                                                                                | `/work/AGENTS.md`, `CLAUDE.md` byte-identical; `CANARY-AGENTS` present in requests under every overlay (overlay does not drop `@AGENTS.md`)                                                                                              |
| I-4                   | Authentication                                                                                                      | real credential files: stat unchanged; no real credential mounted or read                                                                                                                                                                |
| I-5                   | Running sessions                                                                                                    | real provider PID set and live Codex daemon `daemon.pid` unchanged by the harness; harness never signals a PID outside its namespace                                                                                                     |
| I-6 (optional, AP-I6) | Separate-roots feasibility, synthetic only                                                                          | F1b-E3-style: synthetic `auth.json`/`.credentials.json` reached through a link from a second root; record whether the provider writes through or replaces the link. Run only if the owner wants the separate-roots alternative evaluated |

### J. Preview and plan-token compatibility — no provider; static review + unit tests designed now, run in AM1

| ID  | Check                      | Expectation                                                                                                                                                                                          |
| --- | -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| J-1 | Token coverage             | Today: `(agent, profile id, profile ref, resolved, plan)` (`service.rs:685-687`). AM1 adds: selection revision, each capability revision `tree_hash`, materialized-tree hash, provider binary sha256 |
| J-2 | Golden compile             | AM1 compiler output for the A-8 selection is byte-equal to the overlay/plugin that passed A-8 (A0 artefacts become golden files)                                                                     |
| J-3 | Tamper                     | edit a materialized file between preview and start → start refused (new code, same family as `profile_preview_stale`)                                                                                |
| J-4 | Build drift                | preview on 2.1.312, binary becomes 2.1.313 → `provider_version_unverified` (existing; regression test)                                                                                               |
| J-5 | Advisory vs not-applicable | today any `not_applicable` blocks start (`service.rs:780-785`); Arsenal needs a non-blocking **ADVISORY** list requiring explicit acknowledgement, never folded into `applied`                       |
| J-6 | Validation before write    | compiler rejects any `skillOverrides` value outside `{on,name-only,user-invocable-only,off}` and any `hooks`, `disableAllHooks`, `permissions` key (protects against H-1/C-4)                        |

---

## 4. Answers the evidence must produce (architectural questions)

| #   | Question                                                                   | Evidence that decides it                                                                                                                                       | Current class                                                                        |
| --- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| 1   | Claude exposes exactly the selected skills while keeping security controls | A-8 (3/3, H+I), A-6, C-1..C-5, H-1 detectable, I-3                                                                                                             | SOURCE-SUPPORTED (2.1.303 VERIFIED for user/project `off`; plugin skills not hidden) |
| 2   | Codex excludes skills per session safely with `-c`                         | E-2, E-3, E-5, E-8 across all three channels; C-6/C-7                                                                                                          | SOURCE-SUPPORTED (strings in 0.159.0); behaviour NOT_PROVEN                          |
| 3   | Codex background processes compromise isolation                            | F-2 both orders 3/3, F-3, F-4                                                                                                                                  | NOT_PROVEN; D5 shows the socket is shared per UID                                    |
| 4   | Separate config roots needed / auth-safe                                   | Not needed if Q1 and Q2 pass. Separate roots need credential material per root, which is AUTH-MODIFYING; I-6 only measures link behaviour with synthetic files | ASSUMED not needed; the product keeps rejecting separate roots (design ALT-1)        |
| 5   | What can change mid-conversation                                           | G-7 (next turn?), G-8 (resume), G-9 (Codex)                                                                                                                    | NOT_PROVEN; the design assumes "restart required" until G-7 shows otherwise          |
| 6   | Not verifiable without real inference                                      | see below                                                                                                                                                      | —                                                                                    |

Not verifiable with the mock (needs real inference; outside A0):

- Whether a real model **chooses** the right skill from a reduced or `name-only` listing (trigger accuracy).
- Whether skill text changes the model's behaviour or output quality.
- Real token usage and billing figures (mock `usage` is synthetic). Real prompt-cache effects.
- Server-side handling of the request (e.g. provider-side truncation or tool filtering).
- Whether a real orchestrator (GSD) actually spawns the child it is expected to.

Everything about **what the CLI sends, lists, loads on a tool call, and refuses** is verifiable offline with the mock.

---

## 5. Risk assessment and safety gates

| Risk                                            | Mitigation                                                                           | Residual                                                               |
| ----------------------------------------------- | ------------------------------------------------------------------------------------ | ---------------------------------------------------------------------- |
| Hooks/plugins/init code run at startup          | Only synthetic hooks exist in the disposable HOME; real HOME not mounted             | Provider built-in startup code still runs (contained by the namespace) |
| Network egress / billable call                  | `--unshare-all` (no route except loopback); synthetic key; mock answers              | None while S3 holds                                                    |
| Reaching the owner's live Codex daemon          | private `/tmp`; real socket inode checked (I-5)                                      | None if S1 holds                                                       |
| Credential exposure                             | No real credential mounted; `--clearenv`; stat-only checks                           | None                                                                   |
| Auto-update changing a binary                   | read-only bind of versioned path; `DISABLE_AUTOUPDATER`; no network; hash check (S7) | —                                                                      |
| Orphan processes                                | PID namespace + `--die-with-parent`; S2 check                                        | —                                                                      |
| Harness bug writes to real paths                | real HOME not mounted; integrity snapshot                                            | Writes to `A0_ROOT` only                                               |
| Fixture infidelity (plugins, first-run dialogs) | Mark cases `INCONCLUSIVE`, never PASS by assumption                                  | A-6 may stay NOT_PROVEN                                                |
| Policy conflict D3                              | AP-0 records the exception                                                           | —                                                                      |
| Disk fill                                       | `A0_ROOT` on disk, 2 GiB cap (S6)                                                    | —                                                                      |
| Nested KWin display                             | Not used: PTY inside the sandbox is enough for A0                                    | GUI acceptance stays a separate AM1 gate                               |

Execution order (each step gated by the previous step's PASS):

1. P0 prerequisites: bwrap namespace smoke with `/usr/bin/true` (no provider); integrity S0.
2. A-1 and E-1 (oracle and channel sanity).
3. Remaining A, B, C, H(Claude), G(Claude).
4. D.
5. E, C-6/C-7, H(Codex), G-5/G-9.
6. F.
7. Optional I-6, H-7.

Each group's report is reviewed before the next group starts.

---

## 6. Evidence and acceptance thresholds

Per case, `results.json` records:

- `case_id`, `group`, `mode`, binary path and sha256, full argv (synthetic only), env keys (names only).
- exit code, stdout/stderr files with hashes, number of mock requests.
- canary hits `{desc[], body[], agents}`, markers present, integrity diff, duration, verdict and reason.

A group summary table is copied to this folder; raw evidence stays in `A0_ROOT/evidence`.

| Threshold         | Value                                                                                                                                                                                          |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Determinism       | Each core case (A-2, A-8, C-1, C-2, D-1, E-2, E-3, E-5, F-2) passes **3/3** repetitions; any flip = FAIL, never majority                                                                       |
| Mode parity       | A-8, C-1, E-2 pass in both H and I where both exist                                                                                                                                            |
| Negative controls | Every negative control must produce its expected _different_ result; a control that passes like the positive invalidates the case (oracle blind)                                               |
| Integrity         | 100 % identical (I-1) for the whole run                                                                                                                                                        |
| Fail-open         | H-1/H-2/E-8 either error out or emit a detectable signal that a deterministic parser recognises; otherwise the mechanism is not ENFORCED                                                       |
| Capability record | A mechanism becomes **ENFORCED** for an exact build only with all its core cases PASS; partial results become **ADVISORY** with the failing case named; no result is extended to another build |

---

## 7. Owner approvals required

| ID                      | Covers                                                                                                                                                              | Executes                                                                     | Risk level                           |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ------------------------------------ |
| **AP-0**                | Exception to the 2026-10-19 "no disposable HOME" policy for A0 only; harness outside the repo; deletion of harness-created subtrees under `~/.local/state/wyss-a0/` | bwrap smoke (no provider)                                                    | R1                                   |
| **AP-CL**               | Groups A, B, C-1..C-5, G-1..G-4, G-6..G-8, G-10, H-1..H-6, H-10 (Claude part)                                                                                       | installed Claude 2.1.312 inside bwrap, synthetic key, loopback mock, H + PTY | R1 (disposable), no billing possible |
| **AP-CL-D**             | Group D                                                                                                                                                             | two concurrent Claude sandboxed sessions                                     | R1                                   |
| **AP-CX**               | Groups E, C-6, C-7, G-5, G-9, H-8, H-9, H-10 (Codex part)                                                                                                           | Codex 0.159.0 `app-server`, `debug prompt-input`, `exec` inside bwrap        | R1                                   |
| **AP-CX-F**             | Group F                                                                                                                                                             | Codex interactive TUI + its daemon inside bwrap                              | R1                                   |
| AP-H7 (optional)        | H-7                                                                                                                                                                 | Claude 2.1.311 binary inside bwrap                                           | R1                                   |
| AP-I6 (optional)        | I-6                                                                                                                                                                 | synthetic-auth link behaviour                                                | R1                                   |
| AP-GUI (not part of A0) | Nested KWin display acceptance of the AM1 GUI                                                                                                                       | Wyss GUI in nested display                                                   | separate gate, AM1                   |

---

## 8. AM1 — refined dependencies, readiness criteria and blockers

AM1 scope stays:

- versioned store;
- copy import of Claude **user** skills;
- terminal/session-scoped selection UI;
- effective-configuration preview;
- Claude Native launch integration;
- provider-visible verification;
- regression and security acceptance.

Excluded: Android, remote, Managed, full GSD orchestration, Codex enforcement (A6 follows A0-E/F).

| AM1 part                             | Depends on (A0)                                                  | Readiness criterion                                                                                                                                                                                          |
| ------------------------------------ | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Store (A1)                           | —                                                                | Unit: atomic writes, caps, tombstones, crash injection                                                                                                                                                       |
| Copy import (A2, Claude user skills) | A-1 oracle (fixture shapes)                                      | Real-HOME read-only acceptance: 0 source bytes changed, 0 secrets imported                                                                                                                                   |
| Selection + compiler                 | A-2, A-3, A-5, A-7, A-8, J-6                                     | Golden files from passing A-8; compiler refuses forbidden keys                                                                                                                                               |
| Plugin-skill handling                | A-6                                                              | ENFORCED if A-6 passes; otherwise the UI labels plugin skills ADVISORY and excludes them from "exact set" claims                                                                                             |
| Preview                              | J-1, J-5, A-9                                                    | Token covers all inputs; ADVISORY list distinct from `applied`/`not_applicable`; estimate shows predicted truncation                                                                                         |
| Launch integration                   | B-1..B-5, C-1..C-5, I-3, a capability record for the exact build | Only builds with a record launch; a new build launches as configured (no overrides) until its own A0 rerun passes                                                                                            |
| Provider-visible verification        | G-10 (or a pre-launch probe)                                     | If G-10 passes: read-only transcript check after launch. If not: a sandboxed pre-launch listing probe (this harness productized, separate approval). Without either, AM1 may say "applied", never "verified" |
| Change timing UI                     | G-7, G-8                                                         | Labels derived from results; default "restart as new conversation"                                                                                                                                           |
| Concurrency                          | D-1..D-4                                                         | If D-1 fails: one Arsenal Claude session per HOME, enforced                                                                                                                                                  |
| Requalification lane                 | whole A0 suite automated                                         | Re-run on a new build in ≤ 15 min unattended after the owner's per-build OK; output = capability record + report; **never auto-authorized by version number**                                                |

Blockers that would make AM1 unsafe or misleading:

- **B1** A-8 fails (an exact set cannot be expressed) → strict mode cannot be claimed; redesign.
- **B2** H-1/H-2 fail-open is undetectable → strict claims are unsafe; AM1 must not ship strict mode.
- **B3** C-1..C-3 show an overlay can drop a user hook or permission → stop AM1.
- **B4** G-4 shows a hidden skill is still readable → not a blocker, but every UI string must say "hidden from the agent's list", not "blocked".
- **B5** A-6 fails → plugin skills are ADVISORY. Not a blocker if labelled.
- **B6** D-1 crossover → multi-session Arsenal is a blocker; single-session AM1 is allowed.
- **B7** AP-0 not granted → A0 cannot run; AM1 stays blocked by D1 (no qualified build).
- **B8** Claude updates again before AM1 ships → the requalification lane is mandatory, not optional.

---

## 9. Summary by evidence class

- **VERIFIED:**
  - installed builds and hashes;
  - GSD Core 1.15.0 active, 1.42.30 residue;
  - Codex daemon socket shared under `/tmp/codex-daemon-1000`;
  - `/etc/codex` managed config;
  - empty `/etc/claude-code`;
  - existing bwrap/mock harness;
  - 2.1.303 behaviour: `off` refuses user/project skills, plugin skills unaffected, invalid value drops the whole file;
  - D1, D3, D5, D6 discrepancies.
- **SOURCE-SUPPORTED:**
  - Claude 2.1.312 contains `skillOverrides`, `name-only`, `user-invocable-only`, `skillListingBudgetFraction`, `skillListingMaxDescChars`, `disableAllHooks`, `strict-mcp-config`, `plugin-dir`, `enabledPlugins`, `system-prompt-snapshot`, `SubagentStart`, `"disabled via skillOverrides"`, `"did not load"`;
  - Codex 0.159.0 contains `skills.config`, `SessionFlags`, both selector warnings, `max_context_tokens`, `include_instructions`, `no-daemon`, `debug prompt-input`, `forceReload`;
  - upstream: Codex children clone the session layer; Claude subagents get content only via `skills:`.
- **ASSUMED:**
  - `--help`/`--version` run no hooks;
  - foreground Claude does not use its background daemon;
  - separate config roots are unnecessary;
  - `-c` cannot change in a running Codex process.
- **NOT_PROVEN:**
  - every 2.1.312 behaviour;
  - every Codex `-c` skill behaviour and daemon scope;
  - plugin exclusion via `enabledPlugins`;
  - next-turn reload;
  - resume semantics;
  - listing presence in transcripts;
  - Codex hook execution under overrides.

A0_PLAN_READY_FOR_OWNER_APPROVAL
