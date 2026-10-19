# F2b (Codex mutations): prepared, and the decisions left

Nothing in F2b is implemented or authorised. Each item is a decision with a recommended
default and what it costs; none is taken here. Source: roadmap 19 (F2b), doc 22.

| #   | Decision                                                                                                   | Options                                                                                                                                        | Recommended                                                                                                                             | Trade-off                                                                                                                                                  |
| --- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | **Start F2b at all** (roadmap gate: "a phase starts when the owner says so")                               | Now; after real use of F2a; not now                                                                                                            | After he has done the F2a manual checks (acceptance report), because F2b reuses the writer, the journal and the dialog                  | Starting earlier risks building a second writer on an unverified first one                                                                                 |
| 2   | **DN-19: individual Codex hooks before G-INF**                                                             | Offer "turn off" only; offer neither; offer both with a dialog naming the loss of managed Inference                                            | **Neither**: show them and say why. The roadmap says turning one on stays blocked, and turning one off has an unknown storage (point 3) | "Turn off only" is useful but is a one-way switch the Console cannot undo; "both" contradicts the isolation contract                                       |
| 3   | **OQ-01: where Codex keeps a hook switched off in its `/hooks` browser**                                   | He answers by an experiment in a throwaway `CODEX_HOME`; or the Console never writes a Codex hook switch                                       | Run the experiment (disable one hook in Codex's own browser, diff the files). It needs his Codex, interactive                           | Without it F2b can only do skills and MCP switches, which is still the useful part                                                                         |
| 4   | **OQ-03: does a running Codex apply a config change**                                                      | Experiment with an open session; or always say "not confirmed"                                                                                 | Say "not confirmed" and skip the experiment                                                                                             | The honest text costs nothing; the experiment costs an afternoon and a Codex session                                                                       |
| 5   | **Which Codex switches**: skills (`[[skills.config]] enabled`), MCP (`[mcp_servers.<n>] enabled`), plugins | Skills and MCP; plus plugins; plus model keys                                                                                                  | Skills and MCP only (the two the roadmap names)                                                                                         | Plugins have no documented switch semantics here                                                                                                           |
| 6   | **TOML editing**                                                                                           | `toml_edit` (format-preserving) as a direct dependency; a byte-span editor like the JSON one                                                   | A byte-span editor for the touched key, verified against the parsed value as F2a does                                                   | `toml_edit` is already in the lock but rewrites more than the key; the span editor keeps comments and layout (AC-F2b-03) at the cost of a new TOML scanner |
| 7   | **`requirements.toml` and managed hooks**                                                                  | Never write; show only                                                                                                                         | Never write (system sources are `forbidden`, 11 §4)                                                                                     | None; it is the existing rule                                                                                                                              |
| 8   | **Managed-effect prediction** (`managed_effect`)                                                           | Implement the prediction from the same fixtures as the authority's `zero_external_hooks`; or only say "may make managed Inference unavailable" | The fixture-based prediction, since the prediction is the point of F2b                                                                  | It couples the desktop to the authority's rules (tests must hold both)                                                                                     |
| 9   | **Trust records**                                                                                          | Never touch                                                                                                                                    | Never touch (roadmap exclusion)                                                                                                         | None                                                                                                                                                       |
| 10  | **Projects**: write a project's `.codex/config.toml`                                                       | Only user scope; user and project                                                                                                              | User scope only first                                                                                                                   | Project scope needs the trusted-project logic in the write path                                                                                            |

What is **not** a decision, because the owner already made it or the code already enforces
it: the storage policy (ADR 0029), the native dialog for risky plans, byte-identical
restoration, and the rule that no real provider file is changed without his explicit word.

## Prepared (2026-10-19), not wired and not authorised to run

The owner's proposed defaults were adopted for the preparation: user-scope skill and MCP
switches only; no individual Codex hook mutation; byte-preserving TOML editing; predictions
labelled as predictions; isolated fixtures and a complete inverse. What exists:

- `tomlspan.rs`: a TOML scanner that places statements by byte; it refuses layouts it cannot
  place (an inline-table definition, say).
- `codex_plan.rs`: plans `SkillOff/On` and `McpOff/On` as an edit of one `enabled` key,
  checked against the same edit on the parsed value; every comment and blank line survives
  (tested on a corpus); the inverse edit restores the file byte for byte; names go through
  redaction.
- The managed-effect **prediction** (`Facts`, `Verdict`, `Prediction`), equal to the
  authority's rule: `managed_inference_blocker` (a read-only wrapper in the library over
  `zero_external_hooks` and `mcp_names`) is compared with the prediction on an exhaustive
  grid in the desktop's tests (TS-CFG-15), including which cause is named.
- Against the installed Codex 0.159.0 in a temporary `CODEX_HOME`: `codex mcp list` shows the
  switch the planner wrote and the one the inverse restores (ignored test
  `real_codex_reads_the_mcp_switch_the_planner_wrote`).
- No command, capability, `Operation` or UI exists for it, and no real Codex file is read for
  a plan. Turning F2b on is wiring.

## F2b was then implemented (2026-10-19)

The owner changed the strategy (develop in the real environment, finish F2b) and adopted the
proposed defaults, which settles decisions 1, 4, 6 and 7 above. Resolved here, as engineering
choices: the inverse of a TOML change is a closed `prior` word in the journal (option a of
decision 2); MCP and skills both shipped, skills labelled unverified (decision 3); the
prediction is shown with its cause and never blocks (decision 5); project scope stays out.
Demonstrated on the real `~/.codex/config.toml` with an exact backup, one line added,
`codex mcp list` showing the server disabled, and the undo byte-identical (same SHA-256 and
mode), through the service and through the GUI with the native confirmation. See the
acceptance report. The list below is what the code could not settle.

## The decisions that remain, exactly

1. **Start F2b** (19: "a phase starts when the owner says so"). Recommended: yes, after the
   hand checks of the acceptance report.
2. **Where the exact inverse of a TOML change lives.** The planner returns it; F2a keeps no
   content in the journal. Options: (a) the journal records a closed inverse (`remove the
line` / `restore the word true|false`), no key names, no values beyond a boolean; (b) a
   sealed record as for hooks; (c) undo is semantic (set the opposite) and exact only when the
   file is as the change left it and the key was added by us. Recommended: (a), because the
   inverse of these edits holds nothing but a boolean.
3. **Skills of Codex.** `[[skills.config]] enabled` is what the files show and what the
   planner writes; the effect on a running Codex was not verified (no command lists skills
   headlessly). Options: verify by hand on a copy; or ship MCP switches first. Recommended:
   MCP first (verified), skills after the owner has seen Codex follow the key.
4. **Individual Codex hooks:** none (his default; confirmed here). They stay display-only until
   OQ-01 and G-INF; nothing in F2b writes `[hooks.state]` or `hooks.json`.
5. **Whether to show the prediction when it says "unavailable before and after".** The cause
   is a hook the Console will not change. Recommended: show it, labelled "predicted", with the
   cause named, and never block an MCP or skill switch because of it.
6. **OQ-03** (does a running Codex apply a change): recommended "not confirmed" text, as the
   defaults say; an experiment only if he wants the sentence stronger.
7. **Project scope:** user scope only, as proposed; the trust logic for a project's
   `.codex/config.toml` waits for its own decision.

## What actually remains after F2b

1. **`~/.claude` is `0775` with an ACL for the sandboxed `agent` account — resolved without
   touching it.** The rule of 07 §4.2 stays: a folder another account can write is read-only.
   What changed is how "can write" is judged: the ACL is read, a group whose only member is
   the owner does not count, and an account the owner NAMES in
   `~/.local/state/wyss-console/configuration/trusted-writers` is accepted as a co-writer (the
   page says so). World-writable, any other ACL user and any shared group are still refused.
   Shown on the real `~/.claude`: one skill switched off, the installed Claude Code said
   "disabled via skillOverrides", the undo restored the exact bytes, and the directory's ACL
   and the file's mode did not change. To go back to the strict rule, delete that file.
2. **Codex skills**: the effect of `[[skills.config]] enabled` on a running Codex is
   unverified; the plan says so.
3. **Codex hooks** stay display-only until OQ-01 and G-INF.
4. **The external review for F2b** (roadmap 4): not run; the writer and recovery it relies on
   passed it in F2a, the TOML scanner and planner have not been reviewed by anyone but their
   tests.
