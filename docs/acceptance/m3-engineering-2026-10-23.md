# M3 Engineering acceptance record — 2026-10-23

**Branch:** `feat/m3-managed-parity-2026-10-23` · **Base/head at start:**
`def69fe35ddfe8b696b65a12d2f2babe296c8247` · **PR #22:** separate M2 PR, observed open;
not merged or included in this branch.

This record distinguishes executable fixture evidence from real-provider and release
acceptance. A requirement is not PASS without the named runnable test and its result.
No real provider turn, sign-in, credential/config inspection, provider refresh, installation,
host/authority restart, session stop, production database access or deployment is authorized
or performed in this M3 pass.

## Acceptance matrix

| Oracle                                                | Requirements                            | Fixture/unit | Integration/GUI  | Real provider | Engineering result                                                  | Evidence locator                                                           |
| ----------------------------------------------------- | --------------------------------------- | ------------ | ---------------- | ------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| M3-01 Claude exact interface/auth/capability          | FP-SES-02/03, CFG-05, SEC-01/03         | NOT_RUN      | NOT_RUN          | NOT_RUN       | BLOCKED: SDK candidate only; E1–E9 missing for Claude               | [feasibility report](../research/claude-managed-feasibility-2026-10-23.md) |
| M3-02 Claude Inference no-effect profile              | FP-SES-02/03, CFG-05, SEC-01/03         | NOT_RUN      | NOT_RUN          | NOT_RUN       | BLOCKED / NOT_PROVEN                                                | `f8/launcher.rs`, Claude profile absent                                    |
| M3-03 Claude Local Tools action mediation             | FP-SES-02/03, SEC-01/02/03              | NOT_RUN      | NOT_RUN          | NOT_RUN       | BLOCKED / NOT_PROVEN; callback alone insufficient                   | [ADR0053](../adr/0053-claude-code-managed-integration-boundary.md)         |
| M3-04 Provider-neutral lifecycle and Codex regression | FP-SES-02/03, CTX-04                    | PASS: Codex  | PASS: fixtures   | NOT_RUN       | Codex lifecycle regression PASS; cross-provider adapter absent      | `src-tauri/src/f8/`, `src/lib/features/f8/`                                |
| M3-05 Fences/leases/receipts/recovery/capacity 1      | FP-SES-02/03/04, SEC-01/02/03           | PASS: Codex  | PASS: fixtures   | NOT_RUN       | Existing F8 fences/recovery PASS; Claude remains unqualified        | `src-tauri/src/f8/{authority,operations,engine,allocator}.rs`              |
| M3-06 Managed model/effort extension                  | FP-MOD-01, SES-02/03, CFG-05            | NOT_RUN      | NOT_RUN          | NOT_RUN       | PROPOSED; live selection disabled                                   | [ADR0054](../adr/0054-managed-model-selection-and-effort.md)               |
| M3-07 Provider usage/limits                           | FP-MOD-02, SEC-03, NFR-01               | NOT_RUN      | NOT_RUN          | NOT_RUN       | NOT_IMPLEMENTED; F8 exposes no provider-usage DTO                   | F8 provider DTO/source                                                     |
| M3-08 Optional local usage history                    | FP-MOD-02, SEC-03, NFR-01               | NOT_RUN      | NOT_RUN          | NOT_RUN       | PROPOSED; persistence disabled; contract bounds corrected on review | [metrics/store contract](../contracts/m3-model-usage-history.md)           |
| M3-09 GUI, provenance and project/session context     | CTX-04, SES-02/03, MOD-01/02, SEC-02/03 | PASS: jsdom  | NOT_RUN: desktop | NOT_RUN       | Disabled-state/no-fallback view PASS; Claude workflow absent        | `tests/f8-view.test.ts`, `src/lib/features/f8/OverviewPanel.svelte`        |
| M3-10 Adversarial security and recovery               | SES-02/03, CFG-05, SEC-01/02/03         | PASS: Codex  | PASS: fixtures   | NOT_RUN       | Existing Codex fixtures PASS; Claude-specific matrix NOT_RUN        | `src-tauri/src/f8/*tests.rs`, `src-tauri/tests/`                           |
| M3-11 Performance/resource comparison                 | SES-04, MOD-02, NFR-01                  | NOT_RUN      | NOT_RUN          | NOT_RUN       | NOT_RUN                                                             | Architecture15 method; cap remains 1                                       |
| M3-12 Compatibility/release/rollback                  | All M3 requirements                     | NOT_RUN      | NOT_RUN          | NOT_RUN       | BLOCKED pending ADRs, real-provider evidence and owner release      | This record + final PR                                                     |

## Required executable evidence

The commands ran on this branch; exit status and result summaries are recorded below.
Console output was not saved as separate artifacts.

| Area                           | Command / procedure                                                                                      | Result                                                                                                            |
| ------------------------------ | -------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Rust formatting                | `npm run rust:fmt`                                                                                       | PASS (exit 0)                                                                                                     |
| Rust F8/authority contracts    | `npm run rust:test -- --quiet`                                                                           | PASS: 1,561 passed, 24 ignored, 0 failed (exit 0)                                                                 |
| Rust static checks             | `npm run rust:check`, `npm run rust:clippy`                                                              | PASS (both exit 0; Clippy `-D warnings`)                                                                          |
| Frontend lint/type/tests/build | `npm run lint`, `npm run typecheck`, `npm test`, `npm run build`                                         | PASS: 987 tests; build emits existing >500 kB chunk advisory                                                      |
| Store/security harness         | `npm run test:harness`                                                                                   | PASS: 16 tests in 3.974s                                                                                          |
| Format                         | `npm run format:check`                                                                                   | PASS after formatting the new contract, feasibility and acceptance Markdown records                               |
| Tauri GUI                      | Isolated desktop + disposable roots and fake host/provider                                               | NOT_RUN: no fixture desktop runner is wired; no normal app launch that could contact installed authority occurred |
| Security boundary              | `authority_boundary` (1), `f8_contracts` (10), `f8_serve_process` (8) and Rust adversarial unit fixtures | PASS (fixture-only; included in Rust run)                                                                         |
| Codex Managed regression       | Existing F8 unit/process/GUI fixtures; no live authority/provider                                        | PASS (fixture-only); current global runtime capacity remains 1                                                    |
| Real-provider turns/refresh    | Requires separately authorized exact test and rollback                                                   | NOT_RUN; unauthorized in this pass                                                                                |

## Provider feasibility and decisions

- Official candidate: Python Agent SDK `0.2.173` + bundled Claude Code `2.1.309`; TS
  candidate `0.3.308` + Claude Code `2.1.308`. SDKs are not installed or qualified for Wyss.
  Local read-only version probes returned Claude Code `2.1.309` and Codex `0.159.0`; this
  does not establish account, entitlement or Managed compatibility.
- Claude Inference: BLOCKED until no-tool behavior, effective managed/user/project policy,
  hooks, connectors/MCP, plugins, subagents, auth/refresh and recovery pass the pinned
  negative/positive matrix.
- Claude Local Tools: BLOCKED until provider-originated actions can be held and dispatched
  only by Wyss authority, with every bypass excluded. `canUseTool` is not sufficient.
- Managed model selection: remains disabled pending Santiago's acceptance of ADR0054 and
  explicit protocol capability acceptance.
- Metrics persistence: remains disabled pending Santiago's acceptance of the separate
  store/privacy contract.
- F8 runtime capacity stays 1. P1 and existing P2 behavior must remain unchanged.

## Independent review

One focused read-only Luna review found a medium-severity gap in the proposed metrics
contract: row count was bounded but field sizes and provenance values were open. The contract
now limits canonical rows to 1,024 bytes, bounds opaque identifiers, closes provenance to an
enum, and specifies 12 MiB database / 24 MiB total-store / 12 MiB export limits with reject
and rollback tests. The documentation finding is addressed; no persistence implementation
exists yet, so those runtime limits remain unimplemented and must be tested before enablement.
The reviewer found no other confirmed contradiction in the reviewed surfaces. This review
does not qualify a Claude adapter or replace final code review of one.

## Preservation and release gates

No live installation, active host/authority, real session, provider credential, provider
configuration, production database or provider quota was touched. Evidence is repository
source/fixture only until results are appended here. Release requires exact supported build
and authority matrix, both Claude profile-specific evidence sets, accepted ADR0053/0054 as
applicable, accepted metrics policy before enabling history, independent security review,
authorized real-provider tests and explicit Santiago release/adoption decision. Rollback only
disables new Claude/model/metrics admissions; no active service restart or session kill.
