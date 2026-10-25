# M2 + M3 integration acceptance record — 2026-10-24

**Branch:** `integration/m2-m3` · **Integration PR:** one, against `main`, **not merged** · **Inputs:** M2 PR #22
(head `977cb8f`, untouched) and corrected M3 PR #23 (head `d52705c`). Neither PR branch was pushed to, rebased,
force-pushed, deleted or merged by this work.

**Status:** `CODE_INTEGRATED` · `ENGINEERING_TESTS_PASSED` · `SECURITY_REVIEW_PENDING` · `RELEASE_BLOCKED`.
**Not** `RELEASE_READY`: both independent security reviews of M3 returned FAIL, the fixes after the second are
unreviewed, and ADR0053/0054/0055/0056, the usage-history policy, real-provider evidence and the release decision
are the owner's. Integration does not authorize real provider turns, credential provisioning, an authority
restart or any deployment; every live gate (`m3::gate`) is `false` and nothing installed was touched.

## How the two were combined

| Step | Result                                                                                                                                                                                                                                                                                    |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1    | `integration/m2-m3` created in its own worktree from `origin/main` (`def69fe`).                                                                                                                                                                                                           |
| 2    | `git merge --no-ff` of M2 (`977cb8f`), then of M3 — first at `d20d8e6`, finally at `d52705c`. History of both is preserved (no copying, no rebase); each side keeps its own commits. Both merges were textually clean except four documentation/CSS files that both sides edited, merged. |
| 3    | Semantic integration (below): one clippy fix in M2's native code, one worker fix found by the combined run, the fixture host, the harness, and one invariant test that described the old `main.rs`.                                                                                       |

The merge strategy is the dependency strategy: the integration PR **contains** both lines of history, so merging it
makes #22 and #23 redundant: both heads are ancestors of this branch. If the
owner prefers to merge #22 and #23 separately, they must go in that order and this PR is then rebased by a merge of
`main`.

### Conflicts and semantic findings

| Area (as listed in the brief)                           | Finding                                                                                                                                                                                                                                                                                                                                   |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Rust modules / authority contracts                      | No textual conflict. M2 adds `native/*`, `agentconfig/*`, `configuration`; M3 adds `adapters/claude`, `f8/claude`, `m3`. The Managed worker sits at the F8 `Worker` seam; Native sessions are owned by the host. Disjoint owners, one capacity-one registry.                                                                              |
| Tauri permissions / capabilities                        | Disjoint capability files (`m3.json` is new). The five fixture commands are declared in the build manifest but granted by **no** capability; the fixture binary grants them to `main` at start. Test `the_package_grants_no_fixture_command`.                                                                                             |
| Svelte navigation / project workspace context           | M3 adds a Providers entry; M1's project workspace stays behind its off gate. Both navigation tests pass together (1,037 frontend tests).                                                                                                                                                                                                  |
| Native / Managed session views                          | Separate views, separate state. In one window: Native sessions keep running while a Managed fixture is started, used, stopped, restarted or refused (scenarios below).                                                                                                                                                                    |
| Provider capability matrices, model/effort presentation | M2 documents Native model/effort request; M3 adds Managed model facts with the selection gate closed. `provider-matrix.md` carries both without contradiction (M3 states that Native request ≠ Managed selection).                                                                                                                        |
| Packaging                                               | One package; fixture host absent (below).                                                                                                                                                                                                                                                                                                 |
| Test harnesses                                          | `benchmarks/m1_gui.py` (M2's nested display, fixture Native CLIs) now also hosts the M3 fixture through `--m3-*` options; `benchmarks/m3_steps.py` holds the shared M3 checks; `integration_gui_suite.py` runs all 13 scenarios.                                                                                                          |
| Defect found only by combining                          | `clippy -D warnings` rejected two statements in M2's `native/commands.rs` and `native/recipe_tests.rs` (fixed, behaviour unchanged). The combined run also showed that an approval card still waiting when a Managed turn was interrupted stayed on screen: a real M3 bug, fixed in the worker (`withdraw_undecided`) and kept on M3 too. |
| A test that described the old wiring                    | `only_the_command_line_selects_an_authority…` asserted a literal in `main.rs`; it now asserts the two exact forms (package: the whole command line; fixture feature: the fixture host's `--m3-fixture-*` arguments are taken out first).                                                                                                  |

## Combined results (integrated head)

| Area                                | Command                                                            | Result                                                                                                                            |
| ----------------------------------- | ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------- |
| Rust format, check, clippy          | `npm run check:rust`; clippy `--all-targets --features m3-fixture` | PASS (`-D warnings`), with and without the fixture feature                                                                        |
| Rust tests                          | `cargo test`                                                       | PASS: 1,364 lib (27 ignored), 507 bin, 5 + 1 + 4 + 27 + 2 + 11 + 10 + 8 + 8 + 55 + 9 + 7 + 16 process/contract/integration suites |
| Real SDK and CLI, offline           | `npm run test:claude-real`                                         | PASS: 16 of 16                                                                                                                    |
| Bridge, tap, broker, guard          | `npm run test:claude-bridge`                                       | PASS: 77                                                                                                                          |
| Frontend                            | `format:check`, `lint`, `typecheck`, `npm test`, `test:harness`    | PASS: 1,037 tests (63 files), harness 16                                                                                          |
| Repository scan                     | `npm run scan:repo`                                                | PASS                                                                                                                              |
| V8→V9 rehearsal, synthetic only     | `cargo test v9_rehearsal`                                          | PASS: 10 tests (see below)                                                                                                        |
| Production package                  | `npm run desktop:package`, `check:package-fixtures`                | PASS: no fixture marker in `bin/wyss-console` or `libexec/wyss-console/wyss-control`                                              |
| Isolated desktop, integrated binary | `python3 -I benchmarks/integration_gui_suite.py …`                 | PASS: 13 of 13, `leftovers: []` in every one ([evidence](evidence/integration-gui-2026-10-24/))                                   |

## The twelve verifications

Environment of the desktop runs: the real Tauri window of the integrated binary built with `--features m3-fixture`,
on a nested KWin + Xwayland display of its own, with its own session bus, keyring, `HOME`/XDG roots, fixture Native
CLIs, the real pinned Claude Agent SDK and Claude Code CLI behind a mock Messages provider on loopback, and a
private authority directory. No installed Console, host, authority, credential or display was named.

| #   | Verification                                          | Evidence                                                                                                                                                                                                                                                                                                                                                                                       | Result                                                                |
| --- | ----------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| 1   | M2 Native Claude/Codex launch and configuration       | Scenarios `native-then-managed-inference` (Claude Code 2.1.309 Native with the "Review, read only" profile, through the preview), `native-codex-and-managed-local-tools` (Codex CLI 0.159.0 Native), `m2-native-preview-start`                                                                                                                                                                 | PASS (fixture Native CLIs; real-provider consumption remains NOT_RUN) |
| 2   | Profile preview, revision invalidation, restart, Undo | `m2-native-restart`, `m2-config-edit-cancel/apply/undo-apply`, template export/import-undo on the integrated binary; revision invalidation by bin tests `a_profile_change_after_preview_refuses_the_stale_start_without_creating_a_session` and `a_stale_restart_preview_leaves_the_existing_native_session_running`                                                                           | PASS                                                                  |
| 3   | M3 Claude Inference fixture lifecycle                 | `native-then-managed-inference` (ready, answer, usage, stop), `managed-recovery-after-stop` (stop, restart, second life)                                                                                                                                                                                                                                                                       | PASS                                                                  |
| 4   | Local Tools approval, denial, cancellation, recovery  | `native-codex-and-managed-local-tools` (held command accepted and declined, held diff, file written once or not at all), `managed-cancel-turn` ("Cancelled — nothing ran"), `managed-recovery-after-stop`                                                                                                                                                                                      | PASS                                                                  |
| 5   | Existing Codex Managed regression                     | Codex suites of the authority (lib + `f8` acceptance/system) unchanged and green on the integrated tree; the Claude worker cannot present itself as Codex outside tests (negative tests). **Not** exercised through the GUI: the GUI has no Codex Managed runtime in these scenarios.                                                                                                          | PASS in tests; GUI `NOT_RUN`                                          |
| 6   | Project/workspace context isolation                   | Managed fixture sessions use their own disposable workspace; no binding between M1's project workspace (off by gate) and a Managed session exists. The GUI scenarios start with no projects (`world.projects: 0`). M1's own suites pass on the integrated tree.                                                                                                                                | NOT exercised in the GUI; no coupling exists to test                  |
| 7   | Model/effort and usage presentation                   | Integrated run: the Providers window shows the four model facts (offered, configured, requested, reported in use) beside a Native session. Counters, "not reported" and the closed selection gate: the M3 isolated run's screenshots ([`inference-04-usage.png`](evidence/m3-gui-2026-10-23/inference-04-usage.png)), same components; the integrated run's pictures stop above the usage card | PASS                                                                  |
| 8   | Metrics history default-off                           | The M3 isolated run's screenshot shows "Off" and the unaccepted-policy reason ([`inference-04-usage.png`](evidence/m3-gui-2026-10-23/inference-04-usage.png)); the integrated pictures do not reach that card. Tests `history_is_off_until_the_owner_opts_in` and `metrics_history_blockers`                                                                                                   | PASS                                                                  |
| 9   | Authority capacity and security refusals              | Capacity one: `the_global_capacity_of_one_runtime_holds_for_claude_and_a_stop_frees_the_slot`. Refusal in the window: `managed-refused-native-still-works` (an SDK tree the verifier must refuse is refused with a reason, and Native keeps working)                                                                                                                                           | PASS                                                                  |
| 10  | No cross-provider identity confusion                  | Negative tests: the V8 store, the wire and the handshake refuse every real Claude identity; Native session count/identity checked in the window before and after Managed turns (`assert-native-count`, `assert-native-restart`)                                                                                                                                                                | PASS                                                                  |
| 11  | No orphan processes, no unexpected writes             | `leftovers: []` for all 13 runs; filesystem baseline/compare of the disposable roots (`fs-compare`) and `fs-1-installed-console-untouched.json` in the integration scenarios                                                                                                                                                                                                                   | PASS                                                                  |
| 12  | Production packaging excludes fixture capabilities    | `check:package-fixtures`: no `--m3-fixture-*` or fixture-host marker in either binary; the five fixture permission names exist in the build manifest (needed to declare them) but no capability grants them and the handlers are installed only by the feature binary (`the_package_grants_no_fixture_command`)                                                                                | PASS                                                                  |

## M3 independent review (carried unchanged)

Two independent reviews of M3 by Codex returned **FAIL for real-credential admission** (first over `530c49e`, second
over `d20d8e6`). The corrective code for both is in this branch (M3 `5703054`), with regression tests; **it has been
reviewed by nobody but its author**, so no PASS is claimed. A third review is `NOT_RUN`. Details and the status of each
finding: [M3 record](m3-engineering-2026-10-23.md). `m3::gate::INDEPENDENT_REVIEW_PASSED` is `false`.

## V9 (ADR0055) — synthetic only

ADR0055 is **not accepted**. The V8→V9 migration exists only as a rehearsal on synthetic in-memory images compiled
into test builds; the 10 rehearsal tests pass on the integrated tree: every row and the event log byte for byte
preserved, exact V9 schema, an interruption at each of ≥25 stages leaves the source untouched, work in flight is
refused, older binaries refuse a V9 store, a Claude runtime is accepted only by V9, capacity one stays the registry's.
Nothing opened, migrated or advertised a production store; Claude admission is closed in the binary. Writers and replay
validators are still Codex-only, and a V9 verifier does not exist (ADR0055 lists them).

## Outstanding defects and gaps

| Item                                                                                           | State                                                                                 |
| ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Fixes F1–F7 of the second M3 review are unreviewed                                             | Open; request a review by someone other than the author, with working user namespaces |
| Residual of the approved write: another same-user writer between the last check and the rename | Stated, not closed                                                                    |
| No Claude runtime can be persisted by the production authority (V8 store is Codex-only)        | ADR0055                                                                               |
| No source of a real credential exists                                                          | ADR0056 proposed                                                                      |
| Real-provider Native consumption and real-provider Managed turns                               | `NOT_RUN` (not authorized)                                                            |
| Codex Managed through the GUI in the integrated run                                            | `NOT_RUN`                                                                             |
| Project/workspace isolation through the GUI                                                    | Not exercised (project workspace is off by gate)                                      |
| Codex performance arm for M3-11                                                                | Not measured                                                                          |

## Pending owner decisions

ADR0053 (boundary and exact build), ADR0054 (model/effort selection), ADR0055 (store that can hold a non-Codex runtime),
ADR0056 (credential delivery), the usage-history privacy and retention policy, real-provider test authorization,
OD-M2-01 (M2's own release condition), and the release decision. None is taken by this integration.

## Installation and rollback plan (not executed)

1. **Nothing is installed by this PR.** The installed Console, host and authority were not touched.
2. To use the integration: build the package from this branch (`npm run desktop:package`), install it as a new
   directory next to the current one (ADR0027's directory package), and keep the current directory. Do not restart the
   authority or host until the owner decides.
3. The integrated Console has no Claude Managed admission (every gate is closed), so installing it changes Native/M2
   behaviour and the Providers window only; the store stays V8 and Codex data is untouched.
4. **Rollback:** point the launcher back to the previous package directory. No migration happened, so no data rollback
   exists to perform. A future V9 store would not be readable by the previous Console; that decision belongs to the
   release that includes ADR0055 and is described there.

## Preservation

No installed Console file, host, authority, session, credential, configuration, production database or provider quota
was touched. Experiments ran in disposable directories, on a private display and bus; every process was a direct child
ended by its handle. Global runtime capacity stays 1; P1/P2/Codex code paths and the V8 schema are unchanged.
