# Checkpoint

Written so a new session can resume without re-planning. Keep it current at every milestone.

**Branch** `main` (worktree `~/Github_repos/wyss-console-rama-4`; `f1-f2a-configuration` and the two documentation branches were merged into it on 2026-10-19), local only: no
push, no release. Verify with `git log --oneline | head` and `git status`.

## Done (and verified)

| Phase                                       | State | Evidence                                                             |
| ------------------------------------------- | ----- | -------------------------------------------------------------------- |
| F1, F2a, F2b                                | done  | `docs/acceptance/f1-f2a-2026-10-19.md`, `f2b-owner-decisions.md`     |
| Trust for a folder with an ACL              | done  | `agentconfig/acl.rs`, `declared.rs`; `trusted-writers` names `agent` |
| F3a/b/c native launcher, registry, profiles | done  | `docs/acceptance/f3-2026-10-19.md`, ADR 0035                         |
| F9 host process                             | done  | ADR 0036; killed-window test in the real GUI                         |

## Active

Nothing in progress. Next: F6a/b models, F5 authoring, F4a OpenCode, F13 packaging.

## Roadmap still open, in the order being taken

F14 credentials · F11a handoff · F6a/b models ·
F5 authoring · F4a OpenCode · F13 packaging · F10a/b remote · F8 multi-runtime · F12 decision ·
F1b/G-INF (research, needs throwaway homes and the provider's behaviour).
P1 (baselines) needs a reference terminal installed with consent: own-metric scripts only.

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

## Commits of this stretch

F3 backend `277b8f8`, window `5969b8f`, capability `d10b9cb`, env scrub `eb36717`, F9 `ad6a985`, T1/T2 `5708c09` + follow-up.
