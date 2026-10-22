# 27 — Protected restoration vault

> **2026-10-22 consolidation:** this document retains dated design/evidence and detailed
> contracts. Its present-tense status/decision tables describe their stated baseline,
> not necessarily current implementation. Read the [current product set](../product/README.md),
> [verified gaps](../product/implementation-status.md) and [requirement crosswalk](../product/traceability.md)
> first. F8 is active P2/V8, ADR0051 Accepted, cap1; P1 remains available. New product
> direction does not supersede accepted security ADRs or turn proposed tests into PASS.

The store that lets Wyss Console put back, exactly, a configuration entry it removed from a
provider's file, when the provider has no switch of its own and no verified way to keep the
entry inside its own file. Domain L of [04](04-TARGET-ARCHITECTURE.md). All of it is `[P]`:
nothing here exists, nothing was run, and nobody but its author has read it.

**Standing.** That such a store may exist, and may hold content that contains a secret, is a
product requirement the owner approved on 2026-10-13. Everything below it is a proposed
technical contract: [ADR 0029](../adr/0029-secrets-restoration-data-and-configuration-persistence.md)
is **Proposed**, the choices this document leaves open are decisions DN-25 to DN-31
([22](22-DECISIONS-AND-OPEN-QUESTIONS.md)), and no phase that builds it is authorised.
Later on 2026-10-13 the owner gave his direction on those choices: an OS-backed keyring, with no
plaintext fallback; an optional recovery key; no deletion by age at first; conflicting versions
preferably in this vault; export and import designed and deferred; and, new, an **opt-in**
recovery copy for selected entries (§3.6). They are recorded as his design preferences. They
are not an acceptance of this document: its exact contract waits for an independent review.

## 1. Place in the restoration ladder

The vault is the third rung of four (ADR 0029 §3). It is used only when the two above it do not
apply to that entry, on that provider version.

| Rung | Mechanism                                                         | The entry lives                        | Vault used |
| ---- | ----------------------------------------------------------------- | -------------------------------------- | ---------- |
| 1    | The provider's own switch                                         | In the provider's file, switched off   | No         |
| 2    | Kept inside the provider's own file, where verified (ADR 0029 §5) | In the provider's file, in a kept form | No         |
| 3    | Removed from the file and sealed in the vault                     | In the vault, on this computer only    | Yes        |
| 4    | Temporarily unavailable                                           | Where it was; nothing changes          | No         |

What rung 3 needs from a provider is much less than rung 2 does. Rung 2 leaves data in the file
that the provider does not know, and so depends on how the provider treats unknown data, on
every one of its rewrite paths. Rung 3 leaves a file that contains nothing but ordinary
entries: the file a user without that hook would have written. What it still depends on:

| Dependency                                                               | Kind                          | Where it is settled                                     |
| ------------------------------------------------------------------------ | ----------------------------- | ------------------------------------------------------- |
| The file can be edited without touching any other byte                   | Static, checked on every file | The round-trip precondition (07 §5, E4)                 |
| A file with that entry absent is one the provider accepts                | Documented `[D]`              | The provider's configuration reference                  |
| Removing the last entry of a group leaves no empty container             | Design rule, §3.2             | This document                                           |
| A provider write at the same moment is not lost unseen                   | Not guaranteed                | [28](28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md); DN-21 |
| Running sessions stop using the entry                                    | Documented, not observed      | OQ-28                                                   |
| The contract of this document is accepted and the key custody is decided | The owner's decisions         | ADR 0029; DN-25 to DN-31                                |

## 2. Components and boundaries

| Component         | What it is                                                    | Holds                                                                                                                                  | Never holds                            |
| ----------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| Sealed records    | One file per record, written once, never modified             | Encrypted content and an authenticated header                                                                                          | Anything in clear but the header of §4 |
| Vault index       | One file, rewritten beside and renamed                        | Lifecycle state and metadata of each record; tombstones                                                                                | Content; any unkeyed digest of content |
| Consent record    | One file                                                      | What was consented to, under which policy version, and when                                                                            | Content                                |
| Vault key         | 32 random bytes, held outside the vault's directory (§5)      | —                                                                                                                                      | —                                      |
| Vault keys record | One sealed record, written at creation                        | Two keys of the vault's own: one for the digests of records and of the index, one for the file digests of the mutations journal (§5.6) | Anything in clear                      |
| Vault service     | Code in the desktop backend, part of the configuration engine | The key and decrypted bytes, in memory, while an operation runs                                                                        | Either of them after the operation     |

Boundaries:

| Between                         | Rule                                                                                                                                                                                                                                                                                                          |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Vault ↔ authority               | None. The authority does not link, read or know of the vault (C-02, C-03). ADR 0003 stays whole for every store of the authority                                                                                                                                                                              |
| Vault ↔ page                    | The page receives metadata, and content only through the same redacted projection the inventory uses. It never receives a key, a sealed record or unredacted content (FR-CFG-16)                                                                                                                              |
| Vault ↔ journal and indexes     | The journal names a record by its identifier. Nothing derived from a record's content enters the journal, the last-seen index, a log or a diagnostic                                                                                                                                                          |
| Vault ↔ providers               | A provider never reads the vault. The vault directory is never inside a provider's directory                                                                                                                                                                                                                  |
| Vault ↔ replication and backups | Where the vault may be is judged by a criterion that is **open** (§9.1; DN-31). Being under a replication tool's root is not proof that it is replicated, and an ignore rule is evidence, not a guarantee. A snapshot of the home subvolume captures it. What any of them captures of the vault is ciphertext |
| Vault ↔ other processes         | **No boundary against a process of the same user** while the key can be obtained (§10, T-27). The vault protects content at rest, not a live session that is already compromised                                                                                                                              |

Location: `vault/` under the Console's private state directory (14 §3), mode 0700, files 0600,
every ancestor checked by the path rule already in use (CTL-10), opened once and used through
that descriptor. The vault is refused (`vault_location_unsafe`) if an ancestor is a link, is
writable by another user, or carries the marker of a replication folder (§9.1).

## 3. What a record holds

### 3.1 Classes

| Class              | Created by                                                                   | Content                                                          | Why a copy is needed                                                               | Phase |
| ------------------ | ---------------------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ----- |
| `disabled_entry`   | Disabling an entry on rung 3                                                 | The removed span of one entry (§3.2)                             | The entry exists nowhere else once it is removed                                   | F2v   |
| `conflict_version` | A write conflict whose losing version cannot be put back (28 §6; DN-28)      | One whole version of one provider file                           | It is a version only the Console holds, through a descriptor, and it would be lost | F2v   |
| `undo_entry`       | Deleting or editing an entry, if the contract of authoring adopts it (DN-12) | The removed span, or the span before the edit                    | Undo of a delete or of an edit has no other design (25, IR-04)                     | F5    |
| `recovery_copy`    | The user's explicit choice, entry by entry (§3.6)                            | The span of one entry as it stands; the entry is **not** removed | Recovering an entry that something outside the Console later deletes or changes    | F2h   |

`conflict_version` is the one class that copies a whole file, and with it entries and values
unrelated to the operation. It is justified only because the alternative is to lose that
version or to leave it in clear beside the provider's file. It has its own consent line (§7),
its own bound and the shortest retention (§8.3).

One more thing is sealed and holds no content at all: for the duration of a direct write, a
**placeholder** of the file's size (07 §5.4). It shows, before anything is changed, that the
key, the storage and the space a conflicting version would need are there, and it is removed
when the write has an outcome that needs no sealing.

Nothing else is ever sealed: not a file before a change "to be safe", not the entries the
inventory has seen unless the user selected one (§3.6), not a provider's credential or sign-in file, which no adapter opens
(CTL-06).

### 3.2 The removed span

Disabling by removal is a deletion of **one contiguous run of bytes** from the file.

| Term     | Meaning                                                    |
| -------- | ---------------------------------------------------------- |
| F0, h0   | The file and its hash when the plan was computed           |
| F1, h1   | The planned file and its hash                              |
| `span`   | The bytes removed                                          |
| `offset` | Where they were: `F0 = F1[..offset] + span + F1[offset..]` |

Rules for the adapter that plans the removal:

1. The span covers the entry and the separator that goes with it (in JSON, one adjacent comma
   and its whitespace).
2. **Pruning.** If removing the entry leaves its container empty, the container is removed too,
   and so on upward: the last handler of a matcher group takes the group, the last group of an
   event takes the event's key, the last event takes the `hooks` key. The span grows to the
   outermost container removed. The file that remains has no empty array or object that was
   not there before.
3. The removal is refused (`span_not_contiguous`) if F1 cannot be written as F0 minus one run
   of bytes, which is what the round-trip precondition already demands of every edit.
4. Before anything is written, the engine recomputes `F1[..offset] + span + F1[offset..]` and
   checks that it hashes to h0. An exact restoration is therefore shown to be possible at the
   moment the record is made, not hoped for later.

This is the minimum that an exact restoration needs: the entry's own bytes and the punctuation
that held it. No other entry of the file is in the record.

### 3.3 Payload (encrypted)

| Field              | Content                                                                                                                                                  | Needed for                                                                          |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `class`            | §3.1                                                                                                                                                     | Policy                                                                              |
| `span`, `offset`   | §3.2. For `conflict_version`: the whole file, offset 0                                                                                                   | Exact restoration                                                                   |
| `h0`, `h1`         | Hashes of the file before and after                                                                                                                      | Knowing whether an exact restoration applies                                        |
| `path`             | Where the entry sat, as the adapter addresses it: kind, event name, index of the group, index of the handler, and how many levels were pruned            | Restoration into a file that has changed                                            |
| `anchors`          | Digests of the entry before and the entry after, at each level, keyed with the vault digest key (§5.6); and, for a group, the digest of the group itself | Finding the one place where the entry goes back (§3.7)                              |
| `entry_digest`     | The digest of this entry, keyed with the vault digest key (§5.6), and how many entries of the same source had the same digest when the record was made   | Recognising the entry if it reappears; refusing a placement that would be ambiguous |
| `provider`, `kind` | Provider identifier; kind of entry                                                                                                                       | Binding                                                                             |
| `provider_version` | The version observed when the record was made, or `unknown`                                                                                              | Telling the user what it was made under                                             |
| `adapter_format`   | The adapter's version of this document format                                                                                                            | Refusing a restoration it no longer understands                                     |
| `scope`, `source`  | Scope; the source's identifier from the adapter's closed list; its canonical path; for a project source, the project's root                              | Binding to the right file (T-33)                                                    |
| `operation`        | The journal's operation identifier                                                                                                                       | Recovery (§8.2)                                                                     |
| `created`          | Time                                                                                                                                                     | Retention                                                                           |
| `origin`           | `local`, or `imported` with the exporting installation's identifier                                                                                      | The stricter rule for imported content (§9.3)                                       |

The canonical path and the project root are text the user wrote; they are inside the encrypted
payload and nowhere else.

### 3.4 Index entry (in clear, metadata only)

Record identifier; class; provider; scope; source identifier; kind; event name if it is one the
adapter knows; `entry_digest`; state and the time it was entered (§8); key identifier; size;
operation identifier; origin. This is what lets the window list what is kept while the key is
unavailable, and what repair reads at start. It holds nothing from which an entry could be
rebuilt. A purged record leaves a **tombstone**: its identifier and the time, kept for a bounded
period, so that a record file that reappears is recognised as one already retired (T-31).

The `entry_digest` kept in the index is keyed with the vault digest key, and that key is itself
sealed in the vault (§5.6). Without the custody the index cannot be used to confirm a guess at
the content of a held entry; and while the vault is locked, whether a held entry is back in its
file is shown as not known, because telling needs that key.

### 3.5 Bounds

Proposals, to be fixed with the fixtures (OQ-22).

| Bound                                | Proposed                               | When reached                                                   |
| ------------------------------------ | -------------------------------------- | -------------------------------------------------------------- |
| One `disabled_entry` or `undo_entry` | 256 KiB                                | `entry_too_large`: the operation is not offered for that entry |
| One `conflict_version`               | The read bound of its source (07 §4.2) | The version is handled by the fallback of 28 §6                |
| Records in the vault                 | 1,000                                  | `vault_full`: no new record, so no operation that needs one    |
| Tombstones                           | 5,000 or 180 days                      | Oldest dropped                                                 |

**Reaching a bound never evicts a record.** A record may be the only copy of a piece of the
user's configuration; only its restoration or the user's explicit deletion retires it.

### 3.6 Recovery copies of selected entries (opt-in)

The general last-seen index holds no content, so an entry deleted from outside cannot be given
back from it. The owner approved that, and asked for the capability to exist by choice: an
encrypted copy of **entries the user selected beforehand**. This is a different thing from a
disabled hook held in the vault, and the two are never presented as one.

| Aspect                         | Rule                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Opt-in                         | Per entry, by an explicit act in that entry's detail: "Keep a recovery copy". Several entries can be chosen at once, up to a bound, proposed twenty. **Every request is confirmed in a native dialog that lists the exact entries**; a confirmation in the page is never what authorises a copy (§6.1). There is no switch for "all entries" and no rule that covers entries found later |
| Consent                        | Its own line of the consent of §7: a standing second copy of an entry that is still live                                                                                                                                                                                                                                                                                                 |
| What is captured               | The entry's own bytes as they stand, without pruning, since nothing is removed; where it sits and its neighbours' keyed digests; the file's hash at that moment; provider, scope, source identity. Nothing else of the file                                                                                                                                                              |
| When                           | **Only at the user's act**: when he selects the entry, and when he chooses "update the copy". Never by itself: an outside change to a selected entry causes no new capture, so nothing is copied without an act and no key is needed while observing                                                                                                                                     |
| Versions                       | One current copy per entry; if the user updated it, the earlier ones up to a bound, proposed three. Each is its own record                                                                                                                                                                                                                                                               |
| Tracking                       | By the entry's keyed digest. Seen in the source: `current`. No longer seen: `recoverable`. The Console cannot tell a deletion from an edit and says so: "the hook you kept a copy of is no longer in the file as it was"                                                                                                                                                                 |
| Recovery                       | A plan like any other. The copy is inserted at its recorded place, by its neighbours, and shown as a diff. It is `executes_code`: the native dialog shows the content. **Never described as byte-identical**: the file has changed                                                                                                                                                       |
| A different entry in its place | `restore_conflict`: both are shown; nothing is written                                                                                                                                                                                                                                                                                                                                   |
| Ending it                      | "Stop keeping it" purges the copy, by a native dialog in every case. Once the entry is gone from the file the dialog says that the copy is the only one                                                                                                                                                                                                                                  |
| Retention                      | No expiry by age (§8.3). Its age is shown. A copy whose entry changed is marked, so that an old secret is not kept unnoticed                                                                                                                                                                                                                                                             |
| With a disabled entry          | A hook disabled on rung 3 needs no recovery copy: its record is one. The copy is purged once that record holds, and the choice is remembered, as metadata, to be offered again when the hook is restored                                                                                                                                                                                 |

|                                               | `disabled_entry`                                                            | `recovery_copy`                                                                  |
| --------------------------------------------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Who took the entry out of the file            | The Console, because the user asked                                         | Something else, later; or nobody yet                                             |
| The entry in the file while the record exists | Absent                                                                      | Present, until something removes it                                              |
| Copies in existence                           | One: the record                                                             | Two while the entry is live: the file and the record                             |
| Made                                          | By the operation itself                                                     | Only by the user's selection                                                     |
| Invariant                                     | VA1                                                                         | None needed: the Console removes nothing                                         |
| Giving it back                                | The inverse of a known operation; byte-identical if the file did not change | An insertion into a file that changed; by position; never claimed exact          |
| What the window calls it                      | "Disabled, kept here"                                                       | "A recovery copy is kept" and, later, "removed outside; a copy can be recovered" |
| Phase                                         | F2v                                                                         | F2h                                                                              |

What it costs: a standing duplicate of entries that may hold secrets, in the vault and in every
backup of it, for as long as the user keeps the selection. An entry whose secret is changed in
the file leaves the old secret in the copy until he updates or drops it; the window marks such
a copy. The scope and the capture rule are decision DN-30.

### 3.7 Putting an entry back: exact, unambiguous, or not at all

This is the one rule for every restoration and every recovery, whatever the class of record.
No entry is ever placed "near" where it was.

| Mode       | When                                                    | What is done                                                                                                                                | What is said                                          |
| ---------- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| Exact      | The file still has the digest recorded after the change | The span is put back at its recorded offset. The result must have the digest recorded before the change; if it does not, nothing is written | "Restored. The file is byte-identical to before."     |
| Positional | The file changed since                                  | Only if the place is **unambiguous** by the rule below. The plan is a diff the user confirms in the native dialog                           | "Restored. The file had changed: not byte-identical." |
| None       | The place is ambiguous, or gone                         | Nothing is written. The record stays. Its content can be shown to the user, who places it by hand                                           | The reason, by its closed word                        |

The rule for an unambiguous place:

1. **The container.** The level that was not pruned is found by the record's `path`. It must be
   found exactly once. An event is identified by its name; a matcher group by the keyed digest
   of the group recorded when the entry was removed. If levels were pruned, the pruned
   container must be absent and its parent must be found exactly once.
2. **The candidates.** A candidate is a gap between two neighbours of that container, its start
   and its end included, where the element before is the recorded "before" anchor and the
   element after is the recorded "after" anchor.
3. **Exactly one candidate** is a place. The entry goes there.
4. **No candidate** (a neighbour is missing or was changed): `anchor_missing`. **More than one**
   (neighbours or entries are duplicated): `placement_ambiguous`. The container not found, or
   found more than once: `container_missing`, `container_ambiguous`. In each case nothing is
   written.
5. If an entry with the same digest is already anywhere in that source: `already_present`;
   nothing is written.
6. There is no fallback: not the end of the group, not the recorded index alone, not the
   nearest match. An entry whose digest was not unique in its source when it was removed can
   be restored in exact mode only.

"Exactly" therefore means: byte-identical when the file has not changed since; otherwise the
one unambiguous place, said to be not identical; otherwise not at all.

## 4. Record format

A record is one file, `records/<record id>.rec`, written once.

| Part       | Content                                                                                                                                                            | Protected            |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------- |
| Header     | Magic and format version; algorithm identifier; vault identifier; installation identifier; record identifier; key identifier; class; creation time; payload length | Authenticated, clear |
| Nonce      | Random, of the length the algorithm takes                                                                                                                          | —                    |
| Ciphertext | The payload of §3.3 in a canonical encoding                                                                                                                        | Encrypted            |
| Tag        | Authentication tag over the ciphertext with the header as associated data                                                                                          | —                    |

Consequences of authenticating the header: a record moved to another vault or another
installation, renamed to another identifier, or given another class does not verify. A record
whose tag does not verify is `corrupt` and is never used in part.

Format versions are explicit. A record of a newer format than the build understands is left
untouched and listed as `unsupported_format`, as every other store of the Console already does
with a newer schema (14 §3). An older format is read and, on the next rotation, rewritten.

### 4.1 Algorithms

| Choice                        | Proposed                                                               | Alternative                                                                   | Why                                                                                                                                      |
| ----------------------------- | ---------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| Authenticated encryption      | XChaCha20-Poly1305, one random 192-bit nonce per record                | AES-256-GCM with a per-record key; AES-256-GCM-SIV                            | A random nonce of that length cannot realistically repeat; no dependence on hardware instructions; a small, widely reviewed construction |
| Key per record                | Derived from the vault key and the record identifier with HKDF-SHA-256 | The vault key used directly; a random key per record wrapped by the vault key | A nonce reused by a defect cannot then cross records. Wrapping buys cheaper rotation, which records this small do not need               |
| Passphrase stretching (DN-25) | Argon2id, parameters stored in the vault's descriptor                  | scrypt                                                                        | Memory-hard; the standard choice                                                                                                         |
| Encoding of the payload       | One deterministic binary encoding, versioned                           | JSON                                                                          | The span is bytes, not text                                                                                                              |

The repository has no encryption or key-store dependency today (`src-tauri/Cargo.toml` holds
`sha2` only) `[R]`. Each library this needs is a dependency decision of its own, taken in the
phase that builds the vault (DN-04), with its licence and audit record. No primitive is
implemented by hand.

## 5. The key

### 5.1 Custody: alternatives

| ID  | Where the vault key lives                                                                                                                                       | Protects the records against                                                                                 | Does not protect against                                                                                                                                       | Cost                                                                     |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| K1  | The desktop's secret service, through the freedesktop Secret Service interface, as one item. On the owner's two computers its provider is GNOME Keyring (30 §6) | A stolen disk, a copied state directory, a backup or snapshot of it, an accidental replication, another user | A process of the same user while the keyring is unlocked; a keyring with an empty or weak password; a keyring file captured in the same backup and then opened | A secret service must be present and unlocked; one more dependency       |
| K2  | A key file in the state directory                                                                                                                               | Another user (which the directory's mode already does)                                                       | Everything else: the key travels with the ciphertext                                                                                                           | None. **It is plaintext storage with an extra step, and is not offered** |
| K3  | Derived from a passphrase the user types when the vault is first needed in a session                                                                            | The same as K1, to the strength of the passphrase, and independently of any keyring                          | A process of the same user while the key is in memory; a weak passphrase                                                                                       | A prompt per session in which the vault is used                          |
| K4  | Sealed to the machine (`systemd-creds`, a TPM)                                                                                                                  | A disk moved to another machine; a backup restored elsewhere                                                 | Anything running on the machine as the user                                                                                                                    | Unverified on the owner's computers; binds recovery to hardware          |

Recommended (DN-25): **K1 by default; K3 as the user's explicit choice** where there is no
secret service or he does not want to rely on it; K4 studied, not offered; K2 never. Whatever
is chosen, there is **no silent fallback**: if the chosen custody is unavailable, the
operations that need the vault are unavailable and say why.

The owner's direction (2026-10-13): prefer an OS-backed keyring, evaluating KWallet and the
Secret Service interface; explicit behaviour on failure; a secure alternative where necessary;
no plaintext fallback. His decision of the same day, after the inspection of 30 §6: **the
Secret Service interface is the design target** for the current environment, because both
inspected computers expose it. Its provider there is GNOME Keyring, although the desktop is
KDE. **KWallet is a separate custody**: selected explicitly, validated separately, never
reached by a fallback. The evaluation; nothing of it was tried beyond that inspection:

| Interface                                                      | Who provides it                                                                                                                                      | For                                                                                                                    | Against                                                                                                                                                                                                        |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The freedesktop Secret Service interface, over the session bus | On the owner's two computers: GNOME Keyring, observed on 2026-10-13 (30 §6). Elsewhere: KWallet in recent KDE releases `[N]`; some password managers | One integration for every desktop, which a distributed product needs. Exposed and unlocked on both inspected computers | Behaviour differs by provider: when it prompts, when it locks, which collection is the default. Which process holds the name is settled at each session's start: observed once per computer, not across starts |
| KWallet's own interface                                        | KDE only. Its daemon runs, with an open wallet, on both computers (observed)                                                                         | Works where the Secret Service interface is absent or switched off                                                     | A second integration, for one desktop, and **a separate store with separate content**. Its per-application prompt identifies the caller by a name the caller supplies `[N]`: not a boundary                    |

Decided: the Secret Service interface. KWallet's own is not built as an alternative path of
the same custody; if it is ever offered it is another custody, with its own validation. The
states of §5.2 are the same for any custody and nothing falls back. The secure alternative when
the chosen one does not answer is K3, a passphrase, by the user's explicit choice and a new
consent.

| What the custody never does                                             | Instead                                                                                                                                                                      |
| ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Look for the key in another store when the chosen one has none          | `key_missing`, said as such (§5.2)                                                                                                                                           |
| Create a new key because the old one is not found                       | The same. A new key would make every held record unreadable and hide it                                                                                                      |
| Copy the key, or any secret, from one provider to another               | Changing custody is the user's act: a native dialog, a new consent (§7), and a rotation (§5.4)                                                                               |
| Follow the Secret Service name to whichever provider holds it this time | The vault's descriptor records the interface and the provider that held the key when it was stored. A different provider behind the same name is `key_missing`, not a switch |
| Present an unlocked keyring as protection                               | A keyring unlocked at login answers every process acting as the user (§10, T-27). No text of the product says otherwise                                                      |

What the inspection of 2026-10-13 showed, and what it did not (OQ-38; 30 §6). Observed on both
computers: a secret service answers in the owner's session, provided by GNOME Keyring, with its
default collection unlocked. **Not verified**: whether it stays unlocked for the whole session,
whether its own password is empty, how it behaves locked, absent or emptied, and whether the
same provider holds the name after every start. A keyring that opens with no password protects
a copied disk no better than K2.

### 5.2 Vault and key states (SM-VKEY)

| State          | Meaning                                                                                 | Enters when                                            | Operations possible                                 | Leaves to                                                 |
| -------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------ | --------------------------------------------------- | --------------------------------------------------------- |
| `absent`       | No vault was ever created here                                                          | —                                                      | None that needs it; rungs 1, 2 and 4 are unaffected | `ready`, by consent (§7) and creation                     |
| `ready`        | The key can be obtained                                                                 | The custody answered                                   | All                                                 | Any below                                                 |
| `locked`       | The custody exists and refuses for now (keyring locked, prompt dismissed)               | The custody said so                                    | Listing metadata                                    | `ready` when it unlocks                                   |
| `no_custody`   | The chosen custody is not there (no secret service in this session)                     | The service is absent                                  | Listing metadata                                    | `ready`; or a change of custody by the user, with consent |
| `key_missing`  | The custody is there and has no key for this vault                                      | Keyring reset, restored home, another computer's files | Listing metadata; recovery (§5.4); deletion         | `ready` after recovery; `absent` after deletion           |
| `rotating`     | A rotation is in progress                                                               | Rotation started                                       | Reads                                               | `ready`                                                   |
| `newer_format` | The vault's descriptor is of a newer version                                            | An older build opened it                               | None; nothing is written                            | `ready` after an update                                   |
| `unsafe`       | The location fails the path rule, or fails the criterion of §9.1, which is open (DN-31) | The check of §2                                        | None                                                | Fixed by the user, outside                                |

The key is fetched when an operation needs it, not when the application starts: a terminal
opens with the keyring locked, and the vault's code is not on the start-up path (NFR-PERF).

### 5.3 Where content exists in clear, and what is not promised

The vault's guarantee is about **the files the Console writes**. It is not a guarantee about
every place a byte can reach on a computer, and no test can be one.

| In scope: no entry content in clear                                                                                            | Checked by |
| ------------------------------------------------------------------------------------------------------------------------------ | ---------- |
| The vault's directory: records, index, consent, keys record                                                                    | TS-VLT-01  |
| The journals, the last-seen index, logs, diagnostics, support bundles                                                          | TS-SEC-02  |
| Provider directories: no file made by the Console                                                                              | TS-WR-12   |
| The private transaction directory, once a write has reached a final outcome, except in a recovery incident (07 §5.4) (07 §5.4) | TS-WR-12   |

| Out of scope: content or keys may reach it, and the design does not prevent it                   | What is done, as far as it goes                                                                                    |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| Swap, and the image written when the computer hibernates                                         | Locking the buffers that hold keys and decrypted content, where the system allows it: best effort, not a guarantee |
| A core dump of the process, kept by the system's crash handler                                   | Asking the system not to dump the process is a setting of the whole application, and is open (OQ-39)               |
| A crash reporter                                                                                 | The Console is to ship none; whether the code today has one is to be checked in the phase `[N]`                    |
| The process's memory, read by another process of the same user                                   | Nothing (T-27)                                                                                                     |
| The page's memory: the redacted projection of an entry, and whatever the detector failed to mask | The page never receives a key or unredacted content (§6)                                                           |
| The clipboard, after "Copy" in the reveal dialog, and the history a clipboard manager keeps      | Said in that dialog                                                                                                |
| Snapshots, backups and freed blocks of the disk                                                  | They hold ciphertext of the vault, and, if taken during a write, a transient copy in clear (28 §7)                 |

Measures in memory, all proposed:

| Measure                                                                                                                             | Status                                                                    |
| ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| The vault key and decrypted payloads live in buffers that are overwritten when released                                             | Proposed                                                                  |
| The vault key is fetched per operation and not kept between operations. The two keys of the keys record may be kept for the session | Proposed: recognising a held entry needs one of them at every observation |
| Never serialised: not in a command answer, a log, a panic message or a diagnostic                                                   | Proposed; held by TS-VLT-12                                               |

None of this stops a process of the same user that reads the Console's memory. It reduces what
an accident leaves behind.

### 5.4 Loss, recovery and rotation

| Event                                                | Behaviour                                                                                                                                                                                                                                                                                                        |
| ---------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The key is lost (`key_missing`) and nothing else     | Every record is unreadable. **The entries they held are lost.** The window lists them by metadata and says so                                                                                                                                                                                                    |
| A recovery key exists (DN-26)                        | At creation the user may be shown, once, a recovery code of high entropy; the vault keeps a copy of its key wrapped under that code. Typing the code re-creates the key in the custody. The code is never stored by the Console                                                                                  |
| Rotation, asked by the user or after a format change | A new key is created in the custody first. Each record is decrypted, sealed under the new key into a new file, flushed and renamed over the old. The index is updated per record. The old key is removed last. A stop at any step leaves every record readable by one of the two keys; rotation resumes at start |
| A tag does not verify                                | That record is `corrupt`: kept, never used, listed. Nothing is repaired or guessed                                                                                                                                                                                                                               |
| Backing the key up                                   | Not done by the Console. The keyring's own backup, or the recovery code, is the user's                                                                                                                                                                                                                           |

Rotation does not revoke what was already copied: someone who holds an old record and the old
key still reads it. It limits what a key exposed later can open.

### 5.5 The recovery code: what it is, and what losing a key means

Optional, by the owner's direction (2026-10-13; DN-26).

| Aspect                | Design                                                                                                                                                                                                                                               |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| What it is            | A random code of at least 128 bits, shown once, in a native dialog, in groups a person can copy down. The vault keeps one small file beside its records: its key, encrypted under a key derived from the code, authenticated and bound to this vault |
| Offered               | When the vault is created and after each rotation. It can be declined; the dialog says what declining means                                                                                                                                          |
| Stored by the Console | Never the code. It is not written to a file, a log or the clipboard, and is not shown again                                                                                                                                                          |
| Use                   | `recover`: the user types the code in a native input; the key is rebuilt and placed in the custody                                                                                                                                                   |
| After a rotation      | The old wrapped key is void. A new code is shown, or recovery is off until the user asks for one                                                                                                                                                     |

| Threat                                               | Effect                                                                                                                |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Someone has the code and a copy of the vault         | He reads every record. The code is as sensitive as the keyring's password, and is worth nothing kept on the same disk |
| Someone has the code alone, or the wrapped key alone | Nothing: each is useless without the other, and the wrapped key cannot be guessed at that length                      |
| The code is seen while it is displayed               | As the first row, if he also gets the disk. It is shown once, in a dialog of the system, not in the page              |
| The wrapped-key file is altered                      | Its authentication fails: `recovery_failed`. The vault itself is unaffected while its key is in the custody           |

What the code does **not** do: repair a record that fails its authentication; bring back a
vault whose files are gone; replace the keyring for daily use.

| The key in the custody | The recovery code      | Result                                                                                                                                                                           |
| ---------------------- | ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Present                | Any                    | Normal                                                                                                                                                                           |
| Lost                   | Held by the user       | `recover` rebuilds the key and every record is readable again, **provided the wrapped-key file beside the vault is intact and authenticates**                                    |
| Lost                   | Never made, or lost    | **Every held entry and every recovery copy is lost.** They are listed by metadata, the user is told in those words, and the only action left is to delete the unreadable records |
| Lost                   | Held, wrapped key void | The same as the row above: `recovery_failed`                                                                                                                                     |

### 5.6 The vault's own keys, and what depends on them

The vault does not use the key that the last-seen index uses. The full table is 14 §3.2.

| Key                | Where                  | Used for                                                | Lost or deleted when                                   |
| ------------------ | ---------------------- | ------------------------------------------------------- | ------------------------------------------------------ |
| Vault key          | The custody (§5.1)     | Sealing every record                                    | The custody loses it: §5.4, §5.5                       |
| Vault digest key   | The sealed keys record | `entry_digest` and anchors, in records and in the index | Only with the vault                                    |
| Journal digest key | The sealed keys record | The file digests of the mutations journal (07 §5.4)     | Only with the vault, and never while an intent is open |

Consequences. Deleting the engine's history removes the index's own key and none of these, so
a held record is recognised, placed and restored as before. A rotation of the vault key seals
the keys record again under the new key and leaves the two keys inside it as they were: the
anchors of existing records stay comparable. The two inner keys are not rotated on their own,
because an anchor is the digest of an entry the record does not hold; replacing them would
need every held entry to be restored first. Deleting the whole vault is refused while a write
is unfinished or a conflicting version is held.

### 5.7 What works in each state of the vault

A locked keyring does not disable the Console. What it stops is exactly what needs the vault,
and nothing else (the owner's guidance of 2026-10-13).

| Capability                                                                                                    | Vault never set up                | Locked, or the custody absent                | The key is missing                                | Repair pending for a source                               |
| ------------------------------------------------------------------------------------------------------------- | --------------------------------- | -------------------------------------------- | ------------------------------------------------- | --------------------------------------------------------- |
| The terminal; native sessions; the managed plane                                                              | Yes                               | Yes                                          | Yes                                               | Yes                                                       |
| Reading and inventory of configuration; detection of outside changes; history                                 | Yes                               | Yes                                          | Yes                                               | Yes                                                       |
| The list of kept entries, by metadata                                                                         | Empty                             | Yes                                          | Yes                                               | Yes                                                       |
| Whether a held entry is back in its file                                                                      | —                                 | Shown as not known                           | Shown as not known                                | Yes, when unlocked                                        |
| An operation through a provider's own verified writer (07 §5.2; DN-13)                                        | Yes                               | Yes                                          | Yes                                               | Yes, except on the source whose repair is pending         |
| **A direct write to a provider's file** (07 §5.4): a switch written into the file, a kept form, a restoration | Refused until the vault is set up | Refused: `conflict_preservation_unavailable` | Refused                                           | Refused for that source; allowed for others when unlocked |
| Disabling an entry into the vault; keeping a recovery copy                                                    | After consent and creation        | Refused before the user confirms anything    | Refused                                           | Refused for that source                                   |
| Restoring, recovering, revealing a record                                                                     | —                                 | Waits; nothing is lost                       | Not possible; recovery of the key (§5.5)          | Refused for that source                                   |
| Deleting a record                                                                                             | —                                 | Yes, by metadata, with the native dialog     | Yes: the only action left on an unreadable record | No, while it is part of an unfinished write               |
| Finishing the repair of an interrupted write (14 §4)                                                          | —                                 | **Waits.** Every durable record is kept      | Waits; the user is told what cannot be checked    | —                                                         |

What needs the vault, and why: a direct write, because a conflicting version may have to be
sealed and because its journal digests use a key of the vault; anything that makes, opens or
removes a record. What does not: everything read-only, and whatever a provider writes through
its own command. No operation is added to this list for convenience.

Confirmed by the owner on 2026-10-13. Every direct mutation of a configuration file that the
Console itself performs needs the recovery prerequisites of its protocol, the vault's readiness
among them. **A switch in the window that makes the Console rewrite a provider's JSON or TOML
file is a direct mutation, not a provider-native operation.** A verified provider-native
operation that needs no recovery data of the Console's stays available with the vault locked,
under its own provider's capability and security contract. Reading, the terminal, native
sessions and managed operations are never disabled by a locked vault.

## 6. Interface (IF-VAULT)

Operations of the backend. The page reaches them through commands that carry identifiers only
(CTL-01, CTL-03); none takes a path, content or a key.

| Operation      | Input                            | Output to the page                                                | Permission                                                                                                                                     | Errors                                                                                                                                           |
| -------------- | -------------------------------- | ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `status`       | —                                | The state of SM-VKEY; custody; counts; consent on record          | Read                                                                                                                                           | —                                                                                                                                                |
| `list`         | A cursor, a limit ≤ 50           | Index entries                                                     | Read                                                                                                                                           | —                                                                                                                                                |
| `consent`      | The classes asked for            | Granted or refused                                                | **Native dialog** (§7)                                                                                                                         | `dialog_unavailable`                                                                                                                             |
| `create`       | —                                | The vault's state                                                 | After `consent`; internal to the first operation that needs it                                                                                 | `vault_location_unsafe`, `no_custody`, `locked`, `storage_failed`                                                                                |
| `seal`         | Internal: called by `apply`      | —                                                                 | Not callable by the page                                                                                                                       | `locked`, `no_custody`, `key_missing`, `storage_failed`, `vault_full`, `entry_too_large`, `seal_unverified`                                      |
| `plan_restore` | Record identifier                | A plan (07 §5.6) with a redacted diff                             | Read (pure); needs the key                                                                                                                     | `record_unknown`, `locked`, `corrupt`, `unsupported_format`, `source_missing`, `source_identity_mismatch`, `already_present`, `restore_conflict` |
| `apply`        | Plan identifier                  | An outcome of SM-WRITE                                            | Write; **native dialog always** (`executes_code`, 11 §4)                                                                                       | Those of IF-CONFIG `apply` (07 §8); `record_busy`                                                                                                |
| `reveal`       | Record identifier                | Nothing to the page: the content is shown in a native dialog      | **Native dialog**, which itself displays the content and offers to copy                                                                        | `record_unknown`, `locked`, `corrupt`                                                                                                            |
| `delete`       | Record identifier, or all        | The new list                                                      | **Native dialog** naming what is destroyed                                                                                                     | `record_unknown`, `record_busy`                                                                                                                  |
| `keep_copy`    | Item identifiers, at most twenty | The new list                                                      | Consent for this class; and, **for every request, a native dialog that lists the exact entries**, computed by the backend from the identifiers | `confirmation_refused`, `dialog_unavailable`, `locked`, `no_custody`, `key_missing`, `vault_full`, `entry_too_large`, `storage_failed`           |
| `drop_copy`    | Record identifier                | The new list                                                      | **Native dialog**, always                                                                                                                      | `record_unknown`, `record_busy`, `confirmation_refused`                                                                                          |
| `plan_recover` | Record identifier                | A plan with a redacted diff; then `apply`, with the native dialog | Read (pure); needs the key                                                                                                                     | Those of `plan_restore`                                                                                                                          |
| `rotate`       | —                                | Progress                                                          | **Native dialog**                                                                                                                              | `locked`, `storage_failed`                                                                                                                       |
| `recover`      | —                                | The state                                                         | **Native dialog** with a native input for the code                                                                                             | `recovery_unavailable`, `recovery_failed`                                                                                                        |
| `export`       | Record identifiers               | Nothing to the page                                               | **Native dialog** and a native file chooser; not offered at first (DN-29)                                                                      | `export_unavailable`                                                                                                                             |
| `import`       | —                                | The imported entries, as metadata                                 | **Native dialog** and a native file chooser; not offered at first                                                                              | `import_invalid`, `import_duplicate`                                                                                                             |

What the native dialog is worth here is stated in [ADR 0030](../adr/0030-corrections-to-adr-0028.md)
(proposed): it stops a slip and a defect confined to the configuration view. It is not a
boundary against code running in the page, which can already type into a terminal.

### 6.1 What authorises a vault operation

A confirmation shown by the page is never what authorises an operation of the vault. The page
is untrusted input (11 §2): code running in it can call any command the page can call, and can
answer its own confirmation.

| Rule                                                                                                                                                                                                                                     |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Every operation that **creates** a record (sealing a disabled entry, keeping a recovery copy), **decrypts** one (restoring, recovering, revealing, exporting) or **destroys** one is authorised by a native dialog opened by the backend |
| The dialog shows what the backend computed from the identifiers in the request: the exact entries, their file and scope. Its acceptance is valid for that request and those entries only                                                 |
| A plan that would seal a record takes the native dialog whatever its risk class: disabling a hook that cannot block anything still stores its content                                                                                    |
| A request carries identifiers and nothing else: no path, no content, no key. The number of entries in one request is bounded, so that a dialog can be read                                                                               |
| The class consent of §7 says that this kind of record may exist. It never stands in for the confirmation of an operation                                                                                                                 |

What this does and does not do, in the terms of ADR 0030 (proposed):

| It does                                                                                                     | It does not                                                                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Stop the vault's own interface from making, opening or removing a record at the request of code in the page | Contain a compromised page. That code can type into a terminal (ADR 0005), and from a shell it can edit the provider's files, ask an unlocked keyring for the vault's key, and read or write the vault's files directly |
| Stop a slip                                                                                                 | Stop a user who accepts a dialog he did not read                                                                                                                                                                        |

A native dialog alone is not a containment of a compromised page, and nothing in this document
says it is. A boundary would need the configuration view and the terminal to be separate web
views (ADR 0030; DN-22).

## 7. Consent

Nothing is sealed before the user has consented, in a native dialog, to the class of record
about to be made.

| The dialog states                    | Text, in substance                                                                                                                                                           |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| What is stored                       | "The hook you disable is removed from the agent's file and a copy is kept so that it can be put back exactly. A hook can contain a password or a token; the copy will too."  |
| Why                                  | "This agent has no switch for one hook."                                                                                                                                     |
| Where                                | "On this computer only, in Wyss Console's private data."                                                                                                                     |
| Whether it is encrypted, and by what | "Encrypted. The key is kept by your system keyring" or "is derived from a passphrase you will type". And: "Programs running as you can still reach it while it is unlocked." |
| How long                             | Per DN-27: "Until you restore it or delete it."                                                                                                                              |
| Whether it is synchronised           | "Never. Your other computer will see the hook gone and will not be able to restore it."                                                                                      |
| How it is restored                   | "From the Configuration view, on this computer."                                                                                                                             |
| How it is deleted                    | "From the same view. Deleting it is final. Copies in backups or snapshots of your disk are not reached by that."                                                             |
| If the key is lost                   | "If your keyring is reset or lost, the kept hooks cannot be recovered" and, if DN-26 allows it, the offer of a recovery code                                                 |

| Consent is recorded as                                                                                        |
| ------------------------------------------------------------------------------------------------------------- |
| Policy version; classes consented to; custody in use; time. Kept in the vault's consent record, metadata only |

| Consent is asked again when                                                             | Why                                            |
| --------------------------------------------------------------------------------------- | ---------------------------------------------- |
| The policy version changes: what is stored, where, how it is protected, how long        | The earlier consent was to something else      |
| The custody changes (keyring to passphrase, or back)                                    | The protection changed                         |
| A class is used for the first time (`conflict_version`, `undo_entry`)                   | A whole file, or a different retention         |
| A kind of entry is kept for the first time whose fields are credentials by design (MCP) | The likelihood of a secret is of another order |
| The vault was deleted and is created again                                              | Nothing remains of the earlier agreement       |

It is **not** asked again for each entry. Each operation still goes through its own plan and
confirmation (11 §4), and the plan says in one line that the entry will be kept in the vault. A
consent is not a setting: there is no "always" for an operation, and no consent on behalf of a
future policy.

The consent is to a class of record existing. Every operation that makes a record, opens one
or removes one is confirmed on its own, in a native dialog (§6.1).

## 8. Lifecycle

### 8.1 Record states (SM-VAULT)

| State                | Meaning                                                                                               |
| -------------------- | ----------------------------------------------------------------------------------------------------- |
| `sealing`            | The payload is being encrypted in memory. Nothing on disk                                             |
| `stored`             | The record file is written, flushed and named                                                         |
| `verified`           | Read back, decrypted, and found to rebuild F0 exactly (§3.2, rule 4)                                  |
| `holding`            | The entry is out of the provider's file. The record is its only copy                                  |
| `restoring`          | A restoration plan is being applied                                                                   |
| `restored`           | The entry was put back and seen in the file. The record is kept until the user deletes it (§8.3)      |
| `redundant`          | The entry is in the provider's file again without a restoration (put back by hand, or by a provider)  |
| `purged`             | The record file is removed; a tombstone remains                                                       |
| `corrupt`            | The tag does not verify, or the file is truncated                                                     |
| `unsupported_format` | A newer record format                                                                                 |
| `unlisted`           | A record file with no index entry, or with a tombstone                                                |
| `orphaned`           | The record is sound, but what it belongs to is gone: its source file, or the project it was in (§8.4) |

| From        | To          | Initiator        | Guard                                                                                                                                           | Side effect                                                               | On failure                                                                                                                                  |
| ----------- | ----------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| —           | `sealing`   | `apply`          | Consent on record for this class; SM-VKEY `ready`; the plan confirmed **in a native dialog** (§6.1); bounds not reached                         | Intent journalled with the record identifier                              | `locked`, `no_custody`, `key_missing`, `vault_full`, `entry_too_large`: nothing written, provider file untouched                            |
| `sealing`   | `stored`    | Engine           | Encryption succeeded                                                                                                                            | Unnamed file written, flushed, named, directory flushed                   | `storage_failed` (disk full, I/O error): no record, provider file untouched                                                                 |
| `stored`    | `verified`  | Engine           | Read back; tag verifies; `F1[..offset] + span + F1[offset..]` hashes to h0                                                                      | Index entry written                                                       | `seal_unverified`: the record file is removed; provider file untouched                                                                      |
| `verified`  | `holding`   | Engine           | SM-WRITE ended `applied`                                                                                                                        | Index updated; outcome journalled                                         | SM-WRITE ended otherwise: see §8.2                                                                                                          |
| `holding`   | `restoring` | User             | Plan confirmed; native dialog accepted; no other operation on this record                                                                       | Intent journalled                                                         | `record_busy`; a refused dialog writes nothing                                                                                              |
| `restoring` | `restored`  | Engine           | SM-WRITE ended `applied` and the entry's digest is found in the file                                                                            | Index updated with the time                                               | Any other outcome: back to `holding`; the record is untouched                                                                               |
| `restored`  | `purged`    | User             | The user asks to delete the record; native dialog accepted; the source is read again at that moment **and** the entry is still seen in the file | Record file removed; tombstone written                                    | The entry is gone again (a provider wrote an older version back): nothing is removed, the record is back to `holding`, and the user is told |
| `holding`   | `redundant` | Outside change   | The entry's digest reappeared in the source                                                                                                     | The window says the hook is active again                                  | —                                                                                                                                           |
| `redundant` | `purged`    | User             | The user acknowledged it; native dialog accepted; the entry is still present when the source is read again                                      | As above                                                                  | The entry went again: back to `holding`                                                                                                     |
| `holding`   | `purged`    | User             | Native dialog: "this is the only copy"                                                                                                          | Record file removed; tombstone; journalled                                | —                                                                                                                                           |
| any         | `corrupt`   | Engine           | The tag fails                                                                                                                                   | Listed; never used; never removed by the engine                           | —                                                                                                                                           |
| any         | `unlisted`  | Engine, at start | A record file without an index entry                                                                                                            | Listed apart; restoring it takes the native dialog with its content shown | —                                                                                                                                           |

Two invariants hold the lifecycle together:

- **VA1.** An entry is removed from a provider's file only after its record is `verified`. A
  record is removed only after the entry has been seen in the provider's file, or by the user's
  explicit deletion. At every instant the entry exists in at least one of the two places, as far
  as the filesystem and the device honour a flush (28 §2).
- **VA2.** A record is never the input of an automatic action. Restoring, purging a record whose
  entry is not in the file, importing and revealing each need the user.

### 8.2 Interrupted transitions

Order of durable steps when an entry is disabled on rung 3: intent in the journal → record
written and flushed → record verified → index entry → SM-WRITE of the provider's file (07 §5.4)
→ index `holding` → outcome in the journal.

| The process stopped                                 | Record     | Provider file | At start                                                                                                                                     |
| --------------------------------------------------- | ---------- | ------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| After the intent, before the record was named       | None       | h0            | `not_applied (recovered)`                                                                                                                    |
| After the record was named, before it was verified  | Unverified | h0            | The record is verified now; if it verifies it is purged as unused, if not it is removed; `not_applied`                                       |
| After verification, before the exchange of SM-WRITE | Verified   | h0            | The entry is in the file: the record is purged as unused; `not_applied (recovered)`                                                          |
| After the exchange, before the outcome              | Verified   | Planned       | Index set to `holding`; `applied (recovered)`; the file-side repair of 14 §4 runs as for any write                                           |
| The same, and the file is neither h0 nor planned    | Verified   | Other         | The file is read: entry present → record purged as unused; entry absent → `holding`, and the user is told that the file also changed outside |
| The index was lost or is unreadable                 | Present    | Any           | Rebuilt: each record is decrypted when the key is available; its entry present in the source → `redundant`; absent → `holding`               |
| An intent is open and the custody does not answer   | Present    | Any           | Repair waits (14 §4, `repair_pending`): nothing is purged, the source is read only, and the user is told that unlocking is needed            |

SM-WRITE ending without `applied`:

| Outcome of the write                                        | The entry is                       | The record                                                                        |
| ----------------------------------------------------------- | ---------------------------------- | --------------------------------------------------------------------------------- |
| `not_applied`, `lost_race`, `failed`                        | In the file                        | Purged as unused                                                                  |
| The same, but an outside change removed the entry meanwhile | Gone, by someone else              | Kept, `holding`: it is the copy the user asked for; he is told how the entry went |
| `superseded_outside`                                        | Re-read to find out                | Present → purged as unused; absent → `holding`                                    |
| `conflict_preserved`, `unverified`                          | Unknown until the user resolves it | Kept, `holding`; never purged by the engine while the conflict is open            |

Restoration: intent → key fetched, record decrypted → SM-WRITE inserts the span → outcome →
index `restored`. A stop anywhere leaves the record in place: restoration never modifies or
removes a record before `restored`, so the worst result of an interrupted restoration is an
entry that is in the file and in the vault, which is `redundant`.

### 8.3 Other failures, one by one

| Event                                   | Behaviour                                                                                                                                                                                                                                             |
| --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Encryption fails                        | Nothing on disk; the operation is `failed`; the provider's file is untouched                                                                                                                                                                          |
| Storage fails, disk full                | The same. A full disk also stops the journal, and so every mutation (`journal_unavailable`)                                                                                                                                                           |
| Record corrupted later                  | `corrupt`. The entry it held is lost unless a backup of the vault and its key exist. Said to the user in those words                                                                                                                                  |
| Keyring unavailable at plan time        | The plan is refused before confirmation (`locked`, `no_custody`): the user is never asked to confirm something that cannot be kept                                                                                                                    |
| Keyring unavailable at restore time     | The record stays; `reveal` and `plan_restore` wait for the key. Nothing is lost                                                                                                                                                                       |
| Crash, power loss                       | §8.2. Flushes: record before the provider's file is touched; index after                                                                                                                                                                              |
| The provider's file was changed outside | Restoration follows §3.7: exact if the file's digest is the one recorded after the change; otherwise only at the one unambiguous place, as a diff the user confirms, labelled "not byte-identical"; otherwise nothing is written and the record stays |
| Concurrent restoration                  | One operation per record: the second is `record_busy`. Two Console processes are serialised by the lock of the record's file; if the lock is not shared, the second finds the entry present: `already_present`                                        |
| The restore target was removed          | `source_missing`. The record stays. `reveal` lets the user take the content and place it by hand. The Console does not create a provider's file                                                                                                       |
| The source moved (project renamed)      | `source_identity_mismatch`. The same. Restoring into another file is not offered                                                                                                                                                                      |
| The provider's format changed           | The adapter no longer recognises the place: `unsupported_format` for restoration; the record stays; `reveal` works                                                                                                                                    |
| The same entry is already there         | `already_present`: nothing is written; the record becomes `redundant`                                                                                                                                                                                 |
| Another entry occupies its place        | `restore_conflict`: both are shown; nothing is written                                                                                                                                                                                                |
| The record expired                      | Only if DN-27 gives records an expiry: see below                                                                                                                                                                                                      |
| The user asks to delete                 | One record, or all: native dialog that names each entry by metadata and says it is the only copy. "Restore all first" and "reveal" are offered beside it                                                                                              |

Retention (DN-27), proposed:

| Class and state                    | Retention                                                                                                                                                                                                                                                                                                                                                             | Why                                                                                                                                                              |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `disabled_entry`, `holding`        | **No expiry.** The window shows its age and reminds the user at a threshold he can set. The owner's direction (2026-10-13): no deletion by age at first                                                                                                                                                                                                               | The record is the user's configuration. Deleting it by age would destroy configuration without his act                                                           |
| `disabled_entry`, `restored`       | **No expiry.** Kept until the user deletes it. Deleting is offered once the restoration is verified, that is, the entry is seen in the file, and the file is read again when he confirms. Until then the record is listed and counted in the vault's strip, with its age                                                                                              | The owner's decision (2026-10-13): no record is deleted on age alone. A provider that writes an older version back would otherwise leave no copy (28 §4, case 5) |
| `redundant`                        | **No expiry.** Kept until the user acknowledges it and deletes it                                                                                                                                                                                                                                                                                                     | The content is in the file, and removing the copy is still his act                                                                                               |
| `conflict_version`                 | Until the user resolves the conflict, with a reminder at every start. No expiry                                                                                                                                                                                                                                                                                       | It is a version of his file that exists nowhere else                                                                                                             |
| The version of a recovery incident | Not a record, and not a retention: recovery evidence, in clear, kept until it is sealed or the user resolves it. **No timeout.** The engine never destroys it (07 §5.4; DN-28)                                                                                                                                                                                        | The owner rejected destroying the only recoverable version after an arbitrary time, and rejected treating a private directory as protection                      |
| `undo_entry`                       | **No expiry.** Kept for as long as it is the only recoverable version of what was deleted or changed. It stops being required only at a verified terminal state: the undo was applied and the entry is seen in the file (`restored`), or the same content is in the file again (`redundant`). It is then removed by the user's explicit clean-up, and by nothing else | The owner's decision (2026-10-13): no arbitrary expiry; the only recoverable version of a configuration is never destroyed automatically                         |
| `recovery_copy`                    | No expiry. Purged when the user stops keeping it, or when the same entry is disabled into the vault                                                                                                                                                                                                                                                                   | It exists by his choice and ends by his choice                                                                                                                   |

**The rule behind the table** (the owner's decisions of 2026-10-13). No record of any class is
removed because of its age. Retention follows what still depends on a record, and a terminal
state counts only when it was verified by reading the source again.

| A record is        | When                                                                                                                                                                                                                         | Who may remove it                                                                                                                                                                               |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Never needed       | The operation that made it did not apply, and its entry is verified in the provider's file (§8.2, "purged as unused")                                                                                                        | The engine, at once: the record was never the only copy of anything                                                                                                                             |
| Required           | It is the only recoverable version of an entry or of a file; or an intent, a conflict or a recovery incident on its source is open; or its state cannot be verified (the custody does not answer, the source cannot be read) | The user alone, by the explicit deletion of §8.1 with its "only copy" dialog. **Never the engine**                                                                                              |
| No longer required | A terminal state was verified: the content it holds is in the provider's file (`restored`, `redundant`) and nothing open depends on it                                                                                       | The user, by explicit clean-up of one record or of all that are in this state, listed by metadata. Each is verified again at the moment of removal; one that fails is `holding` again and stays |

Storage, since nothing expires:

| Matter                                               | Behaviour                                                                                                                                                                                                                                                                                                                |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Visibility                                           | The vault's strip shows, per class and state: the count, the bytes, the oldest age, and how many records are no longer required and can be cleaned up. The bounds (14 §3) and the free space of the filesystem are shown beside them                                                                                     |
| Warnings                                             | Shown when the count or the bytes pass a fraction of a bound, and when the free space is less than the next operation would need: its record and, for a direct write, the sealing placeholder (07 §5.4). The fractions are fixed with the fixtures (OQ-22). A warning offers the explicit clean-up and removes nothing   |
| A bound is reached, or there is no room              | No new record is made: `vault_full` or `storage_failed`, before anything is touched. The operation that needs one is not offered and says why: disabling into the vault, a delete or an edit with undo, a direct write that needs its placeholder. **Nothing already held is removed to make room**, the oldest included |
| What still works                                     | Listing, revealing, the explicit clean-up and the explicit deletion. A restoration is a direct write and has that write's preconditions: where they cannot be met it is refused and its record stays                                                                                                                     |
| A delete or an edit whose undo record cannot be made | Whether it is refused or offered without undo belongs to the contract of authoring (DN-12). The engine never performs it silently without the record                                                                                                                                                                     |

Deleting everything the engine keeps (FR-DAT-05) no longer loses "history only". It is split:
history (journals, last-seen index) can be deleted freely; the vault is deleted by its own
action, which lists what would be destroyed (14 §8).

### 8.4 Orphaned records

A record is orphaned when it is sound and what it belongs to is not there any more. The
owner's direction asks that such records be neither lost sight of nor deleted behind his back.

| How a record becomes orphaned             | Detected                                         | State                                                 |
| ----------------------------------------- | ------------------------------------------------ | ----------------------------------------------------- |
| Its source file was deleted               | At start and at every observation of that source | `orphaned`, reason `source_missing`                   |
| Its project was moved, renamed or removed | The same; the canonical path no longer matches   | `orphaned`, reason `source_moved`                     |
| The index lost its entry                  | At start, by listing the record files            | `unlisted`                                            |
| Its key is not in the custody             | When the key is next asked for                   | The vault is `key_missing` (§5.2)                     |
| The consent record was lost               | At start                                         | Records stay; the next sealing asks for consent again |

| Protection                                                                                                                                                                        |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| An orphaned record is never purged by the engine, at any age                                                                                                                      |
| It is listed with its reason, and counted in the vault's strip, so that it cannot sit unseen                                                                                      |
| The user can reveal it, delete it, or, for a project that moved, point to the project in a native dialog; its restoration is then planned as usual and its content is shown whole |
| Before a project is removed from the Console's list, and before the engine's history is deleted, the user is told how many records are bound to it                                |
| Removing the application leaves the vault and its key where they are; a later installation finds them                                                                             |

## 9. Two computers

### 9.1 Where the vault may be, and what is checked

The vault is per installation: its own identifier, its own key, records bound to both. A
record copied to the other computer by any means is inert there: that computer has no key for
it, and its header names another installation.

**The criterion is open (DN-31).** An earlier text of this section refused the vault whenever
any ancestor carried the folder marker of a recognised replication tool. The inspection of
2026-10-13 (30 §6) showed what that rule is worth: on both of the owner's computers the whole
home directory is the root of a replicated folder, so the rule refuses everything, while the
state directory is in fact named by an ignore rule. The owner's decision of that day: **a
marker in an ancestor is not proof that the vault is replicated**, and an observed ignore rule
is **evidence, not an unconditional guarantee**. Four situations are distinct:

| Situation                                            | What it means for the vault's ciphertext, and for a file in clear in the transaction directory                                                                                             | What the Console can know                                                                                                                                                | On the owner's two computers (30 §6)                                                                              |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------- |
| 1. Inside a synchronised root                        | By itself, nothing: a tool could carry the directory                                                                                                                                       | The marker of a recognised tool in an ancestor                                                                                                                           | Yes: the home directory                                                                                           |
| 2. Excluded by an effective ignore rule of that tool | Not carried, **for as long as the rule stands**. The rule is a file that the user and other tools edit; it is kept per device; a folder can be reset or shared again without it            | That a rule naming the directory exists now, if the Console reads the tool's ignore file. Not that the tool honours it in every case (30, TV-29), nor that it will stand | Yes; observed, not tested                                                                                         |
| 3. Captured by a filesystem snapshot                 | A copy the Console cannot see, bound or remove. Ciphertext, for the vault. **A file in clear, if the snapshot is taken while a write is in progress**, for as long as the snapshot is kept | Hardly anything without privilege                                                                                                                                        | Yes: the home subvolume has a snapshot configuration. What it captures and for how long is unverified (30, TV-28) |
| 4. Another backup or replication mechanism           | The same as 3, or as 1                                                                                                                                                                     | Nothing                                                                                                                                                                  | None found in a short list; not a finding of absence                                                              |

**The conflict this exposes**, which this design does not resolve:

| Requirement                                                                     | It asks for                                                                                                                                                                                                                                                                              |
| ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The exchange of 07 §5.4 is atomic                                               | The transaction directory on the provider file's filesystem. On a filesystem with subvolumes, in the same subvolume: an exchange across two of them is refused, as a rename is (30 §8: tested with a mount between them on both computers; read in the kernel's source for a nested one) |
| A copy in clear exists only for the time a write strictly needs (DN-28; CTL-40) | That nothing copies the transaction directory during that time                                                                                                                                                                                                                           |
| The home subvolume is snapshotted                                               | Every directory of that subvolume, the transaction directory among them                                                                                                                                                                                                                  |

The three do not hold together on the owner's computers. Putting the transaction directory in
a subvolume of its own, which no snapshot captures, is **not a solution**: the exchange is
refused from there (30 §8), so that placement gives up the first requirement to gain the
second. The vault's records are ciphertext in every copy named
above; the files of the transaction directory are not.

**Until DN-31 is decided, the vault is not created and no direct write is available.** What
does not depend on that decision:

| What is required                                                                                                                                              | What is checked                                                                                                                 | What is not                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| The Console's state directory, with the vault and the transaction directory in it, is not replicated, and a file in clear in it is not captured by a snapshot | **Open: DN-31.** No check is specified. A marker above the directory is not treated as proof, nor an ignore rule as a guarantee | Whatever is decided: a tool that leaves no trace the Console can read; a backup; a snapshot taken by the administrator |
| It is not shared with another user                                                                                                                            | Modes 0700 and 0600; the path rule (CTL-10)                                                                                     | An administrator                                                                                                       |

Where the criterion that is decided is not met, the vault is `unsafe`: nothing is created or
opened there, and every operation that needs it, direct writes included, is refused. Where it
is met, **nothing is proved**: the design does not say the vault cannot be copied, and stores
only ciphertext there. The same holds for the transaction directory, whose files are in clear
while they exist (28 §7).

The deployment assumption is stated to the user when the vault is created: this directory is to
be excluded from replication, from snapshots and from sharing (OQ-32). On the owner's two
computers the snapshot part of it does not hold today (30 §6).

### 9.2 What each computer sees

The provider's file is replicated; the vault is not.

| Case                                                                | Computer A (disabled it)                                                                                                                                                                                                                           | Computer B                                                                                                             |
| ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| A hook disabled on A, rung 3                                        | `holding`; restorable                                                                                                                                                                                                                              | Receives a file without the hook. Shows "removed outside this computer". It cannot restore it and says where it can be |
| Restored on A                                                       | `restored`                                                                                                                                                                                                                                         | Sees the hook appear, as an outside change                                                                             |
| B puts the same hook back by hand                                   | Sees it reappear: `redundant`                                                                                                                                                                                                                      | —                                                                                                                      |
| A's vault is lost (disk, key)                                       | The hook is gone on both. Neither can restore it from the Console                                                                                                                                                                                  | The same                                                                                                               |
| The file was edited on B while the hook was held on A               | Restoration is planned against the merged file, by `path` and `anchors`                                                                                                                                                                            | —                                                                                                                      |
| Both changed the file while disconnected                            | If the replication tool leaves a conflict copy the Console recognises, mutation is blocked, restoration included, until it is resolved outside. A tool that resolves the conflict by itself leaves nothing to see: the result is an outside change | The same                                                                                                               |
| B's older version replaces A's file                                 | The hook is back: `redundant`; A says its change was replaced                                                                                                                                                                                      | —                                                                                                                      |
| A disabled on rung 1 or 2 instead                                   | Nothing in the vault                                                                                                                                                                                                                               | Reads the state from the file and can restore it there (16 §2.2)                                                       |
| A recovery copy of an entry is kept on A; the entry is deleted on B | A sees it removed from outside and can recover it from its copy                                                                                                                                                                                    | Cannot recover it: B holds no copy, unless it kept its own                                                             |

The asymmetry is the price of rung 3 and is said in the consent dialog: a hook held in A's
vault can only come back from A. Replication history (versions kept by the replication tool) and
filesystem snapshots are **not** a recovery mechanism of this design; they may or may not hold
the entry, and the Console neither reads them nor counts on them.

### 9.3 Export and import (designed, not offered at first: DN-29)

| Aspect                        | Contract                                                                                                                                                                                                      |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purpose                       | Carrying a held entry to the other computer, by the user's act (FR-CFG-15)                                                                                                                                    |
| Form                          | One file. The selected records are decrypted and sealed again under a key derived from a passphrase typed in a native dialog. The header names the exporting installation, the time and the number of records |
| Key transfer                  | The passphrase travels by the user's own means. It is never stored, never placed in the file, never shown again                                                                                               |
| Integrity                     | Authenticated as a whole and per record. A file that fails is refused whole (`import_invalid`)                                                                                                                |
| Import                        | Native file chooser; native dialog listing what the file holds, by metadata; each record sealed again under the local key, marked `imported` with its origin                                                  |
| Trust                         | **An imported record is untrusted executable content.** Restoring it always shows its full content in the native dialog, never a redacted view, and never claims to be "the hook you had"                     |
| Replay, duplicates            | A record identifier already in the index or among the tombstones is refused (`import_duplicate`)                                                                                                              |
| Binding                       | An imported record keeps its provider, kind and scope. Its source is matched by identifier, not by path; a project source must be chosen by the user among known projects                                     |
| Where the file may be written | Wherever the user chooses. The dialog warns when the chosen folder is replicated. The file is the user's from then on: the Console does not track or delete it                                                |

It is not offered in the first vault phase because each of these lines is a new way for content
to leave or enter, and the first phase can be useful without it.

### 9.4 Remote management (future)

A remote device (F10) sees vault metadata at most. Restoring, revealing, exporting and deleting
are **not** remote capabilities in the first remote design: each decrypts content or destroys
the only copy, and the native dialog that guards them is on the host. Granting any of them to a
remote device needs its own decision, with a step-up defined (16 §5).

## 10. Threats

Identifiers T-25 to T-41 and controls CTL-28 to CTL-40 are defined in
[11](11-SECURITY-THREAT-MODEL.md). For each, where the control is enforced and how it would be
checked.

| Threat                                                                                     | What the design does                                                                                                                                      | Enforced at                   | Checked by    | What remains                                                                                                                                              |
| ------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- | ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-25 Storage stolen or copied offline                                                      | Records are ciphertext; the key is not in the directory (CTL-28, CTL-29)                                                                                  | Record writer; custody        | TS-VLT-01, 02 | The strength of the keyring's own password, or of the passphrase                                                                                          |
| T-26 A backup, a snapshot or replication captures the vault                                | Records are ciphertext. Where the vault may be is judged by the criterion of §9.1, which is open (CTL-30; DN-31). A backup or a snapshot is not prevented | Vault creation and start      | TS-VLT-03, 28 | A copy of the ciphertext together with a keyring that opens without a password                                                                            |
| T-27 A malicious process of the same user                                                  | **Nothing that holds.** It can ask the keyring, read memory, or edit the provider's file itself                                                           | —                             | —             | Accepted, as everywhere in this product (ADR 0018). The vault is not described as protection here                                                         |
| T-28 Another local user                                                                    | Modes 0700 and 0600; path rule; ciphertext                                                                                                                | Filesystem; CTL-30            | TS-VLT-04     | An administrator                                                                                                                                          |
| T-29 A compromised page                                                                    | The page gets metadata and redacted projections only; restore, reveal, delete, export, import and rotate take a native dialog (CTL-31)                    | Command types; backend        | TS-VLT-05, 06 | Code in the page can type into a terminal: it can edit files and call the keyring from a shell (ADR 0030)                                                 |
| T-30 A compromised native agent                                                            | None beyond T-27: it runs as the user                                                                                                                     | —                             | —             | Accepted and stated in the window where native sessions are described                                                                                     |
| T-31 A record is altered, swapped or replayed                                              | The header is authenticated and binds vault, installation, record and class; tombstones; an unlisted record is never used silently (CTL-32)               | Record reader; index          | TS-VLT-07, 08 | Someone who has the key can forge a valid record: that is T-27                                                                                            |
| T-32 A malicious imported record                                                           | Import validates, seals again, marks the origin; restoring imported content always shows it whole in the native dialog (CTL-35)                           | Import; restore               | TS-VLT-09     | A user who accepts a hook he did not read                                                                                                                 |
| T-33 Restoration into the wrong project or file                                            | The payload binds the source's identity; a mismatch refuses (CTL-32)                                                                                      | `plan_restore`                | TS-VLT-10     | —                                                                                                                                                         |
| T-34 Restoration ordered from a remote device                                              | Not a remote capability (CTL-38)                                                                                                                          | Gateway grants (future)       | TS-REM        | Designed with the remote phase                                                                                                                            |
| T-35 Content leaves by accident: a support bundle, a log, an export to a replicated folder | Bundles and logs are built from closed words and never read the vault; export is explicit and warns (CTL-36)                                              | Diagnostics builder; export   | TS-VLT-11, 12 | A user who exports into a shared place after the warning                                                                                                  |
| T-36 A secret survives its deletion                                                        | The record file is removed and a tombstone kept. **Nothing more can be promised** (CTL-37)                                                                | —                             | TS-VLT-13     | Copies in snapshots, backups and unallocated blocks stay readable to whoever also has the key. If a secret was exposed, the secret itself must be changed |
| T-37 The vault's path is replaced by a link                                                | Opened once, every ancestor checked, used by descriptor (CTL-30)                                                                                          | Vault open                    | TS-VLT-14     | T-27                                                                                                                                                      |
| T-38 The key is lost, and with it the only copy of a hook                                  | Said in the consent; a recovery code if DN-26 allows; nothing is sealed when the custody is not answering                                                 | Consent; SM-VKEY              | TS-VLT-15     | A user with no recovery code whose keyring is reset loses the held entries                                                                                |
| T-39 Protection is weakened without anyone deciding it                                     | No fallback of custody, of algorithm or to plaintext; a change of custody is a consent (CTL-29, CTL-34)                                                   | Vault service                 | TS-VLT-16     | —                                                                                                                                                         |
| T-40 A recovery copy becomes a standing duplicate of the user's hooks                      | Opt-in per entry, by an explicit act; never automatic; no "all entries"; a copy whose entry changed is marked (CTL-39)                                    | `keep_copy`; the capture rule | TS-VLT-21, 22 | A user who selects many entries and forgets them. Their old secrets stay in the vault and in its backups                                                  |

## 11. Audit

What the journal records about the vault: record created, verified, used for a restoration,
restored, purged, deleted by the user, found corrupt, found unlisted, key rotated, consent given
or refused, export and import. Each entry holds identifiers, the class, times and the outcome.
None holds content, a path the user wrote, or an unkeyed digest (CTL-08). The journal is
history: losing it loses no record, because the index and the records stand on their own.

## 12. Tests

Specified in [20 §3.7](20-VERIFICATION-AND-ACCEPTANCE.md) as TS-VLT-01 to TS-VLT-30. They are
written with the phase; none exists. What the set must show before the vault is relied on:

| Property                                                                                                                                                           | Tests                |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------- |
| No content of an entry is in clear in the files the Console writes (§5.3), after any operation. Swap, core dumps, snapshots and memory are outside what this shows | TS-VLT-01, TS-SEC-02 |
| VA1 under fault injection at every step of §8.2                                                                                                                    | TS-VLT-17            |
| Exact restoration: disable then restore is byte-identical; a changed file restores by position                                                                     | TS-VLT-18            |
| No fallback when the custody is absent, locked or emptied                                                                                                          | TS-VLT-15, 16        |
| Tampering, substitution, replay                                                                                                                                    | TS-VLT-07, 08        |
| The page never receives content or keys                                                                                                                            | TS-VLT-05, 06        |
| Rotation interrupted at every step leaves every record readable                                                                                                    | TS-VLT-19            |
| Two computers: the second one cannot restore and says so                                                                                                           | TS-VLT-20            |
| Recovery copies are made only by the user's act, and an entry removed outside is recovered by position                                                             | TS-VLT-21 to 23      |
| An orphaned record is never purged and is always listed                                                                                                            | TS-VLT-24            |
| Code in the page cannot create, open or remove a record without the native dialog                                                                                  | TS-VLT-25            |
| An ambiguous or missing place writes nothing                                                                                                                       | TS-VLT-26            |
| No store holds an unkeyed digest of a file or of an entry; the vault's digest keys are in no file in clear                                                         | TS-VLT-27            |
| Deleting history leaves held records restorable and recognisable                                                                                                   | TS-REC-08            |

Controls of the tests themselves (20 §2): a record written in clear on purpose must fail
TS-VLT-01; a build with the fallback enabled on purpose must fail TS-VLT-16.

## 13. Not verified, and not decided

| Item                                                                                           | How it is settled                                                                                                                                            |
| ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A secret service is present, unlocked and password-protected on the owner's two computers      | Present and unlocked, provided by GNOME Keyring: observed on 2026-10-13 (30 §6). Its password, and its behaviour locked, absent or emptied: OQ-38; 30, TV-14 |
| The key and decrypted content can be kept out of swap and core dumps in this process           | OQ-39                                                                                                                                                        |
| The state directory is outside everything replicated or snapshotted                            | **Not so on either computer** (30 §6): under a replicated root and named by an ignore rule; inside a snapshotted subvolume. OQ-32; DN-31                     |
| Removing a hook and pruning its empty containers leaves a file Claude Code loads as documented | RS1, as a control run; `[D]` today                                                                                                                           |
| Custody of the key                                                                             | DN-25                                                                                                                                                        |
| A recovery code                                                                                | DN-26                                                                                                                                                        |
| Retention                                                                                      | DN-27                                                                                                                                                        |
| Where a conflicting version is preserved                                                       | DN-28                                                                                                                                                        |
| Export and import                                                                              | DN-29                                                                                                                                                        |
| Opt-in recovery copies: scope and capture rule                                                 | DN-30                                                                                                                                                        |
| Where the state directory, the vault and the transaction directory may be                      | DN-31                                                                                                                                                        |
| The libraries for encryption, key derivation and the secret service                            | DN-04, in the phase                                                                                                                                          |
