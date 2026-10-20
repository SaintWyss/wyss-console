# F8 adversarial acceptance specification

2026-10-20. Candidate ADR 0051; **specified, NOT_RUN**. These are future implementation tests,
not claims about the current global-runtime core. Ordinary unit fixtures are allowed; real
acceptance uses the owner's normal environment and requires a safe maintenance decision.

## Common oracle

Seed sessions A/B, distinct launch identities, leases, attachments and approval fingerprints.
Record per-worker outbound method counts, normalized canonical rows and ordered event envelopes.
For every refusal require zero unauthorized provider frames, unchanged unrelated rows and no
secret/prompt/action content in durable audit. Include malformed/unknown fields and identity-domain
confusion. Test both accepted inference and local-tools profiles without adding a model/effort API.

| ID    | Adversarial schedule                                                                            | Required result                                                                                         |
| ----- | ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| F8-01 | A lease used with B runtime/attachment/turn                                                     | Refuse before dispatch; B unchanged                                                                     |
| F8-02 | Identical provider request/item IDs on A and B                                                  | Correlation includes owning connection and launch; no cross-response                                    |
| F8-03 | A relaunches while old catalogue cursor, binding reply or approval answer arrives               | Stale identity refused; no generation-n data accepted as n+1                                            |
| F8-04 | A approved action has unknown outcome at crash                                                  | Indeterminate retained; no retry of effect through new generation                                       |
| F8-05 | Expire/revoke A's lease after admission but before write; B is active                           | A writes zero unauthorized frames; B remains admitted                                                   |
| F8-06 | Runtime loss then replay of old start/lease/approval intent                                     | Original receipt only; no duplicate launch or revived admission; fresh lease required                   |
| F8-07 | Parallel admission exactly at/over capacity, including discovery and stopping slots             | At most the reserved bound; fixed immediate refusal, no unbounded queue                                 |
| F8-08 | Simultaneous starts with different catalogues and failures during staging                       | B's live catalogue bytes/inode remain unchanged; private directories; symlink/hardlink attacks refused  |
| F8-09 | A event flood and blocked shutdown while B expires/revokes or answers approval                  | Fair bounded progress; numeric control-latency and aggregate-memory ceilings met                        |
| F8-10 | A exceeds byte/event/time budget while B waits for approval                                     | Only A cancelled/failed; B approval identity/deadline preserved                                         |
| F8-11 | Two starting/running turns for A; one pending approval each for A/B                             | Second A turn/approval refused; distinct A/B permitted by both SQL and core                             |
| F8-12 | v1 hello, v2 missing session, stale launch, unknown version; A/B changes between hello and call | Fixed version/schema refusal; never choose implicit current/GUI session                                 |
| F8-13 | Crash before/after generation reservation, spawn, handshake, readiness audit                    | No reused generation; no implicit restart; no fabricated ready state or abandoned owned child           |
| F8-14 | Generation counter at maximum; replay of prior receipts                                         | Refuse overflow without spawn; replay unchanged                                                         |
| F8-15 | Runtime A exits while B runs; A stop grace never completes                                      | B state/stream continues; cleanup cannot block sole writer                                              |
| F8-16 | Audit storage full at admission/readiness/approval outcome                                      | Fail closed and preserve unknown effects; cleanup still completes                                       |
| F8-17 | Child ignores EOF/TERM; unrelated same-path process and non-owned descendants exist             | Signals only through verified owned lifecycle; unrelated processes unchanged; containment gaps reported |
| F8-18 | Active attachment/workspace/profile replaced during dispatch                                    | Exact accepted workspace/profile revalidated; zero cross-profile writes; SG-12 unchanged                |

## Migration and rollback

For seeded V1–V7 stores, verify the supported path to V7 and candidate V8 in ordinary unit fixtures.
Inject failure after every DDL, copy, trigger/index change, metadata change and pre-commit validation.
Compare original schema, version, tables, receipts, event bytes, stream ID/head and vocabulary after
rollback. Test unknown future schema, extra/missing objects, corrupt records, FK violations and a
partially migrated database. Runtime history stays a separate legacy record, never assigned to A.

Verify per-session turn and approval indexes independently from in-memory routing. Preserve
immutable-identity/transition and append-only triggers; reject attempts to alter historical identity.
Historical rows must not acquire fabricated launch generations.

Create a SQLite-consistent backup with active WAL in a unit fixture, verify permissions and manifest,
then restore with the matching old binary/schema. Reject partial snapshot, mismatched binary,
missing WAL-dependent data, active writer lock and unknown schema without changing the original.
Document post-snapshot writes that rollback would lose. No production downgrade/restore is tested
against a live authority; installer rollback must not silently stop the host.

## Real provider/resource acceptance

Before starts at 1/2/4, prove exact-version shared-state compatibility and review owned containment.
Record original config checksums/metadata, app/host/authority/session identities, background load,
swap, aggregate process-tree memory and CPU, peak startup cost, latency, failures and cleanup.
Use bounded non-inference operations first; paid turns/auth experiments need explicit authorization.
If safe conditions cannot be met, retain NOT_RUN. Unit fakes cannot substitute for real-provider
isolation, token-refresh safety, quota behaviour or GUI acceptance.

## Candidate contract coverage, 2026-10-20

[Exact candidate artifacts](../contracts/f8/README.md) and `src-tauri/tests/f8_contracts.rs`
provide ten passing ordinary unit tests. This is a bounded implementation input, not F8 acceptance:

- F8-01/03/12: routing and request-delta reference oracles reject crossed session, stale authority,
  instance/generation, unknown fields and generic execution input. No production router/parser tested.
- F8-07/14: SQL lifetime-row limits and counter-reset refusal; finite numeric budget/reservation
  reference checks. No parallel admission, timing, process reap or resource enforcement tested.
- F8-11: real SQLite per-session active-turn/pending-approval indexes and crossed sidecar FKs;
  stale launch and orphan sidecar refusal. Core admission remains future work.
- Migration: every candidate SQL statement boundary rolls back schema/data/version; existing V7
  history bytes remain unassigned; an active lease blocks migration. Fixtures use current schema
  and synthetic records, not full canonical-record validation or all older migration paths.
- Receipt lookup under exhaustion proves preservation/readability only. Actual intent replay,
  scheduler fairness, budget failure, storage faults, crash windows, owned cleanup and consistent
  backup/restore require implementation tests. All installed-provider concurrency remains NOT_RUN.

The independent follow-up reviewed candidate design surfaces and found no new blockers after four
corrections; it did not execute tests or providers. Owner approval and implementation security review
are still required; no production security guarantee is changed.

## Inactive implementation evidence

[Code acceptance record](../acceptance/f8-code-2026-10-20.md): the initial seventeen additional tests, expanded to twenty-three by the correlation follow-up, exercise
real strict codecs, observational registry/replay cache and offline migration preparation. Migration
fixtures use canonical session receipts, runtime history and revoked leases where supported. Every
V0–V7 failure boundary asserts the hook is reached and source rows/schema/version unchanged. A valid
active lease passes canonical verification then refuses at the quiescence guard. This supersedes
reference-only coverage for those slices; installed V8, new-work replay, audit result
validation, scheduler, durable replay and provider concurrency remain unproven.
