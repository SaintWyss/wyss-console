# 09 — Profiles and orchestration

Domain G of [04](04-TARGET-ARCHITECTURE.md), and the part of I that concerns several sessions
at once. All `[P]`. Phases: F3c for session profiles, F5 for global profiles, F11a for handoff.

## 1. Vocabulary

| Term                  | Meaning                                                                                                           |
| --------------------- | ----------------------------------------------------------------------------------------------------------------- |
| **Session profile**   | A named description of what one session should run with. Applied when the session starts. Changes no global file  |
| **Template**          | A profile shipped with the product, meant to be copied                                                            |
| **Project profile**   | The session profile a project names as its default                                                                |
| **Temporary profile** | A session profile made for one start and not kept                                                                 |
| **Global profile**    | A named set of switches on the real global configuration, applied as one reviewed change. It mutates shared files |
| **Project**           | A folder the user has worked in through the Console, with its remembered defaults                                 |

The first four are one mechanism. The fifth is a different one that happens to share a word;
the interface keeps them in different places and never offers one where the other is meant.

## 2. Two things that must not be confused

|                         | Editing the global configuration                | Starting with a session profile                 |
| ----------------------- | ----------------------------------------------- | ----------------------------------------------- |
| What changes            | The provider's real file                        | Nothing on disk that another session reads      |
| Who is affected         | Every session of that agent, here and elsewhere | The one session                                 |
| When it takes effect    | Per the provider's reload rule (07 §6)          | At that session's start only                    |
| Two sessions can differ | No                                              | Yes, as far as the provider's mechanism reaches |
| Mechanism               | 07 §5                                           | §3 below                                        |

If a global hook changes while two sessions depend on it: both are listed in the plan before it
is applied; afterwards each is `using`, `documented_using`, `stale` or `unknown` by the
provider's reload rule (07 §3).
A session that was started with a profile that excludes the user source is listed as
unaffected. Nothing restarts a session.

## 3. How a profile is applied, per provider

| Provider    | Mechanism at start                                                                                                                                                                          | What it can and cannot do                                                                                                                                                                                             | Evidence                                             |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| Claude Code | `--settings <generated file>`; `--setting-sources` to leave out the user source; `--mcp-config` with `--strict-mcp-config`; `--model`; `--append-system-prompt`; `--disable-slash-commands` | Adds freely. To **remove** a global hook for one session the user source must be left out and the wanted set supplied whole, because hooks merge. Whether that leaves out user rules and skills too is to be measured | Flags `[O]`; merge `[D]`; exact effect `[N]` (OQ-02) |
| Codex       | `--profile <name>` reading a profile file; `-c key=value`; `--model`                                                                                                                        | A profile file overlays the user file; system requirements still apply. The profile file must live in Codex's home to be found `[D]`: see §3.1                                                                        | `[D]`                                                |
| OpenCode    | `OPENCODE_CONFIG`, `OPENCODE_CONFIG_CONTENT`                                                                                                                                                | Merges over the global configuration; cannot subtract a plugin `[N]`                                                                                                                                                  | `[D]`, `[N]`                                         |
| Hermes      | A profile home through `HERMES_HOME`                                                                                                                                                        | Full separation, including memory and skills                                                                                                                                                                          | `[D]`, `[N]`                                         |

Full separation of a provider's home (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) also separates its
sign-in. Sharing a credential with a separate home is an operation on authentication and is
not part of session profiles; it would need its own decision (DN-14). Profiles use flags and
overlay files, which keep the user's sign-in.

### 3.1 Generated files, and profiles that are unavailable

A profile may need a file only the session reads. Two kinds must be told apart.

| Kind                                               | Example                                                                             | Rule                                                                                                                                                                                                                                                                                                                                              |
| -------------------------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Holds no entry of the user's configuration         | A model, a permission mode, a list of MCP servers by name                           | A per-session directory in the per-login runtime directory, mode 0700, removed when the session ends and at the next start. Never in the Console's state directory                                                                                                                                                                                |
| Would hold copied entries (hooks, MCP definitions) | Claude Code: "leave one global hook out", which needs the wanted set supplied whole | **Unavailable until DN-20 is decided.** Such a file is a copy of content that may hold a secret (ADR 0029 §7). Candidates for the decision: a memory-only file handed to the agent through an inherited descriptor, with nothing on disk and gone with the session (whether a provider reads such a path is OQ-34); or not offering such profiles |

Leaving out the user source of Claude Code has an effect that is not measured (OQ-02): it may
also drop the user's permission rules, rules files and skills. Until it is measured, a profile
that needs it is unavailable (`source_exclusion_unverified`). Once measured, the launch plan
lists every control that would be left out, computed at that moment from the configuration as
it is, and the start takes the native confirmation of 11 §4 **whenever that list differs from
the one last confirmed for this profile on this computer**. What is remembered is a keyed
digest of the list, never its content. A change of the user's hooks therefore asks again even
though the profile did not change.

Codex is the exception that needs a decision: `--profile name` looks for
`~/.codex/name.config.toml` `[D]`. Using it would mean writing a file into the provider's own
home, which a session profile must not do. Options: express the Codex profile entirely as
`-c key=value` arguments (no file, bounded in what it can carry); or write a clearly named file
there and remove it afterwards, which is a mutation of a provider directory. Recommended: the
first; the second only by a later decision (OQ-24).

### 3.2 Risk evaluation of a launch (DN-20)

A profile that leaves out the user's instructions, hooks or permission rules changes what
protects that session. The risk is a property of the **resolved launch plan against the
configuration as it is at that moment**, never of the profile's name: a profile accepted last
week leaves out more if a blocking hook was added since.

| Step | What happens                                                                                                                                                                                                                                |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1    | The effective configuration of that provider for that project is read now (07 §3), at a revision                                                                                                                                            |
| 2    | The profile is resolved (§4.2) into what the session would load                                                                                                                                                                             |
| 3    | **Left out**: every item present in the first and absent from the second, by kind: hooks of events that can block; permission rules that deny or ask; a switch that turns hooks off; entries the user marked as controls; instruction files |
| 4    | **Added**: executable content the profile brings (hooks, MCP servers, plugins)                                                                                                                                                              |
| 5    | **Still enforced**: provider or system policy the profile cannot leave out. Listed only where the adapter has evidence for it; otherwise shown as "not known", never assumed                                                                |
| 6    | Class: `weakens_security` if a control is left out; `executes_code` if something executable is added; otherwise `normal`                                                                                                                    |
| 7    | A keyed digest of the left-out set, by item identifiers, never by content, is compared with the one last confirmed for this profile, provider and project on this computer                                                                  |

| Kind of item left out                                | What it is to the user                  | Shown as                                                   |
| ---------------------------------------------------- | --------------------------------------- | ---------------------------------------------------------- |
| A blocking hook, a permission rule, a marked control | A safeguard he controls                 | "Left out: …", in the native dialog                        |
| An instruction file                                  | Guidance for the model, not a control   | "Not loaded: …", in the launch plan; no dialog by itself   |
| A system or managed policy                           | Enforced by the provider or the machine | "Still in force", or "not known whether it stays in force" |

Confirmation, as recommended: the native dialog of 11 §4 whenever the left-out set differs from
the one last confirmed, or when the provider's version changed since; no dialog when nothing
changed. The dialog names each control left out and says: "This session will not be equivalent
to your global configuration." A stored confirmation authorises that set and no other.

The evaluation is bound to the revision of step 1. If the configuration changes before the
start, the plan is stale (`launch_plan_stale`) and is evaluated again.

A session started with such a profile carries a marker in the overview for its whole life
("profile X: N controls left out"); it is never listed as running the user's configuration.

Where the generated configuration may exist, for a profile that must supply whole entries:

| Option                                                                                              | On disk                       | Depends on                                             |
| --------------------------------------------------------------------------------------------------- | ----------------------------- | ------------------------------------------------------ |
| Built in memory from the provider's file at that instant and handed over by an inherited descriptor | Nothing                       | The provider reading such a path (OQ-34)               |
| A file in the per-login runtime directory, mode 0600, removed at exit and at the next start         | Memory-backed; gone at logout | That directory; a crash leaves it until the next start |
| Not offering such profiles                                                                          | Nothing                       | —                                                      |

It is never built from the vault, never cached between sessions and never written under the
Console's state directory or a provider's directory. Provider compatibility of all of this is
**not verified**.

### 3.3 Starting with a profile (SM-LAUNCH)

| State        | Enters when                                 | Guard                                                                 | Leaves to                                            | On failure                                                                                                                         |
| ------------ | ------------------------------------------- | --------------------------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `requested`  | The user asks to start with a profile       | The profile, the agent and the project exist                          | `resolving`                                          | `profile_unknown`, `project_unknown`; nothing started                                                                              |
| `resolving`  | —                                           | Everything the profile marks as required is applicable                | `evaluated`                                          | `profile_not_applicable`, `profile_unavailable_copy_needed`, `source_exclusion_unverified`: refused                                |
| `evaluated`  | §3.2 produced the three lists and the class | —                                                                     | `confirming` if a dialog is due; `generating` if not | —                                                                                                                                  |
| `confirming` | The native dialog is open                   | One dialog at a time; the revision of §3.2 unchanged                  | `generating` on acceptance                           | Refused, closed or timed out: `confirmation_refused`. Cannot be shown: `dialog_unavailable`. Revision changed: back to `resolving` |
| `generating` | Per-session files are made, where allowed   | The revision is unchanged; the files could be created with their mode | `starting`                                           | `launch_plan_stale`: back to `resolving`. Cannot be created: `failed`; whatever was made is removed                                |
| `starting`   | `IF-NATIVE.start` (05 §4.2)                 | —                                                                     | `started`                                            | As 05 §4.2; generated files removed                                                                                                |
| `started`    | The agent's process exists                  | —                                                                     | The session's own lifecycle (05 §5)                  | At its end, and at the next start of the Console, generated files are removed                                                      |

Nothing is started in any state but `starting`, and a refusal leaves nothing behind.

## 4. The profile

### 4.1 Content

| Field             | Meaning                                                                                                               |
| ----------------- | --------------------------------------------------------------------------------------------------------------------- |
| `id`, `name`      | A UUID and a display name                                                                                             |
| `version`         | A hash of the content; every save is a new version                                                                    |
| `extends`         | At most one parent profile                                                                                            |
| `applies_to`      | The agents it is written for                                                                                          |
| `rules`           | Instruction files to add, by reference to a file in a project or in the user's configuration; text is never copied in |
| `hooks`           | Hooks that must be present or absent, by item identifier                                                              |
| `skills`, `mcp`   | To include or exclude, by name                                                                                        |
| `model`, `effort` | Where the provider takes one at start                                                                                 |
| `permission_mode` | Where the provider has a start flag for it                                                                            |
| `environment`     | Names from a closed list the adapter publishes; never free variables, never values that are secrets                   |
| `notes`           | Free text for the user                                                                                                |

A profile holds references and names, not copies and not secrets. An imported profile grants
nothing: it is a description, and what it asks for is shown before its first use.

### 4.2 Resolution

`resolve(profile, agent, project)` is a pure function that returns a **launch plan**: the
arguments, the variables, the files to generate, and three lists the user sees before starting:

| List                | Example                                                                                                         |
| ------------------- | --------------------------------------------------------------------------------------------------------------- |
| Applied             | "Model: sonnet. Two MCP servers only."                                                                          |
| Applied with a cost | "To leave out your global hook 'security-preflight', your user settings are not loaded at all in this session." |
| Not applicable      | "OpenCode cannot leave out a plugin for one session."                                                           |

If anything the profile marks as required falls in the third list, the start is refused.

### 4.3 Inheritance

One parent, no cycles (checked on save; `profile_cycle`). A child's field replaces the
parent's for scalars; lists combine with explicit include and exclude entries, exclusion
winning. Depth is bounded at 8. The resolved result is shown, with the profile each value came
from.

### 4.4 Versioning and rollback

Every save writes a new version file and moves a pointer; the previous versions stay, up to a
bound of 20 per profile. "Restore version" moves the pointer. A running session keeps the
version it started with, recorded in the registry.

### 4.5 Import, export, conflicts

Export writes one file the user places with a native dialog. Import validates it, strips
anything outside the schema, shows what it would ask for, and stores it as a new profile; a
name clash gets a suffix, never an overwrite. A profile that references an item the importing
computer does not have is kept and marked "refers to something not present here".

### 4.6 Templates

Shipped as read-only examples; they are names for common shapes, not approved configurations:
_as configured_ (no change), _review, read only_, _exploration, fewer hooks_, _strict_. Each is
only as strong as the provider's start flags make it and says so.

## 5. Projects

A project is a canonical folder path plus remembered defaults: default agent, default profile,
last used. It is created the first time the user starts a native session or a managed session
in that folder. The list is the Console's own state; it never scans the disk for repositories.
Managed sessions keep their workspace in the authority (ADR 0014) `[R]`; the project list only
points at the same folder.

## 6. Several sessions at once

| Situation                                                 | Behaviour                                                                                                                                                         |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Two native sessions of different agents                   | Independent processes in two terminals; each shown with its agent, project and profile                                                                            |
| Two native sessions of the same agent, different profiles | Independent as far as §3 reaches for that provider; the overview shows the difference each profile makes                                                          |
| Two native sessions on the same folder                    | Allowed. The overview warns that both can write the same tree. The Console coordinates nothing; a worktree per session is suggested, never created without asking |
| A native and a managed session on the same folder         | Allowed and shown: the managed one writes only through approvals; the native one writes freely                                                                    |
| More sessions than the terminal limit                     | `limit_reached` (C-06)                                                                                                                                            |
| Managed: a second turn while one runs                     | Refused by the authority today (C-04); the window says a turn is running                                                                                          |

Resource limits: the Console imposes only its own (terminals, queues). It reports processor and
memory per session (05 §6) and does not throttle or kill.

## 7. Switching, and what it is not

Three different things the interface keeps apart (FR-SES-03 to 05):

| Action                       | What happens                                          | What is carried over                |
| ---------------------------- | ----------------------------------------------------- | ----------------------------------- |
| **Switch view**              | Another session is shown                              | Nothing changes                     |
| **Start another agent here** | A new session, same project, another agent or profile | The project folder. No conversation |
| **Hand over**                | A new session receives a package the user reviewed    | Exactly what is in the package      |

### 7.1 Handoff package (F11a)

A document the user sees and edits before it is given to the receiving session as its first
input:

| Part                    | Source                                                         |
| ----------------------- | -------------------------------------------------------------- |
| Objective               | Written or confirmed by the user                               |
| Project and revision    | The folder, the current commit, whether the tree is clean      |
| Decisions taken         | Selected by the user from the sending session's visible output |
| Files that matter       | Paths, chosen by the user                                      |
| Open items              | Written by the user                                            |
| Limits for the receiver | The profile it starts with                                     |

Not carried: the sender's private conversation state, its credentials, its tools, anything the
user did not see. The sending session is never said to continue in the receiver. Content taken
from a session's output is untrusted text: it is given to the receiver as quoted material, and
a managed receiver gets no more authority because of it.

### 7.2 Native to managed, managed to native

| Direction        | What it is                                                                  | What is lost                                                                                                                                         |
| ---------------- | --------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Native → managed | A new managed session on the same folder, optionally with a handoff package | The native conversation; every tool the native agent had beyond the managed profile; the user's hooks and rules (the managed runtime loads none, L4) |
| Managed → native | A new native session on the same folder, optionally with a package          | Approvals and leases: the native session is not mediated                                                                                             |

Both are shown as "start a new session", never as "move" or "convert".

## 8. Orchestration

Coordinating agents automatically (one implements, one reviews) is not designed here beyond
its prerequisites: independent sessions, profiles, a handoff package and a record of who did
what. The research report measured multi-agent runs as expensive and failure-prone (its
findings 11) and the owner's priority is control, not autonomy. An orchestrator is a later
decision (DN-15); nothing in this design assumes one.

## 9. Interfaces

| Operation (IF-PROFILE) | Input                                               | Output                                                                  | Permission                                                                 | Errors                                                                                     |
| ---------------------- | --------------------------------------------------- | ----------------------------------------------------------------------- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `list`                 | —                                                   | Profiles with current version                                           | Read                                                                       | —                                                                                          |
| `get`                  | Profile id, version or current                      | The profile                                                             | Read                                                                       | `profile_unknown`, `profile_version_missing`                                               |
| `create`               | A template id or a copy source                      | A new profile                                                           | Write to the Console's own store; confirmation in the page                 | `profile_invalid`                                                                          |
| `save`                 | Profile id, content, the version it was edited from | A new version                                                           | The same. The content schema has no command, argument, path or value field | `profile_invalid`, `profile_cycle`, `profile_changed` (someone saved in between)           |
| `restore_version`      | Profile id, version                                 | Pointer moved                                                           | The same                                                                   | `profile_version_missing`                                                                  |
| `resolve`              | Profile id, agent id, project id                    | A launch plan (§4.2) with its risk class and the controls it leaves out | Read (pure)                                                                | `profile_not_applicable`, `profile_unavailable_copy_needed`, `source_exclusion_unverified` |
| `export`               | Profile id                                          | A file                                                                  | Native file chooser                                                        | —                                                                                          |
| `import`               | —                                                   | A new profile                                                           | Native file chooser; grants nothing (CTL-19)                               | `import_invalid`                                                                           |

Limits: 64 KiB per profile, 256 profiles, 20 versions each. Starting with a profile is
`IF-NATIVE.start` (05 §4.2) through SM-LAUNCH (§3.3), which adds `confirmation_required`,
`confirmation_refused`, `dialog_unavailable` and `launch_plan_stale`; in each case nothing is
started. Saving a profile is never what weakens a session: the risk is evaluated at each start.

## 10. Testing

Resolution and inheritance as pure functions with a table of cases per provider; cycles;
import of a file with unknown and hostile fields; a start refused when a required item is not
applicable; generated files removed after exit and after a simulated crash. Real-provider
checks of §3 are the measurements of OQ-02 and OQ-24, run only with authorisation.
