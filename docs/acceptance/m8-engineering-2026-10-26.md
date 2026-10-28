# M8 engineering evidence — 2026-10-26

## Baseline and scope of this increment

This branch is `feat/m8-whole-product-reviewed`, based on the separate
`integration/m7-m9-final-reviewed` branch at
`64f5bc6280a10f7433a77ada86ff17daab5b33e9` (PR #33, open and unmerged). That
integration includes the final M7 source/evidence head `f1a59513ec064ed288411cad2132175757449f48`
and final M9 source/evidence head `fa7b1d424bbc3fbdec4b9507087da83988a7f514`.
The M7, M9 and integrated deltas received separate static reviews with no
actionable code findings. Those reviews are not GitHub approvals or production
acceptance. M6 PR #29 remains open and unmerged. This is a substantial M8
accessibility and appearance increment; it is not whole-product closure under
the M8 roadmap DoD.

## Implemented

- Desktop preferences now use closed schema version 2 with `dark`, `light` and
  `high_contrast` appearance choices. Exact schema version 1 documents migrate
  to dark in memory without rewriting on read; an explicit later preference
  update persists version 2. Unknown schema/value shapes refuse.
- Settings exposes the three choices. The selected palette is applied across
  the desktop, pipeline/remote status surfaces and xterm terminal renderer.
  The renderer uses theme-specific ANSI foregrounds and updates existing
  terminals when appearance changes. This setting is presentation-only and
  does not affect provider, authority, approval or terminal input policy.
- Native Android Settings has the same three closed choices and app-private
  persistence. Navigation, terminal and status surfaces apply the selected
  palette. The Android implementation adds no transport, permission, account
  grant or remote-input capability.
- Deterministic tests cover exact settings migration/refusal, closed theme
  values, all CSS semantic foregrounds and focus/boundary tokens against WCAG
  contrast thresholds, terminal ANSI colors against their terminal surfaces,
  renderer appearance-listener disposal, and Android palette spellings and
  contrast values.
- The desktop shell now begins keyboard navigation with a visible-on-focus
  skip link to a programmatically focusable main landmark. Its colors use the
  three theme tokens over an opaque background, with tested text/border contrast
  and a focus outline; shell tests assert the link target, label, contrast and
  landmark semantics. WebKitGTK keyboard and
  assistive-technology behavior remains NOT_PROVEN.
- The light-theme `--faint` token is darkened so it remains at least 4.5:1 on
  every opaque light-theme surface. A regression binds the event timestamp,
  default/hover/selected row states, and those concrete surfaces to the contrast
  assertions.
- Android button minimum height now uses `dp(48)` through the shared density
  conversion. A JVM regression verifies the resulting pixel minimum at
  densities 1.0, 1.5, 2.0, 3.0 and 4.0.

## Verification

- `npm run check`: **PASS** after the skip-link and contrast correction.
  Prettier, ESLint, Svelte/TypeScript; 69 Vitest files and 1,083 tests
  (including the shell landmark/skip-link and theme contrast regressions);
  owned-workspace harness (16); Claude bridge
  (77); Vite build
  (230 modules). The bridge tests emitted existing Python `ResourceWarning`s
  for unclosed test-only files. Vite emitted the repository's advisory for a
  minified chunk above 500 kB; build succeeded.
- The repo-scan Vitest file uses a bounded 30-second file timeout because the
  full Git checkout scan and fixture Git subprocesses exceeded Vitest's default
  five seconds while Rust builds were loaded. Its assertions are unchanged;
  the focused file passed 21 tests.
- Focused light-theme appearance regressions: **3 passed, 0 failed**.
- `npm run check:rust`: **PASS** on the reviewed M7/M9 integration; formatting,
  locked cargo check, 1,480 library tests passed / 29 ignored, 559 app-binary
  tests passed / 16 ignored, credential binary 5 passed / 1 ignored, authority
  boundary (1), CLI help (1), F8 contracts (10), F8 serve process (8), B1
  unit tests (136), and Clippy with warnings denied.
- The first integrated Rust run exposed a test assertion that assumed the
  expected project digest was at index zero in a sorted set. Both digests were
  present; the order varied with a random temporary path. M9 changed the test
  to assert membership, and the focused migration test plus the complete
  integrated Rust suite then passed. No product behavior was changed for this
  test correction.
- Independent Sol High review of the M8 head `37d4802` found two P2s: light
  theme faint text on raised event rows missed 4.5:1, and Android button
  minimum height used raw pixels. Both are fixed. Re-review of implementation
  head `15590c5` found no actionable residual finding; it was static review,
  not formal GitHub approval, physical measurement or production acceptance.
- A subsequent Sol High review of the first desktop skip-link commit found its
  translucent background could fall below 4.5:1 over a light glass desktop.
  The link now uses opaque `--bg`; a new test checks text and boundary contrast
  for all three themes. Re-review at `d295743` found no residual actionable
  finding and closed the evidence-provenance issue. This remains static review,
  not formal GitHub approval. The skip-link change is desktop-only; Android
  source remains unchanged from the separately CI-tested theme build.
- Direct B1 process suite: **22 passed, 0 failed**. The standard script's
  `b1::` filter selects no `b1_process` cases, so this direct invocation covers
  the process regressions.
- `npm run scan:repo`: **PASS**, 5 checks / 0 failures; all 1,628 tracked files,
  including the integrated Android/appearance tests and updated evidence, were
  scanned from Git and the working tree.
- `npm audit --audit-level=moderate`: **0 known vulnerabilities**.
- Android Gradle/JUnit is not available as a local verified run. Android run
  `40303043578` passed on M8 head `0f88cf6`, including the native build and JVM
  tests. That M8 delta added desktop skip navigation; the following correction
  changes desktop CSS/tests only. Final-head CI for the correction is tracked
  by M1–M5 integration run `40304125925` and Android run `40304134561` on
  `d295743`. The complete M1–M5 workflow `40304125925` and Android
  build/JUnit workflow `40304134561` both passed on that exact code head. The
  later `d9cbe92` commit updates this evidence only; no product source changed.
  CI does not prove physical device rendering or touch target measurement.
- No benchmark was run and no M01..M33 p50/p95 values are claimed here.

### Final reviewed-integration head

- The M8 feature branch was updated from PR #33's reviewed integration head
  `64f5bc6280a10f7433a77ada86ff17daab5b33e9`; the resulting M8 head is
  `f97c67d2ec533d1c4330fa6fdebfc189f5921571`. Its product delta relative to
  that integration base is M8-only.
- On `f97c67d`, `npm run check` passed: formatting, lint, Svelte/TypeScript,
  69 Vitest files / 1,083 tests, owned-workspace harness (16), Claude bridge
  (77), and Vite build (230 modules). Existing Python `ResourceWarning`s and
  the Vite chunk-size advisory were emitted; the build succeeded.
- `npm run scan:repo` passed 5/5 checks over the tracked repository, and
  `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` passed on
  `f97c67d`.
- On current PR head `6ecac0f7b0e5b0f9a95b6f3968e31bf3a7984c6c` (docs-only
  commit after code head `f97c67d2ec533d1c4330fa6fdebfc189f5921571`), M1–M5
  integration workflow [40325714895](https://github.com/SaintWyss/wyss-console/actions/runs/40325714895)
  passed both `frontend-and-contracts` and `rust-core-and-v9`, including
  repository scan, formatting, Clippy, focused V9 and credential handoff,
  B1 process, default-feature and all-feature Rust regressions. Android
  build/JUnit workflow [40325714823](https://github.com/SaintWyss/wyss-console/actions/runs/40325714823)
  also passed on that exact PR head. These are deterministic CI results; they
  do not establish physical-device or provider acceptance.
- A final static Sol High review of `64f5bc6..f97c67d` found no actionable
  code, security or architecture findings. It identified the stale baseline
  text corrected above; its follow-up on `6ecac0f` confirmed the correction and
  found no other evidence mismatch. Static review is not GitHub approval,
  hardware/provider acceptance or production acceptance.

## Owner acceptance decision (2026-10-27)

The owner split M8 into two levels, as recorded in the roadmap. Level A (engineering
integration) covers this increment. Level B (whole-product acceptance) keeps every
original requirement; nothing here lowers a threshold, a budget or a security gate.

Level A conditions and status at this record:

| Condition                                                  | Status                                                                           |
| ---------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Exact candidate `77f0c5d` and documentation HEAD `c3bc2d5` | Documentation-only delta; production code is unchanged                           |
| Independent review of the candidate (local Sol High)       | REJECT on whole-product gates only; no security regression found                 |
| Required CI on exact HEAD `c3bc2d5`                        | 3/3 completed successfully; see the check links below                            |
| Approval by a second GitHub collaborator                   | Replaced only for PR #32 by the owner-authorized solo-maintainer exception below |
| Privacy evidence for the candidate                         | PASS for the existing suites listed below; full product corpus still outstanding |
| Explicit owner acceptance recorded in PR #32               | PENDING; preparing this exception does not grant merge authorization             |

### Solo-maintainer governance exception — PR #32 only

The owner authorized preparation of this limited exception for **M8 Engineering
Integration, PR #32**, because the repository has one maintainer. For this increment
only, approval by a second GitHub collaborator may be replaced by all five conditions:

1. A documented independent technical review.
2. Resolution of every blocking security finding.
3. All required CI checks succeeding on the exact final HEAD.
4. Synthetic privacy evidence with its scope declared.
5. The owner's explicit acceptance recorded in PR #32.

The local Sol High review is accepted as technical evidence, not as a human GitHub
approval. Its original REJECT under the whole-product gate is not rewritten as an
`APPROVED` review. The full report is retained outside the repository with the
M8 candidate review evidence. No blocking security regression was identified in that review; the inherited
nonblocking journal duplication and all existing production limitations remain
documented.

The exception does not approve Whole-Product Acceptance, enable Managed, or remove
any AUD-01, M7 or real-provider gate. It applies to no other PR. Acceptance and merge
authorization remain pending; no review submission, commit publication or merge is
performed by recording this preparation.

GitHub CI was verified on exact HEAD
`c3bc2d56786ff246009030c5bd3a2564235f4b0f` (tree
`0bc823715ce0bc3e6cabd3ae2c36fca3c7f2e5bc`), on 2026-10-28 UTC:

| Check                                                                                                         | Final status        |
| ------------------------------------------------------------------------------------------------------------- | ------------------- |
| [frontend-and-contracts](https://github.com/SaintWyss/wyss-console/actions/runs/40617793594/job/122584251995) | completed / success |
| [rust-core-and-v9](https://github.com/SaintWyss/wyss-console/actions/runs/40617793594/job/122584252295)       | completed / success |
| [android-unit-and-build](https://github.com/SaintWyss/wyss-console/actions/runs/40617793533/job/122584251555) | completed / success |

These results belong to `c3bc2d5`. Publishing this governance update creates a new
HEAD, which must receive all three successful checks before owner acceptance of
that HEAD and integration. Success on `c3bc2d5` is not CI evidence for an unpublished
documentation change or a later commit.

Privacy evidence on `77f0c5d` (tree `d3e6f23e`, clean worktree). All suites exit 0:

- `redact` (binary): 18 passed, 0 failed.
- `leak` (library and binary): 1 and 1 passed.
- `secret` (library): 3 passed; (binary): 13 passed, 2 ignored.
- `credential` (library): 14 passed, 2 ignored; (binary): 11 passed, 1 ignored.
- `repo-scan`: 5 passed, 0 failed, over 1720 tracked files.

Credential-shaped strings in the source are synthetic fixtures (for example
`sk-ant-api03-SECRETVALUE` in `src-tauri/src/f8/claude/credential.rs`). These suites
are existing unit tests. They are not the final product corpus across journal,
settings, logs and exports with a live backend, which remains outstanding.

Chromium browser evidence (11 of 11 checks passed) is partial increment evidence on the
shell: skip link, focus, tab sequence, contrast in three themes for the console view,
reflow at 320 px, root font scale 200%, and reduced-motion media. It is not a substitute
for WebKitGTK, terminal-with-backend, IME or TalkBack acceptance.

Level B remains blocked by: FP-NFR-01 (reference hardware and approved budgets),
FP-NFR-02 (WebKitGTK, IME, TalkBack), FP-NFR-03 (installed update, backup, WAL, rollback),
JF-01..JF-12 (journeys, providers, M7 OD01/OD02), and the open AUD-01 items (V2 and
reviewed plan, `model_id`, `approval_reference`, real provider acceptance).

Product status: NOT_ACCEPTED / RELEASE_BLOCKED. M8 whole-product is not CLOSED.
Managed, real providers and unauthorized remote functions remain disabled.

## Requirement status

| Requirement                                                                    | Status                                                   | Evidence / limitation                                                                                                                                                                                      |
| ------------------------------------------------------------------------------ | -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Three desktop themes and preference migration                                  | IMPLEMENTED, TESTED                                      | Closed schema/enum, exact v1-to-v2 migration, unknown-shape refusal and full frontend suite.                                                                                                               |
| Theme semantic contrast and terminal ANSI palette                              | IMPLEMENTED, TESTED                                      | Deterministic CSS and ANSI ratio tests pass; this does not establish assistive-technology usability.                                                                                                       |
| Native Android theme preference                                                | IMPLEMENTED, DETERMINISTIC CI TESTED / DEVICE NOT_PROVEN | Native UI/storage and Java unit tests added; final M8 Android build/JUnit workflow `40325714823` passed on PR head `6ecac0f`; physical rendering and touch target behavior remain untested.                |
| FP-NFR-01 / M01..M33 on two reference PCs and Android                          | NOT_PROVEN                                               | No raw, provenance-bound reference-device p50/p95 measurements or approved frozen budgets were collected. Requires the two specified reference PCs, supported Android hardware and controlled repetitions. |
| FP-NFR-02 keyboard, IME, focus, reduced motion, scale and assistive technology | PARTIAL / NOT_PROVEN                                     | Palette/contrast and existing keyboard regressions pass; real WebKitGTK, TalkBack, IME, text scaling, reduced-motion and narrow-device journeys were not exercised.                                        |
| FP-NFR-03 update, backup, WAL recovery, rollback and power-loss matrix         | NOT_PROVEN                                               | No installed migration, backup/restore, mixed-version or rollback rehearsal was performed. Existing fixture regressions do not substitute for those operations.                                            |
| JF-01..JF-12 full cross-provider and cross-device journeys                     | NOT_RUN / PARTIAL                                        | Existing feature tests and this increment's regressions are not a complete end-to-end run across real reference PCs, providers and Android.                                                                |
| Privacy/no-leak and preservation gates                                         | PARTIAL / TESTED                                         | Repository scanner and existing full tests pass; no new credential, provider output or journal field was added by this theme increment. Full final product privacy corpus remains outstanding.             |

## Explicit blockers and limits

- The M8 DoD requires every in-scope traceability row at its required evidence
  level and reviewed budgets. Hardware-specific measurements, real accessibility
  journeys and long-duration/recovery rehearsals remain NOT_PROVEN; no target
  was relaxed or inferred from fixtures.
- Actual Android screen reader, IME, lifecycle, scaling and hardware behavior
  still require supported physical devices.
- M7 transport OD01 and the M7 same-UID authenticity/OD02 gates remain blocked
  as recorded in the M7 acceptance evidence. This appearance change does not
  make remote access available or widen a grant.
- M9 remains a separately reviewed extension lane and is included through the
  reviewed integration base. Managed production concurrency remains one; Native
  automated dispatch and Hermes planner effects remain behind their accepted
  trust-boundary gates.

## Preservation and readiness

No owner services, installed application, credentials, working sessions or
production data were installed, restarted, migrated or changed for this work.
The M8 change is reviewable and fixture-tested, but M8 is not complete. Engineering
Integration (Level A) remains pending explicit owner acceptance recorded in PR #32
under the limited solo-maintainer exception and successful required CI on the final
published HEAD. Whole-Product Acceptance (Level B) remains NOT_ACCEPTED /
RELEASE_BLOCKED with every original evidence and security gate intact. No production
acceptance or Managed activation is granted.
