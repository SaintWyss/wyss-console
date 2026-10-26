# M6 engineering implementation record — 2026-10-25

## PR #29 review corrections — 2026-10-26

This section records current correction evidence and supersedes the affected
test counts and traceability rows below.

| Finding                                                           | Result                                  | Evidence and limit                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ----------------------------------------------------------------- | --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P1 mandatory failure could dispatch a later independent step      | IMPLEMENTED, TESTED                     | `orchestration::scheduler::tests::mandatory_admission_refusal_cancels_independent_steps_before_dispatch` verifies refusal of the first required eligible step leaves `starts == 0`, cancels the second step, and fails the run.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| P2 paused run could be lost after idle exit                       | IMPLEMENTED, TESTED (simulated clock)   | `orchestration::scheduler::tests::paused_plan_keeps_daemon_alive_past_idle_and_resumes_on_reconnect`, `orchestration::scheduler::tests::paused_plan_expires_and_restart_recovery_does_not_resume_it`, and `orchestration::daemon::tests::paused_plan_survives_idle_exit_predicate_and_resumes_through_ipc`. The test advances injected time by 61 seconds and exercises the IPC handler on the same coordinator; a real daemon process/socket close and reconnect remains NOT_PROVEN. Expiry and restart fail-closed behavior remain explicit.                                                                                                                                                                                                                                                                                                                                                                                                                    |
| P2 handoff review accepted bytes that the generated plan rejected | IMPLEMENTED, TESTED                     | `orchestration::tests::handoff_sealing_uses_the_receiving_plan_byte_limits` accepts a 2,048-byte objective and exact 8,192-byte rendered prompt; it rejects 2,049 bytes and an over-limit 9 KiB excerpt with distinct errors.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Backend receiver freshness admission                              | PARTIAL, fail-closed                    | F8 reads the authority identity/stream/head, prepared Codex session and creation sequence, matching preparation audit receipt, and complete bounded contiguous audit history; prior turn events for that session are refused. A clean snapshot remains NOT_PROVEN as dispatch authority: F8 `managedSession/turn/start` only rejects an active turn, and session revision does not advance when a turn is admitted. No atomic, consumable fresh-only receipt exists. An otherwise clean receiver is returned as `receiver_freshness_unproven` and no handoff turn is dispatched; a receiver with prior durable work in the inspected history is `receiver_not_fresh`. Missing/gapped history, more than 2,000 events, or a changing stream/head fails closed as unproven. The protocol dependency is a one-time preparation capability, bound to authority/session/runtime identity and atomically consumed by `turn/start`, which also guarantees no prior turn. |
| Source freshness and replay                                       | IMPLEMENTED, TESTED with fixture limits | Managed source admission rechecks F8 session, attached workspace binding, runtime identity and generation. Native source identity/generation remains NOT_PROVEN at the daemon boundary because no `wyss-host` identity/receipt API is available there. `orchestration::scheduler::tests::handoff_replay_rejects_mutated_body_and_changed_confirmation_time` rejects changed content or confirmation time before idempotent replay.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

Focused verification on the corrected branch:

- `cargo test --locked --manifest-path src-tauri/Cargo.toml orchestration:: --lib` — **34 passed**.
- `npx vitest run tests/orchestration-view.test.ts` — **4 passed**.

Full corrected-branch verification:

- `npm run check` — PASS; Prettier, ESLint, Svelte/TypeScript, **67 Vitest
  files / 1,074 tests**, owned harness (**16 tests**), Claude bridge (**77
  tests**), and Vite build. The existing large-chunk advisory remains.
- `npm run check:rust` — PASS (exit 0); format, locked check, library
  (**1,474 passed, 29 ignored**), binary (**535 passed, 16 ignored**),
  authority/CLI/F8 integration tests, B1 fixture unit tests (**136 passed**),
  and Clippy with `-D warnings`.
- Direct B1 process run (the package filter selects zero process tests):
  `cargo test --locked --manifest-path src-tauri/Cargo.toml --features b1-fixture --test b1_process -- --test-threads=1`
  — **22 passed**.
- `npm run format:check` — PASS. `npm run scan:repo` — **5 passed, 0 failed**.

The 34-test focused Rust suite also exercises the requested adversarial cases:

- Crash recovery: `crash_reconciliation_marks_running_and_queued_work_for_review`
  (journal fixture; a real process crash remains NOT_PROVEN).
- Ambiguous acknowledgement: `ambiguous_start_is_journaled_and_never_retried`.
- Mutated handoff/replay: `handoff_replay_rejects_mutated_body_and_changed_confirmation_time`.
- Workspace write conflict: `reviewed_workspace_change_and_duplicate_workspace_claim_are_refused`.
- Dependency failure: `dependency_failure_blocks_descendants_and_halts_remaining_dispatch`.
- Cancellation: `cancellation_is_an_idempotent_durable_intent`.

These tests include fixture-backed coordinator behavior and a pure F8-history
evidence parser. They do not establish live F8 server/installed-provider
acceptance, a real process restart/reconnect, or same-UID owner authenticity.
TS-ORC-11 stays `BLOCKED_TRUST_BOUNDARY`; Native automated dispatch stays
blocked.

## Result

The M6 implementation slice is present on `feat/m6-structured-handoff-ol3`.
It adds a finite local OL3 coordinator, versioned structured handoff, a private
metadata-only SQLite journal, typed F8 Managed calls, and a supervised Pipelines
view. The implementation retains the accepted single-turn Managed limit of one.
This record is engineering and fixture evidence; it does not claim production
acceptance for capabilities whose existing security or authority gates remain
closed.

## Implemented

- Versioned plans and handoff envelopes bind revisions and content digests to the
  reviewed bytes. Handoff selection is explicit, workspace revisions are
  rechecked, Managed source generation/identity is checked through F8, and
  selected provider text is quoted as untrusted input. Native source
  identity/generation remains NOT_PROVEN at the daemon boundary.
- Plans are immutable bounded DAGs with finite step, text, attempt, and time
  limits. Dependency failure blocks descendants. Unsupported providers remain
  visible and cannot be dispatched.
- The durable local journal records closed metadata, digests, state transitions,
  attempts, cancellation intent, and workspace claims. It excludes prompts,
  handoff body text, provider output, credentials, and workspace paths. Plan and
  handoff records are immutable; duplicate submissions are idempotent.
- Scheduling uses the existing `wyss-control`/F8 authority protocol. The
  coordinator rechecks session, runtime, generation, profile, binding, lease,
  and the `turn_start` gate before an admitted action. Managed dispatch remains
  serial with `max_parallelism = 1`; a workspace claim prevents simultaneous
  writes to the same workspace.
- Pause, resume, stop, cancellation, timeout, expiry, and reconciliation have
  explicit states. A dispatch with an ambiguous acknowledgement or an
  unconfirmed cancellation becomes `NeedsReview`; the coordinator never
  automatically retries that effect. Startup reconciliation does not silently
  resume a prior active turn.
- The daemon is started on demand by the desktop request, with a private
  same-user socket and lock, bounded request timeouts, an idle exit, and no
  service installation. All effects still cross the existing F8 authority.
- The Pipelines UI supports plan review, exact digest confirmation, monitoring,
  workspace verification, reconcile decisions, pause/resume/stop, explicit
  handoff selection, new F8 receiver preparation, and manual copy for blocked
  Native destinations. Backend Managed receiver validation reads F8 evidence;
  dispatch stays blocked for an otherwise fresh receiver until F8 supplies the
  atomic one-time fresh-only `turn/start` contract described above.
- Native automated dispatch, Managed Claude, Hermes execution, and destinations
  that lack an admitted authority stay visibly blocked. Existing Native and
  Managed controls remain available.

## Test evidence

At the original M6 feature cut, the focused coordinator suite passed **23 tests**. The new UI suite passed
**4 tests**, covering new F8 receivers for Managed and Native sources, refusal
until the observed lease/binding/runtime are ready, a changed source revision
refused before sealing or submitting, and invalidation of a reviewed digest
after selected content changes. The UI work also found and fixed a visibility-effect
refresh loop; the monitor now tracks visibility without tracking its own loading
state.

At the initial M6 feature cut, repository checks passed:

- `npm run check` — Prettier, ESLint, Svelte/TypeScript, **67 Vitest files / 1,074
  tests**, owned harness (**16 tests**), Claude bridge (**77 tests**), and Vite
  production build. The build reports the existing large-chunk advisory
  (958 kB minified JavaScript; not a build failure).
- `npm run check:rust` — Rust format, locked check, library (**1,463 passed,
  29 ignored**), binary (**535 passed, 16 ignored**), authority/CLI/F8
  integration tests, B1 fixture unit tests (**136 passed**), and clippy.
- The package's B1 command filtered its process test target to zero cases, so
  the integration target was executed directly:
  `cargo test --locked --manifest-path src-tauri/Cargo.toml --features b1-fixture --test b1_process -- --test-threads=1`
  — **22 passed**.
- Focused M6 tests — coordinator **23 passed**, handoff UI **4 passed**.

Rust and UI unit/fixture runs establish local behavior only. No test here is
evidence that an installed provider consumed a handoff, that a real provider
completed a multi-step workflow, or that a production crash was injected.

| Traceability item | Status and available evidence                                                                                                                                                                                                                                                                                                   |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TS-ORC-01         | TESTED — bounded DAG validation rejects cycles, missing dependencies, unbounded input, and unsupported dispatch. Rust unit tests.                                                                                                                                                                                               |
| TS-ORC-02         | TESTED — plan digest binds prompt/dependency bytes and review mismatch is refused; journal immutability/idempotency tested. Rust unit tests.                                                                                                                                                                                    |
| TS-ORC-03         | TESTED — mandatory dependency failure blocks descendants; the corrected admission-refusal regression proves no later independent effectful step starts (`mandatory_admission_refusal_cancels_independent_steps_before_dispatch`). Fake authority fixture.                                                                       |
| TS-ORC-04         | BLOCKED — the accepted ADR 0012/0015 baseline permits one shared Managed turn. Cap-one behavior is fixture-tested; multi-runtime isolation awaits a separately reviewed and accepted concurrency ADR.                                                                                                                           |
| TS-ORC-05         | TESTED — malicious quoted text and path escape do not become a command or file capability. Rust unit tests; no Native automatic lane exists.                                                                                                                                                                                    |
| TS-ORC-06         | PARTIAL — durable Managed cancellation and unconfirmed cancellation are exercised with a fake authority. Live Native-process stop semantics are outside the admitted M6 dispatch boundary.                                                                                                                                      |
| TS-ORC-07         | TESTED (simulated) — durable dispatch marker, ambiguous acknowledgement, restart reconciliation, and no retry are tested in journal/coordinator fixtures. A real forced process crash was not injected.                                                                                                                         |
| TS-ORC-08         | NOT_PROVEN — F8 session/runtime/profile/generation/binding/lease/gate are rechecked. Global hook/provider-configuration drift required by DN-20 is not bound into the approval and needs an authority/config-version contract.                                                                                                  |
| TS-ORC-09         | PARTIAL — finite plan bounds, single-slot arbitration, expiry, timeout cancellation, and explicit unknown outcomes are unit-tested. Production provider quotas and every external resource ceiling are not proven.                                                                                                              |
| TS-ORC-10         | BLOCKED — no phone/watch client or remote plan authority is implemented.                                                                                                                                                                                                                                                        |
| TS-ORC-11         | BLOCKED_TRUST_BOUNDARY — same-UID callers can impersonate approval callers under the current host trust model. Internal digest confirmation is not proof of owner identity. A separately privileged, verifiable owner approval path and the required adversarial tests remain dependencies.                                     |
| TS-ORC-12         | PARTIAL — restart reconciliation fails closed; paused plans stay alive beyond the 60-second idle threshold and resume through the IPC handler when the same coordinator remains alive. A real daemon process/socket reconnect, logout, network-loss, and host-crash matrix remains NOT_PROVEN.                                  |
| TS-ORC-13         | PARTIAL — the journal test inspects persisted rows and proves prompt/objective/output bytes are absent. Notification, usage-history, and every production sink were not exercised.                                                                                                                                              |
| TS-ORC-14         | BLOCKED_MANAGED_CONCURRENCY — cap one is enforced and tested across distinct workspaces. Parallel reference-PC acceptance requires the same superseding accepted ADR as TS-ORC-04.                                                                                                                                              |
| TS-ORC-15         | BLOCKED — Hermes has no admitted direct-effect boundary or Wyss-enforced tool adapter; it cannot execute in M6.                                                                                                                                                                                                                 |
| TS-HAND-01        | PARTIAL — a UI fixture confirms only an owner-selected excerpt is sent and no turn-output read is requested; the handoff body remains volatile and the journal stores digest/metadata only. A real second-session byte-transfer test was not run.                                                                               |
| TS-HAND-02        | TESTED — Rust contract tests bind selected content, target, revision, and expiry; corrected boundary test accepts objective=2,048 bytes and prompt=8,192 bytes, rejects the next byte / oversized excerpt precisely; the UI suite invalidates changed reviewed content.                                                         |
| TS-HAND-03        | TESTED (contract) — injection-like text remains quoted and file references reject absolute paths, `..`, and backslashes. Provider-side handling is NOT_PROVEN because no provider acceptance run was made.                                                                                                                      |
| TS-HAND-04        | BLOCKED_PROTOCOL — backend receiver history is checked and prior turn history is rejected. A clean read snapshot cannot be atomically consumed by current F8 `turn/start`; otherwise-fresh Managed handoff dispatch remains blocked pending the one-time preparation capability. Native destinations remain manual-copy only.   |
| TS-HAND-05        | PARTIAL — changed workspace revision is refused; Managed source generation, binding, runtime identity and readiness are backend-checked. Native source identity/generation remains NOT_PROVEN without a daemon-accessible `wyss-host` evidence API. Real source death/detachment and volatile-draft crash loss remain untested. |

## Security findings and remaining gates

- **Implemented:** metadata-only journal; content digests and revision checks;
  strict bounded local IPC; F8 authority fencing; production serial dispatch;
  unique workspace ownership; no silent retry or silent recovery; Native and
  unadmitted provider lanes blocked.
- **BLOCKED:** same-UID owner/approval attribution (`TS-ORC-11`); Native
  automated-dispatch trust boundary; Hermes effect isolation (`TS-ORC-15`);
  parallel Managed dispatch (`TS-ORC-04/14`); remote privilege interface
  (`TS-ORC-10`). These gates were not weakened.
- **NOT_PROVEN:** DN-20 global hook/provider-configuration drift, actual
  installed-provider consumption, full real crash/network/relaunch recovery,
  complete production journal-sink exclusion, a real daemon reconnect, Native
  source identity/generation at the daemon boundary, and atomic F8
  fresh-receiver consumption. The last item currently blocks all Managed
  handoff dispatch.
- No credentials, private prompts, provider output, or installation state were
  read for this implementation. No application or service was installed,
  restarted, or migrated.

## Merge readiness

Do not merge until an independent review is complete. Production acceptance also
requires resolving the explicit blockers above; passing fixture suites alone is
not sufficient.
