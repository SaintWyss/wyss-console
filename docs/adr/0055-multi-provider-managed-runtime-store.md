# ADR 0055 — A store that can hold a Managed runtime that is not Codex

Status: **Proposed; not accepted** · Date: 2026-10-23 · Owner: Santiago

## Context (a demonstrated blocker, not a guess)

M3 built a Claude Managed worker that plugs in at the engine's `Worker` seam and passes the authority-level
lifecycle tests with a Claude factory (`f8/claude/parity_tests.rs`). It cannot be admitted by the production
authority, because the accepted V8 store only knows Codex:

- `docs/contracts/f8/v7-to-v8.sql` declares `adapter_id TEXT NOT NULL CHECK(adapter_id='codex')` and
  `runtime_kind TEXT NOT NULL CHECK(runtime_kind='codex')` on `managed_runtimes` **and** on
  `managed_runtime_launches`. Inserting a Claude runtime fails the CHECK; the triggers that fence launches
  join on those same columns.
- `RuntimeTarget::supported()` and `RuntimeSafetyProfile` (`CodexInferenceV1`, `CodexLocalToolsV1`) have no
  other provider, `complete_handshake` compares the exact profile strings, and `"adapter_id":"codex"` is a
  literal in `allocator.rs` and `operations.rs`.

M3 forbids a schema migration without an accepted ADR, so the integration stops at the seam: admission is
closed by construction (`m3::gate::STORE_HOLDS_CLAUDE = false`) and the window shows that reason.

## Options

| Option                                                                                                                                                                         | Effect                                                                                                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **A. V9 migration** that replaces the two CHECKs with a closed set (`'codex'`, `'claude'`), extends the profile enum and the handshake, and removes the literals (recommended) | One store, one fence/receipt model for both providers. Needs a forward-only migration with a rehearsal on a copy, a rollback that disables Claude admission (old binaries must refuse a V9 store), and a protocol/DTO review. |
| B. A second store beside V8 only for Claude runtimes                                                                                                                           | V8 untouched, but two stores mean two sets of fences; capacity 1 and recovery would need a cross-store rule. Higher risk of a split brain.                                                                                    |
| C. No durable Claude runtime (fixture only)                                                                                                                                    | What ships now. Claude stays a development fixture; nothing to migrate.                                                                                                                                                       |

## Recommendation

**A**, after Santiago accepts ADR0053. Do it as its own change: migration rehearsal on a **copy** of a real
store, a downgrade refusal test, the allocator/operations literals replaced by the closed set, the handshake
profile comparison widened to the two Claude profile digests, then the existing authority parity suite run
against the real store with the Claude factory. Global runtime capacity stays 1.

## Consequences

Until accepted: no Claude runtime in the installed application, the Providers window explains why, Codex is
unchanged. After: a V9 store cannot be opened by an older Console, so the install/rollback procedure needs
an explicit "disable Claude, keep V8-compatible data" step; this ADR does not choose that; the release record
must.
