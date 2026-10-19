# What the roadmap still holds, and what each part needs

State at `main` after the merge of 2026-10-19. "Built" means in the application and verified as
the acceptance records say; nothing below is declared done that is not.

## Built

F1 inventory · F2a Claude Code hooks and skills · F2b Codex skills and MCP servers · the ACL trust
rule · F3a/b/c native launcher, registry, profiles · F9 host that outlives the window ·
T1/T2 panes, search, keys, saved layout · F14 sealed API keys · F11a handoff brief ·
F13 install, upgrade, rollback, diagnose.

## Not built, and why

| Phase                        | What is missing                                                                           | What it needs                                                                                                                                                                                                                                                               |
| ---------------------------- | ----------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P1 baselines                 | Numbers against a reference terminal; budgets restated                                    | A reference terminal installed with the owner's consent (OQ-26, OQ-27 answered). Own-metric measurements are in `f3-2026-10-19.md`                                                                                                                                          |
| F1b / G-INF                  | Whether managed Inference can be isolated from the user's hooks, and Codex hooks switched | Experiments E1 to E8 on throwaway provider homes with a test credential path; an external security review (docs 10, 43 to 49 state the isolation constraints observed so far)                                                                                               |
| F4a OpenCode                 | Switches for OpenCode (the inventory is built, ADR 0041)                                  | The span editor extended to JSONC comments, then an adapter like F2b                                                                                                                                                                                                        |
| F4b Hermes                   | Hermes as an upstream                                                                     | Hermes is not installed; DN-13                                                                                                                                                                                                                                              |
| F5 authoring                 | Create, edit, delete hooks, skills, MCP servers and rule files; global profiles           | DN-12 (the contract for authoring executable content: a native dialog for the command) and the external security review the roadmap requires for F5 (T-02 is the highest risk of the configuration side)                                                                    |
| F6a/b models                 | Seeing the models in use; choosing                                                        | F6a for managed sessions needs DN-09 (a field in the authority's view). For native sessions the model of a profile is already passed to the agent (`--model`/`-m`)                                                                                                          |
| F8 multiple managed runtimes | Several managed turns at once                                                             | A redesign inside the authority (DN-08) with schema migration and per-generation attestation                                                                                                                                                                                |
| F10a/b remote                | A gateway, pairing, grants, a desktop and phone client                                    | DN-05's reachability decision (a private overlay network is a prerequisite the owner must provide, OQ-18), a threat review, and a network-facing service: **not started without separate authorisation to expose a service**. F9 (the host) is the prerequisite and is done |
| F11b a second managed agent  | Another agent under the authority                                                         | F8 first; an ADR per agent                                                                                                                                                                                                                                                  |
| F12 Wyss Agent               | A decision, not a build                                                                   | DN-06 on real installations                                                                                                                                                                                                                                                 |

## Next

Ordered by what can be done without a decision of the owner's: F6 for native sessions (models
seen in files and profiles), F4a once the JSONC editor choice is made, then F5 once DN-12 and
its review are accepted. Everything else waits on the item named in the last column.
