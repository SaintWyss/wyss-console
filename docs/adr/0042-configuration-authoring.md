# ADR 0042: Writing configuration: hooks and MCP servers (F5)

Status: Accepted (owner authorisation, 2026-10-19; DN-12 approved with a native editing window)
Date: 2026-10-19

## Decision

1. **What can be written.** A hook in Claude Code's user settings (add; change its command; remove
   and put back through the existing park/restore), and an MCP server in OpenCode's file (add;
   change its program and arguments or its address; delete and put back). Codex's `config.toml`
   cannot take a new table with the span editor that exists, so authoring there is not offered;
   nothing pretends it is.
2. **Executable text is never built from what the page chose (DN-12).** The page names a kind and
   a place (an event from a closed list, a matcher of plain ASCII, a server name, "program" or
   "address") and the desktop opens a window of its own (`text_form`) in which the user types the
   command. Only what that window returns reaches the planner. The window says what the text will
   become and where, and for an edit names the entry by the backend's own label.
3. **Always the native confirmation.** Every authored plan has `Risk::ExecutesCode`, so the
   confirmation window cannot be skipped by a page message; it shows what will run (cut, plain,
   one line) beside the name, so the user approves the thing and not only a label. The page's own
   diff of the plan is cut and flagged as partial.
4. **Compare and swap.** An edit carries the value the user was shown and is refused if the file
   holds another by the time the plan is made. The plan's hash is checked again at apply, as for
   every change.
5. **Exact take-back.** An authored change keeps, as a record of state `used` (not offered as
   "turned off"), the bytes that went in, the bytes they replaced and where. `RevertAuthored`
   undoes it only while the file is as the change left it, and only if the result is the file as it
   was; it also refuses a record that would change anything but `hooks` (Claude Code) or `mcp`
   (OpenCode). A file that moved on gets `entry_changed`, never a forced overwrite. The record is
   sealed when what it holds may contain a secret, and anything that replaced text is treated so.
6. **What is refused.** Unknown events; empty, multi-line or control-character commands; characters
   that disguise text (line and paragraph separators, bidirectional and zero-width controls);
   matchers with quotes or backslashes; server names outside `[A-Za-z0-9_-]{1,64}`; addresses that
   are not http(s) with a host and no credentials; a name already taken; a server changing kind in
   place; a removal that would take a comment with it.
7. **Independent review** (2026-10-19, no blocker): twelve findings. Fixed with a test each: the
   confirmation now shows what runs; a matcher can no longer spoof the window's wording; a forged or
   imported record cannot take anything but `hooks`/`mcp` out and always asks natively; replaced text
   is sealed; the page is shown a cut of the typed text; the edit window names the entry and
   compares what it showed; a change inside a multibyte character still comes back exactly; offsets
   cannot overflow; "already done" is said only when it is. Left, said: the typed text lives in
   ordinary `String`s until freed (not `Zeroizing`), as does every command line in the process.

## Not done

Import and export of authored entries is the existing export/import of kept records; there is no
template format, so nothing imported is ever a capability. Authoring for Codex and for rule files
is not built.
