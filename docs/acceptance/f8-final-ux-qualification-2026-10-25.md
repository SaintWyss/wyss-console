# F8 final UX fixes and release qualification — 2026-10-25

**Verdict: PASS for the changes in this PR; the release candidate stays PARTIAL on what this PR cannot prove** (V9 for the installed package, a real lost response, the IME defect with a physical keyboard). No high-severity blocker is open.

|                              |                                                                                                                                                       |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Base                         | `main` `0e9a5341a9df4e6169436a1d52320ce8548c031c` (unchanged since the earlier work)                                                                  |
| Preserved, in order          | `01b4f40` unsupported-mode message → `aab42c3` lease confirmation wait → `c808034` its report (linear history; `01b4f40` is an ancestor of `aab42c3`) |
| This PR adds                 | `9ccb220` capacity, unknown-outcome text, `--help`, authority replay regression; this report                                                          |
| Not merged                   | `diag/lease-hang-trace-2026-10-25` and its descendant `diag/final-ux-trace-2026-10-25` (instrumentation, lab only)                                    |
| Package built from `9ccb220` | manifest `source_revision` = `9ccb22046626170a68312109af07a170b891c6d5`; digests below                                                                |

Package digests: `bin/wyss-console` `f967b0d16d01562ba12ca1162ab8088ec0d595af0e387557015ad396590add0f`, `wyss-control` `9086ecbc68bac0a4935f4778b0aa7bbca59694a2838457f6d25661df656d6df5`, `wyss-credential-agent` `a2dbf26c34e24c198bf3a537ff2450294dadb2a58bb6a9eac42ea04915192c6d`, `wyss-screen` `6838ab54b03f5ea5cd5ff3dc2f868433bb33fabb3fc124566349b1409c11620b`. No fixture marker in any of them; the production install, units and launcher were not touched.

## What changed

1. **Confirmation stays native and modal.** A grant with an acting scope and every renewal say "Waiting for your answer in the confirmation window. Nothing is allowed until you choose Allow. If you cannot see it, it may be behind another window." No asynchronous or non-modal authorization was introduced; the window, its Allow gating and its 5-minute bound are untouched.
2. **The page's bound on an answer** (90 s; 5 min 30 s when the user must allow) never says the action was canceled. Its failure is `unavailable`: "The authority did not answer, so the outcome is not known. Nothing was canceled and it may still have happened. The console reads the authority again; check what it holds before sending this again." The authority is read at once; nothing is sent again by itself; retry sends the same identity; a late answer only clears the failure and is read.
3. **Capacity feedback.** A runtime start refused because another session holds the runtime (the authority's limit of one, unchanged) is the new failure `capacity`: "The authority is at its limit for this and did not take it. Nothing changed. Only one runtime runs at a time: stop the one that is running, then start this one." No Retry is offered. Any other capacity refusal (for example an overloaded core queue) stays `busy` and retryable, because only the desktop service can see another live runtime. No admission limit changed.
4. **`wyss-console --help` / `-h`** prints usage and exits first thing in `main`, before activation, the host, GTK or any authority is touched.

## Review of the timeout behaviour

| Concern                                    | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A timed-out grant is not shown as canceled | `failureText('unavailable')` test; the page state is `failed` + retry, never success                                                                                                                                                                                                                                                                                                                                           |
| Idempotent replay                          | the retry runs the closure that holds the original ids (controller test: identical arguments); authority test `a_grant_whose_answer_was_lost_is_replayed_as_it_was_and_never_revived_or_repeated`: replay while active, after expiry, after a restart and after a revocation returns the original receipt, adds no event and creates no lease; `grant_retry_normalizes…` already covered the same-intent/other-method conflict |
| Late response                              | success clears the failure and reads; failure keeps it; a second click while pending sends nothing (controller tests)                                                                                                                                                                                                                                                                                                          |
| Lease expiration                           | enforced by the authority and the gate (`expiry_denies_at_deadline…`); on the real stack the page noticed a 60 s lease expiring by itself at 60 s                                                                                                                                                                                                                                                                              |
| Authority restart                          | a retry after the authority was replaced ends in `stale`/`not_found` → "read again", nothing more is asked (controller test); live: offline state offers no action, then "The authority restarted or was replaced… nothing from before is acted on" and the runtime shows Stopped (`trace-authority-restart-under-live-gui.txt`)                                                                                               |
| Reconciliation after unknown outcomes      | every outcome ends in a read of the authority (`execute` and `sendDecision`); asserted in the tests                                                                                                                                                                                                                                                                                                                            |

## Qualification

| Check                                                                                                                                         | Result                                                                                                                                                                                                                                                                     |
| --------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `npm run check` (format, eslint, svelte-check/tsc, Vitest, harness, Claude bridge, build)                                                     | PASS — Vitest **1070/1070** (1057 before the earlier fixes)                                                                                                                                                                                                                |
| `npm run check:rust` (rustfmt, `cargo check --locked`, `rust:test`, `rust:test:b1`, clippy `--all-targets --features b1-fixture -D warnings`) | PASS — **2134 passed, 0 failed**, 46 ignored (`#[ignore]`d by the suite), no warning                                                                                                                                                                                       |
| Security regression                                                                                                                           | `scan:repo` 5/5; `check:package-fixtures` clean; `authority_boundary`, `f8_contracts`, `f8-capability` and the B1 sandbox suite (inside the two checks above) pass; no new command, permission or capability in the shipped code                                           |
| New Rust tests                                                                                                                                | capacity end to end (second runtime refused with `capacity`, no retry, nothing admitted, frees when the first stops); capacity mapping stays `busy` otherwise; lost-grant replay; `--help` process test (empty environment, private home that must stay empty, bound 10 s) |
| New Vitest tests                                                                                                                              | confirming step; lost/late answers; stale retry; capacity; texts; view banners                                                                                                                                                                                             |

### Packaged production binaries (no lab hooks) on the real KDE Wayland session

- `wyss-console --help` and `-h`: exit 0, usage on stdout, stderr empty, no window, no process left, empty home.
- Packaged authority + packaged GUI with a private home and bus: both stayed up 14 s, the GUI spawned its WebKit processes, authority log 0 bytes, no panic/critical in the GUI log, a clean stop leaves no journal and `integrity_check` ok. **The packaged GUI was not clicked** (no safe way to target its window without OS input).

### Production authority (`wyss-control` from the package, real Codex 0.159.0 Ready, no credential) with the GUI built from the same source plus lab hooks

| Scenario                                                                                                                                                         | Result                                                                       |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Allow: banner names the confirmation window; window FOCUSED; page answers pings in 16–22 ms meanwhile; three clicks → one `f8_lease_grant`; lease active         | PASS                                                                         |
| Lease of 60 s expires and the page notices by itself at 60 s                                                                                                     | PASS                                                                         |
| Second session, runtime start while the first is Ready                                                                                                           | PASS — the `capacity` message, only "Dismiss"                                |
| Decline: "It was not allowed in the confirmation window, so nothing was sent.", only Dismiss, Grant available again; the store holds 0 leases and 0 lease events | PASS                                                                         |
| Authority stopped and started under a live GUI                                                                                                                   | PASS (above)                                                                 |
| Unsupported Inference message (earlier fix)                                                                                                                      | PASS in the previous report; the code is unchanged, not re-run in this round |

### Synthetic versus production

- **Production binaries:** `--help`, authority + GUI start and stop, store integrity.
- **Production authority, lab-hooked GUI:** the lease, capacity, decline and restart rows.
- **Synthetic fixtures (not Codex):** bind, turns, approvals, stop under a turn (previous report), the Rust capacity test (fixture runtime) and all Vitest tests.
- **V9: NOT_PROVEN for the installed package.** The package writes a V8 image (`PRAGMA user_version` is 8 in every store of this session). V9 code and its unit tests exist in the tree and are not offered as validation of the package.

## Remaining

- IME duplication on a physical keyboard: not retested.
- A real lost IPC response: unit-tested only.
- The confirmation window stays modal by decision (needs its own security review to change).

Lab method and incidents are in the earlier report (`m1-m5-lease-confirmation-wait-2026-10-25.md`); in this round no application other than the lab's own processes was targeted, no OS input was sent, and every lab process was ended by recorded PID or by its private home.
