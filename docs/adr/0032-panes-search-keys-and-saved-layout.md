# ADR 0032: Panes, scrollback search, a key map and a saved layout (T1, T2)

Status: Accepted (owner authorisation of the whole roadmap, 2026-10-19)
Date: 2026-10-19

## Decisions

1. **A tab is a tree of panes.** Each leaf is one terminal with its own PTY in the host; at most
   16 panes per tab, nesting at most 6, a pane never below 10 % of a split. Panels stay mounted
   and are only positioned by their share of the tab's area, so splitting never recreates a
   terminal's DOM host. Dividers are dragged with the pointer.
2. **Search without a new dependency (DN-04, deviation).** Doc 05 recommended xterm's search
   add-on. The Console searches `term.buffer` itself (case option, step with Enter and
   Shift+Enter, at most 5000 matches counted, the match selected and scrolled to). No package is
   added to the supply chain. Limit: columns of a line with wide characters are approximate.
3. **One leader chord (OQ-14).** Ctrl+Shift plus the physical key: T new tab, D/E split, W close
   pane, F search, arrows or `.` `,` move between panes, PageUp/PageDown and 1 to 9 for tabs.
   C, V (copy, paste) and U (an input method's unicode entry) are not taken; every other key goes
   to the program. The chord is read by `code`, before the terminal sees it. Seen in the real
   window with fish; not measured with the owner's input method in use.
4. **Layout is saved by the desktop, kept by nobody else.** `<state>/wyss-console/layout.json`
   (0600, atomic, at most 64 KiB, a JSON object): tabs, split trees, focus, and per terminal its
   title, folder and whether it was an agent. The window validates what it reads (bounds, no
   repeated terminal, titles cleaned). On opening it lays the host's live terminals out as saved;
   a shell the layout names that the host no longer has is made again with its title and folder;
   an agent is never started again from a layout; terminals the layout does not name get a tab
   each. Scrollback is the host's (ADR 0031), not the layout's.
5. Two window commands, `layout_read` and `layout_write`, added to the capability list.

## Verified

In the real window: three panes by chords, a divider dragged, a search "1 of 1" with the match
highlighted, then the window killed and reopened with the same tree, ratio and focus.
