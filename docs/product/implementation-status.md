# Verified baseline and remaining gaps

Source baseline after Phase1 merge: `d2ee8d2971ed50de2e39d73ad71f2a29f14c42b2`,
2026-10-22; application trees unchanged from `e480d479`.
Sections1–4 reconcile **recorded** acceptance; [M0 below](#m0-engineering-baseline)
adds narrowly scoped current read-only observations and fixture results, not a repeat of
production acceptance. Phase1 checks remain in [traceability](traceability.md).
Historical process IDs and local artifact paths are evidence locators, never cleanup targets.

## 1. Status vocabulary

| Code | Meaning                                                                   | Required interpretation                                           |
| ---- | ------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| I    | Implemented and production/real-installation verified for the named slice | Never expands beyond the cited observation                        |
| P    | Implemented, partially verified                                           | Source/fixtures or partial real acceptance; list missing proof    |
| D    | Designed, not implemented                                                 | Behaviour/contracts exist; no execution claim                     |
| B    | Blocked by technical/security constraints                                 | Preserve final requirement, identify the unlocking proof/decision |
| U    | Not designed enough or undecided                                          | Specify a discovery/design deliverable before implementation      |

These codes apply per slice, not once to an entire F phase. `PASS` is a recorded assertion,
`FAIL` a contradicted assertion, `NOT_PROVEN` missing conclusive evidence. `NOT_RUN` means
the specific procedure was not executed, not that a feature is absent. Production-active
does not mean every release/security/performance obligation is fulfilled.

## 2. Source and acceptance matrix

Source paths in this table are relative to the repository root. Evidence links are
committed records; some raw screenshots/logs are intentionally local/ignored and cannot
be independently rederived from this checkout. A record without raw public evidence is
reported as recorded evidence, not newly reproduced proof.

| Slice / old phase                                      | Status         | Source inspected                                                                                     | Acceptance and precise remaining gap                                                                                                                                                                                                    | Next milestone                      |
| ------------------------------------------------------ | -------------- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| Plain terminal, bounded input/output, resize           | I/P            | `terminal/`, `src/lib/features/terminal/`                                                            | Baseline real operation; full keyboard/IME/render matrix and P1 budgets remain partial                                                                                                                                                  | M4/M8                               |
| Panes/search/shortcuts/saved layout T1/T2              | I/P            | `src/lib/features/terminal/`, `desktop/terminal.rs`                                                  | [ADR0037](../adr/0037-panes-search-keys-and-saved-layout.md); working layout is not faithful host screen recovery                                                                                                                       | M1/M4                               |
| Persistent terminal host F9                            | I              | `host/`, `main.rs`, `scripts/install.sh`                                                             | [ADR0036](../adr/0036-host-process-for-terminals-and-native-sessions.md), [F5 installed preservation](../acceptance/f5-native-wayland-2026-10-20.md): host/terminal survived GUI-only update                                            | M4 adds final fidelity/logout proof |
| Faithful reattach/parser isolation/logout contract     | D/B            | Current `host/ring.rs` is bounded byte history                                                       | Architecture35 requires separate parser failure domain/checkpoint; not proved by existing host                                                                                                                                          | M4                                  |
| Native Claude/Codex launcher/registry                  | I/P            | `native/catalogue.rs`, `recipe.rs`, `registry.rs`, `service.rs`                                      | [F3](../acceptance/f3-2026-10-19.md): both real CLIs and multiple sessions; full feature/consumption matrix not complete                                                                                                                | M1/M2                               |
| Native profiles/inheritance/project default            | I/P            | `native/profile.rs`, `profiles.rs`, `src/lib/features/native/`                                       | F3; F6 effort acceptance below remains partial; session hooks/skills exclusion unsupported for Codex                                                                                                                                    | M2                                  |
| Project selection/scoped authoring                     | I/P            | `agentconfig/project.rs`, `desktop/workspace.rs`                                                     | [ADR0050](../adr/0050-project-level-files-and-hook-mcp-templates.md), native A/B selector/Undo observed                                                                                                                                 | M1                                  |
| Primary project/workspace identity and context routing | D              | Existing native project list and per-window layout are inputs only                                   | No stable full device/project/workspace model or independent per-project workspace migration                                                                                                                                            | M1                                  |
| Inventory/three-state display F1                       | I/P            | `agentconfig/inventory.rs`, `claude.rs`, `codex.rs`, `declared.rs`                                   | [F1/F2a](../acceptance/f1-f2a-2026-10-19.md): inventory/redaction; provider-resolved/session-effective observations not generally available                                                                                             | M2                                  |
| Claude hook/skill switching and sealed restoration F2a | I              | `agentconfig/plan.rs`, `writer.rs`, `vault.rs`, `restore.rs`                                         | F1/F2a; accepted custody ADR0034, not proposed ADR0029 alternatives                                                                                                                                                                     | M2 preserves accepted baseline      |
| Codex skill/MCP switches F2b                           | I/P            | `codex_plan.rs`, `tomlspan.rs`                                                                       | [F2b](../acceptance/f2b-owner-decisions.md): skills/list and MCP evidence; live reload stays unverified where absent                                                                                                                    | M2                                  |
| Account authoring F5                                   | I/P            | `author.rs`, `codex_author.rs`, `document.rs`                                                        | ADR0042–0047 and [instruction acceptance](../acceptance/f5-instruction-editing-2026-10-19.md); not every format/capability supported                                                                                                    | M2                                  |
| Project rules/skills/instructions create/edit/delete   | I/P            | `agentconfig/author.rs`, `project.rs`, `document.rs`                                                 | [native Wayland](../acceptance/f5-native-wayland-2026-10-20.md): exact create/delete/Undo; provider consumption NOT_PROVEN, Codex project skills not supported                                                                          | M2                                  |
| Encrypted hook/MCP templates                           | P              | `config_template.rs`, `instruction_template.rs`                                                      | Same native record: decrypt/prefill/cancel PASS; confirmed imported-template apply NOT_RUN; provider consumption NOT_PROVEN                                                                                                             | M2                                  |
| Model catalogue/thread observation/reroute F6          | P              | `control/runtime.rs`, `sessions/models.rs`, Codex adapter, `src/lib/features/models/`                | [F6](../acceptance/f6-models-2026-10-20.md): source/contracts/older-authority display; real reroute never observed                                                                                                                      | M3/M8                               |
| Native model/effort request                            | P              | `native/profile.rs`, `host/protocol.rs`                                                              | F6: Claude 2.1.304 parse and Codex 0.159.0 config override evidence; updated-host real GUI consumption NOT_PROVEN                                                                                                                       | M2                                  |
| Managed model/effort selection, historical usage       | D/B            | No admitted P2 model/effort fields                                                                   | Accepted ADR0014/0015 and codec exclude selection; keep SG-12 unchanged (SG-12 itself is not a model prohibition); metrics store not implemented                                                                                        | M3                                  |
| Protocol1/V7 Managed Codex                             | I              | `control/`, `adapters/codex/`, `sessions/`, `turns/`                                                 | Accepted ADR0011–0026; P1 remains available separately after F8 activation                                                                                                                                                              | Preserve; retirement OD-04          |
| F8 protocol 2/V8 per-session runtime                   | I              | `f8/{authority,operations,allocator,registry,provider_adapter,v8}.rs`, `desktop/f8/`, GUI controller | [activation procedure/result](../acceptance/f8-activation-procedure-2026-10-22.md): real installed Local Tools Ready→lease→Attached→turn Completed; restart→Stop→Close reverified; ADR0051 Accepted                                     | Preserve                            |
| F8 broad conformance/real tool effects                 | P              | `f8/native_approvals.rs`, tests and 29-method routes                                                 | [method matrix](../acceptance/f8-protocol2-method-matrix.md), [delivery](../acceptance/f8-delivery-2026-10-21.md): integrated fixtures/GUI; production result does not prove every29-method/error/tool case                             | M3/M8                               |
| Managed concurrency above one                          | B              | `docs/contracts/f8/resources.json`, `f8/budget.rs`, registry                                         | Cap1 enforced; multi-process credential refresh NOT_PROVEN; two/four active runtime resource/provider compatibility not qualified                                                                                                       | M5                                  |
| G-INF/native hooks independent of Managed inference    | D/B/P research | `scripts/research/f1b/`, existing isolated Local Tools path, F8 worker                               | [F1b](../research/inference-isolation-2026-10-19.md): E1/E2/E4/E8 recorded; E3 partial, E5/E6 missing then; F8 Local Tools production is not full E9/policy/inference isolation acceptance                                              | M2/M3                               |
| Claude Code Managed                                    | D/B            | `native/catalogue.rs` has Claude; no Managed Claude adapter                                          | Official SDK/headless surfaces documented; auth, effect mediation, no-tool profile, cancellation and consumption unproven                                                                                                               | M3                                  |
| Manual handoff F11a                                    | I/P            | `handoff.rs`, Native handoff UI                                                                      | [F14/F11a/F13](../acceptance/f14-f11a-f13-closure-2026-10-19.md) and [GUI record](../acceptance/f4a-f5-f6-f1b-2026-10-19.md): reviewed Native brief, new recipient, not internal context transfer or confirmed recipient model response | M6                                  |
| OL3 coordinator/write arbitration                      | D/B            | No production scheduler/run journal                                                                  | Architecture34 finite DAG contract; same-UID/native output trust gates remain, no full OL3 implementation                                                                                                                               | M6                                  |
| B1 restricted staged/quarantined worker                | D/B            | `contracts/b1.rs`, `contracts/b1/`, fixtures where tracked                                           | Architecture49–52/reviews: reviewed pure contracts, no production launcher/capture/promotion; FX-H01 auth/egress open                                                                                                                   | M5 design track                     |
| Remote gateway foundations F10                         | P              | `remote/` library; no production constructor/listener                                                | [ADR0049](../adr/0049-remote-gateway-core-foundations.md): typed decisions/tests, no transport/TLS/device enrollment/real asymmetric verifier                                                                                           | M7                                  |
| PC remote client/native Android                        | D/B            | No client implementation                                                                             | Architecture16/35/37 designs; reachability/auth/absent-host/hardware evidence missing                                                                                                                                                   | M7                                  |
| Sealed credentials F14                                 | I/P            | `credentials/`, `agentconfig/vault.rs`                                                               | [closure](../acceptance/f14-f11a-f13-closure-2026-10-19.md) + GUI evidence in F4a/F5 record; no provider injection under ADR0038                                                                                                        | M2 preserve                         |
| Personal installation/update/rollback F13              | I/P            | `scripts/install.sh`, `lifecycle/`, `diagnose.rs`                                                    | ADR0040, F5 preservation, F8 activation; whole final mixed-version/rollback matrix still partial                                                                                                                                        | M8                                  |
| Performance P1                                         | P              | `benchmarks/`, terminal bench tests, idle optimization                                               | [P1](../acceptance/p1-2026-10-20.md), [engineering](../acceptance/engineering-2026-10-20.md): limited backend/idle observations; no complete visual input/render budgets                                                                | M0/M8                               |
| OpenCode F4a                                           | I/P            | `agentconfig/opencode.rs`, `opencode_plan.rs`                                                        | F4a real byte roundtrip; native catalogue has no OpenCode; project mutation coverage limited                                                                                                                                            | M9                                  |
| Hermes Native/Wyss Agent planner F4b/F12               | D/B            | Archived source, not a production Console adapter                                                    | Owner DN06=D already selected; useful safe planner integration/auth/egress remains unproven                                                                                                                                             | M9                                  |
| All themes/a11y/notifications/final screen states      | D/P            | Existing application styles/settings, no claim of complete matrix                                    | Architecture13/37 target; reference-device end-to-end acceptance remains                                                                                                                                                                | M1/M8                               |

Source root for shortened Rust paths is `src-tauri/src/`. Frontend/layout filenames are
indexed in the documentation inventory/code references; the authoritative current
terminal manager is `src/lib/features/terminal/session-manager.ts`.

## 3. F8 closure, without overclaiming

The last activation result and ADR0051 acceptance addendum supersede earlier F8
inactive/Proposed statements in CHECKPOINT, runbooks, reports and historical architecture.
The latest commit fixes stop/close of a wanted runtime after authority restart.
The real provider is pinned to 0.159.0. The installed F8 authority is separate from the
retained P1 authority/host; neither store is a continuing mirror of the other.

Still open: multi-process refresh behaviour; capacity above1; full tool/error conformance
on real provider; new Managed model choice; Claude Managed; full OL3; retiring P1;
updated host acceptance and final GUI ergonomics. The existence of per-session V8 keys
does not prove any of these. Contract JSON retaining `candidate_not_production` describes
its historical contract artifact status, not whether the installed F8 authority is active.

## 4. Blockers that the redesign must not erase

Same-UID human-approval integrity, unrestricted Native/LP4 effects, rootful admin
delegation, B1 useful auth/egress, parser failure isolation, provider refresh compatibility,
external configuration writers/snapshots and absent-host remote control each need the
specified evidence/decision. Earlier REJECT reports remain historical security findings;
passing this documentation review cannot declare their product-level tests passed.

No current machine observation is inferred from historical reports. Refresh drift-prone
environment facts before any later operational action.

## M0 engineering baseline

Qualification date: 2026-10-22. PR16 head `de0b8c8` merged at `d2ee8d2` after docs-only
scope, six canonical routes, formatting, 208 legacy/31 FP crosswalk and links passed.
The only P2 (supported package installation/launch) was corrected and its thread resolved;
no Changes Requested review remained. GitHub reported no CI workflow runs/statuses, not
an invented CI pass. Merge did **not** accept ADR0052/0053. M0 branch changes documentation
only; no installation, package rebuild, SQL access/migration or provider inference.

### Component ownership and M1 risks

Paths below are relative to `src-tauri/src/` unless marked frontend. This inventory is
bounded to the owners M1 touches; the detailed historical inventory is unchanged.

| Owner / interface                                                                                     | Existing state and required preservation                                                                                                                                                                                                         |
| ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `desktop.rs`, `application.rs`, command extensions/capability manifests                               | GUI composition/closed IPC; add project projection commands without exposing arbitrary argv/path or widening authority capabilities                                                                                                              |
| `host/{client,core,protocol,server}.rs`                                                               | Remote host owns PTYs/Native lifecycle; protocol1 host hello only advertises profile_effort, no stable instance feature. Lazy connect can start a host/local fallback and retry: do not use for M1 observation/recovery                          |
| `terminal/{mod,contracts,worker,buffer}.rs`, `host/ring.rs`                                           | Host terminal UUIDs,16-slot terminal service, bounded byte history; retain owners/cursors and disclose gaps, not faithful screen restoration                                                                                                     |
| `native/{catalogue,recipe,service,registry,profiles}.rs`                                              | Claude/Codex launch/32-hex session IDs, versioned profiles/defaults. Registry schema1 is bounded2MiB/200 sessions/100 folders; open rewrites unfinished sessions Lost and can fall back empty on bad input. Never import by opening that service |
| frontend `terminal/{session-manager,layout,controller}.ts`, `host/commands.rs`                        | Global saved layout v1,64KiB,16 tabs/16 leaves per tab,binary depth6; save debounce400ms. Restore may recreate missing shells (never agents); M1 requires attach-only recovery, explicit new launch and one shared host cap                      |
| `agentconfig/{service,project,plan,writer}.rs`, `native/store.rs`                                     | Real scoped config/native chooser validation, descriptor/ACL/write/Undo owners. Config roots inside HOME excluding HOME/provider folders differ from Native launch eligibility. New project membership cannot widen either                       |
| `sessions/`, `turns/`, `control/`, `f8/{authority,allocator,registry,engine,production,ownership}.rs` | Separate P1/V7 and P2/V8 writers, leases, fences and owned-process lifecycle. M1 associates immutable owner refs outside their stores; never rebinds a runtime's directory                                                                       |
| `lifecycle/`, `scripts/{install.sh,f8-activation.sh}`                                                 | Ordinary GUI/authority installation differs from explicit F8 activation. Keep running binaries/owners and --keep-host behaviour; installer rollback can close live terminals                                                                     |

M1 also needs cross-window CAS, context capture before dialogs/IO, fail-closed newer/corrupt
metadata, no native launch replay after lost receipt, legacy external-writer detection,
profile deletion/version drift, root replacement/relink, and retention beyond legacy
registry eviction. None is solved by a directory path as project ID. Exact proposed
records/DTOs and M1-01..13 oracles: [M1 contract](../contracts/m1-workspace.md).

### Current installation observations (not new provider acceptance)

Read only: bounded `--version`, host hello and F8 authority/hello over existing sockets;
known unit ActiveState/SubState/MainPID/start timestamp; socket/file metadata and binary
digests. No credential files, process environments, prompt/output/session payloads or
installed SQLite files were read. The first malformed F8 hello lacked schema_version and
was refused; corrected closed schema2 hello succeeded, with no operational request sent.

- PATH probes: Claude2.1.309, Codex0.159.0. Exact compatibility limits and mode/profile
  evidence live in the [single provider manifest](provider-matrix.md#m0-installed-build-manifest).
- Host0.2.0, protocol1, native true, feature `profile_effort` observed. This establishes
  feature negotiation only, **not** new real effort/model consumption acceptance.
- `wyss-console-f8.service` active/running; explicit `--protocol 2`; hello reports protocol2,
  storage schema8, service0.2.0. `wyss-control.service` separately active/running. F8 runtime
  activity/capacity was not exercised; real Codex execution remains the recorded acceptance.
- Actual F8 socket is runtime-root/**wyss-console/control.sock**, not directly at the
  runtime-root. Store uses the same product subdirectory under state-root (`control/paths.rs`).
- Installed GUI SHA256 `6467aed1a4ac28ac1dec991d725bb16bb37d7e5c530953cb0320cb5a1ec389d0`;
  installed F8 authority `fbd0e793b95575724716de250a7b46817f35e7d839f79ffd700907c684fddaab`.
  The retained ordinary 0.2.0 artifact in the other checkout has different digests;
  version labels alone do not prove binary equivalence. Neither was rebuilt/replaced.
  Installed binary-to-source reproducibility remains NOT_PROVEN.

### F8 resource enforcement reconciliation

Every numeric ceiling in [resources.json](../contracts/f8/resources.json) is covered
below (contract_version is metadata). `budget::classify` remains unchanged in code;
its NOT_PROVEN labels are conservative/stale for several later paths. A source bound or
clock-injected fixture is not a measured OS deadline/whole-process-tree memory cap.

| Contract keys                                                                                                                                       | Actual path / qualification                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| --------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| max_active_runtimes=1                                                                                                                               | registry::new/with_history fixes limit1; reserve refuses before extra admission; stopping retains slot until owned reap. Test-only with_test_limit does not expose production capacity. Registry+engine fixture PASS; production overload/reap fault campaign NOT_RUN                                                                                                                                                                                                                        |
| max_runtime_rows, max_launch_rows                                                                                                                   | allocator + V8 SQL lifetime triggers; f8_contracts durable_limits fixture PASS; no installed DB inspection/migration                                                                                                                                                                                                                                                                                                                                                                         |
| client_frame_bytes                                                                                                                                  | protocol parse + control/server F8Transport framing, rejects before JSON; transport fixture PASS                                                                                                                                                                                                                                                                                                                                                                                             |
| max_clients, hello_timeout_ms                                                                                                                       | F8Transport::attach caps8, deadline3s, poll expires partial clients; clients_and_partial_frames fixture PASS. budget::classify still says NOT_PROVEN; cooperative timeout, not hard realtime                                                                                                                                                                                                                                                                                                 |
| provider_frame_bytes                                                                                                                                | ProcessWorker inbound bound262144 + Meter; fixtures/source, no all-provider conformance claim                                                                                                                                                                                                                                                                                                                                                                                                |
| provider_scratch_bytes                                                                                                                              | Contract4096; ProcessWorker stdout CHUNK16384 (stderr scratch4096). Exact stdout scratch ceiling **not met**, despite bounded reads; separate from frame/queue bounds                                                                                                                                                                                                                                                                                                                        |
| catalogue_bytes_per_runtime                                                                                                                         | production::PinnedCatalogue read/metadata limit4MiB + adapters/codex/safety catalog validation; source enforced, classifier stale; per-runtime end-to-end allocation ceiling NOT_PROVEN                                                                                                                                                                                                                                                                                                      |
| catalogue_models_per_runtime                                                                                                                        | Contract 50; `f8/provider_adapter.rs` rejects catalogue observations over 50 and `f8/engine.rs::record_models` independently rejects snapshots over 50. **Exact snapshot admission limit is enforced in the F8 path**; the upstream `safety::MAX_CATALOG_MODELS` of 64 is not the F8 accepted snapshot limit. `budget::classify` remains stale (`NotProven`) and should be reconciled separately. This does not certify pagination completeness, per-runtime memory cost or concurrent load. |
| data_queue_events_per_runtime, data_queue_bytes_per_runtime, control_queue_events_per_runtime, control_event_bytes, control_queue_bytes_per_runtime | scheduler::Limits loads contract, lanes refuse/backpressure or fault their runtime; scheduler fixture PASS                                                                                                                                                                                                                                                                                                                                                                                   |
| max_data_events_per_quantum, control_dispatch_deadline_ms                                                                                           | Scheduler/engine control-first dispatch and late/stall detection; fixture PASS. OS scheduling/real-provider latency/fairness not measured; no hard realtime promise                                                                                                                                                                                                                                                                                                                          |
| control_service_quantum_ms                                                                                                                          | Contract50ms is classified Enforced but scheduler does not load this key; service uses nominal25ms supervisor cadence. Exact50ms service guarantee under load is NOT_PROVEN, not certified by scheduler fixtures                                                                                                                                                                                                                                                                             |
| turn_output_bytes, turn_events, turn_events_per_second, turn_event_burst, pending_approvals_per_runtime                                             | Meter exact-key/latching/token-bucket fixtures PASS; engine integration source, real saturation NOT_RUN                                                                                                                                                                                                                                                                                                                                                                                      |
| turn_wall_seconds, provider_init_timeout_ms                                                                                                         | Meter/engine poll deadline and missed-handshake path; source enforcement, cooperative progress required; real load deadlines NOT_PROVEN                                                                                                                                                                                                                                                                                                                                                      |
| stream_turns_per_runtime, stream_events_per_turn, stream_bytes_per_turn                                                                             | control/turn_stream::Streams retains8 turns globally,256 events/256KiB each, drops with gap; used by F8 authority. A global8 is bounded at current cap1, not independently per runtime for future concurrency. Classifier stale; multi-runtime retention/fairness NOT_PROVEN                                                                                                                                                                                                                 |
| max_attachments                                                                                                                                     | Contract128; P1 Conversations has128 cap, F8 operations Prepare caps128 durable sessions and one runtime maps current attachment. A distinct F8 attachment-count enforcement proof is NOT_PROVEN; do not cite P1 bound as F8 proof                                                                                                                                                                                                                                                           |
| eof_grace_ms, term_grace_ms                                                                                                                         | ownership Grace + engine stopping/drain/reap; grace deadlines are bounded requests, uncompleted reap retains slot. No unattended lifecycle experiment run                                                                                                                                                                                                                                                                                                                                    |
| owned_root_rss_soft_kib, aggregate_owned_roots_rss_soft_kib                                                                                         | Soft classification/diagnostics only; no hard cap or aggregate descendant-memory containment proof. NOT measured in M0                                                                                                                                                                                                                                                                                                                                                                       |

The capacity-one release is real; the entire resource contract is **not** certified by
that fact. Track the provider stdout scratch mismatch, stale catalogue classifier and other incomplete enforcement/load evidence in M5,
plus actual launcher/transport/load fairness before any capacity expansion. Keep1 in M1.

### Performance baseline and missing release measurements

[P1 backend/idle report](../acceptance/p1-2026-10-20.md) records three backend runs and
an idle optimization with outliers retained; it explicitly leaves P1 open. PTY-to-batch
throughput/roundtrip and an in-process wakeup result are not input-to-photon, a new host
CPU measurement, or full desktop budgets. [Earlier engineering failures/limits](../acceptance/engineering-2026-10-20.md)
remain historical, superseded only for specifically later accepted slices. M0 runs no
new performance/real-provider benchmark while live work continues.

Freeze [architecture15 sections3–5,11](../architecture/15-PERFORMANCE-ENGINEERING.md):
record source/build/digests/machine/power/load/swap, owned process boundaries including
WebKit/helpers/providers separately; same corpus/sample/warmup procedure, three independent
runs, raw failures/outliers/confidence intervals retained. Missing gates: visual input/
render/resize, foreground/background16-terminal idle CPU/wakeups/RSS/PSS, project-switch
p50/p95/p99 and restore at bounded metadata load, long-run loss/recovery, Managed tree
memory/deadlines under load, reference second-device comparison. M1-13 requires matched
before/after runs and reviewed measured budgets **before release**; no guessed numerical
latency/resource promise is substituted. Full product performance remains M8.

### Preservation and M1 readiness

M0 observes current ownership without signaling anything. Local temporary before/after
records contain only unit start identities/socket inodes/legacy metadata/binary digests;
no personal content is copied into the repository. Keep exact current identities only
as comparison evidence, never future kill targets. M1's prepared preservation/migration/
rollback procedure is [contract section4](../contracts/m1-workspace.md#4-migration-preservation-and-rollback).
It snapshots only relevant private legacy metadata at a future explicit adoption step,
keeps old/new records and a deterministic map, and never restores over a live writer.
GUI rollback must account for new records and old shell recreation behaviour; installer
rollback is not an unattended escape hatch. No host stop, SQL downgrade or F8 replacement.

**Readiness:** bounded M1 implementation can start with pure codecs/fixtures and the
specified increments, without deciding remote transport or Claude Managed feasibility.
M1-01 passes at the closed-schema and strict-codec level only (`contracts::m1::tests`, increment 1,
unwired and unreleased); its "before save" wiring and M1-02..13 and the native project journeys
remain **NOT_RUN** acceptance obligations.
ADR0052 is now Accepted as an architectural decision, not product/release acceptance.
Its 13 M1 acceptance tests remain NOT_RUN and adoption needs real-desktop evidence,
preservation/rollback and a separately authorized deployment. No new owner product choice
is needed for the recommended proposal; retain WD01/02, P1 and F8 cap1. Source/installed
binary correspondence, exact F8 resource mismatches, real-desktop budgets and provider
consumption remain explicit evidence gaps, not reasons to restart production in M0.

### M0 validation record

Targeted fixture commands (repository tool wrapper, locked dependencies; no blanket
cargo test/lifecycle_process, ignored provider or keyring tests):

```sh
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib f8::budget::tests -- --test-threads=1
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib f8::scheduler::tests -- --test-threads=1
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib f8::service::tests -- --test-threads=1
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib f8::tests::registry_holds -- --test-threads=1
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib f8::engine::tests::f8_07 -- --test-threads=1
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --test f8_contracts -- --test-threads=1
npx vitest run tests/pane-layout.test.ts tests/terminal-sessions.test.ts
```

PASS:27 Rust fixture tests (5+8+2+1+1+10),38 frontend tests. An initial wrong Rust module
filter selected zero tests; it is excluded, corrected commands above ran nonzero counts.
Final documentation/scope/preservation checks are recorded with the M0 delivery below.

Final M0 checks: changed Markdown Prettier PASS;487 Markdown files/2101 local references,
zero broken links;31 FP/AC and208 legacy mappings retained; all33 numeric F8 resource
ceilings covered. Accepted/historical ADRs unchanged except ADR0052's own status and acceptance section. Repository scanner5/5 PASS.
Before/after P1/F8 service start identities, host/F8 socket metadata, Native registry,
layout and installed-binary metadata unchanged. This comparison proves no observed owner
replacement or metadata rewrite during M0; it does not certify every live session's
internal state. Scope: documentation files only (the M1 contract, canonical pages and the ADR0052 status), no application implementation.
