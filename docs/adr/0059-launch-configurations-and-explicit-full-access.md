# ADR 0059: Launch configurations and explicit full access

Status: Accepted (owner decision, 2026-11-01)
Date: 2026-11-01

## Context

The owner opens ready-made configurations in Warp: a terminal that runs a system update, and
Claude Code or Codex started with their skip-everything flags after a Syncthing rescan. The
owner asked for the same in Wyss Console and chose, among the options offered: launch files
like Warp's, edited by hand; Claude and Codex presets that stay Native sessions (registry,
profiles, Arsenal) with full access as an explicit word; and three launches shipped:
"Actualizar sistema", "Claude Code" and "Codex". ADR 0035 §9 had kept full access out of
profiles.

## Decisions

1. **Launch files.** One TOML file per launch in `$XDG_CONFIG_HOME/wyss-console/launch/`
   (`~/.config/wyss-console/launch/`), named `[a-z0-9_-]{1,64}.toml`, at most 64 files of
   64 KiB. Unknown keys are refused. A file has `name` (≤ 60 characters), an optional
   `directory` (`~`, `~/…` or absolute; the home folder by default) and exactly one of:
   - `commands`: at most 16 lines, each ≤ 4096 characters with no control character. The
     window opens a new tab titled with the name in that folder and, once the shell runs,
     sends each line followed by Enter, as if the owner typed it.
   - `[session]` with `agent` (`claude_code` or `codex`) and an optional `profile` (a profile's
     name). An optional `before` list (at most 8 lines) runs first.
     A file that breaks a rule is listed with why; the others still load.
2. **The window names a file, never a command.** The desktop reads the file again when a launch
   is opened (`launch_get`, `launch_before`); what the window holds is only shown.
3. **`before`** runs in the desktop, each line in `/bin/sh -c`, in the launch's folder, with no
   input and its output discarded, in its own process group, stopped with that group after
   30 s. Every line runs; a line that fails is reported and the agent starts anyway. The agent
   itself starts through the Native path unchanged (ADR 0035 §1): `before` never becomes text
   of the agent's shell recipe.
4. **Sessions are Native sessions.** A session launch resolves its profile by name for its
   agent (a missing one stops the launch before `before` runs), takes the Arsenal selection its
   folder remembers for that agent (the selection its last terminal of that agent started with),
   previews the profile with that
   selection applied when its plan can apply, and starts with that preview's plan, then records
   the selection on the terminal, exactly as a start from the Agents view. A plan that cannot
   apply records the selection without applying it; an applied preview that cannot start stops
   the launch. The session is recorded, can be restarted, and keeps the project rules of ADR 0035
   §4 (the home folder or a known project).
5. **Shipped launches.** When the folder does not exist, it is made (0700) with three files
   (0600): `actualizar-sistema.toml` (`sysupdate`), `claude-code.toml` and `codex.toml` (the
   Syncthing rescan as `before`, the "Full access (bypass)" profile). A folder that exists is
   never written again, so edits and deletions stay. The folder is in the synchronized home, as
   the shell's start-up files are; it adds no trust that those files do not already carry.
   `sudo -v` is not shipped: both computers allow `sudo` without a password.
6. **Full access is one explicit permission word, `bypass`** (amends ADR 0035 §9). Claude Code
   gets `--dangerously-skip-permissions`, Codex `--dangerously-bypass-approvals-and-sandbox`;
   OpenCode and Hermes get nothing and are told so. No other word reaches either flag. Its cost
   is listed in the plan before the start, under the field in the profile editor, and as a
   "Full access" badge on the launch. A read-only template "Full access (bypass)" ships for both
   agents.
7. **Hosts.** Profiles live in the host. A host advertises `native_permission_bypass_v1`; the
   desktop refuses to create or save a `bypass` profile on a host without it (`host_outdated`)
   instead of sending a word that host does not know.
8. **Exact builds.** Profile flags still pass only to builds with recorded syntax evidence.
   Claude Code 2.1.316, the build installed on both computers, is recorded the way 2.1.309 was
   (`--help` lists `--model`, `--effort`, `--permission-mode`, `--settings` and
   `--dangerously-skip-permissions`); Codex 0.159.0's record adds
   `--dangerously-bypass-approvals-and-sandbox`.

## Consequences

- Launches need no new host operation; a commands launch works with any host.
- A `bypass` profile and the shipped session launches need the host of this version.
- A launch has no selection key of its own; to change the selection a folder remembers, start
  once from Agents with another one.
