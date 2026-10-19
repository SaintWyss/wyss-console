# Checkpoint

Written so a new session can resume without re-planning. Keep it current at every milestone.

**Branch** `main` (worktree `~/Github_repos/wyss-console-rama-4`; `f1-f2a-configuration` and the two documentation branches were merged into it on 2026-10-19), private `origin/main`; validated work is pushed there. No public release.
Recheck both heads and `git status` before writing or pushing.

## Done (and verified)

| Phase                                       | State   | Evidence                                                                                                          |
| ------------------------------------------- | ------- | ----------------------------------------------------------------------------------------------------------------- |
| F1, F2a, F2b                                | done    | `docs/acceptance/f1-f2a-2026-10-19.md`, `f2b-owner-decisions.md`                                                  |
| Trust for a folder with an ACL              | done    | `agentconfig/acl.rs`, `declared.rs`; `trusted-writers` names `agent`                                              |
| F3a/b/c native launcher, registry, profiles | done    | `docs/acceptance/f3-2026-10-19.md`, ADR 0035                                                                      |
| F9 host process                             | done    | ADR 0036; killed-window test in the real GUI                                                                      |
| F14 / F11a closing tests                    | done    | `docs/acceptance/f14-f11a-f13-closure-2026-10-19.md` (isolated Secret Service; real Claude Code to Codex handoff) |
| F4a OpenCode inventory and switches         | done    | ADR 0041; real-file reversible test; `docs/acceptance/f4a-f5-f6-f1b-2026-10-19.md`                                |
| F5 hook/MCP authoring (subset)              | done    | ADR 0042, 0044; reviews; real config and real window (same record)                                                |
| F6a models (managed observed, native)       | done    | ADR 0043; managed choice not built                                                                                |
| F1b research E1–E8                          | partial | `docs/research/inference-isolation-2026-10-19.md`: I4 recommended, not proven; G-INF not implemented              |
| F14 / F11a in the real window               | done    | same record, "In the real window"                                                                                 |

## Active

Product completion authorised by Santiago on 2026-10-19. F5 existing user instruction-file
editing and passphrase-sealed instruction templates are implemented (ADR 0045/0046;
`docs/acceptance/f5-instruction-editing-2026-10-19.md`). Native edit/import apply and exact undo passed in the normal environment. Creation/deletion, Codex skill authoring, project authoring and hook/MCP templates
remain engineering work. Preserve the accepted provider writers and Managed authority guarantees.
See `docs/acceptance/remaining-roadmap-2026-10-19.md` for the remaining scope.

## Current testing policy (owner directive, 2026-10-19)

Development and real acceptance use the owner's normal computer, HOME, XDG directories,
session bus, wallet and installation. No disposable HOME, private keyring/bus, VM,
container or artificial user installation. Ordinary unit fixtures remain permitted;
they are not real integration evidence. Production Managed isolation stays intact.
Historical isolated runs in acceptance records are historical evidence only, never a recipe.

Before a real configuration change: identify the exact path, owner and permissions;
make a verified backup; record its checksum without content; make the smallest reversible
change; check the real provider; restore when appropriate and verify exact bytes and
permissions. Never print credentials. Inspect exact process identities and active native
sessions before changing the installed binary or stopping a host; retain rollback.

## How to resume

1. Check local/remote Git state and recent commits; do not overwrite another session's work.
2. Use `npm run check` and `npm run check:rust`. `rust:test` runs library, binary and
   authority-boundary tests. Do not run blanket `cargo test`: the inherited
   `lifecycle_process` suite creates a disposable user HOME/private systemd installation,
   prohibited by the current owner policy. Its previous run was interrupted; it is not
   current real-environment acceptance evidence.
3. Exercise the native GUI in the normal desktop session and real environment. Never stop
   an unknown host or terminal. Confirm the active pane before typing.
4. Keep Codex hooks blocked until G-INF; no paid credentials, third-party installs or
   network exposure without the specific owner authorisation.

## Commits of this stretch

F5 Codex `e97c48d`; F4a `fc23270`..`0af736b`, F1b `a4a2ca7`, F5 `a5b1614` + `fb9dba2`, F6 `439c741`; F3 backend `277b8f8`, window `5969b8f`, capability `d10b9cb`, env scrub `eb36717`, F9 `ad6a985`, T1/T2 `5708c09` + follow-up.

The still-open roadmap and what each part needs: `docs/acceptance/remaining-roadmap-2026-10-19.md`.
