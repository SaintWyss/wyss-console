# 44 — Disposable trust-boundary and filesystem fixtures: observed outcomes

**Date:** 2026-10-14. **Class:** synthetic security verification on a container,
**NOT** Wyss application verification, production security acceptance, a
new ADR, or permission to implement.

The owner authorised bounded verification in isolated, disposable
environments. This note records what was actually executed through
the test container, not extrapolations about running
Claude Code/Codex/Hermes or the owner's computers.

Context:
- [43](43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md) —
  architectural A (protected authority) versus B (restricted workers)
- [42](42-POST-REJECT-CORRECTIVE-PLAN.md) —
  unresolved same-UID, approvals, Hermes, LP4, filesystem blockers
- [Codex REJECT](reviews/2026-10-14-post-reject-corrections-external-audit.md)
- accepted [ADR 0011](../adr/0011-external-control-authority-and-durable-state.md)
  and [ADR 0018](../adr/0018-core-hardening-and-completion.md)

## 1. Fixture/environment details and rollback

| Attribute | Actual setup |
| --- | --- |
| Kernel | Linux 6.18.44, isolated tool container |
| Interpreter | Python 3.13.5; cryptography 46.0.4 for mock Ed25519 signatures |
| Authority | Disposable Python AF_UNIX mock, real OS UID **1** (`daemon`); not wyss-control |
| Owner/native attacker | Disposable Python client, real OS UID **65534** (`nobody`) |
| Trusted signer | Another disposable Python signer, real OS UID **33** (`www-data`) |
| Setup controller | Root **inside the isolated container only**, to set ownership/run test identities; not the owner's desktop |
| Temporary data | Python `TemporaryDirectory` in `/tmp`, one random Ed25519 mock key, dummy authority state/binary/unit, dummy provider auth with literal `NO_REAL_SECRET` |
| Security-sensitive real data | **NONE**. No provider login, home, keyring, source-code install, actual approval socket, application binary, network listener or remote device |
| Cleanup | The test runner closed its mock server, removed the entire temporary directory and then deleted its own throwaway fixture script/output |
| Permissions | Authority/signer/provider home directories `0700`, mock private files `0600`; mock proposal/approval socket `0666` in a separately controlled `0755` traversable directory |
| Scope | One unprivileged same-user adversary versus genuinely distinct protected UIDs. The writable public socket was **intentionally open to everyone** so cryptographic verification, not connection denial, decides approvals |

The runner used `subprocess.Popen`/`subprocess.run` with explicit
`setgroups([])`, `setgid(uid)`, `setuid(uid)` before executing
each mock component. It did not call the actual application.

## 2. Synthetic protected-authority approval contract

The deliberately minimal server accepted two JSON request types:
- `proposal`: always responds `PENDING_NO_EFFECT`; never writes state
  or signs an approval.
- `approve`: checks a detached **Ed25519** signature over a
  sorted canonical JSON payload containing:
  `intent_id`, `action=write_dummy`,
  `project=test-project`, `plan_digest=fixture-plan-v1`,
  `attempt=step-1-attempt-1`, and a future `expires_at`.
  The public verification key is in the authority UID's private
  directory. The private signing key is in a **different** UID's
  private directory. Valid signed requests insert one dummy
  effect marker and intent ID into a file owned by the authority
  UID. The server reads its seen-intent list on restart.
  Signing was invoked by the fixture controller as an
  **artificial out-of-band signer**, not a real human,
  Android authenticator or desktop UI.

The untrusted client had a valid network (AF_UNIX) path to
the server but **no right to read the signer key, change the
trusted executable or edit the authority state**.

## 3. Actual synthetic test results

| Test | Expected | Observation | Narrow verdict |
| --- | --- | --- | --- |
| FT-01 untrusted `proposal` | No effect | `PENDING_NO_EFFECT` | PASS |
| FT-02 approved exact payload from trusted mock signer | Single dummy update | `ACCEPTED`, `effects=1` | PASS |
| FT-03 identical signed intent again | Reject repeat | `DUPLICATE_REFUSED` | PASS |
| FT-04 attacker-submitted forged signature | Reject before effect | `DENIED` | PASS |
| FT-05 modified signed action (`delete_any_file`) with original signature | Reject altered canonical bytes | `DENIED`, invalid signature | PASS |
| FT-06 valid signature on expired action | Reject expired | `DENIED` | PASS |
| FT-07 valid signature for wrong project | Reject scope mismatch | `DENIED` | PASS |
| FT-08 different UID tries to read/write private state, binary, unit and signer key | OS permission refused | All eight read/write attempts returned `EACCES` (errno 13) | PASS |
| FT-09 mock authority restarted; resend valid intent | No second dummy effect | `DUPLICATE_REFUSED`, dummy `effects=1` | PASS |
| FT-10 same UID reads dummy provider auth | Works | `READ_OK NO_REAL_SECRET` | PASS |
| FT-11 different UID reads owner-private dummy provider auth | Refused | `READ_DENIED 13` | PASS |
| FT-12 rootless user namespace under original host UID | Does **not** restrict same user's files | `unshare -Ur` printed UID `0` inside namespace **and read `NO_REAL_SECRET`** from host-UID-owned file | PASS (negative security proof) |

### Important exclusions from those PASS verdicts

- The fixture's mock signer was trusted **because the test controller
  chose to invoke it**. The fixture **did not** prove human
  presence, a tamper-resistant display, per-use biometric verification,
  attestation, a secure handset, key rotation, or a non-coercible
  approval decision. A public `sign arbitrary payload` endpoint
  would nullify the entire model; none was exposed here.
- The mock approval only altered one private JSON marker. Its
  durable seen-intent record **does not** prove exactly-once
  for a real external command or managed tool effect.
  If the real effect and ledger commit cannot be atomic,
  a crash may remain ambiguous: no automatic replay.
- Ordinary OS UID separation protects only from the simulated
  *unprivileged* different-UID attacker. It does not protect
  from root, capabilities, a compromised privileged process,
  mutable shared executable dependencies, credential leakage
  or an incorrect real service installation.
- The test mock's protected binary and unit were fake files.
  No `systemd --system` service, polkit policy,
  signed installation, actual Wyss control DB/lock/witness,
  runtime namespace or real user session was created or tested.
- The rootless namespace observation shows that UID `0`
  *inside* a user namespace does not automatically confer
  a distinct original-host security principal. A working
  B sandbox needs actual OS access rules, not just a
  remapped displayed UID.

## 4. Disposable filesystem path-swap comparison

**Motivation:** Original Wyss at pinned revision
`SaintWyss/wyss@0761dcdacdafa216c8afc25036e24685855d2228`
uses source code in `src/execution/filesystem.py:129–178`
that checks a resolved path, then
`src/execution/filesystem.py:285–297` that writes
with `resolved_path.write_text/write_bytes`.
The separate check/open can be susceptible to a
path-component modification by a concurrent writer.
The following is **only a Python analogue**, not a proof
of successful exploitation of the original Wyss runtime.

Two new disposable directories were made:
`work/src` (allowed) and `private` (denied destination),
with a dummy file name `policy.txt`.

| Test | Procedure | Actual outcome |
| --- | --- | --- |
| FT-13 path precheck then path-based write | Resolve/approve `work/src/policy.txt`; rename `src` aside; replace original `src` with a symlink to `private`; perform a `Path.write_text` on the formerly validated pathname | **REDIRECTED**: `private/policy.txt` contains dummy `MOCK_UNSAFE_WRITE` |
| FT-14 anchored directory descriptor | Open the verified allowed directory with `O_DIRECTORY|O_NOFOLLOW`; rename directory; install the same symlink; create relative filename via `os.open(dir_fd=validated_fd, O_CREAT|O_EXCL|O_NOFOLLOW)` | **NOT_REDIRECTED**: dummy file created in moved `src-saved`; `private/anchored.txt` does not exist |

These demonstrate a **class** of validation/use race and a
narrow descriptor-based mitigation. They do not prove that
the complete original adapter is exploitable under its
full guard, concurrent orchestration or exact permissions,
nor that a single `O_NOFOLLOW` check is sufficient for
arbitrary ancestor directory traversal, mount races,
rollbacks, backups or indirect effects. A production design
requires descriptor-anchored resolution throughout the
permitted chain and a documented transaction/provenance
model, plus isolated adversarial tests.

Useful Linux interfaces to investigate:
`openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)` or
fully descriptor-anchored directory walks; test on
supported reference kernels and filesystem topology.
Do **not** infer DN-31 snapshots/sync safety from
the success of a descriptor-anchored name creation.

## 5. What these tests actually narrow

1. A physically distinct, protected authority UID **can**
   protect a private socket, dummy state, executable and
   approval-verification key from an ordinary different-UID
   client. That is a useful substrate for **S1/S2** in
   [43](43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md),
   **not** a full security architecture.
2. A separately protected mock signer can authorise
   *specific immutable intent bytes* without giving the
   requester permission to sign. But the **real human
   approval channel** and trusted verifier ownership
   are still unproven.
3. A separate automated worker UID **cannot read**
   the owner's default private mock provider credential;
   mere UID separation risks breaking provider sign-in/
   refresh. A rootless user namespace retaining the
   owner's host UID does **not** solve this security
   separation. Exact Claude/Codex/Hermes support is UNKNOWN.
4. The use of `Path.resolve` and a later pathname
   write is not proof of race-free mutations. Our
   analogue showed a redirect; do not claim an
   actual Wyss file write was exploited.
5. Full native/LP4 account power still allows any
   effects the Linux account can carry out, even if
   protected managed approvals are separately secured.
   Native worktree contamination, device compromise,
   keyring, process/resource isolation and Hermes
   transitive tool effects remain open.

## 6. Design gate for the next test phase

**Do not touch real provider auth.** Before building any
real authority, a new proposed design must specify:

- Who installs/owns the trusted verifier and signer, who
  can alter its binary/unit/DB/witness, and how the owner
  recovers it without trusting compromised same-user files.
- Who actually obtains the **human's** consent and how
  the signing operation binds exactly what was displayed
  to host, model, project, step/attempt, effect, expiry,
  challenge nonce, and signer identity.
- How the authority supervises or obtains verified
  observations of Codex managed runtime without receiving
  access to unrelated user credentials or weakening the
  accepted ADR 0018 token-refresh bound.
- For *B*, a per-provider capability matrix verifying
  auth refresh, profiles, hooks, MCP/subagents,
  project and writable paths, network, PTY and keyring
  in disposable mock environments before real sign-ins.
- Safe file effect mechanism with TOCTOU control and
  caller-accessible symlinks/hardlinks, with transactional
  stop/rollback states. This does not replace DN-31.
- Precise final-product handling of LP4-away with honest
  account compromise risk. Separate permission to execute
  arbitrary host commands from trust to approve
  managed effects.

**Current state:** two small disposable trust-model
fixtures and one filesystem analogue are complete.
No Wyss code was run, no security ADR accepted, no
independent review changed from REJECT, and no
authority implementation has begun.

## 7. Kernel `openat2` anchored path-resolution fixture (2026-10-14)

**Executed in the same throwaway tool container** with Linux
kernel 6.18.44. Using Python `ctypes` to call the x86-64
`openat2` syscall, a real directory descriptor rooted
all operations in a new temporary `allowed/` directory.
`open_how` used
`O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC`,
mode `0600`, and
`RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS`.
A separate `elsewhere/` directory was the canary.
This did not execute original Wyss or modify owner files.

| Test | Submitted relative path | Observed | Meaning (fixture only) |
| --- | --- | --- | --- |
| FT-15 valid nested file | `nested/ok.txt` | **CREATED** | Allowed open still usable |
| FT-16 parent escape | `../elsewhere/bad.txt` | **REFUSED EXDEV** (errno 18) | Escape cannot resolve |
| FT-17 symlink component to outside | `redir/escape.txt` | **REFUSED ELOOP** (errno 40) | Symlink ancestor rejected |
| FT-18 absolute pathname | `/tmp/escape.txt` | **REFUSED EXDEV** (errno 18) | Absolute path cannot bypass root |
| FT-19 canary outside | inspect `elsewhere/` | **UNCHANGED** | None of refused writes reached canary |

All fixture files were automatically removed. The Linux
`openat2(2)` manual describes these resolution restrictions
and `EAGAIN` on races when the kernel cannot ensure
`RESOLVE_BENEATH` safety:
https://man7.org/linux/man-pages/man2/openat2.2.html

**What this does not prove:** an actual file adapter safely
handles user-owned symlinks in *permitted* ancestors,
hard links, shared/mutable mounts, Btrfs snapshots,
concurrent providers with open descriptors, long-lived
file locks, backups, rollbacks and exact loss-free
application of the same changes. Disallowing all symlinks
may conflict with the owner's `~/Data` directory links;
the final per-operation policy must distinguish
allowed stable links from adversarial redirection.
A reference-machine test with a defined versioned
transaction still remains **BLOCKED_FILESYSTEM**.

Systemd corroboration:
https://www.man7.org/linux/man-pages/man5/systemd.exec.5.html
states that normal per-user service managers cannot set
`User=` to an arbitrary different identity. The
different-UID protected-authority design consequently
needs separate deliberately approved system provisioning
or another comparably effective trust-boundary method.
This is a **documented platform constraint**, not a
systemd unit installed or executed in this test.
