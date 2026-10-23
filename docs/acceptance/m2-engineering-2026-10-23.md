# M2 engineering acceptance — 2026-10-23

**STATUS: `ENGINEERING_COMPLETE_RELEASE_BLOCKED`**

Branch `feat/m2-native-parity-2026-10-23` is based on verified `origin/main`
`def69fe35ddfe8b696b65a12d2f2babe296c8247`. The frozen contract and exact oracles
are in [m2-native-parity.md](../contracts/m2-native-parity.md). This record separates
fixture-backed engineering from native-desktop and real-provider release acceptance.

## Acceptance matrix

| Oracle                                                         | Engineering result and evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Remaining release evidence                                                                                                                                                            |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M2-01 Native launch/provider fidelity                          | **PASS, fixtures.** Closed launcher, shell argv and environment tests pass. Version is checked after shell startup, the same resolved CLI path is executed, and the child is admitted as a session only after a private marker confirms the guard passed. PATH-mutation, mismatch and no-provider-start tests pass.                                                                                                                                                                                                                                                                                                                                                                                                          | Real Native desktop workflow on the adopted host/provider builds.                                                                                                                     |
| M2-02 Inheritance, precedence and pinning                      | **PASS, fixtures.** Profile/store tests cover inherited scalars/lists, precedence, versions and limits. Preview token hashes the resolved plan; stale start/restart tests reject changed plans. Restart starts and verifies its replacement before stopping the old session. Legacy hosts without the token warn and may use the latest saved profile instead of the displayed revision; that path does not satisfy exact M2 pinning.                                                                                                                                                                                                                                                                                        | Native desktop start/restart evidence with profile changes between preview and action.                                                                                                |
| M2-03 Native model/effort requests                             | **PASS, syntax and fixtures.** Exact argv/config interface and profile capability metadata are implemented; requested, configured, provider-reported and effective values remain distinct. Legacy hosts keep non-effort profile starts with a warning and refuse effort profiles without the M2 token contract.                                                                                                                                                                                                                                                                                                                                                                                                              | Provider-resolved/effective value and entitlement for a real Native turn: **NOT_RUN**.                                                                                                |
| M2-04 Configuration inventory/provenance                       | **PASS, existing implementation and regression suites.** Inventory fixtures cover real sources, source labels, malformed inputs, unreadable paths, redaction and provider isolation.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Exact current-build provider consumption for each configuration item.                                                                                                                 |
| M2-05 Hooks, skills, MCP, plugins and unsupported capabilities | **PASS, fixtures and explicit capability limits.** Provider-specific inventory/writers and refusal paths remain; Codex project skills, hook mutation, project trust writes, and per-session hook/skill exclusions remain unsupported.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Current-build GUI/provider consumption where applicable: **NOT_RUN**.                                                                                                                 |
| M2-06 Global/project/session boundaries                        | **PASS, fixtures.** Configuration tests cover project identity, path/link refusal, per-project plans and no global write from Native profiles.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | Owner's native GUI workflow on the adopted installation.                                                                                                                              |
| M2-07 Authoring, plans, apply and Undo                         | **PASS, fault-injected fixtures.** Stale plans, external writers, symlinks, ACLs, partial failures and exact restoration paths are covered by the configuration suite. ADR0034's disclosed fallback remains unchanged.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Native confirmation/apply/Undo journey in the desktop.                                                                                                                                |
| M2-08 Templates and encrypted restoration                      | **PASS, disposable stores.** Import/export, wrong keys/domains, tampering, missing keys and sealed restoration tests pass.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Native template-to-confirmation GUI journey: **NOT_RUN**.                                                                                                                             |
| M2-09 Provider-version capability validation                   | **PASS for syntax evidence.** The exact local builds, supported interfaces, scopes, sources, dates, reload semantics and limits are recorded below and in `src-tauri/src/native/capabilities.rs`. Unknown versions do not inherit support.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Provider resolution, entitlement, effective use and reload on exact current builds remain unverified.                                                                                 |
| M2-10 External change, failure, concurrency and recovery       | **PASS, targeted configuration/recovery tests.** Watcher/link refusal, outside writers, interrupted writes, journal recovery, missing vault key, ACL and stale-plan cases pass.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Real filesystem/provider concurrency on the owner's installation: **NOT_RUN**.                                                                                                        |
| M2-11 Native GUI end-to-end                                    | **PARTIAL.** Native/configuration Svelte component tests and the frontend fixture harness pass. The full Tauri desktop workflow with fixture CLIs, disposable HOME/XDG roots and saved GUI evidence was **NOT_RUN**.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Native GUI journey and screenshots/logs are a release gate.                                                                                                                           |
| M2-12 Preservation, costs and regressions                      | **PASS for configured bounds, regression fixtures and preservation; no measured M2 load benchmark.** Inventory defaults: 4 MiB/file, 4096 items/source, 262144 elements/source, depth 32, 64 roots, 2048 listed entries, 3 read attempts; watcher debounce 200 ms, burst cap 1 s, fallback 30 s. Profiles: 100 × 512 KiB, up to 20 versions/profile, depth 8. Journal retention: 10000 entries/180 days, with no separate byte ceiling. Restoration: 512 KiB/file and 500-record/180-day pruning target; parked recovery copies are preserved, so total records can exceed 500. A guarded launch waits at most 6 s and uses a 3-byte private marker. No host, authority, session, credential or provider config was changed. | No desktop load benchmark or standalone mutation-testing campaign was run; aggregate parked-record storage is not hard-capped. Existing resource limits and regression suites passed. |

`PASS` above means the named reproducible fixture evidence passed; it does not imply
real-provider acceptance. M2-11 remains the explicit desktop release gate.

## Provider observations and interface limits

Read-only local checks on 2026-10-23:

| Installed provider | Observed build          | Supported request/configuration surface verified                                                                                                                                                                                        | Not established                                                                                                    |
| ------------------ | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| Claude Code        | `2.1.309 (Claude Code)` | Local `--help`: `--model`, `--effort`, `--permission-mode`, `--settings`; one-process JSON overlay. Exact build is pinned in the capability manifest. Existing F1/F2a and F3 overlay behavior was observed on `2.1.303`, not `2.1.309`. | Current-build account entitlement, provider-resolved/effective values, hot reload and feature-preservation matrix. |
| Codex              | `codex-cli 0.159.0`     | Local `--help`: `-m`, `-c KEY=VALUE`, `-s`, `-a`; F6 exact-build read-only config evidence records `model_reasoning_effort`. `-c` is per-process and does not write `~/.codex/config.toml`.                                             | Native turn resolution/effective use, entitlement and session reload.                                              |

The official [Claude CLI reference](https://code.claude.com/docs/en/cli-usage),
[Claude settings reference](https://code.claude.com/docs/en/settings), and
[Codex configuration reference](https://developers.openai.com/codex/config-reference)
document these interfaces. Local `--version`/`--help` and documentation establish
syntax/interface evidence only. No provider prompt, inference, sign-in, credential read,
or provider configuration write was run for this record.

Previous accepted evidence remains linked: [F1/F2a](f1-f2a-2026-10-19.md),
[F3](f3-2026-10-19.md), and [F6](f6-models-2026-10-20.md). The exact CLI builds differ
in some of those historical observations; they are not silently promoted to current
provider-consumption proof.

## Tests executed

- `npm run check`: Prettier, ESLint, Svelte/TypeScript checks, 59 Vitest files / 991
  tests, 16 owned-resource harness tests, and production Vite build all passed.
  Svelte reported 0 errors and 0 warnings. Vite retains its existing >500 kB chunk
  advisory.
- `cargo fmt`: passed.
- `cargo test native::`: Native launch, capability, profile, version-guard and restart
  fixtures passed; the one ignored test launches installed real providers and was not run.
- `cargo test agentconfig::`: 269 passed, 9 ignored. Ignored tests include real machine
  configuration and real credential-store access.
- `cargo test host::`: 14 passed, 1 ignored. Ignored test launches installed providers.
- `cargo test --test authority_boundary`: 1 passed.
- Targeted shell guard test ran Bash, Zsh, Fish and POSIX recipes available in this
  environment. The simulated startup PATH mismatch did not execute the changed build.
- No blanket Rust suite, dangerous lifecycle suite, paid test or installed-provider
  test was run. No separate mutation-testing framework was run; critical invariants have
  direct negative/race/fault fixtures.

## Independent review

One independent read-only review found and reproduced two issues: shell startup could
change PATH after the earlier version probe, and a restart could stop the old session
before a shell-level version mismatch was known. The implementation now verifies and
executes the shell-resolved path, waits for a private pre-exec guard marker, and refuses
to stop the old session when the guard fails. The same review found legacy hosts were
over-rejected; non-effort profiles remain compatible with an explicit warning, while
legacy effort profiles are refused. On final targeted re-verification, the reviewer
reported **PASS** for preserving the old session after a shell guard mismatch, citing
`a_shell_version_mismatch_during_restart_keeps_the_previous_session_running`. The
reviewer also reported **PASS** for legacy no-token behavior and inherited-effort
detection, citing `a_legacy_host_keeps_non_effort_profiles_compatible_but_detects_inherited_effort`;
the reviewer noted that no full fake-HostLink legacy integration test covers that path.
This was a static review and did not run tests or providers; the test results above were
run independently in this checkout.

## Preservation and scope

All test stores and fake providers were disposable. The only provider invocations were
`--version` and `--help`. No installed host/authority was restarted or replaced; no
session was interrupted; no global provider settings, credentials, stores or history
were read for mutation or rewritten; no migration or deployment occurred. The feature
branch is isolated from the separate working checkout.

## Owner decisions, unsupported behavior and release/adoption

- **OD-M2-01:** ADR0034 remains accepted as written, including its disclosed private
  plaintext fallback for entries classified secret-free when Secret Service is
  unavailable. Any all-sealed policy extension is a separate owner decision; no change
  was silently made.
- Real provider consumption and the native desktop acceptance remain **NOT_RUN**. Their
  execution requires explicit authorization; current syntax evidence does not replace it.
- M2 adoption requires review/merge by the owner, then a controlled update to the M2
  GUI and host together. The updated host must announce `native_profile_plan_token`.
  Do not restart/replace the currently installed host as part of this engineering PR.
- Run M2-11 with disposable HOME/XDG roots and fixture CLIs first, save screenshots and
  logs, and verify cancellation, preview refresh, launch, restart and configuration
  authoring/Undo. After that, obtain explicit authorization for exact-build real Native
  provider model/effort consumption and preservation checks. Record configured,
  requested, provider-resolved and observed/effective facts separately.
- Unsupported Codex project skills, hook mutation, project trust writes and per-session
  hook/skill exclusions remain visible refusals. Claude Managed, Managed model/effort
  selection and authority extensions remain M3.
- This branch is not merged or deployed. Release remains blocked until the native GUI
  and provider-consumption evidence is accepted and any required ADR0034 policy decision
  is recorded.
