# M2 engineering acceptance — 2026-10-23

**STATUS: `ENGINEERING_COMPLETE_RELEASE_BLOCKED`**

Branch `feat/m2-native-parity-2026-10-23` is based on verified `origin/main`
`def69fe35ddfe8b696b65a12d2f2babe296c8247`. The frozen contract and exact oracles
are in [m2-native-parity.md](../contracts/m2-native-parity.md). This record separates
fixture-backed engineering from native-desktop and real-provider release acceptance.

## Acceptance matrix

| Oracle                                                         | Engineering result and evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Remaining release evidence                                                                                                                    |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| M2-01 Native launch/provider fidelity                          | **PASS, fixtures.** Launcher, argv/environment, version guard and no-provider-start failures pass. Tauri GUI run 5 previewed and launched only the Claude fixture; run 9 restarted it and retained the stopped predecessor.                                                                                                                                                                                                                                                                                                                                                                                                                                | Real Native workflow using the adopted host and installed provider builds: **NOT_RUN**.                                                       |
| M2-02 Inheritance, precedence and pinning                      | **PASS, fixtures.** Direct and inherited profile edits invalidate the old preview; Workspace navigation requests one fresh preview, disables Start until it succeeds, and only accepts the latest response. Failed preview has explicit retry without automatic repeated requests. Fake protocol-1 HostLink tests cover non-effort preview/start compatibility and refusal of inherited effort.                                                                                                                                                                                                                                                            | Real-host start/restart after profile edits: **NOT_RUN**. Legacy non-effort mode warns that an older host cannot bind the displayed revision. |
| M2-03 Native model/effort requests                             | **PASS, syntax and fixtures.** Request/configured/provider-reported/effective values remain distinct. Current installed versions and help flags were checked read-only; no provider turn ran.                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Provider-resolved/effective value and entitlement on a real Native turn: **NOT_RUN**.                                                         |
| M2-04 Configuration inventory/provenance                       | **PASS, fixtures and GUI.** Inventory tests cover source labels, malformed/unreadable inputs, redaction and provider isolation; run 10 inspected the disposable Native configuration inventory.                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Consumption by the exact installed provider builds: **NOT_RUN**.                                                                              |
| M2-05 Hooks, skills, MCP, plugins and unsupported capabilities | **PASS, fixtures and explicit limits.** Unsupported Codex project skills, hook mutation, project trust writes and per-session hook/skill exclusions remain visible refusals.                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | Current-build GUI/provider consumption where applicable: **NOT_RUN**.                                                                         |
| M2-06 Global/project/session boundaries                        | **PASS, fixtures.** Identity, path/link refusal, per-project plans and no global write from Native profiles are covered. All GUI writes used disposable HOME/XDG roots.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Adopted-installation acceptance: **NOT_RUN**.                                                                                                 |
| M2-07 Authoring, plans, apply and Undo                         | **PASS, fault-injected fixtures and GUI.** Plan cancellation preserved original bytes; edit/apply required the native confirmation; Undo restored exact original bytes. ADR0034 fallback is unchanged.                                                                                                                                                                                                                                                                                                                                                                                                                                                     | None for fixture engineering; real-provider reload/consumption is tracked separately.                                                         |
| M2-08 Templates and encrypted restoration                      | **PASS, disposable vault and GUI.** Run 27 exported a sealed 543-byte template. Run 31 reviewed/imported it through the native passphrase/editor/plan/confirmation flow; Undo restored the exact pre-import 31-byte file.                                                                                                                                                                                                                                                                                                                                                                                                                                  | Real-provider use of imported instructions: **NOT_RUN**.                                                                                      |
| M2-09 Provider-version capability validation                   | **PASS for interface evidence.** Installed builds and documented request flags are recorded below and in `src-tauri/src/native/capabilities.rs`; unknown builds inherit no support.                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Provider resolution, entitlement, effective use and reload remain unverified.                                                                 |
| M2-10 External change, failure, concurrency and recovery       | **PASS, targeted configuration/recovery fixtures.** Outside writers, symlinks/ACLs, interrupted writes, journal recovery, missing vault key and stale plans are covered.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Real filesystem/provider concurrency on the installed system: **NOT_RUN**.                                                                    |
| M2-11 Native GUI end-to-end                                    | **PASS, fixture-only Tauri.** Runs 5, 9, 10, 13, 15, 19, 27 and 31 cover preview, cancellation, fixture start/restart, inventory, edit, plan cancellation, sealed template export/import, native confirmation and Undo. Every run used isolated HOME/XDG, private display/bus/keyring and fixture executables; accepted run records report no leftover processes. Evidence: `docs/acceptance/evidence/m2-gui-2026-10-23/`.                                                                                                                                                                                                                                 | Real-provider GUI acceptance: **NOT_RUN**.                                                                                                    |
| M2-12 Preservation, costs and regressions                      | **PASS for configured bounds, regression fixtures and preservation; no measured M2 load benchmark.** Inventory defaults: 4 MiB/file, 4096 items/source, 262144 elements/source, depth 32, 64 roots, 2048 listed entries, 3 read attempts; watcher debounce 200 ms, burst cap 1 s, fallback 30 s. Profiles: 100 × 512 KiB, up to 20 versions/profile, depth 8. Journal retention: 10000 entries/180 days with no separate byte ceiling. Restoration: 512 KiB/file and 500-record/180-day pruning target; parked recovery copies can exceed 500 total. Guarded launch: 6 s and 3-byte private marker. No active installation or provider state was modified. | No standalone mutation campaign or desktop load benchmark; aggregate parked-record storage is not hard-capped.                                |

`PASS` above means the named reproducible fixture evidence passed; it does not imply
real-provider acceptance. The fixture-only desktop GUI passes M2-11 engineering; real
provider acceptance remains a release gate.

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

- `npm run check`: Prettier, ESLint, Svelte/TypeScript checks, 59 Vitest files / 995
  tests, 16 owned-resource harness tests, and production Vite build all passed.
  Svelte reported 0 errors and 0 warnings. Vite retains its existing >500 kB chunk
  advisory.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: passed.
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --bin wyss-console native::`:
  44 passed, 1 ignored. Coverage includes preview pinning, version guard, restart
  preservation and both new fake protocol-1 HostLink integration cases. The ignored
  test launches installed real providers and was not run.
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --bin wyss-console agentconfig::`:
  269 passed, 9 ignored. Ignored tests include real machine configuration, real
  credential-store access and real provider writes.
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --bin wyss-console host::`:
  14 passed, 1 ignored. The ignored test launches installed providers.
- `cargo test --test authority_boundary`: 1 passed.
- `cargo build --locked --manifest-path src-tauri/Cargo.toml --bin wyss-console`:
  passed. `python3 -m py_compile benchmarks/m1_gui.py`, `python3 -I
benchmarks/test_owned.py` (16 passed), and `git diff --check`: passed.
- Targeted shell guard test ran Bash, Zsh, Fish and POSIX recipes available in this
  environment. The simulated startup PATH mismatch did not execute the changed build.
- `npm run check:package-fixtures`: **NOT_RUN** at the package-inspection stage; it
  reported `ENOENT` for the absent `.artifacts/package/wyss-console-0.2.0-linux-x64`
  directory. No local package artifact was replaced or created to satisfy this check.
- No blanket Rust suite, dangerous lifecycle suite, paid test or installed-provider
  test was run. No separate mutation-testing framework was run; critical invariants have
  direct negative/race/fault fixtures.

## Independent review

One focused independent read-only review examined the corrected Native preview and the
legacy HostLink integration surfaces. It found a recoverability gap: when preview
refresh failed, Start stayed safely disabled but the user could not retry without
changing profile selection. The UI now offers an explicit retry, with a regression test
that verifies retries do not create automatic loops. The reviewer also identified the
missing full fake-HostLink legacy integration evidence; the new protocol-1 fake-host
tests cover a compatible non-effort preview/start and refusal of inherited effort before
start. The review was static and did not execute tests or providers. These corrections
were tested in this checkout; no repeated review cycle was started.

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
