# M3 Engineering acceptance record — 2026-10-23

**Branch:** `feat/m3-managed-parity-2026-10-23` · **PR #23** · **M2 PR #22:** separate, observed open, not
merged and not touched. **Result:** `ENGINEERING_COMPLETE_RELEASE_BLOCKED`.

Everything that does not need an authorization only Santiago can give is built and tested. What is left
is a decision, a provider run or a release, and each one is listed in
[Owner decisions](#owner-decisions-and-pending-real-provider-tests). Implementation completeness is not
production authorization: every live gate (`m3::gate`) is `false`, and nothing here installs, restarts or
admits anything on the owner's machine.

## Status matrix

Status words: `IMPLEMENTED_AND_TESTED`, `IMPLEMENTED_BUT_UNQUALIFIED`, `PROVIDER_EVIDENCE_REQUIRED`,
`OWNER_APPROVAL_REQUIRED`, `NOT_IMPLEMENTED`, `DEMONSTRATED_TECHNICAL_BLOCKER`.

| Oracle                                                 | Status                        | What exists                                                                                                                                                                                                                                      | What is still needed                                                                             |
| ------------------------------------------------------ | ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| M3-01 Claude interface, version, auth, capability      | `IMPLEMENTED_BUT_UNQUALIFIED` | Exact pair pinned and verified against the pip RECORD files and the bundled CLI hash; hash-locked install; capability discovery and a three-leg attestation; E1–E9 run offline ([report](../research/claude-managed-feasibility-2026-10-23.md)). | Supported auth route: ADR0053. A real-provider run.                                              |
| M3-02 Claude Inference                                 | `IMPLEMENTED_BUT_UNQUALIFIED` | `claude_inference_v1`: no tools offered in any request (checked in three places), hostile configuration changes nothing, nothing outside the workspace changes.                                                                                  | Same as M3-01.                                                                                   |
| M3-03 Claude Local Tools                               | `IMPLEMENTED_BUT_UNQUALIFIED` | `claude_local_tools_v1`: only two authority-owned proposal tools; a proposal blocks until the authority decides; only the authority's executor has an effect; declined, cancelled, failed, indeterminate; no replay.                             | Same as M3-01.                                                                                   |
| M3-04 Provider-neutral lifecycle and Codex regression  | `IMPLEMENTED_AND_TESTED`      | The Claude worker plugs in at the engine's `Worker` seam; nine authority-level tests run the real engine, authority and schema-8 writers with a Claude factory. Codex F8 suites unchanged and green.                                             | —                                                                                                |
| M3-05 Identity, leases, receipts, recovery, capacity 1 | `OWNER_APPROVAL_REQUIRED`     | Fences, receipts, lost-runtime-is-indeterminate and capacity 1 hold with a Claude worker at the seam. **The V8 store cannot hold a Claude runtime** (CHECK `adapter_id='codex'`): ADR0055, demonstrated below.                                   | Accept ADR0055 (V9 migration, rehearsal on a copy), then run the parity suite on the real store. |
| M3-06 Managed model/effort extension                   | `OWNER_APPROVAL_REQUIRED`     | `m3::model` with the gate closed; four facts kept apart; reroute from the provider message; per-turn model and runtime-start effort as the build allows.                                                                                         | Accept ADR0054.                                                                                  |
| M3-07 Provider usage and limits                        | `IMPLEMENTED_BUT_UNQUALIFIED` | Provider-reported counters with provenance and completeness; a figure not reported is "not reported", never zero; estimates labelled and never billing. Codex reports no usage in F8, so none is shown for it.                                   | Real-provider figures. A Codex usage source does not exist yet.                                  |
| M3-08 Optional local usage history                     | `OWNER_APPROVAL_REQUIRED`     | Separate SQLite file, closed schema, 1,024-byte rows, quotas, dedupe, export, reset, failure isolation, default off.                                                                                                                             | Accept the privacy and retention policy.                                                         |
| M3-09 GUI, provenance, project/session context         | `IMPLEMENTED_BUT_UNQUALIFIED` | Providers window (profiles with reasons, four model facts, usage, history settings, fixture console) with a strict decoder; run end to end in the real Tauri window (7 scenarios).                                                               | Claude sessions inside the installed authority wait for ADR0055.                                 |
| M3-10 Adversarial security and recovery                | `IMPLEMENTED_BUT_UNQUALIFIED` | 51 bridge/tap tests, strict-frame and policy suites, 14 real SDK/CLI tests, hostile-config and built-in-tool-demand tests; review finding fixed. The review was by the author, with the repo's review skill.                                     | An independent reviewer other than the author; real-provider adversarial runs.                   |
| M3-11 Performance and resource qualification           | `PROVIDER_EVIDENCE_REQUIRED`  | Claude arm: 7 repeated runs per profile, raw samples kept ([report](evidence/m3-perf-2026-10-23/report.md)).                                                                                                                                     | Codex arm on the same workload (needs a Codex runtime started here).                             |
| M3-12 Compatibility, release, rollback                 | `OWNER_APPROVAL_REQUIRED`     | Rollback disables new Claude admissions only; Codex, Native, history and the installed Console are untouched by this branch.                                                                                                                     | Santiago's release decision.                                                                     |

## The blocker, demonstrated

`docs/contracts/f8/v7-to-v8.sql` declares `CHECK(adapter_id='codex')` and `CHECK(runtime_kind='codex')` on
`managed_runtimes` and on `managed_runtime_launches`; `RuntimeTarget::supported()`, `RuntimeSafetyProfile`,
`complete_handshake` and the `"adapter_id":"codex"` literals in the allocator and operations are Codex-only.
M3 does not migrate without an accepted ADR, so the integration stops at the `Worker` seam and the admission
is closed by construction (`m3::gate::STORE_HOLDS_CLAUDE = false`, shown in the window). Options and a
recommendation: [ADR0055](../adr/0055-multi-provider-managed-runtime-store.md). A V9 rehearsal on a copy
of a store was **not** run.

## Executable evidence

| Area                       | Command                                                 | Result                                                                                         |
| -------------------------- | ------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Rust format, check, clippy | `npm run check:rust`                                    | PASS (clippy `-D warnings`, also with `--features m3-fixture --examples`)                      |
| Rust tests                 | `npm run rust:test`                                     | PASS: 1,312 lib (25 ignored), 493 bins, 1 + 10 + 8 contract/process                            |
| Real SDK and CLI, offline  | `npm run test:claude-real` (`WYSS_CLAUDE_SDK_ROOT` set) | PASS: 14 of 14                                                                                 |
| Bridge and tap             | `npm run test:claude-bridge`                            | PASS: 51                                                                                       |
| Frontend                   | `npm run format:check`, `lint`, `typecheck`, `npm test` | PASS: 1,028 tests, 0 lint or type errors                                                       |
| Store/security harness     | `npm run test:harness`                                  | PASS: 16                                                                                       |
| Repository scan            | `npm run scan:repo`                                     | PASS: 5 of 5                                                                                   |
| Isolated desktop           | `python3 -I benchmarks/m3_gui_suite.py …`               | PASS: 7 of 7 scenarios, no process or directory left ([evidence](evidence/m3-gui-2026-10-23/)) |
| Performance                | `npm run perf:m3 -- --sdk-root … --runs 7 --out …`      | 7 runs per profile ([evidence](evidence/m3-perf-2026-10-23/))                                  |

### Isolated desktop

The real window (Tauri, WebKit) ran in a nested compositor on a private display with its own session bus,
HOME and XDG directories, the real SDK and CLI behind it and a mock provider on loopback. Scenarios:
`inference`, `local-tools-accept` (command run only after Accept, result reaches the model, then a file diff
accepted), `local-tools-decline` (decline and cancel: nothing written), `local-tools-cancel-turn`,
`cancel-stream` (interrupted mid-stream, partial text kept, runtime stays ready), `stop-restart` (two lives of
the runtime), `refused` (an installation the verifier refuses is shown in plain words). Each scenario checks
the disk and the mock's request log, not only the picture.

Defects it found, all fixed with a regression test: (1) the window could not read the state because
`m3_state` was missing from the Tauri ACL (new capability `m3`; the fixture commands are granted at run time
by the fixture binary only, and the package grants none); (2) after an approved command the next turn was
refused for good because the finished executor was never reaped; (3) a 240 px empty box in the history
card and "1 turns". Codex in the window is covered by the existing F8 fixture suites, which are unchanged.

### Performance (Claude arm, offline, 7 runs per profile)

| Figure                      | Inference p50 / max | Local Tools p50 / max | Proposed budget (1.5 × p95) |
| --------------------------- | ------------------- | --------------------- | --------------------------- |
| Launch to ready (cold)      | 2,227 / 2,249 ms    | 2,256 / 2,269 ms      | 3,400 ms                    |
| First streamed text         | 41.0 / 41.1 ms      | 41.0 / 41.1 ms        | 62 ms                       |
| Stop, whole tree reaped     | 503 / 523 ms        | 503 / 522 ms          | 790 ms                      |
| Cancel while streaming      | 10.4 / 10.6 ms      | —                     | 16 ms                       |
| Process-tree PSS when ready | 290 / 291 MiB       | 302 / 303 MiB         | 450 / 460 MiB               |
| Processes, FDs              | 4, 34               | 4, 34                 | 6, 51                       |
| Idle CPU of the tree        | 3.0 / 3.2 %         | 3.2 / 3.2 %           | 5 %                         |
| Left after stop             | 0                   | 0                     | 0                           |

The idle CPU belongs entirely to the `claude` process; the bridge and the sandbox use none. A budget is a
proposal for Santiago to accept or change; nothing enforces it. Codex was not measured (see below).

## Independent review

One review pass over the security-relevant code (tap, bridge, executor, launcher, sandbox, ACL) was made by
the author with the repository's security-review skill; **it is not independent**, and an independent
reviewer remains a release gate. Findings:

| Finding                                                                                                                    | Severity | Fix                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| The tap listens on the host's loopback and injects the real credential into any request: any local process could spend it. | Medium   | The CLI is given a per-run key that is also the tap's password; a stranger gets a silent 401 (no event, no budget, nothing upstream). Tests added. |
| The tap checked `tools` but not body fields that make the provider run something on its side.                              | Low      | `mcp_servers` and `container` are refused. Test added.                                                                                             |
| The `refused` path of the example aborted instead of showing the refusal.                                                  | Low      | The window opens and says why.                                                                                                                     |

Residual risks, not fixed because they are the design: the CLI runs with the host's network (the tap is a
check on what the model is offered, not a network boundary); an administrator's managed policy makes the
installation be refused, never overridden; the 50 notice subtypes seen on this build are the ones classified,
and an unknown one stops the runtime; a later CLI needs a new qualification.

## Owner decisions and pending real-provider tests

| Decision / test                                    | Evidence                                                       | Recommendation                                                                    | Risks                                         | Unlocks                                    |
| -------------------------------------------------- | -------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------- | ------------------------------------------ |
| ADR0053: Claude auth route and exact build         | ADR0053 evidence section                                       | A: API key held by Wyss, injected by the tap; only for the recorded build         | Per-token cost; one real run to prove 401/529 | Real Inference and Local Tools runs        |
| ADR0054: model and effort selection                | ADR0054 evidence section                                       | Accept, with Claude's effort as a runtime-start setting                           | New protocol version and negotiation          | Live selection tests and controls          |
| ADR0055: a store that can hold a non-Codex runtime | [ADR0055](../adr/0055-multi-provider-managed-runtime-store.md) | A: V9 migration, rehearsed on a copy, downgrade refused                           | Older Consoles cannot open a V9 store         | Claude sessions in the installed authority |
| Usage-history privacy and retention policy         | [contract](../contracts/m3-model-usage-history.md)             | Accept as written (off by default, 10,000 records or 30 days)                     | Local metadata on disk                        | History on                                 |
| Real-provider Inference run                        | —                                                              | One short turn, owner-supplied key, rollback = stop runtime and delete the key    | Paid; leaves a provider-side record           | M3-01/02 qualification                     |
| Real-provider Local Tools run                      | —                                                              | One command and one file change, in a throwaway directory                         | Same                                          | M3-03 qualification                        |
| Codex runtime on the same workload                 | Harness is symmetrical                                         | Run `perf:m3` shape against a Codex runtime started for the purpose               | Starts a Codex process                        | M3-11 comparison                           |
| Independent security review                        | Findings above                                                 | A reviewer other than the author, before any release                              | —                                             | M3-10 as `IMPLEMENTED_AND_TESTED`          |
| Release and installation                           | —                                                              | Install the new package only after the above; rollback disables Claude admissions | —                                             | M3-12                                      |

## Preservation

Nothing installed, restarted or admitted: no installed Console file, host, authority, session, provider
credential or configuration, production database or provider quota was touched. The experiments ran in
disposable directories, a private display and a private bus; every process was a direct child ended by its
handle (no process was found or killed by name). Global runtime capacity stays 1, the P1/P2/Codex code paths
and the V8 schema are unchanged, and the fixture host is absent from the package (see
`scripts/package-fixtures.mjs` and the ACL tests).
