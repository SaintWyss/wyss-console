# 45 — Disposable Linux VM verification handoff (next evidence gate)

> **SUPERSEDED as general verification instruction (2026-10-14).** The owner requires native CachyOS, not Ubuntu/VM. Read [46 — CachyOS native verification](46-CACHYOS-NATIVE-VERIFICATION-HANDOFF.md) first. This document is retained **only** as a restricted procedure for risky/privileged tests that genuinely need a separate disposable guest. It is not permission or a requirement to create any VM.


**Status:** procedures ONLY, not a passed test or implementation authorisation.
The owner authorised isolated technical verification, but not application code
changes, real provider execution, use of real credentials, host OS modification
or ADR acceptance. This file is for an agent running **inside a separately
identified throwaway VM**, not for the real CachyOS workstation, Kali CTF VM,
real home directory, or production Wyss service.

Prior evidence:
[43](43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md),
[44](44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md),
[42](42-POST-REJECT-CORRECTIVE-PLAN.md),
[Codex REJECT](reviews/2026-10-14-post-reject-corrections-external-audit.md).
Reference accepted ADRs 0011, 0012, 0015, 0018, 0025.

## 0. Mandatory preflight and stop

- First identify the actual VM name, hostname, unique VM identifier,
  host OS, filesystem, process UID, virtualization boundary and
  clean disposable directory. Obtain a **separate clean VM/snapshot**;
  **do not repurpose the HackLab Kali VM or any existing
  production/workflow session**.
- If the current shell is **not demonstrably inside a disposable
  VM**, stop immediately with `BLOCKED_NO_DISPOSABLE_VM`.
  Do not install VirtualBox, enable hypervisors, change host
  packages, system services, user accounts, keyrings, SSH
  configuration or host files just to proceed.
- Confirm this runner has no access to owner `~/.claude`,
  `~/.codex`, `~/.config`, Chrome profiles, passwords,
  KWallet, SSH keys or real Wyss state. Disable network
  where possible; do not contact provider APIs or pass
  private account keys.
- Use only dummy mock provider `auth.json`, token strings,
  projects, service units, sqlite test DB and test sockets
  under the disposable VM. Root/sudo **inside the verified
  VM only** is allowed for test UID/service provisioning;
  never execute it on the host.
- **Do not run real Claude Code, Codex, Hermes, original Wyss,
  real Wyss Console or actual configuration writers.**
  Do not modify application code, main, accepted ADRs,
  the owner's personal accounts or running services.
- If prerequisites are missing or cannot be safely verified,
  report the precise blocker; never silently weaken isolation.

## 1. Evidence sequence

| ID | Test | Positive condition | Negative condition |
| --- | --- | --- | --- |
| VM-01 | Prove VM isolation and ownership | Unique disposable VM identity, no mounted owner data, available rollback snapshot | Abort if host/identity unclear |
| VM-02 | Protected approval service under a genuinely different OS UID | Synthetic owner intent signed externally, exact scope accepted and durable dummy receipt | Same-UID native client cannot modify verifier binary/unit, DB/WAL/SHM/anchor, signer key, nor self-issue signature |
| VM-03 | Human consent authenticity model | A **trusted simulated** signing actor approves exactly displayed deterministic mock digest, expiry and attempt | Untrusted process cannot invoke signer service or alter displayed digest; report `HUMAN_PRESENCE_NOT_PROVEN` unless a real trusted input/display mechanism exists |
| VM-04 | Native-worker B with rootless bwrap/Landlock candidate | Mock provider process reads designated fake auth, performs mock in-place token refresh, writes its designated project, runs a safe echo/PTY-like command | Cannot access authority sockets/state, unintended home paths, arbitrary outbound TCP, signal protected process, use accessible inherited FD or escape via namespace |
| VM-05 | Auth refresh compatibility | Fake Codex-style *in-place* token write works only where permitted | Read-only bound token fails visibly; do not copy the token into a second persistent home as a workaround |
| VM-06 | Crash and replay | Repeated typed dummy receipt has one committed state transition after service restart | Ambiguous external effect never repeated; distinguish state commit from process/shell effect |
| VM-07 | Symlink/time-of-check race | Descriptor-rooted `openat2` action stays inside allowed tree with ordinary directory links handled per policy | Rapid symlink swap/parent escape/absolute/mount boundary cannot redirect into denied synthetic path; if unavailable `NOT_SUPPORTED` |
| VM-08 | Native/manual/unattended isolation boundaries | Different-UID protected state survives an unrestricted manual owner-UID process | Document that unrestricted same-UID worker still owns its account and can tamper with unprotected client/worktree files. Do **not** claim account isolation |
| VM-09 | Headless process and login/logout semantics | Test-only `systemd --user` service survives only its authorised lifetime; collect actual evidence if VM supports login lifecycle | User service cannot silently assume a different `User=`; no host linger change |
| VM-10 | Resource/performance envelope | Record per-test p50/p95 wall times, peak RSS and process count for A/B mock | Do not infer real provider resource cost from dummy tests |

Read official Linux `openat2`, `landlock` and systemd
manuals, not snippets that omit their restrictions:
- https://man7.org/linux/man-pages/man2/openat2.2.html
- https://man7.org/linux/man-pages/man7/landlock.7.html
- https://man7.org/linux/man-pages/man5/systemd.exec.5.html

A sandbox that denies all native effects but breaks
all useful functionality is NOT a successful B result.
Show both mock positive function and negative protection.

## 2. Recording and output

One report only:
`docs/architecture/reviews/2026-10-14-disposable-VM-isolation-results.md`
(or a unique, non-colliding timestamped name).

For each VM test include:
- exact VM/guest identity, kernel, relevant package and tool
  versions; whether it used a separately provisioned
  test UID, namespace or same host UID
- fixture source/procedure (or hash/pinned location),
  specific command, sanitized stdout and stderr,
  expected vs actual pass/fail result
- rollback/directory destruction and whether
  network, system services or mock credentials were used
- strict evidence label: `OBSERVED_SYNTHETIC`,
  `SOURCE_ONLY`, `BLOCKED_ENVIRONMENT`,
  `NOT_RUN` or `FAILED`.
- clear distinction of mock signature from actual
  human/device attestation and mock provider from
  real upstream provider login compatibility.

Git operations only if already on the existing
`docs/console-planning-review-2026-10-13` branch
and workspace is clean; otherwise prepare a **new
docs-only branch** from its exact HEAD with a unique
report filename, without modifying another agent's
working tree. Commit **only the report Markdown**.
No CI workflow additions, no application source
changes, no PR merge or ADR status updates.

Conclude with a narrow verdict:
- `A_MOCK_FEASIBLE` if trust boundary passes VM tests,
  still real human/provider integration blocked;
- `B_MOCK_FEASIBLE` if useful restricted mock workflow
  and denial cases both pass, still real providers blocked;
- `BLOCKED` if environment is absent or controls fail.

**Success at this handoff never closes REJECT or DN-16.**
