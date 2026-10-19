# 47 — CachyOS native evidence: Docker-root equivalence, authority trust and restricted workers

Date: 2026-10-14. **Status: architectural feasibility delta / NOT_PROVEN, NOT accepted ADR, NOT implementation.**
The owner instructed planning and safe CachyOS-native investigation, without changing real
Docker, user groups, filesystem, services, credentials, provider processes, `main`, or accepted ADRs.

**New evidence:** [CachyOS-native verification report, pinned commit `124360101f488b1e29393293b826240c41030e0e`](reviews/2026-10-14-cachyos-native-feasibility.md).
Read alongside [43](43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md),
[42](42-POST-REJECT-CORRECTIVE-PLAN.md),
[11](11-SECURITY-THREAT-MODEL.md), accepted ADRs 0011/0018/0025,
and [Codex REJECT](reviews/2026-10-14-post-reject-corrections-external-audit.md).
The prior mock experiments under 44 were in a different environment and
are **not** substituted for CachyOS measurements.

## 1. Factual evidence vs inferred consequence

| ID    | What the CachyOS report actually observed                                                                                                                               | Safe interpretation and limit                                                                                                                                                                            |
| ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CE-01 | Native `ID=cachyos`, x86_64 kernel `6.18.55-2-cachyos-lts`, KDE X11, `systemd 262`, cgroup v2, bubblewrap `0.13.0`, Landlock **ABI 7**                                  | These primitives exist on the observed desktop; no claim of final sandbox or security boundary                                                                                                           |
| CE-02 | Same-UID processes connected to a mode-0600 synthetic authority socket; same-UID state file was readable                                                                | An OS-user account is not a human authentication principal (ADR 0011/0018); scope includes native agents launched manually, not only OL3                                                                 |
| CE-03 | Landlock worker denied synthetic authority **file** access, inherited restriction to children; socket **pathname connect succeeded**                                    | **FAIL** for a Landlock-only protected-IPC claim on observed ABI 7. Kernel docs place `LANDLOCK_ACCESS_FS_RESOLVE_UNIX` at **ABI 9**; header macros alone cannot infer runtime support                   |
| CE-04 | Rootless bubblewrap mount namespace omitted mock authority socket, so pathname lookup failed; deliberately exposing the socket with read-only bind permitted connection | A mount made read-only is **not** an IPC security barrier. Missing mounts plus no preopened FDs/alternative channels are necessary; still not proof of complete process containment                      |
| CE-05 | Fake token in-place overwrite worked with file grant; fake `token.new` + `os.replace` **failed** for file bind/grant, **worked** with a dedicated writable directory    | Provider-compatible refresh must be evaluated by _actual provider mechanism_. Widening access to a directory broadens the worker's effects; never bind an entire existing credentials home on assumption |
| CE-06 | Bubblewrap mock performed a project write and read a mandatory read-only policy; no provider executable, network model inference, hooks/MCP or real auth exercised      | Useful synthetic operation is possible; final-product native-provider compatibility **NOT_PROVEN**                                                                                                       |
| CE-07 | `/run/docker.sock` accessible R/W by UID 1000's active group `docker`; rootful `dockerd` was running as UID 0. No Docker API call occurred                              | A process retaining that user's Docker socket capability may obtain root-equivalent host authority. This is a **threat-model blocker**, not evidence an actual attack was attempted                      |
| CE-08 | `openat2`, PTYs, namespaces and mock symlink/rename cases behaved as documented; Btrfs root/home are separate subvolumes, but snapshots and sync not inspected          | No claim that DN-21/DN-31, provider config writes or terminal reattachment passed                                                                                                                        |
| CE-09 | Owner's existing `wyss.service` uses `User=saintwyss`, `Group=wyss-home`, `DynamicUser=no`; no named `wyss-control` process found                                       | An existing unit with protections is **not** proof of a distinct protected Wyss Console authority principal, and a group is not a separate UID                                                           |

Official upstream sources (research only, no vendor integration tested):

- Docker warns that the `docker` group grants **root-level privileges**:
  https://docs.docker.com/engine/install/linux-postinstall/
- Docker rootless executes daemon and containers without root privileges, subject to migration and host prerequisites:
  https://docs.docker.com/engine/security/rootless/
- Landlock pathname UNIX socket restriction appears only at ABI 9 (not observed ABI 7):
  https://docs.kernel.org/userspace-api/landlock.html

## 2. Trust model, corrected

The final product must distinguish:

- **U (full owner account):** unrestricted manually opened native Claude Code/Codex/Hermes terminals, user KDE/X11 session, and LP4 remote shell. Under current CachyOS group privileges, this **can include root-equivalent Docker daemon access**, not merely ordinary UID 1000 filesystem access. No guarantee about what those full-account agents _will_ choose to do.
- **B (restricted automated worker):** rootless sandbox for **only** the explicit OL3 worker process tree, if every inherited FD/socket/namespace mount, process signal, user D-Bus, X11, Docker, network path, provider credential and plugin/hook path has been accounted for. B is a _different_ capability tier from U.
- **A (protected approver/authority):** an independently owned trusted verifier/decision/persistent-state domain, ideally a different genuine host OS identity with a nonforgeable human-approval source. It protects managed **approval integrity** only if U **cannot rewrite the verifier or trusted state by any path**, including rootful Docker/other admin delegations. A same-UID native dialog is not a strong independent signer.
- **M (managed runtime):** `wyss-control` owns its approved managed turns under **accepted ADR 0012/0015 current limits**; no multi-runtime or model-per-turn change is silently authorised.

### 2.1 Critical implication of CE-07

The combination `U has unrestricted access to the rootful Docker daemon` +
`A stores trusted state on the same host protected only by a different UID` +
`A is secure from all U processes` is **not defendable**.

Even if DAC denies the cross-UID socket, a compromised U with Docker-daemon privileges
may obtain elevated host filesystem/process access through the rootful container management
interface. This is a **capability and architecture inference**, corroborated by Docker's
own root-level warning, not a locally executed exploit.
Do NOT perform a proof by contacting the live Docker socket or starting privileged containers.
Mark `BLOCKED_ADMIN_DELEGATION` for any proposed architecture claiming that separation.

The finding also upgrades the remote risk:
**an LP4 full shell on this account may reach root-equivalent Docker control**.
Under those conditions a stolen/untrusted remote input grant can be materially more
powerful than a "user-files-only" risk label. Revoking LP4 cannot reverse prior account
or privileged daemon effects.

### 2.2 Same UID processes _outside_ the B sandbox

Bubblewrap may hide `/run/docker.sock` from a B worker, but does **nothing to an
unrestricted U native process outside B**. Therefore B-only confinement is **not**
proof that same-UID clients cannot forge requests to a same-UID `wyss-control`,
nor that a protected A is safe if arbitrary U processes can access rootful Docker.
No native fully unrestricted agent can simultaneously be presumed hostile and
be granted root-equivalent host capability while promising strong host-enforced
approval isolation without an independent trusted boundary.

### 2.3 X11/local confirmation

The observed desktop uses **KDE X11**. A GUI prompt in the ordinary user's display
must not be represented as a _cryptographically strong human signer_ against a
malicious process with access to that display. A real A requires a signer/authentication
mechanism protected against the attacker in the stated threat model, and an exact
reviewed-intent digest. Changing window toolkit, adding an "Approve" button,
or password-protecting a same-UID file is not such a mechanism.

## 3. Candidates — do NOT silently alter Docker

| Candidate                                                                                                                                                                                    | What it changes                                                                                                                                         | What it might preserve                                                                                       | Current blocker / action                                                                                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D0: retain rootful Docker group access unchanged                                                                                                                                             | **No system changes**; current U retains root-equivalent delegation                                                                                     | Existing Docker development workflows and unconfined native agents                                           | **Cannot claim A secure against compromised U** if it relies only on separate UID / same host                                                                                                     |
| D1: migrate development to **rootless Docker** and remove ordinary U's access to rootful daemon                                                                                              | Requires careful daemon/CLI-context/container/volume/network/service compatibility, revised group memberships and possible administrator-approved setup | Docker CLI and much routine container development, **conditional** on actual workloads                       | **NOT TESTED** for user's current workloads; can require privileged host changes; owner decision only after a reversible migration plan                                                           |
| D2: keep rootful daemon, but access it only through explicit privileged authentication on demand, not automatic group membership                                                             | Removes automatic rootful Docker API from every U agent but keeps rootful tools available to an explicitly authorised user                              | Existing daemon/images/volumes more likely retained; avoids always-on Docker group access from native agents | Requires user privilege changes, re-login/session handling, review of sudo/polkit rules; exact access path unknown                                                                                |
| D3: keep U unchanged, offer only **best-effort / advisory** approvals for native/LP4 full-account work; strong managed approvals asserted only within a narrower, actually provable boundary | Formal reduction of claimed tamper resistance rather than changing Docker                                                                               | Maximum no-change native/remote convenience                                                                  | **Real owner security trade-off**: final product cannot advertise universal nonforgeable approvals from a full-account agent; must not silently choose                                            |
| D4: separate dedicated device/host or independently trusted hardware approval/authority service                                                                                              | More hardware/network/operational cost and cryptographic verification design                                                                            | Existing unrestricted Docker on primary desktop                                                              | Same primary desktop still can change project/agent work and possibly falsify results; trusted external verifier/host would protect only specified effects. No external endpoint/hardware assumed |

**Design direction, not owner authorisation:** research D1 and D2 as risk-reduction
alternatives, because they can preserve normal Docker development while reducing
unattended admin power of every native agent; however, **do not choose either
or change group memberships, daemon, contexts, containers, ports, mounts,
existing volumes or firewall** until the owner sees measured compatibility
and agrees. D3 remains a valid _honest security model_ candidate, not a
silent default to weaken the final-product promise.

## 4. Native B profile: verified vs unsupported

A CachyOS **ABI 7** restricted worker cannot rely on Landlock
to prevent pathname UNIX socket connection. The observed viable
_building blocks_ are:

- bubblewrap rootless minimal mount namespace **omits** home,
  control authority DB/socket, `/run/docker.sock`, X11, D-Bus
  and other session channels by default;
- Landlock further restricts visible filesystem writes and
  certain signals, with irreversible inheritance once applied;
- no inherited authority/socket/Docker FDs; do not pass the
  already-connected FD into a sandbox;
- synthetic project RW + mandatory static policy read-only;
- dedicated fake token **directory** if and only if its
  provider's expected refresh operation really requires
  atomic replacement of the token; a file-only bind can break it;
- controlled outbound model inference needs an egress
  design **not yet tested**; `--unshare-net` with no network
  is not a functional provider replacement;
- every enabled MCP/plugin/hook must be classified as an
  effect path rather than trusted because it starts under B;
- a scope/cgroup/FD and parser isolation proof is separate
  from filesystem namespace containment.

**Current status:** B's core file/socket/synthetic PTY **partially feasible on CachyOS**;
safe actual provider sign-in/refresh, actual CLI/TUI, network inference, hooks, MCP,
Git repository operations and resource bounds **NOT_PROVEN**.
Do not declare "B supports native providers" yet.

## 5. Next non-destructive verification plan

Work may continue _without asking the owner_ for permission for the following
read-only or disposable nonprivileged probes, provided the already agreed
restrictions remain true and the active computer is explicitly CachyOS:

| ID       | Goal and method                                                                                                                                                                           | Pass vs blocker                                                                                              |
| -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| TV-CA-01 | Static/source compare installed provider versions' **official documented** auth refresh, config/home, hooks, MCP and CLI runtime needs; do not read auth bytes or launch providers        | Evidence labelled DOCUMENTED / NOT_OBSERVED. Report unknowns, no invented feature support                    |
| TV-CA-02 | Synthetic B profile with a **fake** provider that performs code-file reads, project writes, token refresh in a dedicated directory, PTY attach, exit/reconnect; no actual provider binary | Preserve useful mock workflow while denying nonproject files, control and Docker sockets                     |
| TV-CA-03 | Audit proposed B mount, FD, D-Bus, X11, `/proc`, inherited group, syscall/IPC and executable origin list; include negative tests for mounted read-only sockets and preopened FDs          | No side-effect claims while any unexamined path could reach protected authority or Docker                    |
| TV-CA-04 | Investigate _read-only_ package/service metadata for rootless Docker availability and features; do not contact the live Docker daemon/socket, install, migrate, disable or change groups  | Produce a migration feasibility table; exact running-workload breakage remains UNKNOWN without later consent |
| TV-CA-05 | Identify accepted ADR changes required for an independently protected A (0011, 0018, 0025 and any associated bootstrap/approval claims); no ADR acceptance                                | Candidate ADR transition proposal that **cannot** be silently applied                                        |
| TV-CA-06 | Native secure-file design review (openat2/FD anchoring) for provider file reads vs future mutation; no private Btrfs snapshots, real provider files or restored data touched              | DN-31 remains blocked until safe transaction/snapshot evidence exists                                        |

**Do not conduct** `docker ps`, `docker inspect`, API probes or
`docker run` on the live daemon merely to "prove" privileged
access. Do not run privileged test containers, `sudo`, user-group
changes, daemon/service restart or modifications to existing
provider login. Such steps require explicit owner approval
and a reversible plan, not another implicit extension of
the docs-only scope.

## 6. Test and acceptance blockers

| New test ID (specification only) | Required assertion                                                                                                                                                                                                          |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TS-DOCKER-01                     | Threat model includes rootful Docker daemon delegation from unrestricted U/LP4; no false "separate UID is safe" statement under root-equivalent group access                                                                |
| TS-DOCKER-02                     | A managed approval is protected against every in-scope U path, including mutable executable/unit/db, Docker broker, X11/local UI spoof and inherited FDs; if no such proof, block **strong approval guarantee**             |
| TS-DOCKER-03                     | A restricted B synthetic worker cannot access `/run/docker.sock`, real user/runtime sockets, inherited authority FD, sensitive home, X11/D-Bus, or create alternate IPC path, yet preserves useful mock provider functions  |
| TS-DOCKER-04                     | Provider fake credentials support **both** in-place and rename refresh according to versioned provider capability; unsupported mode is a visible error, never broad auto-mount of user's home                               |
| TS-DOCKER-05                     | A rootless Docker or privileged-on-demand alternative is assessed for real development needs: images, existing volumes, Compose, ports, networks, GPU/devices and current services; **NOT RUN** until explicitly authorised |
| TS-DOCKER-06                     | LP4-away risk screen explicitly labels **possible root-level delegation** under current host account, device-loss/revocation limits and permissions; do not assert "host secured" after a revoked phone                     |

These are **NOT RUN** product tests. CE-01..CE-09 are source-report evidence,
not new runtime execution in this document. A paper checklist passing
cannot reverse the current independent REJECT verdicts.

## 7. Decision and next user checkpoint

**No owner decision yet.** The conflict is real: a user account retaining
unrestricted access to a rootful Docker daemon cannot be safely modelled
as merely a nonprivileged attacker. Engineering should first document
the least disruptive ways to preserve development workflows while
removing unconditional delegated root access. If each viable secure
path requires host changes, present the owner one clear trade-off:
**preserve unrestricted Docker/native convenience with explicitly
weaker approvals**, or **authorise carefully planned Docker privilege
separation and a genuinely protected approval principal**.

For now, **leave the OS untouched, ADRs Proposed/Accepted states unchanged,
PR #2 draft, and all final-product native OL3 strong-security claims BLOCKED**.

## 8. Provider auth evidence added from official documentation

[48](48-PROVIDER-ISOLATION-AND-AUTH-COMPATIBILITY.md)
refines TV-CA-01 without touching user accounts:
Claude Code's official Linux credential location
is `~/.claude/.credentials.json` and
`CLAUDE_CONFIG_DIR` changes authentication,
plugins/settings and session state scope.
OpenCode documents `~/.local/share/opencode/auth.json`.
Accepted ADR 0018 specifically covers Codex
**0.159.0 managed** in-place credential refresh
through a validated file bind. None proves
new native OL3 provider compatibility, and
Hermes remains an independent code-execution
trust surface. Provider version pinning and
fake token refresh tests must precede any
real-use proposal. No secret bytes were read.

## 9. Rootless Docker migration compatibility: upstream restrictions, no changes made

To support a future informed D1-versus-D2 choice, compare **official
Docker source documentation**, **not installed/rootless measurements**:

| Feature                   | Official upstream rootless condition                                                                                                                       | What native CachyOS evidence supports / does not support                                                                      |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Daemon privilege          | Rootless mode runs dockerd and containers without host-root privileges, unlike `userns-remap` where dockerd stays root                                     | Current daemon **is rootful** with group `docker` read/write. No rootless daemon was started or tested                        |
| Kernel filesystem         | Official supported rootless storage includes `overlay2` on newer kernels and `btrfs` under specified kernel/mount prerequisites                            | CachyOS kernel 6.18 and Btrfs root/home observed, but backing storage configuration and driver are **not verified**           |
| CPU/RAM/PID limits        | Rootless resource limits depend on cgroup v2 and systemd delegation                                                                                        | cgroup v2 + systemd 262 observed; user controller delegation for new Docker daemon still **not verified**                     |
| Published ports           | Low ports (below 1024) need special setup or changed host port selection                                                                                   | Unknown which current workloads rely on 80/443 or host networking. Never silently migrate port bindings                       |
| Networking                | RootlessKit user-mode networking may add overhead, alter source IP behaviour and have feature limitations; overlay network not supported                   | No network/latency/Compose compatibility tests. Performance remains **UNKNOWN**                                               |
| Existing images/volumes   | Rootless uses different daemon context and user-managed storage; must separately validate data and workload migration                                      | Rootful images, database volumes, project networks, GPU/device requirements and current running workloads **NOT INVENTORIED** |
| Boot & user lifecycle     | Rootless Docker normally uses a user systemd service; login/linger lifecycle requires review                                                               | User manager running on observed CachyOS; no rootless unit or linger setting installed/changed                                |
| Remote/admin consequences | Removing automatic rootful Docker access would reduce root-equivalent delegation; keeping `docker` group or live rootful API access nullifies that benefit | No group changes or root API calls authorised; **BLOCKED_ADMIN_DELEGATION** remains                                           |

Upstream references:

- https://docs.docker.com/engine/security/rootless/
- https://docs.docker.com/engine/security/rootless/troubleshoot/
- https://docs.docker.com/engine/security/rootless/tips/
- https://docs.docker.com/engine/security/userns-remap
- https://docs.docker.com/engine/install/linux-postinstall/

**Do not propose simply changing the Docker CLI context** as a root-security
fix while the same user retains membership/access to the rootful daemon.
A correctly configured rootless context might preserve many development
commands, but a rootful daemon still reachable by the account remains a
root-equivalent avenue under this threat model.

**Stop gate:** a future migration plan must inventory the owner's actual
Compose/workload/volumes/ports/service needs with separately authorised,
nonsecret and non-disruptive evidence; define backup, import and rollback;
and ask for **explicit** approval before changing daemon, groups,
contexts or containers. None of this was performed.

## 10. Combined profile next-test contract

[49](49-CACHYOS-COMPOSED-WORKER-SECURITY-CONTRACT.md)
defines the narrowed B1 acceptance tests (FX-B1-A..E)
for a real **synthetic** CachyOS worker with useful
project and fake auth function, negative authority/
Docker/D-Bus/FD coverage, inference-broker design
and truthful PTY lifecycle. It does not alter the
rootful Docker risk or approve a D1/D2 migration.
For correct claim scope, **B1-local containment
does not establish trusted human approvals or
protection from independent unconfined U/LP4**
while host-root delegation is reachable.
