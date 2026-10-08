# ADR 0018: Core hardening and completion

Status: Accepted
Date: 2026-10-08

## Decision

The managed-agent core of Wyss Console is complete. Sprint 2K added no capability. It
reviewed the core that ADRs 0011 to 0017 decided, as implemented, fixed what the review
found, closed the residual risks those ADRs had deferred to it and measured what
remains. Nothing in the accepted architecture was redesigned.

The core is this chain, and every link of it is owned by the external authority:

```text
client intent
  -> wyss-control canonical authority
  -> session, runtime and attachment validation
  -> ExecutionLease
  -> ManagedTurn
  -> isolated, supervised Codex
  -> ManagedApproval when an action is requested
  -> command or file effect
  -> bounded transient stream
  -> durable EventEnvelope audit
  -> restart-safe replay
```

Cancel, revocation, close, crash and uncertainty fail closed, and no external effect
is ever replayed automatically.

"Core complete" is a statement about that chain. The product is not complete: there
is no agent UI, the desktop does not connect to the authority, and adapters other than
Codex, packaging, installation, policies and retention do not exist.

## Scope of the review

The review was independent of the sprints it reviewed: a fresh session read the code
and the tests of ADRs 0011 to 0017 before changing anything, and treated their reports
as claims. It covered the authority state machines, the boundaries of external
effects, approval mediation, leases and revocation, process and configuration
isolation of the runtime, workspace confinement, the credential, the event log and
replay, crash recovery, idempotency, concurrency, resource bounds, privacy, protocol
parsing and the boundary between human terminals and managed agents.

Severity is what the finding could cause in the build as it was, not what it could
cause in principle. No finding was CRITICAL or HIGH.

## Findings and fixes

### Medium, fixed

- **Lost or regressed storage opened as empty or earlier state.** A database that was
  deleted or emptied, with or without its write-ahead log, opened as a new authority
  with a new stream. A lost or damaged log opened as the state of the last checkpoint.
  SQLite cannot tell either from a first start, so a revoked lease or a closed session
  could come back. The authority now keeps a witness next to the database
  (`control.anchor`: stream identity and the highest committed head), written durably
  after commits and therefore always a lower bound. A missing or empty database is
  created only where there is no witness and no WAL, SHM or journal. A store of another
  stream, or behind its witnessed head, is refused. A refused store is closed without
  checkpointing and is left exactly as found.
- **The control plane was reachable by path from inside the local-tools namespace.**
  The namespace bound the whole host filesystem, so the control socket and the
  canonical database were visible to the runtime and to the commands it runs. The only
  thing between an accepted command and the socket, where it could have granted leases
  or answered approvals, was the provider's own network filter. The authority's
  runtime and state directories are now hidden in the namespace; the generation gets
  back its private home and, read-only, its model catalog.
- **The credential was validated by path and bound by path.** Between the check and
  the launcher's bind the path could name another file; a second hard link and a home
  writable by others were accepted; nothing looked at it again. See Credential
  capability.
- **A repository marker in the workspace shortened the masked project chain.** Project
  policy layers were masked from the nearest ancestor holding `.git`. An empty `.git`
  left in a nested workspace, which is what an interrupted sandbox of the provider
  leaves, made the next launch treat the workspace as the project root and leave the
  layers above it unmasked. Whether the provider would then load them depends on how it
  resolves a project root, which the authority does not control. The chain now starts
  at the outermost marker, every directory in it that holds a marker is forced
  `untrusted` together with the workspace, and the chain a namespace was created for
  stays part of every later proof.

### Low, fixed

- The core loop spun for up to five seconds when an approval expired on a request that
  offers no `cancel`: the passed deadline was handed back as the next wake until the
  provider ended the turn.
- An event of a type this build does not define was tolerated in any store, which hid
  a known event damaged into an unknown one. See Event integrity.
- `managed_turn.cancel_requested` carried the turn's last update as its timestamp. It
  carries the authority time of the request, never earlier than that update.
- A binding operation that could not be settled durably stayed open, which is safe,
  but was reported as still being created, and one of the three paths that reach it
  did not close execution. It is reported `indeterminate`, every path closes
  execution, and the next authority start settles and audits it.
- Commits the authority makes on its own (lease expiry, approval expiry, enforcement on
  the active turn) were announced to watchers only at the next message. A watcher could
  wait out its whole timeout and answer empty over an event that had committed. The
  head is now announced after that work.
- The admission flag the supervisor reads before it writes a prompt was withdrawn just
  after a revocation, close or stop committed. It is withdrawn before the commit and
  restored only if the mutation changed nothing.
- A host `/etc/codex` directory was required only as a mount point. See Process
  isolation.
- The process test suite failed under load with "text file busy": a test thread forked
  while another was writing the fake provider executable. Writes and forks are
  serialized.

## Trust boundaries

What the core protects against, and what it does not:

| Against                                               | Protection                                                                                                                                                                                                                                             |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| accidental corruption and loss of storage             | exact schema validation, integrity and foreign-key checks, the log compared with every canonical row, the event vocabulary, the witness of stream and head; refused, never repaired or recreated                                                       |
| malformed or hostile provider data                    | bounded frames and values, exact parameter sets, one request at a time, anything outside the mediated surface is a violation that is never answered and ends the generation                                                                            |
| the model and the commands it is allowed to run       | zero tools in the inference profile; in the local-tools profile every action needs a committed Accept, runs in the provider's sandbox (workspace-only writes, no network, no local sockets) and cannot see the control plane or the user's Codex state |
| another client of the same user                       | none. `SO_PEERCRED` and mode 0600 keep other users out. Any process of this user can connect, grant leases and accept approvals: it is the user                                                                                                        |
| another process of the same user outside the protocol | none. It can rewrite the database, the witness and the workspace, or signal the authority. Integrity checks detect damage, not forgery, and nothing here is authenticated                                                                              |
| a compromised current user                            | none                                                                                                                                                                                                                                                   |

The provider process itself, the pinned Codex 0.159.0 binary, is trusted to enforce its
sandbox and to report honestly what it runs. The authority verifies its configuration
before every conversation, turn and Accept, and contains it on any contradiction; it
does not sandbox the provider's commands itself.

## Credential capability

The read-write bind of the user's `auth.json` stays. Codex 0.159.0 stores a ChatGPT
login in that file and rewrites it in place when it refreshes a token
(`codex-rs/login/src/auth/storage.rs`, `FileAuthStorage::save`, upstream revision
`687a119f`: open with truncate, write, mode 0600). The alternatives were examined:

- No credential exists in the environment of this installation, and requiring one
  would require a login.
- A private copy, or a read-only bind, lets the isolated runtime rotate the refresh
  token without the user's file learning of it: the user's own Codex is then logged
  out. That is the desynchronization the bind avoids.
- The provider's externally managed token mode would make the authority read the
  credential, hold it in memory, speak a refresh protocol and enable the experimental
  API it pins off. That moves the risk into the authority instead of narrowing it.

So the bind is kept as one narrow, validated capability:

- the user's Codex home is resolved once to its canonical directory, which must belong
  to this user and be writable by nobody else;
- the file is opened without following a link and without read access, and must be a
  regular file of this user, with no group or world access and exactly one name;
- the launcher binds that open descriptor, so the inode that enters the namespace is
  the inode that was validated, whatever its path names afterwards, and the path never
  appears in the launch;
- the descriptor stays close-on-exec in the authority and reaches only the launcher;
- nothing else of the user's Codex home exists in the namespace: no configuration,
  rules, hooks, MCP servers, history or model cache;
- with every profile attestation the identity is checked again. A credential that was
  replaced, widened, linked a second time or removed ends the generation. Size and
  times change with each refresh and are not part of the identity;
- no byte of the credential is read, copied or logged by the authority.

Residual: a refresh by the isolated runtime writes the user's own file, as intended;
and a command the user accepts can read the file, exactly as under the user's own
Codex. The provider's sandbox denies writing it (measured).

## Process isolation

Every managed Codex process is a child of the authority, started from fixed arguments
without a shell, with parent-death SIGKILL and a parent-identity check, and reaped
before its exit is announced. No client supplies an executable, an argument, an
environment variable or a working directory; the protocol refuses the fields.

The local-tools namespace now consists of: the host filesystem; a private overlay of
`/etc` carrying the authority's managed policy at `/etc/codex`; an empty tmpfs over the
user's Codex home with only the pinned release brought back read-only; an empty tmpfs
over the authority's runtime and state directories with the generation's home and
catalog brought back; a read-only empty directory over every project policy layer of
the chain; and the credential descriptor. A host `/etc/codex` is no longer needed,
and nothing of one is visible. The host must provide `/usr/bin/bwrap` (root-owned, not
writable by others) with unprivileged overlay support: bubblewrap 0.11 or newer on a
kernel that allows overlayfs in a user namespace. Without it the profile is
unavailable. There is no unisolated fallback, and the inference profile, which offers
the model no tool, does not depend on it.

What an accepted command can do was measured on the production path, with a probe the
real model was asked to run and the user accepted (Codex 0.159.0, 2026-10-08):

| Attempt                                                             | Result                              |
| ------------------------------------------------------------------- | ----------------------------------- |
| write inside the workspace                                          | allowed                             |
| write or create outside it, in `/tmp`, in the home, in `/etc`       | denied                              |
| write through a symlink that leaves the workspace; rename out of it | denied                              |
| create a hard link to a file outside it                             | denied                              |
| write through a hard link that already existed inside it            | allowed                             |
| create `.git`, `.codex` or `.agents` in the workspace               | denied                              |
| see the control socket, the database or the witness                 | they do not exist                   |
| connect to any local socket; connect to the network                 | denied                              |
| list the user's Codex home                                          | only the path to the pinned release |
| read the credential in the private home                             | allowed                             |
| write the credential; write the private home                        | denied                              |
| process state                                                       | seccomp filter, no new privileges   |

### Path validation and the provider's effect

The authority proves confinement when an approval opens and again immediately before
the Accept. The provider acts afterwards. A change in between (a component replaced
by a symlink, a rename, a parent replaced) is not seen by the authority; it is the
provider's sandbox that decides, and the table above is what it decides. A mount
cannot be changed without privilege. One case is not prevented by anything: a file
inside the workspace that has a second name outside it. The authority refuses a file
change that names such a file; a free-form command the user accepts can still write
through it, and so can a same-user process that plants the link in the window. That
process could write the outside file directly. The residual is a bounded race behind
an independently attested sandbox, not an escape the authority enables.

### Directories the provider leaves behind

Sprint 2I observed empty `.git`, `.codex`, `.agents` and `.aws` directories in a
workspace after its acceptance run. Under the production profile of this build none
appeared in more than thirty real turns, including turns whose runtime or authority was
killed while a command ran, in a plain workspace, a repository root and a nested
workspace. The core
does not depend on that: an empty layer is not policy and holds the masks, anything
inside an unmasked layer ends the generation, the four names are refused as targets of
any action, and the masked chain is the widest one. The authority deletes nothing in a
workspace: whether an empty directory is the provider's or the user's cannot be known.

## Storage and recovery

Schema v7 migrates v1 to v6 transactionally and adds one column, `event_vocabulary`.
An older build refuses a newer schema; there is no downgrade path.

Measured with the release binary, every case twice, comparing every file of the state
directory before and after:

| Damage                                                                        | Result                                                                                |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| database missing, with its log; database emptied; database and log missing    | `storage_incompatible`                                                                |
| log missing; log tail damaged; log damaged whole; database truncated          | `storage_incompatible`                                                                |
| future schema version; schema object removed or added                         | `storage_incompatible`                                                                |
| event envelope damaged; event removed                                         | `storage_incompatible`                                                                |
| database of another authority in its place                                    | `storage_incompatible`                                                                |
| witness unreadable, of a later head or of another stream                      | `storage_incompatible`                                                                |
| database, log or witness readable by others, a symlink, or with a second name | `unsafe_path`                                                                         |
| a second authority on the same state                                          | `already_running`                                                                     |
| write failure while running (file size limit)                                 | `storage_unavailable`, execution closed, committed state intact, recovered on restart |
| hard kill during sustained mutation, ten rounds                               | reopened whole; every acknowledged intent present once                                |

Removing the witness is the explicit operator act that accepts the database that is
there; emptying the state directory, witness included, starts a new authority.

## Event integrity

EventEnvelope v1 is unchanged. An unknown event type is accepted only in a store whose
recorded vocabulary is larger than this build's, because only a build with a larger
vocabulary can have written one. In every other store it is corruption. This closes
the limit ADR 0017 documented inside the authority's own history, for all 28 types,
without giving up forward compatibility: a store that a newer build has opened still
replays that build's events as stored. What remains out of reach: in such a store, an
observation without a canonical row retyped into an unknown type; and, in any store,
the value of an observational field, which nothing cross-checks. SQLite has no page
checksums, and this is detection of damage, not authentication.

## Replay

Replay is a read. Measured over history that held a really accepted command: three
full replays in pages of 1, 7 and 50 and two watches produced identical bytes, zero
provider frames, zero executions and zero events. After a graceful restart and after a
hard kill the stream, the bytes and the sequence continued, the accepted command had
still run exactly once, and a new generation wrote no Accept and started no turn.

## Crash matrix

Deterministic: in the store, with the scripted provider, and with a provider that takes
each step only when released so that the authority can be killed at an exact boundary of
an accepted command and of an accepted file change.

| Authority or runtime lost                                        | Approval        | Turn                             | Effect        |
| ---------------------------------------------------------------- | --------------- | -------------------------------- | ------------- |
| before the provider was asked                                    | none            | `indeterminate`                  | none          |
| after the prompt, before an approval                             | none            | `indeterminate`                  | none          |
| while the approval is pending                                    | `invalidated`   | `indeterminate`                  | none          |
| after the decision committed, the answer never read              | `indeterminate` | `indeterminate`                  | none          |
| after the provider read the Accept, before it acted              | `indeterminate` | `indeterminate`                  | none          |
| after the effect, before the provider reported it                | `indeterminate` | `indeterminate`                  | happened once |
| after the report, before the authority confirmed it              | `indeterminate` | `indeterminate`                  | happened once |
| during a cancel, a revocation or a session close after an Accept | `indeterminate` | `indeterminate` or `interrupted` | none          |
| while a watcher waits                                            | unchanged       | unchanged                        | none          |

In every row: no provider outlives its authority, the restarted authority resends
nothing, the original intent still returns its receipt, another Accept is refused, and
a new generation writes no answer, starts no turn and repeats no effect. Exactly-once
execution by the provider is not claimed; what is claimed is that uncertainty is
recorded as uncertainty and nothing is retried.

The same boundaries with the real provider, ten cases: authority or runtime killed
while an approval was pending, while an accepted command ran and after it had written;
session close, turn cancel and lease revocation while an accepted command ran; and a
graceful stop while pending. All ten ended in the states above, with no surviving
process, no later execution and no replay by the next generation.

## Concurrency

All canonical state has one writer, the core thread, and every contested mutation is
arbitrated by its transaction. Tested with every contender on its own connection
released by one barrier: one intent from six clients is one session; six grants for
one session are one lease; renew against revoke commits one of them, and a revocation
that committed is final; close against grant always ends closed without a lease. An
Accept racing a second Accept, an approval cancel, a turn cancel, a revocation, a
close or a runtime stop has one outcome, at most one answer to the provider and at
most one effect, and never an effect without an Accept. Reads and watches run
concurrently with writes without losing, repeating or reordering an event.

## Resource bounds

Measured on the release binary, 3000 operations, each on its own connection:

| Measure                                   | Value                                                           |
| ----------------------------------------- | --------------------------------------------------------------- |
| resident memory                           | 7.0 MiB at start, 9.3 MiB after 1200 operations, flat to 3000   |
| after three full replays of 2501 events   | 9.4 MiB                                                         |
| descriptors, threads                      | 10 and 2 at rest; 22 and 6 with four watchers; back to 10 and 2 |
| child processes without a runtime         | 0                                                               |
| CPU while idle, and with watchers waiting | 0.00 s in 10 s                                                  |
| database for 2501 events                  | 4.7 MiB, plus a log bounded near 4 MiB                          |
| watcher wake after a commit               | median 1 to 3 ms, maximum 16 ms, none lost in 60                |
| fifth watcher                             | `capacity_exceeded`                                             |

Bounds are structural: eight connections, four watchers, one active turn, one pending
approval, eight retained transient streams of 256 events and 256 KiB, 64 KiB frames,
2048-byte events, fifty-entry pages. Six generations with twelve turns leave the
authority with the descriptors and threads it started with and no process behind.

## Privacy

Prompt, assistant output, reasoning summary, raw reasoning, command, command output,
file path, file content, diff, provider credential and an environment secret were
given unique sentinels and searched for in every file under the authority's runtime
and state directories, in the database, its log and shared memory, the witness, the
catalog, the authority's standard error and every replay response: 0 occurrences, in
the deterministic suite and in the runs against the real provider. The provider's own
conversation history in the private Codex home is the provider's, not Wyss durability.
Human terminal bytes never reach the authority: the terminal module writes no file and
has no dependency on the control protocol, and no terminal operation exists in it.

## Sabotage

One defense was disabled at a time, 105 mutations over the execution gate, lease
identity and revision, the Accept gate, the one provider answer, event append and log
validation, workspace confinement, configuration isolation, the model tool surface
and the defenses of this sprint. 90 were detected. The first pass exposed four weak
tests, which were strengthened and are among the 90. The remaining 15 change nothing
observable: twelve are a second or third check of a condition another layer already
enforces (the Accept gate repeats in memory what its transaction decides; a second
active turn is also refused by the reservation and by a unique index), two are
branches that cannot be reached, and one only moves a withdrawal a few instructions
earlier. No mutation revealed a missing defense.

## Acceptance evidence

On 2026-10-08, release binary of this decision, Codex 0.159.0 with the authenticated
provider, isolated authority state, workspaces outside this repository:

| Case                            | Measured                                                                                                                         |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| inference                       | completed; the one traced model request carried 0 tools; 0 approvals, 0 actions; every action gate `local_tools_profile_missing` |
| command accept                  | nothing ran while pending; 1 Accept, 1 execution, confirmed; the retried intent returned the same receipt and wrote nothing      |
| command decline, command cancel | provider `cancel` 1, executions 0, turn interrupted                                                                              |
| file accept                     | absent while pending; 1 Accept, 1 mutation, exact content                                                                        |
| file decline, file cancel       | provider `decline` or `cancel` 1, mutations 0                                                                                    |
| lease revoked while pending     | approval `invalidated`, a later Accept `invalid_transition`, action 0; without a lease a turn is `gate_denied`                   |
| runtime or authority lost       | the ten cases of the crash matrix: no replay                                                                                     |
| forbidden surfaces              | 0 violations; processes of a generation: `bwrap` and `codex`; hooks, MCP servers and enabled features attested before every turn |
| event replay                    | the Replay section above                                                                                                         |

As in ADR 0016, the tool inventory of the local-tools profile is not traced natively,
because its launcher removes the variable that enables the trace. It is established by
the attested configuration, by the normalized catalog and by the fact that every
surface other than the two mediated tools is a violation, of which there were none.

## Accepted residual risks

- The credential capability and its two residuals above.
- The provider's sandbox is the enforcement between an Accept and its effect, and a
  pre-existing hard link in a workspace reaches outside it.
- The same-user boundary: no protection against another process of this user.
- Integrity checks detect damage, not forgery. The witness trails the head by at most
  one iteration of the core loop.
- Unknown event types remain tolerated in a store a newer build has opened.
- Exactly-once execution by the provider is not claimed.
- The local-tools profile is not durable across an authority restart, by design.
- Compatibility is pinned to Codex 0.159.0; the profile needs bubblewrap with overlay
  support.
- The runtime's working directory is the repository root compiled into the binary
  (ADRs 0010 and 0012). A binary moved from its build tree starts no runtime. Packaging
  has to replace it.
- `sha2`, `serde` and `serde_json` are caret requirements; the lockfile and `--locked`
  pin them. No dependency was added, removed or upgraded by this sprint.
- This review was one independent pass. The fixes it made have not themselves been
  reviewed by a second party.

## Deferred, outside the core

Agent UI, the desktop connection to the authority, other adapters, packaging,
installation and autostart, remote transport, policies, retention and compaction,
cost and analytics.
