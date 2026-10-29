# Wyss Arsenal A0 — errata after the independent review (2026-10-29)

Source: independent review (Codex, GPT-6 Luna High, read-only), verdict **NO-GO for full AM1**,
`INDEPENDENT_REVIEW_COMPLETE`. The builder re-checked each item below against the files (one pass).
The frozen documents are left unchanged so their hashes stay valid; this file records the corrections.

| #   | Wrong statement (builder)                                                        | Where                         | Correct statement                                                                                                                                                                                                                   | Check                                                     |
| --- | -------------------------------------------------------------------------------- | ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| 1   | The package freezes "103 artifacts"                                              | review package; chat          | **98** hashed entries; the other lines are headers. Primary captures (`markers/requests.jsonl`, `screen.raw`, specs, markers) are **not** in the manifest                                                                           | `grep -cE '^[0-9a-f]{64}  '` = 98; `markers/` entries = 0 |
| 2   | Validator "29/29"                                                                | isolation report §2; chat     | **27** evaluations (22 case overlays + 5 extra vectors) + 2 cases without an overlay                                                                                                                                                | `validator-offline.out`                                   |
| 3   | `r3-gate.py`, `block-destructive.py` "are registered in user settings" `[OBS]`   | design doc line 58            | **Not registered.** `~/.claude/settings.json` registers other handlers. `~/.codex/hooks.json` states "R3 desactivado por pedido del usuario". The design must preserve the controls that are actually active and must not assume R3 | `hooks.json:2`                                            |
| 4   | "In the sandbox, auto mode still enforced allow/deny rules, ran blocking hooks…" | isolation report §3 point 8   | C-1..C-3 ran headless (`-p`). Their requests carry **no** `permission_mode` field; only the interactive A8-S1-I1 shows `"permission_mode": "auto"`. Hook/deny preservation is shown for **headless, unspecified mode** only         | `grep permission_mode` per case                           |
| 5   | Validator prototype rejects every malformed input                                | isolation report §2           | A list/object value raises `TypeError` (unhashable) instead of a clean rejection                                                                                                                                                    | `overlay_validator.py:38`                                 |
| 6   | Oracle verdicts are complete gates                                               | `run_batch.py`                | `validator_matches` is computed but **not** part of the verdict; A2-03/C-3 do not require the attempted call or the refusal message; synthetic-key check is vacuous with 0 requests                                                 | `run_batch.py:147-174`                                    |
| 7   | The repository probe benefits from the harness fixes                             | isolation report §1 (implied) | `src-tauri/claude-bridge/probe/sandbox.py:59` / `driver.py:28` still launch bwrap with an inherited environment                                                                                                                     | read-only check                                           |

Also accepted from the review:

- The PTY emulator is valid for the recorded case only (fragmented sequences, "visible now" vs scrollback, drain-before-exit).
- Cleanup and leftover detection are path-based. There is no `finally` recovery and no 2 GiB cap.
- `/markers` is writable by the system under test.
- A-8 shows consecutive fresh sessions, not concurrency.
- The plan's repetition and mode thresholds were not met.
