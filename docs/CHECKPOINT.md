# Checkpoint

Written so a new session can resume without re-planning. Keep it current at every milestone.

**Branch** `main` (worktree `~/Github_repos/wyss-console-rama-4`; `f1-f2a-configuration` and the two documentation branches were merged into it on 2026-10-19), local only: no
push, no release. Verify with `git log --oneline | head` and `git status`.

## Done (and verified)

| Phase                                               | State | Evidence                                                                                                          |
| --------------------------------------------------- | ----- | ----------------------------------------------------------------------------------------------------------------- |
| F1, F2a, F2b                                        | done  | `docs/acceptance/f1-f2a-2026-10-19.md`, `f2b-owner-decisions.md`                                                  |
| Trust for a folder with an ACL                      | done  | `agentconfig/acl.rs`, `declared.rs`; `trusted-writers` names `agent`                                              |
| F3a/b/c native launcher, registry, profiles         | done  | `docs/acceptance/f3-2026-10-19.md`, ADR 0035                                                                      |
| F9 host process                                     | done  | ADR 0036; killed-window test in the real GUI                                                                      |
| F14 / F11a closing tests                            | done  | `docs/acceptance/f14-f11a-f13-closure-2026-10-19.md` (isolated Secret Service; real Claude Code to Codex handoff) |
| F4a OpenCode inventory and switches                 | done  | ADR 0041; real-file reversible test; `docs/acceptance/f4a-f5-f6-f1b-2026-10-19.md`                                |
| F5 authoring (Claude hooks, OpenCode and Codex MCP) | done  | ADR 0042, 0044; reviews; real config and real window (same record)                                                |
| F6a models (managed observed, native)               | done  | ADR 0043; managed choice not built                                                                                |
| F1b research E1–E8                                  | done  | `docs/research/inference-isolation-2026-10-19.md`: I4 recommended, not proven; G-INF not implemented              |
| F14 / F11a in the real window                       | done  | same record, "In the real window"                                                                                 |

## Active

Nothing in progress. F10/F8/F12 and F1b/G-INF need the decisions listed below.

## Roadmap still open

What is left needs the owner: see `docs/acceptance/remaining-roadmap-2026-10-19.md` (credential path
for F1b, reference terminal for P1, Hermes for F4b, network and authorisation for F10, DN-08 for
F8/F11b, DN-06 for F12). Codex authoring (F5) is engineering and has no owner dependency.

## How to resume

1. `git log --oneline | head -5`; `git status --short`.
2. Validation: `cd src-tauri && CARGO_HOME=$PWD/../.cache/cargo cargo fmt --check && cargo clippy
--offline --all-targets -- -D warnings && cargo test --offline`; then at the root (with
   `ln -s ~/Github_repos/wyss-console/node_modules node_modules`, removed before committing)
   `npx prettier --check . && npx eslint . && npx svelte-check && npx vitest run`.
3. GUI: `npm run desktop:build`, then a nested KWin + Xwayland (`/tmp/claude-1000/f3/nest-up.sh`
   in the previous session; the recipe is in the memory file `reference_display_anidado_kwin`),
   app with its own `XDG_STATE_HOME` and `XDG_RUNTIME_DIR`, real `HOME`. Stop a leftover host
   with `wyss-console --wyss-host-stop`.
4. Never type into a tab without confirming which one is active.
5. **Do not isolate `XDG_DATA_HOME` for a GUI run with the real HOME**: Claude Code's installer
   moved `~/.local/bin/claude` to a copy under that directory (repaired once; the symlink must point
   at `~/.local/share/claude/versions/<version>`). Isolate only `XDG_STATE_HOME` and `XDG_RUNTIME_DIR`.

## Commits of this stretch

F5 Codex `e97c48d`; F4a `fc23270`..`0af736b`, F1b `a4a2ca7`, F5 `a5b1614` + `fb9dba2`, F6 `439c741`; F3 backend `277b8f8`, window `5969b8f`, capability `d10b9cb`, env scrub `eb36717`, F9 `ad6a985`, T1/T2 `5708c09` + follow-up.

The still-open roadmap and what each part needs: `docs/acceptance/remaining-roadmap-2026-10-19.md`.
