# Arsenal: per-session skill switch qualification, Codex CLI 0.159.0 (2026-10-31)

Build: `codex-cli 0.159.0` (standalone package `0.159.0-x86_64-unknown-linux-musl`), real account
configuration (`~/.codex/config.toml` with 93 `[[skills.config]]` entries), shared app-server daemon
running (0.166.0). Evidence (rollouts, probe outputs) is kept outside the repository in
`~/.local/state/wyss-console/arsenal-trial/codex-qualification/`.

## Mechanism

`codex --no-daemon -c 'skills.config=[{path = "<…/SKILL.md>", enabled = false}, …]'`

- `--no-daemon` runs the session on its own embedded app server, "even if [the shared one] is
  already running" (`codex --help`). The configuration override then reaches only that process.
- Each excluded skill is named by the absolute path of its `SKILL.md`, through the folder it is
  installed in; Wyss serializes the value with a TOML serializer.

## Findings that shape the implementation

| #   | Probe                                                                                                               | Result                                                                                                                      |
| --- | ------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| F1  | `-c skills.config=[{path=<SKILL.md>, enabled=false}]` (`codex debug prompt-input`, no inference)                    | the skill leaves the model-visible list                                                                                     |
| F2  | same with the skill **folder** path                                                                                 | silently ignored: the skill stays → Wyss only emits `SKILL.md` paths and refuses unknown or missing ones                    |
| F3  | `-c` with one entry, against the 93 account entries                                                                 | entries are **added**: every skill the account switches off stays off, only the new one leaves                              |
| F4  | `-c … enabled=true` for a skill the account switches off                                                            | it **reappears** → Wyss never emits `enabled = true`                                                                        |
| F5  | linked GSD skill by link path and by resolved path                                                                  | both switch it off                                                                                                          |
| F6  | Codex system skills (`~/.codex/skills/.system`)                                                                     | left as they are: never named (also refused by the host)                                                                    |
| F7  | first end-to-end start through Wyss: `gstack-upgrade` (a `SKILL.md` inside `~/.codex/skills/gstack/`) stayed listed | Codex lists nested `SKILL.md` files as skills of their own → Wyss now names the nested files of every skill it switches off |

## Cases

| Case                                          | How                                                              | Result                                                                                                                    |
| --------------------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| 1 No selection                                | prompt-input, no override                                        | 93 skills listed; account switches honoured                                                                               |
| 3/4 One or several excluded                   | prompt-input (F1) and two interactive sessions (2 excluded each) | only the named skills leave                                                                                               |
| 5 User skills (`~/.agents/skills`)            | interactive sessions A/B                                         | `frontend-design` off in A only; `gsap-react`, `accessibility` off in B only                                              |
| 6 Project skills (`<project>/.agents/skills`) | probe skill `qproj`                                              | off in A, listed in B                                                                                                     |
| 7 Shared/linked (GSD Core)                    | F5                                                               | switched off by either path                                                                                               |
| 8 System skills                               | F6 + unit test                                                   | untouched                                                                                                                 |
| 9 Invalid/missing paths                       | F2 + unit tests (`validate_files`, `apply_codex`)                | refused before launch                                                                                                     |
| 10 Stale configuration / version              | unit tests                                                       | plan bound to `codex-cli 0.159.0` and its configuration hash; any other build is `NOT APPLIED`                            |
| 12 Two simultaneous terminals                 | sessions A and B started 3 s apart in two Wyss host terminals    | each session's model-visible list differs from the base by exactly its own two skills (rollout `12-20-32` and `12-20-35`) |
| 13 Shared daemon                              | `ss -xp` on both session processes                               | no connection to the daemon sockets in `/tmp/codex-daemon-1000/`; only internal socket pairs                              |
| 14 Positive/negative exposure                 | same prompt in A and B                                           | A: `useGSAP`, `QPROJ-ABSENT`; B: `GSAP-SKILL-ABSENT`, `QPROJ-LISTED`                                                      |

Cases 2 (one selected skill through Arsenal) and 11 (restart) are exercised end to end through the
installed Wyss Console after deployment.

## Limits

- Plugin skills (e.g. `pdf`, `documents`) and system skills are not switched off.
- A switched-off skill is not listed to the model; its files stay readable on disk.
- The session does not join the shared Codex server: it does not appear in `codex agents`.
- Whether the provider used a kept skill is not observed by Wyss.
- A file is checked when the plan is built, again by the host just before the start (inside a Codex
  skill folder, a regular file, not a link), but Codex reads it a moment later. If it disappears in
  between, Codex ignores that entry; the skill it named is gone with it, so nothing extra is exposed.
- A nested `SKILL.md` that is a link, a folder that cannot be read, or a folder past the walk's
  bounds make the inventory incomplete, and an incomplete inventory is never applied.
