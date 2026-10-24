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
   a link, not the root, the home or any place that contains it, not nested in another) and **copies the shim and probe
   from verified descriptors** (`screen::locate`, as for the parser helper): what runs is what was checked.
2. **A trusted pre-exec shim** (`b1::confine`, `wyss-b1-shim`): inside the new namespaces and before any worker
   instruction it closes every descriptor above stderr except one report pipe, sets limits, sets no-new-privs, installs a
   seccomp filter, reads the kernel's own account of the process (`CapEff`, `NoNewPrivs`, `Seccomp`), applies Landlock
   last, writes `{layers, landlock_abi, inherited_fds_closed}` to the pipe and only then executes the worker. A layer the
   launcher requires and the kernel did not give ends in exit 4 and **the worker is never started**: there is no
   degraded mode and no wider-grant retry.
3. **Landlock** handles every filesystem right of the ABI and grants by tree: the runtime read+execute; the workspace read,
   write, create files/directories/symlinks, remove, rename within the tree (`REFER`), truncate, execute; scratch and home
   the same without execute; devices read/write/ioctl; `/proc` and `/etc` read-only. **Never granted anywhere**: creating
   sockets, FIFOs or device nodes. TCP bind/connect handled and not granted (ABI ≥ 4); abstract unix sockets and signals
   outside the domain scoped (ABI ≥ 6, required).
4. **Seccomp** (deny-list, forward-only BPF built from labelled blocks, tested for well-formedness): the ways out end the
   process (`ptrace`, `process_vm_*`, `mount` and the new-mount API, `unshare`, `setns`, `bpf`, `io_uring_*`, `keyctl`,
   `userfaultfd`, `perf_event_open`, module and reboot calls, `mknod*`); `clone` with any `CLONE_NEW*` flag ends the
   process; `clone3` answers `ENOSYS` (its flags cannot be inspected; libc falls back to `clone`); `socket` and
   `socketpair` answer `EPERM` for every family but `AF_UNIX`. Everything a compiler, git or a shell needs stays allowed.
5. **A task workspace from an admitted snapshot** (`b1::workspace`): fresh private directory, files created `O_EXCL |
O_NOFOLLOW` through directory descriptors, bytes copied (never linked), every file re-checked for one name and its
   size; only the entries the owner selected exist, so a selected file that was a hardlink to something forbidden
   becomes a distinct inode.
6. **An exporter** after the worker is gone: no path followed, nothing opened that could block (a FIFO, socket or device
   is `EXPORT_NODE_UNSAFE`), symlinks read as inert data under the same containment rule as on the way in, sizes and
   counts bounded, the worker's `.git` counted and never opened, and a change set (add/modify/delete) as
   `B1ExportRecordV1` plus blobs checked against their digests. **A quarantine store of its own**, created once per task
   and generation (the directory creation is the compare-and-swap), the record written last; never the source
   repository. Publication of `B1QuarantineReceiptV1` and the `PENDING` transition are **not** implemented.
7. **An approval-boundary contract** (`b1::boundary`): a channel is _strong_ only if no same-user process can open it, no
   confined worker can reach it, its approver is a distinct principal, human presence is proven on that side, and the OS
   boundary that makes this true passed an independent review. The channel that exists (same-user socket, `SO_PEERCRED`)
   is stated as weak for the right reasons. Four gates are constants that are `false` and have a test that fails if wired:
   a stronger approval mode, automated Native steps, a production B1 launch, planner effect authority. Planner and Native
   output is `Untrusted<T>` data that leaves only through a validator that can refuse.
8. **Fixture-only.** The module is compiled only with the `b1-fixture` feature, the shim and probe likewise, and the
   package check refuses their marker. A source-scan test asserts that the control protocol, the desktop, the F8
   authority, the Claude launch, the terminal host and the control server do not name the module.

## What it proves and what it does not

Observed from inside, each against an unconfined baseline where the attack works (`tests/b1_process.rs`, 17 tests):
no capability, no-new-privs, filter installed; descriptors left open by the launcher closed; no network and unix-only
sockets; the authority socket reachable by a same-account client and not from the worker (`ENOENT`); abstract and
pathname host sockets, host files, home, runtime and container paths, the environment and other processes' `/proc`
entries absent; the dangerous system calls killed; writes only to workspace, scratch and home, exec only from the
runtime and workspace; the FX-H01 hardlink to a forbidden signer neutralized at materialization; a detached
grandchild dead with its sandbox; a useful edit + `git` commit + file moves ending as typed records in a quarantine;
hostile output refused; a token refreshed in place by write-and-rename in a private home; inference through a broker
that holds the credential the worker never sees. Removing the pid and network namespaces from the plan made four of
those tests fail (mutation check).

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
