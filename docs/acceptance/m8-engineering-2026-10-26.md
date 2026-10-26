# M8 engineering evidence — 2026-10-26

## Baseline and scope of this increment

This branch is `feat/m8-whole-product-reviewed`, based on the separate
`integration/m7-m9-reviewed` branch at
`15f0d1a1e19b9d634699861ef2f197ffba1e1e05` (which contains the final
evidence-only updates from M7 PR #30 head `8abc900` and M9 PR #31 head
`6686687`).
Their implementation sources were independently reviewed at M7 source head
`0a6d5f7` and M9 head `3bbd055`; the evidence updates do not change source code.
Those static reviews found no actionable findings; neither is a GitHub approval
or production acceptance. M6 PR #29 remains open and unmerged. This is a
substantial M8 accessibility and appearance increment; it is not whole-product
closure under the M8 roadmap DoD.

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
  `d295743`. On that exact head, frontend/contracts and scanner passed; the
  Rust integration job is still running. Android native build/JUnit passed.
  CI does not prove physical device rendering or touch target measurement.
- No benchmark was run and no M01..M33 p50/p95 values are claimed here.

## Requirement status

| Requirement                                                                    | Status                                                   | Evidence / limitation                                                                                                                                                                                      |
| ------------------------------------------------------------------------------ | -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Three desktop themes and preference migration                                  | IMPLEMENTED, TESTED                                      | Closed schema/enum, exact v1-to-v2 migration, unknown-shape refusal and full frontend suite.                                                                                                               |
| Theme semantic contrast and terminal ANSI palette                              | IMPLEMENTED, TESTED                                      | Deterministic CSS and ANSI ratio tests pass; this does not establish assistive-technology usability.                                                                                                       |
| Native Android theme preference                                                | IMPLEMENTED, DETERMINISTIC CI TESTED / DEVICE NOT_PROVEN | Native UI/storage and Java unit tests added; Android CI passed on code head `15590c5`; physical rendering and touch target behavior remain untested.                                                       |
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
The M8 change is reviewable and fixture-tested, but M8 is not complete. The PR
must remain unmerged pending formal independent GitHub review and the M8
whole-product evidence gates; neither merge readiness nor production readiness
is claimed.
