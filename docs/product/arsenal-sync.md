# Arsenal synchronization between computers

Each computer keeps its own Arsenal database (`~/.local/share/wyss-console/arsenal/arsenal.sqlite`).
What the computers share is a folder that Syncthing replicates:

```
~/.local/share/wyss-console-sync/arsenal/v1/
  devices/<device>/r-0000000001.json   append-only journal, written only by that device
  devices/<device>/applied.json        what that device has applied of the others
  blobs/<sha256>                       file contents, named by their digest
```

A database file never travels. Each device writes only its own journal, and a blob's name is its
content, so Syncthing has nothing to conflict over. Files are written in a private staging folder
(`~/.local/state/wyss-console/arsenal-sync/staging`) and renamed into place: Syncthing never sees
part of a file.

## What is shared

| Shared                                                               | Per computer                                                                  |
| -------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Imported capabilities, every revision, their files                   | The Arsenal database and its WAL                                              |
| Presets and selections (by id; items by capability identity/content) | Terminal bindings and project selections (they name local sessions)           |
| Deleted presets and selections (as deletion records)                 | `~/.local/state/wyss-console/arsenal-sync/state.json`: device id and cursors  |
| Preferences (`~/.config/wyss-console/preferences.json`, as a file)   | Credentials, provider logins, F8 authority state, host sockets, installations |

A preference for a model or a skill does not mean it is installed or qualified on the other
computer: a terminal plan is computed on the computer that starts the terminal.

## Records and merging

A record says "this object is now this" and names the version it was made from (`base`). The
versions of an object that no record replaced are its heads.

- One head: that is the object on every computer.
- Several heads with the same content: no conflict.
- Several heads with different contents: a conflict. The head with the highest
  (not-a-deletion, time, device, sequence) is shown; the others are kept, never lost: a capability
  keeps them as older revisions, a preset or selection gets a copy named `… (conflict, <host>)`.
  A merge record then makes one head again. A deletion never wins over an edit.

A later owner edit or deletion replaces every visible head with the content that owner had
already observed. This includes equal-content heads produced when both computers independently
resolved the same conflict; those duplicate heads cannot resurrect a subsequently deleted object.
Different concurrent edits are still preserved.
Records with the same content and the same semantic parent versions represent one version, even
when their physical record IDs differ or one arrives later. Parent equivalence is computed with a
bounded, nonrecursive DAG pass. Missing parents or cyclic references keep an object pending:
presets and selections wait before remote replacement/deletion, and automatic conflict merges
wait before writing anything. Available capability revisions can still be imported incrementally.

The same records give every computer the same result, in any order and after any time offline.
Capabilities cannot be deleted in Arsenal, so they have no deletion records.

## What arrives is checked

A record must match its place (device folder, sequence number in the file name) and its kind.
A capability is stored only if:

- its digest matches its files;
- its identity matches what it is (account-wide capabilities);
- the store's own validation passes;
- no file is a credential by its name, binary, or text carrying a credential value. These are the same rules as an import from disk.

A refused record is reported once and not tried again. Nothing received is run, and receiving
changes no provider's files. A record of a newer format is left for a newer version.

The shared folder is read and written without following a symbolic link at any level of it: a
folder or file another computer replaces with a link, a pipe or a device is refused, and a write
never lands outside it. A record or file is renamed into place relative to its folder as opened,
and a journal record never replaces an existing file.

Limits: 16 device journals, 64 entries in `devices/`, 60,000 entries in any one folder, 50,000
candidate journal files and 128 MiB of records in all. Invalid and unsupported records count too;
directory entry limits also count names that are not UTF-8. A larger shared folder makes the run
fail with an error shown in Arsenal, and nothing is applied from part of it. A capability declares
at most 8 MiB of file contents before any blob is allocated. One run reads at most 256 MiB of file
contents; subsequent runs continue with missing revisions rather than rereading already stored
trees. Older revisions delivered after the current head are imported too, so pinned selections
can become available without changing that head.

A preset or selection that another computer deleted is deleted here only after its content is kept
in `~/.local/state/wyss-console/arsenal-sync/trash/`, and Arsenal lists it under "Deleted on another
computer" with where the copy is. Records are not signed: any device that can write the shared
folder can already write the rest of the synchronized home, so a signature would not narrow who
can change this computer.

## When it runs

In the window's process, while the window is open: at start, after each change made in Arsenal,
when Syncthing brings a file into the shared folder (inotify), and every ten minutes as a recovery.
One run at a time per computer (a file lock). `WYSS_ARSENAL_SYNC=off` switches it off.

Arsenal shows the state under its header:

- what this computer published;
- what each other computer acknowledged applying (its `applied.json`), and when it was last heard from;
- what is waiting for files still on their way;
- conflicts and refusals.

A file written here is never presented as applied elsewhere.
Acknowledgments cover a contiguous prefix of each device's journal. Missing or unsupported
records, deferred objects and refused content stop that prefix; a later record cannot acknowledge
an earlier change that has not arrived or was refused. An unchanged acknowledgment is not rewritten.
The prefix uses the last batch actually processed: a record arriving after that batch was read
waits for the next run before it can be acknowledged. A run that fails or stops writes no
acknowledgment. The acknowledgment is written after the library committed and the bookkeeping was
saved; a run stopped between a commit and its bookkeeping is recognized by the next one, which
matches what the library holds against every received version of the object, so a received change
is never published back as a local edit or reported as a conflict.

## Syncthing

The home folder is shared. These per-computer paths are ignored on both computers (`~/.stignore`):

- the installations (`~/.local/opt/wyss-console*`);
- the F8 state (`~/.local/state/wyss-console-f8`);
- installation backups;
- the Wyss service units and their enablement;
- `~/.local/bin/wyss-console`.

`~/.local/share/wyss-console-sync` is not ignored.
