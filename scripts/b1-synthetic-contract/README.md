# Wyss Console — synthetic B1 contract fixture (non-production)

Date: 2026-10-14. This artifact is **not** a Linux security sandbox, a trusted owner-approval service, an implementation of Wyss Console B1, or an independent security review. It contains two dependency-free ES modules to exercise narrow, pure-data admission and quarantine semantics. It never calls a real model provider, Docker, sudo or a user service.

## Reproduce

Requires Node.js >= 20. From this extracted directory:

    node --test b1-contract-fixture.test.mjs

The real-hardlink fixture creates an exclusively synthetic temporary tree under `os.tmpdir()` and removes exactly that tree on completion. No real credential files or pre-existing projects are touched. The admission/export functions operate on in-memory caller-provided bytes, not untrusted host files. Do **not** use these modules as production security controls.

## Covered in this fixture

- Explicit staged/untracked content selection without requiring commits (simplified `put`/`delete` model; renames are not separately modeled).
- Digest check on bytes, kind, mode and selected target; fail-closed path and lexical symlink validation.
- Copying bytes out of a hardlinked fixture creates a distinct inode; subsequent staging writes do not change the synthetic external canary. This demonstrates **inode separation only**, not protection against copying an owner-admitted secret.
- Typed simulated export and an in-memory pending quarantine record; duplicate publication fails CAS.
- Bad `.git` paths, external symlinks and unsupported file kinds cause visible rejection.

## Not covered (do not mark gates PASS)

No native CachyOS verification, Bubblewrap, Landlock, process/FD isolation, broker privilege boundary, resistant snapshot of concurrent writers, immutable source authority, whole-process-tree quiescence, secure exporter OS mounts, durable Git quarantine, crash recovery, provider auth, egress or real host-global approval. None of CG-01..16 can be closed by these unit tests. These are P1 reference tests only and cannot bypass P2 independent security review or P3 deployment authorization.

The source files' SHA-256 values are recorded in the accompanying verification report.
