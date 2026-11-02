# Final-product functional specification

This page owns target behaviour: all 31 FP requirements, user journeys and same-number
AC-FP acceptance specifications. They are requirements, not claims of implementation.
Read [product scope](PRODUCT.md) for owner decisions,
[implementation status](STATUS.md) for delivered evidence and
[architecture](ARCHITECTURE.md) for ownership/security constraints.

## 1. Product and context

<a id="fp-ctx-01"></a>

### FP-CTX-01 — Device → Project → Workspace → Sessions

A **device** is the client used to operate the product. A **host** is the PC executing
work; one PC can be both. The visible breadcrumb is `Host / Project / Workspace` with
connection age and local/remote indication. A project is a stable logical record with
one or more explicitly linked host-local folders. A workspace is a named working
arrangement within one project on one host: tabs, panes, session references, selected
profiles and work coordination. Sessions own execution; workspaces own presentation
and references. Provider thread IDs, terminal IDs and managed runtime IDs remain distinct.

There may be multiple workspaces and multiple sessions in one project, and several
open projects. A session belongs to one host/project at creation; switching the UI
does not move its working directory, provider conversation or authority. A workspace
may reference the same surviving session in another view, but input ownership remains
exclusive. Project names are display labels, never security identities or filesystem paths.

**AC-FP-CTX-01:** two clients view the same host session and see the same identities;
two PCs with the same directory name do not share sessions. Changing the active project
changes zero processes, leases or provider prompts. Identity collisions and stale host
generations are refused before input or execution.

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
An explicitly linked second-PC folder is a new binding, not filesystem replication.

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

**AC-FP-CTX-02:** chooser cancel creates no record/directory; duplicate/replaced/symlinked
roots and conflicting relinks refuse visibly. Archive preserves live sessions; remove
never deletes project files. New-folder failure leaves no half-registered project.

<a id="fp-ctx-03"></a>

### FP-CTX-03 — Workspaces and scratch terminals

`New workspace` creates an empty named arrangement; `Duplicate` copies layout and
launch preferences, not live process references or approvals. The default workspace
offers `Open terminal` and `Start agent`. A global **Scratch** context supports ordinary
terminals with no project/provider/authority. Adopting a scratch terminal into a project
adds a view association only and discloses its actual directory; it does not transform
it into a managed or registered agent session.

Rename/reorder/pin workspaces changes presentation. Closing a workspace detaches its
views; `Stop selected sessions` is a separate reviewed operation listing exact sessions.
Closing the last tab leaves an empty workspace. Deleting a workspace preserves sessions
and offers their reattachment from the project Sessions view.

**AC-FP-CTX-03:** duplicate/close/delete starts and stops no process. Scratch works
offline with no authority and no agents installed. Detached sessions remain discoverable.

<a id="fp-ctx-04"></a>

### FP-CTX-04 — Navigation and context safety

Global navigation contains Projects, Devices, Provider administration, Global
configuration, Global profiles, Global history and Settings. Within a selected project,
Workspace, Sessions/agents, Work, Configuration, Profiles, Models, Approvals and History
are contextual sections. Global attention links open the exact host/project/session,
never silently change the target of an already-open confirmation or composer.

Every write/launch/approval shows host, project, provider, mode, scope and target. Each
async request captures the context revision; a later selection cannot redirect it.
Switching project discards unconfirmed configuration plans after warning about unsaved
native-editor text; confirmed/in-flight operations settle against their original context.
The resulting receipt is displayed there. No cross-project Undo or approval reuse.

**AC-FP-CTX-04:** switch A→B during plan, native dialog and in-flight write; either refuse
before mutation or finish only A and report A. B's files/sessions remain unchanged.

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
no new search add-on is required . Documented leader/remappable shortcuts
include a way to send the leader to the program. Full-screen alternate buffer, mouse,
UTF-8, dead keys and screen-reader operation require reference-terminal acceptance.
OSC clipboard writes/reads, arbitrary URL launch and output-driven application actions
remain disabled unless separately reviewed; terminal output is untrusted data.

**AC-FP-TERM-01:** split/zoom/resize/close and search at bounds preserve focus and PTY
geometry; terminal keys/IME and tmux/editor chords reach the program. Oversized paste
sends nothing. Hostile title/escape/clipboard corpus produces no application action.

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

**AC-FP-TERM-02:** close/reopen GUI, parser crash, host crash, logout with/without
lingering, suspend and power-off have distinct outcomes. Reattachment matches reference
screens; no restart occurs while old descendants may still act. Newer layout versions
remain unmodified; corrupt layouts are preserved aside and recover to a usable empty view.

<a id="fp-ses-01"></a>

### FP-SES-01 — Native launch and provider fidelity

Select project/workspace, Claude Code or Codex, Native, profile and placement. Preview
resolved executable/version, actual directory, requested model/effort, controls inherited,
excluded and unsupported. Launch the provider's own CLI through the bounded backend
catalogue and tested shell recipe . No free frontend argv or shell injection.
Provider login/trust/permission prompts remain in its terminal and require the user's
normal interaction. Wyss never answers them automatically.

Native uses real global/project configuration or supported session overrides without
rewriting global files. Incompatible profile fields refuse launch as a whole; users may
edit the profile, not silently ignore fields. Version/exclusion changes invalidate a
previously acknowledged preview. Authentication stays with the provider; no copying,
export or new sign-in store is implied. Original slash commands, hooks, skills, MCP,
plugins, subagents and provider-specific features remain available through its CLI.
Wyss exposes a GUI control only with versioned capability evidence.

**AC-FP-SES-01:** real Claude/Codex Native workflows retain provider behaviour and
configuration; a fixture proves literal argv/environment derivation. Cancel, unsupported
field, changed version or changed exclusions starts nothing. No global file is swapped.

<a id="fp-ses-02"></a>

### FP-SES-02 — Managed lifecycle and modes

Choose Claude Code or Codex, Managed, then a supported **Inference** or **Local Tools**
profile. The provider card states `available`, `unverified`, `blocked` or `not implemented`
per profile. Today only Codex is implemented; Claude selection explains the missing
adapter instead of launching Native or borrowing Codex's lease. Managed inference is
text-only; local tools are explicitly mediated. Hooks/MCP/skills cannot be enabled
by calling the mode hybrid. New capability profiles need safety contracts and evidence.

The user prepares a session without execution, starts an attested per-session runtime,
grants an expiring scoped lease, binds/resumes its provider conversation explicitly and
submits a turn. Grant alone executes nothing. Session create/resume, turn admission and
tool execution use their own scopes. UI distinguishes desired/actual runtime state,
attachment and lease state. Reconnection never grants/renews/starts/resumes a turn.
Model/effort selection remains disabled until its separate authority extension lands.

**AC-FP-SES-02:** prepare/grant/read/reconnect creates no turn; expired/wrong-scope lease,
wrong generation/profile or unknown provider capability refuses before effect. At current
cap 1 a second runtime returns capacity refusal. Claude's eventual conformance must pass
independently, not through Codex fixtures. No fallback weakens the selected mode.

<a id="fp-ses-03"></a>

### FP-SES-03 — Lifecycle, stop and recovery

Native state: `requested → running → exited/stopped/failed`; lost observation becomes
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

**AC-FP-SES-03:** crash at start/binding/admission/Accept/dispatch/completion produces
truthful states and exactly one durable intent receipt; retry uses the same intent.
Stop/close after an authority restart works with the fence returned by status. A stale
response cannot affect a successor or sibling session.

<a id="fp-ses-04"></a>

### FP-SES-04 — Multiple agents and resource admission

Independent Native sessions can coexist across projects. Managed execution has a
per-session runtime identity and a global release limit, currently one. The final
concurrency count is a qualified reference-PC result, not a UI preference. Reaching
capacity shows the blocker and permits retry/cancel, never spawns a hidden queue.
OL3 has a separate bounded scheduling queue; it cannot change authority capacity.

**AC-FP-SES-04:** at/over every PTY/runtime/queue limit, refuse before allocation;
retain capacity until owned children are reaped. A stalled provider cannot block a
sibling's revocation/status/cancel. Multi-process credential refresh must be proven
before increasing current Managed capacity.

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

**AC-FP-CFG-01:** inherited/system/project conflict and external atomic replacement
show correct provenance; malformed/unreadable source affects no other source. A
newly authored instruction remains consumption-unverified until provider evidence exists.

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

**AC-FP-CFG-02:** cancel at every authoring/confirmation step changes nothing; race,
symlink/ACL/trust failure, disk full and journal/vault failure fail closed. Exact Undo
restores bytes/absence/metadata only when its precondition holds. Provider consumption
is a separate case, including confirmed imported-template application.

<a id="fp-cfg-03"></a>

### FP-CFG-03 — Profiles and precedence

Global profile library, project defaults and session selection are separate features.
A profile is versioned non-secret intent: provider applicability, parent, requested model,
effort, supported permission mode, hook/skill choices and notes. Resolve parents first,
child scalar wins and lists combine per documented field rules; cycles, missing parents,
depth limits and incompatible fields refuse. Pin the resolved revision at launch.

Selection order is explicit session selection → project default → As configured.
This selects a profile, not provider settings precedence; providers still resolve their
own scopes and system constraints. Editing a profile/default affects later launches
only. A named global-switch preset previews all real file writes and affected sessions;
it never impersonates a session-local overlay. Imported profiles grant no permissions.

**AC-FP-CFG-03:** two sessions with different profiles coexist without file swapping;
changes/default updates leave existing sessions labelled with original revision. Missing
parent/version/unsupported override gives a precise refusal; import cannot relax policy.

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

**AC-FP-CFG-04:** wallet locked/lost/full, malicious import, wrong domain/passphrase,
restore conflict and restart during mutation neither leak content nor destroy the only
copy. Credential verification proves sealing only, never provider authentication.

<a id="fp-cfg-05"></a>

### FP-CFG-05 — Capability registry

Every action is gated by provider, exact build/range with justification, mode, profile,
scope, operation and source format. Record documentation/fixture/real-run evidence,
date, limitations, reload semantics and invalidation. Configuration and execution
adapters are separate. Unknown protocol/events fail closed for Managed; unknown
configuration becomes read-only where mutation safety is unproven. Native CLI use
may continue with an explicit version warning without claiming a managed guarantee.

**AC-FP-CFG-05:** a provider upgrade invalidates affected capability evidence without
changing files or silently substituting features. A downloaded/imported manifest cannot
introduce a new command, path, schema or execution capability.

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
their configured/requested/provider-reported/observed fact labels close in M2. Managed
model/effort is a planned typed
authority extension: validate the current generation's catalogue and effort, bind the
selection to admission, reject unsupported values and report reroutes only when observed.
Never switch provider/model automatically outside an explicitly reviewed substitution rule.

**AC-FP-MOD-01:** missing observations stay unknown, a late reroute cannot relabel another
turn, unsupported efforts refuse before launch. New-host feature negotiation handles
older hosts honestly. Until the Managed extension is accepted, controls remain disabled.

<a id="fp-mod-02"></a>

### FP-MOD-02 — Usage and historical metrics

Show only provider-reported usage/limits, source, unit, reset time, age and completeness;
absence is unknown, not zero. Do not turn subscription usage into API cost or promise a
hard money cap when only an unenforceable estimate exists. Separate live observations
from opt-in bounded on-device metric history. Default history off; proposed retention
30 days/10,000 records, whichever is first, with explicit pruning disclosure and export/
reset. This new metadata policy requires its own reviewed store contract before use.
It can never purge vault/recovery entries. Duplicate origin events are not counted twice.

**AC-FP-MOD-02:** duplicates, late events, provider omission, mixed units and full disk
yield labelled partial/unknown values, never fabricated prices or lost protected records.

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

**AC-FP-SEC-01:** wrong/stale/cross-session identity, substituted action, expired scope,
storage/audit rejection and unsupported profile produce zero effect when rejected before
dispatch. A failure after a possibly accepted dispatch preserves its receipt and explicit
unknown/indeterminate outcome; it must not be mislabeled zero-effect or retried as new work. Cross-window tests deny configuration/credential commands from terminal-only surfaces.
Adversarial same-UID/admin scenarios cannot be advertised as protected until the separate
boundary is proven.

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

**AC-FP-SEC-02:** substitution, duplicate/reordered answers, disconnect after dispatch,
incomplete diff and lease expiry all preserve one-answer and no-replay guarantees.

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

**AC-FP-SEC-03:** crash/reconnect/cursor gap and unavailable subsystem are truthful;
secret-marker corpus is absent from every durable/general diagnostic/notification channel.

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

**AC-FP-HAND-01:** changed payload/source/target invalidates review. Escape/path/forged
tool instructions confer no capability. Native↔Managed creates a new session with fresh
permissions; unavailable recipient does not consume source or silently persist payload.

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

**AC-FP-ORC-01:** cyclic/unbounded/edited plans start nothing; failed required steps
block dependents. Crash before/after dispatch, lost ACK, pause, cancel, expiry and
revocation create no duplicate attempt/effect. Unknown quota uses an enforceable proxy
and cannot be called a monetary cap.

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

**AC-FP-ORC-02:** concurrent writer conflict, dirty merge, external editor, resource
pressure and provider stall cannot overwrite unreviewed work or starve cancellation.
No PID discovered by enumeration becomes a cleanup signal target.

<a id="fp-orc-03"></a>

### FP-ORC-03 — Native and planner trust gates

Managed steps use ordinary authority APIs. Automated full-account Native dispatch and
model-output-to-Native handoff remain `BLOCKED_TRUST_BOUNDARY` until an accepted,
independently proven OS/approval boundary preserves required provider functionality.
Whole-DAG consent is not per-launch current-content consent. B1 staged/quarantined
work is a distinct proposed restricted-worker lane, not unrestricted Native and not
production Managed; its typed pure contracts do not constitute a working worker.

Hermes as Wyss Agent planner may propose data only; every effect belongs to Wyss policy/
execution. OS denial of direct effectful escape is required before that integration.
Do not make a sandbox pass by disabling the useful workflow and claiming full parity.

**AC-FP-ORC-03:** compromised planner/native/transport cannot forge accepted permissions;
positive useful workflow and negative escape tests both pass per lane/provider/version.
Until then blocked steps remain visible final-product gaps, not removed requirements.

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
hosting remains DN-05, with evidence comparison in the roadmap. Relay must not see
plaintext application content. Host key change stops reconnection pending local review.
No exposed authority socket, private signing scheme or fabricated hardware attestation.

**AC-FP-REM-01:** expired/replayed pairing, wrong host, revoked/suspect device, missing
scope, audit failure and direct/relay migration cannot widen grants or reach authority
as raw protocol. Lost device revocation blocks future gateway use, not undo past effects.

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

**AC-FP-REM-02:** roaming/lost ACK/gateway crash does not duplicate input; observers
cannot claim write or resize. Lock/logout/wallet lock returns explicit denial without
stopping already authorized work. Revocation is not advertised as account cleanup.

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

**AC-FP-REM-03:** real phone keyboard/accessibility/rotation/background/reinstall/key
invalidation/lost-phone flows preserve identity and no-replay. At narrow viewport every
input/deny/stop control remains usable; software-only/emulator evidence is not hardware proof.

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

**AC-FP-NFR-01:** bound/storm/slow-consumer tests have no unbounded growth; qualified
reference runs meet the approved M budgets with raw provenance. No cache drop, desktop
restart or closure of the owner's sessions occurs merely to obtain a number.

<a id="fp-nfr-02"></a>

### FP-NFR-02 — Usability and accessibility

Dark, light and high-contrast themes, accessible terminal ANSI palette, keyboard-only
flows, visible focus, labels and non-colour state indicators are final requirements.
Respect reduced motion and system text scale. Default shortcuts do not steal provider/
IME/desktop keys; remapping detects conflicts. Screen-reader terminal support requires
actual WebKitGTK and Android testing. Progressive detail does not hide host/mode/risk.

**AC-FP-NFR-02:** keyboard and assistive-technology journeys work in all themes; ordinary
text contrast >=4.5:1, large text/control boundaries >=3:1. Native dialogs and narrow
viewports retain focus and cancellation. No success relies on colour or pointer alone.

<a id="fp-nfr-03"></a>

### FP-NFR-03 — Personal delivery and recovery

Keep private local packages, component version/feature negotiation, safe GUI-only
updates preserving host/authority sessions and explicit update-pending status. An
incompatible host/authority cannot silently receive new fields. Installed migrations,
service restarts and rollback remain reviewed operational actions accounting for live
work. Keep a consistent store backup and matching binaries; no SQL downgrade or copying
only a live SQLite DB while ignoring WAL. P1/V7 and P2/V8 histories remain independent;
retiring protocol 1 is separate, not part of redesign migration.

Corrupt/newer stores fail safely without overwriting them; unavailable authority does
not stop plain terminals. Graceful close detaches viewers; explicit stop is separate.
Host crash/power loss yields truthful unknown/indeterminate state and no replay.
Uninstall defaults to application removal retaining user data; deleting data is separate
and names what would be lost. No public publishing/auto-update/telemetry is implied.

**AC-FP-NFR-03:** mixed-version, interrupted-upgrade, full-disk, corrupt-store, recovery
and rollback rehearsals preserve prior data and live sessions within the chosen operation's
scope. Rollback of an operation that must stop a host cannot be advertised as transparent.

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

**AC-FP-EXT-01:** each added provider passes its capability/positive/negative fixtures,
Native integration and any separately required Managed safety review. No optional
planner bypasses Wyss effects; unavailable integrations remain explicit backlog items.

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
Use case IDs derived from the stable group, such as `AC-FP-CTX-04/switch-before-confirmation`,
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
