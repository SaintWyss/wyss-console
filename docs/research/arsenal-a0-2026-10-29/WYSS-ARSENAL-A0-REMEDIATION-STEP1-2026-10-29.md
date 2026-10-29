# Wyss Arsenal A0 — offline remediation, step 1 (builder's report)

2026-10-29 · run root `~/.local/state/wyss-a0/20261029-pilot-e98a8e/` · baseline: the independent review (NO-GO for full AM1) and `WYSS-ARSENAL-A0-ERRATA-2026-10-29.md`.

Written by the **builder** of this remediation. It is not an independent assessment.

No provider was executed: v2 refuses provider cases without explicit permission, and the Claude binary is not even mounted in v2 cases. There was no inference, no real credential, and no change to the repository (`main` `e9210d9`, clean), global configuration, services, real hooks or active sessions. R3 is untouched. No dependency was installed.

**Status: A0_OFFLINE_REMEDIATION_READY_FOR_REVIEW**

## 1. What was preserved

- `harness/` and `evidence/` were not modified. 500/500 files match the pre-remediation tree (`remediation/backup/pre-remediation-tree.sha256`), and no new file appeared in them.
- Frozen documents and the v1 package manifest: the 98/98 entries still match. The v1 manifest is kept as `remediation/backup/v1-manifest-98.txt`.
- Backup tar of the frozen harness and evidence: `remediation/backup/harness-v1-frozen.tar`, sha256 `e826e1fb…d623`.
- All v2 work lives in `harness-v2/` (copy-then-modify) and `remediation/`.

## 2. Defect matrix

| #   | Defect (review/errata)                                           | Fix (v2)                                                                                                                                                                                                                                                                                                    | Offline proof                                                                                                                                                                                           | Negative control                                                                                              |
| --- | ---------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| 1   | Manifest said 103, covered 98, no primary captures               | `remediation/make_manifest_v2.py` → `MANIFEST-v2.json` (1 356 files + 8 docs, categories, `in_v1_manifest`); per-case `case-manifest.json` written host-side with the v2 harness tree hash                                                                                                                  | v1 rechecked 98/98; 57 historical primary captures now hashed **post hoc** (labelled as such)                                                                                                           | —                                                                                                             |
| 2   | `validator_matches` ignored                                      | Part of the verdict; a mismatch is FAIL                                                                                                                                                                                                                                                                     | oracle `validator_mismatch_fails`; e2e `e2e_validator_mismatch_fails`                                                                                                                                   | —                                                                                                             |
| 3   | Cases could pass without both positive and negative conditions   | `oracle.lint`: requests range plus ≥ 1 positive and ≥ 1 negative expectation; otherwise INCONCLUSIVE                                                                                                                                                                                                        | `lint_missing_negative_inconclusive`, `lint_missing_requests_inconclusive`                                                                                                                              | v1 table linted: 22/24 not gradable (`missing_requests_range` 22, no negative 10, no positive 9)              |
| 4   | Expected calls not proven attempted                              | `tool_attempt`: a follow-up request with a `tool_result` for the scripted id, plus text and `is_error`                                                                                                                                                                                                      | `tool_attempt_proven_passes`, `tool_not_attempted_fails`, `tool_attempt_wrong_outcome_fails`; e2e both directions                                                                                       | —                                                                                                             |
| 5   | Vacuous truths (synthetic keys with 0 requests)                  | Checks without data are UNKNOWN; a refusal case needs exact text + 0 requests                                                                                                                                                                                                                               | `zero_requests_never_pass`, `zero_request_case_keys_not_counted_as_pass`, `refusal_text_required`                                                                                                       | —                                                                                                             |
| 6   | Validator raised TypeError; accepted `{}`                        | Strict UTF-8 JSON, no BOM/dupes/NaN, single key, non-empty map, name alphabet = Rust's, exact names, string states; never raises (`internal_error` fallback)                                                                                                                                                | 35 table cases + 20 000-input fuzz: 0 raises, 0 accepts outside the grammar                                                                                                                             | v1 raises `TypeError` on a list value; v1 accepts `{}`                                                        |
| 7   | Listing parser excluded `.`                                      | Same alphabet as `profile::skill_ok`                                                                                                                                                                                                                                                                        | `listing_parser_accepts_dot_names`                                                                                                                                                                      | —                                                                                                             |
| 8   | PTY: incomplete ANSI/UTF-8 across reads                          | Incremental parser with pending tail                                                                                                                                                                                                                                                                        | Bytewise + 200 random chunkings of both real recordings reproduce the whole-feed screen                                                                                                                 | v1 differs under bytewise feed (both recordings); split CSI/UTF-8 → garbage in v1                             |
| 9   | `visible()` matched scrollback                                   | `visible_text()` is the grid only; `seen_text()` is for audit                                                                                                                                                                                                                                               | `scrolled_text_not_visible`; driver `visible_now_excludes_scrollback`                                                                                                                                   | v1 driver reports scrolled-off text as visible                                                                |
| 10  | No drain before exit                                             | `drain()` until EOF/EIO (bounded) after reap                                                                                                                                                                                                                                                                | `drain_captures_tail_before_exit` (467 KB burst)                                                                                                                                                        | v1 missed the tail in 5/5 runs                                                                                |
| 11  | Teardown not guaranteed                                          | Idempotent `close()` (group SIGTERM→SIGKILL while alive, reap, drain, close fd), context manager; `finally` in the in-sandbox runner and host launcher                                                                                                                                                      | `close_on_exception_kills_reaps_closes`, `no_confirm_forced_bounded` (6.1 s), e2e `e2e_injected_failure_cleans_up`                                                                                      | pilot timing does not exit (fake fidelity)                                                                    |
| 12  | Terminal ops ignored                                             | Alt screen, DECSTBM, IL/DL, ICH/DCH, ECH, SU/SD, IND/RI/NEL, deferred autowrap, OSC with ST; `unknown` counter                                                                                                                                                                                              | 10 vectors                                                                                                                                                                                              | —                                                                                                             |
| 13  | Declared 2 GiB cap not implemented; leftover detection by name   | `limits.py` + `launch.py`. Kernel: sized tmpfs for HOME 256 MiB, /work 128 MiB, /tmp 256 MiB, **/markers 16 MiB**; rlimits FSIZE 64 MiB, CORE 0, NOFILE 4096, CPU 600 s. Host: ROOT_CAP 2 GiB refusal before launch; stream byte cap 48 MiB; timeout kill; PID-namespace membership from `--json-status-fd` | e2e: EFBIG on a 64 MiB file, ENOSPC at 32 MiB in a 64 MiB HOME, /markers flood ≤ 16.0 MiB, stream flood killed at 47.9 MiB, timeout kill 8.7 s, root cap refusal, orphan counted (4 members) and 0 left | e2e run 1 **found** that a polled watchdog overshot 16 MiB to 265 MiB → replaced by a kernel cap (run 1 kept) |
| 14  | `/markers` writable by the system under test; no binding digests | No writable host bind at all (preflight check `v2_no_writable_host_disk_bind`). Evidence leaves only through an HMAC-chained stream (per-case key on stdin, never in env/argv); requests digested live; files exported as authenticated chunks; the host writes them                                        | `e2e_disk_tamper_fails`, `e2e_forged_record_detected_or_blocked` (forgery via `/proc/<harness>/fd/1` succeeded at write level and was **detected**: `bad_record_at_seq:3`); `missing_END_inconclusive`  | —                                                                                                             |
| 15  | Isolation regression risk                                        | v2 preflight through the same launcher/argv as v2 cases                                                                                                                                                                                                                                                     | 34/34 (31 original, none removed or relaxed, + sized tmpfs, rlimits, no writable host bind); host control fails 19 as before; bwrap flags: none removed, `--json-status-fd` and `--size` added          | —                                                                                                             |
| 16  | Accidental provider launch                                       | `run_case(..., allow_provider=False)` refuses any case without a fake `sut_argv` or naming `/opt/a0/bin/claude`; the binary is not mounted                                                                                                                                                                  | `e2e_provider_guard_refuses`                                                                                                                                                                            | —                                                                                                             |

## 3. Final offline results (all against the final harness, `remediation/final-run/`)

| Suite                   | Result                                               |
| ----------------------- | ---------------------------------------------------- |
| `test_screen_v2.py`     | 30/30 (5 negative controls)                          |
| `test_driver_v2.py`     | 14/14 (3 negative controls)                          |
| `test_validator_v2.py`  | 39/39 (2 negative controls)                          |
| `test_oracle_v2.py`     | 24/24 (includes v1 lint and historical re-judgement) |
| `run_preflight.py` (v2) | 34/34, host control 19 failures, integrity PASS      |
| `test_e2e_offline.py`   | 19/19 (1 negative control)                           |

Earlier failing runs are kept, not overwritten:

- `remediation/e2e-evidence-run1-FOUND-OVERSHOOT` — a real defect, fixed by design change;
- `e2e-evidence-run2-ASSERTION-BUG` — my test asserted "≤ 16 files" instead of bytes; corrected;
- `test-results/*.run1/run2.*`;
- the lint mapping bug caught by `test_oracle_v2` (3 failures) before the fix.

**Historical re-judgement:** v2 marks all 24 historical isolation captures `evidence_incomplete` (no authenticated stream). Their earlier verdicts remain _reviewed observations_; they are not certifiable retroactively.

## 4. Host invariants and cleanup

- **Per run:** 54/54 before/after integrity pairs show no change during any sandbox execution. No `bwrap` is left (`pgrep -c bwrap` = 0) and `cases-v2/` is empty.
- **Day start vs now:** hashed configs, binaries and Codex daemon state are unchanged. Two differences, neither attributable to the harness:
  - `~/.claude/.credentials.json` was atomically replaced at 17:18:01 (new inode, same size; only `stat` was read). It happened between runs (0 changes inside run windows), and no v2 code mounts or opens it. Likely cause: a token refresh by the owner's live Claude session — **ASSUMED**, not verified.
  - A new provider PID 258871 is an interactive `codex` started 16:47 from the owner's `fish` shell (the independent review session). The harness never signals PIDs outside its own namespaces.
- **Disk:** the run root uses 534 MiB, below the 2 GiB cap.

## 5. Rollback

1. Nothing outside `~/.local/state/wyss-a0/20261029-pilot-e98a8e/` was changed by this step.
2. To discard the remediation, delete `harness-v2/`, `remediation/` and `cases-v2/` (empty). `harness/` and `evidence/` are untouched (verified 500/500).
3. If the frozen trees were ever damaged:
   ```
   sha256sum -c remediation/backup/harness-v1-frozen.tar.sha256
   tar -xf remediation/backup/harness-v1-frozen.tar
   sha256sum -c remediation/backup/pre-remediation-tree.sha256
   ```
   Keep a copy of `remediation/backup/` outside `remediation/` before step 2, if desired.

## 6. Still pending (builder's list, not a review)

1. **No v2 campaign case table.** The A-2/A-8/C cases must be re-specified to pass `oracle.lint` before any provider rerun. Not written in this step, by scope.
2. **Hook markers are written by the system under test** (hooks run as it). They are exported authenticated, but their content is only as trustworthy as the SUT. Corroborate with streamed request evidence (tool_result text), as the oracle does for C-3.
3. **Stream key secrecy depends on Yama `ptrace_scope` ≥ 1** (observed 1). There is no preflight check for it yet.
4. **The screen model is still a subset of VT** (no IRM/DECOM etc.). The `unknown` counter is not yet wired into the oracle.
5. **No memory or task limits.** RLIMIT_AS is avoided (JS runtimes reserve large VA) and there is no cgroup delegation. The tmpfs mounts themselves consume RAM, up to ~656 MiB per sandbox.
6. **`kill_namespace` has a tiny TOCTOU window** between membership check and signal.
7. **A forged stream line makes the case FAIL** (detected, not prevented). An adversarial SUT can deny service but not fake a PASS.
8. **The fuzz rarely generates valid documents** (2 accepted). The accept path is covered by the table only.
9. **The orphan test proves membership by count** (4 members) rather than asserting the `sleep` process identity.
10. **`src-tauri/claude-bridge/probe/` still launches bwrap with an inherited environment.** It is in the repository and not changed (out of scope).
11. **No independent review** of harness-v2 yet.

## 7. Readiness for a new independent review

Ready to be reviewed. The reviewer has:

- `MANIFEST-v2.json` (sha256 `76fc4e96…d5fda0`, file `.sha256` alongside);
- `harness-v2/`, `remediation/tests/`, `remediation/final-run/`;
- the archived failing runs.

**Not** ready to repeat provider tests until: the v2 case table exists and passes lint, the reviewer accepts the v2 harness, and the owner authorizes the next campaign.
