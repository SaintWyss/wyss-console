# ADR 0044: Writing Codex's configuration: MCP servers as TOML tables (F5, Codex)

Status: Accepted (owner directive of 2026-10-19: Codex authoring, on the real installation)
Date: 2026-10-19

## Decision

1. **What is written.** In `~/.codex/config.toml`: a new MCP server as a new `[mcp_servers.<name>]`
   table at the end of the file, a change to the program, arguments or address of an existing one
   (replaced in place; the arguments line is added or removed as needed), and a server taken out
   with every table of it (`env`, `tools.*`) and put back. Everything of ADR 0042 holds: the
   command is typed in a native window, the confirmation is always native and shows what will run,
   an edit carries what the user saw, every authored change comes back to the exact bytes.
2. **Byte-local, or refused.** `tomlspan` places statements and tables; every edit is checked by
   parsing before and after and comparing with the same change made to the value (an empty file is an
   empty table, and an empty `mcp_servers` is none). Refused, with the file untouched: a server
   defined by dotted keys or an inline table, tables of one server that are not together, the same
   server twice, a statement with a trailing comment that a removal would take, a name already taken.
3. **A take-back is scoped.** A record of an authored change names its server. Taking it back may
   change `mcp_servers` and, inside it, that server and no other; a forged or imported record that
   would reach `approval_policy`, `sandbox_mode`, hooks, `[features]` or `[projects.*]` is refused, and
   the plan shows what goes out or comes back.
4. **Sealing.** A record of a server is sealed when anything it writes or replaces may hold a token:
   always for arguments, always for a parked server.
5. **Not offered.** Codex hooks stay unwritable (where Codex keeps their switch is unverified, and
   enabling one is blocked until G-INF); skills and rule files are not authored.
6. **Independent review** (no blocker): parking the only server was refused as malformed, a server
   with an unusual name could be parked and not restored, a take-back record could reach another
   server, an add kept arguments unsealed, `Debug` of a record or of `Authoring` could print what
   the user typed. All fixed, with a test each.

## Evidence

77 service tests (new table, no final newline, CRLF, empty file, edit in place keeping comments and
the `env` table, park with subtables, fault matrix over every step, structures refused, forged
records). **On the real `~/.codex/config.toml`** with the installed Codex 0.159.0 (a backup of the
exact bytes first; nothing printed): a made-up server added (the real `codex mcp list` showed it), its
command changed (`list` showed the new one), both taken back, then the real `notebooklm` server with
its tables out and back through its record; at the end the file was byte-for-byte the original, same
SHA-256 (`bf11268d…`), same mode 0600, and `codex mcp list` showed what it showed before. Then in the
**real window** (the installed 0.2.0 build, the real state and wallet): "Add an MCP server…" → Codex →
the native window ("Codex will start the program you write…") → plan → native confirmation showing
`starts: true guitest` → Apply (Codex saw the server) → History → Undo (the plan showed what goes
out) → confirmation → Apply: `cmp` against the backup, byte-identical.
