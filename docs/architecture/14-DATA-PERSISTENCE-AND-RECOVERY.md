# 14 — Data, persistence and recovery

> **2026-10-22 consolidation:** this document retains dated design/evidence and detailed
> contracts. Its present-tense status/decision tables describe their stated baseline,
> not necessarily current implementation. Read the [current product set](../product/README.md),
> [verified gaps](../product/implementation-status.md) and [requirement crosswalk](../product/traceability.md)
> first. F8 is active P2/V8, ADR0051 Accepted, cap1; P1 remains available. New product
> direction does not supersede accepted security ADRs or turn proposed tests into PASS.

Domains J (records) and L (persistence) of [04](04-TARGET-ARCHITECTURE.md): who owns each
piece of state, where it lives, how it survives a crash and how it is undone. `[R]` marks what
exists; the rest is `[P]`.

## 1. Rule

Each piece of state has exactly one owner (FR-DAT-01). Everything else that shows it holds a
projection that can be thrown away and rebuilt. No two stores are authoritative for the same
fact.

## 2. Ownership

| State                                                | Owner                                                                                                                                            | Lives in                                                 | Others hold                                                |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------- | ---------------------------------------------------------- |
| An agent's configuration                             | **The provider's files**                                                                                                                         | `~/.claude/…`, `~/.codex/…`, project files, system files | The Console: a projection, hashes                          |
| An agent's sign-in                                   | The provider                                                                                                                                     | Its credential files                                     | Nobody. Managed Local Tools is handed one descriptor `[R]` |
| A provider's trust in a hook or project              | The provider                                                                                                                                     | Its own records                                          | The Console: read only                                     |
| A native agent's conversation                        | The provider                                                                                                                                     | Its session files                                        | Nobody                                                     |
| Managed sessions, bindings, leases, turns, approvals | **The authority** `[R]`                                                                                                                          | `~/.local/state/wyss-console/` (SQLite, schema 7)        | The window: projections                                    |
| The managed execution record                         | The authority `[R]`                                                                                                                              | The same store: an append-only envelope stream           | The window: pages of it                                    |
| The managed runtime's actual state                   | The authority, in memory `[R]`                                                                                                                   | —                                                        | The window                                                 |
| Desktop preferences                                  | The desktop `[R]`                                                                                                                                | `~/.config/wyss-console/preferences.json`                | —                                                          |
| Terminals and their content                          | The desktop, in memory `[R]`                                                                                                                     | —                                                        | —                                                          |
| Native session records                               | The desktop                                                                                                                                      | Registry file                                            | The overview                                               |
| Session profiles, projects                           | The desktop                                                                                                                                      | Profile and project files                                | —                                                          |
| Configuration history                                | The configuration engine                                                                                                                         | Journal                                                  | The History view                                           |
| A disabled entry, rungs 1 and 2                      | **The provider's file**: it stays in it (ADR 0029 §3, proposed)                                                                                  | The same file as the enabled entry                       | The Console: a journal entry with identifiers              |
| A disabled entry, rung 3                             | **The protected vault of the computer that disabled it** (27). It is the one case in which the Console holds a piece of the user's configuration | A sealed record in `vault/`                              | The provider's file: nothing. The other computer: nothing  |
| A recovery copy of an entry the user selected        | The protected vault of the computer where he selected it (27 §3.6). A duplicate by choice: the entry itself stays live in its file               | A sealed record in `vault/`                              | The provider's file: the entry itself                      |
| Last-seen index                                      | The configuration engine                                                                                                                         | Index file                                               | —                                                          |
| Window layout (tabs, panes)                          | The desktop                                                                                                                                      | Layout file (T2)                                         | —                                                          |
| Benchmarks                                           | The developer                                                                                                                                    | Outside the product                                      | —                                                          |

What the Console deliberately does not own: any agent's configuration in force, any conversation,
any credential. The one thing of the user's configuration it holds is an entry he asked it to
remove and keep (rung 3), and it holds it only to give it back.

## 3. Stores

All new stores live under the Console's private state directory, in a subdirectory of their
own, mode 0700, files 0600, never inside a provider's directory. Each file starts with a schema
version; a file with a newer version is left untouched and its feature is read only until the
Console is updated, as preferences already behave (`settings/service.rs`, state `Newer`) `[R]`.

The journals and the last-seen index hold no content of any configuration entry (ADR 0029 §2,
proposed). One store may: the protected restoration vault, and only sealed
([27](27-PROTECTED-RESTORATION-VAULT.md)).

| Store                         | Format                                                                                                       | Content                                                                                                                                                                                                                       | Bound                                                                                                            | Written                                                                     |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| Authority database `[R]`      | SQLite, one writer                                                                                           | Canonical managed state and its event stream                                                                                                                                                                                  | The authority's own rules                                                                                        | Transactionally                                                             |
| Preferences `[R]`             | JSON, one file                                                                                               | A closed set of preferences                                                                                                                                                                                                   | A few hundred bytes                                                                                              | Write beside, rename                                                        |
| Journal: mutations            | Append-only lines in dated segments                                                                          | Intents and outcomes: identifiers, keyed digests of files (§3.2), time, outcome (11 §5.5)                                                                                                                                     | 10,000 entries or 180 days, whichever first. **An intent without an outcome is never removed by a bound** (§4.1) | Append, flush. An intent is written **before** the file is touched          |
| Journal: observations         | A ring of its own                                                                                            | Outside changes seen: source, kind of change, digests of items added, removed, changed                                                                                                                                        | 2,000 entries                                                                                                    | Append, flush. Never evicts a mutation entry                                |
| Last-seen index               | One file per source                                                                                          | A keyed digest of the file, the time, one keyed digest per entry **present in the file at the last observation**                                                                                                              | One per known source                                                                                             | Write beside, rename                                                        |
| Observation key               | One file                                                                                                     | A random value that keys the digests of the last-seen index and of the observations journal. Nothing of the vault depends on it (§3.2)                                                                                        | One                                                                                                              | Created once; never leaves the machine; removed with history                |
| Vault records                 | One file per record, written once                                                                            | Sealed: the removed span of a disabled entry and what places it back; an authenticated header of identifiers (27 §3, §4)                                                                                                      | 1,000 records; 256 KiB each. **Reaching the bound refuses new records and never removes one**                    | Unnamed, flushed, named, directory flushed. Never modified afterwards       |
| Vault index                   | One file                                                                                                     | Per record: identifiers, class, state, times, keyed digest; tombstones                                                                                                                                                        | One; tombstones 5,000 or 180 days                                                                                | Write beside, rename. Rebuilt from the records if lost                      |
| Vault consent                 | One file                                                                                                     | What was consented to, the policy version, the custody, the time                                                                                                                                                              | One                                                                                                              | Write beside, rename                                                        |
| Vault key                     | **Not a file of the Console**: an item in the desktop's secret service, or derived from a passphrase (DN-25) | 32 random bytes                                                                                                                                                                                                               | One, two during a rotation                                                                                       | By the custody                                                              |
| Vault keys record             | One sealed record in the vault                                                                               | Two random keys of the vault's own: one for the digests inside vault records and the vault index, one for the file digests of the mutations journal (§3.2)                                                                    | One                                                                                                              | Sealed under the vault key; rewritten at a rotation, its two keys unchanged |
| Private transaction directory | `txn/`, mode 0700, on the same mount as the files it serves                                                  | During a write: the staged new content, then the displaced old file, in clear (07 §5.4). After a final outcome: nothing, except in a recovery incident (07 §5.4)                                                              | One file per open write                                                                                          | By the protocol of 07 §5.4 only                                             |
| Native registry               | One file                                                                                                     | Session records                                                                                                                                                                                                               | 500 sessions or 90 days                                                                                          | Write beside, rename                                                        |
| Profiles                      | One directory per profile, one file per version                                                              | 09 §4.1: references and names                                                                                                                                                                                                 | 256 profiles, 20 versions each                                                                                   | Write beside, rename; a pointer file names the current version              |
| Projects                      | One file                                                                                                     | Folder paths and defaults                                                                                                                                                                                                     | 256 projects                                                                                                     | Write beside, rename                                                        |
| Layout                        | One file, `layout.json` (05 §15)                                                                             | Tabs, the split tree, pinned titles, working directories, and for a pane that ran an agent its agent, project and profile identifiers. Never terminal content, program titles, command lines, environment or any confirmation | One                                                                                                              | Write beside, rename                                                        |
| Generated session files       | Not in the state directory                                                                                   | A profile's generated files for one session                                                                                                                                                                                   | The life of the session                                                                                          | Where and whether: DN-20                                                    |

Bounds are proposals to be fixed with the fixtures (OQ-22). Reaching a bound removes the oldest
entries first, in every store but the vault. Outside the vault, nothing the user's
configuration depends on is in any of these stores: a bound or a deletion there loses history.
**The vault is the exception**: a record can be the only copy of an entry, so it is never
evicted, and its deletion is an act of its own (§8).

The Console's own stores are written by the Console alone and use the discipline
`settings/store.rs` already follows `[R]`: a new file beside the old one, flushed, renamed over
it, the directory flushed. A provider's file is replaced by the stronger protocol of 07 §5.4,
because the provider writes it too.

### 3.1 Why the journal is not in the authority's database

AD-11 and decision D8: the authority must not learn of user configuration (C-02), and its
stream is the record of managed execution with integrity checks tied to its canonical rows
(ADR 0017) `[R]`. A configuration entry there would have no canonical row behind it.

### 3.2 Keys, and what depends on each

A digest kept by the Console is always keyed. Which key, and where that key lives, decides what
the digest is worth and what deleting it breaks.

| Key                | Lives                                           | Keys                                                                         | Worth                                                                                                                                                                         | Deleting it                                                                                                                           |
| ------------------ | ----------------------------------------------- | ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| Observation key    | A file in the state directory                   | Digests of the last-seen index and of the observations journal               | Keeps those digests from being compared across installations or after they leave the machine. **Nothing against someone who has the state directory**: the key is beside them | Loses the index, which is rebuilt at the next read. Nothing of the vault depends on it                                                |
| Vault key          | The custody (27 §5): a keyring, or a passphrase | Seals every vault record, the vault keys record included                     | What the custody is worth                                                                                                                                                     | Every record is unreadable: held entries are lost (27 §5.4)                                                                           |
| Vault digest key   | Inside the sealed vault keys record             | `entry_digest` and neighbour anchors of vault records and of the vault index | As the vault key: an index digest cannot be used to confirm a guess at a held entry without it                                                                                | Only with the vault. Not rotated on its own: anchors are digests of entries the record does not contain                               |
| Journal digest key | Inside the sealed vault keys record             | The file digests of the mutations journal                                    | As the vault key: a journal entry cannot be used to confirm a guess at a file's content without it                                                                            | Only with the vault, and only when no intent is open. Earlier journal entries stay as history whose digests can no longer be compared |

Consequences. Deleting history (§8) removes the observation key and no key the vault uses, so a
held record is recognised, placed and restored as before. Rotating the vault key rewrites the
vault keys record and leaves the two keys inside it unchanged, so digests stay comparable.
Recognising that a held entry is back in its file needs the vault digest key, and so the
custody: while it is locked, that state is shown as not known. The digests of the last-seen
index cover entries that are in the provider's file, in clear, on the same disk; a digest is
dropped from the index when its entry is no longer in the file.

## 4. Crash consistency

A change to a provider's file follows SM-WRITE (07 §5.4), which is the one normative
description. During a write the staged content, and then the displaced old file, are under the
operation's name in the private transaction directory. At start the engine takes every intent
that has no outcome and looks at the target and at the transaction directory. h0 is the digest
the plan was computed from; hp the digest the plan would produce; both are in the intent as
keyed digests, with the source's identity and the transaction name. Comparing them needs the
journal digest key, and so the vault's custody.

| The process stopped                                                                                            | Target | File under the transaction name | At start                                                                                                                                                                                                                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | ------ | ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| After the intent, before the staged file was named                                                             | h0     | None                            | Outcome `not_applied (recovered)`                                                                                                                                                                                                                                                                           |
| Between naming the staged file and the exchange                                                                | h0     | hp                              | The file is the Console's own staged content: removed (W1). `not_applied (recovered)`                                                                                                                                                                                                                       |
| After the exchange, during verification or settling                                                            | hp     | h0                              | The file is the content the plan was computed from: removed (W1). `applied (recovered)`, marked "settling did not complete"                                                                                                                                                                                 |
| The same                                                                                                       | hp     | Neither h0 nor hp               | A provider wrote into the old file. The target is still the Console's own content, so the rollback the protocol would have made is made now: the two names are exchanged back, the provider's version is the target, the Console's staged content is removed. `lost_race (recovered)`. No sealing is needed |
| At any step, and the target is neither h0 nor hp                                                               | Other  | hp                              | The file changed outside. The Console's staged content is removed. `not_applied (recovered)`                                                                                                                                                                                                                |
| The same                                                                                                       | Other  | h0                              | Removed (W1). `superseded_outside`: the engine re-reads and says whether the planned effect holds                                                                                                                                                                                                           |
| The same                                                                                                       | Other  | Neither                         | Sealed as a conflicting version, then removed. `conflict_preserved (recovered)`                                                                                                                                                                                                                             |
| During `preserving`, before the record was verified                                                            | Any    | The version                     | The record, if any, is verified; if it does not verify it is removed and sealing is done again. The file is removed only after a verified record                                                                                                                                                            |
| Sealing could not be done                                                                                      | Any    | The version                     | `recovery_incident` (07 §5.4): nothing is removed and nothing expires. The source stays blocked for mutation. The user is told again, and recovery goes on only by his act                                                                                                                                  |
| At any step: a sealed placeholder of this operation is in the vault                                            | Any    | Any                             | Removed once the operation has an outcome that needs no sealing; used if `preserving` is reached                                                                                                                                                                                                            |
| After the outcome entry                                                                                        | —      | None                            | Nothing to do                                                                                                                                                                                                                                                                                               |
| After the outcome, the name could not be removed                                                               | —      | h0, marked `cleanup_pending`    | Removed (W1)                                                                                                                                                                                                                                                                                                |
| A file is in the transaction directory and no open intent names it, or the source's identity no longer matches | —      | Unknown                         | Never removed and never used. Reported as held, with what is known; the user decides                                                                                                                                                                                                                        |
| An intent is open and the vault's custody does not answer                                                      | Any    | Any                             | Repair waits: `repair_pending`. **Every durable record is kept**: the intent, the files in the transaction directory, the vault's records. Further mutation of that source is refused. Reading, the inventory and the rest of the Console go on. The window says that unlocking is needed to finish         |

A state that repair cannot name from this table is kept exactly as it is and reported: the
engine never guesses, and it removes nothing it has not verified.

A change that uses the vault has more steps, in a fixed order: intent, record written and
flushed, record verified, the write above, record marked as holding the entry, outcome. Its
table of stops is [27 §8.2](27-PROTECTED-RESTORATION-VAULT.md). The rule that ties the two
stores together is VA1: the entry leaves the provider's file only after its record is verified,
and the record is released only after the entry is seen back in the file.

### 4.1 Open intents

An intent is **open** from the moment it is appended until its outcome is appended.

| Rule                                                                                                                                                                                                                    | Why                                                                                        |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| An open intent is never removed by the journal's bounds, by age or by count. A segment that holds one is not deleted                                                                                                    | Repair needs it to tell what a file in the transaction directory is (focused review, F-09) |
| Repair runs at start before any compaction of the journal                                                                                                                                                               | The same                                                                                   |
| At most one open intent per target; at most 64 in all. At the limit a new mutation is refused (`journal_unavailable`)                                                                                                   | Open intents are bounded by refusing work, never by forgetting it                          |
| If the intent cannot be appended, for lack of space or otherwise, nothing is touched                                                                                                                                    | Fail closed                                                                                |
| If the outcome cannot be appended after the file was replaced, the write stands, the intent stays open, the source is blocked for mutation, and the outcome is appended as soon as it can be                            | A full disk must not turn a finished write into an unknown one                             |
| An intent is closed only by a verified terminal state: an outcome of 07 §5.4, reached by the protocol or by the table above                                                                                             | No outcome is assumed                                                                      |
| A conflicting version that is held or sealed keeps its intent's outcome on record; the journal entry that says a version exists is part of that outcome and follows the ordinary bounds only after the user resolved it | The existence of a kept version must not depend on a line that can expire                  |

Other operations:

| Operation                   | Steps on disk                         | A crash after each step leaves                                                                  |
| --------------------------- | ------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Journal append              | 1 append · 2 flush                    | A torn last line is detected by its length and checksum and dropped; the entry before it stands |
| Last-seen index             | 1 write beside · 2 rename             | The old index; it is rebuilt from the files at the next read                                    |
| Profile save                | 1 write version file · 2 move pointer | 1: an unreferenced version, kept                                                                |
| Registry update             | 1 write beside · 2 rename             | The old registry                                                                                |
| Preferences `[R]`           | As built                              | The old or the new file                                                                         |
| Authority transaction `[R]` | SQLite                                | Rolled back; a state change whose audit fails is rolled back with it                            |

The invariants: a provider's file is always whole (NFR-REL-01); no name is removed without its
content having been verified (W1); no copy in clear made by a write is left outside the private
transaction directory (W2); the Console's records may lag and are repaired from the files,
which are the truth (E1); and an entry removed to the vault exists in the file or in the vault
at every instant, as far as the filesystem honours a flush (VA1). The vault is the one place
where a record of the Console can be lost with something of the user's in it: its losses are
stated in 27 §5.4 and §8.3.

## 5. Recovery, by event

| Event                              | Managed plane `[R]`                                                                                                             | Native plane                                        | Configuration engine                                                                                                                              |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| Desktop crash                      | Unaffected: the authority keeps running; turns continue; the window reconnects                                                  | Terminals and native sessions end; records → `lost` | Plans are lost (memory only); files intact; start-up repair of §4                                                                                 |
| Agent crash (native)               | —                                                                                                                               | `exited` with its code                              | —                                                                                                                                                 |
| Managed runtime crash              | The turn ends `failed` or `indeterminate`; no automatic restart loop                                                            | —                                                   | —                                                                                                                                                 |
| Authority crash                    | Restarted by the service manager a bounded number of times; running turns → `indeterminate`; attachments cleared; bindings kept | Unaffected                                          | Unaffected                                                                                                                                        |
| Partial file write                 | —                                                                                                                               | —                                                   | Cannot leave a provider's file partial (rename)                                                                                                   |
| Concurrent external edit           | —                                                                                                                               | —                                                   | A named outcome of SM-WRITE for every ordering the Console can observe; no version removed unverified. Two orderings cannot be observed (07 §5.5) |
| Replication conflict               | —                                                                                                                               | —                                                   | Source `conflicted` and mutations blocked, when the tool leaves a conflict copy the Console recognises; otherwise an outside change               |
| Upgrade of the Console             | The authority is replaced only when none is running (`update pending`) `[R]`                                                    | Registry migrated                                   | Stores migrated; a newer store is left read only                                                                                                  |
| Migration failure                  | Refused and rolled back; an older build refuses a newer schema `[R]`                                                            | The old file is set aside, not deleted              | The same                                                                                                                                          |
| Interrupted restoration            | —                                                                                                                               | —                                                   | §4: a restoration is a write like any other. A restoration from the vault never releases its record before the entry is seen back (27 §8.2)       |
| A provider changes its protocol    | Fails closed: only the verified version is accepted `[R]`                                                                       | Start allowed with a warning                        | Inventory marked unverified; mutations refused                                                                                                    |
| A provider changes a file's format | —                                                                                                                               | —                                                   | The source becomes `malformed` or fails the round trip → read only                                                                                |

## 6. Rollback semantics

| What                   | How it is undone                                                                                               | Exactness                                    |
| ---------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| A configuration change | A new plan from the journal entry (07 §5.7), and from the vault record when the entry was removed to the vault | Byte-exact if the file has not changed since |
| A profile edit         | Move the pointer to an earlier version                                                                         | Exact                                        |
| A managed decision     | Not undoable: an approval given was acted on; a session closed cannot be reopened `[R]`                        | —                                            |
| A native session       | Not undoable: what the agent did, it did                                                                       | —                                            |
| A preference           | Reset `[R]`                                                                                                    | To defaults                                  |

The interface never offers undo where the effect has already left the Console's control.

## 7. Events and notifications

| Event                        | Raised by           | Shown as                                         | Recorded in                                   |
| ---------------------------- | ------------------- | ------------------------------------------------ | --------------------------------------------- |
| An approval is pending       | The authority `[R]` | A count on Approvals `[R]`                       | The authority's stream                        |
| A managed turn ended         | The authority `[R]` | In the turn                                      | The authority's stream                        |
| The authority is unavailable | The desktop `[R]`   | The notice and Retry `[R]`                       | Not recorded: it is not the authority's event |
| A source changed outside     | The engine          | A banner on the source; a count on Configuration | Journal                                       |
| A conflict copy appeared     | The engine          | The source marked conflicted                     | Journal                                       |
| A plan was refused           | The engine          | In the plan dialog                               | Journal                                       |
| A native session ended       | The supervisor      | In the overview                                  | Registry                                      |

Notifications are in the window only. Desktop notifications of the operating system are a
later option and would be a preference.

## 8. Deletion by the user

FR-DAT-05 and FR-DAT-06. Two actions, never one:

| Action                  | Removes                                                                                  | Loses                                                                                                                                                                                            | Confirmation                                                                                        |
| ----------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------- |
| Delete history          | The closed entries of both journals, the last-seen index, the observation key            | History only. **Nothing a held record depends on**: the keys the vault uses are not touched (§3.2), and open intents are kept (§4.1). Entries disabled on rungs 1 and 2 are still in their files | In the page                                                                                         |
| Delete the kept entries | Vault records, one or all; and, when all go, the index, the consent and the vault's keys | **Each record deleted is an entry that can no longer be restored.** Deleting the whole vault is refused while an intent is open or a conflicting version is held                                 | Native dialog that lists them by metadata and offers "restore all first" and "show" beside "delete" |

Deleting from the vault removes the files of the vault. It does not reach copies in backups,
snapshots or freed blocks, and the dialog says so (27 §10, T-36).

Removing the application removes none of the user's data, as today (README `:97-100`) `[R]`:
the vault and its key in the keyring stay, and a later installation finds them.

## 9. Not verified

| Item                                                                                                                             | How to verify                                                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The recovery rules of §4 hold under real interruption                                                                            | Fault-injection tests that stop the process between every pair of steps (TS-CFG-12)                                                                                                  |
| The bounds of §3 are comfortable for the owner's volume                                                                          | Measure a month of real use in F2a                                                                                                                                                   |
| Start-up repair costs nothing noticeable                                                                                         | P1 measurement with full stores                                                                                                                                                      |
| VA1 holds under real interruption                                                                                                | Fault injection between every pair of steps of 27 §8.2 (TS-VLT-17)                                                                                                                   |
| The vault adds nothing to start-up                                                                                               | P1 measurement: no vault code runs before an operation needs it                                                                                                                      |
| The state directory and the owner's configuration are on one filesystem, and an exchange between two directories of it is atomic | The probe of 07 §5.4, in the phase's own harness (OQ-31). One filesystem for the user-scope files is supported by the inspection of 2026-10-13 (30 §6); the exchange is not verified |
| An open intent survives every bound                                                                                              | TS-REC-06                                                                                                                                                                            |
