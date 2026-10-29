# Wyss Arsenal — A0 Claude Code 2.1.312 skill isolation and hook preservation

Run `20261029-pilot-e98a8e` · 2026-10-29 · scope A-2, A-8 and C-1..C-3 (owner turn `turn:746733c8-…`).
Evidence: `~/.local/state/wyss-a0/20261029-pilot-e98a8e/evidence/isolation/` — `MATRIX.json`, per-case `result.json` / `judgement.json` / `command.txt` / `spec.json` / `overlay.json` / `markers/requests.jsonl`, `PRE-REGISTERED-CASES.json`, `PRE-REGISTERED-C2v2.json`, preflights.
Pre-registration: `evidence/ISOLATION-BATCH-PREREGISTRATION.md`.
Repository untouched: `main` `e9210d90f14ec826cd8cc6f2ad20148a30281867`, clean.

Not done: no Codex, no A-6, no real inference or credentials, no allowlist change, no AM1 code.

**A0_CLAUDE_ISOLATION_REVIEW_REQUIRED**

## 1. Safety envelope

- **Preflight**: 31/31 sandbox checks before the batch, and again before the C-2 v2 pair; the host negative control fails 19.
- **Launches**: 26 provider launches, all in bwrap with `--unshare-all`, empty launcher env, read-only root, synthetic key, loopback mock.
- **Per case**:
  - real-system integrity PASS;
  - no leftover processes;
  - synthetic key only;
  - proxy attempts only within the known baseline;
  - user/project-owned fixture files (`~/.claude/settings.json`, skill trees, `/work/.claude`, `AGENTS.md`, `CLAUDE.md`) byte-identical;
  - disposable trees removed.
- **Day**:
  - first snapshot vs final: PASS. Hashed configs, credential `stat`, Codex daemon state and binaries unchanged; provider PIDs = {224568} (this session).
  - `~/.claude.json` differs only because this live session writes it; the sandbox never mounts it.
- **Stop conditions**: none triggered.

## 2. Pass/fail matrix

H = headless `-p`, I = interactive PTY. "Listed" = fixture skills in the model-visible listing (first `POST /v1/messages`, system-reminder in `messages`).

| Case       | Purpose                                     | Overlay validator | Result                             | Evidence                                                                                                            |
| ---------- | ------------------------------------------- | ----------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| A2-01      | user skill `a0-beta` off                    | accept            | **PASS**                           | listed α γ proj; β DESC absent; no BODY; AGENTS present                                                             |
| A2-02      | project skill `a0-proj` off                 | accept            | **PASS**                           | listed α β γ; proj absent                                                                                           |
| A2-03      | `/a0-beta` while off                        | accept            | **PASS**                           | 0 requests; stdout `Skill "a0-beta" is disabled via skillOverrides…`; body never sent                               |
| A2-03c     | control: `/a0-beta` without overlay         | —                 | **PASS**                           | BODY β sent (invocation path works)                                                                                 |
| A8-S1 ×3 H | selection {α, proj}                         | accept            | **PASS 3/3**                       | listed exactly `[a0-alpha, a0-proj]`                                                                                |
| A8-S2 ×3 H | selection {β, γ}                            | accept            | **PASS 3/3**                       | listed exactly `[a0-beta, a0-gamma]`                                                                                |
| A8-S1 I    | S1, interactive                             | accept            | **PASS**                           | same set; clean exit                                                                                                |
| C1-pos     | user hooks fire under valid overlay         | accept            | **PASS**                           | `hook-SessionStart`, `hook-UserPromptSubmit`                                                                        |
| C1-neg     | control: overlay + `disableAllHooks`        | **reject**        | **PASS (control)**                 | markers absent → an accepted overlay _could_ disable hooks                                                          |
| C2 v1      | guard under valid overlay                   | accept            | positive PASS; **control INVALID** | the control target was outside the workdir, so Claude's permission layer blocked it too (design error, fixed in v2) |
| C2v2-pos   | guard blocks `touch /work/ran-A0-FORBIDDEN` | accept            | **PASS**                           | tool_result `PreToolUse:Bash hook error: … A0 guard: command blocked`; file absent; `guard-denied`                  |
| C2v2-neg   | control: no guard                           | accept            | **PASS**                           | file created; no `guard-denied`                                                                                     |
| C3-pos     | `permissions.deny Bash(rm:*)` holds         | accept            | **PASS**                           | tool_result `Permission to use Bash with command rm /work/a0-victim has been denied.`; file kept                    |
| C3-neg     | control: `rm` allowed                       | accept            | **PASS**                           | file removed                                                                                                        |

Failure behaviour (overlays deliberately unsafe; all rejected by the validator before launch in the product):

| Case  | Overlay                     | Provider behaviour                                                                                                                                                                 | Class                      |
| ----- | --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------- |
| A2-F1 | `{"a0-beta":"bogus"}`       | exit 0, session runs, **whole overlay ignored**; stderr: "These settings files did not load, so nothing in them is in effect, including hooks and permission rules… Invalid value" | LOUD_FALLBACK (β exposed)  |
| A2-F2 | key `skillOverride` (typo)  | exit 0, no warning, ignored                                                                                                                                                        | **SILENT_FALLBACK**        |
| A2-F3 | name `a0-betta`             | exit 0, no warning, no effect                                                                                                                                                      | **SILENT_FALLBACK**        |
| A2-F4 | name `A0-BETA`              | exit 0, no warning: names are case-sensitive                                                                                                                                       | **SILENT_FALLBACK**        |
| A2-F5 | malformed JSON              | exit 0, overlay ignored, stderr "Invalid or malformed JSON"                                                                                                                        | LOUD_FALLBACK              |
| A2-F6 | overlay path missing        | exit 1, `Error: Settings file not found`, 0 requests                                                                                                                               | HARD_FAIL (safe)           |
| A8-BI | S1 + all 13 built-ins `off` | listed fixture set `[a0-alpha, a0-proj]`, **0 built-ins listed** (13 in every other case)                                                                                          | demonstrated once (H only) |

The pre-launch validator prototype (`harness/overlay_validator.py`) agrees with the expected verdict for 29/29 overlays offline. It accepts only a top-level `skillOverrides` whose names are in the inventory and whose values are in {on, name-only, user-invocable-only, off}. It rejects unknown/misspelled keys, `hooks`, `disableAllHooks`, `permissions`, `statusLine`, duplicates, non-objects, malformed JSON, unknown and case-variant names, and invalid values.

## 3. Answers to the eight questions

1. **Individual exclusion of user and project skills** — yes on 2.1.312: `skillOverrides: "off"` in a `--settings` overlay (A2-01, A2-02, A-8).
2. **Absent from model-visible metadata, not just UI** — yes. DESC canaries are absent from the captured request body, and invocation of an excluded skill is refused locally with 0 requests (A2-03).
3. **Two alternative selections** — yes. S1 and S2 produce different exact fixture inventories, 3/3 each headless and 1/1 interactive for S1, with no user/project file modified.
4. **Malformed/unsupported/misspelled overrides** — the provider fails _open_:
   - invalid values and malformed JSON drop the entire overlay (with a stderr warning in `-p`);
   - unknown keys, unknown names and case variants are silent no-ops;
   - only a missing overlay file fails closed.
5. **Can Wyss detect and reject before execution** — yes for every case above, with a strict allow-list validator (prototype, 29/29). Because three failure modes are silent, the product also needs post-launch verification of the actual listing.
6. **Synthetic blocking hooks under a valid overlay** — effective. Session hooks fire (C-1), the blocking PreToolUse guard denies (C-2 v2, with a discriminating control), and the deny rule holds (C-3).
7. **Real mandatory controls under the Arsenal design** — preserved by construction, not by test of the real hooks:
   - all 9 real hook handlers live in `~/.claude/settings.json` (user source), the same source the synthetic hooks used;
   - `/etc/claude-code` is empty;
   - an overlay restricted to `skillOverrides` leaves them active (C-1..C-3).
   - C1-neg proves an _accepted_ overlay containing `disableAllHooks` would turn them off, so the validator rule "only `skillOverrides`" is a security requirement, not a style choice.
   - Owner hooks were not touched. Side observation: `r3-gate.py` exists in `~/.claude/hooks` but is not referenced by Claude user/local settings, `~/.codex/hooks.json` or `provider-hooks.json`. Its registration was not determined.
8. **Default Auto permission mode** — the owner's real `~/.claude/settings.json` already sets `permissions.defaultMode: "auto"`, so profiles without a permission field inherit auto. In the sandbox, auto mode still:
   - enforced `allow`/`deny` rules;
   - ran blocking hooks before execution;
   - asked for approval on paths outside the working directory (C2 v1).

   The launcher's `Permission::Ask → --permission-mode manual` (`profile.rs:276`) is **not verified** (B-3 not approved). 2.1.312 stays unqualified in `capabilities.rs`.

## 4. What Arsenal can now enforce, what needs work, what is NOT_PROVEN

| Capability                                                                          | Status on Claude Code 2.1.312                                                                                                                                         |
| ----------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Hide selected **user** skills per session                                           | **Enforceable** (A2-01, A-8 3/3 H + I)                                                                                                                                |
| Hide selected **project** skills per session                                        | **Enforceable** (A2-02, A-8)                                                                                                                                          |
| Refuse user invocation of hidden skills, body never sent                            | **Enforceable** (A2-03)                                                                                                                                               |
| Two sessions' different selections without touching files                           | **Enforceable** sequentially; _concurrent_ sessions NOT_PROVEN (D not approved)                                                                                       |
| Session hooks and deny rules preserved under an overlay limited to `skillOverrides` | **Enforceable** (C-1..C-3 with controls)                                                                                                                              |
| Overlay safety                                                                      | **Requires work**: strict allow-list validator (prototype exists) plus exact-name, case-sensitive inventory per build, in the Rust compiler with tests                |
| Detection of provider fail-open                                                     | **Requires work**: post-launch check of the real listing (transcript or pre-launch probe); stderr "did not load" detection for `-p`; interactive surfacing not tested |
| Hide **built-in** skills                                                            | **Demonstrated once** (A8-BI, H, 13/13 hidden). Not repeated, not interactive → no enforcement claim yet; built-in names are per-build inventory                      |
| Hide **plugin-provided** skills                                                     | **NOT_PROVEN** (A-6 not approved; 2.1.303 showed `skillOverrides` does not affect them)                                                                               |
| `name-only` / `user-invocable-only`                                                 | NOT_PROVEN (A-3/A-4 not run)                                                                                                                                          |
| Child-agent visibility of hidden skills                                             | NOT_PROVEN (A-10)                                                                                                                                                     |
| Mid-session change / resume                                                         | NOT_PROVEN (G-7/G-8)                                                                                                                                                  |
| Native permission mapping under auto default                                        | NOT_PROVEN (B-3)                                                                                                                                                      |
| Launching overrides on 2.1.312 in Wyss                                              | **Blocked** until the owner approves requalification; the allow-list is unchanged                                                                                     |
| Codex anything                                                                      | NOT_PROVEN (not approved)                                                                                                                                             |
| Whether a real model behaves better with fewer skills                               | needs real inference; outside A0                                                                                                                                      |

## 5. Corrections and disclosures

- **C-2 v1 control was mis-designed** (target outside the workdir). The pre-registered rule made v1 inconclusive. v2 changed only the target path, was pre-registered (`PRE-REGISTERED-C2v2.json`) and ran after a fresh preflight; both rows pass.
- **The batch log labels C1-neg "PASS"** because its pre-registered expectation was "markers absent". It is a control, not a passing security property.
- **Harness changes in this step** (previous versions kept in `evidence/retest/`):
  - `sandbox.py`: read-only case bind;
  - new `run_case.py`, `case_inside.py`, `run_batch.py`, `run_c2v2.py`, `overlay_validator.py`, `hooks/marker.py`, `hooks/guard.py`.

  None has had independent review.
