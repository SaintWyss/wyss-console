# ADR 0055 — A store that can hold a Managed runtime that is not Codex

Status: **Proposed; not accepted** · Date: 2026-10-23 (revised after review) · Owner: Santiago

Nothing here migrates, advertises or enables V9. The migration below exists only as a **rehearsal on
synthetic in-memory images** (`src-tauri/src/f8/v9_candidate.rs`, compiled into test builds only, run by
`src-tauri/src/f8/claude/v9_rehearsal_tests.rs`). No installed store was opened, copied or modified.

## Context: a demonstrated blocker, and a bigger one than first written

M3 built a Claude Managed worker that plugs in at the engine's `Worker` seam. The production authority
cannot admit it. The first version of this ADR named two CHECKs; the review found the list incomplete, and
the rehearsal below was built from the complete one.

| Where                                                                            | What is Codex-only                                                                                                                                                                  |
| -------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `managed_runtimes` (V8: `docs/contracts/f8/v7-to-v8.sql`)                        | `CHECK(adapter_id='codex')`, `CHECK(runtime_kind='codex')`                                                                                                                          |
| `managed_runtime_launches` (V8)                                                  | the same two CHECKs                                                                                                                                                                 |
| `managed_session_bindings` (`control/schema.rs`, a V3 object still in V7 and V8) | the same two CHECKs. **Missing from the first version**: without it a Claude conversation cannot be bound (`binding_confirmed`)                                                     |
| `authority_metadata`                                                             | `CHECK(schema_version=8)`                                                                                                                                                           |
| Replay validation of the event log                                               | `control/store.rs:789-790`, `control/bindings.rs:620-621`, `f8/audit.rs:324` refuse any `adapter_id`/`runtime_kind` other than `codex`                                              |
| Writers                                                                          | literals in `control/bindings.rs:306,322`, `control/store.rs:529,539`, `control/runtime.rs:320,381`, `control/server.rs:819`, `f8/allocator.rs:300-389`, `f8/operations.rs:401,883` |
| The handshake and the wire                                                       | `RuntimeSafetyProfile` is `CodexInferenceV1`/`CodexLocalToolsV1`; `complete_handshake` compares those exact strings; `runtime/start` names only Codex                               |
| The V8 validators                                                                | `f8/v8.rs::validate_v8` and `migration::expected_v8` pin the exact V8 schema text; `Store::validate_offline_connection_v8` pins `schema_version=8`                                  |

SQLite cannot alter a CHECK. Each of the three tables has to be rebuilt, and each has dependents: three
child tables reference the launch table through composite foreign keys
(`managed_turn_launches`, `managed_approval_launches`, `managed_operation_launches`), the runtime table is
itself a foreign-key child of the unique index `managed_sessions_runtime_target`, and a dozen append-only
triggers and two partial unique indexes hang off these tables. `managed_sessions.adapter_id` has **no** CHECK: a Claude _session_ row is already storable; only its
runtime, launch and binding are not.

The V7→V8 migration is not in `schema::migrate` (which stops at 7). It is an offline preparation on a private
clone, validated, with a frozen boundary (`migration_boundary`: the head and a SHA-256 over every V7
event). V9 has to extend that chain of trust, not reuse it implicitly.

The global capacity of one runtime is **not** a store property: the store's triggers are row counts (128
runtimes, 4096 launches); one running runtime is the registry's and the engine's.

## What the rehearsal does (and the procedure it recommends)

On a private clone of a validated V8 image, with foreign keys off and the legacy rename rule on (the
pattern `control::schema` already uses for table rebuilds), inside one `BEGIN IMMEDIATE`:

1. **Precondition: quiescence**, as V7→V8 required: no `starting`/`running` turn, no `pending` approval,
   no `active` lease, no `reserved`/`dispatching` operation, and no runtime whose desired state is
   `running`. A store with work in flight is refused (tested).
2. For `managed_runtimes`, `managed_runtime_launches`, `managed_session_bindings`, `authority_metadata`:
   rename away; recreate from the V8 definition with only these text changes —
   `CHECK(adapter_id='codex')` → `CHECK(adapter_id IN ('codex','claude'))`,
   `CHECK(runtime_kind='codex')` → `CHECK(runtime_kind=adapter_id)`,
   `CHECK(schema_version=8)` → `=9`; refill `INSERT … SELECT` (metadata carries `schema_version` 9);
   drop the old copy; recreate every index and trigger that lived on it, read from the V8 image itself so
   none can be forgotten. A V8 definition that does not contain each expected CHECK exactly once is refused,
   so a stale rehearsal cannot migrate a moved schema.
3. A second boundary, `migration_boundary_v9` (append-only), freezes the V8 head, the stream and a SHA-256
   over every V8 event.
4. `PRAGMA user_version=9` last; commit; the V9 validator runs before the image is handed back.

The unit that is returned is a **new image**; the V8 source is only read. Publishing it to an installation
is not part of this ADR's engineering: it would be an atomic rename of a validated file after a backup,
which is the part that needs the owner's release decision.

## Results of the rehearsal (10 tests, all passing)

| Property                                                                   | Result                                                                                                                                                                                       |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The synthetic V8 image has real work in every table the migration rebuilds | written by the authority's own writers through the protocol: sessions, a runtime and launch, a binding, 3 turns, accepted and declined approvals with delivery records, receipts, ≥20 events |
| Row counts and rows                                                        | every table other than `authority_metadata` is equal **row for row, in order**; metadata differs only in `schema_version` 8→9                                                                |
| Event history                                                              | the same bytes: the SHA-256 chain over all events is identical before and after                                                                                                              |
| Generations, bindings, receipts, approvals                                 | inside the row-for-row equality above                                                                                                                                                        |
| Exact schema                                                               | equals `expected_v9()` (the V8 text with the widened CHECKs plus the new boundary), derived independently of the migration; exactly four tables changed                                      |
| Triggers and indexes of the rebuilt tables                                 | identical, character for character                                                                                                                                                           |
| Differential verification                                                  | the unchanged `validate_v8` run on the V8 view of the V9 image returns the same `V8Summary` as on the source                                                                                 |
| A Claude runtime                                                           | refused by V8 (`CHECK constraint failed`), accepted by V9; `openai`, `codex/claude` and `claude/codex` still refused (closed set, pairing)                                                   |
| Interruption                                                               | an injected failure after each of ≥25 stages returns no image and leaves the source row-for-row unchanged and valid                                                                          |
| Work in flight                                                             | refused                                                                                                                                                                                      |
| Old binaries                                                               | `validate_v8`, the V8 offline opener, the V7 opener and `schema::migrate` all refuse a V9 store (`StorageIncompatible`)                                                                      |
| History rewritten or the boundary moved                                    | the boundary cannot be updated or deleted (`append_only`); a rewritten event or another stream fails validation                                                                              |
| Row-count triggers                                                         | present and identical; capacity one is the registry's (its existing test runs unchanged)                                                                                                     |
| No path, installed store or authority is touched                           | the module and its tests contain none of the words for it (checked by a test), and the module is `cfg(test)`                                                                                 |

## What the rehearsal does **not** prove (still to do before acceptance can mean "ship")

- The **writers and replay validators** above still say `codex`. A V9 image can hold a Claude row; nothing
  yet writes one, and replay would reject its events. Widening them is the real engineering of V9: a new
  **event vocabulary** version (so an older reader treats the new events as unknown-future and refuses),
  validators that take the provider from the closed set, and the allocator/operations/handshake generalized
  from literals to a `Provider` value — all behind the existing Codex tests.
- A **V9 verifier**. `validate_v9` judges Codex-only data through the unchanged V8 verifier on its V8 view;
  it cannot judge a store that already holds Claude rows. That verifier must exist before the first Claude
  row is ever written.
- Publication: backup, atomic rename, and the rollback procedure below, rehearsed against a _file_ image.

## Rollback

There is no in-place downgrade of a V9 store that holds anything V8 could not have written.
`v8_view` (the prototype of the offline rollback) **refuses** any image with a non-Codex row. So:

1. before the first Claude runtime exists: an offline tool may rebuild V8 from V9 (round trip proven row for
   row), after which older binaries work again;
2. after it: the only rollback is to **disable Claude admission** in the binary (a gate, not a store
   change) and keep running V9 with Codex; restoring the pre-migration backup loses everything written since.

The release record must say which of the two it is promising. A backup (`VACUUM INTO` a new private file plus
`integrity_check`) precedes any migration; the migration refuses to start if it cannot be made.

## Options

| Option                                                                                                | Effect                                                                                                                   |
| ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| **A. V9 migration as rehearsed**, then widen writers/validators and add the V9 verifier (recommended) | One store, one fence/receipt model, one event stream. Costs a migration, a vocabulary bump and the generalization above. |
| B. A second store beside V8 only for Claude                                                           | V8 untouched, but two stores mean two sets of fences, a cross-store capacity rule and a split-brain risk.                |
| C. No durable Claude runtime (fixture only)                                                           | What ships now. Nothing to migrate; Claude stays a development fixture.                                                  |

## Recommendation and consequences

**A**, in this order: (1) accept ADR0053 and ADR0056 (otherwise there is nothing to store); (2) widen
writers and validators behind the Codex suites and write the V9 verifier; (3) publication and backup
rehearsed on a file image; (4) only then a release that includes the migration. Until accepted: no Claude
runtime in the installed application (enforced by `Admission`, not only displayed), Codex unchanged. After:
a V9 store cannot be opened by an older Console, so installation and rollback need the explicit step above.
