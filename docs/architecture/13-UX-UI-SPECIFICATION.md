# 13 — UX and UI specification

Every screen of the complete product, its states and the fifteen core journeys. Nothing here
is to be built now. Screens that exist are marked `[R]`; the rest is `[P]`. Before any screen
is built it goes through the project's design pipeline and gets a design contract of its own;
this document is the functional source for those contracts, not a substitute for them.

## 1. Principles

| #   | Principle                                                                                                 |
| --- | --------------------------------------------------------------------------------------------------------- |
| U1  | The terminal keeps the space. Everything else can be hidden with one action                               |
| U2  | The plane of everything shown is visible (FR-UX-06): **Native** or **Managed**, as text, not colour alone |
| U3  | A state that is not verified does not look verified (FR-UX-03)                                            |
| U4  | Nothing changes without a visible effect: what was written, where, and what did not change                |
| U5  | An error says what happened, what was left as it was, and what can be done (FR-UX-04)                     |
| U6  | A control for something not built is absent, or disabled with "not available yet" (FR-UX-05)              |
| U7  | The vocabulary is the user's: no internal service name, no raw code                                       |
| U8  | Every action is reachable from the keyboard; no key is taken from the terminal's program (C-12)           |

### 1.1 State vocabulary, used everywhere

| Word shown      | Meaning                                                | Drawn as                                     |
| --------------- | ------------------------------------------------------ | -------------------------------------------- |
| On / Off        | The provider's file says so                            | Solid indicator                              |
| Removed, kept   | Taken out by the Console and restorable here           | Hollow indicator with a "restore" affordance |
| Verified        | The provider reported it                               | Check mark and "verified"                    |
| Documented      | Deduced from the provider's manual                     | "per documentation" in muted text            |
| Not verified    | Unknown                                                | A question mark and "not verified"           |
| Changed outside | The file differs from what was last shown              | A banner on the source                       |
| Stale           | A running session predates a change it does not reload | A clock mark on the session                  |
| Blocked         | Not offered, with the reason                           | Disabled control and a sentence              |

State is never carried by colour alone (NFR-A11Y-03).

## 2. Global structure

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ ◆ Wyss Console   <view title>            Terminal ● Codex ● Authority ●  v…  │  top bar [R]
├──────────────┬───────────────────────────────────────────────────────────────┤
│ Console      │                                                               │
│ Sessions     │                                                               │
│ Configuration│                    active view                                │
│ Profiles     │                                                               │
│ Agents       │                                                               │
│ Approvals  2 │                                                               │
│ History      │                                                               │
│ Settings     │                                                               │
└──────────────┴───────────────────────────────────────────────────────────────┘
```

| Element        | Today `[R]`                                                                     | Target                                                                  |
| -------------- | ------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Navigation     | Console, Sessions, Agents, Approvals, Events, Settings (`src/lib/shell/nav.ts`) | Adds Configuration and Profiles; Events becomes History with three tabs |
| Top bar        | Three status pills that never disappear, down to 320 px                         | Adds a host selector when remote exists (§16)                           |
| Sidebar        | 216 px; an icon rail at 720 px and below                                        | Collapsible by the user at any width (U1)                               |
| Default window | 1440×810                                                                        | Unchanged                                                               |

Order of items is by frequency of use in the owner's workflow; it is a proposal (OQ-25).

## 3. Screens

Each table uses the same rows. "Contracts" names the interface of documents 05 to 12.

### S1 — Console (terminal workspace) `[R]`, extended

| Aspect        | Specification                                                                                                                                        |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purpose       | Use terminals. Nothing else competes for this view                                                                                                   |
| Layout        | Tab strip, a "New terminal" control, the active terminal filling the rest. Target: panes inside a tab; a "Start agent" control beside "New terminal" |
| Data          | Per tab: title, state; target: agent and plane when it is a native session                                                                           |
| Actions       | New terminal, close, switch, resize panes; target: split, search, start agent (opens S3)                                                             |
| Confirmations | Closing a terminal whose program is still running: one confirmation in the page                                                                      |
| Loading       | "Starting terminal…" in the status line `[R]`                                                                                                        |
| Empty         | "No terminals open. Create a new terminal to continue." `[R]`                                                                                        |
| Unavailable   | Never: the terminal does not depend on the authority or the configuration engine (NFR-REL-03)                                                        |
| Error         | "Terminal failed: <word>" from the closed list `[R]`; the tab stays so the last screen can be read                                                   |
| Offline       | Unaffected                                                                                                                                           |
| Keyboard      | Everything typed goes to the program. Console actions under one leader chord (OQ-14). Tabs: arrows, Home, End `[R]`                                  |
| Accessibility | Tabs are a tab list with panels `[R]`; the status line is a live region for the active tab only `[R]`                                                |
| Responsive    | The terminal refits on every size change `[R]`                                                                                                       |
| Contracts     | The terminal commands (05 §1)                                                                                                                        |
| Performance   | NFR-PERF-05 to 08, 10                                                                                                                                |

### S2 — Sessions (overview of both planes)

| Aspect        | Specification                                                                                                                                                                                                      |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Purpose       | See every session, in either plane, and reach it                                                                                                                                                                   |
| Layout        | A list grouped by project (or by agent, a toggle); each row: name, **plane badge**, agent, profile, state, started, resource sample. A detail panel on the right. Today the list holds managed sessions only `[R]` |
| Data          | Native: from the registry (05 §5). Managed: from the authority's session page `[R]`                                                                                                                                |
| Actions       | Native: go to its terminal, forget (when ended), start another on the same project. Managed: open agent, events, resume, enable Local Tools or use Inference, close `[R]`                                          |
| Confirmations | Managed close and mode change: as today `[R]`. Forget: none (nothing running)                                                                                                                                      |
| Loading       | Rows appear per owner as each answers; neither waits for the other                                                                                                                                                 |
| Empty         | "No sessions yet." with "Start an agent" and "New managed session"                                                                                                                                                 |
| Unavailable   | Authority down: the managed group shows "The authority is unavailable. Native sessions and terminals keep working." `[R]` for the wording about terminals                                                          |
| Error         | Per group, never the whole view                                                                                                                                                                                    |
| Keyboard      | List navigation with arrows; Enter opens; a visible focus ring                                                                                                                                                     |
| Accessibility | The badge is text ("Native", "Managed"); state is text                                                                                                                                                             |
| Responsive    | Detail panel below the list under 600 px of content `[R]` for today's layout                                                                                                                                       |
| Contracts     | IF-NATIVE.sessions; the session commands of ADR 0020 to 0023 `[R]`                                                                                                                                                 |
| Performance   | Resource samples are taken only while this view is visible                                                                                                                                                         |

### S3 — Start agent (launcher)

| Aspect        | Specification                                                                                                                                                                                                              |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purpose       | Start an installed agent in a terminal, on a project, with a profile                                                                                                                                                       |
| Layout        | A sheet over the Console: agent (from the catalogue, with version), project (known ones, or "Choose folder…" which opens the native dialog), profile (default "As configured"), then a summary of what the profile will do |
| Data          | Catalogue, known projects, profiles, the launch plan's three lists (09 §4.2)                                                                                                                                               |
| Actions       | Start; cancel                                                                                                                                                                                                              |
| Confirmations | None for "As configured". A profile with an item in "applied with a cost" shows that item and needs one confirmation                                                                                                       |
| Empty         | No agent installed: "No supported agent was found on this computer." with what was looked for                                                                                                                              |
| Unavailable   | An agent whose version has no evidence: startable, with "not verified with this version"                                                                                                                                   |
| Error         | "The profile cannot be applied to this agent: <list>"; "The folder no longer exists"                                                                                                                                       |
| Text shown    | Always: "This session runs as you, with the agent's own permissions. Wyss Console does not approve or contain what it does."                                                                                               |
| Keyboard      | Full; Enter starts; Escape closes                                                                                                                                                                                          |
| Contracts     | IF-NATIVE.catalogue, .start; IF-PROFILE.resolve                                                                                                                                                                            |

### S4 — Configuration

| Aspect         | Specification                                                                                                                                                                                                                                                                                                                              |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Purpose        | See and change the real configuration of each agent                                                                                                                                                                                                                                                                                        |
| Layout         | Provider tabs (installed ones). Scope selector: System, Global, Project (with a project picker). Category list: Rules, Hooks, Skills, MCP, Plugins, Settings. A list of entries; a detail panel. A fixed strip: "Native sessions use this. Managed sessions do not." For Codex hooks a second strip with the Inference compatibility state |
| Data           | Per entry: name, source file, scope, persisted state, effective state, sessions using it, last read time                                                                                                                                                                                                                                   |
| Actions        | F1: none that changes anything. Each source shows its path as selectable text, so that the user can open it in his editor; the Console opens nothing itself (the window has no opener permission `[R]`). F2a onward: disable, enable, restore, undo. F5: create, edit, delete                                                              |
| Confirmations  | Per risk class (11 §4): in the page for `normal`; a native dialog for `weakens_security` and `executes_code`                                                                                                                                                                                                                               |
| Loading        | Sources appear one by one; a slow source does not hold the others                                                                                                                                                                                                                                                                          |
| Empty          | "No hooks are configured at this scope." per category                                                                                                                                                                                                                                                                                      |
| Unavailable    | Provider not installed: "Codex is not installed. Its configuration files, if any, are listed read only."                                                                                                                                                                                                                                   |
| Error          | Per source: "Cannot be parsed at line N", "Cannot be read", "Larger than the limit", "In a folder others can write: read only"                                                                                                                                                                                                             |
| Offline        | Works without the authority and without a network                                                                                                                                                                                                                                                                                          |
| Outside change | A banner on the source: "Changed outside Wyss Console at 14:02: 1 hook removed." with "Show"                                                                                                                                                                                                                                               |
| Conflict       | "A conflict copy of this file exists (from your sync tool). Changes are blocked until it is resolved."                                                                                                                                                                                                                                     |
| Keyboard       | Tabs, list and panel in tab order; actions have shortcuts shown in tooltips                                                                                                                                                                                                                                                                |
| Accessibility  | Each entry's state is a sentence a screen reader reads whole: "Hook security-preflight, pre tool use, on, per documentation, used by 2 sessions"                                                                                                                                                                                           |
| Responsive     | Under 720 px the category list becomes a selector; the detail panel moves below                                                                                                                                                                                                                                                            |
| Contracts      | IF-CONFIG (07 §8)                                                                                                                                                                                                                                                                                                                          |
| Performance    | The first inventory of a provider within a proposed 200 ms on the fixture set (15 §4); NFR-PERF-09                                                                                                                                                                                                                                         |

Sub-screens by category follow 08: the hook detail shows event, matcher, handler and, for
Codex, "Trust: pending your review in Codex" when it applies; the rule detail shows what the
file imports and whether it is a link or generated; the MCP detail shows transport and the
names, never the values, of its variables.

### S5 — Plan and confirm (part of S4)

| Aspect  | Specification                                                                                                                                                                                                                     |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purpose | Show exactly what a change will do before it happens                                                                                                                                                                              |
| Layout  | What: the entry. Where: provider, scope, file. Diff: redacted. Effect: "2 running sessions adopt it; 1 keeps the previous state until restarted; managed sessions are not affected". Risk sentence when the class is not `normal` |
| Actions | Confirm; cancel                                                                                                                                                                                                                   |
| Then    | For risky plans the native dialog opens with the same facts. The page shows "Waiting for your confirmation in the system dialog"                                                                                                  |
| Failure | "The file changed since this was prepared. Nothing was written." with "Prepare again"                                                                                                                                             |
| After   | "Applied to settings.json." and the per-session result; "Undo" stays available in History                                                                                                                                         |

### S6 — Profiles

| Aspect        | Specification                                                                                             |
| ------------- | --------------------------------------------------------------------------------------------------------- |
| Purpose       | Create and maintain session profiles; see global profiles (F5) in a separate tab                          |
| Layout        | List; editor with the resolved result per agent beside it; version list                                   |
| Actions       | New from template, duplicate, save (new version), restore version, export, import, set as project default |
| Confirmations | Import shows what the profile asks for                                                                    |
| Empty         | The templates                                                                                             |
| Error         | "This profile extends itself", "Refers to something not present on this computer"                         |
| Contracts     | IF-PROFILE (09 §9)                                                                                        |

### S7 — Models

Part of the Agents view for managed sessions and of the session detail for native ones
(12 §4, §7). No separate navigation item. It shows the catalogue, the configured model and the
observed model with the exact wording of 12 §7.

### S8 — Agents (managed turns) `[R]`

As built (ADR 0022): tabs per open session, the turn list, the composer, "Cancel turn". Target
additions: the plane badge "Managed"; the model line of S7; a handoff action (F11a).

### S9 — Approvals `[R]`

As built (ADR 0023): pending actions, each with command or change, directory, deadline, Approve
and Deny. Unchanged. Its header keeps saying what it covers: "In Local Tools, every command and
file change Codex proposes stops here." It never lists anything from a native session.

### S10 — History

| Tab               | Content                                                   | Source               |
| ----------------- | --------------------------------------------------------- | -------------------- |
| Managed execution | The Events view as built `[R]`                            | The authority        |
| Configuration     | Changes, undos, refused plans, outside changes, conflicts | The engine's journal |
| Native sessions   | Starts and ends with exit codes                           | The registry         |

Three lists, never merged into one timeline (AD-11); a shared time filter lets the user read
them side by side.

### S11 — Settings `[R]`

As built (ADR 0026), two columns in a wide window. It stays the place for preferences that
decide nothing. Configuration of agents is not in it.

### S12 — Search and command palette

One palette under the leader chord: go to a view, a session, a project, an entry of the
configuration; run a Console action. It searches names the Console already holds, never file
contents. Terminal scrollback search is a separate control inside the terminal (TF2).

### S13 — Error and conflict states

Every such state is one of a closed list, each with a fixed sentence and the action offered.
The list is the union of the error words of 05 §4.2, 07 §8 and 09 §9; no screen invents a
message.

### S14 — Recovery flows

| Situation                                    | What the user is shown                                                                |
| -------------------------------------------- | ------------------------------------------------------------------------------------- |
| The window crashed with native sessions open | Sessions marked "lost", with what is known; "start again on this project"             |
| A change was interrupted                     | At start: "A change to settings.json was in progress. The file is intact (verified)." |
| A restoration record without its change      | Removed silently at start; noted in History                                           |
| The authority is not running                 | The notice and Retry, as built `[R]`                                                  |
| A parked entry's file changed                | The restore dialog explains and shows both                                            |

### S15 — Host selector (future, F10)

A control in the top bar naming the computer whose terminals and sessions are shown, with its
connection state. Absent until remote exists.

## 4. Responsive behaviour

The breakpoints that exist `[R]` (`src/app.css`): 1040, 940, 800, 720, 560, 520 and 420 px for
the top bar and sidebar; container queries for each view. New views follow the same rule: the
view's own container decides its layout, not the window. Minimum window 320×320 `[R]`: every
view must remain operable there, with lists becoming selectors and panels stacking.

## 5. Accessibility

| Requirement  | Rule                                                                                                                                                       |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| NFR-A11Y-01  | Tab order follows reading order; no keyboard trap; the terminal has a documented way out of focus                                                          |
| NFR-A11Y-02  | Text contrast at least 4.5:1 against its background, computed per pair; the terminal already sets a minimum contrast ratio of 4.5 `[R]` (`renderer.ts:45`) |
| NFR-A11Y-03  | Every state has a word                                                                                                                                     |
| NFR-A11Y-04  | Reduced motion respected `[R]` (`src/app.css`)                                                                                                             |
| Live regions | Only for things the user must hear: a pending approval, an outside change, the result of an action                                                         |

## 6. Performance requirements of the interface

No view may add work to the page while it is hidden. Lists are paged (50 rows) or windowed.
The Configuration view renders from a projection the backend already bounded (256 items a
page). Targets are in [15 §4](15-PERFORMANCE-ENGINEERING.md).

## 7. Copy rules

Sentences, not codes. The provider's own words for its own things ("hook", "skill", "MCP
server"). "Native" and "Managed" for the planes. Never "secure", "safe" or "protected" for a
native session. Never "active" for something only declared.

## 8. Core journeys

Each: the steps, what success looks like, and what happens when a step fails.

| #   | Journey                                        | Steps                                                                                                    | Success                                                                                                           | Failures                                                                                                                  |
| --- | ---------------------------------------------- | -------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| J1  | Ordinary terminal                              | Open the Console                                                                                         | A prompt, with no agent and no authority needed                                                                   | Shell cannot start: "Terminal failed: spawn_failed"; New terminal retries                                                 |
| J2  | Launch Claude Code with the real configuration | Start agent → Claude Code → project → "As configured" → Start                                            | A terminal running `claude`; the session listed as Native                                                         | Not installed; folder gone; terminal limit: each a fixed sentence, nothing started                                        |
| J3  | Disable a global hook                          | Configuration → Claude Code → Global → Hooks → entry → Disable → confirm (native dialog if it can block) | "Removed from settings.json and kept on this computer"; running sessions marked adopted                           | File changed in between: nothing written, prepare again. Entry may hold a secret: per-entry disable not offered (11 §5.4) |
| J4  | Verify outside Wyss                            | Open the file, or start `claude` in any terminal and look at its own hook list                           | The hook is gone there too                                                                                        | — (if it is not, that is a defect: read-back would already have reported it)                                              |
| J5  | Restore the hook                               | The same entry → Restore → native dialog                                                                 | The entry is back at its place, byte for byte                                                                     | The file changed: shown as a merge. Never seen on this computer: import offered                                           |
| J6  | Detect an external change                      | Edit the file in an editor                                                                               | Within the watch delay a banner says what changed                                                                 | Watch lost: "re-synchronised" after a rescan; never a silent stale state                                                  |
| J7  | Change profiles                                | Start agent with profile B on the same project                                                           | A second session; the first keeps profile A                                                                       | A required item not applicable: start refused with the list                                                               |
| J8  | Two native agents in parallel                  | J2 twice, two agents                                                                                     | Two rows, each with its process and resources                                                                     | Same folder: a warning, not a block                                                                                       |
| J9  | Secure managed Codex                           | Sessions → New managed session → folder → Create & activate → Open agent → objective                     | A turn runs under the authority `[R]`                                                                             | Authority down: notice and Retry `[R]`. Inference not provable because hooks are enabled: the cause is named              |
| J10 | Permissions and approvals                      | A Local Tools turn proposes a command → Approvals → Approve or Deny                                      | The command runs, or does not `[R]`                                                                               | Approval expired: the turn ends; nothing ran                                                                              |
| J11 | Inspect models                                 | Agents → model line; session detail                                                                      | The wording of 12 §7                                                                                              | Not reported: said so                                                                                                     |
| J12 | Recover from a configuration conflict          | A conflict copy appears                                                                                  | The source is marked conflicted, changes blocked, both files named                                                | The user resolves it outside; the Console never picks a side                                                              |
| J13 | Recover from a crash                           | Reopen the Console                                                                                       | Terminals are new; native sessions "lost"; managed sessions as the authority has them `[R]`; no file half written | —                                                                                                                         |
| J14 | Resume from another computer                   | (F9, F10) Select the home computer as host                                                               | Its sessions and terminals                                                                                        | Not reachable: the local computer's own view stays usable                                                                 |
| J15 | Connect from a phone                           | (F10b) Open the client, authenticate the device                                                          | Sessions, pending approvals, a terminal                                                                           | Device revoked: nothing is shown and nothing can be sent                                                                  |
