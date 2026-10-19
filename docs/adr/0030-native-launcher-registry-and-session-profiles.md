# ADR 0030: Native launcher, registry and session profiles (F3)

Status: Accepted (owner authorisation of F3a, F3b and F3c and approval of DN-02, 2026-10-19)
Date: 2026-10-19

## Context

F1 to F2b let the Console read and change what Claude Code and Codex are configured with. The
owner's goal is also to start those agents from the Console, see and manage them as sessions,
and start them with saved profiles without touching the global files. Doc 05 §4 and doc 09
describe this and left three decisions open: how an agent is started (DN-02, OQ-07), where
generated files go, and how a profile is applied per provider (OQ-02, OQ-24). The owner approved
DN-02 as recommended: through the user's shell with a controlled exec-replacement, with a safe
documented fallback.

This ADR extends ADR 0005 to 0007 (the window supplies no executable, argument list or
environment) and keeps C-01 and C-02 (doc 04): the code lives in the desktop binary, not in the
library the authority links.

## Decisions

1. **Start through the user's shell, replacing it by the agent.** The terminal runs
   `<shell> -i -c 'exec "$@"' wyss-launch <agent> <args…>` for `sh`, `bash` and `zsh`, and
   `fish -i -c 'exec $argv' -- <agent> <args…>` for fish. The agent's executable name and
   every argument are separate words after the command text; no word is ever placed inside text
   a shell reads. The shell's start-up files run, so the environment is the one the user gets
   when typing the command; the process the terminal owns becomes the agent. Verified for the
   four families against a fake agent (words with spaces, quotes, `$`, `;`, `*`, an empty word
   and `-n` arrive unchanged; the agent's parent is the caller, not a leftover shell).
2. **Fallback.** A shell without a recipe starts the agent directly. The session says so
   (`through_shell: false`) and the page says the environment is then the Console's own.
3. **A closed catalogue.** The agents are compiled in (`claude_code`, `codex`). The window
   names an identifier; it never sends an executable, an argument list, an environment or a
   path. The backend finds the executable (`PATH`, then the usual per-user folders), probes
   `--version` in a process of its own bounded in time and size, and starts the bare name, so
   the user's shell resolves it as it does when typed.
4. **Projects.** A start names a folder that is (a) the home folder, (b) a folder the native
   folder dialog returned in this run, or (c) a project the registry already knows. Anything
   else is `project_invalid`.
5. **Environment.** One closed variable is added, `WYSS_NATIVE_SESSION_ID`, so a hook of the
   agent can tell which session it belongs to; `TERM` is the terminal's own. A closed list of
   variables that belong to _one running agent session_ (`CLAUDECODE`,
   `CLAUDE_CODE_SESSION_ID`, `CLAUDE_CODE_MESSAGING_TOKEN`, `CODEX_THREAD_ID`…) is removed from
   what the agent inherits: found when the Console was started from inside Claude Code, whose
   child it took itself for (transcript saving was off) and whose messaging token it held. The
   user's own configuration variables are not in the list and are inherited as typed.
6. **Registry.** One private file (`<state>/wyss-console/native/registry.json`, 0600, written
   atomically) with the sessions' metadata (agent, version, project, profile and its version,
   times, exit code or signal, restart link) and the known projects. A process id is observed
   and never stored. States: requested (shown as "restarting" for a restart), running, exited,
   stopped (the user stopped it), failed_start and lost. At the next start every session that
   was running is **lost**, never exited: the Console did not see it end, and its terminal died
   with the window. Host-owned persistent terminals are F9; nothing here claims them.
7. **Supervision without a thread.** State is refreshed from the terminal service when the
   overview asks (and slowly, without resources, while a session runs and the view is hidden).
   Processor and memory are read from `/proc` for the process tree only when the overview is
   shown. Nothing is signalled except by Stop (the terminal is closed, which ends its process
   group) and Restart (a new session; the old record stays as it ended).
8. **Native is not managed.** A native session is labelled native everywhere and is never
   presented as mediated, contained or approved by the authority (FR-NAT-11). Two running
   sessions on one folder are flagged: nothing coordinates them.
9. **Profiles** hold references and closed words, never a secret and never a free command:
   a name, the agents it is written for, one parent (depth at most 8, no cycle), a model name
   (a restricted pattern, never starting with a dash), a permission word, a hooks word, a list
   of skill names to switch off, notes (refused when they look like a secret). Dangerous modes
   (`bypassPermissions`, `danger-full-access`) are not words of the schema. Every save is a new
   version (20 kept; bringing one back moves a pointer); a running session keeps the version it
   started with. Export and import move one file through native dialogs; an import drops the
   parent, never overwrites (a suffix is added to a clashing name) and grants nothing. Four
   read-only templates ship; a copy makes one editable.
10. **How a profile is applied (per provider).**
    - Claude Code: `--model`, `--permission-mode` (`manual`, `plan`, `acceptEdits`) and one
      `--settings <overlay>` generated per session under
      `<state>/wyss-console/native/sessions/<id>/` (directory 0700, file 0600), holding
      `disableAllHooks` and `skillOverrides` when the profile asks. The overlay is removed when
      the session ends and all of them at the next start. Observed with the installed Claude
      Code 2.1.303: `disableAllHooks` in the overlay stopped a project `SessionStart` hook for
      that session only (marker absent against present without it), `skillOverrides` in the
      overlay disabled a skill, and the SHA-256 of the user's `settings.json` did not change.
      The same switch covers user hooks by the provider's design; it was measured with a
      project hook, not with the owner's own.
    - Codex: `-m` and `-s <read-only|workspace-write> -a on-request`, expressed as flags only
      (OQ-24 resolved as recommended: no file is written into the provider's home). Hooks and
      skills cannot be switched per session: a profile asking for them **refuses to start**
      with the reason, and so does a profile written for another agent (AC-F3c-02).
11. **Window permissions.** Seventeen new commands, each added to the capability list and to
    `build.rs`. None takes an executable, argument list, environment or path. The capability
    test keeps forbidding the words that would mean a free command for them.

## Consequences

- The Console opens real Claude Code and Codex in its own terminals; the shell the user has
  decides the `PATH`, which is what "the same as typing it" means.
- F9 (sessions outliving the window), the managed plane's approvals and anything that needs
  the authority are untouched.
- Limits, stated: what a running agent does with its configuration is the provider's; the
  hooks-off overlay was measured with a project hook; Codex per-session hooks and skills are
  not offered.
