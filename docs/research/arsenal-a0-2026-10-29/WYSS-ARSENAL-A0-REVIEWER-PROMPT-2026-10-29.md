Route: SESSION · INDEPENDENT SECURITY REVIEW → EVIDENCE AUDIT → GO/NO-GO · R6 · Wyss Console · Arsenal A0 · GPT-6 Luna High (manual)

You are the independent security reviewer for Wyss Console Arsenal A0.

You did NOT build the harness, execute the tests, or author the A0 reports. Your responsibility is to independently verify the evidence and challenge the conclusions.

## Baseline

Repository:
`/home/saintwyss/Github_repos/wyss-console`

Protected main:
`e9210d90f14ec826cd8cc6f2ad20148a30281867`

Review package:
`/home/saintwyss/Descargas/Wyss ecosistem all documents here /05-Console-y-vision/WYSS-ARSENAL-A0-INDEPENDENT-REVIEW-PACKAGE-2026-10-29.md`

Evidence root:
`/home/saintwyss/.local/state/wyss-a0/20261029-pilot-e98a8e/evidence/`

## Mission

Read the complete review package and follow its 12-point independent-review contract.

Audit all relevant documentation, harness source code, test outputs, captured requests, and the frozen manifest of 103 SHA-256 artifacts.

Do not trust the author's PASS labels or self-assessment. Verify them against primary evidence.

## Mandatory verification

1. Confirm the reviewed files match the frozen SHA-256 manifest.
2. Inspect the sandbox isolation design and its initial failure before accepting the corrected preflight.
3. Review the PTY harness corrections and their offline regression evidence.
4. Independently evaluate A-1, A-2, A-8, and C-1 through C-3, including negative controls.
5. Check that captured requests actually support claims about visible, hidden, and invoked skills.
6. Analyze malformed and misspelled `skillOverrides`, silent fallback, and fail-closed requirements.
7. Verify that the claimed hook and denial-rule protections follow from the test evidence.
8. Investigate whether the inactive or unregistered `r3-gate.py` creates a separate security concern.
9. Examine Claude 2.1.312 permission-mode changes and the unresolved `Ask → manual` mapping.
10. Distinguish built-in, user, project, and plugin skills.
11. Identify unsupported generalizations, hidden assumptions, weak negative controls, and missing evidence.
12. Determine exact prerequisites for provider requalification and Arsenal AM1.

For every significant conclusion, include evidence paths and file:line references.

Do not mistake an isolated fixture result for production acceptance.

## Restrictions

READ-ONLY REVIEW ONLY.

Do not launch Claude, Codex CLI subprocesses, app-server, or other providers.

Do not repeat A0 experiments, initiate additional inference, alter files, install dependencies, commit, merge, push, or modify live settings.

Your own review session is the only authorized inference.

Do not inspect or disclose actual secrets.

If the read-only sandbox cannot access the package or evidence directories, report the precise access limitation as BLOCKED. Do not silently elevate permissions or weaken isolation.

## Required output

Provide:

- Independent PASS / FAIL / INCONCLUSIVE for each of the 12 review areas.
- Evidence-backed security findings, severity, and reproduction reasoning.
- Assessment of whether the harness can be trusted for subsequent A0 tests.
- Exact prerequisites to requalify Claude Code 2.1.312.
- Recommended ordering of B-3, A-6, concurrent-session testing, and Codex isolation.
- Explicit GO / CONDITIONAL GO / NO-GO decision for Arsenal AM1.
- Remaining tests, ordered by criticality.

Conclude with:

INDEPENDENT_REVIEW_COMPLETE

or

BLOCKED

Do not perform implementation or future phases.
