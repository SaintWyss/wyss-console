# Checkpoint

Current continuation: 2026-10-20, `main`, worktree `~/Github_repos/wyss-console-rama-4`, private
`origin/main`. Recheck actual Git heads/status before writing or pushing. No public release.
The other worktree and all unrelated pending work are preserved.

## Implementation and acceptance are different states

| Block                                                          | Implementation                                     | Real acceptance / next requirement                                                                 |
| -------------------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| F1/F2                                                          | Implemented                                        | Accepted; `acceptance/f1-f2a-2026-10-19.md`, `f2b-owner-decisions.md`                              |
| F3 native sessions/profiles                                    | Implemented                                        | Accepted baseline; `acceptance/f3-2026-10-19.md`, ADR 0035                                         |
| F9 persistent host                                             | Implemented                                        | Accepted baseline; ADR 0036; preserve live terminals across GUI upgrades                           |
| F14/F11a/F13 baseline                                          | Implemented                                        | Historical acceptance in `acceptance/f14-f11a-f13-closure-2026-10-19.md`                           |
| F4a inventory/switches                                         | Implemented                                        | Baseline accepted; `acceptance/f4a-f5-f6-f1b-2026-10-19.md`                                        |
| F5 account hooks/MCP/instruction edits/templates/create/delete | Implemented                                        | Baseline real acceptance; ADRs 0042–0047 and linked acceptance records                             |
| F5 project files and hook/MCP templates                        | Implemented, ADR 0050                              | **Native acceptance NOT_RUN**, provider consumption NOT_PROVEN                                     |
| F6 models/catalogue/reroute/native effort                      | Implemented, ADR 0048                              | Partial installed-provider acceptance; old host/authority lack new behaviour                       |
| F8 per-session runtimes                                        | **Proposed design only**, ADR 0051                 | Independently reviewed candidate, NOT_PROVEN; not implementation-ready yet; owner has not accepted |
| P1                                                             | Backend baseline and idle optimization implemented | Partial measurements; visual matrix NOT_RUN; no canonical budget pass                              |
| F10 gateway                                                    | Reviewed unreachable foundations, ADR 0049         | Authentication decision preparation only; no transport/listener/device enrollment                  |
| F1b / B1 / G-INF                                               | Research and reviewed pure contracts               | Isolation/provider claims remain partial or NOT_PROVEN; no production B1 worker                    |

Authoritative record of this continuation:
[engineering acceptance and preservation](acceptance/engineering-2026-10-20.md).
Broader remaining scope: [roadmap](acceptance/remaining-roadmap-2026-10-19.md).
This supersedes earlier checkpoint statements that called F5 fully accepted, described project
selection as unimplemented, said gateway had no second review, or used old installed/runtime PIDs.

## Live installation observed on 2026-10-20

- Installed `--diagnose`: revision `2fdb88b2b992`, timestamp `2026-10-20T05:38:40Z`.
- GUI PID 1607295 and host PID 1607394 execute the **previous** installation, not the binary currently
  at the launcher path. Host has one running non-native terminal. Do not replace/stop it.
- Authority PID 1167285 owns Codex PID 1167441, 0.159.0, ready/inference, runtime revision 9.
  Two managed session records; seven cached models. No thread/turn writes observed in status counters.
- Host protocol 1 hello has no features: effort-profile support absent. Authority model-list lacks
  offers/completeness. Keep compatible fallback and update-pending behaviour; do not restart for acceptance.
- These identities are historical evidence, **never signal targets**. Recheck start ticks and owned
  process handles before every operation. No install, authority/host restart or GUI replacement this run.
- Native GUI is Wayland and available UI tools lack native control. F5 real dialogs remain unexercised.
  Do not claim acceptance using a browser, nested display or artificial HOME.

## Current engineering decisions

F8 candidate ADR 0051 now targets **V7→V8**; V7 already exists. Both active-turn and pending-approval
indexes, all transient routing and private per-launch provider/catalogue state need scoping. Candidate
protocol 2 refuses ambiguous v1 targeting. Explicit-only recovery and fresh leases after runtime loss
are proposed changes needing owner acceptance. Independent design review found seven issues and
checked corrections; implementation security review remains future work. Exact SQL, protocol shapes,
numeric budgets, qualified concurrency measurements and provider shared-state compatibility remain.
See [F8 evidence](research/f8-runtime-decision-2026-10-20.md) and
[adversarial specification](spec/f8-runtime-acceptance.md).

F10 authentication preparation recommends standard authenticated TLS plus standard WebAuthn user
verification for further review; no custom production signing protocol and no dependencies added.
Selected-version audit coverage, device/origin/transport policy and dependency approval remain open.
See [F10 decision preparation](research/f10-authentication-decision-2026-10-20.md).

P1 backend and read-only observations continue without interrupting live processes. Visual desktop
benchmarks require Santiago's presence and explicit approval. Do not drop caches, reboot, close other
applications or substitute a software-rendered/nested environment. Unmet conditions stay NOT_RUN.

## Testing and safety policy

Normal owner desktop/HOME/XDG/session bus/wallet only for development and real acceptance. Ordinary
unit fixtures are permitted; no disposable user installation, private keyring/bus, VM or container.
Historical isolated acceptance records are evidence, not current recipes.

Use `npm run check` and `npm run check:rust`. Do **not** run blanket `cargo test`: inherited
`lifecycle_process` creates a prohibited private user/systemd environment. Do not weaken a failed
ownership check. The 2026-10-20 continuation records one transient fail-closed owned-launch check;
its repeat and 500 diagnostic launches passed, but its cause is not established.

Configuration changes need exact paths/metadata, verified backup and byte/mode restoration. Never
print secrets or assume provider consumption. Codex hooks stay blocked until G-INF. Managed model
choice stays excluded under ADRs 0014/0015 and DN-42; SG-12 remains unchanged.

Installer `--keep-host` preserves a compatible live host. Compatible authority mismatch is update
pending, not an automatic restart. Installer rollback **stops the host** and cannot run unattended.
Installed authority migration/rollback, live-session interruption, paid-quota experiments, new
crypto dependencies, listener/transport and real-device enrollment need specific owner decisions.

## Historical integration and incident evidence

Validated branch integration remains preserved, including pure B1 contracts and fixture packaging
checks; excluded F1 trial branches, weaker JavaScript prototype and incompatible native-E2E user
harness remain excluded. See [branch record](acceptance/branch-integration-2026-10-19.md).

The earlier process-name benchmark destroyed the original fish session; a replacement shell is not
recovery. Only verified owned handles may drive cleanup. See
[incident and reviewed lifecycle](incidents/2026-10-20-benchmark-ended-live-session.md).
