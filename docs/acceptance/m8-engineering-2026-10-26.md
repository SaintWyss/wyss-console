# M8 engineering evidence — 2026-10-26

## Baseline and scope of this increment

This branch is `feat/m8-whole-product-closure`, based on M7 head
`03c29e7fb80110741d97fe4ba464739a6276193c`. M6 remains an open engineering
branch and is not promoted to production acceptance by this record. This is a
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

## Verification

- `npm run check`: **PASS**. Prettier, ESLint, Svelte/TypeScript; 69 Vitest files
  and 1,080 tests; owned-workspace harness (16); Claude bridge (77); Vite build
  (230 modules). The bridge tests emitted existing Python `ResourceWarning`s
  for unclosed test-only files. Vite emitted the repository's advisory for a
  minified chunk above 500 kB; build succeeded.
- Focused appearance/renderer regressions: **10 passed, 0 failed**.
- `npm run check:rust`: **PASS** on this code and the M7 source baseline; locked
  Rust checks, default/B1 regressions, and Clippy with warnings denied. The M7
  evidence fast-forward after this run changed only its acceptance document.
- `npm run scan:repo`: **PASS**, 5 checks / 0 failures; scans tracked files in
  Git and the working tree. A final scan after this evidence commit is required
  so newly added Android/appearance tests are included.
- `npm audit --audit-level=moderate`: **0 known vulnerabilities**.
- Android Gradle/JUnit was not available as a local verified run. Run the
  repository Android GitHub workflow on the final M8 head before treating the
  Java palette tests/build as TESTED. Emulator output is not real-device
  evidence.
- No benchmark was run and no M01..M33 p50/p95 values are claimed here.

## Requirement status

| Requirement                                                                    | Status                                    | Evidence / limitation                                                                                                                                                                                      |
| ------------------------------------------------------------------------------ | ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Three desktop themes and preference migration                                  | IMPLEMENTED, TESTED                       | Closed schema/enum, exact v1-to-v2 migration, unknown-shape refusal and full frontend suite.                                                                                                               |
| Theme semantic contrast and terminal ANSI palette                              | IMPLEMENTED, TESTED                       | Deterministic CSS and ANSI ratio tests pass; this does not establish assistive-technology usability.                                                                                                       |
| Native Android theme preference                                                | IMPLEMENTED, NOT YET TESTED ON FINAL HEAD | Native UI/storage and Java unit tests added; final Android workflow and actual-device behavior pending.                                                                                                    |
| FP-NFR-01 / M01..M33 on two reference PCs and Android                          | NOT_PROVEN                                | No raw, provenance-bound reference-device p50/p95 measurements or approved frozen budgets were collected. Requires the two specified reference PCs, supported Android hardware and controlled repetitions. |
| FP-NFR-02 keyboard, IME, focus, reduced motion, scale and assistive technology | PARTIAL / NOT_PROVEN                      | Palette/contrast and existing keyboard regressions pass; real WebKitGTK, TalkBack, IME, text scaling, reduced-motion and narrow-device journeys were not exercised.                                        |
| FP-NFR-03 update, backup, WAL recovery, rollback and power-loss matrix         | NOT_PROVEN                                | No installed migration, backup/restore, mixed-version or rollback rehearsal was performed. Existing fixture regressions do not substitute for those operations.                                            |
| JF-01..JF-12 full cross-provider and cross-device journeys                     | NOT_RUN / PARTIAL                         | Existing feature tests and this increment's regressions are not a complete end-to-end run across real reference PCs, providers and Android.                                                                |
| Privacy/no-leak and preservation gates                                         | PARTIAL / TESTED                          | Repository scanner and existing full tests pass; no new credential, provider output or journal field was added by this theme increment. Full final product privacy corpus remains outstanding.             |

## Explicit blockers and limits

- The M8 DoD requires every in-scope traceability row at its required evidence
  level and reviewed budgets. Hardware-specific measurements, real accessibility
  journeys and long-duration/recovery rehearsals remain NOT_PROVEN; no target
  was relaxed or inferred from fixtures.
- Android CI must run against the final pushed head. Actual Android screen reader,
  IME, lifecycle, scaling and hardware behavior still require supported physical
  devices.
- M7 transport OD01 and the M7 same-UID authenticity/OD02 gates remain blocked
  as recorded in the M7 acceptance evidence. This appearance change does not
  make remote access available or widen a grant.
- M9 remains a separate reviewed extension lane. M8 must integrate it only after
  its independent review. Managed production concurrency remains one; Native
  automated dispatch and Hermes planner effects remain behind their accepted
  trust-boundary gates.

## Preservation and readiness

No owner services, installed application, credentials, working sessions or
production data were installed, restarted, migrated or changed for this work.
The M8 change is reviewable and fixture-tested, but M8 is not complete, the PR
must remain unmerged pending independent review and integration CI, and neither
merge readiness nor production readiness is claimed.
