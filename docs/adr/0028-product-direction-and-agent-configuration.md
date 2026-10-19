# 0028 — Product direction: two execution planes and real agent configuration

Status: **Accepted** on 2026-10-12. The owner answered its ten decisions (see
[Decisions](#decisions)) and its compatibility with ADRs 0001 to 0027 was checked against
their text (see [Compatibility](#compatibility-with-adrs-0001-to-0027)). This closes phase F0.
Nothing in it is implemented.

Date: 2026-10-12

## Context

Four documents described Wyss Console and they did not agree: an architecture research report
(2026-10-01: a control kernel, the terminal last), the base specification v0.1 (2026-10-05:
terminal first, a process supervisor and three agents early), ADRs 0001 to 0027 (what was
built: one deep vertical of securely managed Codex) and a vision report (2026-10-12: a terminal
plus visual control of rules, hooks and several agents). None said which one rules.

What exists at `1ad90ee`:

- Human terminals whose backend starts the account's shell and nothing else
  (`src-tauri/src/terminal/worker.rs:79-82`). An agent's CLI can be started in one by hand;
  the Console neither starts it nor knows it is there.
- One managed Codex runtime, in one profile and for one workspace per generation
  (`src-tauri/src/control/runtime.rs:84-98`), with one managed turn at a time
  (`src-tauri/src/control/turn_store.rs:204-206`).
- Managed Codex that deliberately does not run the user's methodology. Local Tools hides the
  user's Codex home behind a private one
  (`src-tauri/src/adapters/codex/probe/isolation.rs:428-505`). Inference reads the user's real
  configuration and refuses to start unless every external hook is disabled
  (`src-tauri/src/adapters/codex/safety.rs:328-348`).

The owner's decision of 2026-10-12: Wyss Console must read and change the real global
configuration files of the installed agents, visibly and in both directions, and must keep the
managed execution boundaries exactly as they are. Global editing and independent per-session
profiles are two features and both are wanted. Security controls stay under his control and
never change silently.

One fact makes this more than a new view. ADR 0015 records that the owner disabled his four
Codex hooks outside Wyss because managed Inference rejects any enabled hook
(`0015-first-managed-turn-and-streaming.md:142-147`). His Codex hook list is empty today for
that reason. Controlling Codex hooks from the Console and running managed Inference are, as
built, in conflict.

## Decision

### 1. Two execution planes, never presented as one

- **Native plane.** An agent's own CLI in a Console terminal, with the user's real global and
  project configuration or with a session profile. The agent's own permissions and hooks decide
  what it does. Wyss Console configures and observes it; it does not mediate it.
- **Managed plane.** Codex under `wyss-control`, unchanged.

Every session shows its plane and the configuration it actually uses. Editing the global
configuration never makes a managed session claim to use it.

A native session is a human-operated terminal process, not a managed agent in the sense of
ADR 0002. The Console grants it nothing, enforces nothing on it and never presents it as
contained: what it may do is decided by the agent's own permissions and by the user who
started it. ADR 0002 requires an external authority before any managed-agent execution,
"including terminal/L0 fallback" (`0002:63-70`), and ADR 0008 repeats it for PTY fallback
(`0008:197-200`). That stands: the day the Console is to hold any authority over a session in
a terminal (a lease, an approval, a containment), that session belongs to the managed plane
and to the authority.

### 2. A configuration subsystem outside the authority

A Rust library (`src-tauri/src/agentconfig/`) hosted by the desktop backend, with no dependency
on Tauri or on `wyss-control`. The authority gains no access to user configuration; the
subsystem has no access to the control socket or to the authority's state. The window reaches
it through narrow commands that carry an item identifier and an operation, and never a path,
file content or command text.

Each adapter reaches only the files of its own provider, from a closed list it derives itself:
there is no general file operation in the subsystem. Its interface is plain typed requests and
answers, with no window, Tauri or socket type in it, so that it can later sit behind a service
on the host. No remote architecture is built now.

### 3. Provider adapters that state what they can really do

One configuration adapter per agent. For each operation it declares one of `native`,
`by_removal`, `restart_required`, `read_only`, `unverifiable`. What was read on 2026-10-12 in
the providers' own documentation and in the installed binaries:

| Fact                                        | Claude Code 2.1.296                            | Codex 0.159.0                                             |
| ------------------------------------------- | ---------------------------------------------- | --------------------------------------------------------- |
| Per-hook switch                             | None: an entry is deleted or all hooks are off | Yes: `hooks/list` reports `enabled` for each hook         |
| A changed hook                              | Reloaded                                       | Skipped until the user trusts it again (trust is by hash) |
| Settings edited while a session is open     | Reloaded, except `model` and effort            | Not documented                                            |
| Per-session configuration without a rewrite | `--settings`, `--setting-sources`              | `--profile` (`~/.codex/<name>.config.toml`), `-c`         |

Consequences: disabling one Claude Code hook means removing its entry and keeping it elsewhere
so that it can be put back; Wyss Console never writes Codex's hook trust; only for Claude Code
may a change be reported as adopted by an open session.

Order of integration (decision D9): Claude Code, Codex, OpenCode, Hermes. The Codex
configuration adapter is a new, separate piece: managed Codex (`src-tauri/src/adapters/codex/`,
ADRs 0009 to 0018) is neither duplicated nor changed by it, and its read-only decoding is
reused where it serves. Hermes is integrated as the original upstream agent; no fork of it is
started or maintained here.

### 4. Four scopes and three states

Scopes: system (read only), global, project, session. A session profile is applied when the
session is started, through the provider's own mechanism, and never writes a global file.

States, always shown apart: **declared** (what the file says), **effective** (what the provider
resolved, where it can be asked) and **running** (what each open session uses). An item is
never shown as active because a file says so.

### 5. One mutation pipeline

plan (pure) → show scope, redacted diff and affected sessions → confirm → check the file is
still the one planned against → keep what undoing needs → write a new file and rename it over
the old one → read back and verify → journal → report per session.

- **Round-trip precondition.** Before a file is edited, parsing and writing it back unchanged
  must reproduce it byte for byte; otherwise that file is not edited. The diff of a change is
  then the change and nothing else. The rule is checked on every file at the moment of editing
  it, because no sample proves it for all files: that both of the owner's Claude Code settings
  files round-trip exactly (read on 2026-10-12) is evidence, not proof. Format and comment
  preservation are tested per provider with fixtures of their own.
- **Risk is decided and enforced in the backend.** The backend classifies every plan. A plan
  that weakens a security control, or that could introduce executable content, is applied only
  after a native dialog that the backend itself opens and that the page cannot answer, as the
  project folder already is (ADR 0021). The page cannot declare a plan confirmed. An ordinary
  change keeps one confirmation in the page and no more.
- **What that check is.** It protects the user's configuration from a compromised page and
  from a slip. It is not a boundary against the user or against another process of his, which
  can edit the same files directly; ADR 0018 already says the core has no protection there
  (`0018:110-121`). It decides nothing about managed sessions, turns, leases or approvals.
- **First, existing entries only.** The first mutations switch, remove and restore entries that
  already existed, byte for byte. This is the order of implementation and not a limit of the
  product: creating and editing rules, hooks, skills and settings follow in a later phase, as
  far as each provider really allows. They wait because a hook runs commands: letting the page
  author one would turn a compromised page into command execution, so authoring needs a
  contract of its own for executable content.
- Where the provider has its own writer (`claude mcp`, `codex mcp`) it is used. `~/.claude.json`,
  which Claude Code rewrites for itself, is never edited directly.
- Files are classified by owner: user, system, provider-owned, generated, link. A link is
  followed to a regular file of this user and never replaced; a generated file is read only.

### 6. No change to global hooks may break managed Inference silently

Managed Inference requires zero enabled external Codex hooks (ADR 0015). It already starts
Codex with `features.hooks=false` among its fixed overrides (`0015:40-55`); a system policy can
pin hooks on, as this machine's `/etc/codex/requirements.toml` does, which is why the hook
definitions themselves are counted (`0015:145-146`). Therefore:

1. **Shown at rest.** The Configuration view states, on the Codex hooks themselves, that
   managed Inference needs all of them disabled, and whether that holds now.
2. **Predicted before applying.** Every plan against Codex's global or project configuration is
   evaluated for its effect on managed Inference. The prediction is conservative: a plan that
   leaves any external hook declared and enabled is reported as making Inference unavailable.
3. **Refused until resolved.** Until gate **G-INF** below is met, an operation predicted to make
   Inference unavailable is not offered: the adapter reports it as blocked, with that reason.
4. **Named afterwards.** When Inference is unavailable and enabled hooks are declared, the
   window says so in those words. Today the authority answers one generic code
   (`inference_safety_unproven`, `safety.rs:86`); the cause is added by the desktop from what
   the configuration subsystem knows, with no change to the authority.

**Gate G-INF.** The intended resolution (decision D7) is decoupling: managed Inference on a
private Codex home, as Local Tools already has, so that native Codex can run the owner's real
global hooks while managed Inference keeps working, independently of them.

That decision authorises research and security design. It does not authorise changing how
Inference is built. Until the isolation design, its compatibility with the provider and its
safety attestation have been verified and accepted in an ADR of their own:

- the authority's check stays exactly as it is (`safety.rs:328-348`);
- enabling a Codex hook is not offered from the Console (point 3);
- there is no fallback that runs Inference with a weaker check, and no way for it to become
  unavailable without the window saying so first.

The starting point of that research is what Local Tools already does: a private Codex home,
the user's home hidden, an authority-owned requirements directory in place of `/etc/codex`, one
validated credential capability and an attestation repeated before every turn (`0016:37-65`).

Considered and not chosen: an explicit trade, in which the owner enables hooks knowing that
managed Inference stops until they are disabled again.

Claude Code's hooks have no such coupling: no managed runtime reads them.

### 7. Two computers that share configuration

The owner's `~/.claude/` is replicated between two computers; Wyss Console's private state is
not. A hook removed on one computer disappears on the other, which has no parked copy.

- **Parked entries stay in the private state of the computer that parked them** (decision D5).
  A parked entry keeps what an exact restoration needs and no more: its bytes, the file and
  scope it came from, its event and matcher group, its position among its neighbours, the hash
  of the file before and after, and when it was parked.
- **Restoration data is kept to the entries that can be restored.** No whole configuration
  file is copied: the old file stays in place until the new one is renamed over it, so a
  write needs no copy to be safe. Values of environment variables, credentials and any field
  an adapter classifies as secret are never kept.
- **Update 2026-10-19 ([ADR 0029](0029-encrypted-restoration-store-and-secret-bearing-entries.md)):**
  values and secrets may be kept in restoration data, sealed with authenticated encryption under
  a key held by the OS credential store; the previous bullet's "never kept" no longer applies.
- **What is kept may still hold a secret** (a command with a token written into it) and is
  treated so: a private directory of this user, files readable by him alone, a bounded number
  kept, never copied into the history and never placed where it would be replicated.
- **A file that has diverged is never overwritten by itself.** A restoration is planned against
  the file as it is now; if the entry is already there it is not added twice, and if what is
  there contradicts it the two are shown and nothing is written.
- **Every computer keeps its own last-seen entries.** The configuration subsystem keeps, per
  file, the restorable entries it last observed there. When a file changes from outside and an
  entry is gone, that computer can restore it from what it last saw, without anything
  replicated.
- **When there is no local copy** (the Console never saw that file there), the view says:
  disabled on another computer, no local backup. The entry can be carried over by an explicit
  export on one computer and an import on the other; nothing is replicated by itself.
- **Reconciliation.** An entry restored on one computer reappears on the other as an outside
  change; the parked record there is marked restored elsewhere and is not applied again.
- A Syncthing conflict copy of a file blocks changes to that file until the user resolves it.
- What a change does to sessions open on the other computer cannot be known and is said so.

### 8. Honest concurrency

A plan is bound to the hash of the content it was computed from; applying it to changed content
fails without writing. A per-target lock serialises the Console's own writers. A provider
writing between the last check and the rename cannot be excluded: it is detected on read-back
and recoverable from the backup.

### 9. A configuration history of its own (decision D8)

Configuration changes are recorded in a journal that belongs to the configuration subsystem and
is separate from the authority's event stream (ADRs 0003 and 0017), which stays the record of
managed execution. The journal holds, per entry: the item, the operation, the scope, the hash
of the file before and after, the time, the outcome, and the conflicts and outside changes that
were observed. It holds no file content, no value of any setting and no secret. It is what undo
reads, and it has its own list in the window.

### 10. Dependencies (decision D6)

| Need                                  | Chosen                                               | New crates | Why                                                                                               |
| ------------------------------------- | ---------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------------- |
| Edit JSON keeping key order           | `serde_json`, feature `preserve_order`               | 0          | `indexmap` is already in `Cargo.lock`; exact round trip seen on two real files, enforced per file |
| Edit TOML keeping comments and layout | `toml_edit` as a direct dependency                   | 0          | Already in `Cargo.lock` at 0.25.15 as a transitive dependency; maintained by the `toml` authors   |
| Notice outside changes                | `nix`, feature `inotify`                             | 0          | `nix` 0.28 is already a direct dependency; avoids the `notify` crate and its tree                 |
| Edit JSONC keeping comments           | Deferred to the OpenCode phase (`jsonc-parser` then) | 1, later   | Only OpenCode needs it; its file here has no comments today                                       |

Nothing is added before the phase that uses it.

### 11. Which document rules (decision D10)

[`docs/PRODUCT.md`](../PRODUCT.md) states the approved goals; a specification under
`docs/spec/` states the functional contract of a feature; ADRs record technical decisions and
which one supersedes which; the README describes what is implemented and verified. A goal in
the product page does not by itself override an established security control: changing one
takes an explicit technical decision and proof.

## Compatibility with ADRs 0001 to 0027

Each ADR is classed as one of: **Preserved** (stands without modification), **Extended** (0028
adds beside it), **Superseded in one area** (a named statement no longer holds in a named
place; everything else in that ADR stands) or **Unaffected** (no contact). No accepted ADR is
rewritten. No guarantee of the managed plane is changed.

| ADR  | Statement that matters here                                                                                                                                                                                                | Class                  | What follows                                                                                                                                                                                                                                                                                                         |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0001 | Skills, permissions and presets deferred (`:43-47`)                                                                                                                                                                        | Extended               | The deferral ends for configuration in the native plane                                                                                                                                                                                                                                                              |
| 0002 | The Console is a client, not the security authority (`:9-10`); an external authority before any managed-agent execution, "including terminal/L0 fallback" (`:63-70`); permissions and gates are the authority's (`:28-46`) | Preserved              | Section 1 defines a native session inside these terms. The configuration subsystem is Console-local state and holds no permission, lease, approval or gate                                                                                                                                                           |
| 0003 | "Secrets and credentials never enter durable storage" (`:69-72`); local diagnostics apart from control audit (`:55-58`)                                                                                                    | Superseded in one area | Stands for every event, for the authority's stream and for the configuration history, which hold no secret. It does not extend to restoration data (section 7): a user's own configuration entry, kept so that it can be put back, may contain a secret he wrote into it. Decision D5, minimised as section 7 states |
| 0004 | "Configuration projection" deferred (`:43-44`); model provider and agent adapter (`:27-33`)                                                                                                                                | Extended               | A third category: configuration adapter                                                                                                                                                                                                                                                                              |
| 0005 | The frontend supplies no executable, argv, environment (`:49-51`); no arbitrary spawn (`:141`)                                                                                                                             | Preserved              | Limits F3: starting an agent in a terminal needs its own ADR, with a closed catalogue resolved in the backend                                                                                                                                                                                                        |
| 0006 | The frontend cannot choose an executable, argv or environment (`:86-87`)                                                                                                                                                   | Preserved              | Same                                                                                                                                                                                                                                                                                                                 |
| 0007 | Profiles and persistent sessions deferred (`:86-88`)                                                                                                                                                                       | Preserved              | A registry of native sessions comes with the F3 ADR, not with this one                                                                                                                                                                                                                                               |
| 0008 | `hooks` and `mcp_servers` are observe-only (`:91`); capability is not authority (`:14-15`); authority before PTY fallback (`:197-200`)                                                                                     | Preserved              | Configuration capabilities are a separate declaration. No `request` is added to 0008's vocabulary                                                                                                                                                                                                                    |
| 0009 | The Codex codec is offline and starts nothing (`:14-17`)                                                                                                                                                                   | Unaffected             | Its decoding may be reused to read                                                                                                                                                                                                                                                                                   |
| 0010 | The probe reads and changes no configuration file (`:14-16`)                                                                                                                                                               | Preserved              | True of the probe. A live read of Codex's effective state from the desktop is not part of F1 and needs its own ADR                                                                                                                                                                                                   |
| 0011 | Only the authority commits control state (`:8-11`); no generic process or environment in its API (`:34-36`)                                                                                                                | Preserved              | No control method is added; the authority is given no configuration access                                                                                                                                                                                                                                           |
| 0012 | One shared runtime (`:8`); Tauri neither starts nor connects to it (`:11`); "Wyss never reads token files, logs in, modifies Codex config" (`:111-113`)                                                                    | Superseded in one area | The authority and the runtime still never do. As a statement about the whole product, "never modifies Codex config" ends when the configuration subsystem first writes one, at the user's explicit request (F2b). Nothing else changes                                                                               |
| 0013 | Execution denied by default; leases with exact scopes and a TTL (`:8-13`, `:24-31`)                                                                                                                                        | Preserved              | The configuration subsystem neither holds nor reads a lease                                                                                                                                                                                                                                                          |
| 0014 | One permanent binding per session; the client cannot override workspace or thread (`:8-21`)                                                                                                                                | Unaffected             | —                                                                                                                                                                                                                                                                                                                    |
| 0015 | One turn at a time (`:9-10`); zero enabled hooks, unknown is a rejection (`:138-139`); "Wyss does not edit the user's hooks, authentication or system policy" (`:147`)                                                     | Superseded in one area | The check and the single turn stand. As a statement about the whole product, "does not edit the user's hooks" ends with F2a for Claude Code and stays in force for Codex until G-INF. Authentication and system policy are never edited, by any part                                                                 |
| 0016 | Local Tools on a private home, no user configuration loaded (`:43-46`); the profile proved again before every turn and Accept (`:60-65`)                                                                                   | Preserved              | The view says managed Local Tools does not use the global configuration. It is the template for F1b                                                                                                                                                                                                                  |
| 0017 | The stream is audit and history, not the authority (`:13-16`)                                                                                                                                                              | Preserved              | Configuration history is not written to it                                                                                                                                                                                                                                                                           |
| 0018 | The managed core is complete (`:8-11`); no protection against another process of the same user (`:110-121`)                                                                                                                | Preserved              | The configuration subsystem is such a process and gains nothing the trust model had withheld                                                                                                                                                                                                                         |
| 0019 | "The desktop makes no authorization or security decision" (`:22-24`)                                                                                                                                                       | Superseded in one area | Stands for everything managed. In one place it no longer holds: the desktop backend validates and authorises changes to agent configuration files (decision D4, section 5), a domain the authority has never owned                                                                                                   |
| 0020 | The desktop requests and the authority decides (`:20-23`); session intents are "the only" mutations it sends (`:10`)                                                                                                       | Preserved              | Said of control intents. A configuration write is not one and never reaches the authority                                                                                                                                                                                                                            |
| 0021 | "The window is given no filesystem"; the dialog takes nothing from the page (`:94-96`)                                                                                                                                     | Extended               | The window is shown bounded, structured, redacted views of a closed list of provider files, and still names no path. The native dialog becomes the model for confirming a risky change                                                                                                                               |
| 0022 | Managed turns from the window; the authority decides every start and cancel                                                                                                                                                | Unaffected             | —                                                                                                                                                                                                                                                                                                                    |
| 0023 | Local Tools and approvals from the window; the authority holds every action                                                                                                                                                | Unaffected             | —                                                                                                                                                                                                                                                                                                                    |
| 0024 | Events shows what the authority recorded and adds no authority (`:27-30`)                                                                                                                                                  | Preserved              | Configuration history has its own list                                                                                                                                                                                                                                                                               |
| 0025 | Safe paths and atomic install for the files the desktop writes (`:142-171`)                                                                                                                                                | Preserved              | The same rules are the model for configuration writes; nothing of the bootstrap changes                                                                                                                                                                                                                              |
| 0026 | "Preference is not authority"; a program or environment for a terminal is never a preference; anything with security significance needs its own phase (`:33-56`)                                                           | Preserved              | Configuration is a different view with a different contract. It is not added to Settings                                                                                                                                                                                                                             |
| 0027 | No distribution format; a new one is a new decision (`:18-70`)                                                                                                                                                             | Preserved              | No path, user name or host is fixed in code. No packaging work follows from this ADR                                                                                                                                                                                                                                 |

Read directly for this matrix: 0001 and 0004 in full; 0002 `:7-73`; 0003 `:7-36` and
`:53-77`; 0005 `:7-25`, `:47-56`, `:136-146`; 0006 `:7-21`, `:84-90`; 0007 `:7-20`, `:84-90`;
0008 `:6-25`, `:78-100`, `:187-215`; 0009 `:6-38`; 0010 `:6-25`; 0011 `:6-40`; 0012 `:6-50`,
`:104-131`, `:180-200`; 0013 `:6-40`; 0014 `:6-21`; 0015 `:6-19`, `:40-62`, `:120-148`;
0016 `:6-19`, `:36-66`, `:105-116`; 0017 `:6-24`; 0018 `:6-25`, `:110-127`; 0019 `:6-40`;
0020 `:6-34`; 0021 `:6-22` and the passages about the dialog; 0022, 0023 and 0024 `:6-34`;
0025 `:6-23`, `:142-171`; 0026 `:11-60`; 0027 in full. The remaining sections of the long
ADRs (0017, 0018, 0021, 0023, 0025) were searched for statements about user configuration,
hooks, the filesystem and the window's permissions, and not read line by line.

`README.md:848` ("Wyss never edits them") is corrected to speak of the authority when the first
mutation ships, not before.

## Roadmap

Each phase leaves the terminal and managed Codex exactly as they are and can be the last.

| Phase | Delivers                                                                                                                                                           | Before it                                                    | Accepted when                                                                                                                                             |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F0    | **Done.** This ADR, `docs/PRODUCT.md`, the matrix above                                                                                                            | —                                                            | The owner accepts this ADR                                                                                                                                |
| F1    | Read-only inventory for Claude Code and Codex; the Configuration view; outside changes and Syncthing conflicts noticed; section 6 point 1                          | F0 accepted and F1 authorised                                | Fixture inventories match exactly; no secret value reaches the page; **no file is written**                                                               |
| F1b   | Research and security design only: managed Inference on a private Codex home. A written contract, its compatibility tests against the provider and its attestation | F1                                                           | A report and a draft ADR. The Inference that exists is not changed                                                                                        |
| F2a   | Mutations with no managed coupling: Claude Code skills, plugins, all-hooks switch, hook parking and restore; backups, history and undo                             | F1                                                           | A file broken on purpose, an outside change mid-way and a simulated power cut all leave a valid file; undo is exact; a diverged file is never overwritten |
| F2b   | Mutations of Codex configuration, each with its effect on managed Inference predicted                                                                              | F2a; section 6 points 2 and 4                                | Enabling a hook is blocked with its reason                                                                                                                |
| G-INF | Managed Inference on a private home; then enabling Codex hooks from the Console                                                                                    | F1b; its own ADR accepted; its tests and attestation passing | Native Codex runs with the owner's hooks while a managed Inference turn completes; the isolation tests that exist still pass                              |
| F3    | Starting an agent in a terminal, a registry of native sessions, session profiles                                                                                   | Its own ADR (0005 to 0007)                                   | Two sessions with different profiles at once and no global file changed                                                                                   |
| F4    | OpenCode, then Hermes as its original upstream; read only first                                                                                                    | F1                                                           | As F1, per agent                                                                                                                                          |
| F5    | Creating and editing rules, hooks, skills and settings, as far as each provider allows                                                                             | F2a; a contract for executable content                       | To be written with that contract                                                                                                                          |

Real integration on the owner's configuration happens only with his explicit authorisation, per
phase. Everything else runs on synthetic fixtures and private homes.

## Decisions

Answered by the owner on 2026-10-12.

| #   | Decision                                     | Chosen                                                | Conditions the owner attached                                                                                                                                                                                                    |
| --- | -------------------------------------------- | ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | The definition of the product                | Two execution planes                                  | Both kept; told apart in the interface                                                                                                                                                                                           |
| D2  | Where the configuration subsystem lives      | The desktop's Rust backend, apart from `wyss-control` | Narrow, provider-specific file operations; interfaces that could later serve a remote backend, with no remote architecture now                                                                                                   |
| D3  | The first mutations                          | Enable, disable and restore existing entries          | An order of implementation, not a limit: creating and editing follow, as providers allow                                                                                                                                         |
| D4  | A change that weakens security               | A mandatory native confirmation                       | Authorisation and validation enforced in the backend, not only in the page; ordinary changes stay convenient                                                                                                                     |
| D5  | Where disabled hooks are kept                | Private local backups, per computer                   | Enough content and metadata for an exact restoration; outside changes and Syncthing conflicts detected; a diverged file never overwritten by itself; a controlled export and import; backups treated as possibly holding secrets |
| D6  | Dependencies                                 | The approach of section 10                            | Existing dependencies and their features first; format and comment preservation validated with fixtures per provider; a round trip on two samples is evidence, not proof; JSONC only with OpenCode                               |
| D7  | Managed Inference and the user's Codex hooks | An isolated Inference home is the intended solution   | Research and security design only; the present protective check stays until design, provider compatibility and attestation are verified; no silent loss of managed Inference and no unsafe fallback                              |
| D8  | Configuration history                        | An independent audit and history                      | Edits, rollbacks, conflicts and drift kept apart from the managed execution stream; no secret in it                                                                                                                              |
| D9  | Order of agents                              | Claude Code, Codex, OpenCode, Hermes                  | The order of configuration integration; managed Codex neither duplicated nor regressed; Hermes as the original upstream, not a fork                                                                                              |
| D10 | Document hierarchy                           | Product vision, current specification, ADRs, README   | The product vision does not by itself override an established security control: that takes an explicit technical decision and proof                                                                                              |

## Not decided here

Several managed runtimes or turns at once; a managed profile that admits user rules; handing
work between agents; showing the model of a session; remote access; a personal assistant and
the relation between the original Wyss assistant and Hermes; distribution to other users.

## To be measured before acceptance of each phase

- Where Codex persists a hook disabled from its `/hooks` browser.
- The exact effect of `--setting-sources` on hooks, which Claude Code merges across levels.
- Whether Codex and OpenCode apply a configuration change to a running session.
- Whether Claude Code's effective settings can be read without an interactive session.
- Everything about Hermes, which is not installed.

## References

- [`docs/PRODUCT.md`](../PRODUCT.md).
- Claude Code settings and hooks documentation, Codex configuration and hooks documentation,
  OpenCode configuration documentation, read on 2026-10-12.
- ADRs 0001 to 0027.
