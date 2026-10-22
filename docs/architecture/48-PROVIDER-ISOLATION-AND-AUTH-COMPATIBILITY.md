# 48 — Native provider isolation: documented credential and capability compatibility

> **2026-10-22 consolidation:** this document retains dated design/evidence and detailed
> contracts. Its present-tense status/decision tables describe their stated baseline,
> not necessarily current implementation. Read the [current product set](../product/README.md),
> [verified gaps](../product/implementation-status.md) and [requirement crosswalk](../product/traceability.md)
> first. F8 is active P2/V8, ADR0051 Accepted, cap1; P1 remains available. New product
> direction does not supersede accepted security ADRs or turn proposed tests into PASS.

Date: 2026-10-14. **Status: READ-ONLY SOURCE RESEARCH / unverified native provider behaviour.**
No installed provider binary was launched; no user's `auth.json`,
`.credentials.json`, keyring, settings, hooks, profile or Docker
socket was opened. This document cannot be used to claim a
working sandboxed Claude, Codex, Hermes or OpenCode session.
The owner wants intact everyday native features on CachyOS, **not a
sandbox that only passes by disabling normal tool functionality**.

Source and target: [47](47-CACHYOS-DOCKER-PRIVILEGE-AND-SANDBOX-DECISION.md),
[native CachyOS synthetic report](reviews/2026-10-14-cachyos-native-feasibility.md),
[43](43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md),
accepted [ADR 0018](../adr/0018-core-hardening-and-completion.md).

## 1. Source-supported facts — no real credentials touched

| Native agent            | Authentication/config fact, provenance                                                                                                                                                                                                                                    | What it means for B sandbox                                                                                                                                                                                                            | Unknown / blocker                                                                                                                                                                                                      |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Codex managed**       | Accepted ADR 0018 §Credential capability describes _pinned Codex 0.159.0_ saving `auth.json` **in-place with truncate**, using a validated file-descriptor bind; managed local-tools profile already used `bwrap`                                                         | A narrowly bound single file can support **that observed managed** refresh mechanism, matching CachyOS's dummy in-place test                                                                                                           | Future/native version may differ. Managed profile's no-network local-tools process is not equivalent to full native Codex. Cannot treat ADR 0018 as proof of any new native OL3 profile                                |
| **Claude Code**         | Anthropic's official authentication docs state **Linux stores credentials under `~/.claude/.credentials.json` mode 0600**, or under `CLAUDE_CONFIG_DIR`; that directory also houses settings, sessions, plugins. They document login/profile and possible auth precedence | A separate config directory is officially supported as a **separate account/config context**, not automatically a no-copy use of the current login; simply reading token bytes does not prove a safe refresh path                      | Is refresh open/truncate or rename? Which auxiliary files/keychain/helper and runtime sockets are needed for the installed version? **UNVERIFIED**; no real sign-in, copying, re-login or secret inspection authorised |
| **OpenCode**            | Official docs locate provider credentials in `~/.local/share/opencode/auth.json`, with provider login, project/environment credentials and merged config sources. Server mode can expose network and events                                                               | User-provided auth config must be narrowly mounted, and optional environment/provider authentication is **not** permission to forward secrets through the Console. Project/global precedence and writable storage are version-specific | Does auth refresh replace file? Does the actual installed OpenCode version need session cache or socket server? **UNVERIFIED**; any FFI/server or broad auth-dir bind needs review                                     |
| **Hermes native**       | Prior pinned source review [40](40-HERMES-BOUNDARY-STATIC-EVIDENCE.md) shows Python code execution backend and configurable approval bypass modes; Hermes planner under Wyss Agent is separate from unrestricted native Hermes                                            | Native Hermes may need more tool/process/network access; the Wyss Agent _planner_ persona must not share that trust profile                                                                                                            | Provider-specific persistent auth, hooks, external integrations, effective tool registry and model inference flow **UNVERIFIED**                                                                                       |
| **Original Wyss Agent** | Pinned original Wyss source in 40 §6 has `ShellAdapter` **empty allowlist by default**, further execution guard and filename-based write with potential source-level TOCTOU concerns                                                                                      | A closed typed effect vocabulary is possible, but shell denial alone is not a proven functional planner/executor                                                                                                                       | Need positive and adversarial tests of typed effects, directory descriptor handling and protected-approver provenance                                                                                                  |

Sources, with reported evidence levels:

- Anthropic official [Claude Code authentication](https://code.claude.com/docs/en/authentication):
  **documented current upstream**, not pinned installed binary or observed login.
- Anthropic official [Claude Code settings](https://code.claude.com/docs/en/settings),
  [environment variables](https://code.claude.com/docs/en/env-vars):
  provider official docs. `CLAUDE_CONFIG_DIR` is a supported override,
  **not** a guarantee that copying credentials or omitting mandatory controls is safe.
- OpenCode [providers](https://docs.opencode.ai/docs/providers/),
  [CLI](https://docs.opencode.ai/docs/cli/),
  [config](https://docs.opencode.ai/docs/config/):
  provider docs, and actual installed OpenCode version not resolved.
- [ADR 0018](../adr/0018-core-hardening-and-completion.md)
  is an _accepted and version-bound source/measurement_ for
  the existing **managed Codex 0.159.0**, not unrestricted native agents.
- [40](40-HERMES-BOUNDARY-STATIC-EVIDENCE.md):
  pinned Hermes/Wyss source read, not observed production execution.

## 2. What CachyOS synthetic tests actually proved

The [2026-10-14 CachyOS report](reviews/2026-10-14-cachyos-native-feasibility.md)
demonstrated only these fake workloads:

1. A user-namespace process is not a genuinely different host
   UID; the same host user's files may remain accessible.
2. Landlock ABI 7 denied a fake authority **file** but
   permitted an already exposed pathname Unix socket.
3. Bubblewrap omitted authority and Docker runtime
   paths and thereby prevented path-based socket connects.
   **Binding a live socket read-only still allowed connection.**
4. A synthetic credential **in-place** write worked
   with one-file access, while temp-file-and-`rename`
   refresh required write access to a **directory**.
5. A synthetic project write and read-only mandatory
   policy worked. No true CLI/TUI/provider network
   connection, MFA/OAuth refresh, MCP/plugin, SSH,
   Docker, KWallet or GUI feature was tested.

No conclusion that the real provider does (or doesn't)
use any of the two refresh mechanisms is justified
without exact-version source/observed compatibility.

## 3. Candidate B capability tiers (NOT approved)

| Tier   | Owner-visible label                | Authority or allowed effects                                                                                                                                                     | Position                                                                                                                                  |
| ------ | ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| **U**  | Native, full-account session       | Normal CLI/TUI under owner's host identity, **including current delegated rootful Docker privilege if still accessible**                                                         | Expected for manual sessions; managed authority cannot honestly claim unforgeable approval against compromised U under current host model |
| **B1** | Restricted OL3 worker (candidate)  | Rootless bubblewrap mount view + Landlock 7 as secondary file/signal guard, dedicated writable project, verified mandatory policy, **provider-specific minimal auth capability** | Synthetic positive/negative evidence only; not approved until full native provider compatibility and no authority/Docker IPC reach proven |
| **M**  | Managed Codex authority (existing) | Approved managed turn/tool policy under accepted ADR 0012/0015/0018; pinned existing runtime isolation                                                                           | Separate established managed runtime with its **own** boundary and limits, not B1 or U                                                    |
| **P**  | Wyss Agent Hermes planner          | Plan proposals only; no effectful tooling without deterministic Wyss policy and independently enforced OS boundary                                                               | High-priority integration blocker; do not infer from B1                                                                                   |

**No single tier inherits another tier's approval merely
because the same provider binary or model name is used.**
A handoff of LLM text into U/B1 can induce execution,
so actual context and project hooks/commands require
independent review according to risk.

## 4. Credential transport/refresh threat table

| Hazard                                         | Why it matters                                                                                              | Required future proof                                                                                                                                 |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Writable bind of entire provider home          | Exposes auth _and_ hooks, sessions, plugins, MCP, model caches, native controls to the worker               | Exact versioned inventory of mandatory files; deny broader mounts, don't disable user-required controls                                               |
| File-only auth bind where provider uses rename | Fake provider tests showed EBUSY/EACCES                                                                     | Confirm actual provider version/write behaviour; transparent unsupported reason until safely addressed                                                |
| Bind a dedicated auth directory                | Allows atomic replacement, but all sibling files there can be edited by B1 and shared session can race      | Prove directory contains **only** necessary auth material, allowed files, no symlink/hardlink escape, safe sync with real provider; otherwise BLOCKED |
| Copy auth to a new isolated home               | Token refresh may desynchronize credentials or create a second unreviewed secret store                      | Explicitly prohibited as unproven fallback under DN-14; do not silently copy                                                                          |
| API key or session token in process env        | Child/plugin/process-inspection/diagnostics could leak it                                                   | Only consider a documented provider-native channel with safe environment, lifetime and redaction; no silent secrets in event logs                     |
| User keyring/KWallet/desktop bus               | B1 may need login/refresh or user event helpers; exposing DBus/X11 may reintroduce authority spoof channels | Precisely identified broker and confined IPC proof, not global session bus mount                                                                      |
| External model inference                       | `bwrap --unshare-net` alone cannot perform provider API requests                                            | Explicit egress design plus provider TLS/DNS and no arbitrary socket bypass; test positive model mock, then real only with owner permission           |
| Rootful Docker delegation                      | A UID 1000 native session can reach rootful Docker even if `CapEff=0`                                       | Preserve Docker only under a reviewed separate environmental trust model; B1 excludes Docker socket and any alternate daemon/API path                 |

## 5. Next no-secrets verification requests (do not execute on user accounts)

**TV-PROV-01**: static version-pin of installed `claude`,
`codex`, `opencode`, `hermes` executables **without
launching a provider or reading auth**; compare with official
source release/CLI config contracts. Absence is normal.
**READ_ONLY**, only on separately named CachyOS runner.

**TV-PROV-02**: run a deliberately synthetic mock provider
that performs launch, PTY, bounded project Git edit,
read-only required policy, both fake token refresh forms,
explicit model egress through a fake loopback-only
service, and clean stop. Outside owner files; no
actual provider binary. **NOT RUN**.

**TV-PROV-03**: deny connected fake authority, fake Docker,
host session bus, X11 and inherited file descriptors
from that worker; also check attempts via `/proc`,
abstract/path sockets, symlink and malicious tool plugins.
**NOT RUN**.

**TV-PROV-04**: compare actual provider capabilities only
after exact-version evidence and separate permission for
noninteractive disposable runs. **NOT AUTHORISED** to
use existing subscriptions/tokens, break interactive
sessions or modify `CLAUDE_CONFIG_DIR`.

**TV-PROV-05**: exact provider authentication semantics
on an independently credential-free disposable mock;
if unavailable, mark **UNKNOWN**, not a provider-compatible PASS.

## 6. Feasibility statement

The currently supported limited conclusion is:
**bubblewrap + Landlock on native CachyOS can restrict
a synthetic OL3 worker while keeping a tiny fake
project + fake token flow working**.
There is no verified whole-feature restricted
Claude/Codex/Hermes/OpenCode product profile and
no nonforgeable native approval architecture yet.
The administrator-capable host user's rootful Docker
access is a separate and more fundamental A/S1/S2
blocker; changing Docker is **not** a pre-approved
dependency or instruction.

## 7. Exact positive/negative fixture design for TV-PROV-02/03

[49](49-CACHYOS-COMPOSED-WORKER-SECURITY-CONTRACT.md)
specifies FX-B1-A..E as the next **synthetic**
CachyOS verification target. The mock must perform
nontrivial project, PTY, required policy and fake
refresh operations while denying authority and
privileged brokers; its inference call requires
a _real narrow fake egress_, not a total network
blackhole labelled provider-compatible.
No actual Claude/Codex/Hermes/OpenCode binary,
user auth or real provider process is authorised.
