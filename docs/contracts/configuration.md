# Configuration, custody and recovery contract

Current rules for `agentconfig/`, `credentials/`, Native profiles and Arsenal authoring.
Target journeys are in [requirements](../REQUIREMENTS.md#fp-cfg-02); delivered support and
qualification gaps belong to [STATUS](../STATUS.md), not this contract.

## Inventory and scope

Keep configured, provider-resolved, session-observed and actually used facts separate.
Read-only inventory does not launch a provider just to inspect it, set Codex trust, or
rewrite global settings. Provider-specific parsers keep provenance and explicit unknown,
unsupported and refused states. External changes invalidate observations and pending plans.

Supported authoring is kind-specific: Claude hooks/settings/skills/MCP and supported
plugin/instruction records; Codex skills/MCP and supported instruction records; bounded
OpenCode/Hermes surfaces only where the adapter advertises them. A capability descriptor
does not authorize arbitrary provider files or prove actual runtime consumption. Use the
[provider matrix](provider-capabilities.md) and source capability allowlists for exact support.

Native/project launch eligibility is distinct from configuration-write eligibility.
Configuration roots are inside HOME, never HOME itself, provider/credential roots, or
an ancestor reached through a link; ownership and private directory checks apply.
Project selection changes invalidate plans, serialize with apply and never retarget live
sessions. Undo for another project requires selecting that project first.

## Plan and apply

1. Inventory and author in the native editor. Preview names exact scope, kind, risk and
   expected effect; a prediction of Managed eligibility is labeled a prediction.
2. Backend plans are single-use, expire after 60 seconds and are bounded to eight outstanding.
   They bind the current project/context, file identity, digest and parsed semantic change.
3. Security-weakening or code-executing changes require the native confirmation appropriate
   to that risk. The page cannot fabricate approval. Same-UID compromise remains outside
   that human-confirmation guarantee; see [security](security.md).
4. Lock the target, reread its identity/digest and refuse outside edits or adjacent sync
   conflict copies. Record the intent before effect; inability to journal means no write.
5. Seal material requiring custody before removing it. Write a same-directory temporary
   file, preserve mode, fsync, recheck descriptors/identity, rename and fsync the directory.
   Read back, verify and journal the outcome. Inventory is refreshed after the operation.

JSON/TOML edits preserve unrelated bytes, comments, quoting, order and whitespace. Independently
compute the edit on bytes and parsed values and require agreement. Reject duplicate keys,
ambiguous spans, invalid or overly deep input rather than serialize the entire file. Forged
records cannot broaden edits into trust/security settings or another provider/kind. Follow
directory descriptors, not a path re-resolved after validation; preserve an existing final
link rather than replacing the link itself where that operation supports a linked file.
This descriptor-based rename is the implemented protocol; an old proposed exchange/settling
writer is not a second implementation promise. Non-cooperating external writers remain a
TOCTOU risk, not something an advisory lock eliminates.

Creation distinguishes absent from empty: publish with no-replace semantics, never overwrite
a file that appeared after preview. Deletion uses an identity-rechecked tombstone and crash
recovery, never a blind unlink/overwrite. Skill deletion is limited to the folder holding
the selected `SKILL.md`; reject unexpected contents/identities. New directories are private
0700 and cleanup removes only directories still empty and owned by the operation.

## Undo and interruption

Undo is kind-specific. A boolean toggle can retain only prior absent/enabled/disabled state;
restoring a removed structured entry or file needs its sealed bytes and current validation.
An unchanged target can restore exact bytes. A diverged target must be planned against the
current inventory and refuse conflicting entries; there is no universal exact Undo and no
silent overwrite of outside edits. Hook restoration may append into its original group only
when the supported restore rules permit it. Retained paths/records do not grant a new scope.

An unfinished intent is repaired from the current file digest: expected after-image means
`recovered_applied`, original image means `recovered_not_applied`; neither means
`recovered_unverified`. Remove only the operation's exact temporary names. Disk-full,
journal/seal/write/fsync/rename/read-back failures are explicit and preserve recoverable state.
User-held restoration records are never purged by age; the target default is a 30-day
review reminder. Explicit purge is separate from restoring or deleting a credential.

## Sealed custody and residual limits

Accepted custody uses a random key in Secret Service and ChaCha20-Poly1305 records, with
fresh nonces and authenticated binding to record identity, kind and version. The key is
never persisted outside wallet custody. Secret-bearing entries, instructions/templates
and their required inverses use sealed storage; no secret-bearing plaintext fallback.
An explicitly disclosed secret-free fallback remains accepted when custody is unavailable.
The conservative classifier can miss a secret: this is a residual risk, not proof that
every retained entry is harmless. A proposed fully sealed extension has not replaced this
accepted fallback. Key loss is unrecoverable; same-account access to an unlocked wallet
and privileged/admin processes are not defeated. Encrypting the restoration store does
not protect originals, outside snapshots or sync copies of provider files.

## Credentials and templates

API credentials are sealed, never returned in ordinary DTOs, logs or typed into Native
terminals. Bound the store to 100 records and each key to 4096 bytes. Native confirmation
and a record fingerprint identify custody operations. Custody does not authorize delivery
to a provider: the proposed Claude runtime route has separate gates in
[Managed runtime](managed-runtime.md#proposed-claude-and-multi-provider-admission).

Portable instruction templates use passphrase PBKDF2-SHA256 (600,000 iterations) and
ChaCha20-Poly1305, with domain separation from API-key envelopes. A template carries no
destination, provider permission or grant. Import is untrusted content: show it in the
native editor, review, then construct the normal scoped plan. Authentication/decryption
failure refuses; successful import alone does not apply anything or prove provider use.
