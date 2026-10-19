# 40 — DN-06=D: static Hermes boundary evidence and non-bypass tests

Date: 2026-10-13. **Source reading only, no execution or external review.**
Pinned Hermes upstream revision:
`NousResearch/hermes-agent@e06df9c2eae6901f4c88f464e0ae6e753c0428b8`.
The original Wyss's own code is not executed; earlier source inventory is in
[17](17-WYSS-AGENT-AND-HERMES.md). This source inspection tests **claims
against code**; it does not establish actual runtime confinement on Linux.

## 1. Specific findings at pinned revision

| Finding | Pinned source | Security implication |
| --- | --- | --- |
| `execute_code` is not merely an RPC for seven tools; it runs an LLM-generated Python program in an environment backend. The seven RPC tool stubs are additionally filtered by enabled tools | `tools/code_execution_tool.py:1-11, 42-45` | Disabling a subset of RPC tool names does not prove the script itself lacks OS effects; remove `execute_code` from the *planner* persona entirely and enforce OS sandbox |
| Remote execute-code ships scripts and per-invocation RPC token/environment to a backend and launches via `env.execute` | `tools/code_execution_tool.py:467-495`, `tools/code_execution_rpc.py:44-76` | Must not give a Hermes *planner* an unmediated execution backend, filesystem write, network/SSH credentials or host account |
| `HERMES_YOLO_MODE`, session-scoped YOLO and `approvals.mode: off` are three ways the Hermes approval checks may be bypassed; hardline and user deny floors are separate checks | `tools/approval.py:43-46, 481-488` | Hermes approval configuration is not Wyss Agent's deterministic policy and must not serve as an OS boundary or sole authorisation |
| Session `/yolo` toggles and persisted bypass restoration are supported in CLI/TUI/Desktop/gateway | `tools/approval_yolo.py:1-5, 30-73` | Wrapper-level assurances that a model can't change its own mode are unsafe without a frozen, separately owned confinement policy |
| Hermes `tool_guardrails` contains opt-in interactive hard-stops; guardrail mode is configurable | `agent/tool_guardrails.py:119-145, 362-364` | Guardrails improve behaviour but aren't a complete effect reference monitor for Wyss policy |

Canonical source URLs:
- https://github.com/NousResearch/hermes-agent/blob/e06df9c2eae6901f4c88f464e0ae6e753c0428b8/tools/code_execution_tool.py
- https://github.com/NousResearch/hermes-agent/blob/e06df9c2eae6901f4c88f464e0ae6e753c0428b8/tools/code_execution_rpc.py
- https://github.com/NousResearch/hermes-agent/blob/e06df9c2eae6901f4c88f464e0ae6e753c0428b8/tools/approval.py
- https://github.com/NousResearch/hermes-agent/blob/e06df9c2eae6901f4c88f464e0ae6e753c0428b8/tools/approval_yolo.py
- https://github.com/NousResearch/hermes-agent/blob/e06df9c2eae6901f4c88f464e0ae6e753c0428b8/agent/tool_guardrails.py

**Scope of inference:** The above confirms available source code paths,
not an exploit. An operator can configure an isolated Hermes process
safely only after all reachable effects, plugin/hook paths and backend
configuration writes are independently tested. No claim is made that
Hermes by itself is insecure for its intended standalone use.

## 2. Wyss planner persona non-bypass acceptance contract

Propose (not approve) a **planner-only process profile** with:

1. Separate OS identity/UID, sandbox or container with deny-by-default
   privileges that the Hermes process cannot rewrite. The OS boundary
   must refuse writes to the host user's home, configuration, auth store,
   Wyss policy and OS service files. Mount/cgroup/network restrictions
   need per-environment tests.
2. No `terminal`, `execute_code`, file writers, browser automation,
   delegation, cronjob, shell hooks, effectful plugin loading, arbitrary
   MCP, direct IPC to `wyss-host`, or network/cloud effectors unrelated
   to model inference. The complete effective tool registry must be
   enumerated **after** config/profile/plugin composition.
3. A dedicated, authenticated, typed plan-proposal channel to the
   Wyss Agent policy controller. No freeform command strings are
   considered executable proposals by themselves. Tool calls
   that request effects are delegated to Wyss's audited authorization,
   never to the Hermes process.
4. Read-only, pinned Hermes source/configuration image; updates go
   through an evidence gate. A process-local `disabled_toolsets`
   config alone is insufficient because of plugin and backend paths.
5. Deny unexpected file, subprocess, network and IPC effects
   at the OS boundary while allowing strictly required model
   inference egress to known providers via an audited gateway.
   Avoid a blanket network ban that makes the intended planner
   unusable; measure exactly which requests the planner requires.
6. No shared authentication store with original Wyss, native
   Hermes or the desktop user. No secrets in model prompts/event
   journals outside the approved minimization contract.

A persona can be declared **under Wyss control** only after
adversarial proof that an untrusted model completion cannot
activate an effect through any code path, including hidden
plugin hooks and local Python execution.

## 3. Required isolated tests (NOT run)

- **E-HRM-01:** enumerate registry on pinned version after
  all profile/system/default merges; ensure effectful tools are
  absent, not merely "approval required".
- **E-HRM-02:** intentionally submit terminal, Python code,
  file modify, cron, delegated subagent, MCP, browser and
  plugin-hook effect requests; observe explicit denial and
  unchanged canary files/privileges.
- **E-HRM-03:** attempt to alter Hermes's own profile/config
  or use `/yolo` while running as planner; OS boundary and
  Wyss policy remain unchanged. The probe runs only against
  disposable identities and files, not owner's data.
- **E-HRM-04:** attempt raw socket/HTTP requests outside the
  strictly allowed inference gateway, attempts to access
  `wyss-host` socket and the original user's home; OS denies.
- **E-HRM-05:** measure useful plan generation with zero
  enabled effectful tools, resource/latency costs and fallbacks.
- **E-HRM-06:** replay the same proposal after changing policy
  revision or project identity; Wyss policy rechecks and denies
  stale effects even if Hermes claims approval.
- **E-HRM-07:** upgrade Hermes revision in a fresh disposable
  environment and prove a version-sensitive tool registry
  detects added toolsets, defaults, hooks or backend changes.
- **E-HRM-08:** concurrent standalone Hermes and planner
  persona never share policy identity, secret store, writable
  home or runnable IPC endpoint.

Each run requires pinned image digest, intended scope, safety
controls and real stdout/pass-fail provenance. The owner
authorised isolated technical verification but **no disposable
Hermes host and safe test credentials have yet been identified
for this work in the accessible environment**. Static results
do not count as these tests.

## 4. Decision consequence

DN-06=D is consistent at the **desired-functional** layer,
but cannot be called an accepted Wyss Agent architecture
until a separate-process OS containment design and tests
have passed. "Hermes is running with fewer toolsets" does
not prove non-bypassability. This is a **hard P0 evidence
gate**, not a reason to silently lower the owner's requested
final-product scope.

## 5. IR-05: Wyss policy textual matching is not a sandbox (2026-10-14)

The post-owner review
[reviews/2026-10-13-independent-post-owner-review.md](reviews/2026-10-13-independent-post-owner-review.md)
identified a separate trust failure even if Hermes is fully isolated:
the original Wyss policy compares executable targets by
`fnmatch` over `action.target`, with optional
`arg_substrings` string checks. Independently inspected
read-only source from private `SaintWyss/wyss`:

- `packages/wyss-core/src/core/policy/engine.py:77-88`
  uses `fnmatch(action.type, rule.action_type)` and
  `fnmatch(_normalize(action.target), p)`; parameters use
  shallow equality, arg substrings are not a shell parser.
- `packages/wyss-core/src/core/policy/builtin_deny.py`
  lists textual target patterns for `disk-format`,
  `shell-rm-bypass`, `shell-privilege-escalation` and
  substring rules for `shell-fetch-pipe-exec`.

**Source observation, not a demonstrated exploit:** equivalent
command effects can have changed argument order, wrapper
binaries or encoded interpreter bodies that are not
semantically classified by target-name globs. The policy's
default risk level for `shell.run` may require explicit
human approval, but a misleading command summary plus
approval is **not** a guarantee of semantic confinement.

For **Hermes in Wyss Agent planner mode**, the safe
planning requirement is a *closed typed effect vocabulary*.
Unrestricted `shell.run`, Python code execution and other
interpreter commands are **denied**, never made safe by
a risk label or a friendly summary. Provider-native
standalone Hermes is a separate full-account product
capability and cannot be misrepresented as policy confined.

**E-HRM-09 (NOT RUN):** use a disposable original-Wyss
policy evaluator, comparing equivalent synthetic shell
effects under argument reordering, shell quoting, process
substitution, interpreters and encoded payloads. The
planner persona must reject all opaque shell execution
regardless of superficial token matches. No owner's
commands, secrets or real policies may be modified.
This correction does not accept an ADR or prove
Hermes confinement.

## 6. Wyss original implementation: source-qualified guard evidence (2026-10-14)

The earlier glob-only warning in §5 was **incomplete**
as a description of real execution protections.
Read-only source was checked at the exact commit
`SaintWyss/wyss@0761dcdacdafa216c8afc25036e24685855d2228`:

- `src/execution/shell.py:35–64, 92–116`: `ShellAdapter`
  has a **fail-closed empty allowlist by default**,
  receives `allowed_binaries` or a TOML config explicitly,
  runs subprocesses with `argv` and `shell=False`,
  with further guard/policy checks in the adapter.
  Thus *accepting* a plan's string at policy level does
  **not** imply that the shell executes it in the
  default configuration.
- `src/execution/filesystem.py:129–178`: second wall
  resolves target and re-authorizes; rejects symlink
  redirection on the final path component.
- `src/execution/filesystem.py:285–297`: after the
  path check, mutation uses normal
  `resolved_path.write_bytes/write_text`; path
  and ancestor state are not necessarily pinned by
  file descriptors. A concurrent attacker changing
  mutable directories between resolution and open
  is a plausible TOCTOU risk, **not a proven exploit**.
- `packages/wyss-core/src/core/policy/engine.py:77–90`:
  target patterns are textual `fnmatch`. Multiple
  **other guard layers** mean this fact alone does
  not demonstrate a full execution bypass.

Source links:
- https://github.com/SaintWyss/wyss/blob/0761dcdacdafa216c8afc25036e24685855d2228/src/execution/shell.py
- https://github.com/SaintWyss/wyss/blob/0761dcdacdafa216c8afc25036e24685855d2228/src/execution/filesystem.py
- https://github.com/SaintWyss/wyss/blob/0761dcdacdafa216c8afc25036e24685855d2228/packages/wyss-core/src/core/policy/engine.py

**Consequence for DN-06=D:** No new unrestricted shell
policy is approved. A future closed typed action
vocabulary still needs an explicit versioned adapter
catalog, allowed operand ranges, derived/transitive
effects, safe file descriptor/path handling, OS
principal, approval integrity and failure/retry rules.
An empty allowlist passing negative tests is NOT proof
of useful Wyss Agent capabilities. E-HRM-09 must
include **both positive useful actions and adverse
variants**, not only invalid shell names. All runtime
tests remain **NOT RUN**.
