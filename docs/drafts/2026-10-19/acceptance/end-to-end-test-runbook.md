# End-to-end acceptance procedures and evidence requirements

**Status:** Complete proposed acceptance procedures for remaining features; not reports of test execution. The canonical identifiers remain architecture [20](../../../architecture/20-VERIFICATION-AND-ACCEPTANCE.md), [19](../../../architecture/19-IMPLEMENTATION-ROADMAP.md) and [02](../../../architecture/02-REQUIREMENTS.md). Test names below are supplementary `E2E-*`; don't replace canonical `TS-*` or `AC-*`.

## Universal evidence record

Each case must store:
- Test ID, requirement(s), applicable ADR(s), software commit, packaged artifact hash.
- Machine/OS, GUI renderer, provider binary/version, fixture/path class (never private content).
- Starting state, exact reproducible action, actual result, result capture (redacted), process identity or source hash where necessary.
- Whether the verification is automated, real provider, nested-display GUI, normal-display GUI or external device.
- Known limitation, P0/P1/P2 defect ID, cleanup/rollback and owner acceptance if required.

A **passing unit test does not substitute for GUI acceptance**. Missing physical phone or second PC means `unverified`, not pass. Repeat intermittent failures and preserve the occurrence/reproduction.

## Workflows

| Test | Setup/action | Pass and failure criteria |
| --- | --- | --- |
| E2E-LOCAL-01 | Fresh normal-user GUI, no provider installed | Terminal opens, provider discovery accurately empty, no files mutated |
| E2E-LOCAL-02 | Installed Claude + Codex: launch both in distinct projects | Real CLI PTY input and output; provider/version/Native shown; stop affects selected only |
| E2E-LOCAL-03 | Three agents + shell, split/resize/search | No misdirected input; scrollback search; focus and shortcuts work |
| E2E-HOST-01 | Start provider, record host+session identity, force-close GUI, reopen | Same real provider process alive, bounded buffered output restored |
| E2E-HOST-02 | Kill disposable host, reconnect stale socket | Lost/Disconnected accurately shown; no pretend process alive or unsafe duplicate |
| E2E-HOST-03 | Two clients connect to test host and attempt resize/input | Explicit arbitration, no doubled input/data race, closed private peer check |
| E2E-CFG-01 | Toggle/undo real-shaped Claude skill and Codex MCP fixture | Provider reports relevant state and original exact bytes return |
| E2E-CFG-02 | Provider edits file after Console switch; user attempts undo | Conflict; newer provider bytes untouched |
| E2E-CFG-03 | Symlink/ACL/named co-writer/group/world-writable trees | Only explicitly trusted local writers; no unsafe write through symlink |
| E2E-CFG-04 | Provider version with unsupported feature/unknown format | Read-only or refuse with reason; no default guessed |
| E2E-PRF-01 | Profile saved/versioned/exported; overlay launches Claude | Global files identical, profile ID/version accurate |
| E2E-PRF-02 | Try incompatible Codex per-session hook/skill profile | Launch refused before process; reason shown |
| E2E-MOD-01 | Provider reports actual model; configured default differs | M3/M4/M5 visibly distinct; unknown M5 never guessed |
| E2E-MOD-02 | Catalogue lists inaccessible model, attempt start | Catalogue not shown as entitlement; typed provider refusal |
| E2E-AGT-01 | Detect real OpenCode/Hermes versions and supported config | No generic "all features" claim; verified switch only |
| E2E-AUTHOR-01 | Add executable hook via native editor in fixture | Exact validated command, confirmation, atomic patch, provider verification, undo |
| E2E-AUTHOR-02 | Tampered file, changed source after plan, bad import | No overwrite, injection or authorization grant |
| E2E-HANDOFF-01 | Review sanitized package from Claude to Codex | Target receives only chosen context; source permissions not transferred |
| E2E-HANDOFF-02 | Quoted prompt injection, credential in source, cancel | Untrusted instructions inert, redaction and no destination launch |
| E2E-VAULT-01 | Wallet enabled: create/rotate/use/remove test credential | Encrypted at rest; no raw value in UI/log/process args |
| E2E-VAULT-02 | Wallet locked, key lost, ciphertext corrupted, concurrent rotation | Fail closed; no plaintext fallback or false restoration |
| E2E-INF-01 | Canonical E1–E8 fixtures, existing authority sabotage suite | Selected isolation meets all SG controls; G-INF untouched if not |
| E2E-INF-02 | Codex global hook native and managed under authorized isolation | Real native hook runs; managed no external hooks; no bypass |
| E2E-MAN-01 | Two managed runtimes, one crashes or is cancelled | Session/generation isolation and audit remain correct |
| E2E-REMOTE-01 | Gateway off, no listener; then pair test desktop with local approval | Only approved interface and limited scoped access |
| E2E-REMOTE-02 | Revoke live paired device during output/input | Channel drops immediately, stale token denied |
| E2E-REMOTE-03 | Reconnect, replay, unsupported version, wrong host | No input duplication, no grant widening, safe error |
| E2E-PHONE-01 | Physical supported phone pair → lock → reconnect/revoke | Secrets hidden on background, step-up enforced |
| E2E-DIST-01 | Fresh supported user install; no provider; upgrade/uninstall | Safe terminal, migration, rollback and retention choices |
| E2E-PERF-01 | Packaged build, Ghostty comparison and M01–M19 | Conditions captured; median/p95 or explicit nonmeasurable |

## Security fault injection coverage

Test corrupt and oversized frames, request replay, socket spoofing, host generation mismatch, symlink swap between plan and commit, KWallet outage, SIGKILL during every journal phase, external concurrent writer, stale model catalogue, provider exit mid-stream, slow terminal client, stale device grant, wrong destination handoff.

Invariant: a failed validation **does not silently fall back** to weaker permissions, model assumptions, native processing or unverified storage.

## Canonical coverage mapping

| Canonical IDs | Supplementary tests |
| --- | --- |
| TS-TERM-05–09, TS-HOST-01–06 | E2E-LOCAL-03; E2E-HOST-01–03 |
| TS-CFG-01–16, TS-FS, TS-REC, TS-CONC | E2E-CFG-01–04 |
| TS-NAT, TS-SES, TS-PRF | E2E-LOCAL-02; E2E-PRF-01–02 |
| TS-MOD-01–06 | E2E-MOD-01–02 |
| TS-SES-03–04 | E2E-HANDOFF-01–02 |
| AC-GINF-01–08, TS-REG-02 | E2E-INF-01–02 |
| TS-REM-01–13 | E2E-REMOTE-01–03; E2E-PHONE-01 |
| TS-DIST-01–06 | E2E-DIST-01 |
| TS-PERF-01–13, TS-LONG | E2E-PERF-01 and P1 raw metrics |

F14 adds its own test IDs upon inclusion in canonical roadmap; use E2E-VAULT as the seed.

## Exit / severity gates

**P0 blocks release:** unauthorized remote control, cross-session authority bypass, secret exposure, command injection, data corruption, false confirmation, untrusted executable. **P1 blocks affected feature acceptance:** reproducible wrong-host attach, resource leak beyond accepted bound, broken recovery, unsupported version mutation. **P2:** minor visual/formatting problems; triage with evidence.

CI static/unit results and provider fixture integration must pass at a documented commit. Native GUI and physical device acceptance cannot be silently replaced by CI.

Owner signs the accepted deployment scope, which may be narrower than "entire product." No unverified tests marked green.
