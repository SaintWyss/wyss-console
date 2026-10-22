# What the roadmap still holds, and what each part needs

> **Historical record.** For current status use the [gap register](../product/implementation-status.md)
> and [remaining roadmap](../product/roadmap.md). F8 was subsequently activated in production
> with real Codex, P2/V8, ADR0051 Accepted and cap1; P1 remains. The report below retains
> its original observations and limitations; its procedures are not new authorization.

State at `main` after the merge of 2026-10-19. "Built" means in the application and verified as
the acceptance records say; nothing below is declared done that is not.

## Built

F1 inventory · F2a Claude Code hooks and skills · F2b Codex skills and MCP servers · the ACL trust
rule · F3a/b/c native launcher, registry, profiles · F9 host that outlives the window ·
T1/T2 panes, search, keys, saved layout · F14 sealed API keys · F11a handoff brief ·
F13 install, upgrade, rollback, diagnose · F4a OpenCode (inventory and switches) · F5 authoring of
hooks and OpenCode and Codex servers · F6a models (managed observed, native three facts) · F1b research
(recommendation I4, not proven).

## Not built, and why

| Phase                        | What is missing                                                                                                                                        | What it needs                                                                                                                                                                                                                                     |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P1 baselines                 | Canonical repetitions, startup/input/render/throughput/panes/reconnect measurements and budgets                                                        | Ghostty 1.3.1 is already installed. Two preliminary real-desktop observations, including a concurrent reference, are recorded in `p1-2026-10-19.md`; no installation consent is needed. Remaining measurement/instrumentation work is engineering |
| F1b / G-INF                  | The parts of E3, E5, E6 that need a credential, and the implementation of managed Inference isolation (recommendation I4, `docs/research/…`)           | A credential path the owner chooses for E3 (a real refresh), E5 (the authority's preflight on an isolated start) and E6 (one turn, one resume); a decision on old thread bindings (E7); then an ADR and a security review                         |
| F4a OpenCode                 | Nothing planned: inventory, switches and exact undo are built (ADR 0041). Project-level OpenCode files are read, not written                           | —                                                                                                                                                                                                                                                 |
| F4b Hermes                   | Hermes as an upstream                                                                                                                                  | Hermes is not installed; DN-13; the owner's OK to install it                                                                                                                                                                                      |
| F5 authoring                 | Codex hooks (blocked until G-INF); project-level authoring, hook/MCP templates (skill/rule creation and deletion and Codex skills are built, ADR 0047) | Claude Code hooks, OpenCode and Codex MCP servers are done (ADR 0042, 0044). Existing user skill/rule editing and sealed instruction templates are built (ADR 0045/0046, native GUI acceptance). Codex hooks need G-INF                           |
| F6a/b models                 | A managed model **choice** (a catalogue view, `model/rerouted` and native effort are built, ADR 0048)                                                  | A separate authority contract and review (SG-12), and a classifier change reviewed on its own (ADR 0043). Observation, and the three facts of a native session, are done                                                                          |
| F8 multiple managed runtimes | Several managed turns at once                                                                                                                          | A redesign inside the authority (DN-08) with schema migration and per-generation attestation: not decided by the owner                                                                                                                            |
| F10a/b remote                | A gateway, pairing, grants, a desktop and phone client                                                                                                 | DN-05's reachability decision (a private overlay network the owner provides, OQ-18), a threat review, and a network-facing service: **not started without separate authorisation to expose a service**                                            |
| F11b a second managed agent  | Another agent under the authority                                                                                                                      | F8 first; an ADR per agent                                                                                                                                                                                                                        |
| F12 Wyss Agent               | A decision, not a build                                                                                                                                | DN-06 on real installations                                                                                                                                                                                                                       |

## Next

Santiago authorised completion of the remaining eligible roadmap on 2026-10-19.
Start with F5 skills, rules and templates, then F6 native catalogue/selection and P1 own
metrics. F10 local gateway development and F8 foundations may proceed without exposing a
listener or replacing the approved authority contract. Network exposure/device enrolment,
Ghostty/Hermes installation and paid credential use still need specific owner consent.
All real acceptance uses the normal owner environment (see `../CHECKPOINT.md`).
