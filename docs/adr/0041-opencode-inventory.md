# ADR 0041: OpenCode in the inventory, read only (F4a)

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
5. **Not yet.** No switch or write: editing a JSONC file while keeping its comments needs the
   span editor (`jsonspan.rs`) extended to comments, with the same check of each change against
   the same change made to the parsed value. That is the next step, not this one.

## Evidence

`adapter_tests.rs`: comments and trailing commas, no `SEKRET` anywhere in what the window gets,
a broken file reports its own line, comment markers inside strings survive, a project's file is
read only for the selected project. On this machine the real `opencode.jsonc` (with two real
keys) reads and shows none.
