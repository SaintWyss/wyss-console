# Arsenal skill exclusion — focused qualification of Claude Code 2.1.323

Date 2026-11-12 · builder's record · run root `~/.local/state/wyss-a0/20261112-arsenal-q323-a6aa2c/` (raw evidence stays local).

Claude Code updated itself to 2.1.323 on Santiago-PC on 2026-11-12 (2.1.321 and 2.1.322 were installed in
between and were never qualified). The record of 2.1.319 does not cover another build, so the same
qualification was repeated for this one.

- Binary `2.1.323 (Claude Code)`, SHA-256 `604d29a6869409106b6a1110bdadcc8b72e8654a5a865d23258f938a696e348e`.
- Method: identical to [ARSENAL-Q319-QUALIFICATION-2026-11-07.md](ARSENAL-Q319-QUALIFICATION-2026-11-07.md): the
  harness-v2 tree of that run root with only the pinned binary, its digest and the onboarding version
  changed; bwrap `--unshare-all`, empty launcher environment, tmpfs HOME, synthetic key, loopback mock.
  Isolation preflight 34/34.
- Cases: the same nine, the 2.1.319 `evidence/PRE-REGISTERED.json` copied before the first launch.

| Case      | Overlay (off)           | Mode        | Verdict |
| --------- | ----------------------- | ----------- | ------- |
| Q-C0      | none (control)          | headless    | PASS    |
| Q-S1-1..3 | beta, proj              | headless    | PASS ×3 |
| Q-S2-1..3 | alpha, gamma            | headless    | PASS ×3 |
| Q-H1      | beta, proj + user hooks | headless    | PASS    |
| Q-S1-I    | beta, proj              | interactive | PASS    |

The nine cases prove Arsenal skill exclusion only.

## Observations for hooks and MCP per terminal (not qualification)

Run on the real account with Haiku, outside the harness, on 2026-11-12. They inform the design of
selectable hooks and MCP servers; they do not qualify anything.

- A `SessionStart` hook passed only through `--settings <file>` runs.
- `--strict-mcp-config --mcp-config <file with no servers>`: the session starts with 0 MCP servers
  (18 without the flag, counting plugin servers and claude.ai connectors).
- `--setting-sources project,local`: the user's hooks do not run (1 `SessionStart` hook instead of 7),
  but the user's `~/.claude/CLAUDE.md` is not loaded either. Not usable to switch hooks off.
- `disableAllHooks: true` in the `--settings` file also stops the hooks that file adds. Not usable either.

The limits of the 2.1.314 record apply unchanged. saint-acer was not checked: it qualifies only if it
runs this exact build.
