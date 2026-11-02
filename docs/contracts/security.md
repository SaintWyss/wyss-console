# Trust boundaries and restricted-worker candidate

Current lanes are distinct: unrestricted Native/account access, Native delegation, Managed,
restricted B1 candidate, untrusted planner and protected human approval. Same-user native
confirmation/Unix sockets are useful product controls, not an independent approval principal
against arbitrary same-UID processes. Privileged delegation, including a rootful container
daemon available to the account, defeats account-level assumptions; do not infer the current
machine's privileges from an old report. No boundary protects against an already privileged
outside administrator or a compromised kernel.

## Accepted limits and open choices

Native uses the user's account rights. Delegation adds its documented workspace/process
containment but retains writable provider homes and other stated exposure; it is not B1.
Managed Local Tools retains credential/hardlink/TOCTOU risks described in
[Managed runtime](managed-runtime.md). Excluding an Arsenal item is not filesystem denial.
Encrypted custody does not protect originals, external snapshots or an unlocked same-UID wallet.

Stronger approval needs all of: no route from arbitrary same-account processes or confined
workers, a distinct approver principal, verified human presence and independent OS-boundary
review. Those conditions are not established. Protected writer/sync/snapshot conditions
also remain open: marker files or ignores do not prove exclusion from snapshot/replication.
Choose a concrete restricted surface or explicitly accept the narrower risk before changing
the guarantee; do not silently undo accepted custody or already implemented authoring.

## B1 candidate — Proposed, fixture only

The design formerly ADR0058 remains **Proposed**. `b1-fixture` code is excluded from production
admission; `STRONGER_APPROVAL_MODE`, `NATIVE_AUTOMATED_STEPS`, `B1_PRODUCTION_LAUNCH` and
`PLANNER_EFFECT_AUTHORITY` remain false. Native/planner output is `Untrusted<T>` and crosses
only refusing validators. A successful fixture never opens any of these gates.

Required candidate layers: rootless Bubblewrap user/mount/PID/network namespaces, private
selected worktree, verified executable descriptor and pre-exec shim. Close inherited fds;
drop capabilities, set `NoNewPrivs`, apply seccomp and required Landlock ABI6 before work.
Missing required layers refuse. Deny x32 syscall numbers and dangerous namespace/mount,
ptrace, process-memory, keyring, BPF and io_uring paths; `clone3` returns ENOSYS. No host
authority/account bus/keyring/Docker socket or host environment; private network namespace,
AF_UNIX only, no host abstract/path socket route. Provider authentication/egress is unqualified.

Copy selected bytes into distinct inodes, never hardlink a host object. Runtime/workspace
are executable, scratch is not; writes are limited to workspace/private home/bounded scratch.
No worker-created FIFO/socket/device. Kill/reap the owned process tree before export. Export
counts bytes/nodes/names and rejects escaping links, special/unsafe nodes without blocking.
Quarantine records bind exact blobs and digests; no automatic write to source. Real selection,
trusted capture, `B1QuarantineReceiptV1` publication and its `PENDING` state remain unfinished.

## Evidence and remaining gates

Pair denials with the same successful unconfined probe wherever feasible: leaked fd, real
listener/socket, forbidden host canary, hardlink overwrite, dangerous syscall, detached child.
Absence alone does not prove a boundary. Mutation controls must fail when namespaces/x32
guards are removed. Report skips as NOT_RUN with reasons; acceptance uses
`WYSS_B1_REQUIRE_RUN=1` so unavailable required layers fail instead of silently passing.

Still required: useful real-provider authentication and egress, numeric/cgroup quotas and
resource pressure, real trusted capture/publication, independent review and the explicit
worker/approval trust choice. Per-file/per-process/export limits and bounded scratch are
not a hard whole-tree memory quota. Synthetic refresh/broker probes are not provider evidence.
No effectful automated Native/planner lane or absent-host full-account claim follows from B1.
