# 49 — CachyOS combined OL3 worker profile: falsifiable sandbox and trust contract

> **2026-10-22 consolidation:** this document retains dated design/evidence and detailed
> contracts. Its present-tense status/decision tables describe their stated baseline,
> not necessarily current implementation. Read the [current product set](../product/README.md),
> [verified gaps](../product/implementation-status.md) and [requirement crosswalk](../product/traceability.md)
> first. F8 is active P2/V8, ADR0051 Accepted, cap1; P1 remains available. New product
> direction does not supersede accepted security ADRs or turn proposed tests into PASS.

> Integration note (2026-10-19): the dated design and experiment statements below are historical. Only the pure Rust data/validation foundation and test-only filesystem fixtures are being integrated; there is no production B1 launcher, trusted capture, quarantine service or new authority permission. B1 containment and FX-H01 remain NOT_PROVEN. The current owner policy permits ordinary unit fixtures but prohibits disposable HOME, private keyrings/buses, VMs, containers and artificial user installations for development/acceptance. Historical isolated experiment recipes are not current instructions. Accepted authority contracts remain unchanged.

Date: 2026-10-14. **Status: design and test contract only — NOT_PROVEN, not an accepted ADR, not implemented.**
The owner authorised documentation and safe synthetic/non-privileged CachyOS verification,
not installation, privilege changes, provider execution, real credential access, or changes
to the working desktop. This document is not an instruction to silently run the tests.

**2026-10-14 post-experiment status addendum:** the composed native mock has now run: FX-B1-A **FAIL** (FX-H01 forbidden-signer hardlink read); FX-B1-B/C/D/E **PASS only within their synthetic scope**, as documented in [the pinned report](reviews/2026-10-14-fx-b1-composed-native-verification.md) at commit `3ff0d7a865e2b5aeddfa68a6c98a72c6f0667354`. This document's original sections 3/5 are retained as the _pre-execution specification/history_. The approved O-1/O-2/O-3 product choices, proposed RV-1..8 corrections and next candidate are in [50](50-B1-STAGED-WORKSPACE-AND-QUARANTINED-RESULT-CONTRACT.md), with consolidated CG gates in [51](51-B1-CONSOLIDATED-ACCEPTANCE-AND-DELIVERY-PLAN.md). **FX-H01 OPEN; B1 NOT_PROVEN; no ADR accepted or implementation authorized.**

Read with:

- [47](47-CACHYOS-DOCKER-PRIVILEGE-AND-SANDBOX-DECISION.md) — delegated root and possible Docker changes;
- [48](48-PROVIDER-ISOLATION-AND-AUTH-COMPATIBILITY.md) — native provider credential compatibility;
- [CachyOS report](reviews/2026-10-14-cachyos-native-feasibility.md) — real **synthetic** observations;
- [43](43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md) — authority A vs restricted-worker B;
- [42](42-POST-REJECT-CORRECTIVE-PLAN.md) — unresolved REJECT gates;
- accepted ADRs 0011, 0012, 0015, 0018, 0025 and 0027.

## 1. Precisely what B can and cannot prove

A restricted OL3 worker **B1** is an intentionally lower-trust _automated_
native-provider profile. It is separate from:

- **U:** ordinary unrestricted native PTY and LP4 full-account shell, potentially
  root-equivalent through the owner's active membership of `docker` with
  a rootful daemon. U is **not** contained by B1, and can mutate user-owned
  hooks, projects, aliases, launch scripts and provider control state.
- **M:** the presently accepted managed Codex authority, with **one shared
  App Server and one globally active managed turn** under ADR 0012/0015.
  No acceptance of parallel managed runtimes follows from B1.
- **P:** Wyss Agent's Hermes planner, with only typed plan proposals and no
  approved independent effectful-tool access.
- **A:** an independently trusted approval issuer + protected state/verifier;
  B1's sandbox does **not** establish A, physical human consent or resistance
  to a different unrestricted U process.

Two different security claims must be checked independently:

**B1-local containment:** `B1` cannot directly reach sensitive
host IPC, admin brokers or non-project files _from its actual process tree_.
This may be feasible on native CachyOS with bubblewrap, a private mount/PID
view, FD closure, Landlock and additional tested restrictions.

**Host-global strong approval:** **every** same-user process (including
unconfined U and remote LP4) cannot tamper with `wyss-control`
or impersonate human consent. **NOT_PROVEN** and not achievable by
restricting only B1 when U still has direct access, especially with
root-equivalent Docker delegation. A future independently protected
principal/approval design is still mandatory if that is the target.

Passing B1-local tests shall **never** be presented as passing host-global
strong approval. The current threat-model finding is
`BLOCKED_ADMIN_DELEGATION + BLOCKED_TRUST_BOUNDARY`.

## 2. Non-optional B1 containment contract

All clauses below are _candidate acceptance conditions_, not an implementation
prescription; select and independently review exact mechanisms before an ADR:

| Area                    | Minimum positive functionality                                                                                                  | Required negative boundary                                                                                                                                                                    |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Host identity           | Process runs unprivileged on native CachyOS                                                                                     | No rootful Docker delegation, sudo, polkit, host container runtime or alternative privileged broker; no inherited FD to any of them                                                           |
| Mount namespace         | Specific versioned native provider executable/library paths readable; synthetic project and required policy available           | `control.sock`, `control.sqlite3`/WAL/SHM/witness, host Docker socket, keyring, X11, user D-Bus, unintended `~/.config`, `~/.local`, `/run` or whole home not visible                         |
| File policy             | One disposable project writable, mandatory policy read-only, dedicated **fake** auth capability scoped to provider refresh mode | Symlink/hardlink/ancestor swap cannot expose forbidden files; no global hooks edited, no arbitrary home/SSH secrets                                                                           |
| Landlock ABI 7          | Further restrict new file accesses/selected process signalling as supported by actual kernel                                    | **Do not claim pathname UNIX socket protection** under ABI 7; socket-name isolation depends on mount visibility plus other verified controls; do not assume Landlock revokes preopened FDs    |
| Process/FD              | PTY input/resize, useful dummy worker exits cleanly and all descendants are tracked                                             | Close ambient FDs except explicitly reviewed stdio; ensure `/proc`, inherited socket, SCM_RIGHTS, ptrace, child/host signals, abstract UNIX and alternate IPC do not bypass intended boundary |
| Provider authentication | Fake provider can perform _both_ overwrite-in-place and atomic-rename token update in explicitly tested profiles                | A single-file bind **must fail visibly** when rename is required; no actual user credential copy or home-wide bind to fake a positive                                                         |
| Network/inference       | A **separate, explicitly defined** fake inference exchange succeeds via a narrow egress design                                  | A zero-network worker that cannot infer is not provider-compatible; no unrestricted host/session/network access or reused control socket masquerading as an inference channel                 |
| Hooks/MCP/Git           | Required policy and safe mock hook/extension workflow demonstrably work; bounded Git edit in test project                       | A model-provided hook, MCP executable or repo file is not a trusted capability; no arbitrary external exec/broker escape                                                                      |
| Resource accounting     | Measured RSS, process tree, idle CPU, PTY latency and graceful cancellation on the target                                       | No silent global cgroup edits, force-kill of real processes, host service restart, uncontrolled orphan/scope escape or resource-exhaustion tests outside synthetic fixture                    |
| Reconnect/state         | Synthetic host can report live vs detached vs dead honestly                                                                     | Terminal emulator state and host persistence are **separate** from B1 and not proven by PTY-only synthetic success                                                                            |

### 2.1 ABI and Docker facts (source-supported, not new local tests)

The kernel's Landlock documentation states `LANDLOCK_ACCESS_FS_RESOLVE_UNIX`
starts at **Landlock ABI 9**; the referenced CachyOS host observed **ABI 7**.
Abstract UNIX scope and signal restrictions have different ABI and per-domain
semantics. A read-only bind of a socket is not safe: the CachyOS synthetic report
actually **connected through one**.

Linux source:
https://docs.kernel.org/userspace-api/landlock.html

Docker's official documentation states access to the rootful
`docker` group grants root-level privileges. Rootless Docker removes the
daemon's host-root privilege, but a changed CLI context **does not remove**
an independent existing path to the rootful daemon.
Sources:
https://docs.docker.com/engine/install/linux-postinstall/
https://docs.docker.com/engine/security/rootless/

**No daemon query, group change, rootless migration, service start or admin
operation was performed in writing this contract.**

## 3. Distinct synthetic provider fixtures — require _usefulness_, not only denial

The following are test designs to execute **only** from a real, safe native-CachyOS
session authorized for L0/L1 with new isolated fixture directories and no
host-privileged operations. Nothing here authorises a provider binary launch.

### FX-B1-A — closed offline provider surrogate

Use a tiny purpose-built, synthetic executable or interpreter program running
with an explicit clean environment. Fixture setup creates only a newly owned
0700 random temporary tree with `project/`, `policy/`, `fake-auth/`,
`mock-control.sock`, `mock-docker.sock`, `mock-dbus.sock` and
`canaries/`. Avoid any real path, especially `~/.claude`,
`~/.codex`, `~/.local`, KDE user D-Bus/X11 and `/run/docker.sock`.
Test name-only strings may refer to those concepts but must not connect to
real sockets.

Positive requirements: read exact mandatory policy, parse fixture-defined
configuration, write a file under the test project, use a fake token in-place,
open and communicate over a synthetic PTY, exit without survivors.

Negative requirements: cannot resolve/connect to **dummy** control/Docker/dbus
sockets kept outside the worker namespace; cannot read dummy signer/private
authority DB; cannot access synthetic denied home; cannot send signals
to a dedicated harmless canary process; no socket/FD inherited accidentally;
no host filesystem mounted just to make a denial pass.

**Stop on any unexpected host resource lookup**, e.g., an actual control or Docker
socket, and log `BLOCKED_UNSAFE` without attempting an access.

### FX-B1-B — token-refresh compatibility matrix

Perform two provider-mock algorithms against **fake** credentials:

- `overwrite_in_place`: open token for write/truncate and close;
- `replace_atomically`: write `token.new`, fsync file, rename to `token`
  within a private dedicated directory, then verify new exact bytes.

Run each against a narrowly bound file and an explicitly minimal dedicated
fake-auth directory. Record each positive and expected denial, final file
identity/inode if relevant, source version label and failed errno.

**Acceptance is provider-specific**, never a blanket claim that an auth
directory may be exposed. In particular, user-owned existing
`~/.claude` / `~/.codex` / `~/.local/share/opencode`
must NOT be mounted or copied in this experiment.

### FX-B1-C — inference without exposing host authority

An offline `--unshare-net` profile is expected to fail provider inference.
Define and test a separate _mock-only_ egress arrangement with an
explicitly allowlisted address/channel and a fake inference response.
Before testing, enumerate how that broker is authenticated, what
socket/FD is exposed, whether its protocol can be misused as a proxy
to `wyss-control`, and which exact external addresses can be contacted.
An unrestricted bridge to host network or user D-Bus is **not** a PASS.

If no safe mock-only egress channel can be established without privileged
network setup, record `BLOCKED_EGRESS_DESIGN` and continue other cases.
Do not substitute a real API call.

### FX-B1-D — hostile handoff and worktree state

Use only synthetic project files and mock hook definitions.
A step may write text that mimics an agent/system instruction and
a fake `hooks/` or `.git/hooks/` directory. The downstream _mock_
must identify and refuse untrusted instructions and not execute
them. Flag a stale worktree/control digest before any native
step is scheduled. This is not equivalent to actual provider
hook execution or approval integrity.

### FX-B1-E — lifecycle/restore without touching real services

Start and stop one short-lived dummy PTY process with the standard
user's privileges and an isolated temporary tree.
Record a process generation, detach/rejoin and actual observed
child exit status. An interrupted output stream is `UNKNOWN`,
not a verified exit. **Do not** enable linger, change real
`systemd --user` services, kill real child processes or
force a host logout/crash. Those tests stay `BLOCKED_UNSAFE`.

## 4. Fail-fast matrix and evidence levels

| Test family / blocker      | Existing requirement                       | Acceptance                  | Minimum useful evidence                                                                                     | Current status as of 2026-10-14                                                                      |
| -------------------------- | ------------------------------------------ | --------------------------- | ----------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| FX-B1-A                    | FR-SEC-13                                  | TS-DOCKER-03 / AC-DOCKER-02 | Real CachyOS synthetic worker performs mock positive function **and** denies mock control/Docker/DBus paths | **EXECUTED / FAIL — FX-H01**; functionality passed but hardlink exposed forbidden synthetic signer   |
| FX-B1-B                    | FR-PRF-12                                  | TS-DOCKER-04 / AC-DOCKER-03 | Both fake refresh algorithms across file-vs-dir privileges, tested exactly                                  | **EXECUTED / PASS — four fake file-vs-directory refresh cells only**; no real provider compatibility |
| FX-B1-C                    | FR-SEC-13 + future provider contract       | TS-DOCKER-03 / AC-DOCKER-02 | Fake inference succeeds without authority/broker escape                                                     | **EXECUTED / PASS — fake inference and bounded broker assertions only**                              |
| FX-B1-D                    | FR-SEC-13, FR-ORC-04                       | TS-SEC-OL3-02 / AC-IR-03    | Fake prompt/hook injection cannot promote authority                                                         | **EXECUTED / PASS — fake handoff/hook/digest assertions only**                                       |
| FX-B1-E                    | FR-REM-17 / FR-TERM-14                     | TS-SEC-HOST-01 / AC-IR-07   | Truthful synthetic child/PTY generation and stop                                                            | **EXECUTED / PASS — owned synthetic PTY lifecycle only**; host logout/crash persistence not proven   |
| A protected human approver | FR-SEC-12                                  | TS-DOCKER-02 / AC-DOCKER-01 | Legitimate/nonforgeable approval + protected verifier/state against actual declared admin-level attacker    | **BLOCKED_ADMIN_DELEGATION**                                                                         |
| Admin-delegation change    | Explicit owner-only environmental decision | TS-DOCKER-05 / AC-DOCKER-05 | Non-disruptive workload inventory, reversible rootless/on-demand privileged plan and **owner signoff**      | **NOT AUTHORISED** to change Docker                                                                  |
| Real native providers      | FR-PRF-12, DN-14/20/38                     | TV-PROV-04                  | Pinned provider versions and authorized nonsecret/independent auth lifecycle                                | **NOT AUTHORISED** to use real auth/config                                                           |

Do not replace `NOT RUN` with `PASS` because another fixture proved
the isolated kernel primitive or because a test-case identifier is present.
A test report needs precise negative and **positive** observations,
kernel/ABI/version, fixture manifest digest, process/FD inventory,
resource measurements, cleanup and any unexpected exposure.

## 5. Architectural stop gates and the only non-routine owner decision

1. **B1 is not a solution to compromised U/LP4.**
   Normal native sessions and full remote terminal can retain access
   to user files and rootful Docker. A B1 PASS does not certify
   unforgeable managed approvals for these independent processes.
2. **A under a different UID is not sufficient while U can obtain host-root
   via the Docker daemon.** Protecting A from the attacker
   requires either removing always-on delegated root access,
   deliberately reducing the threat guarantee, or genuinely
   independent protected hardware/host. No option has been selected.
3. **Do not trade away existing development functionality silently.**
   Rootless Docker or privileged-on-demand Docker has real
   compatibility, volume, networking, GPU/device, and user
   lifecycle implications, documented as unknown in 47 §9.
   Docker migrations, group changes or service operations
   require explicit owner approval and a reversible plan.
4. **Provider auth and current managed invariants stay distinct.**
   Existing managed Codex uses a tested version-specific credential
   capability under accepted ADR 0018. It does not establish B1
   native full-feature login, and ADR 0012/0015 still limit
   managed concurrency.
5. **No strong human-consent claim yet.** A trusted
   cryptographic signer needs an external approval surface that
   can present and bind exact intent bytes with recoverability.
   Synthetic Ed25519 signing proves only signature checks, not
   human presence or account integrity.

**Historical next deliverable (now completed):** a **CachyOS-native composed mock B1 report** with FX-B1-A..E. Its actual result was **FAIL** because of FX-H01; see the post-experiment addendum and the newer [50](50-B1-STAGED-WORKSPACE-AND-QUARANTINED-RESULT-CONTRACT.md)/[51](51-B1-CONSOLIDATED-ACCEPTANCE-AND-DELIVERY-PLAN.md).
The report can be produced without Docker API, existing
provider credentials, sudo, package installation, VM or
real systemd changes; dangerous/privileged cases stay blocked.

**Only after that evidence** and a nonsecret, read-only
workload/privilege inventory should the owner be asked whether
to preserve always-on rootful Docker delegation with explicitly
weaker approval guarantees or authorise a carefully reviewed,
reversible separation. Until then, do not invent a security
choice or claim final product viability.

**Nothing in this document authorises application implementation,
changes to installed providers, accepted ADR updates or PR merge.**

## 6. Reconciled candidate and next gate (dated addendum)

The tested B1 bound-project tree may contain a hardlink to a forbidden same-inode file; a hidden original pathname plus a Landlock project grant does not prevent reading that alias. Additional path canonicalization is not enough. The reconciled **proposed** design in [50](50-B1-STAGED-WORKSPACE-AND-QUARANTINED-RESULT-CONTRACT.md) is explicit owner-authorized Git-base plus local-modification overlay admission, new inodes on an independent disposable filesystem, trusted pre-exec confinement, frozen restricted export, and pending-only quarantine publication. It cannot promise to detect copies or transformed secret bytes admitted by an authorized owner. No live-tree auto-apply occurs within B1.

The [consolidated acceptance plan](51-B1-CONSOLIDATED-ACCEPTANCE-AND-DELIVERY-PLAN.md) contains **CG-01..CG-14 required synthetic gates** and **CG-15..CG-16 separate real-provider-stage gates**. These new gates have **not** been run; previous FX-B1-B..E PASS results cannot be promoted to a PASS for this candidate. Owner decisions O-1/O-2/O-3 are recorded in the [reconciliation](reviews/2026-10-14-fx-h01-design-reconciliation.md). Design verdict stays **REVISE_DESIGN / NOT_PROVEN** pending typed planning, fresh independent plan review and future verified implementation.

Nothing here authorizes editing code, changing system services, invoking Docker, using real providers or credentials, merging PRs, or accepting an ADR. The original 2026-10-14 contract above is preserved for historical comparison; these links explicitly supersede its old "NOT RUN" and "next deliverable" status lines.
