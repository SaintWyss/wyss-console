# Wyss Console — user guide (versioned draft)

**Audience:** First-time and daily users.
**Verification scope:** Features under "Available according to owner acceptance reports" were exercised on the owner's CachyOS host through local HEAD `5708c09`; this branch cannot read that commit, so instructions are reported, not independently reverified. The "Planned" section is not a promise of a working control. Reconcile names with the running GUI before shipping.

## 1. What Wyss Console does

Wyss Console is a native desktop workspace for local terminals, installed AI coding agents, configuration inspection, session profiles and a separate secure Managed plane. It does not itself replace Claude Code, Codex, their subscriptions or their permissions.

**Native** means a CLI you run in a terminal under its normal provider security and login. **Managed** means execution governed by the separately verified authority. They are not interchangeable.

## 2. First launch

1. Start the packaged application using the install instructions matching your build.
2. The Console is useful as a terminal even without any coding-agent installation.
3. Check the visible host/authority connection statuses. A configuration scan should be read-only.
4. Open **Workspace** to see installed Claude Code/Codex providers and versions.
5. Missing providers are simply unavailable; installation and account sign-in remain provider-owned actions.

If permissions prevent modifying a configuration source, it remains read-only. Do not loosen operating-system permissions merely to force a switch.

## 3. Launch a native agent

1. Open **Workspace**.
2. Choose the detected Claude Code or Codex version.
3. Select your project folder using the native picker.
4. Optionally select a saved session profile.
5. Launch. The installed CLI opens in a Console terminal tab.
6. Interact with the provider directly. Provider trust or login prompts are the provider's, not Wyss Console approvals.

The launcher reportedly uses the user's configured shell with process replacement. It scrubs a documented set of unrelated environment secrets. Do not use the environment as a general-purpose password store; provider-specific authentication belongs to the provider or future credentials manager.

## 4. Profiles

Use **Profiles** to create, version, duplicate, import and export launch profiles. A profile affects the new session; it must not silently rewrite global settings.

A Claude profile may configure launch arguments or an overlay for supported settings; for example disabling hooks for one launch. Settings unavailable on a provider must produce a visible refusal to launch, not an approximate application.

Imported profiles are untrusted; inspect them before use. Do not put passwords or API keys in profiles.

## 5. Sessions, tabs and host persistence

Use the session overview to inspect Native providers, profile, process state, CPU and memory. Stop/restart/remove only the selected sessions.

The reported F9 host process keeps terminals alive after closing the GUI. On reopening the Console, retained host sessions and bounded scrollback reattach. **If the host itself exits, no promise of surviving processes applies.** The GUI must not confuse Lost with clean provider exit.

If the host cannot start, the app falls back to window-owned terminals and displays that closing the window terminates them.

## 6. Configuration and undo

Configuration lists supported hooks, skills, rules and MCP sources at their verified scope and source. For a change:

1. Inspect the entry and affected provider/scope.
2. Review the planned diff/effect, noting predicted versus provider-verified.
3. Confirm if prompted via a native dialog.
4. Check the result. Undo is available only if restoration conditions remain valid.

For Claude, skills reportedly follow the verified `skillOverrides` behavior. Codex skills and user-scope MCP toggles were verified, but individual Codex hook mutation remains read-only until G-INF approval. Live provider sessions may need restart and may not reload external changes.

A restore must not overwrite newer provider edits; the app should surface a conflict.

## 7. Terminal panels and search — GUI acceptance pending at last report

T1/T2 were reported implemented at `5708c09`: split terminal panes, drag dividers, switch focus, search scrollback, use Ctrl+Shift shortcuts and restore layout on reopen. However, the owner said no full real-GUI walkthrough had happened yet. Treat actual shortcut names and detailed menu steps as **to be verified**, not a printed fixed keymap.

Until verified, use the GUI's visible controls and keep independent provider prompts distinct from shell tabs; title/order may change after session restore. The documented test checklist is in `../spec/terminal-and-host-acceptance-T1-T2-F9.md`.

## 8. Errors and safe responses

| Situation                     | What it means                                      | Safe action                                                    |
| ----------------------------- | -------------------------------------------------- | -------------------------------------------------------------- |
| Provider missing              | CLI not found or unsupported                       | Install/sign in via provider; refresh detection                |
| Configuration read-only       | Directory/source trust rule failed                 | Inspect trust and owner policy; do not apply blind chmod       |
| Wallet locked                 | Encrypted restore data unavailable                 | Unlock OS key store; never seek plaintext fallback             |
| Conflict on undo              | Provider file changed since mutation               | Review newest source; do not force old bytes                   |
| Lost session                  | Connection, host or original process state changed | Review host status; explicitly restart if desired              |
| No real in-use model evidence | Only configured/selected value known               | Use provider's own runtime display                             |
| Managed safety unavailable    | Required authority checks failed                   | Do not bypass using native work for the same managed operation |

## 9. Privacy expectations

Normal local terminal/provider execution may access project files and network according to each CLI's own capabilities. The Console's configuration UI should not transmit your configuration files, command history or credentials over the network by default.

Do not share screenshots of terminal content or configuration windows without reviewing them. Only reviewable redacted diagnostics should be exported.

## 10. Planned capabilities — NOT available simply because documented

OpenCode/Hermes adapter extensions (F4), full creation/edit of executable configuration entries (F5), Models (F6), secure API credentials management (F14), reviewed handoff (F11), remote workspace on a second PC/phone (F10), expanded managed concurrency (F8) and distribution/updater (F13) are specified but were **not implemented** in the owner's last report. Wyss Agent strategy (F12) is still a decision.

This guide gains exact screenshots, labels, supported versions, shortcuts and installer procedures only after native acceptance on local HEAD.
