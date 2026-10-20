# Engineering continuation: acceptance boundaries and preserved live sessions

2026-10-20. Started from clean `main` at `fb4a72c`, verified equal to private origin/main.
Worktree `~/Github_repos/wyss-console-rama-4`; the other worktree and its pending file were preserved.
Normal desktop/HOME/XDG/bus/wallet throughout. No authority/host restart, paid model prompt,
cache drop, reboot, broad signal, unrelated application closure or network exposure.

## F5 implementation versus installed native acceptance

F5 is implemented under ADR 0050; it was not rebuilt here. Inspection found three distinct states:

- Installed binary `--diagnose`: revision `2fdb88b2b992`, installation timestamp 2026-10-20T05:38:40Z.
- GUI 1607295 and host 1607394 actually execute the previous installation's binary. The host has
  one running terminal, ID `7a3cccd7-ed49-47bc-84df-0992335ef291`, not a native agent session.
- Authority 1167285 and its owned Codex 1167441 remain ready at their existing generation. Two
  managed session records exist. No managed mutation was sent by this continuation.

The current desktop is Wayland. Native computer control is disabled in the available UI tool;
`xdotool search --pid 1607295` found no X11 window; accessibility was not enabled. No native
dialog was exercised. Installing a new package alone cannot complete acceptance, so no install or
GUI replacement was performed. Do not substitute a nested display, browser page or disposable HOME.

| F5 native acceptance item                 | This run                 | Required next evidence                                                                            |
| ----------------------------------------- | ------------------------ | ------------------------------------------------------------------------------------------------- |
| Native project chooser                    | NOT_RUN                  | Real installed folder picker, selection and cancellation                                          |
| Trust and path rejection                  | NOT_RUN                  | Home/provider/credential/outside/link/untrusted-path refusals through the native UI               |
| Project create/edit/delete and exact Undo | NOT_RUN                  | Benign new project file, exact bytes/absence, ownership/modes after each Undo                     |
| Project switch during an operation        | NOT_RUN                  | A confirmation holds switch; queued B cannot receive A's write; stale plan/Undo refuses           |
| Hook/MCP encrypted template export/import | NOT_RUN                  | Native save/open/passphrase, ciphertext, wrong passphrase/domain, reviewed prefill and exact Undo |
| Native confirmation and cancellation      | NOT_RUN                  | Cancel picker/form/passphrase/confirmation; no plan/apply write after cancellation                |
| Provider discovery/consumption            | NOT_PROVEN               | Observe supported discovery without inference or side effects; do not infer execution/reload      |
| Original user configuration preserved     | PASS for inspected files | Final checksum/owner/group/mode comparison with pre-run snapshot                                  |

For the future GUI run, use only explicitly identified benign project entries, verified backup and
exact restoration. Never create a runnable hook/MCP entry in an original project as an implicit
provider-execution experiment. Keep Codex hooks blocked until G-INF. Importing a template alone
must not plan/apply anything; confirmation must still describe code execution. Closing/replacing
the terminal host would end the live shell and its in-memory state; replacing the authority would
end its current provider generation and invalidate live Managed attachments/work. Those operations
are deferred, not authorized implicitly by a GUI-only upgrade. The established install
`--keep-host` and compatible-authority update-pending mechanism remain the eligible update path;
installer rollback stops the host and must not run unattended.

## F6 non-disruptive provider acceptance

Read-only protocol-1 hello/status/model-list against the live authority passed. It reports Codex
0.159.0 in inference mode, seven cached models, zero thread/start, thread/resume or turn/start
outbound calls, and no offers/completeness fields. Reading the catalogue did not issue a new
provider probe. These are offered models, not entitlement or observed use.

Host hello passed and contains no `features` field: `profile_effort` is unsupported by this host.
Keep effort-bearing profile writes refused by the window; do not restart the host to test them.
New authority offers, completeness, replacement metadata, actual `model/rerouted` and new-host
effort persistence/launch stay **NOT_PROVEN** on this installed runtime.

The actual installed Claude Code, Codex and OpenCode `--help` commands each exited 0 under the
reviewed `Owned` lifecycle. Claude 2.1.304 advertises model and effort with
`low, medium, high, xhigh, max`; Codex 0.159.0 advertises model and `-c` overrides; OpenCode
advertises provider/model selection and a model-list command. No model-list command that might
refresh shared provider state was run; no agent prompt was submitted. CLI help confirms option
syntax only. Provider consumption remains **NOT_PROVEN**. SG-12 and managed model-choice exclusions
are unchanged. Historical stronger provider observations remain in `f6-models-2026-10-20.md`.

## F8, P1 and F10

F8: independent design review found seven issues. ADR 0051 now corrects V7→V8, both global SQL
indexes, per-launch state/correlation, explicit protocol targeting, fair cancellation, recovery
and consistent rollback. A second review checked dispositions; final wording corrections were
incorporated. [Research](../research/f8-runtime-decision-2026-10-20.md) and
[adversarial specifications](../spec/f8-runtime-acceptance.md) separate observations, future tests,
residual risks and owner decisions. Candidate reviewed; **NOT_PROVEN, not implementation-ready yet**.
Exact SQL, protocol shapes, numeric budgets and provider concurrency safety are still needed.

P1: 30-second read-only authority/provider/host resource observation completed, with swapped pages
and uncontrolled compilation/background load explicitly recorded. It is not the canonical idle
matrix. A release backend-only measurement uses the actual PTY/service and the reviewed owned
lifecycle, without a GUI, renderer or replacement host. Its final results are recorded below.
Real-desktop visual benchmarks remain **NOT_RUN**, requiring Santiago's presence and approval.

F10: [production authentication decision preparation](../research/f10-authentication-decision-2026-10-20.md)
compares standard TLS/device identity plus WebAuthn against standard HTTP signatures; explicitly
rejects promoting the test HMAC/custom signed tuple. Audited-version evidence, exact dependencies,
transport and device acceptance remain gated on review and owner decisions. No dependency or
gateway source changed, listener/transport/enrollment **NOT_RUN**.

## Regression findings and validation

The initial full run reproduced two existing failures from the F5 increment:

1. Exact command-surface test omitted the three ADR 0050 commands already in build/capability
   manifests. Updated the explicit expected list; actual permissions remain unchanged.
2. A negative template test contained literal fake URL credentials, correctly detected by the
   strict repository scanner. Constructed the same invalid URL at test runtime; validators and
   scanner remain unchanged. The scanner examines committed HEAD as well as index/worktree, so
   validation of the corrected committed tree follows commit `58bd3ea`.

Full Rust regression subsequently passed: 637 library + 361 binary + 1 authority-boundary tests;
7 library and 12 binary tests ignored. Formatting, check and all-target clippy passed. No blanket
`cargo test` or prohibited inherited `lifecycle_process` suite was run.

Frontend 49 files / 835 tests passed, but its first subsequent harness step failed closed once in
`test_an_error_while_signalling_still_closes_everything_and_ends_the_launch`: launch identity did
not match during `_verify_launch`. No foreign process was signalled. A fresh 15-test harness run
passed, as did 500 diagnostic owned launches; the transient identity failure was not reproduced
or explained, and no safety check was weakened to suppress it. Preserve this failed log alongside
the final checks; repetition is not proof of its root cause.

Final `npm run check` **PASS**: formatting, lint, typecheck, 49 files/835 frontend tests,
15 owned-lifecycle tests and production frontend build. The existing >500 kB chunk warning remains;
no threshold was raised. `npm run check:rust` **PASS** as counted above; repository scan **5/5 PASS**.
Existing packaged binaries passed fixture-marker exclusion; no new desktop package was installed
or claimed built by that exclusion check. Changed documentation relative links were validated.

## Backend measurement and preservation result

Release test executable built from `58bd3ea` (the source changes are tests only), seven ignored
backend measurements explicitly selected, serial execution, exit 0 in 144.20 seconds. The wrapper
retained the direct-child handle and pidfd through `benchmarks/owned.py`; no process-name lookup
or external authority/host was used for cleanup. Normal HOME and desktop environment were retained.

| Backend boundary                                          | Median | Bootstrap 95% interval |
| --------------------------------------------------------- | -----: | ---------------------: |
| M06 backend share per idle terminal, 0→16, KiB (3 sweeps) |  170.9 |            168.7–316.9 |
| M08 typed byte round trip, microseconds (3,000 samples)   |   99.8 |             94.8–106.1 |
| M09 BC-01 ASCII, MiB/s (10 samples)                       |  113.1 |            110.6–117.7 |
| M10 BC-02 dense colour, MiB/s (10 samples)                |  202.7 |            194.7–212.1 |
| M10 BC-03 wide characters, MiB/s (10 samples)             |  140.3 |            136.4–147.7 |
| M10 BC-04 long lines, MiB/s (10 samples)                  |  252.3 |            245.7–262.5 |

M08 p95/p99: 233.1/308.2 microseconds. Three samples beyond three IQR retained. M14 drained `yes`
for 60 seconds at 16.76 MiB/s (reader-paced, not a throughput budget), peak process-tree PSS
23,413 KiB. Ten seconds undrained: 65,536-byte buffer full, `truncated=false`, PSS 23,371→23,372 KiB.
Background load and swap changed throughout; this is one additional backend observation, not a
controlled comparison or renderer/input-to-glyph measurement. No performance budget is accepted.

Preservation comparison **PASS**: all four process start ticks/parents/executable identities,
running terminal ID/state, authority instance, runtime status and outbound counters, managed
session IDs/revisions, and inspected original config hashes/owner/group/modes matched the pre-run
snapshot. Config scope: Claude settings, Codex config and the present/absent OpenCode JSON/JSONC
config paths. No claim of whole-HOME immutability; unrelated live sessions continue writing normally.

Local raw logs, CLI help and metadata-only snapshots:
`.artifacts/acceptance/engineering-2026-10-20/` (`backend.jsonl`, `backend-run.log`,
`live-before.json`, `live-after.json`, resource samples and regression logs).
