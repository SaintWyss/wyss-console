# 02 — Requirements

Every requirement has a stable identifier. **Source** says who asked: `Owner` (stated by the
owner on 2026-10-12, with the decision when there is one), an ADR, or `Design` (proposed by
this set and approved by nobody yet). **Phase** is the first phase of
[19](19-IMPLEMENTATION-ROADMAP.md) that delivers it; `Built` means it exists at `1ad90ee`.
Traceability to design, components and tests is in [23](23-TRACEABILITY-MATRIX.md).

Words used exactly: **must** is a requirement; **never** is a prohibition that a test has to
hold; "where the provider allows" means the adapter declares the capability with evidence
(06 §2) and the requirement does not apply where it does not.

## 1. Functional requirements

### 1.1 Terminal (TERM)

| ID         | Requirement                                                                                                     | Source          | Phase             |
| ---------- | --------------------------------------------------------------------------------------------------------------- | --------------- | ----------------- |
| FR-TERM-01 | Local shell terminals work with no agent installed, no authority running and no network                         | Owner; ADR 0005 | Built             |
| FR-TERM-02 | Several terminals in tabs, each with its own PTY, up to a declared limit                                        | ADR 0007        | Built             |
| FR-TERM-03 | A terminal can be split into panes                                                                              | Design          | T1                |
| FR-TERM-04 | Scrollback is bounded and its size is a preference                                                              | ADR 0026        | Built             |
| FR-TERM-05 | The PTY has the size of what is drawn, from the first byte the shell prints                                     | Owner           | Built             |
| FR-TERM-06 | Keyboard input, dead keys and input methods reach the shell unchanged                                           | Owner           | Built             |
| FR-TERM-07 | Selection, copy and paste work; a paste larger than the input bound fails visibly                               | ADR 0006        | Built             |
| FR-TERM-08 | Text can be searched in the scrollback                                                                          | Design          | T1                |
| FR-TERM-09 | Full-screen programs (alternate screen, mouse reporting, 256 and true colour) behave as in a reference terminal | Owner           | Built, unmeasured |
| FR-TERM-10 | The layout of tabs and panes, their titles and working directories are restored at start                        | Design          | T2                |
| FR-TERM-11 | A terminal can outlive the window that shows it                                                                 | Owner (remote)  | F9                |

### 1.2 Native sessions (NAT)

| ID        | Requirement                                                                                               | Source          | Phase    |
| --------- | --------------------------------------------------------------------------------------------------------- | --------------- | -------- |
| FR-NAT-01 | An installed agent can be started in a terminal from a closed catalogue, without typing its command       | Owner           | F3a      |
| FR-NAT-02 | The project folder of a native session is chosen in a native dialog or from known projects                | Owner           | F3a      |
| FR-NAT-03 | A profile and, where the provider allows, a model can be chosen when the session is started               | Owner           | F3c, F6b |
| FR-NAT-04 | The environment of a native session is the user's, plus a closed set of variables the backend derives     | Owner; ADR 0005 | F3a      |
| FR-NAT-05 | The Console knows the process of a native session: running, exited and with which code                    | Owner           | F3b      |
| FR-NAT-06 | Metadata of native sessions (agent, project, profile, times, exit) persists across restarts of the window | Owner           | F3b      |
| FR-NAT-07 | Processor and memory use of each native session are shown                                                 | Owner           | F3b      |
| FR-NAT-08 | Sessions can be grouped by project and by agent                                                           | Owner           | F3b      |
| FR-NAT-09 | Several native sessions run at once                                                                       | Owner           | F3a      |
| FR-NAT-10 | An agent started by hand in a terminal, or outside the Console, is shown as such when it can be detected  | Design          | F3b      |
| FR-NAT-11 | A native session is never presented as mediated, contained or approved by the authority                   | ADR 0028 §1     | F3a      |

### 1.3 Managed execution (MAN)

| ID        | Requirement                                                                           | Source         | Phase |
| --------- | ------------------------------------------------------------------------------------- | -------------- | ----- |
| FR-MAN-01 | Managed Codex sessions can be prepared, activated, resumed and closed from the window | ADR 0020, 0021 | Built |
| FR-MAN-02 | Inference turns: text only, no tool                                                   | ADR 0015       | Built |
| FR-MAN-03 | Local Tools turns: every command and file change waits for an approval                | ADR 0016, 0023 | Built |
| FR-MAN-04 | Execution needs a lease with exact scopes and an expiry                               | ADR 0013       | Built |
| FR-MAN-05 | Every control decision is recorded in the authority's durable stream                  | ADR 0017       | Built |
| FR-MAN-06 | The authority is installed and started by the product and outlives the window         | ADR 0025       | Built |
| FR-MAN-07 | Managed Inference works independently of the user's global Codex hooks                | Owner (D7)     | G-INF |
| FR-MAN-08 | Several managed runtimes or turns at once                                             | Owner          | F8    |
| FR-MAN-09 | Managed execution of agents other than Codex                                          | Owner          | F11b  |

### 1.4 Configuration engine (CFG)

| ID        | Requirement                                                                                                                                                                                              | Source             | Phase             |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ | ----------------- |
| FR-CFG-01 | The configuration sources of each installed provider are discovered, per scope                                                                                                                           | Owner              | F1                |
| FR-CFG-02 | Each source is parsed into a typed inventory; what cannot be parsed is reported, not guessed                                                                                                             | Owner              | F1                |
| FR-CFG-03 | Persisted, provider-resolved and session-effective state are shown apart for every element                                                                                                               | Owner; ADR 0028 §4 | F1                |
| FR-CFG-04 | A change made outside the Console is detected and shown                                                                                                                                                  | Owner              | F1                |
| FR-CFG-05 | An existing entry can be enabled, disabled and restored, in the provider's real file                                                                                                                     | Owner (D3)         | F2a, F2v, F2b     |
| FR-CFG-06 | Entries can be created, edited and deleted, where the provider allows                                                                                                                                    | Owner (D3)         | F5                |
| FR-CFG-07 | A file that changed since it was read is never overwritten: the change is refused and planned again                                                                                                      | Owner (D5)         | F2a               |
| FR-CFG-08 | A write is atomic, and is verified by reading the file back                                                                                                                                              | ADR 0028 §5        | F2a               |
| FR-CFG-09 | A change can be undone while the file has not diverged                                                                                                                                                   | Owner              | F2a               |
| FR-CFG-10 | Changes, undos, conflicts and outside changes are kept in a history of their own                                                                                                                         | Owner (D8)         | F2a               |
| FR-CFG-11 | Project-level configuration is managed with the same rules as global                                                                                                                                     | Owner              | F1, F2a           |
| FR-CFG-12 | System policy is shown, read only                                                                                                                                                                        | ADR 0028 §4        | F1                |
| FR-CFG-13 | A link, a generated file and a file the provider rewrites are recognised and handled as such                                                                                                             | Owner              | F1                |
| FR-CFG-14 | Whether the configuration is compatible with managed Inference is shown, with its reason                                                                                                                 | Owner (D7)         | F1                |
| FR-CFG-15 | Restoration data can be exported and imported between computers, by an explicit action, encrypted (27 §9.3)                                                                                              | Owner (D5)         | After F2v (DN-29) |
| FR-CFG-16 | A secret value is never sent to the window                                                                                                                                                               | Owner              | F1                |
| FR-CFG-17 | The user can keep, by an explicit choice made entry by entry, an encrypted recovery copy of selected entries, and recover one that was deleted or changed outside. Nothing is copied without that choice | Owner (2026-10-13) | F2h               |

### 1.5 Rules, hooks, skills, MCP (HRS)

| ID        | Requirement                                                                                                         | Source      | Phase                     |
| --------- | ------------------------------------------------------------------------------------------------------------------- | ----------- | ------------------------- |
| FR-HRS-01 | Hooks are listed with event, matcher, handler, source, scope and configured state                                   | Owner       | F1                        |
| FR-HRS-02 | A hook can be disabled and enabled, by the means its provider really has, and otherwise through the protected vault | Owner       | F2a, F2v, G-INF           |
| FR-HRS-03 | A disabled hook can be restored exactly                                                                             | Owner (D5)  | F2v                       |
| FR-HRS-04 | Hooks can be created, edited and deleted                                                                            | Owner (D3)  | F5                        |
| FR-HRS-05 | A hook is validated: known event, well-formed handler, bounded timeout, command that exists                         | Design      | F1 (report), F5 (enforce) |
| FR-HRS-06 | Rules and instruction files are listed with their source, scope, size and what they import                          | Owner       | F1                        |
| FR-HRS-07 | Rules can be switched and edited, where the provider's format makes that well defined                               | Owner       | F2a (switch), F5 (edit)   |
| FR-HRS-08 | Skills are listed with their location and state, and can be switched                                                | Owner       | F1, F2a                   |
| FR-HRS-09 | MCP servers are listed, switched, added and removed, through the provider's own writer where it has one             | Owner       | F1, F2a, F5               |
| FR-HRS-10 | Plugins are listed and can be switched                                                                              | Design      | F1, F2a                   |
| FR-HRS-11 | Hooks, rules and skills can be associated with profiles and projects                                                | Owner       | F3c                       |
| FR-HRS-12 | A provider's trust record for a hook is shown and never written by the Console                                      | ADR 0028 §3 | F1                        |

### 1.6 Adapters (ADP)

| ID        | Requirement                                                                                       | Source     | Phase   |
| --------- | ------------------------------------------------------------------------------------------------- | ---------- | ------- |
| FR-ADP-01 | One configuration adapter per provider, typed, behind one presentation contract                   | Owner (D2) | F1      |
| FR-ADP-02 | An adapter declares each capability with its support, its evidence and the version it was seen on | ADR 0008   | F1      |
| FR-ADP-03 | Execution adapters follow ADR 0008; configuration adapters are a separate contract                | ADR 0028   | F1      |
| FR-ADP-04 | A provider version without evidence is reported as unverified, and mutations on it are refused    | Design     | F1, F2a |
| FR-ADP-05 | An adapter's errors are closed words with no path, value or raw provider text                     | ADR 0008   | F1      |

### 1.7 Profiles (PRF)

| ID        | Requirement                                                                                    | Source | Phase |
| --------- | ---------------------------------------------------------------------------------------------- | ------ | ----- |
| FR-PRF-01 | A session profile describes the configuration one session should run with                      | Owner  | F3c   |
| FR-PRF-02 | Profiles can be made from templates                                                            | Owner  | F3c   |
| FR-PRF-03 | A profile can extend another                                                                   | Owner  | F3c   |
| FR-PRF-04 | A profile is applied when a session starts and never rewrites a global file                    | Owner  | F3c   |
| FR-PRF-05 | Profiles are versioned and a previous version can be brought back                              | Owner  | F3c   |
| FR-PRF-06 | Profiles can be exported and imported; they carry no secret and an import grants nothing       | Owner  | F3c   |
| FR-PRF-07 | A project can name its default profile                                                         | Owner  | F3c   |
| FR-PRF-08 | For each provider a profile says what it can apply, what needs a restart and what it cannot do | Owner  | F3c   |
| FR-PRF-09 | A named set of global switches can be applied as one reviewed change (a "global profile")      | Owner  | F5    |

### 1.8 Models (MOD)

| ID        | Requirement                                                                                  | Source   | Phase                         |
| --------- | -------------------------------------------------------------------------------------------- | -------- | ----------------------------- |
| FR-MOD-01 | Each session shows its provider                                                              | Owner    | F3b (native), Built (managed) |
| FR-MOD-02 | The catalogue of models a runtime offers is shown                                            | Owner    | F6a                           |
| FR-MOD-03 | The model configured at each scope is shown                                                  | Owner    | F1 (declared), F6a            |
| FR-MOD-04 | The model that actually answered a turn is shown when the provider reports it, and only then | Owner    | F6a                           |
| FR-MOD-05 | A model can be chosen for a session when it starts                                           | Owner    | F6b                           |
| FR-MOD-06 | A model can be switched in a running session, where the provider allows                      | Owner    | F6b                           |
| FR-MOD-07 | Reasoning or effort settings are shown and chosen where the provider has them                | Owner    | F6a, F6b                      |
| FR-MOD-08 | Usage and cost are shown only from figures the provider reports                              | Owner    | F6a                           |
| FR-MOD-09 | Changes of model are kept in history                                                         | Owner    | F6a                           |
| FR-MOD-10 | A model listed is never presented as a model the account is entitled to                      | ADR 0012 | F6a                           |

### 1.9 Sessions across planes (SES)

| ID        | Requirement                                                                                               | Source | Phase |
| --------- | --------------------------------------------------------------------------------------------------------- | ------ | ----- |
| FR-SES-01 | One overview lists native and managed sessions, each marked with its plane                                | Owner  | F3b   |
| FR-SES-02 | Every kind of session has a defined lifecycle                                                             | Design | F3b   |
| FR-SES-03 | Moving between sessions is one action and changes nothing but what is shown                               | Owner  | F3b   |
| FR-SES-04 | Work can be handed to another agent as an explicit package the user reviews                               | Owner  | F11a  |
| FR-SES-05 | Continuing a native session as managed, or the reverse, starts a new session and is never called lossless | Owner  | F11a  |
| FR-SES-06 | After a crash every session is shown in the state that is actually known                                  | Design | F3b   |

### 1.10 Observability (OBS)

| ID        | Requirement                                                              | Source     | Phase                 |
| --------- | ------------------------------------------------------------------------ | ---------- | --------------------- |
| FR-OBS-01 | What the authority recorded can be read, newest first                    | ADR 0024   | Built                 |
| FR-OBS-02 | The configuration history has its own view                               | Owner (D8) | F2a                   |
| FR-OBS-03 | The lifecycle of native sessions is recorded                             | Owner      | F3b                   |
| FR-OBS-04 | The user is told of a pending approval, an outside change and a conflict | Owner      | Built (approvals), F1 |
| FR-OBS-05 | A diagnostic report can be produced that holds no secret                 | Design     | F13                   |
| FR-OBS-06 | Resource use of the Console itself can be inspected                      | Design     | P1                    |

### 1.11 Security (SEC)

| ID        | Requirement                                                                                                                                      | Source             | Phase    |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------ | -------- |
| FR-SEC-01 | A change that weakens a security control is confirmed in a native dialog                                                                         | Owner (D4)         | F2a      |
| FR-SEC-02 | The risk of a change is classified and enforced in the backend                                                                                   | Owner (D4)         | F2a      |
| FR-SEC-03 | The window never supplies a path, file content or command text to the configuration subsystem                                                    | ADR 0028 §2        | F1       |
| FR-SEC-04 | Values classified as secret are redacted before they leave the backend                                                                           | Owner              | F1       |
| FR-SEC-05 | A provider's trust database is never written, forged or bypassed                                                                                 | Owner              | F1       |
| FR-SEC-06 | A system policy is never bypassed                                                                                                                | Owner              | F1       |
| FR-SEC-07 | No guarantee of the managed plane changes without an ADR and evidence                                                                            | Owner (D10)        | Always   |
| FR-SEC-08 | What may be kept to restore an entry follows an approved storage policy (ADR 0029, proposed)                                                     | Owner              | F2a, F2v |
| FR-SEC-09 | A remote device acts only within what was granted to that device                                                                                 | Owner              | F10      |
| FR-SEC-10 | Content kept to restore an entry is held only in the protected vault: encrypted, after informed consent, never replicated, and in no other store | Owner (2026-10-13) | F2v      |

### 1.12 Data (DAT)

| ID        | Requirement                                                                                                                        | Source             | Phase  |
| --------- | ---------------------------------------------------------------------------------------------------------------------------------- | ------------------ | ------ |
| FR-DAT-01 | Each piece of state has one owner                                                                                                  | ADR 0002           | Always |
| FR-DAT-02 | A crash at any point leaves every file whole                                                                                       | Design             | F2a    |
| FR-DAT-03 | Every store has a version and a tested migration                                                                                   | ADR 0012           | Always |
| FR-DAT-04 | Everything the Console keeps is bounded in size and in age                                                                         | Design             | F2a    |
| FR-DAT-05 | The user can delete what the Console keeps about his configuration                                                                 | Owner              | F2a    |
| FR-DAT-06 | The user can list, restore, reveal and delete every entry the Console holds for restoration, and is told when one is the only copy | Owner (2026-10-13) | F2v    |

### 1.13 Several computers (SYNC)

| ID         | Requirement                                                                         | Source     | Phase                  |
| ---------- | ----------------------------------------------------------------------------------- | ---------- | ---------------------- |
| FR-SYNC-01 | A configuration file replaced by a replication tool is treated as an outside change | Owner      | F1                     |
| FR-SYNC-02 | A conflict copy left by a replication tool is shown and blocks changes to its file  | Owner (D5) | F1 (show), F2a (block) |
| FR-SYNC-03 | Restoration data is private to the computer that made it and is never replicated    | Owner (D5) | F2v                    |
| FR-SYNC-04 | The design assumes no particular replication tool                                   | Design     | F1                     |

### 1.14 Remote (REM)

| ID        | Requirement                                                                       | Source | Phase |
| --------- | --------------------------------------------------------------------------------- | ------ | ----- |
| FR-REM-01 | A host service lets another device reach the terminals and sessions of a computer | Owner  | F10a  |
| FR-REM-02 | Each device has an identity and is paired by an explicit act of the user          | Owner  | F10a  |
| FR-REM-03 | Terminals can be used remotely                                                    | Owner  | F10a  |
| FR-REM-04 | Agent sessions can be watched remotely                                            | Owner  | F10a  |
| FR-REM-05 | An approval can be given or denied remotely                                       | Owner  | F10a  |
| FR-REM-06 | A dropped connection changes nothing; reconnecting resumes observation            | Owner  | F10a  |
| FR-REM-07 | What each device did is recorded on the host                                      | Owner  | F10a  |
| FR-REM-08 | A device can be revoked, and stops being able to act at once                      | Owner  | F10a  |
| FR-REM-09 | A phone client                                                                    | Owner  | F10b  |

### 1.15 Hermes and Wyss Agent (AGT)

| ID        | Requirement                                                                                                    | Source     | Phase |
| --------- | -------------------------------------------------------------------------------------------------------------- | ---------- | ----- |
| FR-AGT-01 | Hermes is supported as the original upstream agent, like any other provider                                    | Owner (D9) | F4b   |
| FR-AGT-02 | The original Wyss assistant and Hermes are compared capability by capability before any decision on Wyss Agent | Owner      | F12   |
| FR-AGT-03 | A future Wyss Agent enters as a provider behind the same contracts                                             | Owner      | F12   |

### 1.16 Distribution and extensibility (DIST)

| ID         | Requirement                                                                      | Source   | Phase  |
| ---------- | -------------------------------------------------------------------------------- | -------- | ------ |
| FR-DIST-01 | No path, user name, host name or provider setting of one person is fixed in code | Owner    | Always |
| FR-DIST-02 | A new user is shown which agents were found and nothing is changed for him       | Owner    | F13    |
| FR-DIST-03 | Installing, updating and removing are documented and tested                      | Owner    | F13    |
| FR-DIST-04 | A provider can be added without changing the core                                | Owner    | F13    |
| FR-DIST-05 | Supported platforms are a decision, not an accident                              | ADR 0027 | F13    |
| FR-DIST-06 | There is a way to report a vulnerability                                         | Design   | F13    |

### 1.17 Interface (UX)

| ID       | Requirement                                                                        | Source | Phase                     |
| -------- | ---------------------------------------------------------------------------------- | ------ | ------------------------- |
| FR-UX-01 | One navigation reaches every area                                                  | Owner  | Built, extended per phase |
| FR-UX-02 | Every action can be done from the keyboard without stealing keys from the terminal | Design | T1                        |
| FR-UX-03 | Unknown and unverified states look different from verified ones                    | Owner  | F1                        |
| FR-UX-04 | Every error says what happened, what was left unchanged and what can be done       | Design | Always                    |
| FR-UX-05 | A control for something not built yet is absent, or disabled and labelled so       | Owner  | F1                        |
| FR-UX-06 | The plane of everything shown is visible                                           | Owner  | F1                        |

## 2. Non-functional requirements

Budgets marked _proposed_ are targets of [15](15-PERFORMANCE-ENGINEERING.md), not measurements.

### 2.1 Performance (PERF)

| ID          | Requirement                                                                              | Source   | Phase             |
| ----------- | ---------------------------------------------------------------------------------------- | -------- | ----------------- |
| NFR-PERF-01 | Cold start to a usable prompt within a proposed budget                                   | Owner    | P1                |
| NFR-PERF-02 | Warm start within a proposed budget                                                      | Owner    | P1                |
| NFR-PERF-03 | Processor use at rest, with terminals open and nothing running, within a proposed budget | Owner    | P1                |
| NFR-PERF-04 | Memory at rest within a proposed budget                                                  | Owner    | P1                |
| NFR-PERF-05 | Memory added by each terminal within a proposed budget                                   | Owner    | P1                |
| NFR-PERF-06 | Delay from key to glyph within a proposed budget                                         | Owner    | P1                |
| NFR-PERF-07 | Output throughput within a proposed budget, with the window responsive throughout        | Owner    | P1                |
| NFR-PERF-08 | Resize settles within a proposed budget                                                  | Owner    | P1                |
| NFR-PERF-09 | Watching configuration files costs no measurable processor time at rest                  | Owner    | F1                |
| NFR-PERF-10 | Unbounded output from a program never exhausts memory or freezes the window              | ADR 0005 | Built, unmeasured |
| NFR-PERF-11 | The window stays responsive with the declared maximum of sessions running                | Owner    | F3b               |

### 2.2 Security (SEC)

| ID         | Requirement                                                                    | Source   | Phase                     |
| ---------- | ------------------------------------------------------------------------------ | -------- | ------------------------- |
| NFR-SEC-01 | The window holds only narrow, named permissions                                | ADR 0005 | Built                     |
| NFR-SEC-02 | Every input from a file, a provider or the window is bounded in size and count | ADR 0011 | Built, extended per phase |
| NFR-SEC-03 | What cannot be proven safe is refused                                          | ADR 0015 | Built                     |
| NFR-SEC-04 | No log, event, history or diagnostic holds a secret                            | ADR 0003 | Built, extended per phase |

### 2.3 Reliability (REL)

| ID         | Requirement                                                                     | Source   | Phase                 |
| ---------- | ------------------------------------------------------------------------------- | -------- | --------------------- |
| NFR-REL-01 | No configuration file is ever left partly written                               | Design   | F2a                   |
| NFR-REL-02 | The authority does not depend on the window                                     | ADR 0002 | Built                 |
| NFR-REL-03 | The terminal does not depend on the authority or on the configuration subsystem | ADR 0019 | Built, held per phase |
| NFR-REL-04 | A provider's file that cannot be read or parsed disables that source only       | Design   | F1                    |

### 2.4 Maintainability (MNT)

| ID         | Requirement                                                                      | Source     | Phase  |
| ---------- | -------------------------------------------------------------------------------- | ---------- | ------ |
| NFR-MNT-01 | Each provider version the product has evidence for is recorded                   | ADR 0008   | F1     |
| NFR-MNT-02 | A dependency is added only for a need a phase has, with a written reason         | Owner (D6) | Always |
| NFR-MNT-03 | Tests are deterministic and need neither the user's configuration nor a provider | ADR 0012   | Always |

### 2.5 Portability (PORT)

| ID          | Requirement                                                     | Source   | Phase |
| ----------- | --------------------------------------------------------------- | -------- | ----- |
| NFR-PORT-01 | Linux with a `systemd --user` session is the supported platform | ADR 0025 | Built |
| NFR-PORT-02 | Everything that ties the design to that platform is listed      | Design   | F13   |

### 2.6 Accessibility (A11Y)

| ID          | Requirement                                               | Source | Phase  |
| ----------- | --------------------------------------------------------- | ------ | ------ |
| NFR-A11Y-01 | Every control is reachable and operable from the keyboard | Design | Always |
| NFR-A11Y-02 | Text and state indicators meet a stated contrast ratio    | Design | Always |
| NFR-A11Y-03 | State is never carried by colour alone                    | Design | Always |
| NFR-A11Y-04 | Motion respects the system's reduced-motion setting       | Design | Built  |

## 3. Requirements that conflict, and how

| Conflict                                                                                       | Resolution                                                                                                                                                                                                                                                                                                                                                  |
| ---------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| FR-HRS-02 (enable a Codex hook) against FR-MAN-02 (Inference rejects any enabled hook)         | Enabling waits for G-INF (FR-MAN-07); 10 §5                                                                                                                                                                                                                                                                                                                 |
| FR-HRS-03 (restore exactly) against NFR-SEC-04 and ADR 0003 (no secret in durable storage)     | The owner's decision of 2026-10-13: a protected private backup is allowed. NFR-SEC-04 stands for every log, event, history and diagnostic; the content of a disabled entry may be held only in the encrypted vault, as a named exception to ADR 0003 (ADR 0029 §1, proposed; 27). Until that ADR is accepted, disabling one Claude Code hook is unavailable |
| FR-CFG-05 (change the real global file) against FR-PRF-04 (independent sessions)               | Two features: global editing changes everyone; profiles change one session (09 §2)                                                                                                                                                                                                                                                                          |
| FR-NAT-01 (start an agent) against ADR 0005 (the window supplies no executable or argv)        | A closed catalogue resolved in the backend, with its own ADR (05 §4)                                                                                                                                                                                                                                                                                        |
| FR-TERM-11, FR-REM-03 (terminal outlives the window) against ADR 0005 (PTYs are Console-local) | Needs a host-owned PTY and its own ADR (16 §4)                                                                                                                                                                                                                                                                                                              |
| NFR-PERF-* (Ghostty as reference) against the WebView architecture                             | Measure first (P1); no technology change without numbers (15 §6)                                                                                                                                                                                                                                                                                            |

## 4. Owner-selected final-product requirements (2026-10-13; not built)

This appendix extends the original baseline with stable identifiers. The source is the
owner's explicit final-product choices recorded in
[32](32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md), with design in
[33](33-CROSS-DOMAIN-RECONCILIATION.md),
[34](34-OL3-ORCHESTRATION-CONTRACT.md) and
[35](35-HOST-REMOTE-AND-MOBILE-DELTA.md).
These are **targets**, never `Built`, never ADR acceptance and never implementation
authorisation. Phases are candidate delivery dependencies, not authorised work.
When the actual provider/OS does not allow a capability, the product must
say so; it must not invent a fallback that weakens protection.

| ID         | Target functional requirement                                                                                                                            | Phase        |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| FR-ORC-01  | Agent-to-agent delegation proposes a typed task; nothing launches without explicit owner-approved bounded envelope.                                      | F11b         |
| FR-ORC-02  | OL3 executes finite multi-agent DAGs with bounded steps, time, resources, concurrency, stop and audit.                                                   | F11b         |
| FR-ORC-03  | An edit of an approved pipeline invalidates its digest and requires new explicit owner consent.                                                          | F11b         |
| FR-ORC-04  | Coordinator output and handoffs never confer tools, permissions or managed leases.                                                                       | F11b         |
| FR-ORC-05  | Coordinator crash, retry and cancellation never silently duplicate external actions or claim dead processes are active.                                  | F11b         |
| FR-ORC-06  | Each managed session uses an independently bound runtime generation, catalogue, lease and attestation.                                                   | F8/F11b      |
| FR-ORC-07  | An OL3 pipeline cannot self-extend, auto-schedule new workflows or impersonate an owner approval.                                                        | F11b         |
| FR-ORC-08  | Pipeline run metadata excludes commands, prompts, outputs, credentials and recoverable content.                                                          | F11b         |
| FR-REM-10  | Native Android client offers scoped viewing, terminal input and managed approvals under per-device grants.                                               | F10b         |
| FR-REM-11  | Remote connectivity attempts same-LAN/direct when feasible and falls back without manual VPN per connection.                                             | F10a         |
| FR-REM-12  | Path migration and reconnect cannot replay a remote terminal input or create two writers.                                                                | F10a         |
| FR-REM-13  | User-enabled lingering allows host sessions to continue after KDE logout when computer is awake.                                                         | F9           |
| FR-REM-14  | Reconnect presents bounded faithful current terminal screen state with explicit missing-history indicators.                                              | F9/F10a      |
| FR-REM-15  | Android may prepare a versioned configuration change that only native host confirmation can apply.                                                       | F10b/F5      |
| FR-REM-16  | Device pairing/revocation, host pinning, keystore presence checks and input claims survive transport changes.                                            | F10a/F10b    |
| FR-REM-17  | A host crash or power-off is not reported as continued execution; explicit restart proposal only.                                                        | F9           |
| FR-TERM-12 | Global scrollback budget reduces admission for new terminals without trimming existing retained output just for crossing threshold.                      | T1/P1        |
| FR-TERM-13 | Host terminal screen parser bounds untrusted escape-sequence processing, memory and text effects.                                                        | F9           |
| FR-TERM-14 | Restoring saved layout reattaches to existing sessions; stopped agents are presented for individually approved restart.                                  | T2/F9        |
| FR-TERM-15 | Interactive screen, scrollback, output backlog and renderer cache have distinct quotas and states.                                                       | F9/P1        |
| FR-MOD-11  | Managed user may request model/effort at session start and per turn only through validated catalogue and separate authority contract.                    | F6b          |
| FR-MOD-12  | Model requested, model reported and actual observed reroute are separate labelled facts with source and granularity.                                     | F6a/F6b      |
| FR-MOD-13  | Model-observation changes are versioned events without prompt/output or invented per-turn model.                                                         | F6a          |
| FR-MOD-14  | Native model/effort selector acts only via safe verified provider mechanism with explicit limitations.                                                   | F6b          |
| FR-MOD-15  | Provider-reported tokens, limits and history carry source/time, never fabricated costs or secret text.                                                   | F6a          |
| FR-UX-07   | Global navigation groups sessions by project and provides all named final-product areas.                                                                 | F3b/F10a     |
| FR-UX-08   | Disabled contextual actions show blocking reason; unavailable whole areas remain absent.                                                                 | F1/F5        |
| FR-UX-09   | Dark, light and high-contrast modes include accessible terminal colours.                                                                                 | F13/T1       |
| FR-UX-10   | Desktop notifications are opt-in per category and redact content sensitive in lock-screen/history.                                                       | F10a         |
| FR-UX-11   | A configurable leader plus safe direct shortcuts never silently capture program-owned key sequences.                                                     | T1           |
| FR-UX-12   | A 30-day configurable review reminder never deletes or alters kept entries by age.                                                                       | F2v          |
| FR-PRF-10  | Profile exclusions are evaluated against effective controls at every start; changes require native reapproval.                                           | F3a/F3c      |
| FR-PRF-11  | Per-session separated configuration uses official provider authentication-preserving mechanisms only.                                                    | F3c          |
| FR-CFG-18  | Native hook command editor owns authoring; a page cannot submit unchecked executable text to the write path.                                             | F5           |
| FR-CFG-19  | Provider-native config operations use a typed, version-verified closed allowlist with review.                                                            | F4b/F5       |
| FR-SEC-11  | Terminal and configuration views have separately enforced capabilities; confinement claim is limited to view-level boundary.                             | F5           |
| FR-AGT-04  | Wyss Agent owns policies and effects; Hermes may propose plans but cannot invoke effectful tools directly.                                               | F12          |
| FR-AGT-05  | Wyss Console and Wyss Agent stay separate products; no distinct public Wyss CLI required.                                                                | F12          |
| FR-OBS-07  | Durable usage statistics retain only bounded non-secret metrics and source/freshness; no raw content.                                                    | F6a          |
| FR-DIST-07 | Final personal deployment supports tested installation, upgrade, rollback and local recovery on owner's reference computers without publishing binaries. | F13-personal |

### Conditional scope rules

Under DN-07=A, historical FR-DIST-02 to 06 and the distribution acceptance
set are **conditional on public distribution**, not silently deleted or
claimed satisfied. FR-DIST-07 names local/personal delivery instead.
DN-49/51 are not applicable until the owner expands scope.
The selected DN-05 remote transport and DN-31 sensitive-state filesystem
location remain **open evidence gates**; no requirement is interpreted
as authorising a particular relay/vendor, plaintext snapshot exposure,
reproduced provider credentials, or silent remote native consent.

## 5. Native CachyOS privilege and Docker-coupled trust requirements (2026-10-14)

Added following the [CachyOS-native test report](reviews/2026-10-14-cachyos-native-feasibility.md)
and [architecture conflict analysis 47](47-CACHYOS-DOCKER-PRIVILEGE-AND-SANDBOX-DECISION.md).
These requirements are final-product **targets**, not implemented safeguards
or owner consent to change Docker.

| ID            | Requirement                                                                                                                                                                                                                                                                                                           | Design/gate               |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| **FR-SEC-12** | Claims that managed approvals are protected against unrestricted native/LP4 processes MUST account for all delegated host-admin capabilities, including the current rootful Docker socket. If no genuine trusted boundary exists, display/record `BLOCKED_ADMIN_DELEGATION` rather than asserting approval integrity. | 47 §2–3 / TS-DOCKER-01–02 |
| **FR-SEC-13** | An OL3 restricted native-worker profile MUST prove denial of control/authority, privileged Docker socket, host session IPC/X11/D-Bus, inherited FDs and unintended filesystem effects while preserving useful provider/mock workflow. A sandbox name or Landlock-only policy does not satisfy this.                   | 47 §4 / TS-DOCKER-03      |
| **FR-PRF-12** | Provider authentication in a restricted worker MUST preserve the provider-supported in-place or atomic-replacement refresh mode through a narrowly bounded, verified capability; never copy credentials or widen to a whole unreviewed home to obtain a PASS.                                                         | 47 §4 / TS-DOCKER-04      |
| **FR-REM-18** | Before any full-account LP4 grant, the client MUST disclose root-equivalent host delegation where present, bounded consent and honest revocation limitations. A remote gateway grant must not claim to protect a rootful Docker daemon from an unrestricted user shell.                                               | 47 §2/6 / TS-DOCKER-06    |

No Docker privilege migration is a required product feature by default.
Any environmental change remains owner-controlled and must be
independently planned and approved.
