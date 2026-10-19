# ADR 0041: OpenCode in the inventory and its switches (F4a)

Status: Accepted (owner authorisation of the whole roadmap, 2026-10-19); closes DN-04 for reading
Date: 2026-10-19

## Decision

1. **A JSONC reader of our own, no dependency.** OpenCode's `opencode.jsonc` has comments and
   trailing commas. `parse_jsonc` turns comments and the commas before a closing bracket into
   spaces, so every byte keeps its line and column, and reads the rest as strict JSON under the
   same size, depth and element bounds. A failure points at the user's own line and says nothing
   of what the file held.
2. **What is read.** `~/.config/opencode/opencode.jsonc` (else `.json`), the project's
   `opencode.jsonc`/`opencode.json`, and `AGENTS.md` in both places. Items: plugins, MCP servers
   (`mcp`, with `enabled: false` as off), `model` and `small_model`, and providers.
3. **A provider is its name.** `provider.<id>.options` holds API keys. A provider item carries a
   name and whether `disabled_providers` lists it, and no detail, so a key cannot reach an item
   even if redaction missed it.
4. **Static, like Codex.** Every item is `not_verifiable`; nothing runs OpenCode.
5. **Switches, as byte-local edits.** `jsonspan` reads JSONC too (comments are blanks, a comma may
   precede a closing bracket), so every edit is made from the spans of the user's own bytes:
   - an MCP server's `enabled` key is replaced, added or removed, and an undo puts back what it held
     (`Prior`, as for Codex); turning one on asks in the native dialog, since it starts a program;
   - a plugin has no switch in OpenCode, so it leaves the `plugin` list and is kept in the
     restoration store, **always sealed** when it has options (a plausible place for a key); putting
     it back is exact when the file is as the removal left it, otherwise it goes to the end of the
     list, and the native dialog asks, since a plugin is code (`RestoreHook` restores whatever
     record it is given; `ParkPlugin` is the way out).
     Every plan is checked against the same change made to the parsed value; a stretch that would take
     a comment with it, a repeated key, the same plugin twice, an unterminated comment or a record
     that does not match its own entry is **refused**, and the file is never rewritten as a whole.
     Nothing under `provider` is read by the writer, and no plan, record, journal line or `Debug`
     output carries a plugin's options or the file.
6. **Independent review** (a reader that did not write it, 2026-10-19): eleven findings, none
   blocking; the three of medium weight (an undo of an already-present restore parking the user's
   own entry, two entries of one name losing options, an unchecked "exact" restore) and the cheap
   low ones (kind in the item identifier, comment inside an entry, unterminated comment, CRLF,
   duplicate-key cost, `Debug`) are fixed with a test each. Left, said: a comment on the line of
   an entry that moves is not carried along with it (cosmetic; no byte is lost).

## Evidence

`adapter_tests.rs`: comments and trailing commas, no `SEKRET` anywhere in what the window gets,
a broken file reports its own line, comment markers inside strings survive, a project's file is
read only for the selected project. On this machine the real `opencode.jsonc` (with two real
keys) reads and shows none.

Switches: 23 tests for the writer (unit, service, fault matrix with a failure and a crash at every
step of an apply, link, conflict copy, group-writable folder). On the real file (keys included),
one plugin out and back through its record: the bytes after were the bytes before (518 bytes, same
SHA-256 prefix), the `provider` entry was the same bytes in between, nothing of the file was
printed (`real_opencode_config_one_plugin_out_and_back`).
