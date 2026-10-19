# T1/T2/F9 — Native terminal UX and host persistence acceptance (proposed)

**Status:** Documentation supplement based on owner-reported implementation, not an inspection of the local code. Reported F9 commits `ad6a985`, `bb7c4b5`; T1/T2 at local `5708c09` pending native GUI acceptance. Sources: architecture 13 (S1/S12), 19 (T1/T2/F9), 16 (§4.1). Reconcile first with local ADR 0031 and `docs/CHECKPOINT.md`.

## Expected daily-driver experience

Host-owned terminals and agent sessions survive GUI closing; within the UI tabs can contain multiple terminal panes, focus and resize work, scrollback can be searched without changing terminal input, and keyboard shortcuts do not break an interactive provider CLI.

### T1: panes, search and shortcuts

- Horizontal/vertical splits must not create duplicate PTYs unless explicitly requested.
- Each split tree has valid unique leaf/pane identities; removing a pane collapses branches without orphan references.
- Active pane/focus, resize handles and minimum size are accessible with pointer and keyboard.
- Search operates on bounded *rendered scrollback* with documented ANSI/Unicode semantics, result counts and next/previous matches. It must not log query terms, prompts or terminal content.
- Ctrl+Shift shortcuts are interceptable Console commands only; don't swallow necessary provider keys. Show actual mappings and conflicts.
- Full keyboard access, visible focus, pane titles/state not conveyed by color alone.

### T2: layout restore

- Persist split tree, active pane, identifiers and safe UI metadata only. **No transcript, prompt, command history, environment or secret** in layout storage.
- On GUI reopen, attach to existing F9 host sessions using their stable identities and retained output, never spawn duplicates implicitly.
- Missing session must display Lost/Stopped/Disconnected as appropriate and offer an explicit restart.
- Corrupt/newer layout schema recovers to a visible default without deleting host-owned sessions.
- Stale/absent project folders and providers cannot silently change active cwd or launch a substitute.

### F9: host-owned PTY lifetime

- `wyss-console --wyss-host` is distinct from GUI and managed authority, and owns PTYs/scrollback.
- Private Unix socket and parent, verified peer identity, live-host detection (a stale socket alone proves nothing).
- Closing or killing only the GUI does not terminate host-owned provider processes.
- After reopen, the same actual processes reattach; do not infer identity from PID alone.
- If host startup fails, visibly fall back to window-owned terminals with a warning that they end with GUI.
- Host crash/generation mismatch yields honest Lost/Disconnected state. Two clients may not race input ownership or resize unknowingly.

## Immediate GUI acceptance checklist

| Check | Action | Expected |
| --- | --- | --- |
| TERM-UI-01 | Shell tab: vertical then horizontal split | Three panes, correct ownership |
| TERM-UI-02 | Drag dividers, resize, focus by keyboard | Stable PTY sizes and visible focus |
| TERM-UI-03 | Search a harmless marker in scrolled output | Match highlight/count; no prompt mutation |
| TERM-UI-04 | Keyboard shortcuts in Claude/Codex terminal | Console actions and pass-through behave correctly |
| TERM-UI-05 | Close pane, tab, reorder if supported | No dangling tree, orphan or unintended process |
| TERM-UI-06 | Close GUI and reopen with host running | Same sessions/scrollback/split layout |
| TERM-UI-07 | Kill GUI only; relaunch | Host sessions survive |
| TERM-UI-08 | Kill test host/stale socket | Honest Lost/fallback and recovery |
| TERM-UI-09 | Corrupt disposable layout record | Safe default, no crash or session deletion |
| TERM-UI-10 | Narrow width, scale, long titles | Usable responsive layout/focus |
| TERM-UI-11 | Large ANSI/Unicode scrollback search | Bounded execution, accurate renderer |
| TERM-UI-12 | Visit Config/Profiles while PTYs live | No keyboard/resource regression |

IDs TERM-UI-* are proposals; align with canonical TS-TERM/TS-HOST/AC IDs rather than changing the canonical numbering.

## Automated regressions

- Split-tree properties: unique leaves, non-empty branches, parent collapse, focus restoration and no orphan.
- Renderer search over Unicode, ANSI and evicted scrollback, with proper navigation and bounds.
- Layout schema round-trip/migration/corruption fixtures, absent session reference.
- Host generation, rapid disconnect/reattach, stale socket, lost process, EOF, two clients' competing resize/input.
- PTY output survives GUI close/reopen; resize cannot deadlock. No prompt/terminal bytes in persistent layout or diagnostics.
- Benchmarks for split/search/reattach and hidden-view CPU once P1 baseline exists.

## Required local documentation updates

1. ADR/amendment for split-tree ownership, shortcut precedence, renderer search and layout schema; don't invent accepted status.
2. README usage/screenshots (without secrets) for split, search, shortcuts, restore, host fallback.
3. CHANGELOG behavior and limits.
4. Native acceptance evidence with actual commands, resource measurements and F9 process identity.
5. Verify local host socket/service/executable names before documenting installation instructions.

F9 surviving GUI close does **not** mean remote F10, and restoring a layout never guarantees provider survival after a host crash.
