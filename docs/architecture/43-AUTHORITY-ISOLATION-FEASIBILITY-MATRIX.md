# 43 — Authority vs native-worker isolation: feasibility and decision matrix

Date: 2026-10-14. **Stage:** owner-authorised bounded isolated verification; document-only change.
**Status:** security research and candidate design, **NOT an ADR, NOT a final architecture**.
No application implementation, real provider launch, host service installation, account login
or private settings read was performed. A tiny synthetic, automatically deleted Linux IPC
fixture was executed in the isolated tool container under uid 65534. This document does
not assert that Wyss/Claude Code/Codex/Hermes themselves have been exploited.

Review context: [Codex REJECT](reviews/2026-10-14-post-reject-corrections-external-audit.md)
(ER-01/02/03/04/05), [Claude REJECT, separate branch](https://github.com/SaintWyss/wyss-console/blob/santiagosw/tender-albattani-udd3ir/docs/architecture/reviews/2026-10-14-external-audit-post-reject-corrections.md),
[42](42-POST-REJECT-CORRECTIVE-PLAN.md), accepted
[ADR 0011](../adr/0011-external-control-authority-and-durable-state.md),
[ADR 0018](../adr/0018-core-hardening-and-completion.md),
[ADR 0025](../adr/0025-authority-lifecycle-bootstrap.md) and
[ADR 0027](../adr/0027-local-product-no-distribution-packaging.md).

## 1. What is being protected? Separate three different promises

**S1 — Human-approval origin integrity:** a malicious process operating as the
ordinary Linux user must not be able to impersonate the *human's* approval of a
managed tool or a pipeline. Today, `SO_PEERCRED` only attests a UNIX identity.
Accepted ADR 0018 says same-UID processes can connect, grant leases and
approve managed actions and modify user-owned authority storage.

**S2 — Authority service integrity:** the attacker must not alter the
trusted authority's binary, unit file, database/witness/audit, policy,
credential verifier, IPC, launch environment, or signature/key material
used to decide S1. Socket mode `0600` is irrelevant to an attacker
with the *same* UID. Even S1's separate cryptographic approval is
meaningless if the same attacker can replace its verifier or rewrite
the canonical approved state.

**S3 — User-project/account integrity:** an unrestricted native agent
running as the human Linux user can read/change user-owned projects,
configs, other agents' homes, user services and eligible credentials,
subject to actual OS access controls. **Protecting S1/S2 does not make
S3 safe.** This is intrinsic to full-account `terminal.write`
and unconfined native CLI/TUI use. Do not conflate "managed authority
cannot be forged" with "native agent cannot change my computer".
A user may knowingly choose full-account operations; the product
must label that risk and cannot claim they are mediated effects.

The owner wants a full personal product: preserve genuine native
Claude Code/Codex/Hermes operation, finite OL3, and remote control
while away. No proposal below silently removes these features.
The source-level review is **not** a standalone reason to approve an
unattended native OL3 step.

## 2. Synthetic evidence: same-UID process test in disposable environment

**Exact scope:** isolated Linux container, unprivileged uid 65534
(`nobody`), fresh private `tempfile.TemporaryDirectory`
owned by that UID, one AF_UNIX stream socket `0600` and one
dummy text file `0600`. An independent Python child running
under uid 65534 connected to the server, sent the literal
test text `synthetic-intent` and read `DISPOSABLE_ONLY`
from the test file. The server inspected `SO_PEERCRED`.

| Assertion | Actual observation | Interpretation |
| --- | --- | --- |
| Server and client same nonroot UID | Both uid 65534 | Under this ordinary discretionary-access design they belong to one security principal |
| Peer has access to socket `0600` | Connection succeeded; server received `synthetic-intent` | Kernel UID check is not human-consent proof |
| Peer reads same-UID file `0600` | Child read `DISPOSABLE_ONLY` | Secret authority DB/state owned by same UID cannot be claimed protected from other same-UID processes |
| Temporary content remains after test | Directory deleted after context exit | Test touched only dummy fixtures; no product runtime, home or secret |

**Important limitations:** the test did not run the actual
`wyss-control` daemon, attempt any real approval RPC, test
other-UID denial or prove exploitation. It is one confirming
fixture for ordinary same-UID DAC semantics; the real trust
claim also follows independently from accepted ADR 0011/0018
and Linux's `unix(7)` peer-credentials contract.

Relevant official documentation:
- https://man7.org/linux/man-pages/man7/unix.7.html (SO_PEERCRED)
- https://man7.org/linux/man-pages/man7/user_namespaces.7.html (UID mappings)
- https://github.com/systemd/systemd/blob/main/man/systemd.exec.xml
  (`User=` in user services, `DynamicUser=`, sandbox and credential contract)

## 3. Comparison of candidate architectures

| Criterion | **A: protect approval/authority under distinct genuine host principal** | **B: restrict automated native workers** | **H: hybrid A + B when genuinely needed** |
| --- | --- | --- | --- |
| S1 human-approval provenance | **Potentially** strong, if host cannot forge signing proof and authority verifies it | Protects if every untrusted worker and input source truly cannot access the issuer/authority; same-UID unrestricted manual/native/LP4 remains | Potentially strongest, with independent review |
| S2 binary, DB, policy, service integrity | Requires different ownership/access protection plus service startup and unmodifiable verifier; a separate PID or user `systemd --user` unit alone is NOT enough | Worker sandbox may hide trusted surfaces from **that worker**, but not an unconfined same-UID native/LP4 process | Requires protected authority regardless of worker sandbox |
| S3 arbitrary changes to user's files | **Not protected** against unrestricted native/LP4 shell by a separate authority | Worker changes limited to assigned workspace/effects if true OS confinement proven; other unrestricted native processes remain | Restricted auto-work + full-account manually labelled |
| Native agent sign-in and refresh | Can keep original provider user/home in unrestricted native sessions; protected approval service must not ingest auth tokens | **Unverified per provider**: separate UID/mount/home may break refresh, keyring, MCP, hooks, project permissions or own subprocesses | Separate native profiles, provider-by-provider evidence |
| Ordinary native terminal equivalence | Preserved for the native program but does **not** make program activity subject to authority | Altered where controls deny capabilities; not 'unrestricted native' | Separate "unrestricted full account" and "restricted OL3 worker" entry points |
| OL3 native auto start | Still unsafe until trusted approval path, native-work handoff policy, mutable worktree/hooks and effect ownership resolved | Potentially safe inside an expressly bounded sandbox if working provider+project experience demonstrated | Target only if both authority and worker gates pass |
| LP4 remote host shell after logout | Possible as an **inherently full-account** action with explicit remote key/presence, bounded claims, protected grant issuer; compromise still changes user-owned state | Can remote into bounded shell instead, but **not equivalent** to selected LP4 full control | May offer both as distinct grants; no accidental escalation |
| Headless logout/lifecycle | Requires service design independent of user's tamperable unit and proper host admin setup/upgrade/recovery | Can operate under unprivileged user service when reference OS enforces sandbox; keyring auth unresolved | Greater IPC/process/resource maintenance |
| Personal installation with no sudo | **Conflicts with the accepted ADR 0025 experience** if a genuinely different protected OS principal/system unit must be created; one-time privileged provisioning / policy delegation is a *future owner trade-off* | Rootless sandbox may be possible but capabilities, UID mapping, kernel policy and provider behaviour need evidence | Usually incurs provisioning and testing costs |
| Security proof currently available | **SOURCE-SUPPORTED PRINCIPLE / UNIMPLEMENTED** | **SOURCE-SUPPORTED POSSIBILITY / UNTESTED PROVIDERS** | **UNTESTED, NOT SELECTED** |
| Final recommendation status | **A first to investigate**, not adopted | Keep as a comparator and possible OL3 profile | Evaluate after A/B constraints are known |

**Fundamental incompatibility under current accepted trust model:**
`same UNIX UID for authority, unrestricted agent and LP4` +
`no external trusted approval protector` + `human approvals are
nonforgeable against that agent` cannot all hold simultaneously.
A GUI popup, typed IPC, claim token readable in the same account,
namespace with the same mapped host UID, or splitting the same-UID
daemon into extra processes does not change that. This is a
consequence of the declared attacker capability, not a new
exploited vulnerability.

`systemd --user` runs under its user manager's identity.
The systemd documentation explicitly says other users' user
managers cannot change their service's `User=` to another UID.
`DynamicUser=` and `LoadCredential=` are not a drop-in
fix for a normal personal user unit; privileged installation,
credential custody, service manager and kernel policy must
be separately considered. UID remapping inside a user
namespace does not automatically prevent access to the
same mapped host user's ordinary files.

## 4. A: protected-authority candidate — minimal feasible trust structure

```text
[Untrusted native Claude/Codex/Hermes (ordinary account)]
   | may freely access ordinary user's files/projects
   | does NOT own the protected approval issuer/verifier/storage
   +-- typed untrusted action request --> [trusted authority]
[Wyss Console same-user UI] -- request --> [trusted authority]
[Native Android/PC owner presence] -- signed exact operation -->
   [trusted authority under separate genuinely protected identity]
[Trusted authority] -- applies validated managed approval --> [managed runtime]
```

**A critical caveat:** an Android hardware key signing a message
does not by itself guarantee the owner saw the true operation:
the app/view/transport must bind an immutable digest of the
**actual human-displayed target**, plan, scope, timing and
effect. A compromised OS/phone app can misrepresent its
display. The protected authority's code, policy and DB
must be trusted against same-UID modification; mutable
launcher scripts, PATH, plugins and inherited FDs are
part of that asset. If the trusted issuer is unavailable
offline, effectful approval must **pause**, not fall back
to UID-based authentication.

A protected approval subsystem under another UID is a
**candidate design** requiring changes to accepted ADR
0011/0018/0025 and probably a privileged **one-time
provisioning** step. This contradicts today's "just launch
Wyss, no sudo" UX if interpreted literally. A privilege
transition must be deliberate, documented and approved;
no root service was installed or requested here.

**Protection boundary of candidate A:** preserve S1/S2
against an ordinary unprivileged compromised same-UID
native process only after hardening and tests; NOT
S3, malware with root, a compromised system service,
a stolen signing device, or tricks that make the owner
approve a malicious exact action.

## 5. B: restricted-worker candidate — full functionality audit matrix

A restricted OL3 worker runs with explicit effect/protection
limits. It cannot be honestly advertised as the owner's
exact normal unrestricted native terminal. Its sandbox
may still run provider models and write into a bounded
workspace, but each capability needs an actual fixture:

| Native feature | A ordinary user native | B restricted OL3 worker | Required evidence |
| --- | --- | --- | --- |
| CLI/TUI startup, resize, PTY | Expected unchanged; **not newly tested** | Unknown: PTY and terminal helper permissions | Exact provider version and isolated PTY integration |
| Existing provider login / refresh | Original user's normal mechanisms | Unknown; auth copy prohibited by DN-14; read-only file bind may break token refresh | Separate no-real-account provider mock + documented official auth configuration |
| Provider global hooks/rules | User-owned, so mutable by ordinary agent | Unknown: could be absent, blocked, altered by overlay; required controls must remain | Per-provider effective config and mandatory-hook tests |
| MCP/plugins/subagents | Native capabilities | Unknown: sockets/exec/net restrictions may break them or introduce bypass | Enumerate and deny escape, then positive useful pipeline |
| Project read/write and Git | Ordinary user permissions | Needs idmapped grants/ACL/overlay/worktree design | Write allowed only intended path; tests with symlinks and worktree hooks |
| Shell commands/test runners | Ordinary shell | Restricted to verified effect vocabulary or reviewed OS sandbox | Positive build/test workload + negative external action |
| Keyring/KWallet/desktop D-Bus | Native session may access | Unknown; isolated UID/namespace often disrupts access | Mock keyring then real per-device compatibility after separate permission |
| Network/model API | Native account network access | Egress whitelist and per-provider binding need proof | Allow legitimate provider traffic; reject unauthorized destinations |
| Performance/resource use | Baseline | Sandbox/FFI overhead, startup and RAM unknown | p50/p95 launch, idle CPU/RAM, throughput on reference machines |

If the restricted persona cannot preserve a mandatory feature,
there is a **genuine owner decision**: use an alternative
trusted-arbiter architecture, explicitly accept the reduced
capability **for OL3 only**, or reject that route.
No forced adoption merely because B can deny sockets.

## 6. Boundaries both A and B must still solve

1. **Native-to-managed project contamination:** a native
   full-account worker can change `.git`, hooks, instructions,
   provider settings or app files that a managed step later
   consumes. Trusted authorities must revalidate exact
   worktree generation/content source; a label `data-only`
   is not immunity to prompt injection or code execution.
2. **Authority's physical consent path:** host GUI under
   same UID is not a trustworthy separate human authenticator.
   Strong cryptographic step-up must identify the reviewed
   operation and keep signing keys inaccessible to the native
   attacker. The signing device, policy and recovery are
   separate trust assumptions.
3. **Remote LP4-away:** native remote shell after screen lock
   or KDE logout is *explicitly required* by the owner's
   usage pattern; default-off is a safe interim state,
   **not final product acceptance**. A separate, narrowly
   consented absent-host entitlement may enable
   full-account remote control, with honest residual
   exposure and no guarantee that revocation undoes
   earlier account modifications.
4. **Managed model and runtime concurrency:** unaffected by
   A/B alone. Accepted ADR 0012/0015 still serialize
   turns and retain one shared App Server. A separate
   authority ADR with capability proofs remains mandatory.
5. **Filesystem and vault:** DN-21/DN-31 still prohibit
   unsafe mutations/snapshotted plaintext. Protected
   approver alone does not guarantee no data loss,
   atomic restoration or safe credential handling.
6. **Process trees/parser:** different UID or PID alone
   does not prove PTY parser failure isolation, memory
   bounds, no escaping descendants or consistent
   recovered state.

## 7. Verification gates: positive and negative tests

Each needs a *named disposable target*, pinned versions,
test data, expected failure/positive path, teardown and
evidence with output hashes before any implementation/ADR
acceptance. They are listed below as **NOT RUN** except
the single synthetic same-UID observation (§2).

| ID | Test target and assertion |
| --- | --- |
| TV-ISO-01 | **OBSERVED synthetic only:** two unprivileged same-UID processes; socket `0600` and file `0600` are accessible to peer; UID check alone does not attest human |
| TV-ISO-02 | **PARTIAL synthetic only:** different-UID attacker cannot open a 0700 private directory, 0600 socket or dummy state file, while same-UID client can. **NOT RUN for real authority binary/unit, DB/WAL/witness, signer, D-Bus, FDs or real privilege changes.** |
| TV-ISO-03 | Unrestricted native **manual**, OL3 and LP4 each try all `approvals.respond`, `grant_step`, `managed.turns`, `managedSession/*` and alternate executable/service paths; fail closed for protected operations |
| TV-ISO-04 | Legitimate approval signed outside compromised user session verifies exact digest/time/scope and executes only permitted managed action; reordered, replayed, substituted and falsely displayed intents refuse |
| TV-ISO-05 | Provision protected UID/unit without modifying existing user configs; test upgrade/rollback, user reboot, logout/lingering, failure recovery and explicit owner consent; conflict with ADR 0025 surfaced |
| TV-ISO-06 | Restricted provider profile for each of Claude Code, Codex and Hermes: login persistence, token refresh, hooks, plugins/MCP, project file edit, git, test command, required egress, PTY work, restart |
| TV-ISO-07 | Worker tries to reach authority by pathname, inherited FD, D-Bus, process signals, writable launcher and service files; not only authority.sock; test mutable PATH/wrapper and provider configs |
| TV-ISO-08 | Native/managed worktree contamination through hooks, instructions and executable source changes; stale prior approvals never authorize effects |
| TV-ISO-09 | LP4-away valid phone/PC performs an explicitly approved remote input while no KDE session exists; stolen/expired key/revoked device denied; account compromise is not labelled restored |
| TV-ISO-10 | Both candidates measured CPU/RAM/latency, process count, profile usability, maintenance complexity; costs and real hardware scope reported not invented |

**Current evidence by category:**

- Linux kernel and systemd semantics: **documented upstream**,
  including UID and user-service constraints.
- Same UID mock socket/file: **observed only in isolated container**.
- Distinct UID synthetic DAC separation: **OBSERVED only in throwaway environment**, see §9; actual Wyss protected authority: **NOT RUN**.
- Restricted provider model compatibility: **NOT RUN**.
- Tauri IPC native host confirmation, Android key proofs
  and remote LP4-away: **NOT RUN**.
- Owner reference devices' exact kernel/security policy:
  **NOT OBSERVED**. No suitability claimed.

## 8. Provisional design judgment and next action

**Prioritize A for further proof**, because it has the best
*potential* to preserve everyday unrestricted native agents
and existing provider authentication, while moving human
approval integrity away from their identity. It also has
important **costs**: a different system trust principal,
one-time provisioning/installation contrary to today's
no-sudo boot contract, new IPC/protection and strong
out-of-band human approval (the user must have a realistic
non-phone fallback or accept it as a hard requirement).

**B remains a serious, but narrower, candidate** for
automated OL3 steps, not a drop-in replacement for
native full-account sessions. A hybrid might be warranted
if approvals are protected separately *and* unrestricted
automatic workers remain too risky.

**Formal stop condition:** if the owner insists at once
on (a) unrestricted native workers in the same full-account
UID, (b) same-UID authority without external trusted
principals/hardware, (c) fully unforgeable human consent
against those workers, and (d) no privileged installation
or capability restriction, the requested combination
is unsatisfiable under the declared threat model.
Engineering cannot make a policy or UI label override
the operating system's discretionary privileges.

**No user choice requested yet:** after source + isolated
fixture review, the next useful work is a disposable
two-UID/authority mock and provider compatibility
fixtures. Their outputs and an explicit capability
matrix should be presented before seeking a single
informed owner trade-off. **Do not begin implementation
or accept ADRs from this paper comparison.**

## 9. Additional isolated two-UID DAC fixture (2026-10-14)

This **does not simulate Wyss**; it checks only Linux discretionary
access for distinct genuine OS identities, under the same
already-authorised disposable-fixture scope.

**Environment:** isolated tool container, two existing non-login
Linux test identities: `nobody` (UID 65534) for a dummy
authority server, and `daemon` (UID 1) for a dummy
unprivileged client. The fixture runner started as root
**inside the container only** to assign disposable
file ownership and launch both unprivileged processes;
no command ran on either owner computer and no system user,
service, provider, repository content or credential was created.
Files were restricted to a temporary `/tmp` directory
(`0700` UID 65534), mock socket `0600` and
`protected.txt` `0600`.

**Observed output:**

```text
SERVER READY 65534 SERVED_UID 65534 EXIT 0
DIFFERENT_UID 1 => SOCKET_DENIED 13; FILE_DENIED 13
SAME_UID 65534 => SOCKET=ALLOWED; FILE_READ= MOCK_STATE
```

- **PASS within synthetic fixture:** different ordinary UID
  received `EACCES` (errno 13) for both the socket
  and the protected mock state file.
- **PASS within synthetic fixture:** server correctly
  accepted a same-UID client using `SO_PEERCRED` and
  the same-UID client read the mock state file.
- **Cleanup:** temporary directory automatically deleted
  on fixture completion; no file/content persists.
- **Not tested / still blocked:** actual native agent
  under different principal, Wyss authority binary
  or state, admin-installed protected unit, keyring,
  signing/physical presence, socket FD passing,
  supplemental groups/ACL, user service override,
  debugger/ptrace, root, credential refresh or
  real network/remote. No attacker obtained access
  to actual Wyss managed approvals.

**Interpretation:** this directly supports the limited claim
that two distinct unprivileged UIDs can be separated by
ordinary filesystem ownership and Unix socket permissions.
It does **not** establish a functioning secure owner-
approval architecture, or that a protected separate-UID
service can be provisioned without changing accepted
ADR 0025. The next tests for S1/S2 require a
concrete service/approval identity and compatible
credential/host-login story. Do not count this fixture
as having unblocked final-product OL3 or LP4.


## 10. Follow-up simulated signer, credential and file-safety evidence

[44](44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md)
documents **executed** disposable fixtures FT-01..19:

- Separate protected UID's state and signer key were
  inaccessible to the untrusted UID; a public request
  socket accepted untrusted proposals but executed only
  an exact signed dummy intent. Forgery, mutation,
  expiry, scope mismatch and duplicate approval were
  rejected, including after a mock server restart.
- This mock signer was invoked by the fixture controller,
  **not by an actual human**; thus TV-ISO-04 remains
  **PARTIAL synthetic**, with trusted UI/device challenge
  and exact human intent NOT RUN.
- A provider-auth *dummy* file owned by the original UID
  was unreadable by a distinct UID, while a rootless
  `unshare -Ur` process displaying UID 0 inside its
  namespace could still read the original host-UID
  file. Consequently B needs a real auth/refresh
  compatibility strategy; merely remapping IDs is
  not sufficient.
- A synthetic `Path.resolve` then path write redirected
  through a swapped ancestor symlink, whereas an
  anchored directory descriptor prevented redirect.
  Real `openat2` on the disposable kernel allowed
  an intended write and refused `../`, an absolute
  path and symlink-based escape. These are narrow
  kernel/file API tests, **not** original Wyss execution
  or validation of DN-31 replica/snapshot safety.

**Current evidence state:** TV-ISO-02, TV-ISO-04,
TV-ISO-06 and TV-ISO-07 have **partial synthetic**
coverage only. TV-ISO-03/05/08/09/10 for actual
product paths are **NOT RUN**. All accepted ADR
constraints and both REJECT reviews still stand.


## 11. B sandbox availability constraint (2026-10-14)

Before comparing real provider mock compatibility, the
isolated tool container was queried for Landlock.
`landlock_create_ruleset(NULL,0,VERSION)` returned
**ENOSYS (38)** even on kernel 6.18.44.
The container has no working user scope manager,
`bwrap`, `cargo`, `adb` or QEMU.
See [44 §8](44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md).

This is **BLOCKED_ENVIRONMENT**, not a negative
verdict on Linux Landlock in general or on the
owner's PC. No restricted worker profile with
real provider auth, hooks or MCP was run.
An externally available **disposable** Linux
VM/runner with a supported LSM and current
provider-mock fixtures is the next necessary
test target. No personal files, login tokens
or existing home services may be mounted into
that target.

The rootless namespace test from §10 shows
that remapping visible UID alone is not the
missing confinement. Likewise, a protected
authority requires a genuine independent
trust principal and a meaningful human
signer; the dummy signer does not provide it.

