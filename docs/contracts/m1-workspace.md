# M1 workspace implementation contract

**Proposed, not implemented.** Engineering baseline: main
`d2ee8d2971ed50de2e39d73ad71f2a29f14c42b2`, 2026-10-22. Implements only
[FP-CTX-01..04](../product/functional-specification.md#fp-ctx-01), with
[ADR0052 Accepted (architecture only)](../adr/0052-project-centered-workspace-identities.md).
This is the detailed contract owned/referenced by [architecture](../product/architecture.md);
current evidence stays in [status](../product/implementation-status.md#m0-engineering-baseline).
All DTOs, filenames and tests below are implementation proposals, not existing APIs.

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

Propose `$XDG_STATE_HOME/wyss-console/projects/v1/store.json` (fallback
`~/.local/state`), directory 0700, file 0600, user-owned non-symlink ancestors, atomic
same-directory write + fsync file/directory. One transaction holds an interprocess file
lock, rereads the current revision, validates and compare-and-swaps the entire store.
No watcher writes a legacy file. Contending windows return `revision_conflict`; refetch,
never silently last-write-wins. Do not hold this lock across native dialogs/provider IO.

Closed JSON types; reject unknown fields, duplicate JSON keys, unknown enums, nonfinite
numbers, invalid UTF-8, references to absent/wrong-host entities and revision overflow.
IDs are canonical lowercase UUIDs; counters are positive decimal strings (u64, no leading
zeros) to avoid JavaScript precision loss; timestamps are UTC RFC3339, informational only.
Optional values use null; no arbitrary JSON bags. Times/IDs come from the backend.
Paths stay private/local; frontend DTOs use root label/eligibility, not trusted path input.

| Record               | Exact fields and meaning                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| StoreV1              | `schema:1`, `storeId:UUID`, `revision:Counter`, `hostId:UUID`, `projects:Project[]`, `bindings:Binding[]`, `workspaces:Workspace[]`, `sessions:Association[]`, `operations:Operation[]`, `closedOperations:UUID[]`, `migration:Migration \| null`                                                                                                                                                                                                                                                                      |
| Project              | `id`, `label:Text`, `createdAt:Time`, `updatedAt:Time`, `revision`, `archived:bool`, `tombstone:bool`, `defaultBindingId:UUID \| null`, `defaultWorkspaceId:UUID \| null`, `profileDefaults:{claude:ProfileRef \| null,codex:ProfileRef \| null}`                                                                                                                                                                                                                                                                      |
| ProfileRef           | `id:NativeId`, `version:Counter` (1..u32::MAX); existing owner resolves availability, no profile body/credential copy                                                                                                                                                                                                                                                                                                                                                                                                  |
| ManagedSafetyProfile | Closed, provider-qualified requested profile: `inference` (text-only) or `local_tools` (mediated effects); neither grants execution or approval privileges.                                                                                                                                                                                                                                                                                                                                                            |
| Binding              | `id`, `projectId`, `hostId`, `revision`, `path:AbsolutePath`, `observation:{device:Counter,inode:Counter}`, `checkoutKind:folder \| git \| worktree`, `state:valid \| missing \| identity_changed \| unverified \| retired`, `previousBindingId:UUID \| null`                                                                                                                                                                                                                                                          |
| Workspace            | `id`, `projectId:UUID \| null`, `hostId`, `revision`, `label`, `layout:Node \| null`, `focus:UUID \| null`, `pinned:bool`, `leaves:Leaf[]`, `preferences:{provider:claude \| codex \| null,mode:native \| managed,nativeProfile:ProfileRef \| null,managedSafetyProfile:ManagedSafetyProfile \| null}`                                                                                                                                                                                                                 |
| Node                 | `{kind:leaf,id:UUID}` or `{kind:split,dir:row \| col,ratio:number,a:Node,b:Node}`; leaf ID names a workspace Leaf                                                                                                                                                                                                                                                                                                                                                                                                      |
| Leaf                 | `id`, `title:Text`, `associationId:UUID \| null`; null means an empty placement, never permission to launch                                                                                                                                                                                                                                                                                                                                                                                                            |
| Association          | `id`, `originProjectId:UUID \| null`, `hostId`, `bindingId:UUID \| null`, `bindingRevision:Counter \| null`, `target:NativeTarget \| ShellTarget \| ManagedTarget`, `state:attached \| detached \| unavailable \| ambiguous`, `operationId:UUID \| null`                                                                                                                                                                                                                                                               |
| NativeTarget         | `kind:native`, `sessionId:NativeId`, `terminalId:UUID`, `hostEpoch:Text`                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ShellTarget          | `kind:shell`, `terminalId:UUID`, `hostEpoch:Text`                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ManagedTarget        | `kind:managed`, `protocol:1 \| 2`, `storeStreamId:UUID`, `sessionId:UUID`, `requestedSafetyProfile:ManagedSafetyProfile \| null`; a display-only request, never proof of effective authority profile                                                                                                                                                                                                                                                                                                                   |
| Operation            | `id`, `kind:launch`, `projectId:UUID \| null`, `workspaceId`, `leafId`, `bindingId:UUID \| null`, `bindingRevision:Counter \| null`, `provider:claude \| codex \| shell`, `mode:native \| managed`, `nativeProfile:ProfileRef \| null`, `managedSafetyProfile:ManagedSafetyProfile \| null`, `state:pending \| committed \| ambiguous \| refused`, `associationId:UUID \| null`, `claim:Text \| null`, `createdAt`; a durable requested-profile receipt, never an approval, credential or executed-profile attestation |
| Migration            | `version:1`, `id:UUID`, `sources:{kind:registry \| layout,digest:SHA256}[]`, `map:{sourceKind:registry_project \| registry_session \| layout_leaf,sourceKey:Text,targetId:UUID}[]`, `state:committed`; sourceKey is an opaque source-record digest, not a public path                                                                                                                                                                                                                                                  |

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
byte-for-byte; an incompatible ID is an unresolved import, never coerced. `hostEpoch` and `claim` are epoch text, not Text. Epoch text is an
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

Store invariants, validated on decode and on encode: IDs unique per kind and on this store's
host; every reference exists; a live project's default workspace and binding are its own,
a tombstone has neither and owns no workspace or binding; exactly one Scratch workspace;
each workspace's leaves are exactly the leaves its layout places, with focus and associations
valid and no association twice in one workspace; one association per owner target; a bound
association belongs to its binding's project; bindings chain inside one project without a
cycle; `pending` has a `claim` and no association, only `committed` has an association; at
most one unfinished receipt per leaf; the receipt and association that cite each other agree;
a compacted ID is never live. Revisions at the end of the u64 range are refused.
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

| Kind                                     | Payload / rule                                                                                                                                                                                                                                                                                                                                                                            |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `registerExisting`                       | `selectionToken`; backend-owned one-use native chooser token, descriptor-revalidated root; duplicate canonical root on host returns existing project                                                                                                                                                                                                                                      |
| `createFolder`                           | `selectionToken,name`; native-confirmed parent + one basename (no separators/dot segments); mkdir exclusively, no Git/launch; failed store commit leaves visible unregistered empty folder with explicit recovery, never deletes a now-nonempty folder                                                                                                                                    |
| `relink`                                 | `projectId,oldBindingId,selectionToken`; display old/new comparison and per-operation eligibility before commit                                                                                                                                                                                                                                                                           |
| `renameProject` / `archiveProject`       | `projectId,label` / `projectId,archived`; archive hides recent item, preserves attention/live work                                                                                                                                                                                                                                                                                        |
| `removeProject`                          | `projectId,retainHistory:bool`; refuse active/ambiguous/unavailable-not-proven-ended targets; retainHistory keeps a tombstone Project (no active workspace/binding); otherwise remove proven-ended associations/workspaces/bindings. No filesystem/provider/vault deletion                                                                                                                |
| `createWorkspace` / `duplicateWorkspace` | `projectId,label` / `sourceWorkspaceId,label`; duplicate uses new leaf IDs, all associations null; copy preferences only                                                                                                                                                                                                                                                                  |
| `saveWorkspace`                          | `workspaceId,expectedWorkspaceRevision,label,layout,focus,pinned,leaves,preferences`; same-store CAS, all association origins must match project except explicit Scratch adoption                                                                                                                                                                                                         |
| `closeWorkspace` / `deleteWorkspace`     | `workspaceId`; close releases views/input only; delete detaches associations, replaces deleted default with first remaining workspace or a new empty default                                                                                                                                                                                                                              |
| `attach` / `detach`                      | `workspaceId,leafId,associationId`; verified owner lookup, never launch; detach does not close PTY/runtime                                                                                                                                                                                                                                                                                |
| `adoptScratch`                           | `associationId,projectId,bindingId`; only origin-null ShellTarget, confirmed actual cwd disclosure; assignment of origin once, no cd/mode/permission change                                                                                                                                                                                                                               |
| `launch`                                 | `workspaceId,leafId,provider:claude \| codex \| shell,mode:native \| managed,nativeProfile:ProfileRef \| null,managedSafetyProfile:ManagedSafetyProfile \| null`; Native/shell: `managedSafetyProfile` null; Managed: `nativeProfile` null and `managedSafetyProfile` an explicit per-launch choice; fresh binding + eligibility + owner availability, explicit user action, no free argv |

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
not isolation against legacy/direct same-UID writers; compatibility warning is visible.

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
needs a new `operationId`. `context_stale` is evaluated against the immutable captured
context before any dispatch, never against current UI selection.

Errors: `unsupported_schema`, `invalid_record`, `capacity_exceeded`, `revision_conflict`,
`context_stale`, `leaf_busy`, `root_missing`, `root_identity_changed`, `root_ineligible`, `host_offline`,
`host_changed`, `unsupported_capability`, `profile_unavailable`, `owner_refused`,
`effect_ambiguous`, `io_failure`. Failure response includes original operation/context,
known effect `none|committed|unknown`, and safe next action; never raw provider/config bytes.

## 4. Migration, preservation and rollback

1. **Read-only preview.** Use bounded strict legacy registry schema1 (2 MiB,200 sessions,
   100 known folders) and saved layout v1 (64 KiB,16 tabs,16 panes/tab,depth6). Validate
   owner/mode/descriptor and consistent bytes/digest before/after read. If sources change,
   retry preview, never stop their writers. No Registry::open or HostLink lazy startup.
2. Generate one migration UUID/storeId/HostId in preview memory; deterministic UUIDv5
   mapping within that migration namespace from source kind + stable legacy key. Retry of
   that preview yields the same map. Canonical exact root duplicates coalesce after native
   validation; aliases/ambiguous roots require user resolution. Missing roots remain
   unverified/read-only. Do not infer projects from cwd prefixes, Git URL or directory names.
3. Read current host through non-starting checked connection, validate every terminal/native
   tag against its live owner. Map known exact-root sessions; unmatched shells/native work
   stays in Scratch/unassigned recovery view (Native is not adoptable by adoptScratch).
   Import each legacy tab into a separate named workspace (up to16), splitting mixed-project
   tabs by verified origin, and set the first as default. Preview must refuse any resulting
   workspace/leaf count above its bound; never truncate. Prune absent refs into empty leaves,
   never remake shells. Missing agents/history remain discoverable metadata. Never infer
   an ended process merely from a missing layout/registry row.
4. P1/P2 session lists remain separate projections with source store ID; only exact owner
   workspace binding plus explicit project match can associate Managed work. Unknown/root-
   ambiguous work stays unassigned and cannot receive project-scoped commands. No SQLite
   read/write migration and no provider thread resume on import.
5. On explicit commit, recheck preview digests/owner epochs and all bounds. Preserve legacy
   registry/layout bytes in a private migration snapshot (no profile/auth/vault copy), hash
   manifest; stage new validated store and fsync, atomic publish last. Never overwrite the
   source. Crash before publication => old reader; after => fully valid new reader/map.
   A private snapshot contains user paths: never log, commit or attach it to a PR.
6. New UI exclusively owns its new store; old windows continue owning old layout. Detect
   legacy changes and offer incremental import with a **new** preview; never merge them
   silently or overwrite newer workspace state. UI feature flag selects reader, not host
   lifecycle. Reopening old UI can still recreate missing shells by old behaviour: disclose
   this before rollback, prefer attach-only compatibility view if that risk is unacceptable.
7. Rollback is an owner-reviewed GUI/metadata operation: record post-snapshot new records,
   export mapping privately, confirm no unaccounted/ambiguous operation, disable new reader.
   Keep v1 store/tombstones/snapshot for forward recovery; never restore legacy bytes over
   live writers. Installer rollback can close terminals and is **not** this UI rollback.
   No host stop, SQL downgrade, authority switch, provider-setting restoration or PID cleanup.

M0 only prepares this procedure. It does not create snapshots of personal records, publish
stores, install a GUI, or execute rollback. Observe before/after current service start
identities/socket inodes and legacy metadata; differences require investigation, not
cleanup. Absence of a difference is preservation evidence, not proof of provider fidelity.

## 5. Bounded implementation order and executable acceptance handoff

Build in separate reviewable increments: pure closed codec/schema + fixtures; locked
store/CAS + failure injection; non-starting owner projection/checked transport; migration
preview/commit; project/Scratch navigation; layouts/input coordination; launch/config
context integration. ADR0052 accepts the identity/projection architecture; M1 design details and
fixtures remain unimplemented. Release/adoption still requires all applicable
M1-01..13 acceptance evidence, performance budgets, review and a separately authorized
preservation-aware deployment, not merely this ADR's approval. No dependency on Claude Managed,
new remote transport, concurrency increase, parser helpers or OL3 implementation.

| Required test ID | Exact oracle / acceptance                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | FP/AC                |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| M1-01            | Valid empty Scratch/project/Native/P1/P2 records round-trip; unknown/duplicate keys, schema2, dangling IDs, invalid/mixed Native-vs-Managed profile choices, forbidden secret/prompt/argv fields, u64 overflow refuse before save; byte/count/depth at bound and +1                                                                                                                                                                                                                                                     | CTX01/03             |
| M1-02            | Native chooser cancel => zero mkdir/store writes; duplicate root returns existing; symlink/foreign ACL/replaced descriptor refuses; HOME Native allowed only by its owner, config HOME denied                                                                                                                                                                                                                                                                                                                           | CTX02                |
| M1-03            | Missing/replaced/relinked root => metadata visible, new effect denied, existing target unchanged; old binding retained for live association                                                                                                                                                                                                                                                                                                                                                                             | CTX02                |
| M1-04            | Two windows CAS same revision => exactly one commit, other conflict; lock/crash/write/fsync/rename faults leave complete old or new bytes, never empty fallback                                                                                                                                                                                                                                                                                                                                                         | CTX01/04             |
| M1-05            | Switch A→B during chooser, config plan, confirmation and delayed receipt => no B mutation; pending plan expires, confirmed effect/receipt remains A; cross-project Undo refuses                                                                                                                                                                                                                                                                                                                                         | CTX04                |
| M1-06            | Duplicate/close/delete/archive/navigation/reopen => zero spawn/stop/resume/lease calls; default removal leaves empty replacement; detached sessions discoverable; Scratch no providers/authority                                                                                                                                                                                                                                                                                                                        | CTX01/03             |
| M1-07            | Registry/layout corpus: duplicate roots, absent host, missing agents, malformed/newer files,16 tabs, changed sources, interrupted publication and second import; deterministic map; no Registry::open, no shell remake, no legacy writes                                                                                                                                                                                                                                                                                | CTX01..03            |
| M1-08            | P1/P2 same-looking IDs never conflate; wrong store/epoch/generation/context or requested-vs-effective Managed safety profile denies attach/input/effect; project association never changes Managed binding or grant                                                                                                                                                                                                                                                                                                     | CTX01/04             |
| M1-09            | Two mirror views => one input/resize claim, other read-only; owner crash frees claim; replaced socket/PID reuse/unreadable start ticks => refusal before send, zero reconnect replay                                                                                                                                                                                                                                                                                                                                    | CTX01/03             |
| M1-10            | Native dispatch succeeds then association save/ACK lost => ambiguous, zero second launch; same `operationId` twice (identical => stored state, different => `invalid_record`) or from two windows => one dispatch; a different `operationId` on a busy leaf => `leaf_busy`; `pending` with a dead claim found at load => `ambiguous`, with a live claim untouched; compacted ID => refused; Managed receipt retains the explicit requested safety profile and retry uses original intent; startup never replays journal | CTX01/04             |
| M1-11            | Purge active/unknown/ambiguous work refuses; archive preserves execution; retained history tombstone survives; zero project-file/provider-history/vault deletion                                                                                                                                                                                                                                                                                                                                                        | CTX02                |
| M1-12            | Old host missing feature remains browsable; incompatible command disabled; recovery uses checked attach-only path, host start time/socket/store IDs unchanged                                                                                                                                                                                                                                                                                                                                                           | CTX01/03             |
| M1-13            | Owned synthetic 1/16 terminals and 2/32 workspaces: switch/idle/restore trace, listener count and bounded buffers; three runs of matched before/after procedure below, raw outliers retained                                                                                                                                                                                                                                                                                                                            | CTX01..04, FP-NFR-01 |

Fixtures must use synthetic metadata and fail-on-call spawn/stop/provider/config writers.
No test requires real credentials, a live service restart or installed-store migration.
Current fixture passes in the status register prove the old components only; the above
new tests are **NOT_RUN** until implemented. Required native desktop journey after owner
acceptance: A/B/Scratch, Native Claude/Codex and existing F8 visible in correct scope,
archive/reopen, GUI-only recovery, two windows, loss of owner connection. Zero duplicate
processes/prompts and unchanged live ownership are explicit release oracles.

Efficiency: one shared owner inventory per window/backend with bounded subscriptions,
no polling loop per inactive workspace, virtualized project/session lists, lazy renderer
attachment and latest-revision debounced metadata saves (400ms starting point from current
layout). Do not duplicate PTY readers/output history for inactive tabs. Measure whole
GUI/WebKit/host/authority owned trees separately, input-to-render, project switch p50/p95/
p99, restore time, RSS/PSS, CPU and wakeups. Follow architecture15's three independent runs,
warmup/sample rules and machine/load/build disclosure. No numeric latency budget is proved
by M0. Before M1 release, agree measured reference budgets; refuse an unexplained regression
outside baseline uncertainty and retain failed/outlier samples. A fixture-only latency pass
cannot close the real-desktop performance requirement.
