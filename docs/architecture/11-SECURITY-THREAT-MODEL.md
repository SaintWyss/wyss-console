# 11 — Security threat model

Domain K of [04](04-TARGET-ARCHITECTURE.md). Assets, boundaries, threats, controls, the
storage of restoration data and the handling of secrets. The managed plane's guarantees are in
[10 §2](10-MANAGED-RUNTIME-AND-SECURITY.md) and are not re-argued here. All `[P]` unless marked.

## 1. Assets

| ID  | Asset                                                                                   | Why it matters                                                 | Owner                            |
| --- | --------------------------------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------- |
| A1  | Provider credentials (`auth.json`, `.credentials.json`, `.env`, keys in MCP entries)    | Account takeover, spend                                        | The user; the provider           |
| A2  | The user's agent configuration (hooks, rules, skills, MCP, settings)                    | It decides what agents run and block                           | The user                         |
| A3  | The user's security controls among A2 (blocking hooks, permission rules, system policy) | Removing one silently removes a protection                     | The user; root for system policy |
| A4  | The managed plane's canonical state (leases, approvals, audit)                          | It decides what managed Codex may do                           | The authority                    |
| A5  | The user's projects and files                                                           | What agents and terminals can change                           | The user                         |
| A6  | Restoration data kept by the Console                                                    | It can contain what A2 contained, including an embedded secret | The Console, for the user        |
| A7  | The configuration history                                                               | The record of who changed what                                 | The Console                      |
| A8  | Terminal content                                                                        | Commands, output, possibly secrets typed or printed            | The user; never kept             |

## 2. Trust boundaries

Those of [04 §2](04-TARGET-ARCHITECTURE.md). In one line each:

| Between                                | Trust                                                                    |
| -------------------------------------- | ------------------------------------------------------------------------ |
| Page ↔ backend                         | The page is untrusted input: it may be compromised by content it renders |
| Backend ↔ filesystem                   | Paths are untrusted until checked; contents are untrusted data           |
| Backend ↔ configuration adapters       | Adapters are our code; what they parse is untrusted                      |
| Backend ↔ native agents                | No boundary: same user. The agent can do whatever the user can           |
| Backend ↔ authority                    | The backend is a client with no authority over managed work              |
| Authority ↔ managed runtime            | The runtime is contained by a profile it must prove                      |
| Console ↔ remote clients (future)      | Authenticated devices with scoped grants; the network is hostile         |
| Anything ↔ external providers          | Model output is untrusted text everywhere                                |
| Anything ↔ system administrator policy | Trusted, read only, never bypassed                                       |

## 3. Threats and controls

Likelihood and impact are judgements (L low, M medium, H high), not measurements.

| ID   | Threat                                                                                                     | Asset  | L   | I   | Controls                                                                                                                  | Residual risk                                                                                                     |
| ---- | ---------------------------------------------------------------------------------------------------------- | ------ | --- | --- | ------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| T-01 | A compromised page asks the backend to write an arbitrary file                                             | A2, A5 | L   | H   | CTL-01 no path from the page; CTL-02 closed list of targets per adapter                                                   | None by this route                                                                                                |
| T-02 | A compromised page adds a hook that runs a command                                                         | A2, A5 | L   | H   | CTL-03 no command text from the page; first phases only switch existing entries; CTL-04 native dialog for `executes_code` | Restoring a parked hook is still executable content: it takes the native dialog                                   |
| T-03 | A compromised page silently disables a security hook                                                       | A3     | L   | H   | CTL-04 native dialog decided by the backend; CTL-05 backend classifies risk; history                                      | A user who confirms without reading                                                                               |
| T-04 | A secret reaches the page (and from there a log, a screenshot, an extension)                               | A1     | M   | H   | CTL-06 credential files never opened; CTL-07 redaction before projection; corpus tests                                    | A secret written inside a field classified as non-secret (a command line) is shown as the user wrote it           |
| T-05 | A secret is persisted by the Console in restoration data                                                   | A1, A6 | M   | H   | §5: storage policy (DN-01)                                                                                                | Depends on the policy chosen                                                                                      |
| T-06 | A secret is persisted in the history or a diagnostic                                                       | A1, A7 | L   | H   | CTL-08 the journal holds identifiers and hashes only; diagnostics built from the journal                                  | None intended; test TS-SEC-02                                                                                     |
| T-07 | A symbolic link is swapped between the check and the write, redirecting the write                          | A5     | L   | H   | CTL-09 open once, judge by descriptor; CTL-10 ancestors checked; rename within the checked directory                      | A same-user attacker can do the write himself anyway (B5)                                                         |
| T-08 | A provider directory writable by another user lets him plant configuration                                 | A2     | L   | H   | CTL-10 path trust; such a directory is reported and never written                                                         | The provider itself still reads it: reported to the user                                                          |
| T-09 | An outside edit is overwritten by the Console                                                              | A2     | M   | M   | CTL-11 hash precondition; CTL-12 read-back                                                                                | The race of 07 §5.5: one ordering is detected; the other replaces the provider's own write undetected (24, RV-03) |
| T-10 | A crash leaves a configuration file half written                                                           | A2     | L   | H   | CTL-13 write beside and rename                                                                                            | None for the file; a restoration record may exist without its change (cleaned at start)                           |
| T-11 | The interface presents an instruction file as if it prevented an action                                    | A3     | M   | M   | CTL-14 rules are labelled "guidance for the model"; only provider-enforced controls are labelled as controls              | —                                                                                                                 |
| T-12 | The interface shows a change as active when the running agent has not adopted it                           | A3     | H   | M   | CTL-15 three states; reload rules only from documentation                                                                 | Unknown stays unknown                                                                                             |
| T-13 | A change to global Codex hooks silently disables managed Inference                                         | A4     | M   | M   | CTL-16 prediction, block and naming (ADR 0028 §6)                                                                         | None while the block holds                                                                                        |
| T-14 | The Console forges or edits a provider's trust record so that a changed hook runs                          | A3     | L   | H   | CTL-17 trust records are outside every adapter's write set                                                                | —                                                                                                                 |
| T-15 | A system policy is bypassed (by an isolated home, a namespace or an override)                              | A3     | L   | H   | CTL-18 system sources are read only; F1b prefers designs that keep policy visible (10 §5.4)                               | Local Tools already replaces `/etc/codex` in its namespace `[R]`                                                  |
| T-16 | An imported profile or restoration record brings in executable content or asks for more than the user sees | A2     | M   | H   | CTL-19 import is validated, shown and grants nothing; restoring always takes the native dialog                            | —                                                                                                                 |
| T-17 | A malformed or huge file exhausts memory or hangs the reader                                               | —      | M   | L   | CTL-20 bounds on size, count, depth, links, time                                                                          | —                                                                                                                 |
| T-18 | A native agent is believed to be contained because the Console started it                                  | A5     | H   | H   | CTL-21 the plane is always shown; no approval wording in the native plane                                                 | —                                                                                                                 |
| T-19 | Content produced by one agent, handed to another, carries instructions that the receiver follows           | A5     | M   | H   | CTL-22 handoff content is quoted material the user reviewed; a managed receiver gains no authority from it                | Prompt injection cannot be excluded by the Console                                                                |
| T-20 | A replicated configuration change removes a control on the other computer without anyone seeing            | A3     | M   | M   | CTL-23 outside changes are shown with what was removed; last-seen entries allow restoring                                 | Nothing is shown on a computer where the Console is not open                                                      |
| T-21 | Two agents write the same tree at once                                                                     | A5     | M   | M   | CTL-24 the overview warns; worktrees are suggested                                                                        | Not prevented                                                                                                     |
| T-22 | A remote device issues commands after it should have lost access (future)                                  | A4, A5 | —   | H   | 16 §5: device identity, scoped grants, revocation checked on every request                                                | Designed with the remote phase                                                                                    |
| T-23 | The configuration engine becomes a way to reach the authority's state or socket                            | A4     | L   | H   | CTL-25 no code path (C-02, C-03), held by a test                                                                          | —                                                                                                                 |
| T-24 | An adapter built for one provider version misreads a newer file and writes a wrong edit                    | A2     | M   | M   | CTL-26 mutations refused on unverified versions; round-trip precondition                                                  | A change of meaning without a change of format                                                                    |

### 3.1 Controls

| ID     | Control                                                                                 | Where                                  | Verified by |
| ------ | --------------------------------------------------------------------------------------- | -------------------------------------- | ----------- |
| CTL-01 | Requests carry item identifiers and operations only                                     | Command DTOs                           | TS-SEC-04   |
| CTL-02 | Targets come from a closed list the adapter derives                                     | Adapter `sources()`                    | TS-FS-01    |
| CTL-03 | No command, argument or environment text accepted from the page                         | Command DTOs                           | TS-SEC-04   |
| CTL-04 | Native confirmation for `weakens_security` and `executes_code`                          | Backend, GTK dialog                    | TS-SEC-05   |
| CTL-05 | Risk classification in the backend, from the plan, never from the request               | Planner                                | TS-SEC-05   |
| CTL-06 | Credential files are never opened                                                       | Adapters                               | TS-SEC-01   |
| CTL-07 | Secret classes redacted before any projection exists                                    | Adapter `redact`                       | TS-SEC-01   |
| CTL-08 | Journal entries hold identifiers, hashes, times and outcomes                            | Journal type                           | TS-SEC-02   |
| CTL-09 | A file is opened once and judged by that descriptor                                     | Reader, writer                         | TS-FS-03    |
| CTL-10 | Every ancestor is the user's or root's, not a link, not writable by others              | `control/paths.rs:20-44` `[R]`, reused | TS-FS-02    |
| CTL-11 | A plan is bound to a content hash                                                       | Engine                                 | TS-CFG-10   |
| CTL-12 | Every write is read back and verified                                                   | Engine                                 | TS-CFG-11   |
| CTL-13 | Write beside, flush, rename, flush directory                                            | Writer, as `settings/store.rs` `[R]`   | TS-CFG-12   |
| CTL-14 | Labels distinguish guidance from enforced control                                       | View                                   | TS-UX-05    |
| CTL-15 | Persisted, effective and running state shown apart; reload from documentation only      | Engine, view                           | TS-CFG-08   |
| CTL-16 | Managed-effect prediction; block; cause named                                           | Codex adapter, view                    | TS-CFG-15   |
| CTL-17 | Trust records excluded from every write set                                             | Adapter, planner                       | TS-CFG-14   |
| CTL-18 | System sources have no write capability                                                 | Adapter                                | TS-CFG-09   |
| CTL-19 | Imports are validated against the schema, displayed, and never applied without a plan   | Import                                 | TS-SEC-06   |
| CTL-20 | Bounds on every read                                                                    | Adapters                               | TS-CFG-05   |
| CTL-21 | Plane marker on every session; no "approved" or "contained" wording in the native plane | View                                   | TS-UX-04    |
| CTL-22 | Handoff content is quoted, reviewed and carries no authority                            | Handoff                                | TS-SES-03   |
| CTL-23 | Outside-change records list removed entries                                             | Watcher, view                          | TS-CFG-07   |
| CTL-24 | Same-folder warning                                                                     | Overview                               | TS-SES-02   |
| CTL-25 | `agentconfig` and `control` do not reference each other                                 | Build graph                            | TS-SEC-03   |
| CTL-26 | Unverified provider version → no mutation                                               | Engine                                 | TS-CFG-13   |

## 4. Authorisation of mutations

Risk is a property of the **plan**, computed in the backend from what the plan does.

| Class              | What falls in it                                                                                                                 | Confirmation                 |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------- | ---------------------------- |
| `normal`           | Switching a skill; switching off an MCP server or a plugin; switching off a non-blocking hook; a model preference                | One confirmation in the page |
| `weakens_security` | Disabling a hook that can block an action; `disableAllHooks`; loosening a permission rule; anything the user marked as a control | Native dialog                |
| `executes_code`    | Restoring or enabling a hook; switching on an MCP server or a plugin; later, creating or editing any of them                     | Native dialog                |
| `forbidden`        | Any write to a system source, a provider-owned file, a generated file or a trust record                                          | Never offered                |

The native dialog states the provider, the scope, the file, the entry, the effect on running
sessions and on managed execution. It has no "do not ask again" (ADR 0026 `:47` already rules
that out for preferences `[R]`). The page learns only whether the plan was applied.

What this is and is not (ADR 0028 §5 `[R]`): a safeguard against a compromised page and a
slip. It is not an access control against the user or his other programs.

## 5. Restoration data and secrets

### 5.1 The problem

Claude Code has no per-hook switch `[D]`. To disable one hook and be able to put it back, its
entry has to live somewhere while it is out of the file. A hook entry is configuration, but it
can hold a secret: a token written into a command line, a header of an `http` handler.

ADR 0003 says "Secrets and credentials never enter durable storage" (`0003:69-72`) `[R]`.
ADR 0028 records a supersession of that sentence for restoration data, on the basis of
decision D5 `[R]`. The owner has since asked that this not be taken as permission to keep
secrets, and that the options be compared before anything is built. **This section does that.
DN-01 was decided on 2026-10-19 for strategy B by [ADR 0029](../adr/0029-encrypted-restoration-store-and-secret-bearing-entries.md);
§5.4 below is kept as the comparison it was written as, and no longer binds.** Until it is made, ADR 0003 is to be read at full strength:
nothing that may hold a secret is persisted by the Console.

### 5.2 Threat model specific to reversible changes

| Who                                                      | Can do what with restoration data                                            |
| -------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Another user of the machine                              | Nothing, if the directory is 0700 and the files 0600                         |
| A process of the same user                               | Read it, as it can read the original file                                    |
| A backup or snapshot of the home directory               | Capture it; deleting the record later does not remove it from past snapshots |
| A replication tool, if the state directory were included | Copy it to another machine                                                   |
| Someone with the disk offline                            | Read it unless it is encrypted and the key is elsewhere                      |
| A support bundle or bug report                           | Include it by mistake                                                        |

The original file has every one of these exposures already. What a restoration record adds is
**a second place and a longer life**: the secret outlives its removal from the configuration.

### 5.3 Strategies

| Criterion             | A. Minimal private records                     | B. Encrypted local store                                                       | C. Provider-native or in-place                                                                                                             | D. Refuse entries that may hold a secret                                    |
| --------------------- | ---------------------------------------------- | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| What it is            | The removed entry, 0600 in a 0700 directory    | The same, encrypted with a key held by the desktop's secret service            | Use the provider's own disabled state; where there is none, keep the entry inside the provider's own file under a key the provider ignores | Classify the entry; if it may hold a secret, do not offer per-entry disable |
| Confidentiality       | Same as the source file                        | Better offline and in backups; same against a same-user process while unlocked | Nothing new is stored anywhere                                                                                                             | Nothing new is stored                                                       |
| Integrity             | Hash recorded in the journal                   | Authenticated encryption                                                       | The file's own                                                                                                                             | —                                                                           |
| Recovery              | Simple                                         | Fails if the key store is locked, reset or absent                              | Works on every computer the file reaches                                                                                                   | No recovery needed                                                          |
| File permissions      | Relied on entirely                             | Defence in depth                                                               | The provider file's own                                                                                                                    | —                                                                           |
| Same-user compromise  | No protection                                  | No protection while unlocked                                                   | No protection                                                                                                                              | —                                                                           |
| Backups and snapshots | Captured in clear                              | Captured encrypted                                                             | As the provider file already is                                                                                                            | —                                                                           |
| Replication           | Must be kept out; a mistake leaks it           | A mistake leaks ciphertext                                                     | Travels with the file: solves the second-computer case                                                                                     | —                                                                           |
| Key management        | None                                           | A new dependency on the secret service; behaviour without one must be defined  | None                                                                                                                                       | —                                                                           |
| Crash recovery        | An orphan record is possible; cleaned at start | Same                                                                           | One file, one rename: no orphan                                                                                                            | —                                                                           |
| Retention             | Must be bounded and purged                     | Same                                                                           | Lives as long as the user leaves it                                                                                                        | —                                                                           |
| Deletion by the user  | Delete the record (past snapshots keep it)     | Delete the record or the key                                                   | Delete it from his own file                                                                                                                | —                                                                           |
| Portability           | Explicit export                                | Explicit export, re-encrypted                                                  | Automatic                                                                                                                                  | —                                                                           |
| Tampering             | Detected by hash against the journal           | Detected                                                                       | An outside edit of a parked entry is just an outside edit                                                                                  | —                                                                           |
| Complexity            | Low                                            | High                                                                           | Low, **if** the provider tolerates it                                                                                                      | Low; needs a classifier                                                     |
| Fit with ADR 0003     | Breaks its sentence for entries with secrets   | Arguable: the secret is durable but not in clear                               | Keeps it: the Console stores nothing                                                                                                       | Keeps it                                                                    |
| Fit with decision D5  | Yes                                            | Yes                                                                            | Differs: the owner chose private local storage                                                                                             | Partial: some hooks cannot be disabled one by one                           |
| Open questions        | —                                              | Which service; headless and remote hosts                                       | Does Claude Code accept, ignore and preserve an unknown key in `settings.json`? `[N]` (OQ-06, OQ-19)                                       | How good can the classifier be?                                             |

### 5.4 Recommendation

A combination, in this order, per entry:

1. **Use the provider's own state wherever it has one** (Codex hooks, skills, plugins, MCP
   switches). Nothing is stored. This is C in its plain form and covers most kinds.
2. **For a Claude Code hook, classify the entry.** A structural check in the backend: handler
   type `http` or `mcp_tool` with headers or credentials, an environment assignment or a
   long random-looking token in the command, a URL with user information. The check is
   conservative: doubt counts as "may hold a secret".
3. **An entry that passes** is parked with strategy A, minimal and private (what ADR 0028 §7
   describes): it is configuration the Console already shows on screen.
4. **An entry that may hold a secret** is not parked (strategy D). The user is offered the
   coarse native switch (`disableAllHooks`), or editing the file himself.
5. **Research OQ-06 and OQ-19.** If Claude Code accepts and preserves an unknown key, in-place
   parking (C) becomes available for every entry, stores nothing in the Console, and solves the
   second-computer case; it would then replace step 3 and remove step 4, by a new decision.
6. **B is not recommended now**: it adds a dependency and failure modes to protect against
   offline readers, who can already read the original files.

Under this recommendation the Console never persists an entry it has reason to think holds a
secret, and ADR 0003's sentence needs no exception for secrets: the supersession recorded in
ADR 0028 would be narrowed by a new ADR to "configuration entries classified as holding no
secret". That narrowing is DN-01's recommended outcome.

### 5.5 What may be stored, and what never

| Store             | May hold                                                                                                                            | Never holds                                                                               |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Journal           | Item identifier, kind, provider, scope, operation, hashes before and after, time, outcome, error word                               | Content of any entry; any value; a path outside the provider's known files                |
| Restoration store | Per DN-01. Under the recommendation: entries classified as holding no secret, with their place and hashes                           | Credential files; environment values; anything classified as possibly secret; whole files |
| Last-seen index   | Per source: hash, time, the identifiers of its items; under the recommendation also the restorable entries that pass classification | The same exclusions                                                                       |
| Session registry  | Agent, project path, profile id and version, times, exit code                                                                       | Command lines typed by the user; terminal content; environment values                     |
| Profiles          | References and names                                                                                                                | Secrets; copied instruction text                                                          |
| Logs, diagnostics | Closed error words, counters, versions                                                                                              | Paths outside known provider locations; any file content; any value                       |

Never logged anywhere: a credential, an environment value, a header, a prompt, terminal bytes,
a diff of a configuration file, a provider's raw output.

## 6. Validation of the model

| Claim                                             | How it is checked                                                                                                                       |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| No secret reaches the page                        | TS-SEC-01: a corpus of fixtures seeded with marker secrets in every secret class; the serialised projection is searched for the markers |
| No secret reaches a store                         | TS-SEC-02: the same corpus through every operation; every store is searched                                                             |
| The page cannot cause an arbitrary write          | TS-SEC-04: every command's request type is enumerated and has no free path, content or command field                                    |
| A risky plan cannot be applied without the dialog | TS-SEC-05: `apply` on such a plan with the dialog stubbed to refuse writes nothing                                                      |
| The engine and the authority are apart            | TS-SEC-03                                                                                                                               |
| Filesystem attacks                                | TS-FS-01 to 06 (20 §3.3)                                                                                                                |
| The classifier of §5.4                            | A labelled corpus; the measured rate of secrets it lets through is reported, not assumed zero                                           |

## 7. Residual risks accepted, to be confirmed by the owner

1. A same-user process can do everything the Console can, without it.
2. A user can confirm a dialog without reading it.
3. A secret the user wrote into a field shown as configuration is displayed to him.
4. The race between the last hash check and the rename, whose length is not measured and one
   ordering of which is not detected as designed (24, RV-03). Not to be accepted as it stands:
   it is to be narrowed before F2a.
5. Whatever DN-01 leaves: under the recommendation, a secret the classifier does not recognise.
6. A provider can change the meaning of a setting without changing its format.
7. A compromised page can type into a terminal, so the native confirmation is a safeguard
   against mistakes, not a boundary against the page (24, RV-01).
