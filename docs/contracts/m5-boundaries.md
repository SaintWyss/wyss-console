# M5 track (b) — the B1 candidate and the approval boundary: the contract the tests hold

Status: engineering contract for [M5](../product/roadmap.md#m5--boundaries-b1-feasibility-and-concurrency); decisions in
[ADR 0058](../adr/0058-b1-candidate-launcher-and-approval-boundary.md) (proposed). Lanes stay distinct:
**U** unrestricted Native (the user's own rights), **B1** restricted staged worker, **M** Managed, **P** planner (untrusted
proposals), **A** approval authority. B1 is not U with a lock on it, and nothing here makes A stronger.

## 1. What a B1 worker can and cannot do (each row is a test in `tests/b1_process.rs`; § 4 says which have an unconfined control)

| Surface                                           | The worker sees                                                                                                                                                                     | Observed as                                                                               |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| privilege                                         | no capability, `NoNewPrivs`, a seccomp filter; the shim reported it before the worker ran                                                                                           | `CapEff 0`, `CapBnd 0`, `NoNewPrivs 1`, `Seccomp 2`, receipt                              |
| a required layer missing                          | the worker is not started                                                                                                                                                           | exit 4, no output file, no "confined" report                                              |
| inherited descriptors (CG-07)                     | none above stderr                                                                                                                                                                   | `EBADF` on a leaked fd; the same fd readable without the shim                             |
| network (CG-16 synthetic)                         | no interface; `AF_UNIX` is the only socket family (`EPERM` for inet, inet6, netlink, packet)                                                                                        | loopback listener reachable unconfined, not from B1                                       |
| authority / approval socket                       | not in its mount namespace (`ENOENT`); not the runtime directory either                                                                                                             | a same-account client connects to a private authority; B1 cannot                          |
| abstract and pathname host sockets                | unreachable (own network namespace, Landlock scope)                                                                                                                                 | connectable unconfined, denied from B1                                                    |
| host files, home, runtime, container paths        | absent; the account database is not bound                                                                                                                                           | canary readable unconfined, `ENOENT` from B1; `/home`, `/run`, docker socket not present  |
| environment and other processes                   | none of the host's variables; its own pid namespace (`kill` → `ESRCH`, `/proc/<host pid>` absent)                                                                                   | env canary absent, `/proc/1/environ` empty of it                                          |
| kernel entries                                    | `ptrace`, `mount`, `unshare`, `setns`, `bpf`, `io_uring`, `keyctl`, `process_vm_*`, `clone(CLONE_NEW*)`, `mknod*` and **any x32 call number** end the process; `clone3` is `ENOSYS` | exit by `SIGSYS` (reported 128+31); unconfined, the kernel answers instead                |
| writes                                            | workspace, scratch (`/tmp`, memory-bounded), home; nothing else                                                                                                                     | `EACCES`/`EROFS` elsewhere; cross-tree rename and hardlink out denied                     |
| execution                                         | runtime and workspace; **not** scratch                                                                                                                                              | a script in `/tmp` fails, the same in `/work` runs                                        |
| sockets/FIFOs/devices                             | cannot create any                                                                                                                                                                   | `bind` on a path `EACCES`; `mkfifo`/`mknod` killed                                        |
| FX-H01 hardlink to a forbidden signer (CG-01..03) | a distinct inode with the selected bytes; unselected paths absent                                                                                                                   | in-place overwrite leaves the signer intact; through the alias, unconfined, it changes it |
| process tree (CG-10)                              | a detached grandchild dies with the sandbox                                                                                                                                         | it writes unconfined, never inside                                                        |
| hostile output (CG-05/09)                         | absolute or escaping symlinks, FIFOs, sockets in the tree                                                                                                                           | `EXPORT_NODE_UNSAFE`, no block, the signer unchanged                                      |
| useful work (CG-12)                               | edit, create, move, delete, `chmod`, run a script, `git init/add/commit`                                                                                                            | exact add/modify/delete record, `.git` counted and not read                               |
| credentials (CG-15/16 synthetic)                  | a private home with a synthetic token refreshed by write-and-rename; inference only through a broker socket that holds the credential                                               | the reply and the worker's view never contain the broker's secret                         |

## 2. The approval boundary

`b1::boundary` is the contract; its facts are checked against the real control server. Today's channel is a same-user Unix
socket guarded by `SO_PEERCRED` (`control::server::peer`): **any process of the account passes** (observed: a second client of
the same account connects to the private authority). That is the accepted U trust model and stays. B1's addition is that a worker
confined as above has **no route** to it. A _strong_ approval needs every one of: not reachable by the same account, not reachable
from a confined worker, a distinct approver principal, proven human presence, and an independently reviewed OS boundary. None of the
last three exists, so `STRONGER_APPROVAL_MODE`, `NATIVE_AUTOMATED_STEPS`, `B1_PRODUCTION_LAUNCH` and `PLANNER_EFFECT_AUTHORITY`
are `false` constants guarded by tests; planner and Native output are `Untrusted<T>` and leave only through a validator that can
refuse. A source-scan test fails if the control protocol, the desktop, the F8 authority, the Claude launch, the terminal host or the
control server names the B1 module.

## 3. Facts of this machine that the design must not hide (read, never used)

- The account is a member of the **`docker` group** (the group exists; no daemon socket is present now). A member of that group can
  obtain root through a container daemon: **rootful delegation is a fact here** (`BLOCKED_ADMIN_DELEGATION`), and no B1 control
  changes it. B1 protects the _worker_ from the host's secrets and the host from the _worker_; it does not protect anything from a
  process that is already outside it with that group.
- Same-account U/Native processes can use the authority socket, ptrace (Yama scope 1 limits it to descendants), read the account's
  files and credentials: unchanged, and the reason B1 exists as a separate lane.

## 4. Evidence rules that held

- **Controls.** An unconfined run of the same probe shows the attack working for: leaked descriptors, a loopback TCP
  listener, the authority socket, abstract and pathname host sockets, the signer file, signalling a host process, every
  killed system call (answered by the kernel, not by `SIGSYS`; `clone` with a new-user flag excepted), `mknod`/`mkfifo`,
  the FX-H01 hardlink overwrite, a detached grandchild; `/home` and `/run` exist unconfined. **No control** (plain
  absence or a right of a tree): the per-family `socket`/`socketpair` refusals, the environment, the write and exec
  rights, the exporter's refusals, the rename/hardlink out of the tree (an `EXDEV` or a Landlock `REFER` denial is
  not told apart).
- **Mutations.** Removing the pid and network namespaces made four tests fail; breaking the x32 guard made the seccomp test
  fail with an x32 `socket(AF_INET)` that succeeded inside the sandbox.
- **Skips are visible.** A check the machine cannot run prints `NOT_RUN: <why>`, appends it to `b1-not-run.txt` in the
  cargo temporary directory and passes; with `WYSS_B1_REQUIRE_RUN=1` (the acceptance run) it fails. A capability
  check with a bare `bwrap` separates "this machine cannot" from "the candidate is broken": once the machine can, a launch
  that does not work fails the test. (The first run of the suite on this machine skipped everything silently until
  `--nocapture` showed it; `--disable-userns` needs an explicit `--unshare-user`.)

## 5. Not done

CG-14 numeric budgets and quotas (no privilege to mount a size-limited tmpfs; the limits are per-file, per-process, the node and
name budgets of the exporter, and the plan's memory-bounded `/tmp`); the `B1QuarantineReceiptV1` publication and the `PENDING` state; owner selection and trusted capture from a
real repository (the capture fixture stays test-only); real provider authentication or egress (CG-15/16 with a provider); a
reviewed implementation: see the acceptance record for the one independent review this milestone had and what it found.
