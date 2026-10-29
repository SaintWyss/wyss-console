# Wyss Arsenal — A0 pilot report (AP-0 + AP-CL, A-1 only)

Run `20261029-pilot-e98a8e` · 2026-10-29 · evidence `~/.local/state/wyss-a0/20261029-pilot-e98a8e/evidence/` (0700).
Repository untouched: `main` `e9210d90f14ec826cd8cc6f2ad20148a30281867`, clean.
No allowlist change, no production session, no real inference, no provider installation change, no Codex launch.

**Pilot result: INCONCLUSIVE.**

- A-1/H: PASS.
- A-1/I: the auto-computed label was FAIL. The skill-exposure oracle passed. Two PASS conditions were missed because of harness defects; see §3.

**A0_PILOT_REVIEW_REQUIRED**

## 1. AP-0 — exception and isolation preflight

- Exception recorded only in external evidence: `evidence/A0-EXCEPTION-AP0.md`. Repository policy is unchanged.
- Preflight run 1: **FAIL**, kept in `evidence/preflight-run1-FAILED/`. Inside the sandbox, `/proc/1/environ` exposed 3 806 bytes of the launcher's environment. bwrap's `--clearenv` clears only the child; PID 1 (bwrap init) kept the host environment. This is the same defect M3 recorded. The probe stored only the length, never the values.
- Fix:
  - bwrap is now launched with an empty environment (`env={}`);
  - the sandbox root is remounted read-only (`--remount-ro /`) — in run 1, `/etc` was writable only because it sat on the sandbox's private root tmpfs; host `/etc/a0-probe` does not exist;
  - `PWD` is accepted only with the exact value `/work`.
- Preflight run 2: **31/31 PASS** in the sandbox, `evidence/preflight/`:
  - Disposable HOME and XDG; `/home` contains only `probe`; the real HOME is absent and not mounted.
  - `/tmp` is a private tmpfs, empty; no `/tmp/codex-daemon-1000`.
  - No real credential paths. Env names are exactly the allow-list; no sensitive names.
  - No inherited file descriptors besides 0/1/2.
  - No D-Bus, display or SSH variables or paths; the X11 abstract socket returns ECONNREFUSED.
  - Network: only `lo`, 0 routes; 1.1.1.1:443, 192.168.100.5:22 and 192.168.100.1:53 give ENETUNREACH; DNS fails.
  - PID namespace holds 2 processes; host provider PIDs are invisible; `/proc/1/environ` is empty.
  - `/usr`, `/etc`, `/` and the harness are read-only, and so is the provider binary (sha256 `4f4178ce…d01d` verified). `/work` and `/markers` are writable. `NoNewPrivs` = 1.
- Negative control, the same probe on the host: 19 checks FAIL, so the probe distinguishes an isolated environment from one that is not.
- Real-system integrity before/after: PASS.

## 2. A-1/H — `claude -p ping` (Claude Code 2.1.312)

Command: `evidence/A1-H/command.txt`. bwrap exit 0, CLI exit 0, stdout `ok from mock`, stderr empty.

| Check                                   | Result                                      |
| --------------------------------------- | ------------------------------------------- |
| Mock requests                           | 1 × `POST /v1/messages`, synthetic key only |
| DESC canaries α β γ proj                | present                                     |
| Negative 1 (`disable-model-invocation`) | absent                                      |
| Negative 2 (outside the skills root)    | absent                                      |
| BODY canaries (listing ≠ loading)       | all absent                                  |
| `@AGENTS.md` project instructions       | present                                     |
| Proxy attempts                          | none                                        |
| Disposable `settings.json`              | byte-identical                              |
| Real-system integrity                   | PASS                                        |
| Leftover processes                      | none                                        |
| Cleanup                                 | done                                        |

**Verdict: PASS.**

## 3. A-1/I — interactive TUI in a PTY

| Check                        | Result                                                                                                       |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Mock requests                | 1 × `POST /v1/messages`, synthetic key                                                                       |
| Canaries                     | exactly as expected: positives present, negatives absent, no BODY, AGENTS present                            |
| Real-system integrity        | PASS                                                                                                         |
| Disposable `settings.json`   | unchanged                                                                                                    |
| Leftovers / cleanup          | none / done                                                                                                  |
| `ok from mock` on screen     | **not matched**                                                                                              |
| Orderly exit after 2× Ctrl-C | **no**: SIGTERM after 10 s (exit status 143)                                                                 |
| Proxy attempts               | `CONNECT downloads.claude.ai:443`, `CONNECT github.com:443` — refused by the in-sandbox logger; no route out |

Diagnosis, from the stored screen with no new run:

- **Missing `ok from mock`**: the TUI rendered `●okfrommock`, painting spaces with cursor moves. The harness's ANSI stripper removed them. This is a harness defect; the response was shown.
- **No orderly exit**: the screen shows "Press Ctrl-C again to exit" twice. The harness sent the second Ctrl-C 1.5 s after the first, outside the provider's confirmation window. This is a harness defect; the provider behaved as designed.

Classification against the pre-registered definitions: no FAIL condition was triggered (canaries, credentials, integrity), and two PASS conditions could not be established because of harness defects → **INCONCLUSIVE** (the script printed FAIL). The fix is to normalise the screen without dropping spaces and to send the two Ctrl-C within ≤ 0.5 s. A re-run needs a new owner approval.

## 4. Provider findings (Claude Code 2.1.312)

1. **Skill listing location**: the skill list sits in the first user message (a system-reminder), not in `system` or `tools`. The oracle and the AM1 verification must read `messages`.
2. **Provider built-ins**: besides fixture skills, 13 built-in skills are listed (`dataviz`, `update-config`, `keybindings-help`, `code-review`, `simplify`, `fewer-permission-prompts`, `loop`, `claude-api`, `workflow-authoring`, `run`, `plugin-authoring`, `init`, `security-review`), plus built-in agents. An "exact selected set" in AM1 means selection ∪ built-ins, unless `skillOverrides` also hides built-ins (A-8, not approved).
3. **Default permission mode changed**: 2.1.312 shows "Auto mode is now Claude Code's default permission mode" and runs in auto mode when no `--permission-mode` is given. The Native launcher's mapping (`Ask` → `manual`) and the owner's real settings must be rechecked under group B before any requalification.
4. **Interactive-only network attempts**: `downloads.claude.ai` and `github.com`, despite `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and `DISABLE_AUTOUPDATER=1`. Headless `-p` made none.
5. **Interactive-only writes**: `~/.local/share/applications/claude-code-url-handler.desktop`, `history.jsonl` and `~/.claude.json` backups. In the owner's real environment these are provider-owned side effects of any interactive launch (already happening today).
6. A built-in plugin `agents-md@builtin` wrote store state (`plugins/store/cc-plugin-agents-md_builtin-…json`). The owner's real settings disable it. Whether the AGENTS canary came via `CLAUDE.md @AGENTS.md` or via this plugin is not separated by A-1.
7. With no `--model`, the request used `claude-opus-5-5`; the banner showed "API Usage Billing". Nothing could be billed: no route exists out of the namespace.

## 5. Implications for AM1

- **Proven**: the offline method (bwrap + mock + canaries) observes exactly what Claude 2.1.312 offers the model, without credentials or network, in both headless and interactive launches. It is viable as the requalification lane and as a possible pre-launch visibility probe.
- **Still unproven (requires A-2/A-8/A-6/C, not approved)**: exclusion via `skillOverrides` on 2.1.312, plugin exclusion, hiding built-ins, hook preservation under an overlay.
- **New AM1 requirements**:
  - verification reads the system-reminder in `messages`;
  - the UI separates provider built-ins from the selection;
  - the permission-mode default is checked per build (finding 3);
  - interactive network attempts are documented as provider behaviour.
- **Harness hardening to keep**: empty bwrap environment, `--remount-ro /`, PID-1 environ check. The repository's own `claude-bridge/probe/sandbox.py` is not changed here; whether it has the same launcher-environment exposure was not checked.

## 6. Final state

- Real config hashes unchanged from the first snapshot to the final one. `~/.claude.json` changed because the owner's live Claude session (this one) writes it; the sandbox never mounted it.
- No new or lost provider PIDs; no leftover sandbox process; no `bwrap` alive.
- `homes/` and `work/` are empty. Evidence: 316 KiB, kept.
- Next steps, all waiting for owner approval:
  - fix the two harness defects and re-run A-1/I;
  - then A-2 and A-8 (`skillOverrides` on 2.1.312) and C-1..C-3 (hooks).
