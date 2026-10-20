# Existing branch reconciliation, 2026-10-19

Santiago authorised integrating valid remaining branch work into private `main`, then
explicitly excluded the F1 trial branches `rama-1`, `rama-2` and `rama-3`. Baseline:
`16c95fed36dbfbd10a95be598ddf4b49292cb63a`. Remote heads were fetched and checked before
editing. Existing worktrees and branch references are retained; none is deleted or reset.

## Integrated and recovered

| Source                                                                                                 | Decision                                                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `docs/b1-fxh01-reconciled-design-2026-10-14` (`eadc42b`) and local `feat/b1-pure-contracts-2026-10-15` | Integrated through the B1 chain; preserve the design as dated proposals, with current policy/scope notes. No ADR accepted.                                                                 |
| `santiagosw/b1-pure-rust-contract-foundation` (`e48b011`)                                              | Integrated: pure typed identities/records, path validation, advisory classification and lifecycle.                                                                                         |
| `santiagosw/b1-canonical-admission` (`7cb218b`)                                                        | Integrated: canonical domain-separated digests and bounded admission of caller-supplied in-memory bytes.                                                                                   |
| `santiagosw/b1-snapshot-verify-capture-fixture` (`1550410`)                                            | Integrated: byte-recomputed snapshot verification and Linux filesystem fixtures compiled only under `cfg(test)`.                                                                           |
| `santiagosw/tender-albattani-udd3ir` (`344143d`)                                                       | Integrated as historical planning audit, explicitly scoped to old HEAD `906822f`; neither a current product audit nor an independent review.                                               |
| `feat/native-e2e-fixture-harness` (`7e7106d`)                                                          | Selective recovery only: standalone product-binary fixture-marker detection. The harness, fake provider launch, isolated service lifecycle, environment overrides and seeder are excluded. |

The B1 merge is `3f9bf38`, and the historical audit merge is `c2fc6c2`. This B1 design's
historical phase name “P1” is unrelated to the product performance baseline P1.

The pure B1 module is not called from the desktop, host, authority methods or provider
launcher. No trusted capture/materialization/worker runtime, quiescence attestation,
export service, durable quarantine publisher or real authorisation service is delivered.
Production authority/protocol/capability/session/configuration source paths match the
baseline. There are no new dependencies or runtime permissions. **FX-H01 remains open;
B1 containment and G-INF remain NOT_PROVEN.** Actual B1 runtime integration still needs
the dedicated contract and security verification; these unit tests cannot substitute it.

## Review corrections and verification

The original B1 tests passed (119). Two additional negative tests then reproduced:

1. `PENDING` accepted a workspace whose source manifest differed from the admitted tree.
   Pending evidence now compares those digests.
2. Export validation accepted a deletion and addition whose names differed only by ASCII
   case. All export entries, including tombstones, now undergo case-collision checks.

Both new tests failed on the incoming code and passed after correction. A current Clippy
warning was fixed using `as_chunks` (within the existing Rust minimum version); outdated
comments about computed digests were corrected.

Executed on the normal workstation:

- `npm run check`: **48 files / 804 tests**, format, lint, Svelte/TypeScript and frontend
  release build passed. Existing Vite bundle-size advisory remains.
- Policy-compatible Rust regression (`--lib --bins --test authority_boundary`):
  **622 library + 291 binary + 1 boundary = 914 passed**, 12 ignored.
- After the final equivalent hex-decoding cleanup, focused B1: **121 passed**;
  documentation/compile-fail examples: **14 passed**.
- `cargo fmt --check` and all-target Clippy with `-D warnings`: passed.
- Standalone fixture scan: the existing real package's two binaries passed.

The recovered packaging check uses bounded chunked reads, scans exactly the two expected
package binaries, detects markers crossing chunk boundaries and fails on missing,
incomplete, linked, special, empty or oversized payloads. It runs before the package
artifact is replaced; `npm run check:package-fixtures` also invokes it independently.
Marker absence is not a security attestation or general malware detection.

The new filesystem fixtures are ordinary unit tests in task-owned temporary directories;
they create no HOME, bus, keyring, user installation or provider/service environment.
No inherited private-systemd lifecycle suite or excluded native E2E harness was run.
No model inference, credential experiment, network listener, third-party install or
provider configuration mutation occurred in this reconciliation.

Native GUI behaviour is unchanged. No new GUI/provider acceptance is claimed for these
backend contracts. The existing installed `e17dfc2d8264` diagnostic still passed with
host PID 1188641, normal Secret Service and installed providers. The installation,
previous rollback copy, running GUI and original terminal were not replaced or stopped.

Package rebuild and final remote verification will be recorded below after completion.
Local execution logs: `.artifacts/acceptance/branch-integration-2026-10-19/`.

## Intentionally excluded

| Branch                                                           | Reason                                                                                                                                                                                                          |
| ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `rama-1` (`628c2b1`), `rama-2` (`07ae7fc`), `rama-3` (`d9963aa`) | Explicit owner exclusion. Early alternative F1 implementations; current main uses the subsequent reviewed configuration implementation (`4e6fc4b` and successors). No merge or cherry-pick from these branches. |
| `prototype/b1-synthetic-contract-2026-10-14` (`10b9c33`)         | Historical JavaScript reference is weaker than the recovered Rust contracts and is not a valid production security implementation. Only the shared design ancestors are integrated.                             |
| Remaining native E2E harness                                     | Constructs disposable HOME/XDG/provider environments and a private systemd manager; incompatible with current owner policy. Its isolated-environment safety utilities have no current product caller.           |

The excluded JavaScript prototype was inspected and reproduced only as a pure-data
review, without running its historical container recipe. It accepted `C:escape` and
`.git./config`; changing an export from mode `100644` to `100755` without changing its
export digest still published `PENDING`. Result: `prototype-review.json` in the local log
directory. These are defects in the excluded prototype, not reproduced product bugs.

All other existing local/remote branches had no unique commits relative to baseline main.
“Behind main” alone does not mean pending work. Excluded branches remain available for
history; their continued unique commit counts are deliberate, not a missing push.
