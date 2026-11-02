# Project and workspace contract

Implemented in `src-tauri/src/contracts/m1.rs` and `src-tauri/src/projects/`; adoption and reference-installation acceptance remain open in [STATUS](../STATUS.md). This specification owns identities, storage, commands and preservation oracles.

## 1. Ownership and compatibility

One new desktop backend project module owns presentation metadata. It must not open a
NativeService/Registry to inspect a live registry: `Registry::open` writes and marks
unfinished records Lost. It must not use lazy `HostLink::call` for discovery/recovery:
that path may spawn a host, create a local fallback and retry a failed operation.
Existing `restoreOrCreate` may remake missing shells; M1 recovery must use an attach-only
path instead. Leave existing host, authority, profiles and configuration writers owning
their state. No M1 SQL migration, authority restart or runtime-capacity change.

HostId is a random installation UUID persisted in the new metadata store, not a machine
name, boot ID, path or PID. A local terminal reference also needs an ephemeral host epoch.
For an old host without an instance feature, derive that observation from socket identity,
SO_PEERCRED and peer process start ticks; never persist PID as session authority. Every
read/input/resize/close uses a connection whose peer is checked **on that same connection**
before sending. Replaced/missing/unverifiable peer => `host_changed`, no automatic retry,
spawn or fallback. A future instance feature can replace this compatibility observation.
PID reuse/start-tick denial fails closed. These checks do not establish human approval or
contain a hostile same-UID process. Do not require a live-host upgrade to browse/adopt work.

Managed references include protocol, authority/store stream ID and session ID. Every
effect still uses the existing authority-instance/runtime/generation/revision fence.
Never translate P1 IDs into P2, correlate by title, or infer a lease from project selection.
A Native session remains unrestricted under the user's provider permissions. No Claude
Managed entry can launch until M3 qualifies it. Unsupported features disable only their
own controls with a reason, leaving inspection/global administration accessible.

## 2. Versioned storage and validation

Store: `$XDG_STATE_HOME/wyss-console/projects/v1/store.json` (fallback
`~/.local/state`), directory 0700, file 0600, user-owned non-symlink ancestors, atomic
same-directory write + fsync file/directory. One transaction holds an interprocess file
lock, rereads the current revision, validates and compare-and-swaps the entire store.
No watcher writes a older file. Contending windows return `revision_conflict`; refetch,
never silently last-write-wins. Do not hold this lock across native dialogs/provider IO.

Closed JSON types; reject unknown fields, duplicate JSON keys, unknown enums, nonfinite
numbers, invalid UTF-8, references to absent/wrong-host entities and revision overflow.
IDs are canonical lowercase UUIDs; counters are positive decimal strings (u64, no leading
zeros) to avoid JavaScript precision loss; timestamps are UTC RFC3339, informational only.
Optional values use null; no arbitrary JSON bags. Times/IDs come from the backend.
Paths stay private/local; frontend DTOs use root label/eligibility, not trusted path input.

| Record               | Exact fields and meaning                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| StoreV1              | `schema:1`, `storeId:UUID`, `revision:Counter`, `hostId:UUID`, `projects:Project[]`, `bindings:Binding[]`, `workspaces:Workspace[]`, `sessions:Association[]`, `operations:Operation[]`, `closedOperations:UUID[]`, `migration:Migration \| null`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Project              | `id`, `label:Text`, `createdAt:Time`, `updatedAt:Time`, `revision`, `archived:bool`, `tombstone:bool`, `defaultBindingId:UUID \| null`, `defaultWorkspaceId:UUID \| null`, `profileDefaults:{claude:ProfileRef \| null,codex:ProfileRef \| null}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ProfileRef           | `id:NativeId`, `version:Counter` (1..u32::MAX); existing owner resolves availability, no profile body/credential copy                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ManagedSafetyProfile | Closed, provider-qualified requested profile: `inference` (text-only) or `local_tools` (mediated effects); neither grants execution or approval privileges.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Binding              | `id`, `projectId`, `hostId`, `revision`, `path:AbsolutePath`, `observation:{device:Counter,inode:Counter}`, `checkoutKind:folder \| git \| worktree`, `state:valid \| missing \| identity_changed \| unverified \| retired`, `previousBindingId:UUID \| null`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Workspace            | `id`, `projectId:UUID \| null`, `hostId`, `revision`, `label`, `layout:Node \| null`, `focus:UUID \| null`, `pinned:bool`, `leaves:Leaf[]`, `preferences:{provider:claude \| codex \| null,mode:native \| managed,nativeProfile:ProfileRef \| null,managedSafetyProfile:ManagedSafetyProfile \| null}`                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Node                 | `{kind:leaf,id:UUID}` or `{kind:split,dir:row \| col,ratio:number,a:Node,b:Node}`; leaf ID names a workspace Leaf                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Leaf                 | `id`, `title:Text`, `associationId:UUID \| null`; null means an empty placement, never permission to launch                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Association          | `id`, `originProjectId:UUID \| null`, `hostId`, `bindingId:UUID \| null`, `bindingRevision:Counter \| null`, `target:NativeTarget \| ShellTarget \| ManagedTarget`, `state:attached \| detached \| unavailable \| ambiguous`, `operationId:UUID \| null`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| NativeTarget         | `kind:native`, `sessionId:NativeId`, `terminalId:UUID`, `hostEpoch:EpochText`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ShellTarget          | `kind:shell`, `terminalId:UUID`, `hostEpoch:EpochText`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ManagedTarget        | `kind:managed`, `protocol:1 \| 2`, `storeStreamId:UUID`, `sessionId:UUID`, `requestedSafetyProfile:ManagedSafetyProfile \| null`; a display-only request, never proof of effective authority profile                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Operation            | `id`, `kind:launch`, `projectId:UUID \| null`, `workspaceId`, `leafId`, `bindingId:UUID \| null`, `bindingRevision:Counter \| null`, `provider:claude \| codex \| shell`, `mode:native \| managed`, `nativeProfile:ProfileRef \| null`, `managedSafetyProfile:ManagedSafetyProfile \| null`, `managedAuthority:{protocol:1 \| 2,storeStreamId:UUID} \| null` (the authority the launch was first asked of: present exactly for a Managed launch, and the one a Managed association of the receipt must name), `state:pending \| committed \| ambiguous \| refused`, `associationId:UUID \| null`, `claim:EpochText \| null`, `createdAt`; a durable requested-profile receipt, never an approval, credential or executed-profile attestation |
| Migration            | `version:1`, `id:UUID`, `sources:{kind:registry \| layout,digest:SHA256}[]`, `map:{sourceKind:registry_project \| registry_session \| layout_leaf,sourceKey:Text,targetId:UUID}[]`, `state:committed`; sourceKey is an opaque source-record digest, not a public path                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |

`id`, `projectId`, `hostId`, `workspaceId`, `bindingId`, `associationId` above are UUIDs;
`revision` is Counter; `label`/`title` are Text. `profileDefaults` is presentation for
**Native** launches; Managed ignores it. `managedSafetyProfile` is a separate, closed
requested-profile selector, never a Native `ProfileRef` or proof of the authority's
actual attached profile. Preferences obey the same cross-field rule as launches (native mode requires `managedSafetyProfile=null`, managed mode requires `nativeProfile=null`) and prefill the launch choice only, and never storing `local_tools` (a preference holds `inference` or null; `local_tools` is chosen
explicitly on each launch). Preferences never grant a lease, authorize tools, or trigger a launch. A new Managed launch must
explicitly choose `inference` or `local_tools` from capabilities advertised by that
provider/authority; unsupported or unverified selections refuse with zero side effects.
Native launches require `managedSafetyProfile=null` and use `nativeProfile` if present;
Managed launches require `nativeProfile=null` and nonnull `managedSafetyProfile`;
plain shells require Native mode and both profiles null. A stored requested choice is
validated again against the current owner before effectful dispatch. Scratch has
exactly one workspace with projectId null, native preferences, no binding and label Scratch;
non-tombstone projects each have at least one workspace. Tombstones retain id/label/history only; defaultWorkspaceId is null for them (UUID otherwise). `NativeId` is exactly 32 lowercase hex characters (existing Native session/profile format),
not the hyphenated UUID format of new metadata and terminal/Managed IDs. Preserve owner IDs
byte-for-byte; an incompatible ID is an unresolved import, never coerced. `hostEpoch` and `claim` are epoch text (non-empty, no control characters), not Text. Epoch text is an
opaque bounded compatibility observation, max256 UTF-8 bytes. No runtime epoch survives
validation against a changed host.

Bounds: store <=4 MiB **before JSON parsing**, <=256 projects, <=32 workspaces/project
plus Scratch, <=16 bindings/project including relink history, <=2000 associations and
<=512 operation receipts. Refuse admission at a ceiling; no automatic project/session
history eviction. Explicitly compact only terminal operation receipts with no ambiguous
work, preserving their associations and migration mapping; each compacted ID moves to `closedOperations` (<=4096, refuse compaction and admission at the ceiling). Text is 1..64 Unicode scalars,
no controls; AbsolutePath <=4096 UTF-8 bytes, absolute, with no control character and no empty, `.` or `..` segment; total migration mapping <=4096 rows.
Each workspace <=16 leaves, binary depth <=6 (root level0), ratio0.1..0.9, focus references
its own leaf, no repeated leaf/association within one workspace. This preserves the current
binary tree depth6, refining the earlier proposed depth8. Existing host limit16 PTYs is
shared across workspaces/windows; this store cannot allocate another capacity pool.
Managed still cap1 independently. Bounds are proposed safety ceilings, not measured costs.

Store invariants, validated on decode and on encode: IDs unique per kind (leaf IDs across the
whole store) and on this store's host; every reference exists; a live project's default
workspace and binding are its own, a tombstone has neither and owns no workspace or binding;
exactly one Scratch workspace; each workspace's leaves are exactly the leaves its layout places,
each placed once, with focus and associations valid and no association twice in one workspace;
one association per owner target (a terminal is one target whether it runs Native or a shell,
and a Native session ID is held once); a bound association belongs to its binding's project; a workspace shows only sessions whose origin is its own project (Scratch: origin null), so a session moves to a project by explicit adoption, never because a layout names it, and unassigned or unavailable history stays discoverable without an invented owner;
bindings chain inside one project without a cycle; migration sources are one or two distinct
kinds. Receipts: `pending` has a `claim` and no association and names an existing leaf, project
and binding that agree; only `committed` must have an association, `ambiguous` may keep the
one a lost acknowledgement left; at most one unfinished receipt per leaf, whether or not the
leaf still exists; where a receipt and its association both exist they cite each other and
agree on kind (Native, shell or Managed) and on the requested Managed profile; `committed`,
`refused` and `ambiguous` receipts are history and may outlive the records they name; a
compacted ID is never live. Revisions at the end of the u64 range are refused, and split
ratios must survive a round trip exactly. A newer schema is recognised only in a store that
is otherwise well-formed JSON without repeated members.
Preferences hold `inference` or null as the Managed profile, never `local_tools`.

On corrupt/newer/oversized store: read-only recovery screen, preserve exact bytes, no empty
fallback/save. Missing store: offer migration preview; no auto terminal. Deleted or older
profile: show unavailable and require a new explicit choice. Binding relink creates a new
binding with previousBindingId, retires the old binding for future effects, retains original session attribution, and CAS-updates only
future-launch selection; missing/changed roots block new scoped effects, not observation.
Native chooser and configuration-root eligibility remain separate, as FP-CTX-02 requires.

## 3. Typed operations and UI state

Common mutation envelope: `{schema:1,operationId:UUID,expectedStoreRevision:Counter,
context:{hostId,projectId:UUID|null,workspaceId:UUID|null,bindingId:UUID|null,
bindingRevision:Counter|null,selectionRevision:Counter},command:Command}`.
Backend validates all parent relationships and records an immutable context before IO.
Selection revision is per window, not execution identity; changing it invalidates only
unconfirmed plans. Side-effect receipts always name their captured context.

Closed Command variants (tag `kind`, exact remaining fields):

| Kind                                     | Payload / rule                                                                                                                                                                                                                                                                                                                                                                                                        |
| ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `registerExisting`                       | `selectionToken`; backend-owned one-use native chooser token, descriptor-revalidated root; duplicate canonical root on host returns existing project                                                                                                                                                                                                                                                                  |
| `createFolder`                           | `selectionToken,name`; native-confirmed parent + one basename (no separators/dot segments); mkdir exclusively, no Git/launch; failed store commit leaves visible unregistered empty folder with explicit recovery, never deletes a now-nonempty folder                                                                                                                                                                |
| `relink`                                 | `projectId,oldBindingId,selectionToken`; display old/new comparison and per-operation eligibility before commit                                                                                                                                                                                                                                                                                                       |
| `renameProject` / `archiveProject`       | `projectId,label` / `projectId,archived`; archive hides recent item, preserves attention/live work                                                                                                                                                                                                                                                                                                                    |
| `removeProject`                          | `projectId,retainHistory:bool`; refuse active/ambiguous/unavailable-not-proven-ended targets; retainHistory keeps a tombstone Project (no active workspace/binding); otherwise remove proven-ended associations/workspaces/bindings. No filesystem/provider/vault deletion                                                                                                                                            |
| `createWorkspace` / `duplicateWorkspace` | `projectId,label` / `sourceWorkspaceId,label`; duplicate uses new leaf IDs, all associations null; copy preferences only                                                                                                                                                                                                                                                                                              |
| `saveWorkspace`                          | `workspaceId,expectedWorkspaceRevision,label,layout,focus,pinned,leaves,preferences`; same-store CAS, all association origins must match project except explicit Scratch adoption                                                                                                                                                                                                                                     |
| `closeWorkspace` / `deleteWorkspace`     | `workspaceId`; close releases views/input only; delete detaches associations, replaces deleted default with first remaining workspace or a new empty default                                                                                                                                                                                                                                                          |
| `attach` / `detach`                      | `workspaceId,leafId,associationId`; verified owner lookup, never launch; detach does not close PTY/runtime                                                                                                                                                                                                                                                                                                            |
| `adoptScratch`                           | `associationId,projectId,bindingId`; only origin-null ShellTarget, confirmed actual cwd disclosure; assignment of origin once, no cd/mode/permission change                                                                                                                                                                                                                                                           |
| `launch`                                 | `workspaceId,leafId,provider:claude \| codex \| shell,mode:native \| managed,nativeProfile:ProfileRef \| null,managedSafetyProfile:ManagedSafetyProfile \| null`; Native/shell: `managedSafetyProfile` null; Managed: `nativeProfile` null and `managedSafetyProfile` an explicit per-launch choice; fresh binding + eligibility + owner availability, explicit user action, no free argv                             |
| `reconcileLaunch`                        | `launchOperationId,resolution`; `resolution` is `identified{terminalId,sessionId\|null}` (the user found the session an unknown launch started among those no association holds), `absent` (the owner is readable and shows nothing no association holds) or `retryManaged` (the Managed owner is asked again with the receipt's original intent); only the user sends it, and none of the three starts a new process |

Reads `listProjects`, `getProject`, `getWorkspace`, `listSessions` are bounded/paginated
(max100 rows/page, opaque store-revision cursor); return source revision, availability and
capability reasons. `previewMigration` reads only and returns counts/conflicts/source
digests; `commitMigration` requires that exact preview ID/digests and expected absent store.
Global navigation/settings do not require a project. Contextual sections use the project
breadcrumb; selecting A→B changes presentation only, does not call launch/stop/cd/resume.
Unsaved native editor text requires the existing warning; no silent write. Unconfirmed
plans/dialog selections expire on switch. Confirmed work completes only in its captured
context; global attention opens the exact target without retargeting a composer.

One app-coordinated input claim per host epoch/terminal, held by backend interprocess
lock and released on blur/detach/crash; mirror views are read-only. Taking input is explicit.
Resize only by input owner; read cursors stay per viewer, gaps disclosed. Never stop or
resize a terminal merely to select a project. This is cooperative client coordination,
not isolation against older/direct same-UID writers; compatibility warning is visible.

Launch journal: persist pending before dispatch, then committed only after exact owner
receipt and association CAS. Refusal before dispatch => refused. Loss after possible
dispatch => ambiguous, expose recovery and block automatic retry. Existing Native launch
has no caller-idempotency receipt: without unambiguous returned ID, do not guess by cwd,
name or timing and do not issue another launch; user identifies the original target or
confirms it absent after owner reconciliation. Existing Managed intent retry uses the same
original intent/fence only. The launch operation receipt immutably records provider,
mode, Native profile reference or requested Managed safety profile as applicable,
with the captured workspace/context. A Managed association may retain that requested
choice for display, but the current effective profile must be freshly obtained from
the owning authority and matched to the requested choice and generation before any
attach, lease or turn. Old/unknown imports use null and remain read-only until verified;
never infer `local_tools` from workspace, path or UI state. The authority's existing
Inference/Local Tools selection is mapped using its verified provider-specific API,
not by a frontend safety-profile override or a Native profile field. No F8 protocol,
authority permission or runtime capacity changes are authorized by this contract.
App restart never replays pending operations or prompts.

Idempotency and recovery. `operationId` is unique in the store; under the store lock every
mutation looks it up first. In `closedOperations`: refuse `invalid_record`, effect `unknown`,
never proceed. Absent: proceed. Present with every immutable receipt field (kind, workspaceId,
leafId, projectId, bindingId, bindingRevision, provider, mode, both profile fields) equal to
the request: return the stored state and association, with no new dispatch. Equality excludes
`selectionRevision` (per window, not execution identity) and `expectedStoreRevision`. Present
with any other difference: refuse `invalid_record`, effect `none`. Concurrent submissions
(two windows, double click) serialize on the lock and `expectedStoreRevision`: exactly one
writes `pending` and may dispatch, the others read it and return its state. Different
`operationId`s never bypass this: refuse `leaf_busy`, effect `none`, while any non-terminal
(`pending` or `ambiguous`) Operation exists for the same (workspaceId, leafId). `pending`
records a `claim`, the dispatching backend process's epoch observation (same rules as
`hostEpoch`, never a PID as authority). A `revision_conflict` while recording the owner receipt
re-reads and re-applies only that receipt and association; it never re-dispatches. If the leaf,
workspace or binding revision changed meanwhile, the receipt becomes `ambiguous` and the
association stays unattached and discoverable, never silently dropped. Commands without a
receipt cannot duplicate an effect on replay: stale `expectedStoreRevision` gives
`revision_conflict`, and `registerExisting` returns the existing project. On load, a `pending`
receipt whose `claim` no longer matches a live backend process is CAS-updated to `ambiguous`
(a crash before or after dispatch cannot be told apart) and is never replayed; a live claim
is left alone. Transitions: `pending` to `committed`, `refused` or `ambiguous` only by the
claim holder; `ambiguous` to `committed` or `refused` only by an explicit user reconciliation
against the owner (target identified or confirmed absent), recorded by CAS. Any new launch
needs a new `operationId`. A command that makes no receipt of its own may not reuse an ID a receipt owns or one
already in `closedOperations`: it is refused `invalid_record`, effect `none` for an open receipt and `unknown` for a closed ID. `context_stale` is evaluated against the immutable captured
context before any dispatch, never against current UI selection.

Errors: `unsupported_schema`, `invalid_record`, `capacity_exceeded`, `revision_conflict`,
`context_stale`, `leaf_busy`, `root_missing`, `root_identity_changed`, `root_ineligible`, `host_offline`,
`host_changed`, `unsupported_capability`, `profile_unavailable`, `owner_refused`,
`effect_ambiguous`, `io_failure`. Failure response includes original operation/context,
known effect `none|committed|unknown`, and safe next action; never raw provider/config bytes.

## 4. Migration, preservation and rollback

1. **Read-only preview.** Use bounded strict older registry schema1 (2 MiB,200 sessions,
   100 known folders) and saved layout v1 (64 KiB,16 tabs,16 panes/tab,depth6). Validate
   owner/mode/descriptor and consistent bytes/digest before/after read. If sources change,
   retry preview, never stop their writers. No Registry::open or HostLink lazy startup.
2. Generate one migration UUID/storeId/HostId in preview memory; deterministic name-based
   mapping (SHA-256, UUID version-8 layout, no new dependency) within that migration namespace from source kind + stable older key. Retry of
   that preview yields the same map. Canonical exact root duplicates coalesce after native
   validation; aliases/ambiguous roots require user resolution. Missing roots remain
   unverified/read-only. Do not infer projects from cwd prefixes, Git URL or directory names.
3. Read current host through non-starting checked connection, validate every terminal/native
   tag against its live owner. Map known exact-root sessions; unmatched shells/native work
   stays in Scratch/unassigned recovery view (Native is not adoptable by adoptScratch).
   Import each older tab into a separate named workspace (up to16), splitting mixed-project
   tabs by verified origin, and set the first as default. Preview must refuse any resulting
   workspace/leaf count above its bound; never truncate. Prune absent refs into empty leaves,
   never remake shells. Missing agents/history remain discoverable metadata. Never infer
   an ended process merely from a missing layout/registry row.
4. P1/P2 session lists remain separate projections with source store ID; only exact owner
   workspace binding plus explicit project match can associate Managed work. Unknown/root-
   ambiguous work stays unassigned and cannot receive project-scoped commands. No SQLite
   read/write migration and no provider thread resume on import.
5. On explicit commit, recheck preview digests/owner epochs and all bounds. Preserve older
   registry/layout bytes in a private migration snapshot (no profile/auth/vault copy), hash
   manifest; stage new validated store and fsync, atomic publish last. Never overwrite the
   source. Crash before publication => old reader; after => fully valid new reader/map.
   A private snapshot contains user paths: never log, commit or attach it to a PR.
6. New UI exclusively owns its new store; old windows continue owning old layout. Detect
   older changes and offer incremental import with a **new** preview; never merge them
   silently or overwrite newer workspace state. UI feature flag selects reader, not host
   lifecycle. Reopening old UI can still recreate missing shells by old behaviour: disclose
   this before rollback, prefer attach-only compatibility view if that risk is unacceptable.
7. Rollback is an owner-reviewed GUI/metadata operation: record post-snapshot new records,
   export mapping privately, confirm no unaccounted/ambiguous operation, disable new reader.
   Keep v1 store/tombstones/snapshot for forward recovery; never restore older bytes over
   live writers. Installer rollback can close terminals and is **not** this UI rollback.
   No host stop, SQL downgrade, authority switch, provider-setting restoration or PID cleanup.

This contract describes the procedure; it does not execute it. It does not create snapshots of personal records, publish
stores, install a GUI, or execute rollback. Observe before/after current service start
identities/socket inodes and older metadata; differences require investigation, not
cleanup. Absence of a difference is preservation evidence, not proof of provider fidelity.

## 5. Acceptance oracles

The schema, locked store, projection, migration and context integration exist.
Adoption still requires the oracles below, agreed reference budgets and a session-preserving
installation. None grants permission to start Claude Managed or raise runtime capacity.

| Required test ID | Exact oracle / acceptance                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | FP/AC                |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| M1-01            | Valid empty Scratch/project/Native/P1/P2 records round-trip; unknown/duplicate keys, schema2, dangling IDs, invalid/mixed Native-vs-Managed profile choices, forbidden secret/prompt/argv fields, u64 overflow refuse before save; byte/count/depth at bound and +1                                                                                                                                                                                                                                                     | CTX01/03             |
| M1-02            | Native chooser cancel => zero mkdir/store writes; duplicate root returns existing; symlink/foreign ACL/replaced descriptor refuses; HOME Native allowed only by its owner, config HOME denied                                                                                                                                                                                                                                                                                                                           | CTX02                |
| M1-03            | Missing/replaced/relinked root => metadata visible, new effect denied, existing target unchanged; old binding retained for live association                                                                                                                                                                                                                                                                                                                                                                             | CTX02                |
| M1-04            | Two windows CAS same revision => exactly one commit, other conflict; lock/crash/write/fsync/rename faults leave complete old or new bytes, never empty fallback                                                                                                                                                                                                                                                                                                                                                         | CTX01/04             |
| M1-05            | Switch A→B during chooser, config plan, confirmation and delayed receipt => no B mutation; pending plan expires, confirmed effect/receipt remains A; cross-project Undo refuses                                                                                                                                                                                                                                                                                                                                         | CTX04                |
| M1-06            | Duplicate/close/delete/archive/navigation/reopen => zero spawn/stop/resume/lease calls; default removal leaves empty replacement; detached sessions discoverable; Scratch no providers/authority                                                                                                                                                                                                                                                                                                                        | CTX01/03             |
| M1-07            | Registry/layout corpus: duplicate roots, absent host, missing agents, malformed/newer files,16 tabs, changed sources, interrupted publication and second import; deterministic map; no Registry::open, no shell remake, no older writes                                                                                                                                                                                                                                                                                 | CTX01..03            |
| M1-08            | P1/P2 same-looking IDs never conflate; wrong store/epoch/generation/context or requested-vs-effective Managed safety profile denies attach/input/effect; project association never changes Managed binding or grant                                                                                                                                                                                                                                                                                                     | CTX01/04             |
| M1-09            | Two mirror views => one input/resize claim, other read-only; owner crash frees claim; replaced socket/PID reuse/unreadable start ticks => refusal before send, zero reconnect replay                                                                                                                                                                                                                                                                                                                                    | CTX01/03             |
| M1-10            | Native dispatch succeeds then association save/ACK lost => ambiguous, zero second launch; same `operationId` twice (identical => stored state, different => `invalid_record`) or from two windows => one dispatch; a different `operationId` on a busy leaf => `leaf_busy`; `pending` with a dead claim found at load => `ambiguous`, with a live claim untouched; compacted ID => refused; Managed receipt retains the explicit requested safety profile and retry uses original intent; startup never replays journal | CTX01/04             |
| M1-11            | Purge active/unknown/ambiguous work refuses; archive preserves execution; retained history tombstone survives; zero project-file/provider-history/vault deletion                                                                                                                                                                                                                                                                                                                                                        | CTX02                |
| M1-12            | Old host missing feature remains browsable; incompatible command disabled; recovery uses checked attach-only path, host start time/socket/store IDs unchanged                                                                                                                                                                                                                                                                                                                                                           | CTX01/03             |
| M1-13            | Owned synthetic 1/16 terminals and 2/32 workspaces: switch/idle/restore trace, listener count and bounded buffers; three runs of matched before/after procedure below, raw outliers retained                                                                                                                                                                                                                                                                                                                            | CTX01..04, FP-NFR-01 |

Fixtures must use synthetic metadata and fail-on-call spawn/stop/provider/config writers.
No test requires real credentials, a live service restart or installed-store migration.
Required native desktop journey after owner
acceptance: A/B/Scratch, Native Claude/Codex and existing F8 visible in correct scope,
archive/reopen, GUI-only recovery, two windows, loss of owner connection. Zero duplicate
processes/prompts and unchanged live ownership are explicit release oracles.

Efficiency: one shared owner inventory per window/backend with bounded subscriptions,
no polling loop per inactive workspace, virtualized project/session lists, lazy renderer
attachment and latest-revision debounced metadata saves (400ms starting point from current
layout). Do not duplicate PTY readers/output history for inactive tabs. Measure whole
GUI/WebKit/host/authority owned trees separately, input-to-render, project switch p50/p95/
p99, restore time, RSS/PSS, CPU and wakeups. Follow the [measurement contract](acceptance.md): three independent runs,
warmup/sample rules and machine/load/build disclosure. No numeric latency budget is established by a specification. Before M1 release, agree measured reference budgets; refuse an unexplained regression
outside baseline uncertainty and retain failed/outlier samples. A fixture-only latency pass
cannot close the real-desktop performance requirement.

## 6. Implementation-specific behavior

What the implementation decided where the proposal left room, and what it does not do. Anything
here that disagrees with the sections above is a defect to fix, not a second contract.

**Release gate.** Every command, reads included, answers `unsupported_capability` (field
`disabled`) until the owner turns the workspace on; the one exception is `projects_overview`, which
only says it is off. With the gate off nothing is read or made: no `projects/` directory, no store
file, no call to the terminal host, no Managed interface. The gate is the private file
`$XDG_STATE_HOME/wyss-console/projects/feature`; it is switched by `projects_set_enabled`, which
asks in a native confirmation window the page cannot draw on, and turning it off is refused while a
launch of unknown outcome exists. The Projects entry in the navigation exists only while it is on.
Turning it off changes no session, no store and no older window.

**Windows.** Another window is `w-<id>`, opened by `projects_open_window`, and carries only the
`projects` capability. Windows follow the store by one poll of its revision (2.5 s, only while the
view is shown), not by a push; a change from another window or process is seen at the next poll. At most
eight extra windows are open.

**Owner reads.** The host is read through `HostSocket`: every connection checks the peer's uid,
pid and process start time and the socket file's identity, and the epoch it yields
(`h1:<device>:<inode>:<pid>:<start ticks>`) is compared on every later call. A different epoch is
`host_changed`, an absent host `host_offline`; neither starts, retries or falls back. The terminals'
output is read through the same socket (`projects_term_read`), so a recovered view never creates a
shell. Managed sessions come from the desktop's own P1 and P2 interfaces, each under its
authority's stream ID, bounded to eight pages.

**Managed launch prepares a session and nothing else.** It first reads the one authority the launch
would go to (the protocol-2 interface when it is active, the installed authority otherwise) and
what that authority says of itself, and refuses with nothing written unless that shows it serves
the requested profile. No wire field lists the profiles an authority supports (adding one would
change the authority protocol, which this contract may not do), so a profile counts as advertised
only when the authority's own status proves what it needs: Inference needs the greeting of that
interface's own protocol and storage schema (protocol 1 / schema 7, protocol 2 / schema 8);
Local Tools, the privileged profile, also needs the authority to report the same version of the product as this desktop
(the rule the protocol-2 client already applies to everything; the version string is the only build identity an authority reports), so an older protocol-1 authority is
offered Inference only. An authority that cannot be read, or that proves nothing, refuses
`host_offline` or `unsupported_capability`; the window's state is never evidence. The receipt
records the authority (`managedAuthority`: protocol and stream) before the owner is asked, the
owner is sent that authority and refuses (`host_changed`, nothing sent) if the authority that would
answer is another one, and a reply that names another authority is ambiguous, never recorded as
this launch's. The operation ID is the intent, and the requested profile is recorded. Binding a
workspace, starting a runtime, leases and turns stay in the Managed screens, which read the request
as a hint and grant nothing from it. A launch whose outcome is unknown is retried only by the user
(`reconcileLaunch retryManaged`), with the same intent, of the recorded authority and no other: a
desktop that has moved between protocol 1 and 2 refuses (`host_changed`, effect unknown, nothing
sent) rather than prepare a second session elsewhere, and the profile must be shown again. `absent`
for a Managed launch is judged by that same authority's sessions. Claude Managed is
`unsupported_capability`.

**Launch journal.** `pending` carries a `claim` (`c1:<pid>:<start ticks>:<nonce>`); on start the
service makes `pending` receipts of a gone process `ambiguous`. Receipts are compacted only by an
explicit action (`projects_compact_receipts`); a compacted ID stays in `closedOperations` (at most 4096) so it can never be live again.

**Terminals.** A Scratch shell starts in the host's own default directory. A session the window
started takes input as its pane opens (a new shell asks the terminal questions only the holder of
input may answer); every other view is a read-only mirror until input is taken, and input is
released on blur, on detach and when the window or process ends. Input is sent one call at a time,
in the order typed.

**Migration.** Preview reads the files of the older windows as bytes (never `Registry::open`), twice, and refuses
a source that changes under it. A folder that is missing or not usable is not imported and is not
invented; an alias of a registered root needs an explicit acceptance; tabs with nothing alive are
skipped; a tab that mixes origins becomes one workspace per origin; all Scratch panes share the one
Scratch workspace in a balanced layout. Commit preserves the exact bytes in
`migration-<id>/` with a hash manifest, then publishes the store last. A later preview is
incremental: it adds registered folders and live sessions the store lacks and never changes or
removes a record it has. Profile defaults of the older registry are not imported. A preview that has a blocking conflict is
never committed, whatever aliases were accepted, and an update preview is committed at the store
revision it was made against or not at all (`revision_conflict`, then a new preview).

**Unknown launches.** A Native or shell launch of unknown outcome is resolved by looking
(`projects_orphans` lists the running terminals no association holds, of the kind the launch asked
for, by the last component of their folder) and picking one (`identified`), or by saying nothing
started (`absent`, refused while the owner shows something nobody holds). Only a Managed launch
offers to ask its owner again. A launch is settled from the project and workspace that made it. A
store at its session ceiling refuses a launch before the owner is asked.

**Operation IDs.** The ID is looked up under every command before the store's revision or the
window's selection are weighed. A launch already known answers from its receipt (`replayed`, naming the
association it made) whatever the store or the window has become since; the same ID with any other
field of the request, or in `closedOperations`, is `invalid_record`. A command that makes no receipt of
its own may not reuse an ID a receipt owns or one already closed. A window that loses the race to
record the same launch (its compare-and-swap fails) reads what the winner recorded and answers from it,
never with `revision_conflict`, and never sends anything.

**Adopting a shell.** Where a Scratch shell really is is read from its process (`/proc/<pid>/cwd`, the
user's own process, the same start time before and after, the pid the host reports) through
`projects_adoption_preview`, never from where it started. The page shows that directory and whether it is
the project's folder or inside it; `adoptScratch` goes through only with a directory shown for that
association, project and binding, still the same directory of the same process when it is
applied, and used once; a disclosure belongs to the process, not to a window, so selecting a project or
workspace in any window drops it (the move then asks again). A directory that cannot be read, or has changed, or was not shown, refuses
(`owner_refused` / `context_stale`, field `cwd`) and nothing moves.

**Project folders.** A root is looked at through the descriptor opened on it (not the path). Besides
the owner and no link, it must not be writable by anybody else, by its ACL, its mode bits or the
default ACL it gives what is made in it, except the accounts the owner named in
`configuration/trusted-writers`, the file the configuration engine already believes; an ACL that cannot
be read or understood is not private. A folder refused for this is `root_ineligible` (field
`foreignAcl`); in an import it is not imported (`root_shared`).

**Efficiency as built.** One shared owner inventory: a display read of the host and of the Managed
authorities is kept for one second for every window (one read under a lock serves windows that ask
together) and forgotten after any command; whatever decides or does something reads the owner itself.
No timer exists per workspace: one revision poll per shown window, none while hidden. A renderer and its
reader exist only for a pane that is mounted, and a pane is mounted only while its workspace is in view
and the view is visible. The project rail and the sessions list draw only the rows in view past 60 rows
(fixed row height; whatever grows a row is shown outside the list). A divider moved is shown at once and
saved once, 400 ms after the last move, flushed before anything else is sent, before the view goes
quiet and before another project or workspace is selected; a move made on a workspace that has since
changed is let go, so it never overwrites the change. One write to a terminal is at most 4 KiB, the host's own limit (the window sends a longer paste in
pieces of that size, in order, and stops at the first one refused); at most eight extra windows are open.

**The older terminal view and a second writer.** The host takes one write at a time for a terminal and
refuses another as `busy`, before any byte is taken. The older Console view closes a terminal when one of
its writes fails, so a pane holding input on a terminal the older view also shows could destroy it. A
`busy` write is therefore offered again, 40 times 25 ms apart, by the older view and by the Projects
side; any other refusal, or a terminal that stays busy, ends the attempt as before.

**Not done here** (see the delivery record for why each is optional or a limit, not a contract
requirement):

- A push channel between windows; a way to browse Managed sessions older than the owner's first eight
  pages; any change to the host, the authorities, their stores, the F8 capacity of one or the
  installed package.
- Agreement of the GUI reference budgets, which needs the owner's own installation (the figures and the
  proposal are in the delivery record); the owner's installation has not been migrated.
- A removed project whose folder is still in the older registry is offered again by an update preview,
  and an accepted alias is asked again each time (explicit offers, never silent).
- A launch of unknown outcome cannot be settled while the store is at its session ceiling.
