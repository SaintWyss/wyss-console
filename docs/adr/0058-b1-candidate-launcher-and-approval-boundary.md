# ADR 0058 — A constrained B1 candidate, and the approval boundary it does not change

Status: **Proposed; not accepted** · Date: 2026-10-24 · Owner: Santiago

Builds on [49](../architecture/49-CACHYOS-COMPOSED-WORKER-SECURITY-CONTRACT.md)–[52](../architecture/52-B1-TYPED-PROTOCOL-AND-FAIL-CLOSED-LIFECYCLE.md)
and the pure contracts of `contracts/b1/`. Nothing here is reachable from the control protocol, the desktop, a provider
launch or any packaged binary. **B1 containment and FX-H01 stay NOT_PROVEN**; the owner policy OD02 is untouched.

## Context

B1 was a paper contract plus pure types and a test-only capture fixture. Milestone M5 asks for a constrained candidate
with safe primitives, positive workflow evidence and a negative matrix, an approval-boundary contract that keeps
untrusted Native/planner output from forging approval, and an independent security review, without a new privileged
service, a system permission change, or an opened production gate.

## Decision (proposed)

1. **A rootless launch plan** (`b1::plan`): `bwrap` (the root-owned `/usr/bin/bwrap`, started with no environment)
   with user, IPC, pid, network and UTS namespaces, `--disable-userns`, all capabilities dropped, a new session,
   `--die-with-parent`, a private memory-bounded `/tmp`, the read-only system runtime, and only what the plan names:
   the task workspace (`/work`), an optional private home (`/home/b1`, synthetic credentials) and an optional broker
   directory (`/broker`, one socket). No host home, runtime directory, bus, display, keyring, container socket or
   authority socket; no account database. The plan validates every directory (a real private directory of the user, not
   a link, not the root, the home or any place that contains it, not nested in another) **on the open descriptor** and
   binds those descriptors (`--bind-fd`), so a path swapped after the check changes nothing; a plan made for a
   materialized workspace is also bound to that directory's device and inode. It **copies the shim and probe from
   verified descriptors** (`screen::locate`, as for the parser helper): what runs is what was checked.
2. **A trusted pre-exec shim** (`b1::confine`, `wyss-b1-shim`): inside the new namespaces and before any worker
   instruction it closes every descriptor above stderr except one report pipe, sets limits, sets no-new-privs, installs a
   seccomp filter, reads the kernel's own account of the process (`CapEff`, `NoNewPrivs`, `Seccomp`), applies Landlock
   last, writes `{layers, landlock_abi, inherited_fds_closed}` to the pipe and only then executes the worker. A layer the
   launcher requires and the kernel did not give ends in exit 4 and **the worker is never started**: there is no
   degraded mode and no wider-grant retry.
3. **Landlock** handles every filesystem right known up to ABI 5 and grants by tree: the runtime read+execute; the workspace read,
   write, create files/directories/symlinks, remove, rename within the tree (`REFER`), truncate, execute; scratch and home
   the same without execute; devices read/write/ioctl; `/proc` and `/etc` read-only. **Never granted anywhere**: creating
   sockets, FIFOs or device nodes. TCP bind/connect handled and not granted (ABI ≥ 4); abstract unix sockets and signals
   outside the domain scoped. **ABI 6 is the floor the shim enforces**, whatever the launcher asks (`MIN_LANDLOCK_ABI`;
   a profile can raise it, never lower it): below it the worker is not started. Rights later ABIs add (pathname unix
   sockets at ABI 9) are not known to the shim and not covered.
4. **Seccomp** (deny-list, forward-only BPF built from labelled blocks, tested for well-formedness): the ways out end the
   process (`ptrace`, `process_vm_*`, `mount` and the new-mount API, `unshare`, `setns`, `bpf`, `io_uring_*`, `keyctl`,
   `userfaultfd`, `perf_event_open`, module and reboot calls, `mknod*`); **a call number with the x32 bit set ends the
   process** (the architecture check passes it and every native comparison would miss it; on this machine an x32
   `socket(AF_INET)` succeeded when the guard was removed); `clone` with any `CLONE_NEW*` flag ends the
   process; `clone3` answers `ENOSYS` (its flags cannot be inspected; libc falls back to `clone`); `socket` and
   `socketpair` answer `EPERM` for every family but `AF_UNIX`. Everything a compiler, git or a shell needs stays allowed.
5. **A task workspace from an admitted snapshot** (`b1::workspace`): fresh private directory, files created `O_EXCL |
O_NOFOLLOW` through directory descriptors, bytes copied (never linked), every file re-checked for one name and its
   size; only the entries the owner selected exist, so a selected file that was a hardlink to something forbidden
   becomes a distinct inode.
6. **An exporter** after the worker is gone: no path followed, nothing opened that could block (a FIFO, socket or device
   is `EXPORT_NODE_UNSAFE`), symlinks read as inert data under the same containment rule as on the way in, sizes and
   counts bounded (every node, directory or not, and the bytes of names, counted while a directory is listed, before
   anything is read), the worker's `.git` counted and never opened, and a change set (add/modify/delete) as
   `B1ExportRecordV1` plus blobs that are **exactly** the contents the record names, once each, checked against their
   digests and sizes before anything is written. **A quarantine store of its own**: a private directory reached through
   descriptors that follow no link, apart from the workspace and the source, created once per task and generation (the
   directory creation is the compare-and-swap), the record written last and put in place by a rename; never the source
   repository. Publication of `B1QuarantineReceiptV1` and the `PENDING` transition are **not** implemented.
7. **An approval-boundary contract** (`b1::boundary`): a channel is _strong_ only if no same-user process can open it, no
   confined worker can reach it, its approver is a distinct principal, human presence is proven on that side, and the OS
   boundary that makes this true passed an independent review. The channel that exists (same-user socket, `SO_PEERCRED`)
   is stated as weak for the right reasons. Four gates are constants that are `false` and have a test that fails if wired:
   a stronger approval mode, automated Native steps, a production B1 launch, planner effect authority. Planner and Native
   output is `Untrusted<T>` data that leaves only through a validator that can refuse.
8. **Fixture-only.** The module is compiled only with the `b1-fixture` feature, the shim and probe likewise; the guard is
   that no build the packager runs enables the feature (`tests/package.test.ts`), with the package check's marker as
   defence in depth. A source-scan test asserts that the control protocol, the desktop, the F8 authority, the Claude
   launch, the terminal host and the control server do not name the module; it checks those surfaces, not every route a
   future change could add. The four gates and `Untrusted<T>` are statements and regression checks, not enforcement.

## What it proves and what it does not

Observed from inside by `tests/b1_process.rs` (22 tests, `WYSS_B1_REQUIRE_RUN=1`, none skipped), with an unconfined
**positive control wherever the attack can be run unconfined**: descriptors left open by the launcher (readable
without the shim, `EBADF` with it); TCP to a local listener; the authority socket (a same-account client connects;
the worker gets `ENOENT`); abstract and pathname host sockets; the forbidden-signer file; signalling a host
process; every killed system call (unconfined it is answered by the kernel, not by `SIGSYS`; `clone` with a new-user
flag has no control because unconfined it forks the probe); `mknod`/`mkfifo`; the FX-H01 hardlink overwrite; a detached
grandchild that keeps writing. Checks that are plain absence or presence have no control and say so: the per-family
`socket`/`socketpair` refusals, the paths under `/home`, `/run` and the container sockets (their existence is
checked unconfined for `/home` and `/run`), the environment, the write/exec rights of each tree, the
`interrupt`-style negative cases of the exporter. Also observed: no capability, no-new-privs, filter installed;
the dangerous system calls killed; a useful edit + `git` commit + file moves ending as typed records in a
quarantine; hostile output refused; a token refreshed in place by write-and-rename in a private home; inference
through a broker that holds the credential the worker never sees. Mutation checks: removing the pid and network
namespaces from the plan made four tests fail; breaking the x32 guard made the seccomp test fail with an x32
`socket(AF_INET)` that **succeeded** inside the sandbox.

Not claimed: a kernel exploit from inside the domain; protection of pathname unix sockets by Landlock (ABI 9; the mount
namespace and the `AF_UNIX`-only filter are what this relies on); anything against a process **outside** the domain
(a same-user attacker, a member of the `docker` group, root); human approval authenticity (the stronger mode stays
closed); real provider authentication or egress (CG-15/16 are synthetic here); tmpfs quotas, cgroup limits and a
measured CG-14 budget (no privilege to mount them; per-file and total-byte limits, descriptor and file-size ceilings
only); a reviewed implementation (an independent review of this diff is recorded in the acceptance record).

## Consequences and open decisions

- **Owner (OD02):** accept or amend this ADR; whether a protected approval principal is worth the privileges it needs
  (a distinct account or service: the user is a member of the `docker` group, so rootful delegation is a fact of this
  machine and B1 does not "fix Docker"); whether B1 may ever launch a real provider (needs CG-15/16 with provider
  evidence and a separate authorization).
- The shim and probe are **not packaged**; a production launcher is a separate change after the review.
- Rollback: delete the module and the two binaries. Nothing in the product references them.
