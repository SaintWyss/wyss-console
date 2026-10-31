# Agent delegation

An orchestrator session started from Wyss Console (Workspace › Start an agent, Claude Code or Codex,
"Let it delegate…" on) can give a task to a separate Claude Code or Codex agent through one Wyss
tool. The same service runs both directions; only the two worker adapters differ.

## Pieces

| Piece               | Where                                                                | What it does                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------------------- | -------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Delegation tool     | `wyss-console --wyss-delegate-mcp` (`src-tauri/src/delegate/mcp.rs`) | MCP server over stdio. Tools: `wyss_delegate_options`, `wyss_arsenal_capabilities`, `wyss_delegate_preview`, `wyss_delegate`, `wyss_delegation_wait`, `wyss_delegation_status`, `wyss_delegation_cancel`, `wyss_delegation_list`. Holds no right: it forwards to the terminal host with the session's token.                                                                                                                                                                                                                                                                                   |
| Orchestrator wiring | `native/service.rs` `delegation_words`                               | Claude Code: `--mcp-config=<session>/wyss-delegate.mcp.json`. Codex: `--no-daemon -c mcp_servers.wyss_delegate.{command,args,env_vars,tool_timeout_sec}`. The token travels in the session's environment (`WYSS_DELEGATION_TOKEN`), never on a command line, and is only in the host's memory. No global provider setting changes.                                                                                                                                                                                                                                                             |
| Service             | terminal host, `src-tauri/src/delegate/mod.rs`                       | Ops `delegate_preview/start/get/list/cancel/options/capabilities`. Checks the token, the policy, the model and effort, plans the worker's skills, starts the worker, reads its stream, verifies its configuration, enforces the deadline, keeps the record.                                                                                                                                                                                                                                                                                                                                    |
| Worker adapters     | `src-tauri/src/delegate/worker.rs`                                   | Codex: `codex exec --json -m <model> -c model_reasoning_effort=<e> -s workspace-write -C <workspace> -c approval_policy="never" -c sandbox_workspace_write.network_access=false`, the user's MCP servers and plugins switched off for that process, Arsenal's `-c skills.config=[…]`. Claude Code: `claude -p --output-format stream-json --verbose --model <m> --effort <e> --settings <overlay> --permission-mode acceptEdits --strict-mcp-config --disallowedTools Bash,WebFetch,WebSearch,Agent,Task`; the overlay holds Arsenal's `skillOverrides` and switches every enabled plugin off. |
| Window              | Workspace › Delegated agents                                         | Every delegation with its provider, model, effort, skills, check, result, files; Cancel for a running one.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |

## Rules

- **Policy** `~/.config/wyss-console/delegation.json` (all optional): `max_concurrent` (2, at most 8),
  `max_minutes` (20, at most 120), `providers` (`codex`, `claude_code`), `claude_models`, `claude_allow_bash`
  (false). A missing file uses the defaults. An invalid or unreadable file blocks new delegations
  until it is corrected; the options response reports its source and warning.
- **Models and effort**: Codex's are read from `codex debug models` (listed models and their
  `supported_reasoning_levels`); Claude Code's are the policy's list and `low…max`. Anything else is refused,
  never substituted. Options label catalogue/policy support separately from model/effort pairs
  confirmed by successful verified execution records. Claude's init does not report reasoning
  effort, so its requested effort is not presented as runtime-confirmed.
- **Preview**: `wyss_delegate_preview` takes the same request as `wyss_delegate` and returns its
  effective skills, model, effort, policy and restrictions without launching, reserving capacity or
  creating execution history. Execution revalidates everything. Pin the revisions returned by the
  preview when a stable capability choice is needed; a preview is not a launch authorization.
- **Skills**: the worker gets only the Arsenal capabilities the request names (ids with optional pinned
  revisions, a preset, or a selection of its provider), planned exactly like a terminal's. With none named it
  gets none of the owner's skills. Every named capability must be the installed revision and the provider
  build must have its own exclusion evidence; otherwise nothing starts. Duplicate picks keep an
  explicit revision over an unpinned pick; conflicting explicit revisions are refused.
- **Isolation**: each worker has its own workspace under `~/.local/state/wyss-console/delegations/<id>/`
  and its own process group inside a Bubblewrap PID namespace. `/usr/bin/bwrap` is required; there
  is no uncontained fallback. Descendants using `setsid` remain inside that namespace. The wrapper
  preserves existing host filesystem permissions; it does not make skill files unreadable. Workers
  receive no parent's delegation token, MCP servers or plugins. Codex native multi-agent support
  and Claude's Agent/Task tools are disabled.
- **Verification**: Claude Code's `init` event (model, skills, plugins, MCP servers) is compared with the plan
  as soon as it arrives and a mismatch stops the worker; Codex's session record (the `SKILL.md` files listed
  to the model and authoritative model/effort settings) is compared after it ran. The record says
  `verified`, `mismatch` or `unavailable`; a skill's
  use by the worker is never claimed from configuration.
- **Lifecycle**: a task ends `succeeded`, `failed`, `cancelled`, `timed_out` or, if the host stopped while it
  ran, `interrupted`. Admission reserves capacity atomically, including workers being prepared.
  Cancellation and deadlines terminate the worker namespace; capacity is released after process
  cleanup and reaping, including when a descendant holds stdout open. Finished records stay on disk
  while their entries are removed from the live-process map.
  A finished parent session loses its delegation token, including when its ending cannot be
  persisted. Launch and revocation are serialized after preparation so a stale right cannot
  start a new worker after revocation.

Codex's stdio MCP environment explicitly forwards both `WYSS_DELEGATION_TOKEN` and
`XDG_RUNTIME_DIR`. The latter locates the host socket; neither value is embedded in the Codex
command line. Desktop listing and cancellation are granted to the local main window's Tauri
capability. The window shows completed-step counts as progress.
Preset/selection identities are retained in execution history; the UI distinguishes requested
model/effort from observed provider metadata.

Provider catalogue or CLI syntax support does not prove account availability for every offered
model. A skill's inclusion in the provider catalogue verifies exposure; reading or invoking it
is separate evidence of use.
