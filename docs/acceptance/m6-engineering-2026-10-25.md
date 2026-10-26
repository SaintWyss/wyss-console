# M6 engineering implementation record — 2026-10-25

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
  reviewed bytes. Handoff selection is explicit, source and workspace revisions
  are rechecked, and selected provider text is quoted as untrusted input.
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
  Native destinations. Managed handoff requires a fresh, empty receiver whose
  F8 binding, runtime, profile, lease, and gate are observed ready.
- Native automated dispatch, Managed Claude, Hermes execution, and destinations
  that lack an admitted authority stay visibly blocked. Existing Native and
  Managed controls remain available.

## Test evidence

The focused coordinator suite passed **23 tests**. The new UI suite passed
**4 tests**, covering new F8 receivers for Managed and Native sources, refusal
until the observed lease/binding/runtime are ready, a changed source revision
refused before sealing or submitting, and invalidation of a reviewed digest
after selected content changes. The UI work also found and fixed a visibility-effect
refresh loop; the monitor now tracks visibility without tracking its own loading
state.

Final repository checks passed:

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

| Traceability item | Status and available evidence                                                                                                                                                                                                                                                               |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TS-ORC-01         | TESTED — bounded DAG validation rejects cycles, missing dependencies, unbounded input, and unsupported dispatch. Rust unit tests.                                                                                                                                                           |
| TS-ORC-02         | TESTED — plan digest binds prompt/dependency bytes and review mismatch is refused; journal immutability/idempotency tested. Rust unit tests.                                                                                                                                                |
| TS-ORC-03         | TESTED — mandatory dependency failure blocks descendants and stops further dispatch. Fake F8 authority unit test.                                                                                                                                                                           |
| TS-ORC-04         | BLOCKED — the accepted ADR 0012/0015 baseline permits one shared Managed turn. Cap-one behavior is fixture-tested; multi-runtime isolation awaits a separately reviewed and accepted concurrency ADR.                                                                                       |
| TS-ORC-05         | TESTED — malicious quoted text and path escape do not become a command or file capability. Rust unit tests; no Native automatic lane exists.                                                                                                                                                |
| TS-ORC-06         | PARTIAL — durable Managed cancellation and unconfirmed cancellation are exercised with a fake authority. Live Native-process stop semantics are outside the admitted M6 dispatch boundary.                                                                                                  |
| TS-ORC-07         | TESTED (simulated) — durable dispatch marker, ambiguous acknowledgement, restart reconciliation, and no retry are tested in journal/coordinator fixtures. A real forced process crash was not injected.                                                                                     |
| TS-ORC-08         | NOT_PROVEN — F8 session/runtime/profile/generation/binding/lease/gate are rechecked. Global hook/provider-configuration drift required by DN-20 is not bound into the approval and needs an authority/config-version contract.                                                              |
| TS-ORC-09         | PARTIAL — finite plan bounds, single-slot arbitration, expiry, timeout cancellation, and explicit unknown outcomes are unit-tested. Production provider quotas and every external resource ceiling are not proven.                                                                          |
| TS-ORC-10         | BLOCKED — no phone/watch client or remote plan authority is implemented.                                                                                                                                                                                                                    |
| TS-ORC-11         | BLOCKED_TRUST_BOUNDARY — same-UID callers can impersonate approval callers under the current host trust model. Internal digest confirmation is not proof of owner identity. A separately privileged, verifiable owner approval path and the required adversarial tests remain dependencies. |
| TS-ORC-12         | PARTIAL — restart reconciliation fails closed to review and does not resume a dead turn in unit tests. A full close/relaunch, logout, network-loss, and real-host-crash matrix remains NOT_PROVEN.                                                                                          |
| TS-ORC-13         | PARTIAL — the journal test inspects persisted rows and proves prompt/objective/output bytes are absent. Notification, usage-history, and every production sink were not exercised.                                                                                                          |
| TS-ORC-14         | BLOCKED_MANAGED_CONCURRENCY — cap one is enforced and tested across distinct workspaces. Parallel reference-PC acceptance requires the same superseding accepted ADR as TS-ORC-04.                                                                                                          |
| TS-ORC-15         | BLOCKED — Hermes has no admitted direct-effect boundary or Wyss-enforced tool adapter; it cannot execute in M6.                                                                                                                                                                             |
| TS-HAND-01        | PARTIAL — a UI fixture confirms only an owner-selected excerpt is sent and no turn-output read is requested; the handoff body remains volatile and the journal stores digest/metadata only. A real second-session byte-transfer test was not run.                                           |
| TS-HAND-02        | TESTED — Rust contract tests bind selected content, target, revision, and expiry; the UI suite confirms an edited excerpt clears the reviewed preview and prevents submission.                                                                                                              |
| TS-HAND-03        | TESTED (contract) — injection-like text remains quoted and file references reject absolute paths, `..`, and backslashes. Provider-side handling is NOT_PROVEN because no provider acceptance run was made.                                                                                  |
| TS-HAND-04        | PARTIAL — UI fixtures cover Managed→fresh Managed and Native→fresh Managed, and confirm the receiver is empty and F8-ready. Native destinations remain manual-copy only; provider authority-loss acceptance is NOT_PROVEN.                                                                  |
| TS-HAND-05        | TESTED (fixture) — a changed Managed source revision is rejected before seal/submit; handoff digest binds workspace revisions. Real source death/detachment and volatile-draft loss across an app crash were not exercised.                                                                 |

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
  and complete production journal-sink exclusion.
- No credentials, private prompts, provider output, or installation state were
  read for this implementation. No application or service was installed,
  restarted, or migrated.

## Merge readiness

Do not merge until an independent review is complete. Production acceptance also
requires resolving the explicit blockers above; passing fixture suites alone is
not sufficient.
