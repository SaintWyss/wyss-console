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
  the containing directory after rename. If migration cannot complete, all M9
  sessions/defaults still held in memory are copied into bounded recovery before
  the shared registry is projected; inability to preserve them refuses startup.
  Legacy-provider override records are domain-separated project digests, bounded
  for the complete recovery envelope, and are written only when a recovered M9
  default actually exists. The legacy v1 path-bearing tombstone file is validated
  and merged into the v2 digest set before v1 is removed; malformed, oversized,
  unreadable, or unpersistable v1 state blocks M9 admission and preserves the
  original bytes. Dangling symlinks are treated as present and fail closed.
- **Concurrent settings reads:** a reader that loses an atomic-replacement race
  yields and retries up to 16 times before returning the existing bounded
  `Unsafe` result. This keeps replacement handling bounded while giving a
  legitimate writer time to finish.
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

- Focused `native::service_tests`: **28 passed, 0 failed, 1 ignored**; includes
  the P1 recovery-capacity regression across two restarts, corrupt-sidecar
  legacy availability without irrelevant tombstone growth, v1-to-v2 tombstone
  migration before sidecar recovery, malformed-v1 fail-closed preservation,
  hashed override metadata, stale M9 default clearing, and OpenCode direct-handoff
  version revalidation. The multi-entry tombstone assertion checks membership,
  not serialized index order; the digests are held in a sorted set and one entry
  is derived from a random temporary home path.
- `orchestration::hermes_proposal::tests`: **4 passed, 0 failed**; valid and
  hostile JSON, unknown effect fields, size boundaries, dangling/duplicate and
  cyclic dependency cases.
- `settings::tests::a_reader_of_the_file_only_ever_sees_a_whole_set`: **PASS**
  after a full-suite scheduling failure exposed that five immediate retries
  could exhaust under concurrent atomic replacements. The bounded retry/yield
  correction passed both a focused rerun and the subsequent full Rust suite.
- `npm run check`: **PASS**; 67 Vitest files / 1,074 tests, Prettier, ESLint,
  Svelte/TypeScript, owned harness (16), Claude bridge (77), and Vite production
  build. Vite reports the repository's >500 kB chunk advisory.
- `npm run check:rust`: **PASS** on corrected source head
  `7f48ceb2680d7be2e8f1ab7f3d53129758be9291`, including formatting, locked
  cargo check, 1,478 library tests
  passed / 29 ignored, 550 main-binary tests passed / 16 ignored, credential
  binary 5 passed / 1 ignored, authority boundary (1), CLI help (1), F8 contracts
  (10), F8 serve process (8), B1 unit tests (136), and Clippy with warnings
  denied.
- Separately ran the bounded B1 process suite: **22 passed, 0 failed**. The
  repository script's `b1::` filter selects zero `b1_process` tests, so this
  unfiltered invocation supplies the process-test evidence.
- `npm audit`: **0 known vulnerabilities** after the Vite 8.3.5 and undici
  security updates.
- `npm run scan:repo`: **PASS**, 5 checks / 0 failures across 1,606 tracked
  files on corrected source head `7f48ceb`.
- GitHub Actions run `40289260676` covers the earlier source head `2d3603f` and
  remains in progress: its frontend/contracts and scanner jobs passed; the Rust
  job remains at runner dependency installation. It is not final-head CI. The
  order-independent test-only correction does not change adapter behavior.
- Independent Sol High static review at `2d3603f` found no actionable issue in
  the implementation. The final `7f48ceb` test-only delta awaits re-review;
  neither review status is a formal GitHub approval or real-provider
  acceptance.
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
services or production data were changed. Final GitHub Rust CI remains pending;
real OpenCode/Hermes acceptance is unrun. It is not production-ready.
