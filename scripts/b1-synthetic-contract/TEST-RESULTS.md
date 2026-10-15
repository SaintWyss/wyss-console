# B1 synthetic contract reference — local test record

Date: 2026-10-14 (Argentina local date). **Fixture-only; not a native CachyOS isolation verification.**

Command:

```sh
node --test scripts/b1-synthetic-contract/b1-contract-fixture.test.mjs
```

Locally executed in an isolated Linux container using **Node.js v22.16.0**, without any provider, user service, sudo, rootful Docker daemon, real project, credentials or personal files. All hardlink/filesystem resources were created under a fresh uniquely named temporary directory by the test itself and removed after the test.

**Observed result: 12 tests executed; 12 PASS; 0 FAIL; 0 skipped.** The test runner printed TAP v13, exit code 0; test duration approximately 70 ms. This result applies to the source files below, not to Wyss B1 or CG-01..CG-16.

SHA-256 (exact source files tested and uploaded to this branch):
- `b1-contract-fixture.mjs`: `740ae51a36f88961e68382b06c82af9985906c3b3cb3e8e1e17fc50d75f03188`
- `b1-contract-fixture.test.mjs`: `2a0c61a84665afc5d8d8d034ff4b825e18a2ba24eb897077c21f8887a79a78b3`

The fixture tests pure admission/export/quarantine semantics and one **synthetic hardlink-to-independent-inode copy**. It does not establish: authoritative human approval, Git OID provenance, OS process namespace isolation, Landlock, bubblewrap, privileged broker defense, real worker execution, authenticated provider use, egress containment, cgroups, crash recovery, durable CAS or final product compatibility.

**Security state remains FX-H01 OPEN / B1 NOT_PROVEN / CG-01..CG-16 NOT_RUN on the reconciled implementation.** No PR merge, ADR acceptance, or B1 runtime enablement follows from this fixture.
