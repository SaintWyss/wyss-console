# What the owner has to decide before the next phase

Written 2026-10-19 after F2b. Nothing here is approved: each item is a decision for the owner,
with the recommendation of the architecture documents. Finished work (F1, F2a, F2b) is not
reopened.

## Why there is no phase to start without the owner

Every phase left on the roadmap (19 §3) has a gate that is not mine to pass:

| Phase                      | Gate                                                         | What it asks                                                          |
| -------------------------- | ------------------------------------------------------------ | --------------------------------------------------------------------- |
| F1b research               | the owner's authorisation to run experiments E1 to E8        | Throwaway homes and a test credential path, each described beforehand |
| P1 performance baseline    | the reference terminal installed, with consent; OQ-26, OQ-27 | Installing software; a measuring method                               |
| F3a launcher               | an ADR extending 0005 to 0007 (DN-02); OQ-07                 | How an agent is started in a terminal                                 |
| G-INF                      | DN-03, after F1b                                             | Whether managed Inference is isolated from the user's hooks           |
| F4a OpenCode, F5 authoring | F4a has none beyond F1; F5 needs DN-12                       | See 22                                                                |

F1b and P1 are independent of everything built so far and can run in parallel; F3a is the next
step on the owner's central goal (visual control of agents started from the Console).

## Decisions, in the order they unblock work

1. **DN-02 — how the Console starts an agent (unblocks F3a).** Options: through the user's
   shell with a replacing command; directly; typing into a shell. Recommended: the first,
   with a direct start as a labelled fallback (05 §4.3). To close OQ-07 first, a fixture (a
   fake agent printing its arguments and environment) is run over `sh`, `bash`, `zsh`, `fish`;
   that is test code only and needs no approval. Needed from the owner: "ADR 0030 as
   recommended", or another option.
2. **Authorise F1b (unblocks DN-03 and G-INF).** The experiments E1 to E8 run on throwaway
   homes, not on `~/.claude` or `~/.codex`. This is the exception to "no temporary HOME for
   ordinary development": it is research, and it is named in the roadmap. Needed: yes or no.
3. **P1 consent and OQ-26 / OQ-27.** Install the reference terminal (which one is the owner's
   choice; nothing is installed without his word), and decide whether a start-up timestamp may
   exist in packaged builds (recommended: no, measure a special build and say so) and how
   input latency is measured externally (recommended: instrumented number first, a camera
   later). Needed: the terminal, and the two answers.
4. **Standing arrangements from this stretch** (nothing to decide, only to know):
   `~/.local/state/wyss-console/configuration/trusted-writers` names `agent`, so Claude Code
   changes work with the ACL as it is; delete the file to return to the strict rule. Codex
   skill switches are verified through Codex's `skills/list`; what a running Codex session does
   with them stays labelled unverified.
