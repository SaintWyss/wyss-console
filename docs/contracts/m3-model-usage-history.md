# M3 model and usage history — proposed privacy/store contract

Status: **Proposed; persistence disabled pending owner acceptance** · Date: 2026-10-23

This contract is separate from the F8 control audit, configuration journal, encrypted
recovery/vault and provider-owned conversation transcript. It defines an optional local
history of provider-reported usage/capability metadata only. Product-spec retention values
are proposals; this document does not record Santiago's acceptance.

## Store and schema

- Disabled by default. No production write path or user toggle is enabled before explicit
  owner acceptance and a security review of the final implementation.
- Store under an application-owned private local state directory with owner/mode/symlink
  checks and a separate SQLite database. Never add rows, cleanup, export or migrations to
  the control authority, configuration journal or vault.
- Each record contains only: schema version, provider/build, authority/session/runtime
  opaque IDs, generation, turn/observation ID, observation kind, provider-reported numeric
  usage fields with provider unit semantics, source/provenance, event timestamp/received
  timestamp, completeness/freshness, optional provider-reported reset/limit and an origin
  event deduplication key.
- Exclude prompts, responses, reasoning text, secrets/tokens, arbitrary tool output,
  commands/diffs, account identifiers, filesystem paths, full provider payload and
  unverified cost estimates. No raw event JSON is stored.
- Distinguish API billing from subscription allowance. Unknown, absent or delayed usage is
  null/unknown, never zero. Store provider-estimated cost only if separately accepted and
  clearly labeled as an estimate, not as an enforceable financial limit.

## Bounds and semantics proposed for review

- Maximum retention: 30 days. Maximum record count: 10,000. Whichever bound is reached
  first prunes oldest eligible metrics only; all deletion is explicit in UI/export metadata.
- The schema is closed and versioned. Reject unknown columns/fields and reject a row whose
  canonical encoded form exceeds 1,024 bytes. The only text fields are bounded opaque
  identifiers: `provider` (32 ASCII bytes), `provider_build` (64), `session_id` and
  `runtime_id` (64 each), `turn_id` and `origin_event_id` (128 each). These values must
  match `[A-Za-z0-9._:-]+`; reject rather than truncate. Generation and timestamps are
  signed 64-bit integers, usage/limit values are nullable non-negative signed 64-bit
  integers, and observation kind/completeness/freshness are closed enums.
- `source/provenance` is a closed enum (`provider_usage_event`, `provider_limit_event`,
  `provider_status`, `provider_catalogue`). It is never free text and cannot contain a path,
  account name, command, prompt fragment or provider payload. The deduplication key is
  derived only from the bounded provider, generation, origin event ID and observation kind;
  it is not an arbitrary caller-supplied string.
- The store has a hard 12 MiB database limit and a 24 MiB combined limit for the database
  plus its SQLite journal/WAL sidecars. Enforce SQLite page-count limits and check the
  combined on-disk quota before and after each transaction. If a write cannot fit within
  either bound after eligible pruning, roll it back and report partial history; never
  increase the quota or silently omit the event. Export is streamed with a 12 MiB maximum
  output size and aborts cleanly at the limit.
- Deduplicate by `(provider, runtime-generation, origin-event-id, observation-kind)`; a
  duplicate is ignored, not counted again. A late event is retained only if its originating
  turn/generation still identifies the same observation; otherwise show it as stale or drop
  it according to the accepted policy.
- A transaction inserts a complete record and dedupe marker atomically. Startup recovery
  validates schema/integrity before use. Corruption quarantines only this optional store;
  the application reports history unavailable and does not rewrite control/audit/vault.
- Disk-full, locked database, export failure or pruning failure disables history writes and
  returns an explicit partial-history state. It cannot block turns, approvals, cancellation
  or recovery.
- Export contains only the allowlisted schema. Reset deletes this store only, reports the
  result and never touches protected records. No automatic telemetry or upload exists.
- Metrics settings, retention and reset/export affordances are GUI-visible. Since history
  defaults off, no background polling or provider querying is initiated to populate it.

## Required tests before enabling persistence

Schema allowlist and secret-marker corpus; rejection of unknown fields, invalid provenance,
oversized identifiers/records/exports and a store exceeding its combined 24 MiB quota;
duplicate/late/mixed-unit events; 30-day and 10,000-record pruning; interrupted insert/index
update; locked/full disk; corrupt database; export/reset isolation; symlink/ACL/ownership
attacks; and assertions that audit/config/vault digests remain unchanged. Benchmarks must
show bounded write/read/export cost. A focused independent security review and Santiago's
explicit approval are required before enabling the setting or write path.
