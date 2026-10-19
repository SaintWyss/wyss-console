# What the roadmap still holds, and what each part needs

State at `main` after the merge of 2026-10-19. "Built" means in the application and verified as
the acceptance records say; nothing below is declared done that is not.

## Built

F1 inventory · F2a Claude Code hooks and skills · F2b Codex skills and MCP servers · the ACL trust
rule · F3a/b/c native launcher, registry, profiles · F9 host that outlives the window ·
T1/T2 panes, search, keys, saved layout · F14 sealed API keys · F11a handoff brief ·
F13 install, upgrade, rollback, diagnose · F4a OpenCode (inventory and switches) · F5 authoring of
hooks and OpenCode servers · F6a models (managed observed, native three facts) · F1b research
(recommendation I4, not proven).

## Not built, and why

| Phase                        | What is missing                                                                                                                              | What it needs                                                                                                                                                                                                             |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P1 baselines                 | Numbers against a reference terminal; budgets restated                                                                                       | A reference terminal installed with the owner's consent (OQ-26, OQ-27 answered). Own-metric measurements are in `f3-2026-10-19.md`                                                                                        |
| F1b / G-INF                  | The parts of E3, E5, E6 that need a credential, and the implementation of managed Inference isolation (recommendation I4, `docs/research/…`) | A credential path the owner chooses for E3 (a real refresh), E5 (the authority's preflight on an isolated start) and E6 (one turn, one resume); a decision on old thread bindings (E7); then an ADR and a security review |
| F4a OpenCode                 | Nothing planned: inventory, switches and exact undo are built (ADR 0041). Project-level OpenCode files are read, not written                 | —                                                                                                                                                                                                                         |
| F4b Hermes                   | Hermes as an upstream                                                                                                                        | Hermes is not installed; DN-13; the owner's OK to install it                                                                                                                                                              |
| F5 authoring                 | Codex hooks (blocked until G-INF), skills and rule files; a template format for import                                                       | Claude Code hooks, OpenCode and Codex MCP servers are done (ADR 0042, 0044). Codex hooks need G-INF                                                                                                                       |
| F6a/b models                 | A managed model **choice**; accepting `model/rerouted`; a model catalogue view                                                               | A separate authority contract and review (SG-12), and a classifier change reviewed on its own (ADR 0043). Observation, and the three facts of a native session, are done                                                  |
| F8 multiple managed runtimes | Several managed turns at once                                                                                                                | A redesign inside the authority (DN-08) with schema migration and per-generation attestation: not decided by the owner                                                                                                    |
| F10a/b remote                | A gateway, pairing, grants, a desktop and phone client                                                                                       | DN-05's reachability decision (a private overlay network the owner provides, OQ-18), a threat review, and a network-facing service: **not started without separate authorisation to expose a service**                    |
| F11b a second managed agent  | Another agent under the authority                                                                                                            | F8 first; an ADR per agent                                                                                                                                                                                                |
| F12 Wyss Agent               | A decision, not a build                                                                                                                      | DN-06 on real installations                                                                                                                                                                                               |

## Next

Everything that can be built without a decision of the owner's is built. What follows is the
owner's: a credential path for F1b, consent for a reference terminal (P1) or for Hermes (F4b), the
overlay network and the OK to expose a service (F10), DN-08 (F8, F11b) and DN-06 (F12). The one piece of engineering left
is authoring of skills and rule files, which nothing blocks.
