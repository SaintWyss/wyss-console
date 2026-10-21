# Checkpoint

Current continuation: 2026-10-21, `integrate/f8-final`, worktree `~/Github_repos/wyss-console-rama-4`, private
`origin/main`. Recheck actual Git heads/status before writing or pushing. No public release.
The other worktree and all unrelated pending work are preserved.

## Implementation and acceptance are different states

| Block                                                          | Implementation                                                                                                          | Real acceptance / next requirement                                                                                                                           |
| -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| F1/F2                                                          | Implemented                                                                                                             | Accepted; `acceptance/f1-f2a-2026-10-19.md`, `f2b-owner-decisions.md`                                                                                        |
| F3 native sessions/profiles                                    | Implemented                                                                                                             | Accepted baseline; `acceptance/f3-2026-10-19.md`, ADR 0035                                                                                                   |
| F9 persistent host                                             | Implemented                                                                                                             | Accepted baseline; ADR 0036; preserve live terminals across GUI upgrades                                                                                     |
| F14/F11a/F13 baseline                                          | Implemented                                                                                                             | Historical acceptance in `acceptance/f14-f11a-f13-closure-2026-10-19.md`                                                                                     |
| F4a inventory/switches                                         | Implemented                                                                                                             | Baseline accepted; `acceptance/f4a-f5-f6-f1b-2026-10-19.md`                                                                                                  |
| F5 account hooks/MCP/instruction edits/templates/create/delete | Implemented                                                                                                             | Baseline real acceptance; ADRs 0042–0047 and linked acceptance records                                                                                       |
| F5 project files and hook/MCP templates                        | Implemented, ADR 0050                                                                                                   | Native authoring/Undo and template roundtrip PASS; confirmed template apply NOT_RUN; provider consumption NOT_PROVEN                                         |
| F6 models/catalogue/reroute/native effort                      | Implemented, ADR 0048                                                                                                   | Partial installed-provider acceptance; old host/authority lack new behaviour                                                                                 |
| F8 per-session runtimes                                        | Source complete behind the inactive boundary (protocol 2, V8, native local-tools, `Runtimes` window); ADR 0051 Proposed | Integrated acceptance PASS (F8-01..18, 29 methods); provider, installed migration, owner acceptance NOT_PROVEN; see f8-delivery-2026-10-21.md; no activation |
| P1                                                             | Backend baseline and idle optimization implemented                                                                      | Partial measurements; visual matrix NOT_RUN; no canonical budget pass                                                                                        |
| F10 gateway                                                    | Reviewed unreachable foundations, ADR 0049                                                                              | Authentication decision preparation only; no transport/listener/device enrollment                                                                            |
| F1b / B1 / G-INF                                               | Research and reviewed pure contracts                                                                                    | Isolation/provider claims remain partial or NOT_PROVEN; no production B1 worker                                                                              |

Authoritative record of this continuation:
[engineering acceptance and preservation](acceptance/engineering-2026-10-20.md).
Broader remaining scope: [roadmap](acceptance/remaining-roadmap-2026-10-19.md).
This supersedes earlier checkpoint statements that called F5 fully accepted, described project
selection as unimplemented, said gateway had no second review, or used old installed/runtime PIDs.

## Live installation observed on 2026-10-20

- Installed GUI revision `2f503902946a`, PID 1852991, is open after a GUI-only `--keep-host` update.
  Existing packaged authority payload was preserved byte-for-byte; compatible update pending remains.
- Host 1607394 and fish 1607420 retained identities/inode; the same terminal remains running.
  Authority 1167285 and Codex 1167441 retained identities, schema 7, runtime revision 9, ready/inference,
  seven cached models, unchanged outbound counters and durable head 76. No authority/host restart.
- Original provider configuration bytes/owners/modes and inventoried live process identities unchanged.
  Both project fixtures returned empty through Undo; GUI project selection cleared.
- Native AT-SPI control was available. Real chooser, trust/path rejection, instruction authoring,
  confirmations/cancellations, deletion/exact Undo and encrypted hook/MCP roundtrip observed PASS.
  Confirmed template application and provider consumption remain separate outstanding slices.
- Host protocol 1 still lacks effort-profile support; authority model-list lacks new offers/completeness.
  F6 incompatible-runtime behavior remains explicit; SG-12 unchanged. No paid provider experiment.
- Retained packages preserve previous GUI and the older inode used by the host. Rollback stops the
  host and must not run unattended. These historical PIDs are never cleanup targets.
- Details and local evidence: [native acceptance](acceptance/f5-native-wayland-2026-10-20.md).

## Current engineering decisions

F8 execution continues on the private integration branch, based on `172a1c4`; `main` remains
`1117217` until mandatory integration and security gates pass. All 29 protocol-2 methods route through
`authority.rs` or `operations.rs`; V8 durable writers, process workers, bindings and reads exist.
The local application selector now composes a validated prepared V8 image, one registry/writer,
F8 transport and supervised service. Native inference uses DeferredProcessWorker and the native
Codex adapter; the installed entry point still explicitly selects protocol 1. No installed database
is opened by the prepared-F8 constructor and no listener or environment activation switch was added.

Cold reviews found and corrected recovery/replay, native normalization and spawn ownership defects.
Pending spawn and timeout retain capacity/staging until reap. The child transfers its self-pidfd
before exec, avoiding parental acquisition by a recyclable child PID. See the
[execution and precise remaining gates](acceptance/f8-final-2026-10-21.md),
[independent security evidence](acceptance/f8-security-review-2026-10-21.md) and
[29-method source matrix](acceptance/f8-protocol2-method-matrix.md).
Native local-tools approvals and the coordinated protocol-2 window are implemented and accepted at
the application level ([delivery report](acceptance/f8-delivery-2026-10-21.md)); the activation
runbook is [prepared, not authorized](acceptance/f8-activation-runbook-2026-10-21.md).
Capacity stays one; multiprocess credential refresh, real provider, installed migration and owner
acceptance remain separate NOT_PROVEN gates. Historical installed observations below are not current
process identities or cleanup targets.

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
