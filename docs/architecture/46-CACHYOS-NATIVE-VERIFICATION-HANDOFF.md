# 46 — CachyOS native verification plan (corrects mandatory-VM mistake)

Status: **candidate verification procedure**, not implementation, security PASS, ADR acceptance, or approval to change installed settings.

Owner target: Wyss Console runs **natively on the owner's CachyOS KDE computers**, with the original user session, PTYs, managed agents, and mobile remote access. **Ubuntu is not a dependency, target, fallback, or required test platform.** The earlier document [45](45-DISPOSABLE-VM-VERIFICATION-HANDOFF.md) overstated a clean Linux VM as a universal prerequisite. That restriction was appropriate only to experiments that need privileged identity/service or unsafe fault injection. This document supersedes 45 as the normal **CachyOS-first** verification order.

## 1. Three levels, not one mandatory virtual machine

| Level                                                 | Target                                                                                                   | Permitted verification                                                                                                                                                                                                    | Forbidden                                                                                                                                                   |
| ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| L0 — Native CachyOS read-only inventory               | Owner's actual installation, through an explicitly authorised local Codex session                        | Kernel/LSM feature and Landlock availability, filesystem mounts and btrfs/snapshot topology metadata, availability/version of bwrap/systemd/cgroups, read-only schema/code/docs and nonsecret system capability detection | Editing existing config/auth, querying provider tokens, service restart, privileged OS changes, installing packages, writing to project files               |
| L1 — Native CachyOS disposable **synthetic** fixtures | Unique freshly created private temporary directory, only synthetic data, no personal mount               | Unprivileged test programs limited to dummy sockets, mock state, directory access controls, dummy PTY flow, negative/positive namespace/LSM tests **only if the environment supports them**                               | Touching `~/.codex`, `~/.claude`, `~/.config`, `~/.local/share`, original Wyss state, real accounts, persistent systemd services, broad filesystem deletion |
| L2 — High-risk / privileged fault injection           | **Separate disposable isolated guest or other owner-approved sacrificial environment only if necessary** | Creating truly different Linux users, system services, crashing/restarting a dummy authority, privileged namespaces, aggressive race injection; disposable content only                                                   | Running these destructive or privileged tests on the normal CachyOS installation                                                                            |

L2 may use a disposable Linux guest matching CachyOS as closely as practical; **never presume Ubuntu**. If a clean guest is unavailable, label those tests `BLOCKED_ENVIRONMENT` and continue other useful L0/L1 checks. Do not repurpose HackLab Kali.

## 2. Safe initial Codex procedure on CachyOS

1. Confirm the host is the intended user's CachyOS system. Work on the **existing docs planning branch**, check dirty worktrees and ownership before modifying anything. Do not make code or runtime changes.
2. Read [43](43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md), [44](44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md), [42](42-POST-REJECT-CORRECTIVE-PLAN.md), and this document.
3. Perform **read-only, nonsecret** checks of kernel version, distro/version, capabilities/features for Landlock/user namespaces, bwrap version/presence, systemd user-manager and cgroup availability, PTY and sandbox primitives, and filesystem mount/topology needed for future validation. Report absent tools without installing anything.
4. Only if a process can demonstrate the test uses _solely_ unique disposable files/paths, run minimal nonprivileged synthetic tests in a newly created 0700 temporary directory. No authentication material, no external network traffic, no real provider, no user/system service, no root.
5. For each result, record `OBSERVED_ON_CACHYOS_SYNTHETIC`, `READ_ONLY_HOST_FACT`, `BLOCKED_UNSAFE`, `BLOCKED_ENVIRONMENT` or `NOT_RUN`. Include actual host kernel, version, test directory class (not sensitive path), procedure, expected/observed, cleanup. Do not call documentation-only facts test passes.
6. Remove only disposable test files that the runner itself created and can positively identify. No recursive cleanup of an existing user directory.
7. Commit **only** a new Markdown report on a documentation branch or existing planning branch when no concurrent writer is touching it. No software changes, no merge.

## 3. What to prioritise

- A: Is there any genuine protected authority principal already configured? **Do not create one on the host.** Record what installation/consent would be required under ADR 0025's no-sudo current design.
- B: Which usable native containment mechanisms exist for _synthetic workers_ under this CachyOS kernel? Landlock/bwrap/user namespaces are candidates; no mere PID or displayed UID considered an independent security boundary.
- Mock provider authentication: a **fake** credential with fake in-place refresh must remain usable under the worker policy, or mark the target capability broken. No live provider credential access.
- The original Wyss filesystem TOCTOU concern can be assessed in isolated dummy directories only. Never race a real project/config path.
- Same-UID authority request forgery is a documented current **architectural limitation**, not permission to call real approval APIs on the user's live control.sock.
- Record likely missing **L2** tests of distinct protected UID, actual service startup, signed human approval, and crash/restore. Their absence does not block L0/L1 research.

## 4. Nonnegotiable guards

- Do **not** install Ubuntu or another distribution, create a VM, install packages, enable linger, edit KDE settings, service units or user accounts.
- Do **not** start Claude Code, Codex App Server or Hermes just for inspection, change their configs, parse secrets, alter native hooks, or change identity/permissions of user assets.
- Do **not** run sudo/polkit/systemd service changes on the real CachyOS host for any test.
- No uncontrolled network listeners, open firewall ports, unattended daemons or changes to Sarmiento.
- Stop immediately if a test requires those effects or if its temporary directory cannot be isolated.
- This work does not supersede the accepted ADRs, lift DN-16 or reverse REJECT.

## 5. Expected deliverable

A _single_ read-only/native-safe verification report, `docs/architecture/reviews/2026-10-14-cachyos-native-feasibility.md`, or collision-free variant. Include (1) installed capability matrix, (2) tested synthetic successes/denials, (3) provider/OS compatibility still unknown, (4) experiments that really require isolation, (5) remaining owner-only choices. Never claim that an unprivileged synthetic success provides production approval provenance.

**Next action for the user:** none until Codex requires a real separately approved privileged host change or the architecture trade-off is ready. No mandatory VM, and definitely no Ubuntu.
