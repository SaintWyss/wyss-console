# M1–M5 installed-product E2E — 2026-10-24

**Verdict: PARTIAL.** The packaged build installs, launches and runs the local-terminal path in an isolated desktop. The authority-backed flows (M1 projects, M2 native agents, M3 managed, M5 sessions, V9 recovery) were **not exercised**: `--gui-only` starts no authority and the production package carries no fixture backend.

## 1. Tested revision
`0e9a5341a9df4e6169436a1d52320ce8548c031c` (`origin/main`, PR #27). GitHub CI for that SHA: success (run 40152262031). Built from a clean detached worktree; no other worktree touched.

## 2. Package and hashes (`npm run desktop:package`, exit 0)
| File | SHA-256 |
| --- | --- |
| bin/wyss-console | 312921111f02e6344d985f77d393fd344a464ed39ad304929a2412898a20df83 |
| libexec/wyss-console/wyss-control | 14ae3a596eed9b85effb0b60d7793ec8dbe0d98292ca1e5890a9069be90d5a04 |
| libexec/wyss-console/wyss-screen | 362d8cd91c68eaeef9ff15fa20cce3358408513adffecef48bd275be0605f7bd |
| libexec/wyss-console/wyss-credential-agent | f30f7514239291a29bb1041f87ea95e101783e6b0c3337dfabec344386843488 |

Authority digest embedded in the GUI; manifest `source_revision` = tested SHA. No experimental feature flags enabled.

## 3. Environment
CachyOS, Node 26.11.1, rustc 1.98.0. Install with `install.sh` under private `HOME`/`XDG_RUNTIME_DIR` (`env -i`), GUI in a nested KWin/Xwayland display `:77` (1400×900), private D-Bus. Not a VM. Production install, units, authority (pid 255925/701187) and launcher symlink (dated 2026-10-23) were unchanged before and after.

## 4. Installation
`install.sh install` PASS (manifest digests verified). Reinstall created `.previous`; `rollback` swapped back; `status` reported 0.2.0 at the tested revision; `uninstall --purge` removed app and Console state and left agent configs alone. Caveat: previous and candidate were the same build, so rollback proves the mechanism, not a version change.

## 5. E2E matrix
| Area | Result | Evidence |
| --- | --- | --- |
| A. Startup, version 0.2.0 + build SHA, close/reopen | PASS | 01, 03-Settings, 04 |
| A. GUI-only: Authority "Offline", no bootstrap | PASS | 01, 03-Settings |
| B. M1 projects/workspaces | BLOCKED | needs authority; Sessions/Agents show empty state |
| C. M2 native Codex/Claude sessions | BLOCKED | no authority, no fixture backend in package; Agents view shows "No session is open" |
| D. M3 managed / real Claude disabled | NOT_PROVEN | gate not visible without authority; 1056 vitest + 1435/535 Rust tests pass |
| E. M4 terminal: open, run `echo`/`ps`, shell ownership | PASS | 02-terminal |
| E. Resize / full-screen app rendering | NOT_PROVEN | not driven |
| E. Terminal recovery across GUI restart | NOT_PROVEN | in `--gui-only` closing the window stops its private host (documented); reopen gave a fresh terminal |
| F. M5 lifecycle / capacity-one / refusals | NOT_PROVEN | unit/integration tests only |
| F. Orphan processes after close and teardown | PASS | none left from the run |
| G. Persistence, V9 recovery, audit | NOT_PROVEN | Rust tests only (`rust:test` exit 0) |
| H. Binaries, install, reinstall, rollback, uninstall | PASS | rollback.log |
| Visual: navigation across views | PASS (Agents, Sessions, Providers, Credentials, Settings) | 03-* |
| Visual: dialogs, loading/error states | NOT_PROVEN | |

## 6. Evidence
`docs/acceptance/evidence-m1-m5-e2e-2026-10-24/` (screenshots, build/install/test logs, hashes).
Automated: `npm test` 1056/1056; `npm run rust:test` exit 0 (1435 + 535 + integration tests, 0 failed).

## 7. Defects
- **Low / environment-or-layout (unconfirmed):** window opened at 1440×810 on a 1400×900 screen, so the right 40 px (header version, toolbar buttons) are clipped. Probable cause: minimum window width above the test screen; not checked on a larger display.
- **Info:** `rust-test.log` contains `gitstatus failed to initialize` from the user's fish prompt inside a test; harmless.
No FAIL-class defect found in what was exercised. No production code modified.

## 8. Untested / blocked
Everything needing a running authority (M1 persistence, M2, M3, M5, V9). Unblocking requires either a protocol-2 authority in the disposable HOME (needs a Codex executable `codex-cli 0.159.0` and runtime scenario setup per `f8-activation-procedure`) or a fixture-feature build (`m3-fixture`/`b1-fixture`), which is a different binary from the shipped package and needs owner sign-off.

## 9. Rollback outcome
Disposable install removed and nested display torn down by exact SID; no mounts or processes remain. Production untouched.

## 10. Verdict
**E2E PARTIAL.**

## 11. Next step
Run a second pass with an isolated protocol-2 authority (fixture Codex, disposable stores) under the same nested display to cover M1/M2/M3/M5/V9 graphically; decide beforehand whether a fixture-feature build is acceptable. Check the 1440-px minimum width on a ≥1440 display.
