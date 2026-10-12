# 14 — Data, persistence and recovery

Domains J (records) and L (persistence) of [04](04-TARGET-ARCHITECTURE.md): who owns each
piece of state, where it lives, how it survives a crash and how it is undone. `[R]` marks what
exists; the rest is `[P]`.

## 1. Rule

Each piece of state has exactly one owner (FR-DAT-01). Everything else that shows it holds a
projection that can be thrown away and rebuilt. No two stores are authoritative for the same
fact.

## 2. Ownership

| State                                                | Owner                          | Lives in                                                 | Others hold                                                |
| ---------------------------------------------------- | ------------------------------ | -------------------------------------------------------- | ---------------------------------------------------------- |
| An agent's configuration                             | **The provider's files**       | `~/.claude/…`, `~/.codex/…`, project files, system files | The Console: a projection, hashes                          |
| An agent's sign-in                                   | The provider                   | Its credential files                                     | Nobody. Managed Local Tools is handed one descriptor `[R]` |
| A provider's trust in a hook or project              | The provider                   | Its own records                                          | The Console: read only                                     |
| A native agent's conversation                        | The provider                   | Its session files                                        | Nobody                                                     |
| Managed sessions, bindings, leases, turns, approvals | **The authority** `[R]`        | `~/.local/state/wyss-console/` (SQLite, schema 7)        | The window: projections                                    |
| The managed execution record                         | The authority `[R]`            | The same store: an append-only envelope stream           | The window: pages of it                                    |
| The managed runtime's actual state                   | The authority, in memory `[R]` | —                                                        | The window                                                 |
| Desktop preferences                                  | The desktop `[R]`              | `~/.config/wyss-console/preferences.json`                | —                                                          |
| Terminals and their content                          | The desktop, in memory `[R]`   | —                                                        | —                                                          |
| Native session records                               | The desktop                    | Registry file                                            | The overview                                               |
| Session profiles, projects                           | The desktop                    | Profile and project files                                | —                                                          |
| Configuration history                                | The configuration engine       | Journal                                                  | The History view                                           |
| Restoration data                                     | The configuration engine       | Restoration store (subject to DN-01)                     | —                                                          |
| Last-seen index                                      | The configuration engine       | Index file                                               | —                                                          |
| Window layout (tabs, panes)                          | The desktop                    | Layout file (T2)                                         | —                                                          |
| Benchmarks                                           | The developer                  | Outside the product                                      | —                                                          |

What the Console deliberately does not own: any agent's configuration, any conversation, any
credential.

## 3. Stores

All new stores live under the Console's private state directory, in a subdirectory of their
own, mode 0700, files 0600, never inside a provider's directory. Each file starts with a schema
version; a file with a newer version is left untouched and its feature is read only until the
Console is updated, as preferences already behave (`settings/service.rs`, state `Newer`) `[R]`.

| Store                    | Format                                              | Content                                         | Bound                                       | Written                                                        |
| ------------------------ | --------------------------------------------------- | ----------------------------------------------- | ------------------------------------------- | -------------------------------------------------------------- |
| Authority database `[R]` | SQLite, one writer                                  | Canonical managed state and its event stream    | The authority's own rules                   | Transactionally                                                |
| Preferences `[R]`        | JSON, one file                                      | A closed set of preferences                     | A few hundred bytes                         | Write beside, rename                                           |
| Journal                  | Append-only lines, one per entry, in dated segments | 11 §5.5: identifiers, hashes, outcome           | 10,000 entries or 180 days, whichever first | Append, flush                                                  |
| Restoration store        | One file per record                                 | Per DN-01                                       | 500 records or 180 days                     | Write beside, rename                                           |
| Last-seen index          | One file per source                                 | Hash, time, item identifiers; entries per DN-01 | One per known source                        | Write beside, rename                                           |
| Native registry          | One file                                            | Session records                                 | 500 sessions or 90 days                     | Write beside, rename                                           |
| Profiles                 | One directory per profile, one file per version     | 09 §4.1                                         | 256 profiles, 20 versions each              | Write beside, rename; a pointer file names the current version |
| Projects                 | One file                                            | Folder paths and defaults                       | 256 projects                                | Write beside, rename                                           |
| Layout                   | One file                                            | Tabs, panes, titles, working directories        | One                                         | Write beside, rename                                           |
| Session scratch          | A directory per running native session              | Generated profile files                         | Removed at exit and at next start           | Plain                                                          |

Bounds are proposals to be fixed with the fixtures (OQ-22). Reaching a bound removes the oldest
entries first; a restoration record still referenced by a parked entry is never removed by age
alone: the user is warned 14 days before and at expiry it is kept until he decides.

### 3.1 Why the journal is not in the authority's database

AD-11 and decision D8: the authority must not learn of user configuration (C-02), and its
stream is the record of managed execution with integrity checks tied to its canonical rows
(ADR 0017) `[R]`. A configuration entry there would have no canonical row behind it.

## 4. Crash consistency

| Operation                                      | Steps on disk                                                                              | A crash after each step leaves                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Configuration change with a restoration record | 1 write record · 2 write new file beside · 3 rename · 4 flush directory · 5 append journal | 1: a record without its change. 2: the same, plus a temporary file, removed at start by its name pattern. 3 or 4: the new file is in place; the journal lacks the entry. At start, a record with no journal entry is judged **by the entry, never by the file's hash**: entry present in the file and identical to the record → the change did not happen, the record is an orphan and is removed; entry absent → the change happened (the file may have moved on since), the record is kept as parked and the missing entry is appended as `applied (recovered)`. 5: complete. A write-ahead intent entry would make the case unambiguous (24, RV-12) |
| Configuration change with a native switch      | 1 write beside · 2 rename · 3 flush · 4 journal                                            | As above without the record; a missing journal entry is then indistinguishable from an outside change and is reported as one                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Restore                                        | 1 write beside · 2 rename · 3 flush · 4 mark record used · 5 journal                       | 3 done, 4 not: the record still says parked while the entry is in the file → at start the engine sees the entry present and marks the record "restored"                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Profile save                                   | 1 write version file · 2 move pointer                                                      | 1: an unreferenced version, kept                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Registry update                                | 1 write beside · 2 rename                                                                  | The old registry                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Preferences `[R]`                              | As built                                                                                   | The old or the new file                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Authority transaction `[R]`                    | SQLite                                                                                     | Rolled back; a state change whose audit fails is rolled back with it                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |

The invariant: a provider's file is always whole, either as before or as after (NFR-REL-01).
The Console's own records may lag by one entry and are repaired from the file, which is the
truth (E1).

## 5. Recovery, by event

| Event                              | Managed plane `[R]`                                                                                                             | Native plane                                        | Configuration engine                                               |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- | ------------------------------------------------------------------ |
| Desktop crash                      | Unaffected: the authority keeps running; turns continue; the window reconnects                                                  | Terminals and native sessions end; records → `lost` | Plans are lost (memory only); files intact; start-up repair of §4  |
| Agent crash (native)               | —                                                                                                                               | `exited` with its code                              | —                                                                  |
| Managed runtime crash              | The turn ends `failed` or `indeterminate`; no automatic restart loop                                                            | —                                                   | —                                                                  |
| Authority crash                    | Restarted by the service manager a bounded number of times; running turns → `indeterminate`; attachments cleared; bindings kept | Unaffected                                          | Unaffected                                                         |
| Partial file write                 | —                                                                                                                               | —                                                   | Cannot leave a provider's file partial (rename)                    |
| Concurrent external edit           | —                                                                                                                               | —                                                   | `changed_outside`; nothing written (07 §5.5)                       |
| Replication conflict               | —                                                                                                                               | —                                                   | Source `conflicted`; mutations blocked                             |
| Upgrade of the Console             | The authority is replaced only when none is running (`update pending`) `[R]`                                                    | Registry migrated                                   | Stores migrated; a newer store is left read only                   |
| Migration failure                  | Refused and rolled back; an older build refuses a newer schema `[R]`                                                            | The old file is set aside, not deleted              | The same                                                           |
| Interrupted restoration            | —                                                                                                                               | —                                                   | §4, row "Restore"                                                  |
| A provider changes its protocol    | Fails closed: only the verified version is accepted `[R]`                                                                       | Start allowed with a warning                        | Inventory marked unverified; mutations refused                     |
| A provider changes a file's format | —                                                                                                                               | —                                                   | The source becomes `malformed` or fails the round trip → read only |

## 6. Rollback semantics

| What                   | How it is undone                                                                        | Exactness                                    |
| ---------------------- | --------------------------------------------------------------------------------------- | -------------------------------------------- |
| A configuration change | A new plan from the journal entry (07 §5.7)                                             | Byte-exact if the file has not changed since |
| A profile edit         | Move the pointer to an earlier version                                                  | Exact                                        |
| A managed decision     | Not undoable: an approval given was acted on; a session closed cannot be reopened `[R]` | —                                            |
| A native session       | Not undoable: what the agent did, it did                                                | —                                            |
| A preference           | Reset `[R]`                                                                             | To defaults                                  |

The interface never offers undo where the effect has already left the Console's control.

## 7. Events and notifications

| Event                                   | Raised by           | Shown as                                         | Recorded in                                   |
| --------------------------------------- | ------------------- | ------------------------------------------------ | --------------------------------------------- |
| An approval is pending                  | The authority `[R]` | A count on Approvals `[R]`                       | The authority's stream                        |
| A managed turn ended                    | The authority `[R]` | In the turn                                      | The authority's stream                        |
| The authority is unavailable            | The desktop `[R]`   | The notice and Retry `[R]`                       | Not recorded: it is not the authority's event |
| A source changed outside                | The engine          | A banner on the source; a count on Configuration | Journal                                       |
| A conflict copy appeared                | The engine          | The source marked conflicted                     | Journal                                       |
| A plan was refused                      | The engine          | In the plan dialog                               | Journal                                       |
| A native session ended                  | The supervisor      | In the overview                                  | Registry                                      |
| A restoration record is about to expire | The engine          | A notice in Configuration                        | —                                             |

Notifications are in the window only. Desktop notifications of the operating system are a
later option and would be a preference.

## 8. Deletion by the user

FR-DAT-05: one action removes everything the configuration engine keeps (journal, restoration
store, last-seen index), after showing what will be lost: every parked entry that only this
computer can restore is listed, and the user is offered to restore or export them first.
Removing the application removes none of the user's data, as today (README `:97-100`) `[R]`.

## 9. Not verified

| Item                                                    | How to verify                                                                       |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| The recovery rules of §4 hold under real interruption   | Fault-injection tests that stop the process between every pair of steps (TS-CFG-12) |
| The bounds of §3 are comfortable for the owner's volume | Measure a month of real use in F2a                                                  |
| Start-up repair costs nothing noticeable                | P1 measurement with full stores                                                     |
