# Checkpoint

Documentation baseline: `main` at `e480d479fdbd8a215612b1d809862a62aa914eb5`,
2026-10-22. Phase1 consolidates the complete personal product only. No application
source, installed state, credentials, services or user sessions are changed.

## Source and recorded production state

| Area                | Current status                                                                        | Next work                                                         |
| ------------------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| F8                  | Production-active P2/V8, real Codex 0.159.0, ADR0051 Accepted                         | Preserve recovery/safety; broader exact conformance evidence      |
| Managed capacity    | One runtime                                                                           | Shared-provider refresh/resource proof before increase            |
| P1/V7               | Still available, separate authority/store                                             | Retirement is a separate owner operational decision               |
| Claude/Codex Native | Launcher/registry/profiles and parallel sessions built                                | Full original-feature/consumption/effort acceptance               |
| Claude Managed      | Not implemented                                                                       | Pinned integration, auth/effect/attestation proof and safety ADR  |
| Projects            | Known folders/defaults, scoped config/layout built                                    | Primary stable project/workspace model and migration              |
| F5                  | Supported authoring/Undo/templates built                                              | Confirmed imported-template apply NOT_RUN; consumption NOT_PROVEN |
| F6                  | Catalogue/observed model/reroute handling and Native effort built, partially verified | Managed choice and durable metrics not built                      |
| F9/T1/T2            | Persistent host, panes/search/keys/layout baseline                                    | Faithful checkpoint/parser isolation and final lifecycle evidence |
| OL3/B1              | Contracts/research                                                                    | No production coordinator or staged/quarantined worker            |
| F10                 | Unreachable gateway foundations                                                       | Transport/auth/enrollment, remote PC/native Android               |
| P1 performance      | Backend/idle observations                                                             | Complete visual/reference/device budgets NOT_PROVEN               |

F8 closure: [activation/result](acceptance/f8-activation-procedure-2026-10-22.md) and
[ADR0051 final addendum](adr/0051-per-session-managed-runtimes.md#acceptance-and-activation-2026-10-22).
Restart/wanted-runtime Stop→Close is fixed at this baseline and recorded as reverified
in production. Earlier reports remain dated evidence. This phase does not refresh live
process identities; historical PIDs are never targets.

## Resume

Read the [product index](product/README.md), [functional specification](product/functional-specification.md),
[architecture](product/architecture.md), [provider matrix](product/provider-matrix.md),
[gap register](product/implementation-status.md), [roadmap](product/roadmap.md) and
[traceability](product/traceability.md). WD-01/02 are owner-approved product scope.
ADR0052/0053 are Proposed. This Phase1 PR authorizes no implementation/experiment.

## Verification policy preserved

Use `npm run check` and `npm run check:rust` for their defined repository suites,
plus docs-reference/scope checks. **Do not run blanket `cargo test` or `lifecycle_process`**:
the inherited latter creates a private user/systemd environment. Real acceptance uses
the owner's normal native environment; old disposable HOME/keyring/bus/VM/container
procedures are historical. Unit fixtures and intrinsic Managed isolation are distinct.

Never restart/upgrade host or authority to obtain evidence without the applicable
operational authorization. Installer rollback may stop the host; account for live work,
matching binaries, consistent backup and post-snapshot loss. No SQL downgrade or arbitrary
PID cleanup. No live SQLite DB-only copy ignoring WAL consistency.

Configuration changes retain exact paths/metadata, current-plan checks, sealed restoration
and applicable Undo. Do not assume provider consumption. Codex hook authoring stays
blocked until G-INF/trust requirements pass. Managed model choice is excluded by current
authority/adapter contract; SG-12 itself is not a model-selection rule and is unchanged.

The previous [checkpoint snapshot](history/CHECKPOINT-before-phase-1-2026-10-22.md)
preserves continuation history without treating old observations as fresh. Current
checks/review belong to [traceability](product/traceability.md) and
[independent review](product/independent-review.md).
