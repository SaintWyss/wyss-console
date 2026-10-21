# F8 TASK 07A — trusted grant FD inheritance

Starting commit: `686c5ba525b88ebd739412dda7258528833beca3`.
Scope: the inactive F8 launcher boundary only. Native local-tools remains refused
by `NativeInferenceSpawner`; no TASK 07B adapter work or deployed-state changes.

## Cause and correction

The isolation builder's earlier pre-exec hook cleared CLOEXEC on its owned
credential duplicate. F8 subsequently set CLOEXEC on every FD >= 3, undoing that
preparation. The initial real-exec regression failed with `Reaped(1)` instead of
`Reaped(0)`: the shell's `cat /proc/self/fd/<grant>` could not read the grant.

The trusted isolation module now has a separate, internal F8 preparation path.
It returns `IsolationLaunch`, with private command and owned descriptor fields.
Only that module constructs production values; the unrestricted fixture
constructor is compiled exclusively for tests. This is not a wire DTO and has
no deserialization, raw-number authorization, frontend or provider entry point.
The existing protocol-1 command path retains its original preparation hook.

Ownership chain:

1. Host isolation validates and owns the credential inode (`Credential`).
2. The builder duplicates that open file with CLOEXEC and writes the duplicate's
   number into the fixed bwrap `--bind-fd` argument.
3. `IsolationLaunch` owns both command and duplicate; neither can be independently
   changed by a caller. No pathname reopening occurs during F8 handoff.
4. `PendingOwnedSpawn::spawn_isolated` checks the owned descriptors before
   enqueueing; standard streams and unsealed descriptors are rejected.
5. The command's pre-exec closure owns the duplicates throughout the coordinator
   queue, fork, exec handshake, cancellation and command disposal.
6. The child sets CLOEXEC on all nonstandard descriptors, checks syscall success,
   then clears it only on these owned grants. Failed sweep or failed grant
   preservation returns a pre-exec error; the payload cannot execute.
7. bwrap receives the exact descriptor, mounts its inode and consumes the grant.
   The contained payload accesses the allowed mount, not the original host FD.

The new post-fork work consists solely of the checked close_range syscall,
iteration over an already allocated vector and fcntl calls. It allocates nothing,
takes no locks, logs nothing and invokes no runtime services. Error paths use
OS errors. All ownership preparation and validation happen before enqueueing.
The existing self-pidfd/SCM_RIGHTS gate and coordinator/reaper paths remain in use.
`DeferredProcessWorker` has an internal entry for the prepared launch, with the
same nonblocking polling, capacity retention and bounded cleanup as inference.

## Regression evidence

Nine new tests use harmless fixture processes or reject a malformed fixture
before enqueueing:

- Authorized grant: after a real exec, reads `trusted-grant` through the FD.
- Unapproved descriptor and source substitution: an unrelated descriptor is
  deliberately inheritable in the parent but unreadable after exec. Atomically
  substituting the original source FD with a different inode does not change the
  held grant: the child reads exactly `original-inode`, never `SUBSTITUTED`.
- Closed grant: a fault hook closes only the fork's copy before preservation;
  launch fails, the payload marker is absent and the reserved permit is released
  only after spawn reports the failed child reaped.
- Failed exec and cancellation after pidfd acquisition: delayed coordinator
  handoff retains capacity; terminal pidfd readiness and eventual grant socket
  EOF prove process termination and release of all grant owners.
- Unsealed grant: rejected before enqueueing.
- Earlier arbitrary pre-exec hook: clearing CLOEXEC on a plain Command's FD does
  not authorize it; after exec the child reports `sealed`.
- Failed blanket sweep: a child-only seccomp filter forces real close_range to
  return ENOSYS. A private socket confirms the filter actually denied the syscall;
  the final sweep refuses execution and the payload marker remains absent.
- Cancellation before gate acknowledgement: payload never executes; grant socket
  EOF proves no queued command or child retains the descriptor.
- Trusted builder through real bwrap and DeferredProcessWorker: the original
  credential owner is dropped before spawn; the namespace payload prints only
  `FIXTURE-CREDENTIAL` and `grant-consumed`. All credential material is synthetic
  and resides in the test's temporary directory.

The first two FD assertions are covered by two separate ownership tests; the
list groups their related authorization properties. Existing regressions also
cover child pidfd identity and CLOEXEC, rejection of malformed SCM_RIGHTS,
namespace observation, descendant teardown, bounded reaper capacity, deferred
application cleanup and the native inference-only durable lifecycle.

## Security review and limits

PASS: safe Rust ownership prevents closing/recycling an authorized duplicate
while the command is queued. Replacing or dropping the original source cannot
retarget that duplicate. CLOEXEC remains set in the parent, so other execs do not
receive it. The private prepared command prevents adding hooks or changing grant
arguments after preparation. No protocol or authority policy has been broadened.

PASS: blanket sweep precedes the grant exceptions, and failure of either step
refuses exec. Identity transfer still obtains the pidfd in the child itself;
cleanup never acquires ownership from a reusable PID. No new wait, lock or writer
operation was added. Runtime and reaper permits follow the existing lifecycle.

NOT_PROVEN: exhaustive kernel/resource-exhaustion schedules, uninterruptible
kernel tasks and malicious in-process unsafe Rust that closes or overwrites
another value's owned FD cannot be proved by these tests. The existing stuck
cleanup semantics retain ownership/capacity rather than claim reap. Unsupported
close_range kernels now fail closed; successful launch requires Linux support.
The complete authenticated native local-tools lifecycle and approval translation
remain outside TASK 07A and have not been exercised. No installed services,
private session bus, real credentials or authenticated Codex sessions were used.

## Validation (2026-10-21)

All commands use the repository's canonical wrapper:

```sh
node scripts/tool.mjs cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
node scripts/tool.mjs cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib f8::ownership::tests -- --nocapture
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib f8_owned_grant_crosses_exec -- --nocapture
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib f8::
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --bins --test authority_boundary --test f8_contracts
```

PASS: format and clippy, 27 ownership regressions, real bwrap grant regression,
256 F8 tests, 894 library tests (7 existing benchmarks ignored), 361 binary tests
(12 existing installed-machine/service/credential tests ignored), 1 authority
boundary test and 10 F8 contract tests. No tests were removed or weakened.
The previous 885 passing library / 248 F8 baseline increases by 9 / 8 respectively.
