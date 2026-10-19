# 0029 — Secrets, restoration data and what the configuration subsystem may persist

Status: **Proposed**. Not accepted. Nothing in it is implemented. Until the owner accepts it,
or another ADR on the same subject, no mutation phase (F2a onward) may start and nothing that
the configuration subsystem of ADR 0028 would persist may contain an entry of the user's
configuration. Accepting it would authorise no implementation and no experiment: phases start
only by the owner's word.

This is the second draft, of 2026-10-13. The first, of 2026-10-12, proposed that the Console
hold no copy of any entry at all; it was reviewed once by an independent reviewer
(`docs/architecture/25-INDEPENDENT-REVIEW.md`) and rejected with the rest of the set. On
2026-10-13 the owner decided that a protected private backup **is** allowed, which reverses the
centre of that draft. What changed between the drafts is in "History of this proposal". **This
draft has not been reviewed by anyone but its author.**
Later on 2026-10-13 the owner gave his position on each question of this ADR ("The owner's
positions, and what is still open"). They are his design preferences and they confirm requirements he had
already approved. **They do not accept this ADR.** Its exact wording, and every technical
guarantee it describes, stay proposed and unproven until an independent review and his formal
acceptance.
A second independent review, focused on this ADR and its two specifications, returned
**REJECT** on 2026-10-13 with eleven findings
(`docs/architecture/reviews/2026-10-13-focused-security-review-report.md`). By the owner's
instruction they were corrected in the documents the same day, this ADR included: sections 1,
2, 9 and 10 changed. **Those corrections have not been reviewed, the verdict is not
overturned, and no guarantee is made verified by them.**

Date: 2026-10-13

## Context

Three accepted or approved texts have to be made to agree.

- ADR 0003, accepted: "Secrets and credentials never enter durable storage. Minimization and
  redaction happen before persistence." (`0003:69-70`)
- ADR 0028, accepted, classes that sentence as "superseded in one area": it "does not extend to
  restoration data", because "a user's own configuration entry, kept so that it can be put
  back, may contain a secret he wrote into it" (`0028`, compatibility row for 0003). Its
  section 7 keeps, in the Console's private state, the bytes of every parked entry and, per
  file, "the restorable entries it last observed there". Its decision D5 reads "private local
  backups, per computer", with "enough content and metadata for an exact restoration".
- The owner's policy of 2026-10-12, nine points, recorded here because it existed only in a
  conversation:

  1. ADR 0028 does not grant unrestricted permission to persist secrets.
  2. Credentials, API keys, tokens and other sensitive values must not be copied into ordinary
     audit records, configuration history, diagnostics or restoration indexes.
  3. Restoration data must be strictly minimised.
  4. Provider-native disable and restore mechanisms are preferred where they exist.
  5. Reference-based or in-place approaches that create no second durable copy of sensitive
     content are preferred, but only if provider compatibility is actually verified.
  6. A heuristic secret detector is never assumed to prove that arbitrary hook content contains
     no secret.
  7. Where safe reversible persistence cannot be guaranteed, the operation stays unavailable, or
     requires a separately designed and explicitly approved security mechanism.
  8. The last-seen index must not become a permanent duplicate of all the owner's hooks.
  9. Existing security guarantees stay in force until a new accepted ADR explicitly changes them.

And one decision taken after them, on 2026-10-13, confirmed by the owner in his own words when
asked:

> Wyss Console may store protected private backups to allow exact restoration of individual
> hooks, including potentially sensitive configuration content, provided that appropriate
> security controls and informed user consent are implemented.

The decision and the policy do not contradict each other: the vault this ADR proposes is the
"separately designed and explicitly approved security mechanism" of point 7. Points 1 to 6, 8
and 9 stand as written. What the decision removes is the first draft's reading of the policy,
which took "no second durable copy" as absolute.

Facts that bound the choice:

| Fact                                                                                                                                    | Evidence                                                                                                                                                                                         |
| --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Claude Code has no per-hook switch: one hook is disabled by deleting its entry, or all hooks at once with `disableAllHooks`             | Provider documentation read 2026-10-12, as recorded in ADR 0028 §3                                                                                                                               |
| Codex reports `enabled` per hook                                                                                                        | `src-tauri/src/adapters/codex/safety.rs:344-346`                                                                                                                                                 |
| A hook entry can hold a secret: a token written into a command, a header of an HTTP handler, a URL with user information                | The shape of the entries, provider documentation                                                                                                                                                 |
| A disabled entry that can be restored is, by definition, still stored somewhere. The only question is where                             | —                                                                                                                                                                                                |
| The owner's `~/.claude/` is replicated between two computers; the Console's private state is not                                        | ADR 0028 §7                                                                                                                                                                                      |
| Whether the owner's home, and with it the Console's state directory, is covered by filesystem snapshots                                 | Reported so by the owner's own notes; **not verified here**                                                                                                                                      |
| Whether Claude Code tolerates, ignores and preserves data it does not know inside `settings.json`, across its own rewrites of that file | **Not verified.** Nothing in this ADR may be built on it until it is                                                                                                                             |
| Whether a secret service runs, unlocked and with a password, in the owner's sessions on both computers                                  | Observed on 2026-10-13: the Secret Service interface answers on both, unlocked, provided by GNOME Keyring (`30` §6). Its password, and its behaviour locked, absent or emptied: **not verified** |
| The repository has no encryption or key-store dependency                                                                                | `src-tauri/Cargo.toml` holds `sha2` only                                                                                                                                                         |
| Any process of the same user can read every file discussed here, original or copy                                                       | ADR 0018 `:110-121`                                                                                                                                                                              |

## What was approved, and what was not

| Approved by the owner, 2026-10-13                             | Not approved by that                                                      |
| ------------------------------------------------------------- | ------------------------------------------------------------------------- |
| A protected private store for restoration may exist           | Any particular design of it: this ADR and its specification are proposals |
| It may hold configuration content that contains a secret      | Storing secrets anywhere else, or in clear, or without limit              |
| It needs security controls and informed consent               | That encryption answers every threat                                      |
| Individual hook control is a requirement of the final product | Any implementation, any experiment, any phase                             |

## Decision (proposed)

### 1. ADR 0003's sentence stands, with two named exceptions

The classification "superseded in one area" that ADR 0028 gives ADR 0003 is **kept and made
exact**. ADR 0003 is not edited. This ADR records the exceptions in these words, whose exact
wording waits for review:

> ADR 0003's paragraph on secrets (`0003:69-72`) applies unchanged to every event, to every
> store of the authority, to audit, to the configuration journal, to every index, to logs,
> diagnostics and support bundles, to preferences, registries and profiles.
>
> Two exceptions, and only these two.
>
> **(A) The protected restoration vault.** The vault defined by ADR 0029 may durably hold
> configuration content that the user asked Wyss Console to keep restorable, and that content
> may include a secret he wrote into it. The exception holds only (a) for the classes of
> record ADR 0029 names; (b) encrypted as it requires, with no unencrypted fallback; (c)
> after the consent it requires; (d) never for a provider's credential or sign-in files, which
> are not read at all; (e) never for anything outside the vault: no excerpt of a record, no
> reference that resolves to its content and no unkeyed digest of it enters any other store.
>
> **(B) The transient copy of a file being replaced.** While Wyss Console replaces a
> provider's configuration file, a copy of that one file may exist in clear, because replacing
> a file without destroying what it replaces needs the old file to stay reachable under a
> name. The exception holds only (a) where that is technically indispensable: the direct
> replacement of a provider's file, and nothing else; (b) for the one file being replaced; (c)
> in a private directory of the Console, never in a provider's directory and never in a
> directory found to be replicated; (d) for the duration of the write, bounded by a fixed
> ceiling; (e) with a defined repair for every point at which the process can stop; (f) on a
> filesystem where the required primitives were probed and the file's metadata can be carried;
> (g) refused, with the write, wherever one of these cannot be established. A version of the
> file that would otherwise be lost is not left under this exception: it is sealed in the
> vault under exception (A). Exception (B) does not cover keeping a version in clear: where
> the protocol cannot securely preserve a possible conflict, the direct write is unavailable. An
> unexpected failure after the mutation is a recovery incident: evidence is preserved, both
> risks are disclosed, nothing is destroyed by time, and private storage in clear is never
> treated as equivalent to encryption.

Exception (B) is new in this draft. The first independent review of the vault found that the
earlier text called the vault the only exception while the write protocol made copies in clear
(focused review, F-02). It is a real second exception and is asked of the owner as one.

Nothing else is loosened. No history, index, log, diagnostic or profile may hold a secret or
the content of an entry, in clear or otherwise.

The controls that replace the blanket restriction are, for (A), those of section 9 and of
`docs/architecture/27-PROTECTED-RESTORATION-VAULT.md`; for (B), those of section 10 and of
`docs/architecture/28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md` §7.

Because of policy point 6, **every hook, MCP server and plugin entry is treated as content that
may contain a secret**, whatever a detector says about it. A detector of secret patterns keeps
one role: deciding what is masked when configuration is shown in the window, and warning the
user. It never decides that something may be persisted, and never that something may be
persisted in clear.

### 2. What may be persisted, in eight categories

One rule does not fit all of them.

| #   | Category                                                   | May hold                                                                                                                                                                                                                                                                 | Never holds                                                                                                                                                                                                                                                   | Rule comes from                                       |
| --- | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| 1   | Application audit: the authority's event stream and stores | What ADRs 0003, 0011 and 0017 already allow                                                                                                                                                                                                                              | Any secret; anything about the user's agent configuration                                                                                                                                                                                                     | ADR 0003, unchanged                                   |
| 2   | Configuration change history (journal)                     | Provider, scope, an identifier of the source file from the adapter's closed list, kind of entry, event name if it is one the adapter knows, ordinal position, operation, **keyed** digests of the file before and after, the identifier of a vault record, time, outcome | A command, an argument, a URL, a header, a matcher, an environment name or value, a path the user wrote, any value; **any unkeyed digest of a file or entry**                                                                                                 | Policy point 2                                        |
| 3   | Last-seen inventory and drift index                        | Per source: a keyed digest of the file, the time, and one keyed digest per entry present in the file                                                                                                                                                                     | The entry's bytes or any part of them. What the user chooses to keep recoverable is not here: it is a vault record (category 6)                                                                                                                               | Policy point 8; approved by the owner, 2026-10-13     |
| 4   | Transient files of a replacement                           | A copy of the one file being replaced, in clear, in the Console's private transaction directory, for the bounded time strictly needed to finish the write or roll it back                                                                                                | Anything in a provider's directory; anything after the write has reached a final outcome                                                                                                                                                                      | Exception (B) of section 1; section 10                |
| 5   | Conflict-preservation artefacts                            | One version of a provider's file that would otherwise be lost, sealed in the vault                                                                                                                                                                                       | A file in clear beside the provider's file, or in a replicated directory. A version in clear as a way of preserving it, for any length of time. After an unexpected failure the version is recovery evidence of an incident, disclosed as a risk, not a store | The owner's decision on DN-28, 2026-10-13; section 10 |
| 6   | Protected restoration vault                                | The removed span of an entry the user disabled, with what an exact restoration needs; and, only for entries the user selected one by one, a recovery copy of the entry as it stands. Encrypted; after consent                                                            | Anything not needed for a restoration; anything the user did not disable or select; a whole file, except a conflicting version; any provider credential file                                                                                                  | Exception (A) of section 1; section 9                 |
| 7   | Provider-owned configuration files                         | Whatever the provider and the user put there. The Console is one of its writers and never its owner                                                                                                                                                                      | A file made by the Console                                                                                                                                                                                                                                    | ADR 0028 §2                                           |
| 8   | Provider credentials and sign-in data                      | **Nothing is read, copied or stored by the configuration subsystem.** Adapters do not open these files                                                                                                                                                                   | —                                                                                                                                                                                                                                                             | ADR 0028; CTL-06                                      |

Digests. Every digest the Console stores is keyed, and which key decides what it is worth
(`docs/architecture/14-DATA-PERSISTENCE-AND-RECOVERY.md` §3.2).

| Digest                                                                     | Key                                                                  | What that is worth                                                                                                                                                                                                            |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Of entries and files in the last-seen index                                | The observation key: a random value in a file of the state directory | It keeps the digests from being compared across installations or after they leave the machine. Nothing against someone who holds that directory. The entries it covers are in the provider's file, in clear, on the same disk |
| Of files, in the journal of changes                                        | The journal digest key, sealed in the vault                          | Without the vault's custody a journal entry cannot be used to confirm a guess at what a file held                                                                                                                             |
| Of entries and their neighbours, in vault records and in the vault's index | The vault digest key, sealed in the vault                            | The same, for a held entry                                                                                                                                                                                                    |

An unkeyed digest is not treated as harmless: someone who knows the rest of a file can confirm
a guess at a short secret in it. None is stored. Deleting the history removes the observation
key and nothing the vault depends on.

Diagnostics, logs and support bundles hold closed error words, counters and versions; no digest
derived from entry content and no file content.

### 3. How a disabled entry stays restorable: a ladder of four rungs

Per provider, kind of entry and version, the first rung that applies is used.

1. **Provider-native state.** Where the provider has its own switch for that kind of entry, the
   Console writes that switch and nothing else. The entry never leaves the provider's file.
   Codex hooks, skills, plugins and MCP servers are in this case; so are Claude Code's skills
   override, its plugin switch and `disableAllHooks`.
2. **In place, in the provider's own file.** Where there is no native switch, the entry is kept
   inside the same file it came from, in a form the provider does not act on, so that no
   second durable copy exists anywhere. This rung is available for a provider version **only
   after** the compatibility test of section 5 has passed on that exact version. Unknown data
   is never assumed to be preserved or safely ignored.
3. **The protected restoration vault.** Where neither of the above applies, the entry is
   removed from the provider's file and sealed in the vault of section 9, on that computer.
4. **Temporarily unavailable.** Where rung 3 cannot yet be used safely for that entry (its
   contract is not accepted, its key custody is unavailable, the write protocol is undecided,
   the entry is too large), the operation is not offered. The window says why and what would
   enable it, and offers what does exist: the provider's coarse switch, and the file's path.

Rung 4 is a state of an implementation, never a limit of the product: individual control of a
hook is a requirement of the final product (FR-HRS-02, FR-HRS-03), and rung 3 is its designed
path wherever rungs 1 and 2 fail.

Consequence for Claude Code hooks, stated plainly. They have no rung 1. Rung 2 is unverified
and needs research with a real provider binary, which is not authorised. Rung 3 does not need
that research: it leaves in the provider's file nothing the provider does not know. It needs
this ADR accepted, the decisions of section 9 taken, the write protocol decided (DN-21) and the
vault built and reviewed. **Until then, disabling one Claude Code hook stays on rung 4.**

An entry is not moved between rungs by itself. One sealed in the vault stays there until it is
restored, even if rung 2 is verified later.

### 4. What changes in ADR 0028, provision by provision

ADR 0028 is accepted and is not edited. Each provision below is extended, kept or replaced
**by this ADR, if accepted**.

| ADR 0028                                                                                                        | Under this ADR                                                                                                                                                                                                                                             | Kind                                        |
| --------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| Compatibility row for ADR 0003: "superseded in one area … does not extend to restoration data"                  | Kept, and made exact: the area is the vault of section 9 and nothing else (section 1)                                                                                                                                                                      | Narrowed                                    |
| Decision D5, "private local backups, per computer … enough content and metadata for an exact restoration"       | **Stands, strengthened.** The backup is the vault: encrypted, consented, minimised, never replicated. It is used on rung 3 only, after the two rungs that keep no copy                                                                                     | Kept, strengthened                          |
| §7, "Parked entries stay in the private state of the computer that parked them … its bytes"                     | Kept as rung 3, with the contract of section 9                                                                                                                                                                                                             | Kept, strengthened                          |
| §7, "a private directory of this user, files readable by him alone, a bounded number kept"                      | Replaced by encryption with a key held outside the directory, plus the same modes. Reaching the bound refuses new records; it never evicts one                                                                                                             | Strengthened                                |
| §7, "Values of environment variables, credentials and any field an adapter classifies as secret are never kept" | **Replaced for vault records.** An exact restoration needs the entry's own bytes, whatever a classifier thinks of them, and a classifier proves nothing (policy point 6). The sentence stands for every other store                                        | Replaced, narrowly                          |
| §7, "Every computer keeps its own last-seen entries … the restorable entries it last observed"                  | Replaced: the index holds hashes and keyed digests, and gives nothing back. What that sentence gave by default, the recovery of an entry removed from outside, is given **by choice**: an encrypted recovery copy of entries the user selected (`27` §3.6) | Replaced; approved by the owner, 2026-10-13 |
| §7, export of a parked entry on one computer and import on the other                                            | Kept as a requirement (FR-CFG-15); designed as an encrypted export in `27` §9.3; not offered in the first vault phase (DN-29)                                                                                                                              | Kept, deferred                              |
| §3, "disabling one Claude Code hook means removing its entry and keeping it elsewhere"                          | "Elsewhere" is, in order: the same file in a kept form where verified; otherwise the vault                                                                                                                                                                 | Made exact                                  |

Everything else in ADR 0028 stands, including decisions D1 to D4 and D6 to D10, the mutation
pipeline of its section 5 and the gate G-INF of its section 6. Two of its sentences are known
to be wrong and are corrected by a separate proposal, ADR 0030, not here: section 8 on
read-back, and section 5 on what the native confirmation protects.

### 5. The compatibility test that opens rung 2

In-place keeping has two candidate forms. Which one, if either, is a result of the test and
not of this ADR.

| Form                        | What it does                                                                                                                                                                                                               | Depends on                                                                                  |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| **Set aside**               | The entry is moved, inside the same document, under a key or section the provider does not read                                                                                                                            | The provider tolerating unknown data: not rejecting the file, not warning, not removing it  |
| **Neutralised where it is** | The entry stays in its place with one field changed so that the provider loads it and never runs it (for example a matcher that matches nothing), and the original value of that field recorded beside it or encoded in it | The provider's documented semantics of that field, and where the original value can be kept |

A form is not accepted for "hooks" in general. It is accepted cell by cell: for one exact
provider version, one hook event, one handler type and one position (the only handler of its
group, one of several, the only group of its event). The action is offered on rung 2 only for
the cells that passed; the others fall to rung 3. Each cell passes only if **every** check
below passes for it, on a throwaway home, with a real provider binary:

| #   | Check                                                                                                                                                                                                                                                                    | Control that must fail                                              |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------- |
| V1  | The provider starts and loads **the rest of the file**: every other hook, permission rule and setting is in force                                                                                                                                                        | A file made invalid on purpose: the provider reports it or skips it |
| V2  | The kept entry never runs: its event is triggered and a marker it would create does not appear                                                                                                                                                                           | The same entry in its normal place: the marker appears              |
| V3  | The provider shows no error or warning caused by the kept entry, at start or in its own diagnostics                                                                                                                                                                      | —                                                                   |
| V4  | When the provider rewrites the file itself, the kept entry survives byte for byte. Run once for **each** way the provider is found to rewrite that file; the list of those ways is itself a result of the research and is recorded. A way not on the list is not covered | A kept entry removed by hand: the check reports it gone             |
| V5  | Restoring produces a file byte-identical to the file before the entry was disabled                                                                                                                                                                                       | —                                                                   |
| V6  | The provider's own listing of hooks does not present the kept entry as active                                                                                                                                                                                            | —                                                                   |
| V7  | Every check above, V1 to V6, passes again with the provider version installed on each other computer that shares the file                                                                                                                                                | —                                                                   |

V1 is the one that matters most: a provider that answers unknown data by ignoring the whole
file would silently drop every security hook in it. If V1 cannot be shown, rung 2 is closed for
that provider.

The result is tied to the provider version, as every other capability is
(`docs/architecture/06-AGENT-ADAPTERS.md` §6). On a version without a recorded pass, rung 2 is
not offered; entries already kept in place by an earlier version are shown, are never touched
automatically, and can be restored only by the user's explicit action after a warning.

Designing this research and its acceptance criteria is authorised. Running it is not
(question Q6).

### 6. Consequences, by rung

| Subject                | Rungs 1 and 2: the entry stays in the provider's file                                                                    | Rung 3: the entry is in the vault                                                                                                       |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| Truth                  | The provider's file is the only place where configuration lives. A lost Console state loses history, never configuration | The vault of one computer holds a piece of the user's configuration. **A lost vault, or a lost key, loses that entry**                  |
| Second copy            | None                                                                                                                     | One, encrypted, on one computer                                                                                                         |
| The other computer     | Reads the state from the file and can restore there. Rung 2 needs the provider there to tolerate the form (V7)           | Sees the entry gone and cannot restore it; it says where it can be restored                                                             |
| Retention              | The Console retains nothing; the entry lives in the user's file until he restores or deletes it                          | Until restored or deleted by the user; never deleted by age while it is the only copy (DN-27)                                           |
| Deleting a secret      | Disabling is not deleting: the entry still holds it, in the file                                                         | The same, in the vault. Deleting the record removes it from the vault and from nowhere else: backups and snapshots keep the ciphertext  |
| Snapshots and backups  | Capture what they already captured, for longer                                                                           | A new place that did not hold it before, in ciphertext. Its worth is the worth of the key's custody                                     |
| Risk from the provider | Rung 2: the provider may reject, warn about, drop or run the kept data (section 5)                                       | None from unknown data: the file holds only ordinary entries                                                                            |
| Crash recovery         | One file, replaced atomically; journal intent and outcome around it                                                      | Two stores in a fixed order: the record is verified before the file is touched and released only after the entry is seen back (`27` §8) |
| Same-user threats      | No protection, as everywhere in this product (ADR 0018)                                                                  | No protection either: a process of the user can reach an unlocked key. The vault is not a boundary against it                           |
| Visibility             | A disabled entry is visible to anyone who opens the file                                                                 | The entry is no longer in the file. The window lists it; no other tool sees it                                                          |

### 7. Things that copy configuration without being called restoration

- **Generated files of a session profile.** A profile holds references. The file generated from
  it for one session may have to contain whole hook entries, because a provider that merges
  hooks across levels can only "leave one out" by being given the wanted set
  (`09-PROFILES-AND-ORCHESTRATION.md` §3). Such a file is a copy of content that may hold a
  secret. It is never placed in the Console's state directory and never in the vault, which
  exists for restoration only. Whether it may exist at all, and where, is decision DN-20: a
  memory-only file handed to the agent through an inherited descriptor, a file in the per-login
  runtime directory, or not offering such profiles.
- **Rule files.** Disabling an instruction file moves the user's own document; nothing of its
  content is copied. Where it may be moved to is unverified per provider and stays so.
- **The diff shown before a change.** It exists in memory, for the window, and is never
  written anywhere.

### 8. Existing guarantees

Until this ADR or another is accepted: ADR 0003 is read at full strength; no mutation of any
agent's configuration is built; the read-only inventory of phase F1 persists nothing at all.
Nothing here changes the managed plane, the authority, or any guarantee of ADRs 0011 to 0018:
the vault lives in the desktop's configuration subsystem and the authority neither reads nor
knows of it.

### 9. The protected restoration vault

Specified in full in `docs/architecture/27-PROTECTED-RESTORATION-VAULT.md`. What this ADR would
fix, if accepted:

| Subject               | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| What is held          | One record per disabled entry: the one contiguous run of bytes removed from the file, with the punctuation that held it, and what places it back (position, neighbours' keyed digests, file hashes, provider, scope, source identity). No whole file, no other entry                                                                                                                                                                                                                                                         |
| Exactness             | Before the provider's file is touched, the record is read back and shown to rebuild the original file byte for byte                                                                                                                                                                                                                                                                                                                                                                                                          |
| Encryption            | Every record is sealed with authenticated encryption; its header is authenticated and binds it to this vault, this installation and its own identifier. There is no unencrypted form of a record and no fallback to one                                                                                                                                                                                                                                                                                                      |
| Key                   | Never in the vault's directory. In an OS-backed keyring, through the Secret Service interface: the owner's decision of 2026-10-13, after the inspection that found it on both computers, provided by GNOME Keyring. KWallet's own interface is a separate custody, selected explicitly and validated separately. Nothing is migrated or switched silently. Or derived from a passphrase, by the user's choice. If the custody does not answer, the operations that need the vault are unavailable. **No plaintext fallback** |
| Consent               | A native dialog before the first record of each class, stating what is stored, why, where, how it is protected, for how long, that it is not synchronised, how it is restored and deleted, and what happens if the key is lost. Asked again when any of those changes                                                                                                                                                                                                                                                        |
| Order                 | An entry leaves the provider's file only after its record is verified; a record is released only after the entry is seen back in the file, or by the user's explicit deletion                                                                                                                                                                                                                                                                                                                                                |
| Retention             | A record that is the only copy of an entry is never deleted by age (question Q10)                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Recovery copies       | Only for entries the user selected, one by one; captured only at his act, never by observation; kept apart, in name and in behaviour, from a disabled hook held in the vault                                                                                                                                                                                                                                                                                                                                                 |
| Recovery of the key   | An optional code, shown once and never stored, with which the key can be rebuilt. Without it, a lost key loses every record                                                                                                                                                                                                                                                                                                                                                                                                  |
| Orphaned records      | Never purged by age or by the engine; always listed with their reason                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| Keys                  | The vault has keys of its own for its digests and for the journal's file digests, sealed in it. Nothing of the vault depends on the key of the last-seen index, so deleting history breaks no held record                                                                                                                                                                                                                                                                                                                    |
| Putting an entry back | Exact when the file has not changed; otherwise only at one unambiguous place; otherwise nothing is written (`27` §3.7)                                                                                                                                                                                                                                                                                                                                                                                                       |
| Where it may be       | By a criterion that is open (DN-31; `27` §9.1). A replication marker above the directory is not proof of replication; an ignore rule is evidence, not a guarantee; a snapshot of the home subvolume captures it                                                                                                                                                                                                                                                                                                              |
| Two computers         | The vault is per installation and is refused in a replicated location. The other computer cannot restore; an encrypted export is designed and deferred (question Q12)                                                                                                                                                                                                                                                                                                                                                        |
| The page              | Receives metadata and redacted projections only. **Every operation that creates, decrypts or destroys a record is authorised by a native dialog opened by the backend, for that request and those entries**: sealing a disabled entry, keeping a recovery copy, restoring, revealing, deleting, exporting, importing, rotating the key. A confirmation in the page authorises none of them                                                                                                                                   |
| What it is not        | A boundary against a process of the same user, or against code in the page, which can type into a terminal and from a shell reach the files and an unlocked keyring. A promise about swap, core dumps, snapshots or memory (`27` §5.3). It protects content at rest in the files the Console writes: against a copied disk, a backup, a snapshot of those files, a replication by mistake, another user                                                                                                                      |

Other classes of record are proposed in the specification and are separate questions: a
conflicting version of a file (section 10, question Q7b) and the previous bytes of an entry
that was edited or deleted (the contract of authoring, DN-12).

### 10. Transient files and conflicting versions

The one protocol for writing a provider's file directly is
`docs/architecture/07-CONFIGURATION-ENGINE.md` §5.4; its contract is
`docs/architecture/28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md`. **It is a design candidate, not
a verified implementation.** The owner's directions of 2026-10-13 fix what follows.

**Transient copies (exception B).** Replacing a file without destroying what it replaces needs
the old file to stay reachable under a name, so that the same file can be put back if the
provider wrote meanwhile. That copy is in clear. It is kept in the Console's private
transaction directory, on the same filesystem as the file, never beside it; the new content is
unnamed while it is written; the old file keeps its name only through a settling time with a
fixed ceiling; every point at which the process can stop has a repair. A file of a write is
never in a provider's directory, so the design does not depend on a short-lived file going
unseen by a replication tool. Wherever a condition of exception (B) cannot be established, the
direct write is refused: there is no weaker way, and never an ordinary rename.

What is not promised: that the Console's directory is not copied by a tool it does not
recognise, by a backup or by a snapshot; that removing a name erases the bytes; that a file
left by a stop is ever removed if the Console is not started again.

**Conflicting versions.** When a write ends with a version of the file that cannot be made the
file again, that version is sealed in the vault. The vault must be ready before any direct
write is started, so that this is always possible: if it is not, the write is refused. If
sealing still fails at the moment it is needed, that is an unexpected failure after the
mutation: a **recovery incident**. The engine reports no success, blocks conflicting writes,
keeps every piece of recovery evidence it has, the version in the private
transaction directory included, discloses the risk to confidentiality and to the data, and
waits for a controlled recovery by the user. Nothing is destroyed by time. It is never written
beside the provider's file and never dropped. A file in clear, of the user's choosing, exists
only by his own act, through a native file chooser.

## Threat model

In full in `27` §10 and `docs/architecture/11-SECURITY-THREAT-MODEL.md` (T-25 to T-41).

| Threat                                          | The vault                                                                                               |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| A stolen disk; a copied state directory         | Mitigated, to the strength of the key's custody                                                         |
| A backup, a snapshot, an accidental replication | Mitigated the same way; a replicated location is refused                                                |
| Another local user                              | Mitigated: modes, path rule, ciphertext                                                                 |
| A process of the same user, a compromised agent | **Not mitigated.** Accepted, as in ADR 0018, and never described otherwise                              |
| A compromised page                              | Limited to metadata through the vault's own interface; **not contained**, because it can use a terminal |
| Tampering, substitution, replay of records      | Detected by authentication and binding, except by someone who holds the key                             |
| A malicious import                              | Imported content is untrusted executable content and is shown whole before any restoration              |
| Restoration into the wrong file or project      | Refused by the binding                                                                                  |
| A secret that survives its deletion             | **Not solved.** Copies in snapshots and backups remain; the secret itself must be changed               |
| The key is lost                                 | The held entries are lost, unless a recovery code was made (question Q9)                                |

## Failure and recovery

`27` §8 gives the state machines and the table of interrupted transitions. The contracts:

| Failure                                                       | Contract                                                                                           |
| ------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Encryption, storage or the key's custody fails before a write | The provider's file is not touched                                                                 |
| The custody is unavailable when a plan is asked for           | The plan is refused before the user confirms anything                                              |
| A stop between the record and the file's replacement          | The entry is still in the file; the unused record is purged                                        |
| A stop after the replacement                                  | The record is the copy; it is marked as holding the entry                                          |
| A restoration is interrupted, loses a race or is reverted     | The record is still there. It is deleted only by the user, after the entry is seen in the file     |
| A record does not authenticate                                | It is never used, never repaired, never removed by the engine; the user is told the entry is lost  |
| The source was changed, moved or removed                      | Restoration is re-planned against what exists, or refused; the content can be revealed to the user |

## Alternatives considered

| Criterion                        | A. Private local copy, in clear                             | B. Encrypted local copy (the vault)                                    | N. Provider-native switch | I. In place, in the provider's file                                       | E. Sidecar file beside the provider's file            | D. Refuse                 |
| -------------------------------- | ----------------------------------------------------------- | ---------------------------------------------------------------------- | ------------------------- | ------------------------------------------------------------------------- | ----------------------------------------------------- | ------------------------- |
| Second durable copy of the entry | Yes, in the Console's state                                 | Yes, encrypted                                                         | No                        | No                                                                        | Yes, in the provider's directory                      | No                        |
| New exposure in snapshots        | Yes: a place that did not hold it before                    | Ciphertext only                                                        | None                      | None                                                                      | The same snapshots, one more file                     | None                      |
| Replication                      | Must be kept out; a mistake leaks it                        | A mistake leaks ciphertext                                             | Travels with the file     | Travels with the file                                                     | Travels, if the directory is replicated               | —                         |
| Portability                      | Explicit export, a plain file leaving the private directory | Explicit export, re-encrypted                                          | Complete                  | Complete                                                                  | Complete                                              | —                         |
| Crash recovery                   | Two stores to reconcile                                     | The same, in a fixed order                                             | One file                  | One file                                                                  | Two files                                             | —                         |
| Permissions                      | Relied on entirely (0700, 0600)                             | Defence in depth                                                       | The provider file's own   | The provider file's own                                                   | Must be created private; can differ from the original | —                         |
| Same-user process                | Reads it                                                    | Reads it while the key can be obtained                                 | Reads it                  | Reads it                                                                  | Reads it                                              | —                         |
| Key management                   | None                                                        | A secret service or a passphrase; behaviour when locked, reset, absent | None                      | None                                                                      | None                                                  | —                         |
| Retention                        | Must be bounded; a purge can destroy the only copy          | Never purged while it is the only copy                                 | The user's file           | The user's file                                                           | The user's file                                       | —                         |
| Lost Console state               | **Loses the user's configuration**                          | Loses it; so does a lost key                                           | Loses nothing             | Loses nothing                                                             | Loses nothing                                         | —                         |
| Risk from the provider           | None                                                        | None                                                                   | None                      | The provider may reject, warn about, drop or run the kept data: section 5 | The provider may read the sidecar as configuration    | None                      |
| Depends on a secret detector     | Yes, to stay inside ADR 0003                                | No                                                                     | No                        | No                                                                        | No                                                    | No                        |
| Fit with ADR 0003                | Breaks it                                                   | A named exception (section 1)                                          | Keeps it                  | Keeps it                                                                  | Arguable: a second copy, outside the Console's stores | Keeps it                  |
| Fit with the owner's decisions   | No (policy points 1, 6)                                     | Yes: the decision of 2026-10-13, as the mechanism of point 7           | Yes (point 4)             | Yes, once verified (point 5)                                              | Weaker than I on point 5                              | Only as a temporary state |
| What the user loses              | Nothing                                                     | Nothing while the key lives                                            | Nothing                   | Nothing, where verified                                                   | Nothing                                               | Per-entry control         |

Chosen: N, then I, then B, with D as a temporary state. Not chosen: A, because it stores in
clear what policy point 6 says cannot be shown free of secrets. Not chosen: E, because the
provider's directory is replicated and the sidecar may be read as configuration.

## Consequences and residual risks

- Disabling one Claude Code hook has a path to delivery that does not wait for research on a
  real provider: rung 3. It waits for decisions and for a build that is reviewed.
- The Console becomes, on rung 3, the only holder of a piece of the user's configuration. That
  was the main reason the first draft avoided it. It is accepted here because the owner asked
  for it, and it is bounded by the order of section 9 and by consent that says so.
- A hook held on one computer cannot be restored on the other.
- Reading, the inventory, the detection of outside changes and an operation through a
  provider's own verified writer do not depend on the vault. A locked keyring stops direct
  writes and the vault's own operations, and nothing else (`27` §5.7).
- No file of a provider is written directly unless the vault is ready to take a conflicting
  version. The vault's core therefore comes before the first mutation, not after it.
- Disabling one hook always asks in a native dialog, because it stores the hook's content.
- The vault's protection is the keyring's. On a machine whose keyring opens without a password,
  it is close to none against a copied disk.
- A secret in a held hook is still a secret held, now in two generations of backups.
- Three new dependencies at least (encryption, key derivation, the secret service) enter a
  product that today has one hash library.
- The window gains a list, a consent dialog and several states that have no design contract yet.

## Open technical questions

| Question                                                                                                                        | Recorded as                                                        |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Checks V1 to V7, for Claude Code, on each of the owner's computers                                                              | RS1; OQ-06, OQ-19                                                  |
| For each provider, which files it rewrites by itself, when, and from what                                                       | OQ-31, OQ-35                                                       |
| A secret service present, unlocked and password-protected on both computers                                                     | OQ-38: present and unlocked by inspection (`30` §6); the rest open |
| Keeping keys and decrypted content out of swap and core dumps                                                                   | OQ-39                                                              |
| The state directory outside everything replicated or snapshotted                                                                | OQ-32: **not so** on either computer (`30` §6); DN-31              |
| The Console's state directory and the owner's configuration on one filesystem, with an exchange of names across two directories | OQ-40                                                              |
| Unnamed files and an atomic exchange on the owner's filesystems                                                                 | OQ-31                                                              |

## Approval gates

Nothing below follows from anything above it automatically.

| Gate                                                                                                | Who                               | Opens                                             |
| --------------------------------------------------------------------------------------------------- | --------------------------------- | ------------------------------------------------- |
| 1. This ADR accepted, changed or replaced                                                           | The owner                         | The policy; nothing else                          |
| 2. The owner's positions of 2026-10-13 turned into exact text, and that text accepted with this ADR | The owner                         | A complete contract for the vault                 |
| 3. The write protocol decided (DN-21) and ADR 0030 accepted                                         | The owner                         | A way to write a provider's file at all           |
| 4. An independent security review of `27`, `28` and this ADR                                        | A reviewer who did not write them | The right to plan the vault's phase               |
| 5. The phase authorised                                                                             | The owner                         | Building, on fixtures only                        |
| 6. The phase accepted, with its external security review                                            | The reviewer, then the owner      | A run on a copy of a home                         |
| 7. Use on the owner's real configuration                                                            | The owner's explicit word         | The feature                                       |
| Separately: research on a real provider (Q6, DN-24)                                                 | The owner                         | Rung 2; the observations the write protocol needs |

## The owner's positions, and what is still open

Stated by the owner on 2026-10-13. Each is recorded in his terms. "Direction given" means a
design preference to be followed; **no row accepts this ADR, and none marks a technical
guarantee as proven**. Q1 to Q7 are in the words of the first draft; Q7a to Q13 are the
questions this draft added.

| #   | Question                                                                                                                                                      | The owner's position                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Still open                                                                           |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Q1  | Is ADR 0003's sentence restored in full for the configuration subsystem (section 1)?                                                                          | No: strict for ordinary persistence, with a narrowly scoped exception for the vault                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | The wording: Q13                                                                     |
| Q2  | Is the ladder native, in place, unavailable (section 3) the rule, with no Console-held copy of any entry?                                                     | No: native, in place where verified, the protected vault, and unavailable only as a temporary state                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | —                                                                                    |
| Q3  | Is decision D5's mechanism, private local backups holding content, replaced as section 4 says?                                                                | No: D5 stands and is strengthened                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | —                                                                                    |
| Q4  | Is the last-seen index reduced to hashes and keyed digests, so that an entry removed from outside is not restorable from this computer?                       | **Approved**: a metadata-only general index. And: design an explicitly opt-in, encrypted recovery capability for selected entries; nothing discovered is duplicated automatically; recovering a disabled hook held in the vault is kept distinct from recovering an entry deleted outside                                                                                                                                                                                                                                                                                                                                                                         | The design of that capability (`27` §3.6; DN-30)                                     |
| Q5  | Is it accepted that per-hook disabling for Claude Code stays unavailable until the test of section 5 passes, even if that empties most of F2a?                | Unavailable until a reversible implementation is validated, as a temporary restriction. The test of section 5 opens rung 2 only; the vault is the other path                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | The vault's review and build                                                         |
| Q6  | Is the research of section 5 authorised, on a throwaway home, with a real provider binary?                                                                    | **Not authorised.** Restated on 2026-10-13                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | —                                                                                    |
| Q7  | Is the transient copy of section 6 accepted: unnamed except for an instant, beside the provider's file, with a named copy left only by a crash or a conflict? | Split into Q7a and Q7b                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | —                                                                                    |
| Q7a | Is a named whole copy of the file accepted beside it for the duration of a write, including the settling time?                                                | Directed on 2026-10-13, twice. The protocol is retained as an **unverified candidate**, in one canonical form: the previous file kept under a name through a bounded settling time. A provisional strict mode is adopted. No ordinary-rename fallback. A transient copy in clear only where indispensable, under a narrow, explicit exception: minimal lifetime and content, restrictive permissions, safe handling of stops, explicit filesystem and metadata guarantees, a defined recovery, protection against unintended synchronisation with no assumption that a short-lived file goes unseen. Where that cannot be established, the direct write is denied | The wording of exception (B) in section 1; every technical guarantee of the protocol |
| Q7b | Where does a version go that could not be put back?                                                                                                           | Directed on 2026-10-13: preserved in the authenticated, encrypted vault where possible. Long-lived copies in clear are not an acceptable default. Where the environment cannot meet the controls, fail closed                                                                                                                                                                                                                                                                                                                                                                                                                                                     | The handling when sealing itself fails (section 10; DN-28)                           |
| Q8  | Where does the vault's key live, and what happens when that place does not answer?                                                                            | Prefer an OS-backed keyring, evaluating KWallet and Secret Service support. Explicit failure behaviour; a secure alternative where necessary; no plaintext fallback. Then, after the inspection (2026-10-13): the Secret Service interface as the design target; KWallet separate, explicitly selected and separately validated; no silent migration or switch of provider                                                                                                                                                                                                                                                                                        | The keyring's behaviour when it does not answer (OQ-38; `30`, TV-14)                 |
| Q9  | Is a recovery code offered when the vault is created?                                                                                                         | Design optional recovery-key support, with clear threat and loss-of-key semantics                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | The design's review (`27` §5.5)                                                      |
| Q10 | Are held entries ever deleted by age?                                                                                                                         | Not at first. Visibility, reminders, explicit deletion, and protection against orphaned records. Then (2026-10-13): no record of any class is removed on age, restored and undo records included; explicit, verified clean-up; nothing removed under storage pressure                                                                                                                                                                                                                                                                                                                                                                                             | The design's review (`27` §8.3, §8.4)                                                |
| Q11 | Is it accepted that section 7 of ADR 0028, "any field an adapter classifies as secret [is] never kept", does not apply to vault records?                      | Permitted: the complete individual entry where an exact, byte-level restoration requires it. Stored content is minimised; whole files are not copied by default                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | —                                                                                    |
| Q12 | Is export and import of held entries between computers offered?                                                                                               | Design it, authenticated and encrypted; defer its implementation beyond the first vault phase                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | —                                                                                    |
| Q13 | Is the amended reading of ADR 0003 in section 1 approved in those words?                                                                                      | The policy direction is approved: a narrowly scoped exception. **The exact wording stays pending review.** Unrelated prohibitions on storing secrets are not weakened                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | The wording                                                                          |

Guidance the owner gave after the corrections to the focused review (2026-10-13). It keeps the
corrections in the proposed documents, overturns no verdict and validates no guarantee.

| #   | Guidance                                                                                                                                                                                  | Where it is followed                             |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| 1   | The vault is operational before any direct mutation of a configuration file that needs transactional recovery or protected restoration data                                               | `07` §5.4, `preparing`                           |
| 2   | Read-only operations and verified provider-native switches are not coupled to the vault's availability without need                                                                       | `27` §5.7                                        |
| 3   | A locked keyring does not disable the Console; the degradation is stated capability by capability                                                                                         | `27` §5.7                                        |
| 4   | A narrow exception for indispensable transient replacement artefacts is supported. Keeping a conflicting version in clear indefinitely is **not** approved, in a private directory or not | Section 1, exception (B); section 10             |
| 5   | Recovery material is prepared before the mutation where it can be; otherwise fail closed where possible; a failure after the mutation is modelled without claiming recovery               | `07` §5.4, `preparing` and `preserving`; `28` §6 |
| 6   | Staging is private, correctly permissioned, outside unintended synchronisation and on the same mount as the file                                                                          | `07` §5.4; `28` §7                               |
| 7   | A native confirmation for what introduces a vault-backed record; never described as containing a compromised page that can write to a terminal                                            | `27` §6.1; ADR 0030                              |
| 8   | Crash recovery may need the vault unlocked. Meanwhile every durable record is kept, the state is shown, conflicting mutations are blocked, reading stays                                  | `14` §4; `27` §5.7                               |
| 9   | Losses L1, L2 and L5 are unresolved. No claim of zero loss or of full detection                                                                                                           | `28` §5, §9                                      |

Two decisions the owner confirmed afterwards (2026-10-13):

| Decision                 | In his terms                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| DN-28                    | The proposed seven-day retention in clear is **rejected**, and no other arbitrary timeout replaces it. No automatic destruction of the only recoverable version. Staging in clear only for the bounded time a transaction strictly needs. Persistent conflict recovery in the encrypted vault. Where a possible conflict cannot be securely preserved, the direct-write mechanism stays unavailable until its failure path is designed and validated. An unexpected failure after the mutation is a documented recovery incident. DN-28 stays a technical design gate |
| The vault's availability | Every direct configuration-file mutation managed by Wyss needs the approved recovery prerequisites, the vault's readiness among them where the protocol depends on it. A graphical switch that makes Wyss itself rewrite JSON or TOML is a direct file mutation. Verified provider-native operations that need no Wyss-managed recovery data may stay available with the vault locked. Reading, the terminal, native sessions and managed operations are not disabled by a locked vault                                                                               |

Five decisions the owner confirmed after the inspection of his two computers (2026-10-13;
`30` §6). They are recorded in `22` §1 and change no status of this ADR:

| Matter                    | Decision                                                                                                                                                                                                                                                                                                                           | Where               |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- |
| The inspection            | Its results are recorded per computer. Partial checks stay partial: the exchange, the behaviour at a stop and the keyring's behaviour when it does not answer are not verified                                                                                                                                                     | `30` §6             |
| Retention                 | The thirty-day purge of undo records is removed, and no other expiry by age replaces it. Retention follows what depends on a record and a verified terminal state; explicit, verified clean-up; nothing removed under storage pressure                                                                                             | `27` §8.3           |
| `~/.claude`               | Its access-control list, group, permissions and configuration are not changed. The rejection by the path rule stays a genuine blocker. CTL-10 is not weakened and gets no unreviewed exception. A read-only dependency analysis was run on 2026-10-13 (`30` §7.2, §7.3): the access has a recorded purpose and nothing was changed | `30` §6.4, B1; §7.7 |
| Replication and snapshots | A marker above the directory is not proof of replication; an ignore rule is evidence, not a guarantee; a snapshot is a separate exposure. The conflict between an atomic exchange on one filesystem, staging in clear and snapshots of the home subvolume is unresolved. Direct writes stay blocked                                | `27` §9.1; DN-31    |
| The keyring               | The Secret Service interface is the design target. Its provider on both computers is GNOME Keyring. KWallet is a separate custody. No silent migration or switch. An unlocked keyring is not protection against processes of the user                                                                                              | `27` §5.1; DN-25    |

What remains for the owner after the review: accepting, changing or replacing this ADR as a
whole, with its exact text.

## Not decided here

The write protocol and the concurrent-writer race (DN-21, ADR 0030); what the native
confirmation protects (DN-22, ADR 0030); whether a session profile may copy hook entries into a
generated file (DN-20); how configuration is shown and masked in the window; the libraries the
vault would use (DN-04); anything about the managed plane.

## History of this proposal

| Date       | Draft             | Centre                                                                                                                                                                                                                                                         | Fate                                                                                                           |
| ---------- | ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| 2026-10-12 | First             | No copy of any entry held by the Console. Ladder: native, in place where verified, unavailable                                                                                                                                                                 | Reviewed once (25); revised in sections 2, 5, 6 and 7 after that review; superseded as a proposal the next day |
| 2026-10-13 | Second            | The owner's decision that a protected private backup is allowed. Ladder of four rungs; the vault; eight categories                                                                                                                                             | This text. Unreviewed                                                                                          |
| 2026-10-13 | Second, refined   | The owner's positions on every question recorded; an opt-in recovery copy for selected entries; orphaned records; the keyring evaluation; the recovery code                                                                                                    | This text. Unreviewed; a focused review scope is prepared (`25` §7.1)                                          |
| 2026-10-13 | Second, corrected | After the focused review (REJECT, F-01 to F-11): a second named exception for the transient copy; keyed digests with keys of the vault's own; every vault operation authorised by a native dialog; one write protocol; a conflicting version only in the vault | This text. The corrections are unreviewed                                                                      |

What survives from the first draft unchanged: the treatment of every entry as possibly holding
a secret; the detector's single role; metadata-only journal and index; keyed digests; the
compatibility test V1 to V7; the rule for generated profile files. What is reversed: its
sections 1 to 4 and 6, and its answers to Q1, Q2 and Q3.

## References

- ADR 0003 `:53-76`; ADR 0018 `:110-121`; ADR 0028, sections 3, 5, 7, 8 and decision D5.
- `docs/architecture/27-PROTECTED-RESTORATION-VAULT.md`;
  `docs/architecture/28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md`;
  `docs/architecture/11-SECURITY-THREAT-MODEL.md` §5;
  `docs/architecture/25-INDEPENDENT-REVIEW.md`, findings IR-02 to IR-05.
- The owner's policy statement of 2026-10-12 and his decision of 2026-10-13, both quoted in the
  context above.
