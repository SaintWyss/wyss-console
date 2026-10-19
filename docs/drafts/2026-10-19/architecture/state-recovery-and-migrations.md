# State stores, migrations, crash recovery and retention — complete design addendum

**Status:** Proposed supplemental contract to architecture [14](../../../architecture/14-DATA-PERSISTENCE-AND-RECOVERY.md), 07, 09, 11 and ADR 0029/0031 (reported locally, not read). F9 is reported implemented; this document is **not** a declaration of its actual schema. Existing stores and accepted ADRs remain authoritative.

## Ownership by data class

| State | Sole writer | Readers | Durable? | Sensitive? | Recovery |
| --- | --- | --- | --- | --- | --- |
| Managed approvals, leases, events, attestations | Managed authority | Authorized local control view | As authority contracts | Yes: audit metadata | Authority's existing recovery only |
| Native session registry, live PTYs, bounded scrollback | F9 host | Local/authorized clients | Registry yes; process/PTY volatile | Terminal output may contain secrets | Attach if same generation; otherwise Lost |
| Window layout, focus, pane tree and UI preferences | Desktop UI store | Desktop | Yes | Metadata; sanitized titles/path refs | Safe default, never spawn unknown process |
| Config inventory cache/last-seen fingerprints | Configuration engine | Desktop | Metadata only by default | Paths may be sensitive | Rescan; no automatic outside restore |
| Reversible config mutations, encrypted undo | Configuration engine | Owner via confirmation | Yes within retention | Possibly secrets | Journal replay + wallet key, fail closed |
| Provider settings/hooks/skills/MCP | Provider itself | Config engine | Provider-defined | Frequently sensitive | Byte-preserving plan/apply, never duplicate |
| Session profiles and version history | Profile store | Desktop/launcher | Yes | Must contain **no secrets** | Versioned import/export with explicit user review |
| Handoff package | Handoff store only on explicit save | User, chosen target | Explicit, bounded | Untrusted/confidential user context | Reopen review; no automatic send |
| Device identities, pairing and grants | Future gateway | Local owner management | Yes, with revocation | Device key material | Revoke if integrity or migration uncertain |
| F14 API credential values | OS wallet/locked vault | Dedicated native credential handler only | Yes when secure store available | **Secret** | Key loss/wallet locked yields unavailable, not plaintext |
| Diagnosics, stats, performance samples | Observability collector | Owner | Bounded and opt-in | No prompt/credential/terminal bytes | Delete/export with clear policy |

Never store terminal content as a side effect of persisting UI layout. Persistent PTY host means its process survives the GUI, **not** that active providers survive a host crash.

## Cross-store mutation protocol

1. Verify owner, XDG root, file trust, symlinks, source revision and store schema.
2. Build a pure plan referencing exact expected revision; do not expose unredacted prior bytes to webview.
3. Persist a bounded journal intent only where recovery requires one; secrets sealed using approved keys.
4. Confirm native risky operation according to existing ADR.
5. Apply atomically with correct fsync order and directory entry checks. Revalidate expected source immediately before commit, maintaining rename-exchange protections as required by local F2.
6. Persist completion receipt, verify provider-visible file changes, annotate live reload as verified/predicted/unknown.
7. On interruption, recover by journal/state-machine evidence. Never restore stale bytes over a newer provider write.

Unrelated components must not invent "transactions" across independent stores: use explicit compensating actions with observable partial failure. No implicit "best effort" restore of user settings.

## Schema envelope (candidate)

```json
{
  "schema_version": 1,
  "object_type": "window_layout",
  "object_id": "opaque-id",
  "writer_generation": "host-or-store-generation",
  "created_at": "RFC3339",
  "updated_at": "RFC3339",
  "payload": {}
}
```

This is illustrative only: no implementation may claim compatibility until fixtures match the actual local Rust and TS schemas. Each store declares version support and a maximum object size. Unknown critical fields or newer major version must fail closed; unrecognized optional fields are allowed **only** if the schema says so.

## Migration rules

| Scenario | Correct behavior |
| --- | --- |
| Same-version restart | Read-only scan then ordinary attach/recovery |
| Supported older schema | Dry-run plan, verified backup, atomic migration, post-check, new schema receipt |
| Failed migration mid-way | Restore exact verified pre-migration snapshot where safe; no silent data wipe |
| Newer unsupported schema | Enter read-only or unavailable with actionable error; never downgrade automatically |
| Replaced/symlinked store | Reject; do not open untrusted write location |
| Missing KWallet key | Mark encrypted data unrecoverable until key restored; do not derive a replacement silently |
| Host upgraded with live provider processes | Version negotiation or defer upgrade until graceful detach; don't SIGKILL by default |
| Remote grants during gateway upgrade | Reject stale/revoked identities across restarts; no fresh broad grant |
| Provider external edit concurrent with undo | Conflict receipt; preserve the newer provider bytes |

## Deletion and uninstall

Deletion is per-store and explicit: profiles, layouts, local logs, undo metadata, encrypted blobs, device grants and wallet entries must each have declared retention policy. Deleting a profile **does not** stop a running session unless the UI explicitly confirms both. Deleting an active credential must warn about references; revoke its grants where possible; don't expose the secret to search for uses.

Distinguish logical deletion, key deletion and secure erasure guarantee. Flash filesystems and wallet backups mean software cannot promise physical erasure. A diagnostic export defaults to omit content and path detail and is always previewable.

## Required validation

- Deterministic migration fixture N−1 → N and corrupt/newer version.
- Forced termination between each journal/storage step.
- Concurrency with provider rewrite and two Console clients.
- F9 host restart with orphaned sockets, mismatched generations, missing process and scrollback gap.
- Wallet lock/key loss/tampering; no fallbacks.
- Symlink and ancestor ACL attacks, including trusted-writer restrictions.
- Import/export round trip and unrelated state preservation.
- Repeated initialize/migrate is idempotent.
- Every error documented with no secret leakage.

## Integration obligations

Read the actual local F2/F9 stores and ADR 0029/0031. Replace candidate envelopes with their measured formats rather than creating parallel ownership. Add retention policies and migrations only for existing or newly authorized stores, not speculative tables.
