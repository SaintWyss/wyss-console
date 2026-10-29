# Wyss Arsenal — A0 A-1 interactive retest

Run `20261029-pilot-e98a8e` (same harness root) · 2026-10-29.
Evidence: `~/.local/state/wyss-a0/20261029-pilot-e98a8e/evidence/` — `retest/`, `preflight/`, `A1-I-retest/`, `A1-I-RETEST-EXPECTATIONS.md`.
Repository untouched: `main` `e9210d90f14ec826cd8cc6f2ad20148a30281867`, clean.

**Result: A1_INTERACTIVE_PASS** (exactly one provider launch).

## 1. Harness fixes (validated before any launch)

| Defect (pilot)             | Root cause                                                                                                                                            | Fix                                                                                                                                                                                                                                | Offline validation                                                                                                                                                                                                                             |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ok from mock` not matched | The ANSI _stripper_ deleted cursor moves. The TUI places words with `CSI n G` (`\x1b[3Gok\x1b[6Gfrom\x1b[11Gmock`), so spaces vanished (`okfrommock`) | `harness/screen.py`: a cell-grid screen model (CUP/CUU/CUD/CUF/CUB/CHA/VPA/EL/ED, save/restore, CR/LF/BS/TAB, scrollback) plus a history of displayed rows, so text later erased is still auditable                                | `retest/test_screen.out`: **16/16**. 12 synthetic vectors with negatives; on the pilot recording the old method gives `okfrommock`, the new one `ok from mock`, "Press Ctrl-C again to exit" and the auto-mode banner                          |
| No orderly exit            | The second Ctrl-C was sent 1.5 s after the first, outside the provider's confirmation window ("Press Ctrl-C again to exit" appeared twice)            | `harness/pty_driver.py`: Ctrl-C → wait ≤ 3 s for the confirmation to be visible now → immediate second Ctrl-C → wait ≤ 10 s; ≤ 2 rounds, then SIGTERM/SIGKILL reported as **forced**; every step audited with monotonic timestamps | `retest/test_exit_driver.out`: **8/8**. Against `fake_tui.py` (raw mode, 0.8 s window, cursor-positioned prompt): 5/5 clean, latency 0.05 s; pilot timing does **not** exit (control); a TUI that never confirms ends forced, bounded at 6.1 s |

Prior versions are kept as `retest/a1_inside.pilot.py` and `retest/run_a1.pilot.py`. The retest
verdict also requires: no proxy destination outside the pilot baseline, and complete cleanup.

## 2. Isolation recheck (before launch)

Preflight re-run (`evidence/preflight/`):

- **31/31** sandbox checks PASS: disposable HOME/XDG; real HOME absent and not mounted; private empty tmpfs `/tmp`; no `/tmp/codex-daemon-1000`; no real credential paths; allow-listed env names only; no inherited FDs; no bus/display; only `lo` and 0 routes (1.1.1.1, LAN host and gateway → ENETUNREACH, DNS fails); 2 PIDs; `/proc/1/environ` empty; read-only `/`, `/usr`, `/etc`, harness and binary (sha256 `4f4178ce…d01d`); `NoNewPrivs` = 1.
- The host negative control fails 19 checks, as before.
- Real-system integrity before/after: PASS.

The run 2 directory from the pilot is kept as `preflight-run2-PASS/`.

## 3. The single interactive run — Claude Code 2.1.312

Command: `evidence/A1-I-retest/command.txt` (bwrap, empty launcher env, `--unshare-all`, `--remount-ro /`).

Driver audit (`markers/driver-audit.json`):

|  t (s) | Event                                        |
| -----: | -------------------------------------------- |
|  0.001 | spawned                                      |
| 12.045 | typed `ping`                                 |
| 13.058 | Enter                                        |
| 13.593 | request seen; response visible               |
| 15.601 | first Ctrl-C                                 |
| 15.655 | confirmation visible (0.054 s)               |
| 15.655 | second Ctrl-C (latency 0.054 s)              |
| 15.936 | child exited with code 0 — clean, not forced |

| Pre-registered condition                                                                 | Result                                                                                                                     |
| ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Isolation preflight                                                                      | 31/31 PASS                                                                                                                 |
| Mock traffic                                                                             | 1 × `POST /v1/messages`, synthetic key only                                                                                |
| DESC α β γ proj                                                                          | present                                                                                                                    |
| Negatives `a0-hidden` (`disable-model-invocation`), `a0-ghost` (outside the skills root) | absent                                                                                                                     |
| BODY canaries                                                                            | all absent: listing ≠ loading                                                                                              |
| `@AGENTS.md` instructions                                                                | present                                                                                                                    |
| `ok from mock` on the reconstructed screen                                               | yes                                                                                                                        |
| Clean exit                                                                               | code 0, not forced                                                                                                         |
| Proxy attempts                                                                           | `CONNECT downloads.claude.ai:443`, `CONNECT github.com:443` = baseline; unexpected: none. Refused in-sandbox; no route out |
| Real-system integrity                                                                    | PASS (before/after the run)                                                                                                |
| Leftover sandbox processes                                                               | none (`pgrep -c bwrap` = 0)                                                                                                |
| Disposable `settings.json`                                                               | byte-identical                                                                                                             |
| Cleanup                                                                                  | disposable home/work removed; evidence kept (512 KiB total)                                                                |

## 4. Recorded observations (provider, 2.1.312, interactive)

- **Built-in skills (13)** listed beside the fixtures: `dataviz`, `update-config`, `keybindings-help`, `code-review`, `simplify`, `fewer-permission-prompts`, `loop`, `claude-api`, `workflow-authoring`, `run`, `plugin-authoring`, `init`, `security-review`.
- **Skill listing location**: first user message (system-reminder), not `system`/`tools`.
- **Permission mode at startup**: banner "Auto mode is now Claude Code's default permission mode" and status "auto mode on". No `--permission-mode` was passed.
- **Banner** shows "API Usage Billing" with the synthetic key; default model `claude-opus-5-5`. Nothing could be billed: no route out of the namespace.
- **Outbound attempts**: only the two baseline destinations above.
- **Disposable-HOME writes**:
  - `.claude.json`
  - `.claude/backups/.claude.json.backup.*`
  - `.claude/history.jsonl`
  - `.claude/projects/-work/<session>.jsonl`
  - `.local/share/applications/claude-code-url-handler.desktop`
- **Host invariants**:
  - First snapshot of the day vs final: all hashed configs, credential stat entries, Codex daemon state and binaries unchanged; provider PIDs = {224568} (this session) before and after.
  - `~/.claude.json` differs across the day because this live Claude session writes it. Within each run window it was unchanged, and the sandbox never mounted it.

## 5. Remaining blockers before AM1

1. **Exclusion is still unproven on 2.1.312.** `skillOverrides` (A-2/A-3/A-5), the exact selected set (A-8), plugin exclusion (A-6), and hooks/permissions under an overlay (C-1..C-5) are not approved and not run.
2. **The 13 built-in skills** are always listed. Whether they can be hidden is unknown (A-8). Until then AM1's "exact set" = selection ∪ built-ins.
3. **Default permission mode is auto on 2.1.312.** The launcher's `Ask → manual` mapping and B-3 must be checked before any requalification of `capabilities.rs` (still 2.1.309 only; D1).
4. **Interactive network attempts** to `downloads.claude.ai` and `github.com` happen despite the disable variables. This must be documented as provider behaviour, and AM1 must not claim "no network".
5. **Codex is untested.** AP-CX and AP-CX-F are not approved; D4/D5 remain.
6. **AP-0 exception** still lives only in external evidence; the repository policy text still forbids disposable HOMEs (D3).
7. **Harness** is outside the repository and unreviewed by another model. Adopting it (`scripts/research/a0/`) needs a reviewed PR.
