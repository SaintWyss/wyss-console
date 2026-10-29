# Wyss Arsenal — design and A0 validation record (2026-10-29)

Research material, not implementation status. Nothing here changes product code, provider
capability allow-lists or accepted ADRs. Claude Code 2.1.312 remains unqualified in
`src-tauri/src/native/capabilities.rs`.

**Current verdict:** independent review (Codex, read-only): **NO-GO for full AM1**. Read the
[errata](WYSS-ARSENAL-A0-ERRATA-2026-10-29.md) before relying on any earlier document.

## Documents, in order

| #   | Document                                                                                                                                        | What it is                                                                                               |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| 1   | [Design](WYSS-ARSENAL-DESIGN-2026-10-29.md)                                                                                                     | Arsenal architecture and roadmap (contains a corrected R3 claim, see errata)                             |
| 2   | [A0 validation plan](WYSS-ARSENAL-A0-VALIDATION-PLAN-2026-10-29.md)                                                                             | Test matrix A–J, isolation procedure, approvals                                                          |
| 3   | [A0 pilot](WYSS-ARSENAL-A0-PILOT-REPORT-2026-10-29.md)                                                                                          | Isolation preflight and A-1 control                                                                      |
| 4   | [A-1 interactive retest](WYSS-ARSENAL-A0-A1-INTERACTIVE-RETEST-2026-10-29.md)                                                                   | PTY harness fixes and retest                                                                             |
| 5   | [Claude isolation](WYSS-ARSENAL-A0-CLAUDE-ISOLATION-2026-10-29.md)                                                                              | A-2, A-8, C-1..C-3 on Claude Code 2.1.312                                                                |
| 6   | [Review package](WYSS-ARSENAL-A0-INDEPENDENT-REVIEW-PACKAGE-2026-10-29.md) and [reviewer prompt](WYSS-ARSENAL-A0-REVIEWER-PROMPT-2026-10-29.md) | Frozen inputs for the independent review                                                                 |
| 7   | [Errata](WYSS-ARSENAL-A0-ERRATA-2026-10-29.md)                                                                                                  | Builder's corrections confirmed after the review                                                         |
| 8   | [Offline remediation, step 1](WYSS-ARSENAL-A0-REMEDIATION-STEP1-2026-10-29.md)                                                                  | harness-v2, oracle, limits and tamper-evident evidence; builder's report, not yet independently reviewed |

The documents are copies of the originals kept outside the repository. Markdown formatting may
differ from the frozen copies because of the repository's prettier formatting. The SHA-256 values
in the review package and in `evidence/MANIFEST-v2.json` refer to the original files, not to these
copies.

## Evidence in this directory

`evidence/` holds only summaries: `MANIFEST-v2.json` (+ `.sha256`), the original 98-entry v1
manifest, and the final offline test outputs (`final-run/`). The raw captures stay local under
`~/.local/state/wyss-a0/20261029-pilot-e98a8e/`, as the repository does with other raw evidence:
request bodies, screen recordings, integrity snapshots with process and file metadata, flood
fixtures and the backup tarball.

## Harness source

`scripts/research/a0/` holds reference copies:

- `harness-v1/`: the frozen harness used by the pilot and isolation runs;
- `harness-v2/`: the remediated harness;
- `tests/`: the offline suites;
- `make_manifest_v2.py`.

The tests expect the run-root layout (`harness/`, `harness-v2/`, `evidence/`), not these repository
paths. Two intentional differences from the frozen copies, same runtime values: `SYNTH_KEY` is
written as `"sk" + "-ant-api03-…"` so secret scanners do not flag the synthetic placeholder, and
`preflight_probe.py` resolves the owner's home with `pwd.getpwuid(os.getuid()).pw_dir` instead
of a literal path (repository rule: no home paths in code).
