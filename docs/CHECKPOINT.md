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
| F5 project files, hook/MCP templates        | done    | ADR 0050; tests only, real window not yet exercised on the owner's desktop (installed build is older)             |
| F6a models (managed observed, native)       | done    | ADR 0043; managed choice not built                                                                                |
| F6 catalogue view, `model/rerouted`, effort | done    | ADR 0048; `docs/acceptance/f6-models-2026-10-20.md`; new authority behaviour proven by tests only                 |
| F10a gateway core (no transport)            | partial | ADR 0049; 31 tests; unreviewed, unreachable; no signature scheme, no listener (DN-05)                             |
| F1b research E1–E8                          | partial | `docs/research/inference-isolation-2026-10-19.md`: I4 recommended, not proven; G-INF not implemented              |
| F14 / F11a in the real window               | done    | same record, "In the real window"                                                                                 |

## Active

Product completion authorised by Santiago on 2026-10-19. F5 user instruction-file editing,
passphrase-sealed templates (ADR 0045/0046) and now **absent-aware creation/deletion of
instruction files and skills, including Codex skills in `~/.agents/skills`** (ADR 0047,
`docs/acceptance/f5-create-delete-2026-10-20.md`) are implemented and accepted in the real
window. Project-level authoring (no project selection yet for the configuration engine) and
hook/MCP templates remain engineering work. Preserve the accepted provider writers and Managed
authority guarantees. See `docs/acceptance/remaining-roadmap-2026-10-19.md`.

## Installed/runtime state at this milestone

- Code `e17dfc2` is built, installed at `~/.local/opt/wyss-console`, and opened in the
  normal desktop. `--diagnose` reports revision `e17dfc2d8264`; the preceding installation
  is at `.previous`. `--keep-host` retained the live protocol-1 host and its terminal.
- Last observed GUI PID 1310334, host 1188641, original fish 1188667. These are historical
  identities to recheck, never targets to kill blindly. No native agent is running under
  the host. The temporary release GUI and benchmark Ghostty window were closed.
- Frontend 800 tests; Rust 501 library + 291 binary + 1 boundary, 12 ignored; clippy,
  formatting, repository scan and package build passed. Native editing/template import,
  separate confirmations and exact undo passed on real benign acceptance rules/skill;
  all created provider files were removed after exact restoration. Original real provider
  configuration checksums/owners/modes matched. Provider consumption/hot reload is not proven.
- P1 is partial: two 300-second observations, one alongside installed Ghostty 1.3.1;
  see `docs/acceptance/p1-2026-10-19.md`. Significant swap and uncontrolled background/
  visibility conditions prevent canonical conclusions. Ghostty no longer needs installation.
- The mistakenly started inherited private-systemd lifecycle run was interrupted. Its
  exact private manager was identified and stopped, and its generated fixture removed.
  Do not repeat that suite under the current owner policy.

## F6 increment (2026-10-20)

Models view (offered / configured / selected / reported, with provenance and unknown entitlement),
`model/rerouted` tolerated for exactly the running turn, native reasoning effort in profiles
(ADR 0048). Independently reviewed; its fix is in. The new authority behaviour is **not** running on
this computer (the installed authority is older and replacing it interrupts managed sessions); the
host keeps working with an older host through feature negotiation. Managed model choice is not
built (SG-12, DN-42).

## P1 increment (2026-10-20)

Backend baseline over three runs, an idle-wake-up optimization of the host and the page's output pull
(`docs/acceptance/p1-2026-10-20.md`). **P1 stays open.** The host change is not running: the installed
host is the older one (replacing it ends the owner's terminals), so it applies at the next install and
host restart. Rule learned: never end a process found by name; only the PID of one's own launch, verified.

## Next unblocked engineering

F5 is complete (ADR 0050). Remaining: eligible F6 acceptance against the installed runtime, the
rest of the P1 matrix through `benchmarks/owned.py`, a second focused review of the F10a core. F10 local foundations
may proceed without a listener; F8/F11b cannot replace the authority contract before its
security verification. Hermes installation, real credential/quota experiments and remote
exposure/device enrolment still require specific owner authorisation. No G-INF completion
claim: E3 remains partial, E5/E6 unresolved.

## Existing branch reconciliation (subsequent 2026-10-19 task)

Santiago authorised integrating valid branch work and explicitly excluded `rama-1`,
`rama-2` and `rama-3`. Reviewed B1 pure contracts/admission/snapshot/test-fixture work
was merged (`3f9bf38`), with two reproduced validation defects fixed. The old planning
audit was merged as historical evidence (`c2fc6c2`). Only fixture-marker packaging
checks were recovered from the native E2E branch; its disposable HOME/private-systemd
harness remains excluded. The weaker JavaScript B1 prototype remains excluded after
reproducing path and export-digest defects. Existing worktrees/branches are preserved.

Validation: frontend 804 tests, Rust 622 library + 291 binary + 1 boundary (12 ignored),
14 documentation tests; final focused B1 121 tests and all-target clippy passed.
No production B1 worker or new authority permission was enabled. FX-H01/B1 containment
and G-INF are still not proven. No GUI/provider behaviour was added or newly accepted;
the real installed `e17dfc2d8264` GUI/host/terminal and rollback copy are retained.
See `docs/acceptance/branch-integration-2026-10-19.md` for per-branch decisions and
the final package/push record.

Integration code `ac3b056cd0c5` was release-packaged, its manifest hashes and fixture
exclusion verified, and pushed to private main (remote HEAD checked). The package was
not installed. Repository scan: 830 tracked files, four checks passed. The only branch
work deliberately left out is the three F1 trials, the weaker JavaScript prototype and
the incompatible native E2E harness; the latter's packaging check was recovered safely.
This acceptance closure follows as a documentation-only commit. Recheck Git for its
exact current local/remote HEAD. No new roadmap feature is claimed complete here.

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

Current product completion: `fc1473a` policy/context; `1c6df1d` host-preserving upgrade;
`1600b78` instruction edits; `e17dfc2` sealed instruction templates. All pushed to private main.

F5 Codex `e97c48d`; F4a `fc23270`..`0af736b`, F1b `a4a2ca7`, F5 `a5b1614` + `fb9dba2`, F6 `439c741`; F3 backend `277b8f8`, window `5969b8f`, capability `d10b9cb`, env scrub `eb36717`, F9 `ad6a985`, T1/T2 `5708c09` + follow-up.

The still-open roadmap and what each part needs: `docs/acceptance/remaining-roadmap-2026-10-19.md`.
