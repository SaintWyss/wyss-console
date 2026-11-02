# Managed runtime and pending admission contracts

Ordinary P1 and explicitly selected F8/P2 remain separate authorities/stores. Current F8
per-session architecture is accepted (formerly ADR0051); production capacity is **one**,
with the pinned Codex 0.159.0 Local Tools spawner. This spawner refuses production inference
without a workspace. Exact P2 fields remain in [protocol 2](f8/protocol-2.md), executable
limits in [resources](f8/resources.json). Neither a GUI connection nor a capability label grants permission.

## Authority, state and audit

The independent authority is the sole writer of canonical SQLite state. GUI/Rust desktop
are clients. Transactional state, receipts and bounded audit belong together; the audit
is not event-sourced authority. Validate exact schemas, integrity and witness/anchor state;
unknown/newer/corrupt stores refuse rather than reset, downgrade or silently recreate.
Back up SQLite/WAL consistently and preserve `control.anchor`; never delete a witness to
make an error disappear. A GUI close/crash neither revokes nor renews permissions. The
authority remains while work/obligations exist; explicit stop/revoke is a validated intent.

Replay preserves original IDs, sequence and durable ordering, not timestamp sorting, and
has no effects. Unknown future events require the vocabulary guard. Audit allows only
bounded metadata, never prompt/output/reasoning/diff/command/path/secret payload. Provider
conversation is separate. UI reconnection reads authority truth rather than cached grants.

## Leases, effects and recovery

Deny by default. Scopes imply no additional scopes. Bind identity, authority instance,
session/runtime, generation, revision, workspace and original lease. TTL is independently
enforced; a connected GUI cannot renew it. Reevaluate gates immediately before effects;
expiry/revocation invalidates pending receipts/approvals. Pending approval retains its
fingerprint, deadline and original lease; reconnect never substitutes a fresh grant.

An intent UUID is idempotent only for the same payload; a different payload conflicts.
Retry refers to the original receipt. Dispatch uncertainty is `indeterminate`, never an
excuse to resubmit after crash or a missing acknowledgment. No exactly-once external-effect
guarantee exists. Cancellation, stop/reap and resource release remain independently bounded.
Production Local Tools uses a private empty runtime cwd, not an old compiled repository root.

Accepted Local Tools residual risks include the validated provider `auth.json` descriptor
(read/write for refresh), readable by an explicitly approved command, pre-existing workspace
hardlink effects, approved free commands and TOCTOU. Provider binaries remain trusted within
the qualified sandbox. Same-UID socket checks do not prove human presence or protect from
account/admin compromise. Do not relabel these profiles as restricted B1 or strong approval.

## Proposed Claude and multi-provider admission

The following proposals retain their status. Implementation, offline fixtures or this
consolidation do not accept them. All relevant `src-tauri/src/m3/gate.rs` gates remain false.

| Proposal                       | Required contract and evidence before admission                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ADR0053 — Claude boundary      | Pin the official SDK/CLI/auth surface and enumerate **all** executable effects, including inherited tools/hooks. A callback is not complete mediation. Inference must prove no effects; Local Tools must hold every exact action until current authority approval, with at most one dispatch. Offline SDK tests are not real-provider authentication or complete security evidence.                                       |
| ADR0054 — Model/effort         | Negotiate a separately versioned capability; do not add optional caller fields to old P2. Model is per turn; effort is per turn only where supported, otherwise fixed at runtime start/generation. Reject stale catalogue/generation and unsupported choices without fallback. Offered, configured, requested and reported facts remain distinct. Only bounded metadata is audited; ambiguous dispatch is never replayed. |
| ADR0055 — Provider-aware store | Exact schemas, event vocabulary and verifier, mixed-provider replay, consistent backup and atomic publication/rollback need acceptance. Offline V8-to-V9 preparation/replay exists in `f8/v9.rs`; it is not merely a test rehearsal, but installed automatic migration/live admission remains closed. Field recovery must be qualified before operating on a real store.                                                  |
| ADR0056 — Credential delivery  | Owner chooses the Anthropic record for one runtime and native confirmation shows name/fingerprint/profile. A broker holds the key; the worker does not. No credential DTO/log/persistence or zeroization claim. Same-UID ptrace remains a residual risk. Deleting a key and stopping its runtime are separate actions. Independent review and acceptance of ADR0053/0055 are prerequisites to real delivery.              |

Model/usage code and bridge workers already exist. Optional historical metrics require the
separate proposed [usage-history](usage-history.md) policy; usage absent/unknown is not zero,
an offer is not entitlement and subscription allowance is not API billing. Native/delegation
Claude acceptance does not admit Claude Managed.

## Required qualification

Each provider/profile needs source, fixture and real-provider evidence for closed fields,
no-effect/complete mediation, stale/cross-scope/generation rejection, lease expiry, cancellation,
malformed/oversized/late events, crash at dispatch, broker/credential failure and honest recovery.
Measure readiness, stream/backpressure, cancel/reap, idle trees and fairness against accepted
budgets. Real GUI journeys must show mode/profile/authority, requested and effective state,
pending/declined/expired permissions and partial history truthfully. Review auth/refresh,
mixed-provider isolation, backup/rollback and negative controls independently. Enabling one
profile never enables another, raises capacity or falls back to Native silently.
