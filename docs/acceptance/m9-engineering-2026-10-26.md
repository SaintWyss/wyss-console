# M9 engineering evidence — 2026-10-26

## Result and baseline

M9 is developed on `feat/m9-opencode-hermes-wyss-agent`, based on the open M6
engineering branch at `328c54e35658fbd2b7ef00dad94676505702079d`. M6 engineering
tests passed on that branch; M6 remains unmerged and has not received the
independent security/architecture review or real-provider acceptance required
for production. This M9 record is not an M6 or M9 production approval.

## Implemented

- **OpenCode Native adapter:** the catalogue identifies OpenCode separately,
  qualifies Native model override and brief handoff to exact version `1.19.4`,
  passes `--model provider/model` and `--prompt <reviewed brief>` as distinct
  arguments, and launches with the selected project as working directory. Its
  source descriptor is pinned to `anomalyco/opencode@148842ef7cb1bc34192b3fff57d0f9721ab9c801`.
  Provider resolution and effective model use are not asserted. Version drift
  disables the qualified override/handoff path.
- **OpenCode configuration:** the existing read adapter and JSONC-span writer
  remain the source of inventory and writes. Existing tests cover comments and
  trailing commas, secret exclusion, malformed files, selected-project scope,
  and no-write inventory. This M9 branch does not widen the accepted mutation
  surface or infer support from successful parsing.
- **Native provider selection and crash-safe metadata:** switching to a legacy
  Claude/Codex provider durably removes an older M9 project default before
  launch and restores it if launch or shell-version verification fails. M9
  recovery snapshots contain bounded session/default metadata only; they are
  copied before a safe M1–M6 registry projection is published, migrated
  idempotently after the sidecar becomes healthy, and removed only after the
  primary sidecar has been durably written. Directory scanning is capped at 16
  entries; corrupt or over-cap recovery keeps M9 unavailable and preserves the
  source bytes rather than silently discarding them. Atomic replacement syncs
  the containing directory after rename.
- **Hermes Native:** adds a distinct Native catalogue entry and keeps the real
  interactive command available as a full-account Native process. Automatic
  version probing is disabled because upstream `hermes --version` performs an
  update check. No profile override or automatic handoff is offered; UI copy
  distinguishes Native privileges from a restricted planner.
- **Hermes proposal boundary:** `HermesProposalV1` accepts bounded,
  versioned descriptive DAG data only. `deny_unknown_fields`, size, identity,
  dependency and cycle validation reject extra tool/command fields. It is
  ephemeral and has no execution, journal, F8 or OL3 dispatch method.
  `HermesPlanner` remains blocked in M6 until the effect boundary is independently
  demonstrated.
- No original Wyss sockets, databases or files are opened. The read-only source
  inspected for DN06=D was `SaintWyss/Wyss@0761dcdacdafa216c8afc25036e24685855d2228`.

## Verification

- Focused `native::service_tests`: **25 passed, 0 failed, 1 ignored**; includes
  stale M9 default clearing, corrupt-sidecar legacy availability, recovery
  migration/preservation, and OpenCode direct-handoff version revalidation.
- `orchestration::hermes_proposal::tests`: **4 passed, 0 failed**; valid and
  hostile JSON, unknown effect fields, size boundaries, dangling/duplicate and
  cyclic dependency cases.
- `npm run check`: **PASS**; 67 Vitest files / 1,074 tests, Prettier, ESLint,
  Svelte/TypeScript, owned harness (16), Claude bridge (77), and Vite production
  build. Vite reports the repository's >500 kB chunk advisory.
- `npm run check:rust`: **PASS**; locked cargo check, library tests (1,478
  passed / 29 ignored), main binary tests (547 passed / 16 ignored), credential
  binary tests (5 passed / 1 ignored), `authority_boundary` (1), `cli_help` (1),
  F8 contracts (10), F8 serve process (8), B1 unit tests (136), and Clippy with
  warnings denied.
- Separately ran the bounded B1 process suite without the repository script's
  `b1::` name filter: **22 passed, 0 failed**. The filtered script run selected
  zero `b1_process` tests; this separate run supplies the process-test evidence.
- `npm audit`: **0 known vulnerabilities** after the Vite 8.3.5 and undici
  security updates.
- `npm run scan:repo`: **PASS**, 5 checks / 0 failures; all 1,606 tracked files
  were scanned from Git and the working tree after source changes and before
  this evidence-only update.
- GitHub Actions on the corrected M9 commit remain pending; they will be
  dispatched after the correction/evidence commit is pushed.
- No real OpenCode process or provider account was run. No Hermes process was
  run. These are fixtures and source-bound argument/capability tests only.

## Requirement status

| Requirement                                                  | Status                  | Evidence / limit                                                                                                                                                                                                       |
| ------------------------------------------------------------ | ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TS-ADP-OC-01..09                                             | PARTIAL, fixture-tested | Existing JSONC read/write suite plus exact-version Native launch tests; real installed OpenCode, symlink matrix, shared skill directories and external-writer races need target-provider/runtime evidence.             |
| TS-ADP-HR-01..05                                             | PARTIAL / BLOCKED       | Native launch is separate and explicitly unconfined. Planner parser tests enforce data-only proposals; no isolated Hermes planner runtime exists, so non-bypass, config immutability and real planning remain BLOCKED. |
| TS-AGT-01..02                                                | NOT_PROVEN              | The Console does not call original Wyss, but a full provider discovery/absence and source suite result are not claimed here.                                                                                           |
| TS-AGT-03..05                                                | BLOCKED                 | Requires Hermes with effectful tools removed inside a demonstrated OS boundary and the selected Wyss typed policy/effects contract. In-process hooks and prompt rules do not satisfy this.                             |
| Real provider quality, resource usage, upgrade compatibility | NOT_RUN / NOT_PROVEN    | No credential-backed OpenCode/Hermes run or resource measurement was performed.                                                                                                                                        |

## Blockers

1. Hermes `PlannerProposalV1` is intentionally not wired to a provider. Blocking
   dependency: an independently reviewed OS isolation boundary that prevents
   Hermes tools/plugins/network/subprocesses from bypassing Wyss, plus typed
   Wyss IPC/action contracts and E-HRM-09 non-bypass evidence. No Native tool
   approval is treated as that boundary.
2. The exact installed Hermes build is not observed without executing its
   update-checking version command. Therefore capability-specific profiles,
   automatic brief handoff and planner launch remain unavailable.
3. OpenCode 1.19.4 source parsing does not prove the owner-installed CLI,
   configured provider or effective model. Those remain provider acceptance
   items, not inferred PASS results.

## Preservation and readiness

Managed production concurrency remains one; F8 approvals and authority paths are
unchanged; Hermes planner admission remains blocked; Native sessions are clearly
labelled as full-account provider processes. No provider files, credentials,
services or production data were changed. This PR requires independent Sol High
review and final CI before it is review-ready; it is not production-ready.
