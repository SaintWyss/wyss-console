# Final-product functional specification

This page owns target behaviour: all 31 FP requirements, user journeys and same-number
AC-FP acceptance specifications. They are requirements, not claims of implementation.
Read [product scope](PRODUCT.md) for owner decisions,
[implementation status](STATUS.md) for delivered evidence and
[architecture](ARCHITECTURE.md) for ownership/security constraints.

Acceptance case IDs are `AC-FP-<group>/<suffix>`; for example
`AC-FP-CTX-04/before-confirmation`. Each parameterized stimulus is assessed independently,
including applicable boundary, invalid, interrupted and recovery variants. These tables
refine the existing 31 parent AC groups; they do not replace the detailed FP constraints.
A case is not PASS because code or a similarly named test exists. The
[current evidence map](contracts/acceptance.md#functional-case-evidence-map) identifies
review locations and still-required qualification without claiming those tests were run.

## 1. Product and context

<a id="fp-ctx-01"></a>

### FP-CTX-01 — Device → Project → Workspace → Sessions

A **device** is the client used to operate the product. A **host** is the PC executing
work; one PC can be both. The visible breadcrumb is `Host / Project / Workspace` with
connection age and local/remote indication. A project is a stable logical record with
one or more explicitly linked host-local folders. A workspace is a named working
arrangement within one project on one host: tabs, panes, session references, selected
profiles and work coordination. Execution stays with the host/authority; sessions identify
that work, while workspaces own presentation and references. Provider thread IDs, terminal
IDs and managed runtime IDs remain distinct.

There may be multiple workspaces and multiple sessions in one project, and several
open projects. A project-origin association belongs to one host/project; an unassigned
Scratch shell may receive its project origin once through reviewed adoption. Switching
the UI does not move its working directory, provider conversation or authority. Workspaces
of that same project may reference the same surviving session; a different project cannot
attach it by reusing its label or ID. Cooperative input ownership remains exclusive through
claim-aware Console clients; older/other same-UID writers are not contained. Project names
are display labels, never security identities or filesystem paths.

**AC-FP-CTX-01:** each case below is required at its applicable evidence level.

| Case suffix   | Stimulus                                                   | Required observable outcome                                                     |
| ------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `same-owner`  | Two cooperating views attach to one existing host session. | Both retain the same owner/session identity; a view creates no duplicate owner. |
| `same-name`   | Two hosts expose equal project/folder labels.              | Labels do not merge host bindings or execution identities.                      |
| `navigation`  | Select A, B and Scratch while A runs.                      | Existing processes, authority grants and provider prompts remain unchanged.     |
| `stale-owner` | Replace or make the observed host epoch unverifiable.      | Old references refuse new input/effects before sending to a replacement.        |

<a id="fp-ctx-02"></a>

### FP-CTX-02 — Register, relink, archive and remove projects

Projects opens with recent/pinned projects, host availability, active sessions and
attention counts. `Add existing folder` uses the native chooser and backend validation;
`Create project folder` shows the exact path in a native confirmation and creates only
the chosen empty directory, never initializes Git or launches an agent implicitly.
Importing a project/profile does not trust its code or grant provider access.

Git is optional. Record a separate checkout/worktree root beneath the project when
needed; do not assume Git remote URL or branch is a unique project identity. Duplicate
canonical folders on one host offer the existing project instead of creating an alias.
An explicitly linked second-PC folder is a new host-local binding, not filesystem
replication. The current workspace store/Owner route is local-host only; actual multi-host
registration and operation remain part of the remote target.

Missing or changed roots become `missing`/`identity_changed`; show metadata read-only,
block new scoped operations, and offer native relink with a comparison. Existing
processes are observed, never killed because their folder disappeared. Relink cannot
retarget a running session. Path eligibility remains operation-specific: existing configuration authoring restricts
roots inside HOME, while Native launch accepts its separately validated chooser/known
folders (including HOME). Registration never widens either capability. A future extension
to configuration roots requires the project-identity/security review; a rejected root is not silently relocated.

Archive hides a project from recent work without stopping anything. Removing a record
requires separate choices for retaining session history; never delete its files, Git
worktrees, provider history or vault data. A project with active/ambiguous work cannot
be purged; it may be archived with a visible active-work marker.

**AC-FP-CTX-02:** each case below is required at its applicable evidence level.

| Case suffix      | Stimulus                                                                         | Required observable outcome                                                                                    |
| ---------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `register`       | Confirm one eligible existing root.                                              | Create one project/binding/default workspace, without Git initialization or agent launch.                      |
| `duplicate`      | Register the same canonical root on the same host again.                         | Return the existing project; create no duplicate project or binding.                                           |
| `cancel`         | Cancel the chooser before confirming a root.                                     | Create no directory or project record.                                                                         |
| `ineligible`     | Supply a missing, linked, replaced or foreign-writable root.                     | Refuse registration with its reason, retaining existing records and files.                                     |
| `relink`         | Relink a project whose old binding has live sessions.                            | New launches use the new binding; existing sessions retain their original binding and cwd.                     |
| `partial-create` | Directory creation succeeds but metadata publication fails.                      | Report the created unregistered folder; preserve it and publish no partial project.                            |
| `archive`        | Archive/unarchive a project with live work.                                      | Change discoverability only; retain an active-work indication and the running sessions.                        |
| `remove`         | Remove proven-ended metadata, then attempt removal with live or unprovable work. | Metadata-only removal respects retention; unsafe purge refuses and never deletes project/provider/vault files. |

<a id="fp-ctx-03"></a>

### FP-CTX-03 — Workspaces and scratch terminals

`New workspace` creates an empty named arrangement; `Duplicate` copies layout and
launch preferences, not live process references or approvals. The default workspace
offers `Open terminal` and `Start agent`. A global **Scratch** context supports ordinary
terminals with no project/provider/authority. Adopting a Scratch terminal assigns its
previously null project origin once, after a
fresh disclosure of its actual process/cwd. It preserves the terminal/process identity and
mode, removes its Scratch placement and leaves it detached/discoverable for explicit
attachment in the target project. Only an unassigned plain shell is eligible; this cannot
move a Native/Managed session or execute cd, restart, mode change or permission grant.

Rename/reorder/pin workspaces changes presentation. Closing a workspace detaches its
views; `Stop selected sessions` is a separate reviewed operation listing exact sessions.
Closing the last placement leaves an empty arrangement. Deleting the last/default
project workspace retains or creates an empty default, without starting a shell. Scratch
is a singleton and cannot be deleted or duplicated. Live sessions remain discoverable
for reattachment; project metadata removal is not a process-stop action.

**AC-FP-CTX-03:** each case below is required at its applicable evidence level.

| Case suffix        | Stimulus                                                                                      | Required observable outcome                                                                                                       |
| ------------------ | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `duplicate`        | Duplicate a workspace containing live sessions.                                               | Allocate new placement IDs and copy preferences, with no live associations, approvals or new processes.                           |
| `detach`           | Close/detach a workspace view.                                                                | Release its view/input claim; leave the underlying execution discoverable.                                                        |
| `last-workspace`   | Delete the default or last project workspace.                                                 | Keep or create an empty default arrangement, without launching a shell.                                                           |
| `scratch`          | Open an explicit Scratch shell with no provider or authority available.                       | Plain terminal use works; Scratch is not an agent/profile/Managed context.                                                        |
| `adopt`            | Review and adopt one unassigned Scratch shell using its current process/cwd disclosure.       | Assign project-origin metadata once, preserve process/cwd/mode, detach its Scratch placement and offer attachment in the project. |
| `invalid-adoption` | Change the disclosed cwd/host/selection, or select a Native/Managed/already-assigned session. | Refuse adoption without changing ownership or execution.                                                                          |

<a id="fp-ctx-04"></a>

### FP-CTX-04 — Navigation and context safety

Global navigation contains Projects, Devices, Provider administration, Global
configuration, Global profiles, Global history and Settings. Within a selected project,
Workspace, Sessions/agents, Work, Configuration, Profiles, Models, Approvals and History
are contextual sections. Global attention links open the exact host/project/session,
never silently change the target of an already-open confirmation or composer.

Every write/launch/approval shows host, project, provider, mode, scope and target. Each
async request captures the context revision; a later selection cannot redirect it.
Changing a scoped editor's own target invalidates its unconfirmed plans after warning
about unsaved native-editor text. Navigating another independent view does not retarget
that editor. Confirmed/in-flight operations settle only against their captured original
context and display their receipt there. No cross-project Undo or approval reuse.
The current Projects/configuration selectors are independent; REQ-01 owns the missing
contextual bridge, so this target is not inferred from Projects navigation alone.

**AC-FP-CTX-04:** each case below is required at its applicable evidence level.

| Case suffix           | Stimulus                                                           | Required observable outcome                                                                                 |
| --------------------- | ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| `before-confirmation` | Switch project while a chooser or unconfirmed scoped plan is open. | Invalidate the old selection/review; it cannot silently become an operation on B.                           |
| `after-admission`     | Switch A to B after A's operation was accepted.                    | Reconcile only A's receipt/effect; B remains untouched.                                                     |
| `late-result`         | Return an asynchronous A response after the view changed to B.     | Do not label it as B's state or redirect B's composer.                                                      |
| `two-windows`         | Keep A's editor/confirmation open while another window selects B.  | Each operation retains its captured target; shared selection cannot retarget an existing draft or approval. |
| `layout-save`         | Switch with a pending layout save for A.                           | Save/refuse A's metadata explicitly; do not move its placements to B or affect execution.                   |

## 2. Screens and interaction contract

All screens have `loading`, `empty`, `current`, `stale`, `offline`, `denied`, `failed`
and `unsupported` states as applicable. Stale data shows source/time, action buttons
requiring freshness disable with the exact reason. Errors explain what happened,
whether an effect is known, and the next safe action; an ambiguous result is never
labelled success or retried as a fresh operation. Secret content is absent from
attention counts, OS notifications, diagnostics and general history.

| Surface       | Main content and actions                                                         | Failure/empty behaviour                                                         |
| ------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Projects/home | Host selector, projects, pinned workspaces, live work and attention              | No projects → add folder or Scratch; offline host → last-known metadata         |
| Workspace     | Tabs/splits, native terminal or Managed turn panel; identity strip; start/attach | Empty → explicit open/start; lost session → recover/reattach options            |
| Sessions      | Native and Managed groups with distinct state, provider, profile, resources      | Partial host/authority outage leaves the other plane usable                     |
| Work          | Handoffs, finite pipelines, dependencies, ownership/conflicts, stop              | Unsupported automation shows blocker, not a runnable button                     |
| Configuration | Scope/source tree, three-state inventory, plan/history/Undo; native authoring    | Bad source disables only that source; vault locked disables dependent writes    |
| Profiles      | Templates, versions, inheritance, applied/unsupported/excluded preview           | Missing parent/version refuses launch; import grants nothing                    |
| Models        | Offered/configured/requested/reported facts; effort; provenance                  | No catalogue is unknown, not an empty entitlement list                          |
| Approvals     | Exact action, scope, expiry, generation, complete diff/command                   | Stale/expired action cannot Accept; deny/cancel remain reachable                |
| History       | Separate authority/config/native/workflow/remote/metrics filters                 | Output gap or unavailable stream is labelled; no fabricated merge order         |
| Global admin  | Devices/grants, provider versions/capabilities, services/update status, vault    | Unavailable service has diagnostic/retry; never restarts live services silently |
| Settings      | Appearance, shortcuts, accessibility, budgets/retention preferences, opt-ins     | Cannot change execution policy, grant privileges or bypass an accepted decision |

Keyboard focus returns to the initiating control after cancel; destructive confirmation
does not default to Accept. Context badges and execution state sit outside terminal
titles/provider text. No output can impersonate the application chrome.

## 3. Terminals and independent agents

<a id="fp-term-01"></a>

### FP-TERM-01 — Layout, input and output

A tab contains a normalized split tree; each leaf has one terminal or Managed panel.
Keep TI1–TI9/PS1–PS6 from the [terminal contract](contracts/terminal-host.md#terminal-layout-invariants)
for terminal leaves: deterministic weights/focus, bounded depth, fit-before-input,
hidden panes keep size, constrained display at insufficient space and no terminal key
capture during IME composition. Managed panels consume no PTY slot. Admission counts
PTYs and runtime resources independently across all projects, not once per workspace.

Support new/split/close/resize/equalize/zoom/reorder, plain bounded scrollback search,
selection/copy and guarded paste. Plain search uses the existing implementation;
no new search add-on is required. Documented leader/remappable shortcuts
include a way to send the leader to the program. Full-screen alternate buffer, mouse,
UTF-8, dead keys and screen-reader operation require reference-terminal acceptance.
OSC clipboard writes/reads, arbitrary URL launch and output-driven application actions
remain disabled unless separately reviewed; terminal output is untrusted data.

**AC-FP-TERM-01:** each case below is required at its applicable evidence level.

| Case suffix      | Stimulus                                                                         | Required observable outcome                                                                                              |
| ---------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `geometry`       | Split/resize/zoom/close at the owning layout's count/depth/size boundaries.      | Preserve focus and normalized geometry; reject overflow without a hidden PTY allocation.                                 |
| `keyboard`       | Use terminal chords, dead keys and IME composition in a focused pane.            | Deliver intended program input without intercepting composition or hidden-pane keys.                                     |
| `search`         | Search plain scrollback at its configured limits.                                | Return bounded matches without interpreting a regular expression or changing the program.                                |
| `input-overflow` | Enqueue input beyond the 64 KiB retained-input bound, including in-flight bytes. | Refuse that new input visibly before partial admission; preserve already admitted input order.                           |
| `hostile-output` | Emit hostile titles, OSC clipboard/link and string sequences.                    | Treat output as terminal data without clipboard, URL or application actions.                                             |
| `claim`          | Observe and request input from two cooperating project views.                    | Only the current claimant can submit input/resize through the claim-aware API; disclose the same-UID/older-client limit. |

<a id="fp-term-02"></a>

### FP-TERM-02 — Persistence and fidelity

Save layout/preferences/opaque session references, not terminal bytes or approvals.
Reopen first queries the host and reattaches surviving identities. Dead/unknown sessions
show `Lost`/`Outcome unknown`; offer explicit individually selected restart through
the full current launcher checks, unchecked by default. No layout restore replays a
command, prompt or provider trust answer. Opening a new plain shell is explicit in a
restored dead slot; initial Scratch may open one ordinary shell as today's product does.

Faithful reattachment needs bounded volatile screen state, alternate screen, cursor,
modes, resize generation and a labelled retained-history tail. A byte ring alone is
not final acceptance. The parser is isolated from PTY ownership (§architecture); its
failure degrades fidelity without terminating sibling sessions. Output gaps and a
resync are visible. Full power-off cannot preserve a process.

**AC-FP-TERM-02:** each case below is required at its applicable evidence level.

| Case suffix       | Stimulus                                                                                   | Required observable outcome                                                                                  |
| ----------------- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `snapshot`        | Reattach a surviving terminal through a supported snapshot path.                           | Draw its bounded screen before committing the one-use output cursor; do not restart the program.             |
| `fallback`        | Refuse/lose a helper or fail to draw a snapshot.                                           | Replay only retained output with an explicit gap/fidelity label; disclose any fallback resize/redraw effect. |
| `parser-failure`  | Kill or stall only the parser in a controlled acceptance run.                              | Report lost fidelity without terminating the PTY, siblings or host.                                          |
| `missing-owner`   | Lose the host while descendants or effects may remain.                                     | Show lost/unknown state; do not restart a command automatically.                                             |
| `layout-recovery` | Read a corrupt or newer layout.                                                            | Preserve original bytes and recover/refuse according to its version, without destructive reset.              |
| `lifecycle`       | Exercise GUI exit, lock, logout with/without lingering, suspend and power loss separately. | Report each actual outcome; never infer process survival from a saved layout.                                |

<a id="fp-ses-01"></a>

### FP-SES-01 — Native launch and provider fidelity

Select project/workspace, Claude Code or Codex, Native, profile and placement. Preview
resolved executable/version, actual directory, requested model/effort, controls inherited,
excluded and unsupported. Launch the provider's own CLI through the bounded backend
catalogue and tested shell recipe. No free frontend argv or shell injection.
Provider login/trust/permission prompts remain in its terminal and require the user's
normal interaction. Wyss never answers them automatically.

Native uses real global/project configuration or supported session overrides without
rewriting global files. Incompatible profile fields refuse launch as a whole; users may
edit the profile, not silently ignore fields. Version/exclusion changes invalidate a
previously acknowledged preview. Authentication stays with the provider; no copying,
export or new sign-in store is implied. Original slash commands, hooks, skills, MCP,
plugins, subagents and provider-specific features remain available through its CLI,
subject to explicit disclosed provider/profile restrictions. Native delegation workers
have their own narrower configuration and must not be presented as unrestricted Native.
Wyss exposes a GUI control only with versioned capability evidence.

**AC-FP-SES-01:** each case below is required at its applicable evidence level.

| Case suffix         | Stimulus                                                                    | Required observable outcome                                                            |
| ------------------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `qualified-launch`  | Launch each qualified provider/profile from a reviewed project binding.     | Use the exact bounded argv/environment recipe and leave provider files unchanged.      |
| `invalid-preview`   | Cancel or change the provider build, profile or exclusion set after review. | Refuse before spawn until a new applicable preview is reviewed.                        |
| `unsupported-field` | Request an unsupported profile field.                                       | Refuse the launch as a whole rather than silently omit the field.                      |
| `provider-prompt`   | Encounter a provider sign-in, trust or permission prompt.                   | Leave the decision to its normal user interaction; Wyss supplies no automatic answer.  |
| `observed-settings` | Run two Native sessions with distinct requested settings.                   | Preserve their launch revisions and distinguish requested, reported and unknown facts. |

<a id="fp-ses-02"></a>

### FP-SES-02 — Managed lifecycle and modes

Choose Claude Code or Codex, Managed, then a supported **Inference** or **Local Tools**
profile. The provider card states `available`, `unverified`, `blocked` or `not implemented`
per profile. Codex has an admitted production F8 Local Tools route; Claude adapter/bridge
code exists but admission remains blocked by the exact gates/evidence in STATUS. Selection
names those blockers instead of claiming the adapter is missing, launching Native or
borrowing Codex's lease. The F8 production spawner refuses its unqualified inference route. Managed inference is
text-only; local tools are explicitly mediated. Hooks/MCP/skills cannot be enabled
by calling the mode hybrid. New capability profiles need safety contracts and evidence.

The user prepares a session without execution, starts an attested per-session runtime,
grants an expiring scoped lease, binds/resumes its provider conversation explicitly and
submits a turn. Grant alone executes nothing. Session create/resume, turn admission and
tool execution use their own scopes. UI distinguishes desired/actual runtime state,
attachment and lease state. Reconnection never grants/renews/starts/resumes a turn.
Projects Managed launch records a requested safety profile and prepares a session only;
runtime start, lease, binding and turn remain separate explicit operations. Native profile
selection is a distinct type and cannot stand in for a Managed safety profile.
Model/effort selection remains disabled until its separate authority extension is accepted
and qualified; existing implementation alone does not open it.

**AC-FP-SES-02:** each case below is required at its applicable evidence level.

| Case suffix            | Stimulus                                                                                   | Required observable outcome                                                                                       |
| ---------------------- | ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------- |
| `prepare-only`         | Choose Managed in Projects and submit its launch request.                                  | Prepare a session association/requested safety profile only; start no runtime, lease, binding or turn implicitly. |
| `grant-only`           | Grant or read a valid lease without turn submission.                                       | Create no provider turn or tool execution.                                                                        |
| `invalid-scope`        | Submit with wrong authority/store/session/generation, expired lease or unattested profile. | Refuse before effect; never reuse another provider's grant.                                                       |
| `blocked-claude`       | Select an implemented but unadmitted Claude profile.                                       | Name its current gate/evidence blockers; do not claim the adapter is absent or fall back to Native.               |
| `profile-availability` | Request a profile the selected production spawner cannot attest.                           | Refuse that exact profile; F8 Local Tools availability does not establish F8 inference support.                   |
| `capacity`             | Start a second F8 runtime while its production admission domain is full.                   | Refuse before allocation, retaining the existing runtime and cap one.                                             |

<a id="fp-ses-03"></a>

### FP-SES-03 — Lifecycle, stop and recovery

Native display lifecycle: `requested → running → exited/stopped/failed`; the Native wire
enum uses `failed_start` for a failed launch. Lost observation becomes
`lost`, never guessed exited. Restart makes a new session linked to its predecessor.
Managed runtime state follows the authority's generation/fence; turns move
`starting → running → completed/interrupted/failed/indeterminate`. User-visible waiting
approval is a turn substate. Closed sessions stay historical. Provider conversation
resume is distinct from process restart, UI reattach and explicit context handoff.

Stopping one session targets only its owned resource. Cancellation/deny remains
available after lease expiry. Pending approvals invalidate on loss; accepted effects
without confirmed outcome become indeterminate. Restart reconciliation launches nothing
implicitly; a wanted-but-stopped F8 runtime offers Stop and then Close. No prompt or
accepted action is replayed after a crash. Detached native descendants may outlive their
terminal: show uncertainty and block duplicate automated effectful retries.

**AC-FP-SES-03:** each case below is required at its applicable evidence level.

| Case suffix      | Stimulus                                                                          | Required observable outcome                                                                     |
| ---------------- | --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `same-intent`    | Retry an accepted Managed intent with its original identity/payload.              | Return/reconcile the original receipt without a second dispatch; conflicting payload refuses.   |
| `crash-boundary` | Interrupt start, binding, approval, dispatch or completion at each boundary.      | Distinguish no-send from possibly-sent work and preserve an indeterminate result when required. |
| `stale-reply`    | Deliver a predecessor generation's response after a successor is active.          | Leave successor and sibling state unchanged.                                                    |
| `stop-target`    | Stop/close one owned session after authority restart using fresh status fences.   | Affect only that target and keep pending/unknown cleanup truthful.                              |
| `resume-kind`    | Request UI reattach, provider conversation resume and process restart separately. | Require each operation's own preconditions; never substitute one for another.                   |

<a id="fp-ses-04"></a>

### FP-SES-04 — Multiple agents and resource admission

Independent Native sessions can coexist across projects. F8 Managed execution has a per-session runtime identity and one authoritative admission
domain across its projects/windows, currently capped at one. P1 and P2 are separate owners;
this is not an OS-wide limit on arbitrary processes or Native/delegated sessions. The final
concurrency count is a qualified reference-PC result, not a UI preference. Reaching
capacity shows the blocker and permits retry/cancel, never spawns a hidden queue.
OL3 has a separate bounded scheduling queue; it cannot change authority capacity.

**AC-FP-SES-04:** each case below is required at its applicable evidence level.

| Case suffix        | Stimulus                                                                | Required observable outcome                                                                                  |
| ------------------ | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `admission`        | Reach and exceed a PTY, runtime or bounded scheduler-queue limit.       | Refuse before allocation at the relevant owner; project/window count cannot multiply that owner's allowance. |
| `reap`             | Crash or cancel an owned worker while a descendant remains unreaped.    | Retain its capacity until owned cleanup finishes.                                                            |
| `control-fairness` | Stall/flood one provider during another session's status/cancel/revoke. | Preserve control availability within qualified budgets, not a claimed hard OS deadline.                      |
| `refresh`          | Propose raising F8 capacity above one.                                  | Keep production unchanged until the exact provider/build refresh and resource protocol is qualified.         |

## 4. Configuration and profiles

<a id="fp-cfg-01"></a>

### FP-CFG-01 — Real sources and three states

Global administration edits account-wide sources; Project configuration starts scoped
to that project; Session details show its fixed launch snapshot and subsequent observed
changes. Show system policy read-only, generated/provider-owned files read-only except
verified provider-native writer operations. Show source/precedence and imports; do not
impose a universal merge algorithm on different providers.

Each item separates **persisted**, **provider-resolved** and **session-effective** with
evidence/freshness. File write success does not prove consumption/reload. Unknown,
documented/inferred, observed and stale are visually distinct. External edits update the
inventory and invalidate plans; overflow uses bounded rescan and marks stale state.
Do not launch a provider/model just to populate a screen without an authorized probe.

**AC-FP-CFG-01:** each case below is required at its applicable evidence level.

| Case suffix     | Stimulus                                                                  | Required observable outcome                                                                      |
| --------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `provenance`    | Inspect inherited/system/account/project sources with conflicting values. | Preserve each provider's source/precedence and persisted/resolved/session-observed distinctions. |
| `bad-source`    | Make one inventory source malformed or unreadable.                        | Mark that source unavailable without inventing values or disabling unrelated valid sources.      |
| `external-edit` | Replace a source externally after inventory.                              | Mark stale data and invalidate affected plans before mutation.                                   |
| `consumption`   | Create an instruction file successfully without provider evidence.        | Show persisted content as consumption-unverified rather than effective.                          |

<a id="fp-cfg-02"></a>

### FP-CFG-02 — Author, switch, plan, apply, Undo

Where supported, list/create/edit/delete/enable/disable/restore rules, instruction files,
skills, hooks, MCP and plugins. Unsupported cells have reasons, not invented parity.
Executable hook/MCP text and secret-bearing template input use native authoring surfaces.
The page sends bounded metadata/IDs, never an arbitrary executable command for mutation.

Flow: read → draft → validate → review plan → native confirmation where required →
recheck context/source identity/revision → journal intent → approved writer → read-back
verify → outcome. Expired/stale/conflicted plans cannot apply. Absence differs from an
empty file. Exact Undo requires the expected postimage; outside changes never permit a blanket
overwrite. Entry restoration may replan only under its accepted kind-specific contract
(for example, reviewed OpenCode plugin append after divergence). Partial multi-file progress is reported per file;
no claim of cross-file atomicity. Imported templates only prefill a new plan.

Preserve the actual accepted writer and vault semantics, including their external-writer
limits. Historical proposed atomic-exchange/settling protocol is not a built guarantee.
Do not claim that hash+rename detects every concurrent write, that a project lock
stops external editors, or that private directories exclude sync/snapshots automatically.

**AC-FP-CFG-02:** each case below is required at its applicable evidence level.

| Case suffix     | Stimulus                                                                            | Required observable outcome                                                                                  |
| --------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `cancel`        | Cancel native authoring or confirmation before admission.                           | Leave target files and mutation records unchanged except any explicitly recorded refusal metadata.           |
| `stale-plan`    | Change context, identity, digest, trust or ACL before apply.                        | Refuse the stale plan without writing a different target.                                                    |
| `write-failure` | Fail journal/seal before write, then fail outcome recording after a possible write. | Distinguish no-write from recovered/unverified outcome; never promise a multi-file rollback.                 |
| `absent-file`   | Create a formerly absent file after preview while another writer creates that path. | No-replace publication refuses instead of overwriting it or treating absent as empty.                        |
| `undo`          | Undo an unchanged postimage, then a conflicting/diverged postimage.                 | Restore exact bytes/absence only when valid; otherwise use the kind-specific reviewed restoration or refuse. |
| `template`      | Import a template and later confirm its new scoped plan.                            | Import alone writes no provider file; applied/verified-file and provider-consumed are separate results.      |

<a id="fp-cfg-03"></a>

### FP-CFG-03 — Profiles and precedence

Global profile library, project defaults and session selection are separate features.
These versioned launch profiles are Native intent, not Managed safety profiles or
execution grants. Any future Managed preset uses its accepted typed selection contract.
A Native profile is versioned non-secret intent: provider applicability, parent, requested model,
effort, supported permission mode, hook/skill choices and notes. Resolve parents first,
child scalar wins and lists combine per documented field rules; cycles, missing parents,
depth limits and incompatible fields refuse. Pin the resolved revision at launch.

Selection order is explicit session selection → project default → As configured.
This selects a profile, not provider settings precedence; providers still resolve their
own scopes and system constraints. Editing a profile/default affects later launches
only. A named global-switch preset previews all real file writes and affected sessions;
it never impersonates a session-local overlay. Imported profiles grant no permissions.

**AC-FP-CFG-03:** each case below is required at its applicable evidence level.

| Case suffix      | Stimulus                                                                        | Required observable outcome                                                                          |
| ---------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `selection`      | Combine an explicit Native profile, project default and provider configuration. | Resolve profile selection separately from provider settings precedence and pin the applied revision. |
| `invalid-parent` | Resolve a cycle, missing version/parent or depth overflow.                      | Refuse without a launch or partial profile.                                                          |
| `coexistence`    | Start two qualified Native profiles and edit their library/default afterward.   | Existing sessions retain their own revision; no shared-file swapping occurs.                         |
| `mode-boundary`  | Import a Native profile or choose a Managed safety profile.                     | Neither substitutes for the other or changes execution scopes/authority gates.                       |

<a id="fp-cfg-04"></a>

### FP-CFG-04 — Restoration, credentials and templates

Secret-bearing restoration is always sealed under Secret Service custody.
The accepted custody contract permits an explicitly disclosed private plaintext fallback only for entries
classified secret-free when custody is unavailable; instructions/templates have later
always-sealed contracts. The final target is sealed restoration for all kinds, but
removing that older fallback requires a focused reviewed policy extension before release; logs/indexes hold metadata only. Credentials management
can add/replace/verify/delete sealed keys but returns no plaintext key to the
page and currently injects none into a provider. These are distinct record domains.
Locked/lost keys report locked/unrecoverable, never create a replacement key over data.
Removal warns if it destroys the only recovery copy. Review reminders never purge it.

Export/import is explicit, encrypted, domain/version checked and confirmed; no implicit
PC replication of vault/key/credential/provider-session data. Wrong passphrase, unknown
domain or unavailable key changes nothing. Native unredacted reveal, where offered,
requires the dedicated reviewed surface and is never an event payload. Recovery
incidents preserve evidence, disclose risk and require an explicit recovery act.

**AC-FP-CFG-04:** each case below is required at its applicable evidence level.

| Case suffix        | Stimulus                                                            | Required observable outcome                                                                                     |
| ------------------ | ------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `custody`          | Lock/remove the custody key and attempt a secret-bearing operation. | Refuse unavailable custody; preserve sealed data and report unrecoverable key loss without overwriting its key. |
| `fallback`         | Attempt a classified secret-free operation without custody.         | Use only the accepted disclosed fallback; always-sealed kinds remain sealed or refused.                         |
| `bad-import`       | Import with a wrong passphrase, domain or unsupported version.      | Publish no restored credential/template/provider mutation.                                                      |
| `restore-conflict` | Restore across outside edits or a crash during mutation.            | Preserve the remaining recovery material and require digest/identity reconciliation.                            |
| `retention`        | Reach the review-reminder age for a user-held restoration entry.    | Remind without deleting it; metric retention cannot purge custody or restoration records.                       |
| `credential-check` | Verify a sealed key record.                                         | Prove custody/format only, not provider authentication or permission to deliver the key.                        |

<a id="fp-cfg-05"></a>

### FP-CFG-05 — Capability registry

Every action is gated by provider, exact build/range with justification, mode, profile,
scope, operation and source format. Record documentation/fixture/real-run evidence,
date, limitations, reload semantics and invalidation. Configuration and execution
adapters are separate. Unknown protocol/events fail closed for Managed; unknown
configuration becomes read-only where mutation safety is unproven. Native CLI use
may continue with an explicit version warning without claiming a managed guarantee.

**AC-FP-CFG-05:** each case below is required at its applicable evidence level.

| Case suffix          | Stimulus                                                    | Required observable outcome                                                                       |
| -------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `changed-build`      | Upgrade a provider beyond the qualified build/range.        | Invalidate affected evidence and show unverified/unsupported controls without changing its files. |
| `unknown-format`     | Encounter an unrecognized configuration mutation surface.   | Keep inspection read-only until a safe writer is qualified.                                       |
| `imported-manifest`  | Import a descriptor claiming a new command/path/permission. | Treat it as data; do not extend backend allowlists or execution authority.                        |
| `surface-separation` | Qualify one configuration or Native surface.                | Do not infer Managed mediation or another provider/profile's qualification.                       |

## 5. Models, reasoning and usage

<a id="fp-mod-01"></a>

### FP-MOD-01 — Four facts and safe selection

Keep **offered**, **configured**, **requested** and **reported in use** separate. Attach
source/time/grain (account/session/turn) and generation. A thread-reported model is not
proof for every turn. A catalogue is not entitlement. No private reasoning is captured;
only provider-exposed summaries may be shown transiently.

Native launch uses verified provider flags/overrides; running-session changes use only
a proven supported provider interface. Otherwise guide the user to its own control,
without typing slash commands automatically. Native profile model/effort requests and
their configured/requested/provider-reported/observed facts remain distinct. Managed
model/effort has typed implementation behind a closed authority extension: validate the current generation's catalogue and effort, bind the
selection to admission, reject unsupported values and report reroutes only when observed.
Never switch provider/model automatically outside an explicitly reviewed substitution rule.

**AC-FP-MOD-01:** each case below is required at its applicable evidence level.

| Case suffix             | Stimulus                                                                         | Required observable outcome                                                             |
| ----------------------- | -------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `four-facts`            | Offer/configure/request one model while the provider reports another or nothing. | Retain distinct source/grain labels and unknown values; a catalogue is not entitlement. |
| `late-observation`      | Receive a reroute/model report from an old turn/generation.                      | Do not relabel the current or sibling turn.                                             |
| `unsupported-selection` | Request a missing model/effort or stale catalogue.                               | Refuse without a silent substitute or provider dispatch.                                |
| `closed-extension`      | Open Managed selection before its accepted versioned capability is available.    | Keep its controls disabled without hiding existing read-only observations.              |
| `native-control`        | Change a running Native session's setting without a qualified interface.         | Offer the provider's own control; inject no slash command automatically.                |

<a id="fp-mod-02"></a>

### FP-MOD-02 — Usage and historical metrics

Show only provider-reported usage/limits, source, unit, reset time, age and completeness;
absence is unknown, not zero. Do not turn subscription usage into API cost or promise a
hard money cap when only an unenforceable estimate exists. Separate live observations
from opt-in bounded on-device metric history. Default history off; proposed retention
30 days/10,000 records, whichever is first, with explicit pruning disclosure and export/
reset. This new metadata policy requires its own reviewed store contract before use.
It can never purge vault/recovery entries. Duplicate origin events are not counted twice.

**AC-FP-MOD-02:** each case below is required at its applicable evidence level.

| Case suffix       | Stimulus                                                                       | Required observable outcome                                                               |
| ----------------- | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| `omission`        | Receive no usage field or only a provisional observation.                      | Show unknown/partial with provenance, not zero.                                           |
| `duplicate`       | Deliver the same origin usage event twice.                                     | Do not increment the same counter twice.                                                  |
| `mixed-units`     | Compare subscription allowance, API units and provider estimates.              | Preserve units/grain and avoid fabricated totals or money caps.                           |
| `history-off`     | Open the product with history disabled or policy unaccepted.                   | Persist no history and initiate no background provider query to populate it.              |
| `history-failure` | Under an accepted opt-in policy, fill/lock/corrupt the optional history store. | Report partial/unavailable history without blocking control or touching protected stores. |
| `pruning`         | Under that policy, cross a retention/count bound or export/reset history.      | Affect only the allowlisted bounded metrics store; preserve vault/audit/provider history. |

## 6. Permissions, observability and safety

<a id="fp-sec-01"></a>

### FP-SEC-01 — Distinct authority boundaries

Native is full user-account execution under the provider's own controls. Managed is
Wyss authority-owned execution under its exact accepted profile; provider sandboxing,
application mediation and human approval authenticity are separate claims. Same-UID
socket authentication is not an unforgeable human approval boundary. Accepted Local Tools
commands retain accepted Local Tools residual credential-read and pre-existing hardlink risks; this is
not absolute confinement of an arbitrary approved command. Rootful Docker/
other admin delegation may defeat a proposed distinct-UID boundary; F8 does not solve it.

The target separates terminal/provider display from configuration/credential authoring
into differently permissioned windows/WebViews. Shared project context crosses a closed,
validated interface; changing a Svelte route is insufficient. This limits frontend exposure
and does not establish a protected principal against same-UID compromise. The current main
window combines permissions, so this remains an implementation gap.

Preserve leases, instance/generation fencing, current state/revision, workspace/profile
attestation and exact action fingerprint checks immediately before effect. No view,
profile, coordinator, plugin, handoff or remote device becomes a source of leases or
permission escalation. No broad argv/env/command field enters a Managed API. Additional
provider features need separately attested profiles rather than relaxation of Local Tools.

**AC-FP-SEC-01:** each case below is required at its applicable evidence level.

| Case suffix       | Stimulus                                                                                    | Required observable outcome                                                                 |
| ----------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `pre-dispatch`    | Reject a stale identity, substituted action, invalid scope/profile or required audit write. | Produce no effect when refusal precedes dispatch.                                           |
| `post-dispatch`   | Lose confirmation of an effect that might have been accepted.                               | Preserve its intent and unknown outcome; dispatch no fresh retry.                           |
| `window-boundary` | Invoke credential/configuration actions from a terminal-only surface.                       | Deny those commands through its actual permission boundary.                                 |
| `principal-limit` | Evaluate same-UID/admin access or a planner-produced capability claim.                      | Do not advertise protected approval or admit authority absent the separate proven boundary. |

<a id="fp-sec-02"></a>

### FP-SEC-02 — Review and approvals

Approval displays the exact complete bounded command/directory or every file/change/diff,
provider, host/project, turn and generation, lease scope/expiry and risk. Truncation or
missing details disables Accept. Decline/cancel needs no new execution grant. Native
launch confirmation, DAG scheduling consent, provider prompt and Managed tool Accept
are different decisions. A remote biometric does not confirm a host-only config dialog.

Use revision-checked idempotent intents. A disconnected client decides nothing. An
unconfirmed accepted effect becomes indeterminate and cannot be replayed. Stale UI
re-fetches metadata and asks for new review; never auto-accept a successor operation.

**AC-FP-SEC-02:** each case below is required at its applicable evidence level.

| Case suffix        | Stimulus                                                          | Required observable outcome                                                                           |
| ------------------ | ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `exact-action`     | Change command/directory/diff/fingerprint after review.           | Refuse the stale answer and require review of the exact current action.                               |
| `incomplete`       | Omit/truncate required approval details.                          | Disable Accept while retaining the applicable decline/cancel path.                                    |
| `duplicate-answer` | Submit repeated or conflicting approval decisions.                | Reconcile one original intent; never dispatch the action twice.                                       |
| `expiry`           | Expire execution permission before a decline/cancel decision.     | Require no new execution grant for refusal, while retaining authenticated channel/grant requirements. |
| `disconnect`       | Disconnect/reconnect after a decision may have reached authority. | Show current/unknown receipt state without automatically accepting a successor.                       |

<a id="fp-sec-03"></a>

### FP-SEC-03 — Observation and audit

Project attention lists pending approvals, failed/lost work, config conflicts, stale
capabilities and budget pressure. Histories preserve their original owners/stream IDs;
cross-stream sorting is display-only, never a reconstruction of authority. All metadata
has provenance/freshness. Native resource samples are observations, not containment.

Authority audit excludes prompts, commands, output, diffs, reasoning and secret content.
Separate transient streaming from durable control metadata and provider-owned legitimate
conversation history. Diagnostics are explicit and redacted before export; include
versions, capability/refusal words and counts. No automatic telemetry/public uploads.
Desktop notifications default off per category and carry no sensitive payload.

**AC-FP-SEC-03:** each case below is required at its applicable evidence level.

| Case suffix      | Stimulus                                                                | Required observable outcome                                                                                                                          |
| ---------------- | ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `stream-gap`     | Reconnect with a stale cursor or unavailable subsystem.                 | Retain owner/stream provenance and explicitly show gaps/staleness.                                                                                   |
| `content`        | Seed sensitive prompt/output/command/key text in a controlled case.     | Keep it out of general durable metadata, diagnostic and notification channels; sealed custody/provider-owned history retain their separate policies. |
| `diagnostics`    | Export diagnostics intentionally.                                       | Redact before export and disclose bounded versions/counts/refusal facts without automatic upload.                                                    |
| `notification`   | Leave category notifications disabled, then enable an allowed category. | Default to off and expose no sensitive lock-screen payload.                                                                                          |
| `resource-label` | Display Native RSS/CPU or soft budget observations.                     | Label measurements as observations, not hard containment guarantees.                                                                                 |

## 7. Handoff and work coordination

<a id="fp-hand-01"></a>

### FP-HAND-01 — Explicit context handoff

Source session → select excerpts/file references/objective → review exact payload and
target host/project/provider/mode/profile → confirm immutable digest → prepare recipient
under its normal authorization → deliver first input → show Delivered/Failed/Unknown.
Source stays intact. Payload is quoted untrusted context, not authority or private model
state. Project switch transfers no context. Cross-PC handoff explicitly warns about
unavailable files and requires a new target folder binding; no automatic file sync.

The current Native handoff brief is <=16 KiB and Native-only. Final extension adds structured
provenance, expiry and source revision; keep this bound unless separately qualified.
Draft content is volatile by default. Optional saved handoff content needs opt-in sealed
storage and its own privacy contract, never the workflow metadata journal. Failure
after possible delivery is Unknown and blocks automatic resending to an effectful target.

**AC-FP-HAND-01:** each case below is required at its applicable evidence level.

| Case suffix        | Stimulus                                                           | Required observable outcome                                                                   |
| ------------------ | ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------- |
| `stale-review`     | Change selected payload, source generation or target after review. | Invalidate the reviewed digest and do not deliver.                                            |
| `receiver`         | Prepare a supported receiver for reviewed untrusted context.       | Preserve source work and require the receiver's own fresh permissions/identity.               |
| `unknown-delivery` | Lose the acknowledgment after possible first-input delivery.       | Show Unknown and block automatic resend to an effectful receiver.                             |
| `unavailable`      | Refuse an unavailable or unqualified recipient before delivery.    | Preserve source/context and persist no payload silently.                                      |
| `saved-content`    | Request durable handoff content.                                   | Require an accepted opt-in sealed policy; never put content in the workflow metadata journal. |

<a id="fp-orc-01"></a>

### FP-ORC-01 — Finite supervised OL3

Work creates a bounded acyclic DAG: objective, project/host binding, immutable task IDs,
dependencies, required/optional outputs, provider/mode/profile/model, effect class,
maximum attempts/parallelism/time/resources and stop policy. Show exact plan revision/
digest before scheduling consent. User changes require a new revision and approval;
agents may propose amendments but cannot extend an approved run. No recursive or event-
triggered OL4 scheduler. A separate headless coordinator survives UI detach.

Run states: Draft → Validating → Awaiting review → Ready → Running →
Completed/Failed/Cancelled/Expired/Needs review; Pause stops new dispatch and records
already-running steps. Step states: Proposed → Waiting dependencies → Eligible →
Awaiting permission → Queued → Starting → Running → Verifying → Succeeded, with
Blocked/Skipped/Failed/Cancelled/Needs review outcomes. Success requires all required
steps verified and no unaccounted live effect. An optional skipped step satisfies a
dependency only if the reviewed DAG explicitly allows missing output.

Use distinct run/step/attempt IDs. Journal intent before dispatch; reconcile spawn/ACK
ambiguity against the owning host/authority. Never retry ambiguous effects. Retry of
known no-effect failure is bounded by the reviewed plan and gets a new attempt ID;
denied permission is not a retryable provider error. Reconnect/restoring journal gives
no new authority. Execution remains serialized where cap 1 applies.

**AC-FP-ORC-01:** each case below is required at its applicable evidence level.

| Case suffix          | Stimulus                                                             | Required observable outcome                                                                       |
| -------------------- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `invalid-plan`       | Submit a cyclic, oversized, expired or changed plan.                 | Start no step until the exact finite revision satisfies review and admission.                     |
| `dependency`         | Fail a required predecessor or skip an optional output.              | Block dependents unless the reviewed DAG explicitly permits that missing output.                  |
| `dispatch-ambiguity` | Crash or lose a reply at attempt admission/dispatch.                 | Retain the original attempt/intent and dispatch no duplicate effect.                              |
| `pause-stop`         | Pause/cancel/expire/revoke a run with a step already active.         | Stop new admission and distinguish scheduling state from termination/settlement of existing work. |
| `quota`              | Set a budget for which the provider supplies no enforceable counter. | Apply local bounded proxies and label any cost estimate as soft.                                  |

<a id="fp-orc-02"></a>

### FP-ORC-02 — Filesystem ownership and resource fairness

Each task declares read/propose/write intent over a specific checkout. Conflicting
Console writers serialize by default; explicit parallel writes require separate
worktrees and reviewed merge ownership. Never auto-merge over external edits or dirty
work. Unexpected file identity/revision change pauses promotion and asks for review.
An ownership lock coordinates cooperating tasks only; it is not OS confinement.

Bound queued work, active runtimes/PTYs, stream buffers, attempts, wall time and metadata.
Fair admission reserves cancellation/revocation capacity. Declared model budget without
provider counters is a soft estimate; enforce local step/time/resource bounds instead.
Stop reports scheduling stopped separately from observed process termination.

**AC-FP-ORC-02:** each case below is required at its applicable evidence level.

| Case suffix    | Stimulus                                                            | Required observable outcome                                                                        |
| -------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `writers`      | Schedule overlapping writers to one checkout.                       | Serialize/reject conflicts before dispatch; parallel work requires reviewed independent worktrees. |
| `outside-edit` | Change the root identity or workspace digest externally.            | Block promotion/settlement and preserve unreviewed work.                                           |
| `verification` | Finish a provider step without its required workspace verification. | Retain the claim and prevent dependent promotion until valid settlement.                           |
| `pressure`     | Flood output or stall a provider at resource bounds.                | Keep cancellation/revocation available within the declared enforcement level.                      |
| `cleanup`      | Clean up a completed or interrupted worker.                         | Signal owned handles only; use no PID discovered by name/enumeration.                              |

<a id="fp-orc-03"></a>

### FP-ORC-03 — Native and planner trust gates

Managed steps use the authority's accepted, capability-negotiated guard APIs; an
unguarded ordinary turn is not an alternate route around a missing workspace guard. Automated full-account Native dispatch and
model-output-to-Native handoff remain `BLOCKED_TRUST_BOUNDARY` until an accepted,
independently proven OS/approval boundary preserves required provider functionality.
Whole-DAG consent is not per-launch current-content consent. B1 staged/quarantined
work is a distinct proposed restricted-worker lane, not unrestricted Native and not
production Managed; its launcher/staging/export machinery exists under the fixture feature, but that is
not admitted production execution, useful real-provider qualification or protected approval.

Hermes as Wyss Agent planner may propose data only; every effect belongs to Wyss policy/
execution. OS denial of direct effectful escape is required before that integration.
Do not make a sandbox pass by disabling the useful workflow and claiming full parity.

**AC-FP-ORC-03:** each case below is required at its applicable evidence level.

| Case suffix       | Stimulus                                                                   | Required observable outcome                                                                                                          |
| ----------------- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `native-gate`     | Submit a full-account Native automated step without the accepted boundary. | Keep it blocked despite whole-plan consent.                                                                                          |
| `b1-gate`         | Offer the implemented restricted-worker fixture as a production lane.      | Refuse production admission; fixture code is not qualification or protected approval.                                                |
| `planner`         | Parse hostile Hermes/planner output.                                       | Retain data-only proposals; grant no direct effect or forged approval.                                                               |
| `useful-boundary` | Qualify a proposed effect lane.                                            | Require useful positive workflow and negative escape evidence for its exact provider/build, plus independent review where specified. |

## 8. Remote PCs and native Android

<a id="fp-rem-01"></a>

### FP-REM-01 — Device enrollment and scoped gateway

From an authorized host, Pair creates a short-lived offer. Android/PC verifies pinned
host identity, binds device identity and compares the pairing proof on both devices;
host-local confirmation enrolls with watch-only privileges. Device grants are explicit,
expiring, revocable and checked on every request. Watch, managed intents, config proposals
and full-account terminal control are separate grants. Relay/network membership grants
nothing. Default no remote listener/enrollment; turning on access is an explicit deployment
action after transport/authentication review.

Target connectivity is LAN/direct where feasible, encrypted authenticated remote P2P
with relay fallback and no manual per-connection VPN operation. Transport/vendor/relay
hosting remains DN-05, with evidence comparison in the roadmap. No vendor or working transport is established
by the current local core or offline Android UI. Relay must not see
plaintext application content. Host key change stops reconnection pending local review.
No exposed authority socket, private signing scheme or fabricated hardware attestation.

**AC-FP-REM-01:** each case below is required at its applicable evidence level.

| Case suffix        | Stimulus                                                      | Required observable outcome                                                             |
| ------------------ | ------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `pairing`          | Expire/replay a pairing offer or change pinned host identity. | Refuse enrollment/reconnection until the required fresh local proof.                    |
| `watch`            | Enroll a watch-only device.                                   | Grant neither terminal input, configuration writes nor raw authority access.            |
| `revoke`           | Revoke or mark a device suspect.                              | Deny subsequent dependent gateway requests without pretending past effects are undone.  |
| `transport-change` | Move between direct and relay paths.                          | Preserve authenticated host/device identity and scope; relay membership grants nothing. |
| `default`          | Start without an accepted/configured remote deployment.       | Open no production listener or enrollment automatically.                                |

<a id="fp-rem-02"></a>

### FP-REM-02 — Terminal input, reconnect and absent host

Viewing is distinct from input. Input is full host-user shell power, default off,
requiring explicit risk consent and verified per-use presence where supported. One
writer claim per terminal binds host/session/gateway generation, device, claim epoch,
expiry and input sequence. Observers cannot resize the authoritative grid; the claim
owner does. Claim loss disables input immediately and keeps watching if allowed.

The input result carries the accepted byte count: a PTY write can be partial. Never
claim all-or-nothing delivery of a keystroke batch. Within one gateway instance duplicates
are refused; a known partial result shows exactly the accepted prefix and requires a new
explicit user action for any remainder. After possible write-before-ACK
or restart, report Outcome unknown and never replay buffered keystrokes. Reconnect
fetches a checkpoint/tail with gaps, reacquires a fresh claim and retains project
identity. Offline clients queue no arbitrary shell input or stale approvals.

Full terminal input while the host is locked/logged out remains blocked pending the
specific absent-host approval/principal decision (OD-02 in roadmap). Lingering can keep
existing host work alive; it cannot authenticate a remote human or unlock the wallet.
Remote config requests are proposals awaiting fresh host confirmation; full shell access
can bypass that API using account rights, and must be disclosed as such.

**AC-FP-REM-02:** each case below is required at its applicable evidence level.

| Case suffix     | Stimulus                                                                  | Required observable outcome                                                                                                          |
| --------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `observer`      | View a terminal without a valid input claim.                              | Permit only the granted observation; deny input and authoritative resize.                                                            |
| `known-partial` | Receive a known accepted prefix for an input batch.                       | Report that prefix and require a new explicit action for a known remainder.                                                          |
| `lost-ack`      | Lose an input acknowledgment or restart the gateway after possible write. | Show unknown outcome and replay no buffered keystrokes.                                                                              |
| `claim-loss`    | Expire/revoke the writer claim.                                           | Deny newly admitted input; continue watching only if its independent grant remains valid.                                            |
| `presence`      | Lock/log out the host or lose required wallet/key/presence availability.  | Deny privileges with missing prerequisites without stopping already authorized work; absent-host full-account input remains blocked. |

<a id="fp-rem-03"></a>

### FP-REM-03 — Native Android behaviour

Native Android has Hosts, Pair, Projects/workspaces, Sessions, Terminal, Work/pipelines,
Approvals, Devices/grants, Notifications and Settings. Cover host availability, pairing/code comparison, grouped session states, terminal/input claim,
pipeline progress, exact approval/deny, non-sensitive notifications, device grants, read-only
configuration proposals and offline last-known state with no queued input; project/workspace
selector sits between host selection and session attachment. No PWA substitute.

Terminal has Ctrl/Esc/Tab keys, explicit keyboard toggle, paste-size/multiline guard,
current writer indicator and detach separate from stop. Approval opens exact fresh
authority action after authentication/step-up, with deny reachable after execution-lease expiry while the device remains authenticated
and holds the required valid remote grant.
App background, Android process death and network loss leave host work under its
existing finite authorization; reconnect does not resend. Store device keys using the
platform keystore and verify claimed presence properties on actual supported hardware.
If unavailable, deny the dependent privilege and explain why. Notifications contain no
prompt/path/output; locked screen shows only non-sensitive counts. No durable terminal
transcripts or clipboard sync by default.

**AC-FP-REM-03:** each case below is required at its applicable evidence level.

| Case suffix           | Stimulus                                                       | Required observable outcome                                                       |
| --------------------- | -------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `hardware`            | Enroll/use a device on supported physical Android hardware.    | Verify actual keystore/presence properties or deny the dependent privilege.       |
| `input-ui`            | Use IME, extra keys, guarded paste and narrow/rotated layouts. | Keep input/deny/stop controls usable without stealing composition.                |
| `background`          | Background/kill the Android app and reconnect.                 | Preserve host work under its existing authorization and resend no input/approval. |
| `reinstall-revoke`    | Reinstall, invalidate a key or revoke a lost device.           | Treat identity changes explicitly and reject stale credentials/claims.            |
| `locked-notification` | Receive attention while the phone is locked.                   | Show only non-sensitive permitted metadata, never prompt/path/output.             |

## 9. Performance, accessibility and reliability

<a id="fp-nfr-01"></a>

### FP-NFR-01 — Measured resource efficiency

Terminal prompt/input stays usable while configuration scans, Managed work or remote
reconnect fail. Hidden views do no unnecessary render/timer work; discovery/catalogues
are demand-driven. State/queues have hard counts/byte ceilings; scrollback, screen,
transport backlog and renderer cache have separate budgets. Global memory admission
reduces/rejects new work without silently trimming existing retained history solely
because a threshold was crossed. Do not promise hard process-tree containment from
sampling `/proc` or direct-child PDEATHSIG.

Use M01..M33 and benchmark corpora/procedures in
[measurement contract](contracts/acceptance.md). Budgets require explicit agreement;
a metric definition is not a measurement. Qualify idle/start/input/render/throughput/search/panes/reconnect,
two-provider work and long-running sessions on both reference PCs and actual Android.
Report app and provider costs separately, p50/p95, repetitions, software/display setup
and preserved live-work conditions. Budgets are frozen from those results before
release; missing results mean NOT_PROVEN, not a passing performance score.

**AC-FP-NFR-01:** each case below is required at its applicable evidence level.

| Case suffix        | Stimulus                                                                     | Required observable outcome                                                                                         |
| ------------------ | ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `bounded-work`     | Exercise each named queue/buffer at and beyond its count/byte ceiling.       | Refuse/degrade as specified without retained unbounded growth or silently lost admitted data.                       |
| `reference-budget` | Measure a named M01–M33 metric on an exact reference setup.                  | Compare actual samples against an agreed budget; missing measurements do not pass.                                  |
| `cost-boundary`    | Measure Console/WebKit, host, authority and provider trees.                  | Report their separate costs and ownership uncertainty without calling soft RSS a hard limit.                        |
| `background`       | Hide views or fail configuration/authority while using an existing terminal. | Preserve required output/control work while avoiding unnecessary view work.                                         |
| `preservation`     | Prepare a measurement on the owner's environment.                            | Preserve existing apps/sessions; perform no cache drop, desktop restart or provider call merely to obtain a number. |

<a id="fp-nfr-02"></a>

### FP-NFR-02 — Usability and accessibility

Dark, light and high-contrast themes, accessible terminal ANSI palette, keyboard-only
flows, visible focus, labels and non-colour state indicators are final requirements.
Respect reduced motion and system text scale. Default shortcuts do not steal provider/
IME/desktop keys; remapping detects conflicts. Screen-reader terminal support requires
actual WebKitGTK and Android testing. Progressive detail does not hide host/mode/risk.

**AC-FP-NFR-02:** each case below is required at its applicable evidence level.

| Case suffix | Stimulus                                                               | Required observable outcome                                                                             |
| ----------- | ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `contrast`  | Check each theme's semantic text, controls and focus.                  | Meet ordinary-text 4.5:1 and large-text/control-boundary 3:1 requirements with non-colour state labels. |
| `keyboard`  | Complete each supported journey without a pointer.                     | Preserve focus, cancellation and provider/desktop/IME chords.                                           |
| `assistive` | Exercise actual WebKitGTK screen-reader and Android TalkBack journeys. | Verify the offered controls on real environments; source/DOM presence is insufficient evidence.         |
| `display`   | Change text scale, reduced motion and viewport size.                   | Preserve readable actions and access to denial/stop controls without clipping essential scope/risk.     |

<a id="fp-nfr-03"></a>

### FP-NFR-03 — Personal delivery and recovery

Keep private local packages, component version/feature negotiation, safe GUI-only
updates preserving host/authority sessions and explicit update-pending status. An
incompatible host/authority cannot silently receive new fields. Installed migrations,
service restarts and rollback remain reviewed operational actions accounting for live
work. Keep a consistent store backup and matching binaries; no SQL downgrade or copying
only a live SQLite DB while ignoring WAL. P1/V7 and the separately versioned P2 history remain independent; P2 has validated
V8/V9/V10 source support, not an automatic installed-store upgrade;
retiring protocol 1 is separate, not part of redesign migration.

Corrupt/newer stores fail safely without overwriting them; unavailable authority does
not stop plain terminals. Graceful close detaches viewers; explicit stop is separate.
Host crash/power loss yields truthful unknown/indeterminate state and no replay.
Uninstall defaults to application removal retaining user data; deleting data is separate
and names what would be lost. No public publishing/auto-update/telemetry is implied.

**AC-FP-NFR-03:** each case below is required at its applicable evidence level.

| Case suffix     | Stimulus                                                            | Required observable outcome                                                                  |
| --------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `mixed-version` | Update only the GUI while compatible host/authority work is active. | Preserve owner/process identities and disable unsupported new controls explicitly.           |
| `backup`        | Back up and restore control state plus required witnesses.          | Demonstrate a consistent set and declared loss window, not merely a copied live SQLite file. |
| `corruption`    | Open a corrupt/newer/oversized store.                               | Preserve original bytes and refuse unsafe mutation rather than reset/downgrade.              |
| `interruption`  | Interrupt install/migration/rollback within its declared scope.     | Keep a recoverable old/new state and disclose any required authority/host stop.              |
| `uninstall`     | Remove the application without choosing data purge.                 | Retain user/provider/recovery data; explicit purge separately names the affected data.       |

## 10. End-to-end journeys and outcomes

| Journey                                      | Steps and successful terminal state                                                   | Required failure outcome                                                      | Acceptance coverage     |
| -------------------------------------------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------- |
| JF-01 First use                              | Open → choose host → add project → new workspace → open shell                         | Missing folder/provider/authority does not block Scratch                      | CTX-01..03, TERM-01     |
| JF-02 Native Claude                          | Project → Start → Claude Native/profile → preview → launch → own CLI                  | Unsupported profile refuses before spawn; trust prompt stays provider-owned   | SES-01, CFG-03          |
| JF-03 Native Codex alongside Claude          | Split/new pane → Codex Native → work independently                                    | No shared profile-file swap; capacity refusal preserves Claude                | SES-01/04, TERM-01      |
| JF-04 Managed Codex                          | Prepare → runtime ready → lease → bind → turn → exact tool review → complete          | No scope/attestation → refused; lost accepted effect → indeterminate          | SES-02/03, SEC-01/02    |
| JF-05 Managed Claude                         | Same lifecycle through its accepted adapter/profile                                   | Admission blocked; no Native fallback                                         | SES-02, CFG-05          |
| JF-06 Change a project rule/hook             | Scope → native author → plan → confirm → verify → separate consumption check          | Outside edit/project change cancels plan; Undo checks current bytes           | CTX-04, CFG-01/02/04    |
| JF-07 Change model/effort                    | Inspect four facts → supported selection → observe reported result                    | Unavailable/unsupported/unknown stays labelled, no silent reroute             | MOD-01/02               |
| JF-08 Hand off                               | Select context → review bytes/target → new authorized session → Delivered             | Stale digest blocks; possibly delivered result is Unknown, no resend          | HAND-01, SEC-01         |
| JF-09 Coordinated implementation/test/review | Bounded DAG → consent → resources → verify dependencies → complete                    | Native boundary/merge conflict/denial pauses; no duplicate retry              | ORC-01..03              |
| JF-10 Reopen/log out                         | Save layout → detach UI → surviving host → query/reattach                             | Dead/ambiguous work gets explicit restart proposal, never auto-spawn          | TERM-02, SES-03, NFR-03 |
| JF-11 Second PC/Android                      | Pair watch → choose host/project → view → explicit input/approval grant               | Roaming/lock/revoke invalidates claims; unknown input not replayed            | REM-01..03              |
| JF-12 Recovery/update                        | Inspect component versions → preserve live work → approved compatible update/recovery | Locked vault/corrupt store/incompatible host remains explicit and recoverable | CFG-04, NFR-03          |

### JF-13 — Switch away from live work and return

1. Register A and B, explicitly start one qualified Native agent in A, and record its
   terminal/session identity, host epoch, launch profile revision and current output offset.
2. Select B and open only its metadata. A's process, prompt, cwd, grants and profile stay
   unchanged; any pending save remains A's metadata operation. No B agent/shell starts.
3. Return to A and locate the existing association. Do not infer a new process from a
   missing renderer or a changed visible title.
4. Attach through the checked owner; require the matching epoch and supported screen path.
   Draw before cursor commit. Where only ring recovery is currently implemented, show its
   replay/gap/fidelity limitation rather than claim this final screen criterion passes.
5. Acquire input explicitly. A second cooperating view remains read-only; account/older-client
   interference remains outside the cooperative claim guarantee.
6. Replace or lose the host at each boundary in a controlled test. Refuse stale sends and
   preserve unknown work; do not automatically restart or resend input.

Coverage: AC-FP-CTX-01/navigation, AC-FP-CTX-04/layout-save, AC-FP-TERM-02/snapshot,
AC-FP-TERM-02/fallback, AC-FP-TERM-02/missing-owner and AC-FP-SES-01/observed-settings. Current missing implementation: REQ-02 project screen recovery;
real provider/full-screen/IME acceptance still follows separately.

### JF-14 — Keep a configuration draft scoped while navigating

1. Open an explicitly identified configuration draft/plan for A. Record its project/binding/
   revision and the target file identity, digest and plan lifetime.
2. Select B in another project view. B's navigation cannot relabel or retarget A's editor,
   plan or native confirmation. Global configuration administration keeps its own scope.
3. Before confirmation, choose either to continue editing A or discard/review a new B plan;
   do not transfer executable draft content or reuse A's approval implicitly.
4. After A's effect was admitted, reconcile its A receipt even while B is visible. A failed
   response is not proof that no A write occurred; B stays unchanged.
5. Exercise outside edit, plan expiry, B selection and window closure separately. Preserve
   user drafts/recovery data according to the owning editor contract without overwriting a
   changed provider file.

Coverage: AC-FP-CTX-04/before-confirmation, AC-FP-CTX-04/after-admission,
AC-FP-CTX-04/late-result, AC-FP-CTX-04/two-windows, AC-FP-CFG-02/stale-plan,
AC-FP-CFG-02/write-failure and AC-FP-CFG-02/undo. Current selectors are independent; the missing scoped
bridge is REQ-01. Its bounded DTO/window design must be specified before implementation.

## 11. Extensions retained in final scope

<a id="fp-ext-01"></a>

### FP-EXT-01 — Other providers and Wyss Agent

OpenCode inventory/switches and existing authoring remain supported at their proven
scope. Hermes upstream Native is a separately version-qualified adapter/launcher target;
do not pretend it is currently installed/integrated. Wyss Agent is the owner's separate
policy/execution product, with Hermes intelligence behind an independently proven effect
boundary. Neither extension delays Claude/Codex parity milestones or uses a private
permission path. Original Wyss source/docs remain historical input, not a running
Console subsystem. Public distribution requirements remain conditional.

**AC-FP-EXT-01:** each case below is required at its applicable evidence level.

| Case suffix         | Stimulus                                                                          | Required observable outcome                                                                       |
| ------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `provider`          | Add or qualify a supported OpenCode/Hermes Native surface.                        | Record exact build/scope and useful positive/negative evidence; CLI syntax alone is insufficient. |
| `unknown-extension` | Offer an unavailable or unqualified integration.                                  | Show the reason without a silent provider/mode fallback.                                          |
| `planner-effects`   | Connect a Wyss/Hermes data-only planner proposal.                                 | Keep all effects mediated by accepted Wyss policy and boundary gates.                             |
| `scope`             | Evaluate an optional extension against the retained complete-product requirement. | Keep unmet scope visible instead of dropping it because implementation is difficult.              |

## 12. Source invariants and retained clarifications

These legacy identifiers remain in source comments/tests. Their obligations belong to
the current FP/AC groups below; the old phase and delivery-status columns are retired.

| Source alias | Current requirement   | Invariant                                                                          |
| ------------ | --------------------- | ---------------------------------------------------------------------------------- |
| FR-NAT-07    | FP-SES-01 / FP-SES-04 | Show native session CPU and memory; report provider process-tree cost separately.  |
| FR-NAT-08    | FP-SES-01 / FP-CTX-01 | Group sessions by project and agent without changing execution ownership.          |
| FR-SES-04    | FP-HAND-01            | Handoff is an explicit package reviewed by the user.                               |
| FR-ADP-05    | FP-CFG-05             | Adapter errors are closed words, without paths, values or raw provider text.       |
| NFR-MNT-03   | FP-CFG-05             | Unit tests are deterministic and do not need personal configuration or a provider. |
| NFR-REL-03   | FP-NFR-03             | Ordinary terminals do not depend on the authority or configuration subsystem.      |
| FR-UX-03     | FP-CTX-04             | Unknown/unverified states are visibly different from verified ones.                |

FP-CFG-04 retains held user records until explicit disposition: age does not purge them.
The target default is a 30-day review reminder, separate from optional metrics retention.
FP-NFR-01 requires a global terminal memory policy that adapts scrollback for **new**
terminals without silently trimming existing output; it is not a hard process-tree RAM
guarantee. Notifications are opt-in per category, default off, with no sensitive lock-screen
content. Native model/effort selection uses only a verified provider mechanism; never inject
commands into a running agent silently. Remote configuration proposals still require host
native confirmation. A remote control grant is powerful account access, not a restricted worker.

## 13. Verification precision and implementation handoff

This document describes target behavior. Present source, qualification, deployment and
acceptance remain separate facts in STATUS; a requirement is not evidence that its behavior
is already implemented. In particular, generic host transport no-replay, separated terminal/
configuration surfaces and production guarded coordination still have named source gaps.

For each AC-FP group, enumerate independently observable cases in the relevant implementation
task: normal path; boundary and invalid input; denied/stale identity or scope; interruption/
recovery; and real provider/device interaction where applicable. Reuse existing tests and
name uncovered cases. An optional case is not silently assumed applicable or passed.
Use case IDs derived from the stable group, such as `AC-FP-CTX-04/before-confirmation`,
without renumbering FP/AC definitions or recreating the retired historical crosswalk.

Assertions of zero effect apply only before an effect can have been accepted. Separate
no-send, known refusal, completed and unknown-after-send. After an interrupted effect,
preserve evidence and reconcile instead of promising rollback or absence of external effects.
"Fail closed" denies further authority; it does not undo an already executed command.
Atomic means a named store/file transaction, not an arbitrary multi-store journey.

Performance criteria refer to a named metric, workload, boundary, environment and agreed
budget. Missing measurements remain unresolved; do not invent thresholds to make a task
READY. Configured/requested/reported/used and accepted/implemented/qualified are distinct
labels. Every offered control needs its exact provider/version/scope and honest unsupported
behavior. [EXECUTION](EXECUTION.md) defines readiness and model handoff; the roadmap must link
each implementation slice to its relevant FP/AC and owning contract.
