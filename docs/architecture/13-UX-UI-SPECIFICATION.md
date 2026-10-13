# 13 — UX and UI specification

The functional specification of every screen of the final product: what it shows, what it lets
the user do, every state it can be in, and the exact words it uses. It is the source from which
each screen's design contract is written; it is not that contract, and it is not a visual
design. Nothing here is to be built now (22, DN-16; the owner's instruction of 2026-10-13).
Screens and texts that exist are marked `[R]` with their file; everything else is `[P]`.

**Revision of 2026-10-13 (second pass).** Every screen was re-derived against the entry states
of [08 §2](08-RULES-HOOKS-SKILLS-MCP.md), the outcomes of [07 §5.4](07-CONFIGURATION-ENGINE.md),
the launch states of [09 §3.3](09-PROFILES-AND-ORCHESTRATION.md), the vault and key states of
[27 §5.2, §8.1](27-PROTECTED-RESTORATION-VAULT.md) and the strict mode of
[28 §5](28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md). The three provenance labels are placed on
every screen that shows configuration (§1.1). Every closed error word of the interfaces has one
fixed sentence (§9). Decisions that are the owner's preference are presented with alternatives
and a recommendation and left **open** (§12; 22, DN-32 to DN-38). Written by the author of the
set; **unreviewed**; no screen has a design contract approved by the owner.

What this document does not change: no security control, no gate and no contract of the domain
documents. Where a screen would need a capability that a domain document does not provide, the
screen shows it as unavailable and names the gate. Where this document and a domain document
disagree on behaviour, the domain document rules and this one is corrected; on wording, this
document rules.

## 1. Principles

| #   | Principle                                                                                                                                                                                    |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| U1  | The terminal keeps the space. Everything else can be hidden with one action                                                                                                                  |
| U2  | The plane of everything shown is visible (FR-UX-06): **Native** or **Managed**, as text, not colour alone                                                                                    |
| U3  | A state that is not verified does not look verified (FR-UX-03)                                                                                                                               |
| U4  | Nothing changes without a visible effect: what was written, where, and what did not change                                                                                                   |
| U5  | An error says what happened, what was left as it was, and what can be done (FR-UX-04)                                                                                                        |
| U6  | A control for something not built is absent, or disabled with "not available yet" (FR-UX-05); which of the two, per kind of capability, is DN-33                                             |
| U7  | The vocabulary is the user's: no internal service name, no raw code                                                                                                                          |
| U8  | Every action is reachable from the keyboard; no key is taken from the terminal's program beyond the documented few (C-12)                                                                    |
| U9  | Three sources of truth are never merged into one word: what the **file** says, what the **agent reported**, what was **seen in a session** (§1.1.1). A slot with no evidence says so         |
| U10 | No optimistic display of a mutation. The page shows a change only after the backend returns its outcome; a change that restores a control reads "written" until it is observed again (28 §5) |
| U11 | The page never authorises what a native dialog must authorise. While the dialog is open the page says so and offers nothing that could answer it (11 §4; 27 §6.1)                            |
| U12 | A capability that waits for a gate says which gate and what would open it, in one sentence, where the user would have looked for the action (08 §3.2; 27 §5.7)                               |
| U13 | A view does no work while it is hidden: no timer, no sampling, no rescan (§6; 15 §8)                                                                                                         |
| U14 | One closed word, one sentence. No screen composes its own error text (§9)                                                                                                                    |
| U15 | Text that comes from a file, a provider or an agent is shown as text, never interpreted as markup, and never as an instruction of the Console (CTL-27)                                       |

### 1.1 State vocabulary, used everywhere

The vocabulary is fixed here and used by every screen. Every word below has a non-colour
indicator (a shape or a glyph) **and** the word itself (NFR-A11Y-03). Colours are tokens of §11.

#### 1.1.1 Provenance: three slots, never merged

The configuration engine computes, for every element, a persisted state, a provider-effective
state and a state per session (07 §3). Wherever an element of configuration is shown, it is shown
with **three slots**, always in the same order, each with its own label. A slot is never left out
because it is empty; it says what is not known.

| Slot     | Label shown               | Values and their words                                                                                                                                                                                                                                                                                                                                                                        | Source (07 §3)       |
| -------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| File     | **File verified**         | The entry state of §1.1.2, with "read at 14:02". After the fallback period without confirmation (07 §7): "last seen at 14:02"                                                                                                                                                                                                                                                                 | `persisted`, `as_of` |
| Agent    | **Reported by the agent** | `verified_enabled` "Agent reports: on"; `verified_disabled` "Agent reports: off"; `documented` "Per documentation: on" (or off), in muted text with "not observed"; `not_verifiable` "The agent's view is not known"                                                                                                                                                                          | `effective`          |
| Sessions | **Seen in sessions**      | `using` "In use in 2 sessions (seen)"; `documented_using` "2 sessions are documented to pick this up (not observed)"; `stale` "2 sessions still run the previous version"; `not_using` "Not used by 1 session (profile Review)"; `unknown` "Use in sessions not known". Sessions started outside the Console are a count only: "and 1 session started outside Wyss Console, not visible here" | `sessions`; 05 §6    |

Rules:

1. "Reported by the agent" and "seen in sessions" appear with a positive value only when the
   agent supplied it by a supported means. In F1 the Agent slot of Claude Code is at best
   `documented` (07 §3; DN-10) and the Sessions slot at best `documented_using` (OQ-28).
2. A `documented` value is drawn in muted text with the word "documentation"; it never uses the
   check mark of a verified value (U3).
3. A successful write changes the File slot only. The Agent and Sessions slots change only when
   their own evidence changes (07 E6; 25, IR-10).
4. Screen-reader text reads the three slots in order, whole: "Hook security-preflight, pre tool
   use. File: off, kept on this computer, read at 14:02. Agent: per documentation, off, not
   observed. Sessions: 2 documented to pick this up, not observed."

#### 1.1.2 Entry states (SM-ENTRY, 08 §2)

| State             | Word shown                                               | Indicator                                      | Sentence in the detail panel                                                                                                           | Actions offered (when their gate is open)              |
| ----------------- | -------------------------------------------------------- | ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| `enabled`         | **On**                                                   | Filled circle                                  | "On in <file>."                                                                                                                        | Disable; Keep a recovery copy (F2h); Edit, Delete (F5) |
| `disabled`        | **Off**                                                  | Hollow circle                                  | "Off, by <agent>'s own switch in <file>. Any computer that shares this file sees it off."                                              | Enable                                                 |
| `kept_disabled`   | **Off, kept in the file**                                | Hollow circle with a bracket                   | "Off. Wyss Console keeps it inside <file> in a form <agent> skips. Any computer that shares this file can restore it."                 | Restore; Delete                                        |
| `vault_disabled`  | **Off, kept on this computer**                           | Hollow circle with a lock glyph                | "Off. Removed from <file> and kept, encrypted, on this computer only. Your other computer sees it as removed and cannot restore it."   | Restore; Show; Delete (S4b)                            |
| `removed_outside` | **Removed outside**                                      | Dashed circle                                  | "Removed from <file> outside Wyss Console on <date>. This computer holds no copy." Or, with a recovery copy: "A copy can be recovered" | Recover (only with a recovery copy, F2h)               |
| `trust_pending`   | **Pending your review in <agent>**                       | Hollow circle with a clock glyph               | "<agent> will skip this hook until you review it in <agent>. Wyss Console never marks a hook as trusted."                              | None by the Console (FR-HRS-12)                        |
| `unreadable`      | **Can't be read**                                        | Ring with a slash                              | The sentence of the source's state (§1.1.4)                                                                                            | None                                                   |
| `absent`          | Not listed                                               | —                                              | —                                                                                                                                      | —                                                      |
| `unavailable`     | Not a state of the entry: an action is not offered (U12) | The action's control disabled, with a sentence | One sentence of §9.2 naming the gate                                                                                                   | —                                                      |

A hook that the user marked as a security control, or that is of an event that can block an
action, carries the tag **Control** (11 §4). A rule file never carries it (CTL-14): its tag is
**Guidance for the model**.

#### 1.1.3 What happened to a change (SM-PLAN, SM-WRITE; 07 §5.4, §5.6)

| Outcome or state           | Word shown in the result                            | Indicator                                    | Severity |
| -------------------------- | --------------------------------------------------- | -------------------------------------------- | -------- |
| `drafted`                  | Prepared                                            | Outline                                      | Neutral  |
| `authorising`              | Waiting for your decision                           | Outline, pulsing only without reduced motion | Neutral  |
| `applying`                 | Writing                                             | Progress bar, indeterminate                  | Neutral  |
| `applied`, weakening       | **Done**                                            | Check                                        | Success  |
| `applied`, strengthening   | **Written; checking that it stays**                 | Check with a clock                           | Pending  |
| Strengthening, re-observed | **In the file**                                     | Check                                        | Success  |
| `not_applied`              | **Not applied**                                     | Ring                                         | Notice   |
| `lost_race`                | **Not applied: the agent wrote at the same moment** | Ring                                         | Notice   |
| `superseded_outside`       | **Replaced from outside**                           | Triangle                                     | Warning  |
| `reverted_outside`         | **No longer in place**                              | Triangle                                     | Warning  |
| `conflict_preserved`       | **Two versions exist**                              | Triangle                                     | Warning  |
| `recovery_incident`        | **Needs your attention**                            | Octagon                                      | Critical |
| `unverified`               | **Written, but not as planned**                     | Triangle                                     | Warning  |
| `failed`                   | **Failed**                                          | Octagon                                      | Error    |
| `expired`, `discarded`     | Expired; Cancelled                                  | Outline                                      | Neutral  |

A strengthening change is one that restores or enables a hook that can block, a permission rule
that denies or asks, or anything marked as a control (28 §5). The rules of re-observation are
those of 28 §5: at the end of settling, at the next observation of the file and at the next focus
of the window. Only then does "Written; checking that it stays" become "In the file".

#### 1.1.4 Source states (SM-SOURCE, 07 §7.1)

| State             | Word                      | Sentence                                                                                          |
| ----------------- | ------------------------- | ------------------------------------------------------------------------------------------------- |
| `unknown`         | Reading                   | "Reading…"                                                                                        |
| `current`         | (no word; the time)       | "Read at 14:02"                                                                                   |
| `changed_outside` | **Changed outside**       | "Changed outside Wyss Console at 14:02: 1 hook removed." with "Show"                              |
| `stale`           | **Not watched**           | "Changes to this file are checked every N seconds, not as they happen." Every state shows its age |
| `missing`         | **Not present**           | "This file does not exist. Nothing is configured here at this scope."                             |
| `unreadable`      | **Can't be read**         | §9, `source_unreadable`                                                                           |
| `malformed`       | **Can't be parsed**       | §9, `source_malformed`                                                                            |
| `untrusted`       | **Read only**             | §9, `source_untrusted_path`                                                                       |
| `conflicted`      | **Conflict copy present** | §9, `conflict_copy_present`                                                                       |

#### 1.1.5 Sessions and terminals

| Kind                       | States and words                                                                                                                                                                                                                                  |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Terminal (SM-TERMINAL)     | Starting · Running · Exited (code N) · Failed (§9.1) · Closing. `[R]` for "Starting terminal…" (`TerminalPanel.svelte:33`)                                                                                                                        |
| Native session (SM-NATIVE) | Starting · Running · Exited (code N) · **Lost** ("Wyss Console restarted and could not see what became of it") · Did not start · (forgotten: not shown)                                                                                           |
| Launch (SM-LAUNCH)         | Checking the profile · Waiting for your decision · Preparing · Starting · Started; refusals by §9.4                                                                                                                                               |
| Managed session `[R]`      | As built (ADR 0020 to 0023): its own words are kept unchanged                                                                                                                                                                                     |
| Markers on any session     | **Native** or **Managed** (always, U2) · **Stale** with a clock glyph · **Profile: N controls left out** (09 §3.2) · **Started outside Wyss Console** · **Environment may differ from your shell's** (05 §8) · **Not verified with this version** |

#### 1.1.6 Availability of an action

| Kind                                    | Shown as                                                                                                                                                        |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Available                               | An enabled control                                                                                                                                              |
| Waiting for a gate of the final product | Per DN-33: recommended, a disabled control with one sentence naming the gate and what would open it (§9.2)                                                      |
| Not part of this provider at all        | Absent. The detail says, once, what the provider has instead (for example "Claude Code has no switch for one hook")                                             |
| Forbidden (11 §4)                       | Absent, with the reason in the source's detail: system policy, a provider-owned file, a generated file, a trust record                                          |
| Temporarily refused                     | Enabled control; the refusal arrives as the outcome of planning, with its sentence (for example strict mode: "Close your Claude Code sessions to change this.") |

## 2. Information architecture and navigation

### 2.1 The frame

```text
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ ◆ Wyss Console  <view title>  [host ▾]   Terminal ● Codex ● Authority ●  ⚑ 2   v…   │  top bar
├──────────────┬───────────────────────────────────────────────────────────────────────┤
│ Console      │                                                                       │
│ Sessions     │                                                                       │
│ Configuration│                         active view                                   │
│ Profiles     │                                                                       │
│ Agents       │                                                                       │
│ Approvals  2 │                                                                       │
│ History      │                                                                       │
│ Settings     │                                                                       │
└──────────────┴───────────────────────────────────────────────────────────────────────┘
```

| Element        | Today `[R]`                                                                          | Final product                                                                                                                                                                                              |
| -------------- | ------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Navigation     | Console, Sessions, Agents, Approvals, Events, Settings (`src/lib/shell/nav.ts`)      | §2.2. Configuration and Profiles added; Events becomes History with three tabs; Devices when remote exists. The order is DN-32                                                                             |
| Top bar        | Three status pills that never disappear, down to 320 px (Terminal, Codex, Authority) | The same pills; an **attention** indicator (§2.4); a host selector when remote exists (S15). No pill for the vault: its state lives in Configuration, and only its incidents reach the attention indicator |
| Sidebar        | 216 px; an icon rail at 720 px and below                                             | Collapsible by the user at any width (U1); collapsed state is a preference                                                                                                                                 |
| Default window | 1440×810                                                                             | Unchanged (22 §1)                                                                                                                                                                                          |
| Minimum window | 320×320                                                                              | Unchanged; every view operable there (§4)                                                                                                                                                                  |

### 2.2 Navigation items

| Item              | Purpose                                                                   | Plane                                       | Screens     | First phase with content                    | Before that phase                                    |
| ----------------- | ------------------------------------------------------------------------- | ------------------------------------------- | ----------- | ------------------------------------------- | ---------------------------------------------------- |
| **Console**       | Terminals, panes, agent sessions in terminals                             | Native (and none)                           | S1, S3      | Built `[R]`                                 | —                                                    |
| **Sessions**      | Every session of both planes, and its detail                              | Both, each marked                           | S2          | Built for managed `[R]`; F3b for native     | Managed only, as today                               |
| **Configuration** | The real configuration of each agent; kept entries                        | Native files; the Codex strip names Managed | S4, S4b, S5 | F1                                          | Absent, or disabled with "not available yet" (DN-33) |
| **Profiles**      | Session profiles; later, global profiles in a tab of their own            | Native                                      | S6          | F3c                                         | The same                                             |
| **Agents**        | Managed turns with Codex                                                  | Managed                                     | S8, S7      | Built `[R]`                                 | —                                                    |
| **Approvals**     | Managed actions waiting for a decision                                    | Managed                                     | S9          | Built `[R]`                                 | —                                                    |
| **History**       | Three separate records: managed execution, configuration, native sessions | Each tab marked                             | S10         | Built as Events `[R]`; F2a and F3b add tabs | The Managed tab only                                 |
| **Settings**      | Preferences that decide nothing; about                                    | —                                           | S11         | Built `[R]`                                 | —                                                    |
| **Devices**       | Paired devices, their grants, revocation, pairing                         | Remote                                      | S16         | F10a                                        | Absent                                               |

Not navigation items, by design: **Models** (a line in session details, a category of
Configuration and a field of the launcher, S7); **Kept entries** (a sub-view of Configuration,
S4b); **Search** (the palette, S12); **Handoff** (an action on a session, S17).

The order above is the recommendation of DN-32 (by frequency of use in the owner's workflow,
the terminal first). OQ-25 asks the same question and is answered by DN-32 when the owner
decides.

### 2.3 Hierarchy, routes and return

| Level         | Example                             | Rule                                                                                                                                                         |
| ------------- | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| View          | Configuration                       | Chosen from the navigation or the palette. Each view remembers its last sub-view and selection for the run of the window, not across restarts (no new store) |
| Sub-view      | Claude Code · Global · Hooks        | Tabs and selectors inside the view; they are part of the view's state                                                                                        |
| Detail        | One hook                            | A panel beside or below the list (§4); never a separate window                                                                                               |
| Sheet         | Start agent (S3), Plan (S5)         | Over the view, modal to it, dismissed by Escape; the view behind stays readable                                                                              |
| Native dialog | Consent, risky confirmation, reveal | Owned by the backend (§10.2). The page shows a waiting state and cannot close it                                                                             |

A link inside the product (for example "Show in History" from a result) moves to the target
view and selects the target; Back (§5) returns to where the link was. There are no external
links that open a browser: the window has no opener permission `[R]`.

### 2.4 Attention

One indicator in the top bar, ⚑ with a count, lists what needs an act of the user, newest first,
each with a link to where it is resolved. It is not a notification centre and keeps no history
of its own: every item is a live state owned elsewhere and disappears when that state ends.

| Item                                                       | Owner                | Resolved in                                |
| ---------------------------------------------------------- | -------------------- | ------------------------------------------ |
| A recovery incident (07 §5.4)                              | Configuration engine | S14, S4b                                   |
| A source blocked by `repair_pending`                       | Configuration engine | S14                                        |
| A conflicting version held in the vault                    | Vault                | S4b                                        |
| A conflict copy of a provider file                         | Configuration engine | S4                                         |
| A change of the user's that is no longer in place (L2, L5) | Configuration engine | S4, S10                                    |
| The vault key missing (`key_missing`)                      | Vault                | S4b                                        |
| A native session lost after a restart                      | Native registry      | S2                                         |
| A pending approval                                         | The authority `[R]`  | S9 (its count on Approvals stays as built) |

Live-region announcements follow §5.3. Whether these also reach the desktop's own notifications
is DN-35.

### 2.5 The plane on every surface

| Surface                                                     | How the plane is shown                                                                                                                                   |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A terminal tab or pane running an agent the Console started | A **Native** badge in the tab and the pane header                                                                                                        |
| A plain terminal                                            | No badge: a shell is not an agent session                                                                                                                |
| A session row or detail                                     | **Native** or **Managed**, text first                                                                                                                    |
| Configuration                                               | A fixed strip: "Native sessions use this configuration. Managed sessions do not." For Codex a second strip with managed Inference's compatibility (S4.6) |
| History                                                     | One tab per record; the tab names its plane                                                                                                              |
| Approvals                                                   | "In Local Tools, every command and file change Codex proposes stops here." `[R]` It never lists a native session                                         |

Words never used in the native plane: approved, contained, mediated, sandboxed, secure, safe,
protected (CTL-21; §7).

## 3. Screens

Each screen uses the same aspects; an aspect that does not apply is left out. "Contracts" names
the interfaces of documents 05 to 12 and 27 that the screen calls; a screen calls nothing else.
"Phase" is the first phase of [19](19-IMPLEMENTATION-ROADMAP.md) that delivers it.

### S1 — Console (terminal workspace) `[R]`, extended

| Aspect        | Specification                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Purpose       | Use terminals. Nothing else competes for this view                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Phase         | Built `[R]`; panes and search T1; layout restore T2; agent start F3a                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Layout        | A tab strip, "New terminal", and, from F3a, "Start agent" beside it. The active tab fills the rest. From T1 a tab holds a tree of panes (05 TF1); each pane has a one-line header only when the tab has more than one pane or the pane runs an agent session: title, working directory, and for an agent the badge **Native**, the agent, the profile and markers of §1.1.5. A thin focus border marks the pane that receives keys                                                   |
| Data          | Per tab: title (from the program's title sequence, bounded and sanitised; pinnable, 05 TF5), state. Per pane: the same, and for an agent session its registry record (05 §5)                                                                                                                                                                                                                                                                                                         |
| Actions       | New terminal; close; switch tab. From T1: split right, split down, close pane, focus the next pane in a direction, resize a pane (drag the divider, or by keyboard in steps of one cell), zoom one pane to the whole tab and back, equalise. Search in the focused pane's scrollback (S1.1). From F3a: Start agent (S3); "Open a shell here" on an exited agent pane; "Start again" on an exited or lost agent pane, which opens S3 prefilled and starts nothing by itself (05 §4.3) |
| Confirmations | Closing a terminal or a pane whose program is still running: one confirmation in the page naming the program when it is known. Closing a tab with several panes: one confirmation that names how many are running                                                                                                                                                                                                                                                                    |
| Loading       | "Starting terminal…" in the status line `[R]` (`TerminalPanel.svelte:33`)                                                                                                                                                                                                                                                                                                                                                                                                            |
| Empty         | "No terminals open. Create a new terminal to continue." `[R]` (`TerminalView.svelte:132`). From F3a, with "Start an agent" beside it                                                                                                                                                                                                                                                                                                                                                 |
| Unavailable   | Never: the terminal depends on neither the authority nor the configuration engine (NFR-REL-03; TS-REG-01)                                                                                                                                                                                                                                                                                                                                                                            |
| Error         | The tab or pane keeps its last screen so it can be read, and its status line shows the sentence of §9.1. **Today it shows the raw word** ("Terminal failed: spawn_failed", `TerminalPanel.svelte:41`) `[R]`, which U7 and FR-UX-04 do not allow: the sentences of §9.1 replace it in the first phase that touches this view                                                                                                                                                          |
| Limit         | At 16 terminals (leaves, from T1) "New terminal", split and Start agent are disabled with "Wyss Console runs at most 16 terminals. Close one to open another." (C-06)                                                                                                                                                                                                                                                                                                                |
| Offline       | Unaffected                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Keyboard      | Everything typed goes to the focused pane's program. Console actions only under the leader chord, which is OQ-14 and is designed in 05 (TF3); this document fixes only that the chord exists, that it is shown in every tooltip and in the palette, and that a documented chord moves focus out of the terminal (§5.2). Tabs: arrows, Home, End in the tab strip `[R]`                                                                                                               |
| Accessibility | Tabs are a tab list with panels `[R]`; the status line is a live region for the active tab only `[R]`. A pane header is a labelled group; the focus border has a non-colour form (thicker line). Screen-reader support of the terminal's content is OQ-44                                                                                                                                                                                                                            |
| Responsive    | The terminal refits on every size change `[R]`. Below the minimum pane size (proposed 20 columns × 5 rows; fixed in 05) a split is refused with "Not enough room to split here." and an existing split narrower than that shows only the focused pane, with the others reachable by keyboard and by a pane switcher in the header                                                                                                                                                    |
| Restore (T2)  | At start, tabs, splits, titles and working directories come back with new shells; scrollback does not (05 TF4). What a pane that ran an agent comes back as is DN-36                                                                                                                                                                                                                                                                                                                 |
| Contracts     | The terminal commands (05 §1) `[R]`; IF-NATIVE (05 §7) from F3a                                                                                                                                                                                                                                                                                                                                                                                                                      |
| Performance   | NFR-PERF-05 to 08, 10. A hidden pane draws nothing; search is loaded on first use (05 TF2)                                                                                                                                                                                                                                                                                                                                                                                           |

#### S1.1 Search in scrollback (T1)

A bar at the top of the focused pane: a text field, previous, next, "case sensitive", "whole
word", a count "3 of 12", close. Opening it moves focus into the field; Escape closes it and
returns focus to the pane. It searches the pane's own scrollback only, in the page; nothing is
sent anywhere and nothing is kept. With no match: "No match in this terminal's scrollback." The
regular-expression option is not specified until 05 designs search (TF2).

### S2 — Sessions (both planes)

| Aspect        | Specification                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purpose       | See every session, in either plane, and reach it                                                                                                                                                                                                                                                                                                                                                                |
| Phase         | Managed list `[R]`; native rows F3b                                                                                                                                                                                                                                                                                                                                                                             |
| Layout        | A list grouped by project, or by agent (a toggle; the default is DN-32). Each row: name, **plane badge**, agent, profile (with "N controls left out" when 09 §3.2 applies), state (§1.1.5), started, resource sample (native, while visible). A separate group, collapsed by default: "Started outside Wyss Console" (FR-NAT-10). A detail panel on the right. Today the list holds managed sessions only `[R]` |
| Data          | Native: IF-NATIVE `sessions` (05 §7). Managed: the authority's session page `[R]`. Outside: processes whose executable matches a catalogue entry (05 §6), labelled best effort                                                                                                                                                                                                                                  |
| Actions       | Native: go to its terminal; start another on the same project (S3 prefilled); hand over (S17, F11a); forget (when ended). Managed: open agent, events, resume, enable Local Tools or use Inference, close `[R]`. Outside: none but "show the process" (pid and command name only)                                                                                                                               |
| Confirmations | Managed close and mode change: as built `[R]`. Forget: none (nothing runs)                                                                                                                                                                                                                                                                                                                                      |
| Loading       | Rows appear per owner as each answers; neither waits for the other                                                                                                                                                                                                                                                                                                                                              |
| Empty         | "No sessions yet." with "Start an agent" (F3a) and "New managed session" `[R]`                                                                                                                                                                                                                                                                                                                                  |
| Unavailable   | Authority down: the managed group shows "The authority is unavailable. Native sessions and terminals keep working." (`[R]` for the wording about terminals). Native registry unreadable: the native group shows §9, `registry_unreadable`, and the managed group is unaffected                                                                                                                                  |
| Warnings      | Two sessions on one folder: "Two sessions can change files in this folder. Wyss Console does not coordinate them." (CTL-24). A native and a managed session on one folder: "The managed session changes files only through approvals; the native one changes them freely." (09 §6)                                                                                                                              |
| Recovery      | After a restart, sessions that were running are **Lost** with "Wyss Console restarted and could not see what became of it." and "Start again on this project" (05 §5). Never "exited"                                                                                                                                                                                                                           |
| Error         | Per group, never the whole view                                                                                                                                                                                                                                                                                                                                                                                 |
| Keyboard      | Arrows move in the list; Enter opens the detail; a second Enter performs the row's first action; a visible focus ring                                                                                                                                                                                                                                                                                           |
| Accessibility | The badge is text ("Native", "Managed"); every marker is text                                                                                                                                                                                                                                                                                                                                                   |
| Responsive    | Detail panel below the list under 600 px of content `[R]` for today's layout                                                                                                                                                                                                                                                                                                                                    |
| Contracts     | IF-NATIVE `sessions`, `session`, `forget`; the session commands of ADR 0020 to 0023 `[R]`                                                                                                                                                                                                                                                                                                                       |
| Performance   | Resource samples are taken only while this view is visible (05 §6; TS-NAT-08); at most one sample per session per two seconds, proposed, fixed in 15                                                                                                                                                                                                                                                            |

#### S2.1 Native session detail

| Part          | Content                                                                                                                                                                                                                                 |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Header        | **Native**; agent and version, with "not verified with this version" when it applies; project folder as selectable text; state                                                                                                          |
| What it runs  | "Your global configuration" or "Profile <name>, version <n>", with the three lists the launch plan showed (09 §4.2) and, when controls were left out, "This session is not equivalent to your global configuration." for its whole life |
| Configuration | Per element the Sessions slot of §1.1.1 for **this** session, with a link to the element in S4                                                                                                                                          |
| Model         | The model line of S7                                                                                                                                                                                                                    |
| Process       | Started, ended, exit code; processor and memory sample (while visible); "Environment may differ from your shell's" when the start used the fallback (05 §8)                                                                             |
| Honesty line  | Always: "This session runs as you, with the agent's own permissions. Wyss Console does not approve or contain what it does."                                                                                                            |
| Actions       | Go to terminal · Start another here · Hand over (F11a) · Forget (when ended)                                                                                                                                                            |

### S3 — Start agent (launcher)

| Aspect        | Specification                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Purpose       | Start an installed agent in a terminal, on a project, with a profile and, where the provider allows, a model                                                                                                                                                                                                                                                                                                                                                 |
| Phase         | F3a (agent, project); F3c (profiles); F6b (model)                                                                                                                                                                                                                                                                                                                                                                                                            |
| Layout        | A sheet over the Console, in four parts, top to bottom: **Agent** (the catalogue, each with version and "verified" or "not verified with this version"); **Project** (known projects, most recent first, or "Choose folder…", which opens the native folder dialog); **Profile** (default "As configured"); **Model** (only for an agent whose start flag has evidence, 06 §2; empty means the profile's or the agent's own). Below them the **launch plan** |
| Launch plan   | Three lists (09 §4.2): Applied · Applied with a cost · Not applicable. Then "Still in force" (system or managed policy, only with evidence; otherwise "not known whether it stays in force"). Then, if anything is left out, "Left out: …" with each control named. Choosing a model writes no file: it makes a temporary profile for this start (09 §1)                                                                                                     |
| Actions       | Start; Cancel                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| States        | The SM-LAUNCH states of §1.1.5, shown in the sheet's footer. While a native dialog is open: "Waiting for your decision in the system dialog." and Start is disabled                                                                                                                                                                                                                                                                                          |
| Confirmations | None for "As configured". A plan classed `weakens_security` (a control left out) takes the native dialog whenever the left-out set differs from the one last confirmed for that profile, provider and project on this computer, or the provider's version changed (09 §3.2; DN-20). A plan classed `executes_code` (the profile adds a hook, an MCP server or a plugin) takes the native dialog                                                              |
| Empty         | No agent installed: "No supported agent was found on this computer." followed by the agents of the catalogue Wyss Console looked for, by name, each with what it is (05 §4.1)                                                                                                                                                                                                                                                                                |
| Unavailable   | A profile that would copy entries into a generated file, or that needs leaving out the user source before OQ-02 is answered, is listed but not selectable, with its sentence (§9.4)                                                                                                                                                                                                                                                                          |
| Error         | The sentences of §9.3 and §9.4. A refusal starts nothing and leaves nothing behind (09 §3.3)                                                                                                                                                                                                                                                                                                                                                                 |
| Text shown    | Always: "This session runs as you, with the agent's own permissions. Wyss Console does not approve or contain what it does."                                                                                                                                                                                                                                                                                                                                 |
| Keyboard      | Full. Enter starts when Start has focus or the sheet's last field is filled; Escape closes; arrows choose in each list                                                                                                                                                                                                                                                                                                                                       |
| Contracts     | IF-NATIVE `catalogue`, `start`; IF-PROFILE `resolve`; SM-LAUNCH                                                                                                                                                                                                                                                                                                                                                                                              |

### S4 — Configuration (provider browsers)

The central screen. It shows the real configuration of each installed agent and, from F2a and
F2v, lets the user change it. It never shows content the backend did not project and redact
(FR-CFG-16; CTL-07).

| Aspect         | Specification                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purpose        | See and change the real configuration of each agent                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Phase          | F1 (read only); F2a, F2v, F2h, F2b (switches, the vault, recovery copies, Codex); F4a, F4b (OpenCode, Hermes); F5 (authoring)                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Layout         | **Provider tabs**: the installed agents, then agents not installed whose files exist ("not installed"). **Scope selector**: System, Global, Project (with a project picker of known projects). **Category list**: Rules, Hooks, Skills, MCP servers, Plugins, Settings, Model, and at the end **Kept entries** (S4b). **Entry list** with the three slots of §1.1.1 in compact form. **Detail panel**. Fixed strips at the top: the plane strip (§2.5); for Codex, the Inference strip (S4.6); while a known session of the provider runs, the strict-mode strip (S4.5) |
| Data           | IF-CONFIG `inventory` and `changes` (07 §8): sources with state, items with the three states, capabilities, revision                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Actions        | F1: none that changes anything. Each source shows its path as selectable text so that the user can open it in an editor; the Console opens nothing itself (no opener permission `[R]`). From F2a: the actions of S4.4, each through a plan (S5). From F5: create, edit, delete                                                                                                                                                                                                                                                                                          |
| Confirmations  | Per risk class (11 §4): one confirmation in the page for `normal`; a native dialog for `weakens_security` and `executes_code`, and for every plan that seals a vault record whatever its class (27 §6.1)                                                                                                                                                                                                                                                                                                                                                                |
| Loading        | Sources appear one by one; a slow source does not hold the others. A source being read shows "Reading…"                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Empty          | Per category and scope: "No hooks are configured at this scope." (and the like)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Unavailable    | Provider not installed: "Codex is not installed. Its configuration files, if any, are listed read only." Provider version without evidence: "Claude Code <version> is not verified with this version of Wyss Console. Everything is shown; changes that need evidence for this version are not offered." (06 §6.1, by tier)                                                                                                                                                                                                                                             |
| Error          | Per source: §1.1.4 and §9.2. Per operation: §9.2. A failing source never empties the others (NFR-REL-04)                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Outside change | S4.5                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Conflict       | A conflict copy left by a replication tool: the source is **Conflict copy present**, both files are named, and every action on that source is disabled with "A conflict copy of this file exists (from your sync tool). Changes are blocked until it is resolved." Resolving it is done outside; the Console never picks a side (FR-SYNC-02; J12)                                                                                                                                                                                                                       |
| Permission     | A source in a folder another account can write is read only with "This folder can be changed by another account on this computer, so Wyss Console will not write to it." (CTL-10). On both of the owner's computers this applies to `~/.claude` today (30 §6.4, B1); the owner treats the second account as an active requirement and the rule is not weakened                                                                                                                                                                                                          |
| Offline        | Works without the authority and without a network                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Keyboard       | Provider tabs, scope, categories, list and detail are in tab order; arrows inside each; actions have shortcuts under the leader chord, shown in tooltips                                                                                                                                                                                                                                                                                                                                                                                                                |
| Accessibility  | Each entry's state is one sentence read whole (§1.1.1, rule 4). The strips are landmarks with names                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Responsive     | Under 720 px the category list becomes a selector; the detail panel moves below the list; the three slots stack (§4)                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Contracts      | IF-CONFIG (07 §8); IF-VAULT (27 §6) for S4b                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Performance    | The first inventory of a provider within a proposed 200 ms on the fixture set (15 §4, M19); NFR-PERF-09. The list is paged by the backend at 256 items (07 §8) and windowed in the page                                                                                                                                                                                                                                                                                                                                                                                 |

#### S4.1 Sources

Every category starts with its sources: file name, scope, class (06 §4.1: user, project, system,
provider-owned, generated, link), state (§1.1.4), "read at", and whether it can be edited.
Read-only reasons, one sentence each:

| Reason                | Sentence                                                                                                    |
| --------------------- | ----------------------------------------------------------------------------------------------------------- |
| System policy         | "Set by your system or organisation. Read only." (C-09)                                                     |
| Provider-owned file   | "<agent> rewrites this file itself. Wyss Console reads part of it and never writes it." (07 §4.4)           |
| Generated file        | "Produced by another tool<, by <marker>>. Read only." (07 §4.3)                                             |
| Link                  | "A link to <target>. Changes, where allowed, go to the target." (07 §4.3)                                   |
| Round trip not exact  | "Wyss Console cannot rewrite this file without changing its layout, so it will not edit it." (07 E4; OQ-29) |
| Untrusted folder      | As the Permission row of S4                                                                                 |
| Protected by the user | "You marked this file as protected. Read only." (06 §4.1)                                                   |

#### S4.2 The entry list

Each row: name (the provider's identifier, or event and matcher for a hook), tags (**Control**,
**Guidance for the model**, **Brings hooks** for a plugin or skill that carries hooks, 08 §5),
the File slot's word and indicator, then the Agent and Sessions slots in compact form ("per
documentation", "2 documented"). A row whose entry changed outside since it was last shown has
a dot and "changed" until the user opens it. Sorting: by file order (default), name, state.
Filter: by state word, and "only controls".

#### S4.3 Detail, per kind

| Kind       | Shows                                                                                                                                                                                                                                                                        | Never shows                                               |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| Hook       | Provider, scope, source, event, matcher, handler type, the command or target as redacted by 06 §4.2, timeout, the three slots, tags; for Codex the trust state "Trust: pending your review in Codex" where it can be read; validation notes reported, not enforced (08 §3.1) | A value of a secret class; a header; an environment value |
| Rule file  | Path, scope, size, link or generated, what it imports (`@path` lines), whether it is loaded per documentation; the tag **Guidance for the model**                                                                                                                            | Its text: the user opens it in an editor (F1 to F5)       |
| Skill      | Directory, name and description from its header, state, hooks it brings                                                                                                                                                                                                      | Files of the skill                                        |
| MCP server | Name, transport, non-secret fields, the **names** of its variables and headers                                                                                                                                                                                               | Every value of a variable, header or token field (OQ-37)  |
| Plugin     | Name, state, what it brings (hooks, skills, servers)                                                                                                                                                                                                                         | —                                                         |
| Setting    | Key, value at each scope, the documented winner marked "takes precedence (documented)" (07 §2.2), "constrained by system policy" when it is                                                                                                                                  | Values of a secret class                                  |
| Model      | S7                                                                                                                                                                                                                                                                           | —                                                         |

Every detail ends with **What changing this does**: the reload rule (07 §6) in one sentence, for
example "Claude Code documents that running sessions pick this up; not verified for your
sessions." or "Applies to new sessions. 2 running sessions keep the previous one."

#### S4.4 Actions per provider, kind and operation

Final product against what opens each action. "Gate" names what must be decided, observed or
built; until then the action is unavailable and says so (§1.1.6; DN-33 decides whether it is
shown disabled or absent). No action is offered on a provider version whose evidence tier does
not allow it (06 §6.3).

| Provider    | Kind                      | Operation                                                                     | Final behaviour                                                                                                                | Confirmation                      | Gate today                                                                                 |
| ----------- | ------------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ | --------------------------------- | ------------------------------------------------------------------------------------------ |
| Claude Code | Hook                      | Disable one                                                                   | Removed and kept in the vault (rung 3); kept in the file where verified (rung 2)                                               | Native dialog (seals a record)    | ADR 0029; DN-21; DN-25 to DN-28; DN-31; B1; strict mode; F2v. Rung 2: RS1 (not authorised) |
| Claude Code | Hook                      | Restore                                                                       | From the vault, exact or at the one unambiguous place (27 §3.7)                                                                | Native dialog (`executes_code`)   | As above                                                                                   |
| Claude Code | Hook                      | All hooks off and on                                                          | `disableAllHooks`, a direct write                                                                                              | Native dialog                     | DN-28 gate; DN-31; B1; F2a                                                                 |
| Claude Code | Hook                      | Create, edit, delete                                                          | Command typed in a native input; undo by `undo_entry` if adopted                                                               | Native dialog                     | DN-12; DN-22; F5                                                                           |
| Claude Code | Rule file                 | Disable                                                                       | Moved where the provider does not read it                                                                                      | Page                              | OQ-23; importers check; crash cases in 14 §4                                               |
| Claude Code | Rule file                 | Edit                                                                          | Whole-document edit with a diff                                                                                                | Page                              | F5                                                                                         |
| Claude Code | Skill                     | Off, on                                                                       | The override map                                                                                                               | Page; native when it brings hooks | OQ-15; the direct-write gates; F2a                                                         |
| Claude Code | Plugin                    | Off, on                                                                       | `enabledPlugins`                                                                                                               | Page; on with hooks: native       | The direct-write gates; F2a                                                                |
| Claude Code | MCP server                | Off, on                                                                       | Not designed: no documented switch `[N]`; the provider's writer with the definition held in the vault while off (29 §2, row 7) | Native for on                     | DN-13; DN-12; a design that does not exist                                                 |
| Claude Code | MCP server                | Add, remove                                                                   | Through `claude mcp`                                                                                                           | Native dialog                     | DN-13; F5                                                                                  |
| Claude Code | Model                     | Change the default                                                            | The `model` setting, a direct write                                                                                            | Page                              | The direct-write gates; F2a                                                                |
| Codex       | Hook                      | Off, on                                                                       | The native switch                                                                                                              | Native                            | DN-19 (recommended: neither before G-INF); OQ-01; G-INF                                    |
| Codex       | Skill, MCP server, plugin | Off, on                                                                       | `enabled` on the entry                                                                                                         | Page; on is native                | F2b; DN-21; the direct-write gates                                                         |
| Codex       | MCP server                | Add, remove                                                                   | Through `codex mcp`                                                                                                            | Native dialog                     | DN-13; F5                                                                                  |
| Codex       | Rule file                 | Any                                                                           | The owner's global `AGENTS.md` is generated: read only                                                                         | —                                 | —                                                                                          |
| OpenCode    | Any                       | Inventory, then switches                                                      | Per 06 §3.3                                                                                                                    | Per 11 §4                         | F4a; OQ-04; OQ-12; DN-04 (JSONC)                                                           |
| OpenCode    | Plugin                    | Off                                                                           | No switch `[D]`; only a verified kept form                                                                                     | —                                 | Not designed                                                                               |
| Hermes      | Any                       | Inventory; changes through `hermes config set`                                | Per 17 §2                                                                                                                      | Per 11 §4                         | F4b; DN-13; OQ-11                                                                          |
| Any         | Any                       | Write to a system source, provider-owned file, generated file or trust record | Never offered                                                                                                                  | —                                 | `forbidden` (11 §4)                                                                        |

Before the vault and the direct-write gates open, the detail of an enabled Claude Code hook shows,
in place of Disable: "Not available yet: Claude Code has no switch for one hook, and Wyss
Console's protected storage is not set up." with the path of the file as selectable text. The
all-hooks switch shows its own gate: "Not available yet: Wyss Console does not write agents'
files directly until keeping a second version safely has been validated." (DN-28; 07 §5.4). No
closed word exists for a gate that is closed by design rather than refused at planning time; this
sentence is the interface's, and the domain documents are to name it when the gate is specified.

#### S4.5 Outside changes, strict mode, and the other computer

| Situation                                                        | Shown                                                                                                                                                                                                                                                                                                   |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A source changed outside                                         | A banner on the source: "Changed outside Wyss Console at 14:02: 1 hook removed, 1 changed." with "Show". A removed hook is named by event and position only; its command is not kept (08 §3.6). The plans on it are void                                                                                |
| The watch is degraded                                            | A strip: "Changes are checked every N seconds, not as they happen." Every state shows its age (07 §7.2)                                                                                                                                                                                                 |
| Re-synchronised after an overflow                                | A transient line: "Re-synchronised." Never a silent stale state (J6)                                                                                                                                                                                                                                    |
| A known session of the provider is active (strict mode)          | A strip: "2 Claude Code sessions are running. Changes to this file wait until they are closed." with "Show sessions". The actions stay enabled; planning one returns `provider_session_active` (§9.2). Strict mode is not a proof: the strip never says that closing the sessions makes the change safe |
| A change of the user's was replaced (L2, L5; `reverted_outside`) | First thing shown for that source, and at every start until acknowledged: "Your change to settings.json is no longer in place: <entry> was put back by a version written <by the agent / from outside>." with "Apply again" and "Show in History". Never applied again by itself (28 §5)                |
| The file may be used elsewhere                                   | After a conflict copy or an outside change the Console did not cause: "This file may be used on another computer. Its sessions are not visible here." (16 §2.2)                                                                                                                                         |
| An entry disabled on the other computer into its vault           | **Removed outside**, with "Removed outside this computer. If you disabled it in Wyss Console on another computer, restore it there." (27 §9.2)                                                                                                                                                          |

#### S4.6 Codex and managed Inference

A strip on every Codex screen: "Managed Inference: available" or "Managed Inference: unavailable
while any Codex hook is enabled (2 enabled)." (FR-CFG-14; CTL-16). A plan whose managed effect
would make Inference unavailable is refused with `blocked_managed_inference` (§9.2). Before G-INF
each Codex hook shows its configured state where it can be read and "Enabling a Codex hook would
make managed Inference unavailable. It will be offered when managed Inference no longer depends
on your hooks." (08 §3.3; DN-19).

### S4b — Kept entries and the protected vault

A sub-view of Configuration, last in its category list, for every provider. Specified to the
vault's contract (27); the vault itself is a proposal (ADR 0029, Proposed) and is **not created
until DN-31 is decided** (27 §9.1).

| Aspect       | Specification                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Purpose      | See everything Wyss Console holds for the user; restore, show, delete; understand the vault's state and what it does and does not protect                                                                                                                                                                                                                                                                                                                          |
| Phase        | F2v; recovery copies F2h; export and import after F2v (DN-29)                                                                                                                                                                                                                                                                                                                                                                                                      |
| Strip        | The vault's state (§S4b.1) in one sentence; where its key is kept ("in your system keyring (GNOME Keyring)" or "derived from a passphrase"); per class and state: count, bytes, oldest age; how many records are no longer required and can be cleaned up; the bounds (14 §3) and the free space of the filesystem (27 §8.3)                                                                                                                                       |
| List         | One row per record: provider, scope, source, kind, event, class (**Disabled, kept here** · **Recovery copy** · **Second version of a file** · **Undo copy**), state (§S4b.2), date and age. Orphaned and unlisted records are in groups of their own, never hidden                                                                                                                                                                                                 |
| Data         | Metadata only, from the vault's index (27 §3.4), without the key. Whether a held entry is back in its file needs the key: while it is not available, "not known" (27 §3.4)                                                                                                                                                                                                                                                                                         |
| Actions      | Restore (native dialog, always) · Show (the content in a native dialog, with Copy) · Delete (native dialog that says when it is the only copy) · Clean up records no longer required (native dialog listing them) · Rotate key (native dialog) · Recover the key with a code · Change where the key is kept (a new consent). From an entry's detail in S4 (F2h): Keep a recovery copy · Update the copy · Stop keeping it · Recover. Later: Export, Import (DN-29) |
| Availability | By the vault's state, exactly as 27 §5.7: the table §S4b.3                                                                                                                                                                                                                                                                                                                                                                                                         |
| Consent      | The first time an operation needs the vault: the consent dialog (S4c, ND-02). The page shows "Waiting for your decision in the system dialog."                                                                                                                                                                                                                                                                                                                     |
| Reminder     | A record older than the user's threshold is marked "kept for 94 days"; nothing is deleted by age (DN-27). The default threshold is DN-37                                                                                                                                                                                                                                                                                                                           |
| Empty        | "Nothing is being kept." and, while the vault was never set up, "Wyss Console keeps a disabled hook here only when its agent has no switch for it. It asks you first."                                                                                                                                                                                                                                                                                             |
| Contracts    | IF-VAULT (27 §6)                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Performance  | The list is read from the index without the key; nothing of the vault runs at start-up (27 §5.2)                                                                                                                                                                                                                                                                                                                                                                   |

#### S4b.1 Vault states (SM-VKEY, 27 §5.2)

| State          | Sentence in the strip                                                                                                                                                                               | Offered                                                         |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| `absent`       | "Not set up. Wyss Console will ask before keeping anything."                                                                                                                                        | Nothing until an operation needs it                             |
| `ready`        | "Ready. Kept entries are encrypted; the key is in your system keyring. Programs running as you can reach them while the keyring is unlocked."                                                       | Everything of §S4b.3                                            |
| `locked`       | "Your keyring is locked. Kept entries are listed; they can be restored when it is unlocked. Nothing is lost."                                                                                       | Listing; deletion by metadata                                   |
| `no_custody`   | "Your system keyring is not available in this session. Kept entries are listed; nothing else that needs them can be done."                                                                          | Listing; deletion; changing where the key is kept (new consent) |
| `key_missing`  | "The key for the kept entries is missing from your keyring. They cannot be read." With a recovery code: "Enter your recovery code to get them back." Without: "They are lost. You can delete them." | Recovery by code, if one exists; deletion                       |
| `rotating`     | "Changing the key: 12 of 40 entries done. Everything stays readable."                                                                                                                               | Reading                                                         |
| `newer_format` | "These entries were kept by a newer version of Wyss Console. Update it to use them. Nothing was changed."                                                                                           | Nothing                                                         |
| `unsafe`       | "Wyss Console's private folder is in a place where it will not keep entries: <reason>. Nothing is kept or changed."                                                                                 | Nothing                                                         |

`key_missing` is shown as a loss, never as a passing state. A keyring of another provider behind
the same interface is `key_missing`, never a quiet switch (27 §5.1).

#### S4b.2 Record states (SM-VAULT, 27 §8.1; §3.6)

| State or class               | Word                  | Sentence                                                                                                   |
| ---------------------------- | --------------------- | ---------------------------------------------------------------------------------------------------------- |
| `holding`                    | **Kept**              | "Kept. Can be restored here. This is the only copy."                                                       |
| `restoring`                  | **Restoring**         | "Being put back."                                                                                          |
| `restored`                   | **Restored**          | "Back in the file since <date>. The kept copy can be removed when you choose."                             |
| `redundant`                  | **Active again**      | "This hook is active again; the kept copy can be removed."                                                 |
| `corrupt`                    | **Damaged**           | "Damaged. Cannot be restored. The hook it held is lost unless you have a backup of the vault and its key." |
| `unsupported_format`         | **Not readable here** | "Kept by a newer version of Wyss Console, or in a form this version no longer reads. It can be shown."     |
| `unlisted`                   | **Unexpected**        | "Wyss Console did not expect this record. It will not use it unless you show and restore it yourself."     |
| `orphaned`, `source_missing` | **File gone**         | "The file this belonged to is gone. Nothing is removed by itself." With Show, Delete                       |
| `orphaned`, `source_moved`   | **Project moved**     | "The project this belonged to has moved or was removed." With "Point to the project" (native dialog)       |
| Recovery copy, entry present | **Copy kept**         | "A recovery copy is kept. The hook is still in the file."                                                  |
| Recovery copy, entry gone    | **Can be recovered**  | "The hook you kept a copy of is no longer in the file as it was. A copy can be recovered."                 |
| Recovery copy, entry changed | **Copy is older**     | "The hook changed since you kept the copy. The copy may hold an old secret." With "Update the copy"        |
| `conflict_version`           | **Second version**    | "A second version of settings.json, kept when the agent wrote at the same moment. Open both to choose."    |
| Imported (later)             | **Imported**          | "Imported from another computer. Restoring it shows its full content first." (27 §9.3)                     |

Never shown as one: "Disabled, kept here" and "A recovery copy is kept" (27 §3.6).

#### S4b.3 What works in each state (27 §5.7)

| Capability                                        | Never set up  | Locked or no keyring                                | Key missing               | Repair pending for that source        |
| ------------------------------------------------- | ------------- | --------------------------------------------------- | ------------------------- | ------------------------------------- |
| Terminal, native sessions, managed plane          | Yes           | Yes                                                 | Yes                       | Yes                                   |
| Reading, inventory, outside changes, history      | Yes           | Yes                                                 | Yes                       | Yes                                   |
| The list of kept entries                          | Empty         | Yes                                                 | Yes                       | Yes                                   |
| Whether a kept entry is back in its file          | —             | "Not known"                                         | "Not known"               | Yes, when unlocked                    |
| A change through the agent's own verified command | Yes           | Yes                                                 | Yes                       | Except on that source                 |
| A direct write to an agent's file                 | Not offered   | `conflict_preservation_unavailable`                 | Not offered               | Not for that source                   |
| Disabling into the vault; keeping a recovery copy | After consent | Refused before you confirm                          | Not offered               | Not for that source                   |
| Restore, recover, show                            | —             | "Unlock your keyring to continue. Nothing is lost." | Recovery of the key first | Not for that source                   |
| Delete a record                                   | —             | Yes, by metadata, native dialog                     | Yes: the only action left | Not while part of an unfinished write |

### S4c — Native dialogs

The backend opens these; the page never draws them and cannot answer them (U11; 11 §4; 27 §6.1).
Every dialog states what the backend computed, not what the page sent. The safe button
(Cancel, Keep, Do not restore) has the initial focus; the acting button names the act
("Disable and keep", "Restore", "Delete for good"); there is no "do not ask again" (11 §4). A
dialog not answered within five minutes is treated as refused (SM-PLAN); if it cannot be shown,
nothing is done (`dialog_unavailable`). What a native dialog can display (length, monospace text,
scrolling, screen-reader access) is OQ-43.

| ID    | Dialog                                | Opens when                                                                       | Content                                                                                                                                                                                                      | Acting button                 |
| ----- | ------------------------------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------- |
| ND-01 | Confirm a risky change                | A plan classed `weakens_security` or `executes_code`, or one that seals a record | Provider, scope, file, the entry, the effect on running sessions and on managed execution (07 §6; S4.6), the risk in one sentence, and "it will be kept, encrypted, on this computer" when it seals          | The act ("Disable", "Enable") |
| ND-02 | Consent to the vault                  | The first operation of a class that needs the vault; and each case of 27 §7      | The nine statements of 27 §7, in that order, with the custody named and "Programs running as you can still reach it while it is unlocked." The recovery-code offer (ND-09) follows it when DN-26 allows      | "Allow keeping entries"       |
| ND-03 | Restore a kept entry                  | Restore in S4b or S4                                                             | The entry's content (redacted only where 06 §4.2 requires it on screen; imported content always whole), where it goes back, "byte-identical" or "the file changed: not byte-identical" with a diff (27 §3.7) | "Restore"                     |
| ND-04 | Show a kept entry                     | Show                                                                             | The content, with Copy, and "Copied text can stay in your clipboard history." (27 §5.3)                                                                                                                      | — (Close)                     |
| ND-05 | Delete kept entries                   | Delete, Clean up, delete all                                                     | Each entry by metadata, "This is the only copy" where it is, "Copies in backups or snapshots of your disk are not reached by this." With "Restore all first" and "Show" beside the act (14 §8)               | "Delete for good"             |
| ND-06 | Keep a recovery copy                  | Keep a recovery copy, for up to twenty entries                                   | The exact entries the backend resolved, "A second copy of each, encrypted, on this computer, until you stop keeping it." (27 §3.6)                                                                           | "Keep copies"                 |
| ND-07 | Stop keeping a recovery copy          | Stop keeping it                                                                  | The entry; "the only copy" when the entry is gone from the file                                                                                                                                              | "Stop keeping"                |
| ND-08 | Rotate the key                        | Rotate key                                                                       | What rotation does and does not do (27 §5.4)                                                                                                                                                                 | "Change the key"              |
| ND-09 | Recovery code                         | Vault creation; after each rotation                                              | The code once, in groups, and what keeping and losing it mean (27 §5.5). Declinable                                                                                                                          | "I have written it down"      |
| ND-10 | Enter the recovery code               | Recover                                                                          | A native input; nothing typed reaches the page                                                                                                                                                               | "Recover"                     |
| ND-11 | A recovery incident                   | At once and at every start                                                       | The sentence of S14, never worded as success; with: try to protect it again; show it; save it where I choose; delete it, each a further confirmation                                                         | Per choice                    |
| ND-12 | A launch that leaves out a control    | SM-LAUNCH `confirming`                                                           | Each control left out, by name; "This session will not be equivalent to your global configuration."; what is still in force, or "not known" (09 §3.2)                                                        | "Start without them"          |
| ND-13 | Pair a device (F10a)                  | A device presents its code                                                       | The device's name and key fingerprint, the grants asked (default `observe` only) (16 §5.2)                                                                                                                   | "Pair"                        |
| ND-14 | Author a command (F5)                 | Create or edit a hook                                                            | The native input for the command (DN-12); the exact file, scope and resulting entry                                                                                                                          | "Save to <file>"              |
| ND-15 | Export or import kept entries (later) | DN-29                                                                            | 27 §9.3: passphrase input, the list by metadata, the warning when the folder is replicated                                                                                                                   | "Export", "Import"            |

### S5 — Plan and confirm (part of S4)

| Aspect   | Specification                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purpose  | Show exactly what a change will do before it happens                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Layout   | A sheet. **What**: the entry. **Where**: provider, scope, file. **How it stays restorable**: "Its agent's own switch" · "Kept inside the file" · "Removed and kept, encrypted, on this computer (only this one)" · "Cannot be undone" (07 §5.2). **Change**: a structured summary ("1 hook removed from PreToolUse, matcher Bash") and the redacted diff, collapsed by default. **Effect**: per running session, with the Sessions slot words ("2 sessions are documented to pick this up, not observed; 1 keeps the previous state until restarted"), managed effect, and "agents running outside Wyss Console: 1, not visible here". **Risk** sentence when the class is not `normal`. **Expires in** 60 s, shown as a countdown only in its last 10 s |
| Actions  | Confirm; Cancel                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Then     | For a risky plan, or one that seals a record, ND-01 opens with the same facts; the page shows "Waiting for your confirmation in the system dialog." Then "Writing…"                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| Refusals | Before anything is touched, with their sentences (§9.2): strict mode, the vault not ready, consent missing, the filesystem or private folder unsuitable, a repair pending, the journal unavailable, too many plans, an expired plan                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| Result   | §S5.1                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Limits   | At most eight prepared plans at once (`too_many_plans`); a plan is single use; it expires after 60 s or when its source changes (SM-PLAN)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

#### S5.1 The result

The sheet becomes the result, with the word of §1.1.3, the sentence of §9.2, and what was left
as it was. Then the entry's slots update. "Undo" is offered in the result and stays available in
History while the file has not diverged (07 §5.7). For a strengthening change the result reads
"Written to settings.json. Checking that it stays…" and changes to "In the file." only after the
re-observations of 28 §5; if one of them fails, the result becomes `reverted_outside` and enters
the attention list (§2.4).

### S6 — Profiles

| Aspect        | Specification                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purpose       | Create and maintain session profiles; see global profiles (F5) in a separate tab whose name says that it changes shared files                                                                                                                                                                                                                                                                           |
| Phase         | F3c; global profiles F5                                                                                                                                                                                                                                                                                                                                                                                 |
| Layout        | List of profiles (name, agents it applies to, current version, "project default for …"); an editor of the fields of 09 §4.1, each a selector of things the backend lists (no free path, command, argument or value field: 09 §9); beside it the **resolved result per agent**, each value with the profile it came from (09 §4.3) and the three lists of 09 §4.2 for a chosen project; the version list |
| Actions       | New from a template; duplicate; save (a new version); restore a version; export (native file chooser); import (native file chooser, then the page shows what the profile asks for before it is stored); set as a project's default; delete                                                                                                                                                              |
| Confirmations | Import shows what the profile asks for. Saving never weakens a session: the risk is evaluated at each start (09 §9)                                                                                                                                                                                                                                                                                     |
| Empty         | The templates, each with "only as strong as <agent>'s start flags make it" (09 §4.6)                                                                                                                                                                                                                                                                                                                    |
| Error         | §9.4                                                                                                                                                                                                                                                                                                                                                                                                    |
| Global tab    | F5: a named set of switches on the real global configuration, applied as one reviewed plan (FR-PRF-09). Its design is not written (23 §3); until it is, the tab is absent                                                                                                                                                                                                                               |
| Contracts     | IF-PROFILE (09 §9)                                                                                                                                                                                                                                                                                                                                                                                      |

### S7 — Models

No navigation item. Four places, one vocabulary (12 §7), and the rule that a configured or chosen
model is never shown as the model that answered (AD-16).

| Place                                 | Shows                                                                                                                                                     | Phase                   |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| Configuration · Model category (S4.3) | The configured model and effort at each scope, the documented winner marked; "Applies to new sessions. N running sessions keep the previous one." (07 §6) | F1 (read), F2a (change) |
| Launcher (S3)                         | A model field, for agents whose start flag has evidence; "listed, not guaranteed for your account" for a catalogue entry                                  | F6b                     |
| Native session detail (S2.1)          | "Configured: X. In use: not reported" unless the agent reports it by a supported means (12 §4)                                                            | F3b                     |
| Agents view, managed (S8)             | The catalogue with the default marked and "Offered by this runtime; not a guarantee for your account"; the observed model per turn once DN-09 is decided  | F6a                     |

What appears per provider, by evidence (12 §3). A field whose evidence is `[N]` is not shown;
nothing is inferred.

| Field                       | Claude Code                                           | Codex (native) | Codex (managed)                                  | OpenCode       | Hermes         |
| --------------------------- | ----------------------------------------------------- | -------------- | ------------------------------------------------ | -------------- | -------------- |
| Configured model            | Shown `[D]`                                           | Shown `[D]`    | "Not applicable: the managed runtime takes none" | Shown `[D]`    | Shown `[D]`    |
| Effort                      | Shown `[D]`                                           | Shown `[D]`    | From the catalogue `[R]`                         | Hidden         | Hidden         |
| Choose at start             | Field `[O]`                                           | Field `[O]`    | Not offered (SG-12)                              | Hidden         | Hidden         |
| Catalogue                   | "This provider has no list the Console can read"      | The same       | Shown `[R]`                                      | The same       | The same       |
| Model of a turn             | "Not reported"                                        | "Not reported" | After DN-09                                      | "Not reported" | "Not reported" |
| Switch in a running session | DN-38                                                 | DN-38          | Not designed                                     | DN-38          | DN-38          |
| Usage and cost              | Only figures the provider reports; none today (12 §6) | The same       | The same                                         | The same       | The same       |

### S8 — Agents (managed turns) `[R]`

As built (ADR 0022): tabs per open session, the turn list, the composer, "Cancel turn". Additions:
the plane badge **Managed**; the model line of S7; "Hand over" (S17, F11a). Its words are kept.

### S9 — Approvals `[R]`

As built (ADR 0023): pending actions, each with command or change, directory, deadline, Approve
and Deny. Unchanged. Its header keeps saying what it covers: "In Local Tools, every command and
file change Codex proposes stops here." It never lists anything from a native session. A remote
approval (F10a) is the same `respond` intent and shows on the host as decided "from <device>".

### S10 — History

| Tab               | Content                                                                                                                                                                                            | Source               | Phase |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- | ----- |
| Managed execution | The Events view as built `[R]`                                                                                                                                                                     | The authority        | Built |
| Configuration     | Changes with their outcome word (§1.1.3), undos, refused plans, refused dialogs, outside changes, conflicts, vault events (created, restored, deleted, rotated, consent; 27 §11), repairs at start | The engine's journal | F2a   |
| Native sessions   | Starts and ends with exit codes, lost sessions                                                                                                                                                     | The registry         | F3b   |

Three lists, never merged into one timeline (AD-11); a shared time filter lets the user read them
side by side. Rows hold identifiers, words and times; a configuration row names an entry by
provider, scope, kind and event, never by content (CTL-08). From a change row: "Undo" (when
undoable), "Show the entry" (S4). The Configuration tab carries "Delete history…", which removes
the closed entries of both journals and the last-seen index, keeps open intents and touches
nothing the vault depends on (14 §8); its confirmation is in the page and says so.

### S11 — Settings `[R]`

As built (ADR 0026), two columns in a wide window. It stays the place for preferences that decide
nothing. Configuration of agents is not in it. Preferences this specification adds, each only
with its phase: collapsed sidebar (§2.1); the reminder threshold for kept entries (DN-37); the
theme and contrast mode (DN-34); notifications outside the window (DN-35).

### S12 — Search and command palette

One palette under the leader chord: go to a view, a session, a project, an entry of the
configuration, a profile, a kept entry; run a Console action that has no further input. It
searches names the Console already holds, never file contents, never terminal output. Every
result says what it is and its plane. Every action of every view is listed with its chord, so the
palette is also the key map's reference (FR-UX-02). Terminal scrollback search is a separate
control inside the terminal (S1.1).

### S13 — Error and conflict states

Every such state is one of a closed list, each with a fixed sentence and the action offered:
§9. The list is the union of the error words of 05 §4.2, 07 §8, 09 §9, 27 §6 and the terminal's
closed list `[R]`; no screen invents a message (U14). A word that two interfaces share has one
sentence per interface (§9, rule 2).

### S14 — Recovery flows

| Situation                                                 | What the user is shown                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The window crashed with native sessions open              | Sessions marked **Lost**, with what is known; "Start again on this project"                                                                                                                                                                                                                                                                                                                                                                              |
| A change was interrupted and repaired                     | At start: "A change to settings.json was in progress. The file is intact (verified)." With the outcome found (14 §4): "It was applied." / "It did not take place. The file is as it was." / "The agent wrote at the same moment; its version was kept."                                                                                                                                                                                                  |
| A change was interrupted and the keyring is locked        | "A change to settings.json was in progress. Unlock your keyring to finish checking it. Nothing was removed. You can go on reading; changes to this file wait." (`repair_pending`)                                                                                                                                                                                                                                                                        |
| A file in the private folder that no change explains      | "Wyss Console found a file in its private folder that it cannot explain. It has not been used or removed." With Show and Delete, each a native dialog (14 §4)                                                                                                                                                                                                                                                                                            |
| A recovery incident                                       | ND-11, at once and at every start, never worded as success: "Something went wrong while protecting a second version of settings.json. It is kept, unprotected, in Wyss Console's private folder: it may contain passwords or tokens, and it may be the only copy of that version. Nothing was removed and nothing will be removed by itself. Changes to this file are paused." With: try to protect it again; show it; save it where I choose; delete it |
| The authority is not running                              | The notice and Retry, as built `[R]`                                                                                                                                                                                                                                                                                                                                                                                                                     |
| A disabled entry was edited outside                       | The restore dialog (ND-03) shows the current content, which is what would run                                                                                                                                                                                                                                                                                                                                                                            |
| A hook was being disabled when the Console stopped        | At start, one of: "The hook is still in settings.json; nothing was kept." or "The hook was removed and is kept; it can be restored."                                                                                                                                                                                                                                                                                                                     |
| A restoration was interrupted                             | "The hook is still kept. Restore it again." Nothing is lost by an interrupted restoration                                                                                                                                                                                                                                                                                                                                                                |
| The key for the kept entries is missing                   | "Wyss Console cannot read the hooks it was keeping: the key in your keyring is gone." With the recovery code, if one was made; otherwise "They are lost."                                                                                                                                                                                                                                                                                                |
| A version of a file was kept after a conflict             | Shown at every start until resolved: "Two versions of settings.json exist. Open both."                                                                                                                                                                                                                                                                                                                                                                   |
| A change of the user's was replaced while closed (L2, L5) | At start, before anything else about that source: the `reverted_outside` sentence (§9.2) with "Apply again"                                                                                                                                                                                                                                                                                                                                              |
| A store of a newer version                                | "This data was written by a newer version of Wyss Console. It is read only until you update." (14 §3)                                                                                                                                                                                                                                                                                                                                                    |
| A migration failed                                        | "Wyss Console could not update its data to this version. The previous data was set aside and nothing was deleted." (14 §5)                                                                                                                                                                                                                                                                                                                               |

### S15 — Host selector (future, F10a)

A control in the top bar naming the computer whose terminals and sessions are shown, with its
connection state: **This computer** · **<host>: connected** · **<host>: reconnecting** · **<host>:
not reachable**. Absent until remote exists. Switching host changes only what is shown (FR-SES-03);
a host that is not reachable leaves this computer's own views usable (J14). Every remote view
carries the host's name in its title. Its design waits for DN-05.

### S16 — Devices and remote access (future, F10a, F10b)

**Blocked by DN-05**; not designed beyond what 16 §5 fixes. What this specification fixes now, so
that no later design weakens it:

| Element                  | Fixed now                                                                                                                                                                       |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Where it lives           | A navigation item **Devices**, on the host only, absent until F10a                                                                                                              |
| Pairing                  | Started at the host's keyboard; a code or QR shown in the window; confirmed in ND-13 with name and fingerprint (16 §5.2)                                                        |
| The list                 | Always visible on the host: each device, its grants, last seen, expiry; "Revoke" acts at once (FR-REM-08)                                                                       |
| Grants                   | `observe` by default; `terminal_write` labelled "Full control of your account from that device"; `approve` and `configure` say they need a fresh unlock on the device each time |
| What a device never does | Restore, show, export or delete kept entries (CTL-38; 27 §9.4); answer a native dialog of the host                                                                              |
| On the device            | Every screen names the host; a dropped connection changes nothing and says "reconnecting" (FR-REM-06); a revoked device shows nothing and can send nothing (J15)                |

The phone client's screens (F10b) are a future deliverable (§13).

### S17 — Hand over (future, F11a)

A sheet from a session's detail. Parts as 09 §7.1: objective (written by the user); project and
revision (read); decisions taken (selected by the user from the sending session's visible
output, quoted); files that matter (chosen from the project); open items; the receiving agent and
profile (S3's fields). A footer: "The receiving session starts new. It does not continue the
conversation, and it gets no more authority from this text." Then Start. **Partial**: the
package's format and states are not designed (26 §2); this sheet is fixed as far as 09 §7.1 goes.

### S18 — First run and onboarding

On the first start with nothing configured in the Console: one page that says which agents were
found, at which versions, what Wyss Console can do with each on this version and what it cannot,
and "Nothing of yours has been changed." (FR-DIST-02). It offers the terminal first. It asks for no
account and needs no agent. Its full design belongs to F13 (18 §3).

### S19 — About and diagnostics

In Settings: version, the provider versions with evidence, and, from F13, "Create a diagnostic
report", which shows the report before it is saved: versions, closed words and counters, never
content, paths outside known locations or values (FR-OBS-05; CTL-36).

## 4. Responsive behaviour

The breakpoints that exist `[R]` (`src/app.css`): 1040, 940, 800, 720, 560, 520 and 420 px for
the top bar and sidebar; container queries for each view. New views follow the same rule: the
view's own container decides its layout, not the window. Minimum window 320×320 `[R]`: every
view must remain operable there.

| Pattern                           | Wide                         | Narrow (content below 720 px)           | Smallest (content below 420 px)                 |
| --------------------------------- | ---------------------------- | --------------------------------------- | ----------------------------------------------- |
| List and detail (S2, S4, S4b, S6) | Side by side                 | Detail below the list                   | One at a time, with Back                        |
| Category list (S4)                | A column                     | A selector                              | A selector                                      |
| Three slots (§1.1.1)              | One line, three columns      | Three short lines                       | Three lines; the compact words of S4.2          |
| Sheets (S3, S5, S17)              | Centred, at most 720 px wide | Full width                              | Full window, scrolling                          |
| Panes (S1)                        | As split                     | As split, down to the minimum pane size | Only the focused pane; a switcher in its header |
| Strips (S4)                       | One line each                | Wrapped                                 | Collapsed to one line with "More"               |
| Top bar                           | Pills with words             | Pills with words `[R]`                  | Pills `[R]`; attention count kept               |

Native dialogs are not laid out by the page; their content must fit what OQ-43 finds.

## 5. Accessibility

| Requirement | Rule                                                                                                                                                                                                        |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| NFR-A11Y-01 | Tab order follows reading order; no keyboard trap; the terminal has a documented way out of focus (§5.2)                                                                                                    |
| NFR-A11Y-02 | Text contrast at least 4.5:1 against its background, computed per pair of tokens; 3:1 for indicators and focus outlines; the terminal already sets a minimum contrast ratio of 4.5 `[R]` (`renderer.ts:45`) |
| NFR-A11Y-03 | Every state has a word and a shape (§1.1)                                                                                                                                                                   |
| NFR-A11Y-04 | Reduced motion respected `[R]` (`src/app.css`); nothing pulses or animates under it                                                                                                                         |
| Text size   | Every view stays usable at 200 % of the base text size, with no text cut off; the terminal's font size is its own preference `[R]`                                                                          |
| Targets     | Pointer targets at least 24×24 px                                                                                                                                                                           |

### 5.1 Names and roles

Every control has an accessible name equal to its visible words. Lists are lists with a count
("12 hooks"); the three slots are one sentence (§1.1.1, rule 4); strips and banners are named
landmarks or regions; sheets are dialogs with a title and trap focus while open; the attention
indicator is a button whose name includes its count.

### 5.2 Keyboard outside the terminal

| Where                 | Keys                                                                                                                              |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Anywhere              | The leader chord, then a letter, for every Console action (05 TF3; OQ-14). The palette lists them                                 |
| Leaving the terminal  | One documented chord moves focus from the terminal to the sidebar; another returns it. Both are part of the leader set (TS-UX-03) |
| Lists                 | Arrows; Home and End; Enter opens; type-ahead on the first letters                                                                |
| Sheets                | Tab through fields; Enter on the primary action; Escape cancels and returns focus to what opened the sheet                        |
| After a native dialog | Focus returns to the control that asked for it, and the result is announced (§5.3)                                                |
| Back                  | Returns to where an internal link was followed (§2.3)                                                                             |

### 5.3 What is announced

Live regions only for what the user must hear, and politely unless stated.

| Event                                                | Announced                   |
| ---------------------------------------------------- | --------------------------- |
| A pending approval                                   | Yes, as built `[R]`         |
| An outside change of a source the user is looking at | Yes                         |
| The result of the user's own action                  | Yes                         |
| A recovery incident                                  | Yes, assertively; and ND-11 |
| A change of the user's no longer in place            | Yes                         |
| Resource samples, timers, re-synchronisation         | No                          |

## 6. Performance requirements of the interface

No view may add work to the page while it is hidden (U13). Lists are paged (50 rows) or windowed.
The Configuration view renders from a projection the backend already bounded (256 items a page).
Targets are in [15 §4](15-PERFORMANCE-ENGINEERING.md).

| Rule                                                                                                      | Why                         |
| --------------------------------------------------------------------------------------------------------- | --------------------------- |
| Nothing of the configuration engine, the vault or the registry runs before the first terminal prompt      | NFR-PERF-01; 27 §5.2; 14 §9 |
| A view's data is fetched when it is first shown, not at start                                             | M01, M02                    |
| Resource sampling only while S2 is visible (TS-NAT-08)                                                    | NFR-PERF-03                 |
| The fallback rescan only while S4 is visible (07 §7)                                                      | NFR-PERF-09                 |
| The strict-mode strip reads the process list when S4 is shown and on focus, never on a timer while hidden | NFR-PERF-03; 28 §5.1        |
| Change records are applied to the shown list in place; the list is not rebuilt                            | M11                         |
| The palette searches an index of names built on first use and updated from change records                 | M01                         |
| No animation runs while nothing changes                                                                   | M03                         |

Each phase with a screen repeats M03, M05 and M16 against the baseline (15 §8).

## 7. Copy rules

Sentences, not codes. The provider's own words for its own things ("hook", "skill", "MCP
server"). "Native" and "Managed" for the planes. Never "secure", "safe" or "protected" for a
native session. Never "active" or "in force" for something only declared. Never "synced",
"backed up" or "safe" for the vault: it is "kept, encrypted, on this computer".

| Rule                                                                                                     | Example                                                                                                          |
| -------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Say what happened, what stayed as it was, what can be done (U5)                                          | "The file changed. Nothing was written. Prepare the change again."                                               |
| Name the file by its name; give the path as selectable text in the detail                                | "settings.json"; `~/.claude/settings.json`                                                                       |
| Name the agent; never "the provider" in the window                                                       | "Claude Code"                                                                                                    |
| Times in the user's locale; ages in days beyond a day                                                    | "14:02"; "kept for 94 days"                                                                                      |
| Counts in words the list uses                                                                            | "2 sessions", "1 hook removed"                                                                                   |
| A gate in one sentence: what is missing, then what would make it available                               | "Not available yet: Claude Code has no switch for one hook, and Wyss Console's protected storage is not set up." |
| "Not verified", "per documentation", "not reported": never softened                                      | —                                                                                                                |
| Words never used for a native session: approved, contained, mediated, sandboxed, secure, safe, protected | —                                                                                                                |
| A rule file is "guidance for the model", never a control                                                 | CTL-14                                                                                                           |

## 8. Core journeys

Each: the steps, what success looks like, and what happens when a step fails. J1 to J15 are those
of [01 §6](01-PRODUCT-VISION.md); J16 to J25 are added by this pass.

| #   | Journey                                                     | Steps                                                                                                                                 | Success                                                                                                                                                                          | Failures                                                                                                                                                                                                                                                                                                                                     |
| --- | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| J1  | Ordinary terminal                                           | Open the Console                                                                                                                      | A prompt, with no agent and no authority needed                                                                                                                                  | Shell cannot start: the sentence of `spawn_failed` (§9.1); New terminal retries                                                                                                                                                                                                                                                              |
| J2  | Launch Claude Code with the real configuration              | Start agent → Claude Code → project → "As configured" → Start                                                                         | A terminal running `claude`; the session listed as **Native**                                                                                                                    | Not installed; folder gone; terminal limit: each its sentence, nothing started                                                                                                                                                                                                                                                               |
| J3  | Disable a global hook                                       | Configuration → Claude Code → Global → Hooks → entry → Disable → plan (S5) → (the first time, ND-02) → ND-01                          | "Disabled. Removed from settings.json and kept, encrypted, on this computer." File slot **Off, kept on this computer**; Sessions slot "documented to pick this up, not observed" | Not offered, with its gate (S4.4). Strict mode: "Close your Claude Code sessions to change this." Vault locked or not set up, consent refused: nothing written. The file changed, or the agent wrote at the same moment: a named outcome and "Prepare again" (07 §5.4); the kept record is purged if the hook is still in the file (27 §8.2) |
| J4  | Verify outside Wyss                                         | Open the file, or start `claude` in any terminal and look at its own hook list                                                        | The hook is not in the file and no longer runs there                                                                                                                             | If it still runs, a session is using settings it loaded earlier: the Console never said otherwise. If the hook is back in the file, the agent wrote an older version back, and the Console reports it (28 §4, case 5)                                                                                                                        |
| J5  | Restore the hook                                            | Kept entries, or the same entry → Restore → ND-03                                                                                     | The entry is back at its place, byte for byte when the file has not changed since; "Written; checking that it stays", then "In the file"                                         | The file changed: placed only at its one unambiguous place, shown as a diff to confirm; otherwise not at all, with its reason. Keyring locked: "unlock it and try again"; nothing is lost. Removed outside, or kept on another computer: not restorable here. Record damaged or key lost: said plainly, the hook is lost                     |
| J6  | Detect an external change                                   | Edit the file in an editor                                                                                                            | Within the watch delay a banner says what changed                                                                                                                                | Watch lost: "Re-synchronised." after a rescan; never a silent stale state                                                                                                                                                                                                                                                                    |
| J7  | Change profiles                                             | Start agent with profile B on the same project                                                                                        | A second session; the first keeps profile A                                                                                                                                      | A required item not applicable: start refused with the list                                                                                                                                                                                                                                                                                  |
| J8  | Two native agents in parallel                               | J2 twice, two agents                                                                                                                  | Two rows, each with its process and resources                                                                                                                                    | Same folder: a warning, not a block                                                                                                                                                                                                                                                                                                          |
| J9  | Secure managed Codex                                        | Sessions → New managed session → folder → Create & activate → Open agent → objective                                                  | A turn runs under the authority `[R]`                                                                                                                                            | Authority down: notice and Retry `[R]`. Inference not provable because hooks are enabled: the cause is named                                                                                                                                                                                                                                 |
| J10 | Permissions and approvals                                   | A Local Tools turn proposes a command → Approvals → Approve or Deny                                                                   | The command runs, or does not `[R]`                                                                                                                                              | Approval expired: the turn ends; nothing ran                                                                                                                                                                                                                                                                                                 |
| J11 | Inspect models                                              | Agents → model line; session detail; Configuration → Model                                                                            | The wording of 12 §7, per S7                                                                                                                                                     | Not reported: said so                                                                                                                                                                                                                                                                                                                        |
| J12 | Recover from a configuration conflict                       | A conflict copy appears                                                                                                               | The source is marked, changes blocked, both files named                                                                                                                          | The user resolves it outside; the Console never picks a side                                                                                                                                                                                                                                                                                 |
| J13 | Recover from a crash                                        | Reopen the Console                                                                                                                    | Terminals are new (from T2, in their layout); native sessions **Lost**; managed sessions as the authority has them `[R]`; no file half written; repairs reported (S14)           | A repair that needs the keyring: `repair_pending` until unlocked                                                                                                                                                                                                                                                                             |
| J14 | Resume from another computer                                | (F9, F10a) Select the home computer as host                                                                                           | Its sessions and terminals                                                                                                                                                       | Not reachable: the local computer's own view stays usable                                                                                                                                                                                                                                                                                    |
| J15 | Connect from a phone                                        | (F10b) Open the client, authenticate the device                                                                                       | Sessions, pending approvals, a terminal if granted                                                                                                                               | Device revoked: nothing is shown and nothing can be sent                                                                                                                                                                                                                                                                                     |
| J16 | Keep a recovery copy and recover (F2h)                      | Entry → Keep a recovery copy → ND-06; later the hook is deleted in an editor → the entry shows **Can be recovered** → Recover → ND-03 | The hook is back at its recorded place, shown as a diff, never called byte-identical                                                                                             | Its place is ambiguous or gone: nothing written, the copy stays, Show lets the user place it by hand                                                                                                                                                                                                                                         |
| J17 | Work with the keyring locked                                | Log in with the keyring locked; use the terminal; open Configuration; try to restore a kept hook                                      | Terminal, sessions, reading and history work; S4b lists kept entries; Restore says "Unlock your keyring to continue. Nothing is lost."                                           | A direct write is refused with `conflict_preservation_unavailable`                                                                                                                                                                                                                                                                           |
| J18 | Handle a recovery incident                                  | ND-11 appears at start                                                                                                                | The user tries to protect it again, or saves it elsewhere, or deletes it; the source unblocks only after that act                                                                | Protecting again fails: the incident stays, unchanged; nothing is removed by itself                                                                                                                                                                                                                                                          |
| J19 | A restored control is undone by the agent                   | J5; a running session writes its older settings back                                                                                  | The result changes from "Written; checking that it stays" to **No longer in place**, the attention list has it, "Apply again" is offered                                         | The Console was closed when it happened: shown first at the next start (28 §5, L2)                                                                                                                                                                                                                                                           |
| J20 | Lose the key                                                | The keyring is reset; open Kept entries                                                                                               | With a code: ND-10, the entries are readable again                                                                                                                               | Without a code: "They are lost. You can delete them." No other action                                                                                                                                                                                                                                                                        |
| J21 | Split and search (T1)                                       | Split right; run a program in each pane; search the scrollback of one                                                                 | Two panes with their own shells; matches highlighted; focus returns to the pane                                                                                                  | Not enough room: "Not enough room to split here."; the limit: its sentence                                                                                                                                                                                                                                                                   |
| J22 | Restore the workspace (T2)                                  | Close and reopen the Console                                                                                                          | The same tabs and splits, titles and folders, with new shells                                                                                                                    | A folder that no longer exists: that pane opens in the home folder with "The folder of this terminal no longer exists." What an agent pane becomes: DN-36                                                                                                                                                                                    |
| J23 | Hand over to another agent (F11a)                           | Session detail → Hand over → S17 → Start                                                                                              | A new session with the reviewed package as its first input; the sender unchanged                                                                                                 | The receiving agent cannot start: as J2; the package is kept in the sheet until the sheet is closed, and nowhere else                                                                                                                                                                                                                        |
| J24 | Pair a device and revoke it (F10a)                          | Devices → Pair → the device shows the code → ND-13 → later Revoke                                                                     | The device sees what it was granted; after revocation its next request is refused                                                                                                | A code typed wrongly or expired: pairing ends; nothing is granted                                                                                                                                                                                                                                                                            |
| J25 | Start without one of the user's controls (F3c, after DN-20) | S3 → profile that leaves out a blocking hook → ND-12 → Start                                                                          | The session runs, marked "Profile: 1 control left out" for its whole life; the global file is unchanged                                                                          | Dialog refused or unavailable: nothing started. The user's hooks changed since the last confirmation: ND-12 again                                                                                                                                                                                                                            |

## 9. Closed sentences

One sentence per closed word (U14; TS-UX-01). Rules:

1. A sentence says what happened, what was left as it was, and what can be done, in that order,
   as far as each applies (U5).
2. A word that two interfaces share has a sentence per interface; the screen uses the one of the
   interface that returned it.
3. `<agent>`, `<file>` and the like are filled by the page from identifiers the backend sent;
   nothing of a file's content or of a provider's raw text enters a sentence (FR-ADP-05).
4. Where two interfaces spell one condition differently, both spellings map to one sentence. The
   known case: IF-CONFIG says `vault_locked`, `vault_no_custody`, `vault_key_missing` (07 §8) where
   IF-VAULT says `locked`, `no_custody`, `key_missing` (27 §6); recorded as a contradiction to be
   resolved in the domain documents (31 §5).

### 9.1 Terminal `[R]` words

The page's closed list (`controller.ts:290-301`) and the backend's codes
(`terminal/contracts.rs:110-125`) `[R]`; today the page shows the word itself (S1, Error).

| Word                 | Sentence                                                                                          |
| -------------------- | ------------------------------------------------------------------------------------------------- |
| `spawn_failed`       | "The shell could not be started. Nothing else was affected. Open a new terminal to try again."    |
| `invalid_cwd`        | "The folder for this terminal does not exist or cannot be opened. Open a new terminal elsewhere." |
| `invalid_dimensions` | "The terminal could not be given a size. Resize the window or open a new terminal."               |
| `write_failed`       | "Input could not be sent to the program. The terminal stopped; its last screen is kept."          |
| `input_limit`        | "Too much input was waiting to be sent, so the terminal stopped. Its last screen is kept."        |
| `resize_failed`      | "The terminal could not be resized. Its program keeps the previous size."                         |
| `busy`               | "The terminal was busy and did not answer in time. Its last screen is kept."                      |
| `limit_reached`      | "Wyss Console runs at most 16 terminals. Close one to open another."                              |
| `operation_timeout`  | "The terminal did not answer in time and was stopped. Its last screen is kept."                   |
| `close_timeout`      | "The terminal did not close in time. It was stopped."                                             |
| `close_failed`       | "The terminal could not be closed cleanly. Its program may still be running."                     |
| `terminal_error`     | "The terminal stopped because of an error. Its last screen is kept. Open a new terminal."         |

### 9.2 Configuration (IF-CONFIG, SM-WRITE, SM-SOURCE; 07, 08) and the vault (IF-VAULT; 27)

| Word                                | Sentence                                                                                                                                                                                   |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `provider_unknown`                  | "Wyss Console does not know this agent."                                                                                                                                                   |
| `project_unknown`                   | "This project is not known to Wyss Console. Choose its folder again."                                                                                                                      |
| `source_untrusted_path`             | "This folder can be changed by another account on this computer, so Wyss Console will not write to it. It is shown read only."                                                             |
| `source_unsupported`                | "This file has permissions, attributes or links Wyss Console cannot preserve, so it will not write to it. Edit it by hand."                                                                |
| `source_too_large`                  | "Larger than the limit; not read."                                                                                                                                                         |
| `source_malformed`                  | "Cannot be parsed at line N. Nothing of it is shown; other files are."                                                                                                                     |
| `source_unreadable`                 | "Cannot be read (permission or a read error). Other files are shown."                                                                                                                      |
| `source_missing`                    | "This file is no longer there. Anything prepared for it was cancelled."                                                                                                                    |
| `not_supported`                     | "<agent> does not support this here."                                                                                                                                                      |
| `unverified_version`                | "Not verified for <agent> <version>. Nothing was written. It was last verified on <version>."                                                                                              |
| `unavailable_no_safe_restore`       | "Not available yet: <agent> has no switch for one hook, and Wyss Console's protected storage is not set up." (or "is locked", or "is not part of this version"), with what would enable it |
| `blocked_managed_inference`         | "Enabling this would make managed Inference unavailable, so it is not offered yet."                                                                                                        |
| `too_many_plans`                    | "Too many changes are prepared at once. Confirm or cancel one first."                                                                                                                      |
| `confirmation_required`             | "This change needs your confirmation in the system dialog."                                                                                                                                |
| `dialog_unavailable`                | "The confirmation could not be shown. Nothing was written."                                                                                                                                |
| `plan_expired`                      | "This change was prepared too long ago, or the file changed. Nothing was written. Prepare it again."                                                                                       |
| `conflict_copy_present`             | "A conflict copy of this file exists (from your sync tool). Changes are blocked until it is resolved."                                                                                     |
| `no_atomic_exchange`                | "Wyss Console cannot replace this file safely here. Nothing was written. Edit it by hand."                                                                                                 |
| `no_private_transaction_dir`        | "Wyss Console's private folder cannot be used for this file, so it will not write it. Nothing was written."                                                                                |
| `provider_session_active`           | "Close your <agent> sessions to change this. Nothing was written."                                                                                                                         |
| `file_held_open`                    | "A program still had the old file open, so your change was withdrawn. The file is as it was."                                                                                              |
| `conflict_preservation_unavailable` | "Wyss Console cannot keep a second version safely right now, so it will not write this file. Nothing was written."                                                                         |
| `repair_pending`                    | "An earlier change to this file is not finished. Changes to it wait; reading goes on."                                                                                                     |
| `journal_unavailable`               | "Wyss Console cannot record this change, so it will not make it. Nothing was written." (a full disk is the usual cause)                                                                    |
| `entry_gone`                        | "This entry is no longer in the file. The list was refreshed."                                                                                                                             |
| `entry_changed`                     | "This entry changed since it was shown. Nothing was written. Look at it again before you change it."                                                                                       |
| `write_failed`                      | "The file could not be written. It is as it was."                                                                                                                                          |
| `span_not_contiguous`               | "This entry cannot be removed without touching others, so it was not changed."                                                                                                             |
| `not_undoable`                      | "This change can no longer be undone automatically."                                                                                                                                       |
| `restore_conflict`                  | "Another entry is in its place. Both are shown. Nothing was written."                                                                                                                      |
| `placement_ambiguous`               | "This hook cannot be put back automatically: its place in the file is not clear. Nothing was written. You can show it and place it by hand."                                               |
| `anchor_missing`                    | "The entries around its place changed, so it cannot be put back automatically. Nothing was written."                                                                                       |
| `container_missing`                 | "The group it belonged to is gone, so it cannot be put back automatically. Nothing was written."                                                                                           |
| `container_ambiguous`               | "More than one group matches where it belonged. Nothing was written."                                                                                                                      |
| `already_present`                   | "This hook is already in the file. Nothing was written."                                                                                                                                   |
| `source_identity_mismatch`          | "The file this belonged to was replaced or moved. Nothing was written; it can be shown."                                                                                                   |
| `consent_required`                  | "Wyss Console needs your consent before it keeps this."                                                                                                                                    |
| `vault_locked`, `locked`            | "Your keyring is locked. Nothing was written. Unlock it to continue."                                                                                                                      |
| `vault_no_custody`, `no_custody`    | "Your system keyring is not available in this session. Nothing was written."                                                                                                               |
| `vault_key_missing`, `key_missing`  | "The key for the kept entries is missing. Nothing was written."                                                                                                                            |
| `vault_full`                        | "Wyss Console is keeping as many entries as it can. Nothing was removed to make room. Clean up entries that are no longer needed."                                                         |
| `entry_too_large`                   | "This entry is too large to keep, so it cannot be disabled here."                                                                                                                          |
| `storage_failed`                    | "The kept copy could not be written (the disk may be full). The agent's file was not touched."                                                                                             |
| `seal_unverified`                   | "The kept copy could not be checked, so it was removed and the agent's file was not touched."                                                                                              |
| `vault_location_unsafe`             | "Wyss Console's private folder is in a place where it will not keep entries. Nothing is kept or changed."                                                                                  |
| `record_unknown`                    | "This kept entry no longer exists."                                                                                                                                                        |
| `corrupt`                           | "This kept entry is damaged and cannot be restored."                                                                                                                                       |
| `unsupported_format`                | "This kept entry is in a form this version cannot place. It can be shown."                                                                                                                 |
| `record_busy`                       | "Something else is being done with this kept entry. Try again when it finishes."                                                                                                           |
| `confirmation_refused`              | "Cancelled. Nothing was changed."                                                                                                                                                          |
| `recovery_unavailable`              | "No recovery code was made for this key."                                                                                                                                                  |
| `recovery_failed`                   | "The recovery code did not open the key. Nothing was changed."                                                                                                                             |
| `export_unavailable`                | "Export is not available in this version."                                                                                                                                                 |
| `import_invalid` (vault)            | "This file is not a valid export, or it was altered. Nothing was imported."                                                                                                                |
| `import_duplicate`                  | "These entries were imported or kept before. Nothing was imported."                                                                                                                        |
| `registry_unreadable`               | "The list of native sessions cannot be read. Terminals and managed sessions are not affected." (a word of the native registry, to be added to 05 §7)                                       |
| Outcome `not_applied`               | "The file changed. Nothing was written. Prepare the change again."                                                                                                                         |
| Outcome `lost_race`                 | "The agent wrote this file at the same moment. Your change was not applied and its version was kept."                                                                                      |
| Outcome `superseded_outside`        | "The file was replaced from outside during the change. Wyss Console read it again: <the effect holds / the effect is gone>."                                                               |
| Outcome `reverted_outside`          | "Your change to <file> is no longer in place: a version written <by the agent / from outside> replaced it. Apply it again?"                                                                |
| Outcome `conflict_preserved`        | "The agent wrote this file at the same moment. Its version is in the file; another version is kept, encrypted. Open both to choose."                                                       |
| Outcome `recovery_incident`         | The sentence of S14 and ND-11                                                                                                                                                              |
| Outcome `unverified`                | "The file was written but does not read back as planned. This is what it holds now."                                                                                                       |
| Outcome `failed`                    | "The change failed. The file is as it was." with the error's own sentence                                                                                                                  |

### 9.3 Native sessions (IF-NATIVE; 05 §4.2, §7)

| Word                       | Sentence                                                                                                                                                        |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agent_not_installed`      | "<agent> is not installed on this computer. Nothing was started."                                                                                               |
| `agent_version_unverified` | "<agent> <version> is not verified with this version of Wyss Console. You can start it; changes to its configuration stay off." (a warning the user can accept) |
| `project_invalid`          | "The folder no longer exists or is not a folder. Nothing was started."                                                                                          |
| `profile_not_applicable`   | "The profile cannot be applied to <agent>: <list>. Nothing was started."                                                                                        |
| `limit_reached`            | "Wyss Console runs at most 16 terminals. Close one to start another."                                                                                           |
| `spawn_failed`             | "<agent> could not be started. Nothing was left running."                                                                                                       |
| `unknown_session`          | "This session is no longer known."                                                                                                                              |
| `still_running`            | "This session is still running. Close its terminal first."                                                                                                      |

### 9.4 Profiles and launches (IF-PROFILE, SM-LAUNCH; 09)

| Word                              | Sentence                                                                                                                          |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `profile_unknown`                 | "This profile no longer exists."                                                                                                  |
| `profile_version_missing`         | "That version of the profile is no longer kept."                                                                                  |
| `profile_invalid`                 | "This profile has a field Wyss Console does not accept. It was not saved."                                                        |
| `profile_cycle`                   | "This profile extends itself through <name>. It was not saved."                                                                   |
| `profile_changed`                 | "Someone saved this profile in the meantime. Your edit was not saved. Open the current version."                                  |
| `profile_unavailable_copy_needed` | "This profile would need a copy of your hooks on disk. It is not available until that is decided." (DN-20)                        |
| `source_exclusion_unverified`     | "This profile needs your user settings left out, and what that also leaves out is not measured yet. It is not available." (OQ-02) |
| `launch_plan_stale`               | "Your configuration changed while the start was prepared. It was checked again."                                                  |
| `confirmation_refused`            | "Not started. Nothing was changed."                                                                                               |
| `import_invalid` (profile)        | "This file is not a profile Wyss Console can read. Nothing was imported."                                                         |
| `failed`                          | "The start could not be prepared. Nothing was started and nothing was left behind."                                               |

Managed-plane words are those already built (`src/lib/features/events/present.ts`) `[R]` and are
not re-derived here.

## 10. Interaction contracts

### 10.1 A change, from click to result

| Step | Page                                                     | Backend             | The user sees                                                            |
| ---- | -------------------------------------------------------- | ------------------- | ------------------------------------------------------------------------ |
| 1    | Sends item id and operation                              | `plan` (pure)       | S5 with its facts, or a refusal sentence; nothing written                |
| 2    | Confirm                                                  | SM-PLAN `confirmed` | Confirm disabled at once (no double submission)                          |
| 3    | —                                                        | ND-01 if due        | "Waiting for your confirmation in the system dialog."                    |
| 4    | —                                                        | SM-WRITE            | "Writing…"; the entry keeps its old state (U10)                          |
| 5    | Receives the outcome                                     | Journal             | §S5.1; the entry's slots update; History has the row                     |
| 6    | For a strengthening change, receives the re-observations | 28 §5               | "Written; checking that it stays" → "In the file", or `reverted_outside` |

A closed window, a closed sheet or a navigation during steps 2 to 4 does not cancel the backend's
work; the result is in History and, if it needs an act, in the attention list.

### 10.2 Native dialogs

The rules of S4c. In addition: one native dialog at a time (SM-PLAN); a second request that needs
one waits and says "Another confirmation is open."; the page's controls that would duplicate the
dialog's question are disabled while it is open.

### 10.3 Destructive and irreversible acts

| Act                        | Where it is confirmed | Rule                                                                  |
| -------------------------- | --------------------- | --------------------------------------------------------------------- |
| Deleting a kept entry      | ND-05                 | Says "only copy" when it is; offers Restore all first and Show beside |
| Stopping a recovery copy   | ND-07                 | The same                                                              |
| Deleting history           | The page              | Says what is kept (open intents, the vault)                           |
| Forgetting a session       | None                  | Only for an ended session; nothing runs                               |
| Closing a running terminal | The page              | Names the program when known                                          |

### 10.4 Clipboard and text

Paths and identifiers are selectable text. A kept entry's content is copied only from ND-04, which
warns about clipboard history. Text from agents and files is rendered as text (U15). Nothing is
copied for the user automatically.

### 10.5 Time and freshness

Every configuration state carries its age (07 §7). A state older than the fallback period is drawn
"last seen at …". A plan shows its expiry in its last 10 s. Nothing refreshes while hidden (U13).

## 11. Visual consistency

The tokens are those of `src/app.css` `[R]` (`--bg`, `--text`, `--muted`, `--faint`, `--accent`,
`--ok`, `--pending`, `--warn`, `--error`, the radii, `--font-mono`); a new screen adds no colour
of its own. The visual design itself (layout grid, spacing, typography scale, iconography) is
the design contract's subject, per screen (19 §4), and the static mockups of §13.

| Element             | Rule                                                                                                                                                     |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| State indicators    | The shapes of §1.1, one set for the whole product; colour adds to the shape and never replaces it                                                        |
| Severity            | Success `--ok`, pending `--pending`, warning `--warn`, error and critical `--error`; neutral `--muted`. Critical differs from error by shape and wording |
| Plane badge         | The same text badge everywhere; Native and Managed differ by word first                                                                                  |
| Provenance          | The three slots always in the order File, Agent, Sessions, with the same labels                                                                          |
| `documented` values | `--muted` text with the word "documentation"; never the verified check                                                                                   |
| Paths and commands  | `--font-mono`, selectable                                                                                                                                |
| Sheets and strips   | One style each across views                                                                                                                              |
| Motion              | Only to show a change of state, under 200 ms, none under reduced motion                                                                                  |
| Themes              | One dark theme `[R]`; others are DN-34                                                                                                                   |

## 12. Decisions left open for the owner

Each is recorded in full in [22](22-DECISIONS-AND-OPEN-QUESTIONS.md). The recommendation binds
nobody.

| ID    | Question                                                                      | Alternatives                                                                                                                                              | Recommended                                                                                  |
| ----- | ----------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| DN-32 | Navigation: which items, in which order, and the default grouping of Sessions | A flat list (§2.2) · grouped by plane · a workspace per project with its own navigation                                                                   | The flat list of §2.2, in that order; Sessions grouped by project                            |
| DN-33 | A capability that waits for a gate: shown disabled with its reason, or absent | Always shown disabled · always absent · shown disabled on an entry's own actions, absent for whole areas                                                  | The third                                                                                    |
| DN-34 | Themes and contrast modes                                                     | The dark theme only · dark and light · dark, light and high contrast                                                                                      | Dark and a high-contrast mode before any distribution; light with F13                        |
| DN-35 | Notifications outside the window                                              | None · pending approvals and recovery incidents only, opt-in · per category, opt-in                                                                       | None until remote; then the second, without content in the notification                      |
| DN-36 | What layout restore does with a pane that ran an agent                        | A shell in the same folder with "Start again" · nothing · start the agent again by itself                                                                 | The first; never the third (05 §4.3)                                                         |
| DN-37 | Default reminder threshold for kept entries                                   | None by default · 30 days · 90 days                                                                                                                       | 90 days, adjustable                                                                          |
| DN-38 | Changing the model of a running native session                                | The user types the agent's own command; the Console shows which · the Console puts the command in the terminal's input for the user to send · not offered | The first now; the second studied with DN-22, because it is the page writing into a terminal |

## 13. Future design deliverables

Static mockups are design deliverables, made in the project's design pipeline when a screen's
phase is planned; none is produced by this document and **no application code is written to
represent them** (the owner's instruction of 2026-10-13).

| Deliverable                              | Content                                                                             | Needed by                   |
| ---------------------------------------- | ----------------------------------------------------------------------------------- | --------------------------- |
| Design contract per screen               | Layout, spacing, type, components, every state of §3, approved by the owner (19 §4) | Each phase with a screen    |
| Static mockups: S4 and S4.3 per kind     | Read-only inventory with the three slots, every source state, every entry state     | F1                          |
| Static mockups: S5 and S5.1              | Each outcome of §1.1.3, including "Written; checking that it stays"                 | F2a                         |
| Static mockups: S4b and its strip        | Each vault state of §S4b.1, each record state of §S4b.2                             | F2v                         |
| Native dialog drafts ND-01 to ND-12      | The text and layout each dialog must hold; checked against OQ-43                    | F2a, F2v, F3c               |
| Static mockups: S1 with panes and search | A split tree, focus, the narrow case                                                | T1                          |
| Static mockups: S2 and S2.1              | Both planes, lost sessions, outside sessions, markers                               | F3b                         |
| Static mockups: S3 with the launch plan  | Each SM-LAUNCH state; a profile unavailable; ND-12                                  | F3a, F3c                    |
| Static mockups: S6                       | Editor, resolved result, versions                                                   | F3c                         |
| Static mockups: S7 per provider          | The visibility table of S7                                                          | F6a                         |
| Static mockups: S15, S16, phone client   | After DN-05                                                                         | F10a, F10b                  |
| Static mockups: S17                      | After the handoff format exists                                                     | F11a                        |
| A token and component sheet              | The tokens of §11 with computed contrast for every pair (TS-A11Y-02)                | Before the first new screen |

## 14. Traceability of this document

| Requirement                      | Sections            | Tests                          |
| -------------------------------- | ------------------- | ------------------------------ |
| FR-UX-01                         | §2.2, §2.3          | Existing shell tests `[R]`     |
| FR-UX-02, NFR-A11Y-01            | §5.2, S12, S1       | TS-UX-03, TS-A11Y-01           |
| FR-UX-03, FR-CFG-03              | §1.1.1, S4.2, S4.3  | TS-UX-01, TS-UX-06             |
| FR-UX-04                         | §9, S13             | TS-UX-01, TS-UX-07             |
| FR-UX-05                         | §1.1.6, S4.4        | TS-UX-02                       |
| FR-UX-06, FR-NAT-11              | §2.5, S2, S2.1, S3  | TS-UX-04                       |
| FR-SES-01, 03, 06; FR-NAT-08, 10 | S2, S2.1, S14       | TS-SES-01                      |
| FR-OBS-04                        | §2.4, §5.3          | TS-UX-09                       |
| FR-DAT-06, FR-SEC-10             | S4b, S4c            | TS-VLT-05, 06, 25              |
| FR-MOD-01 to 10                  | S7                  | TS-UX-01; TS-MOD-01 to 06      |
| NFR-A11Y-02, 03, 04              | §5, §11             | TS-A11Y-02, 03                 |
| U10 (28 §5)                      | §1.1.3, S5.1, §10.1 | TS-UX-08                       |
| U13, NFR-PERF-03, 09             | §6                  | TS-NAT-08, TS-PERF-13          |
| U15, CTL-27                      | §10.4               | TS-UX-10                       |
| U11, CTL-04, CTL-31              | §10.2, S4c          | TS-UX-11; TS-SEC-05; TS-VLT-25 |

## 15. What this specification does not establish

| Assumption                                                                                                                     | State                                 |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------- |
| A native dialog can show a redacted diff, a list of twenty entries and a recovery code legibly, and is read by a screen reader | `[N]` OQ-43                           |
| The page's accessibility tree, and the terminal's screen-reader mode, reach assistive technology through WebKitGTK             | `[N]` OQ-44                           |
| The provider facts behind every slot and every action of S4.4                                                                  | As 06; mostly `[D]`, some `[N]`       |
| The proposed budgets of §6                                                                                                     | Unmeasured (15)                       |
| The leader chord                                                                                                               | OQ-14                                 |
| That the vault, the write protocol and the launcher exist                                                                      | Proposed; blocked as 26 §2 says       |
| That any screen here is the right design                                                                                       | No design contract exists; unreviewed |
