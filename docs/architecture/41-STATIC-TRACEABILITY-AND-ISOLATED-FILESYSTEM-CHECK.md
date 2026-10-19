# 41 — Static documentation integrity and disposable-filesystem verification

Date: 2026-10-13. **Limited verification report, NOT a pass of the final product.**
The owner explicitly allowed a bounded technical-verification stage in
isolated environments on 2026-10-13, while continuing to prohibit
application implementation, modification of real configuration,
uncontrolled provider execution and merge to `main`.
This report used only GitHub source/doc reads and freshly created,
automatically cleaned test files in the execution container.

## 1. Documentation cross-reference verification (PASS within scope)

**Procedure:** read the GitHub planning branch's directory inventories
(`docs/architecture/`, `docs/adr/`,
`docs/architecture/reviews/` and `docs/`), then fetch
**41 architecture Markdown files** (`00` through `40`).
Parse Markdown relative link destinations of the form
`[label](relative/path)` and normalise `.` and `..`
relative to the source document; check existence in the
repository inventory. External links and fragment identifiers
are outside this existence check.

| Batch     | Architecture docs read | Relative links checked | Missing destinations |
| --------- | ---------------------: | ---------------------: | -------------------: |
| 00–11     |                     12 |                    121 |                    0 |
| 12–23     |                     12 |                    162 |                    0 |
| 24–35     |                     12 |                     87 |                    0 |
| 36–40     |                      5 |                     40 |                    0 |
| **Total** |                 **41** |                **410** |                **0** |

The root index links to `../adr/` as a directory; the checker
verified that directory exists. This report does **not** claim:

- anchor/heading fragment resolution,
- external URL liveness,
- natural-language consistency between linked documents,
- Markdown formatting or the repository's npm tests passing.

## 2. New owner requirements and traceability (static PASS within scope)

Fetched [02](02-REQUIREMENTS.md), [20](20-VERIFICATION-AND-ACCEPTANCE.md)
and [23](23-TRACEABILITY-MATRIX.md) on the planning branch.
Identified _*40 new stable FR-* requirements_* added in 02 §4.
Checked each has a matching identifier in 23 §5.
Checked the 40 associated `AC-FINAL-01..40` in 23
exist in the acceptance specification in 20 §14, and each
new `TS-ORC/TS-HYB/TS-FINAL` used in that crosswalk has a
matching test ID in 20 §14. Results:

- Missing new requirement IDs from trace: **0**.
- Missing new AC-FINAL specifications: **0**.
- Missing crosswalk TS-ORC/TS-HYB/TS-FINAL specifications: **0**.

This is an **identifier-presence** check, not proof of test
quality, test execution, correctness of mappings or coverage
of the historically missing 40 acceptance criteria counted
by [31](31-PLANNING-COVERAGE-AUDIT.md).
The further handoff/adapter/authoring/observability mappings
of 23 §6 are a supplemental paper specification; none
has been executed.

## 3. Disposable Linux atomic name-exchange experiment (PASS/EXPECTED FAIL)

**Question tested:** Can `renameat2(RENAME_EXCHANGE)`
exchange existing file names inside one directory/mount,
and can it exchange across two different filesystem mounts?

**Environment:** isolated execution container, Linux kernel
`6.18.44`; two freshly created private temporary
directories under `/tmp` and `/dev/shm`, respectively.
`stat.st_dev(/tmp)=27` and `stat.st_dev(/dev/shm)=26`.
No user home, GitHub checkout, provider config, secrets or
system service was touched. Test only creates and cleans
three files with fixed one-character strings.

**Method:** invoke libc `renameat2(AT_FDCWD, a,
AT_FDCWD, b, RENAME_EXCHANGE)` using `ctypes`.
First exchange `a='A'` and `b='B'` inside `/tmp`.
Then attempt exchange across mounts between
`a='B'` under `/tmp` and `c='C'` under
`/dev/shm`; check both contents unchanged.

| Case                                 | Expected                                         | Observed result                                                     |
| ------------------------------------ | ------------------------------------------------ | ------------------------------------------------------------------- |
| Same mount, same directory           | Successful atomic name exchange                  | Success; `a='B', b='A'`                                             |
| Different mounts, even both writable | Explicit error; no exchange, preserve both files | `errno=18 EXDEV` (Invalid cross-device link); `a='B', c='C'` intact |

**Interpretation:** reinforces the known boundary that a staging
area cannot be moved to a different mount and still use
the proposed atomic name-exchange mechanism. This is
consistent with the prior **owner-machine** observed EXDEV
reported in 30 §8, but it is **a separate container**
with different filesystem types.

**Does NOT prove:** atomicity of a proposed complex
transaction protocol, `fsync` durability, provider
behaviour with an open descriptor, Btrfs nested subvolume
behaviour on owner's exact layout, Syncthing ignore
effectiveness, snapshot exclusion, vault key security, or
loss-freedom under concurrent external writers.
L1/L2/L5 and DN-31 therefore remain BLOCKED.
This kernel observation cannot accept ADR 0029/0030
or authorise direct writes.

## 4. Additional static source-reading evidence

- [39](39-REMOTE-CANDIDATE-SOURCE-EVIDENCE.md):
  pinned `n0-computer/iroh-ffi` version and Kotlin
  Android API, compared against Tailscale documentation;
  no Android connection, latency or cost was measured.
- [40](40-HERMES-BOUNDARY-STATIC-EVIDENCE.md):
  pinned `NousResearch/hermes-agent` paths showing
  Python-code execution backends and process/session
  bypass toggles. Does not prove confinement, exploits
  or behaviour on user's machine.

## 5. Gates still outstanding, all unaffected by these checks

- A genuinely independent post-REJECT review of the whole
  amended plan is **not yet complete**. Its handoff is
  [38](38-INDEPENDENT-REVIEW-HANDOFF.md).
- Remote H1/H2 and Android tests require an isolated
  Android target and Linux service/emulator setup, with
  representative networks, not merely upstream docs.
- Hermes tool isolation requires disposable OS
  identity/process and pinned executable with no
  real provider account or owner credentials.
- DN-21/DN-31 require **evidence on the user's actual
  filesystem/snapshot/replication semantics** before
  protecting real provider configuration. The container's
  successful same-directory rename cannot unblock them.
- All selected ADRs are still Proposed or not authored;
  no application code implemented or released.

**Result for final-product signoff: BLOCKED.** These
limited PASS results concern link integrity, ID
presence and a narrow POSIX-kernel behaviour,
not architecture acceptance or product readiness.
