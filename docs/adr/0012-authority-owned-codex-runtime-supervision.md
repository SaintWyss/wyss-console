# ADR 0012 — Authority-owned Codex runtime supervision

Status: Accepted
Date: 2026-10-07

## Decision

`wyss-control` owns at most one shared, long-lived Codex App Server. The authority
core commits canonical control state and is the only SQLite writer; an owned worker
handles the process and protocol. Connection workers only submit typed intents.
Tauri neither starts nor connects to this runtime. Human PTYs remain Console-local.

An App Server can host several future threads. It is not allocated per
ManagedSession. Sprint 2E implements only AgentRuntimeInstanceId → process. It
creates no Codex threads, binds no sessions and changes no prepared/closed session
record. ManagedSessionId, AgentRuntimeInstanceId, CodexThreadId, PID and authority
instance ID remain separate identity domains. A fresh launch attempt reserves a
fresh existing AgentRuntimeInstanceId, published only after the owned child has
spawned. A failed/restarted process never reuses an old instance ID. PID is an OS
observation, absent from canonical runtime rows and runtime audit.

Process readiness is not permission to perform managed work. No thread/start,
thread/resume, thread/read, turn/start, turn/interrupt, review/start, prompt, tool,
approval response or permission response is available through supervision.
Execution gates, leases and revocation must precede any such work in a later phase.

## Durable desired state and observed actual state

Desired `running`/`stopped` is canonical and durable. Actual `stopped`, `starting`,
`initializing`, `ready`, `stopping` or `failed` is an in-memory observation.
`running/ready` and `running/failed` are both meaningful. Child failure never changes
desired state. No runtime row means desired stopped, revision zero, actual stopped.
Migration and first startup cannot implicitly launch Codex.

Protocol v1 gains four deliberately typed methods behind the existing hello and
same-UID SO_PEERCRED boundary:

- `runtime/start`: intent_id, adapter_id, runtime_kind, optional expected_revision.
  Every new accepted intent commits desired running and the next runtime revision.
- `runtime/stop`: the same mutation fields; commits desired stopped and next revision.
- `runtime/status`: adapter_id/runtime_kind; returns desired/revision, actual state,
  live instance ID, numeric observed version, compatibility, bounded model count,
  fixed diagnostic and sanitized successful outbound method counts for the latest
  attempt (retained after exit, reset on the next attempt).
- `runtime/model/list`: adapter_id/runtime_kind, nullable cursor, limit 1–20; returns
  normalized AgentModelDescriptorV1 values and a nullable continuation cursor.

Only codex/codex is supported. Other pairs return unsupported without a control
mutation or spawn. Unknown fields (including conversational input), methods and
versions reject. There is no generic process, executable, argv, environment, PID,
shutdown, prompt or approval operation. Same UID is not future execution authority.

Start/stop use the existing IMMEDIATE transaction: canonical intent verification,
desired mutation, independent runtime revision, audit append, original result
receipt, commit. Only after commit/receipt delivery is attempted does the core
reconcile. The reply describes the committed desired state, not successful external
initialization. Optional expected_revision checks zero for no row or the current
revision. New intents advance even when desired is unchanged. A new start while
failed permits exactly one recovery attempt; a new start while live retains that
child. No automatic restart loop/backoff exists in the same authority lifetime.

Intent retries normalize UUID case and return the original committed projection,
without another event, revision or recovery attempt. Changed payload/method with
the same intent_id returns intent_conflict. A retry of an old start after a later
stop returns the old receipt without restarting anything. Clients read status for
current truth. Runtime revision is unrelated to ManagedSession revision.

## Schema v2 and audit

The explicit v1 → v2 SQLite transaction rebuilds authority_metadata to constrain
schema_version=2 and control_events to permit null session_id, and creates only
managed_runtimes. Existing envelopes and receipts are copied unchanged, with the
same event IDs, sequence keys, stream ID and sequence head. Session tables and
records remain unchanged. Append-only triggers are recreated in the transaction.
Fresh empty databases create v2 directly. The runtime table has adapter_id,
runtime_kind, desired_state, positive revision and authority-created/updated UTC
timestamps; it starts empty. Exact schema, integrity, foreign keys, stored records,
receipts and audit validation still reject unknown/future/corrupt state. Failed
migration rolls back DDL, data and user_version. There is no production downgrade
or repair operation; restore a prior application/database snapshot for rollback.
Tests exercise actual seeded v1 SQLite, reopening v2 and injected migration failure.

The existing EventEnvelope v1 is unchanged. New types are:

- authority.runtime.desired_state_changed: adapter_id, runtime_kind, desired_state,
  revision; correlation_id=intent_id.
- authority.runtime.instance_ready: adapter_id, runtime_kind, runtime_instance_id,
  observed numeric runtime_version and compatibility; correlation_id=null.
- authority.runtime.instance_exited: adapter_id, runtime_kind, runtime_instance_id,
  fixed exit_kind, expected boolean and desired_state at observation; correlation_id=null.

All use the existing durable/internal/none policy, session_id=null, authority writer
identity, UTC timestamps and one monotonically increasing stream. Exit is recorded
after reaping. No PID, catalog, frames, stderr, provider messages, auth, environment
or prompt enters audit. Protocol notifications remain transient; there is no event
per model/notification/token. A hard-killed writer cannot append an exit observation;
startup does not fabricate one or interpret a historical ready event as live state.

Ready is published only after its required audit transaction commits. If readiness
audit fails, the core cancels/joins/reaps the worker, clears catalog/identity and
reports failed/audit_unavailable. Further audit failure cannot prevent cleanup.
Control mutation audit failure rolls back desired state, revision and receipt.

## Process ownership and containment

Reuse Sprint 2C discovery, OwnedProcess, JSONL reader, codec, correlation and the
same handshake function. Production does not invoke the example probe. Discover
only `codex` from absolute PATH entries, verify a regular executable and canonicalize
symlinks. Run `codex --version` as a bounded separate owned observation, then directly
`codex app-server`, without a shell. Cwd is the repository root compiled from the
crate manifest. Inherit local environment/configuration normally; no client can
supply environment or choose a workspace. Wyss never reads token files, logs in,
modifies Codex config or attempts authentication remediation.

On Linux, before exec, use existing nix 0.28.0
`nix::sys::prctl::set_pdeathsig(SIGKILL)`. Capture getpid in the spawning parent,
then set the death signal and compare getppid against that captured PID in the child.
If the relationship was lost before setup, fail startup with ECHILD. Death after
setup triggers unmaskable SIGKILL. Only syscalls/OS-error construction run in
pre_exec. Clear the authority's inherited signal mask there so normal termination
works. The process-owning worker thread lives until its child has been reaped;
Linux associates the death signal with that spawning thread. See the
[Linux parent-death semantics](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html).

No separate process group is needed for this fixed, direct runtime with no managed
tool execution. Cleanup signals only the unreaped Child it owns: stdin EOF, bounded
grace, SIGTERM and finally SIGKILL/wait. No process enumeration drives signals;
external Codex processes are untouched. Drop is a final cleanup guard, not the
hard-crash mechanism. This containment covers the owned App Server, not future
tool descendants; work execution requires its own gates and containment design.

## Handshake, lifetime and resource bounds

Each new child sends initialize with experimentalApi=false; validate the matching
response, then send initialized, then one model/list with cursor=null,
includeHidden=false, limit=50. Two fresh request IDs and strict type/value matching
reject unrelated/duplicate completions. The runtime IO boundary additionally
allowlists exactly initialize, initialized and model/list, each at most once; it
counts successful writes without retaining their frames. There is no response
encoder in this path. Any server-initiated request, known or unknown, is never
answered and causes termination/reaping and failed actual state.

A successful, normalized catalog is required for ready. An ordinary catalog RPC
error after handshake is failed/catalog_rejected with degraded compatibility,
without inferring authentication/configuration cause or entitlement. This is
stricter than the standalone diagnostic probe, where optional catalog RPC failure
remains a degraded diagnostic result. Malformed catalog/protocol is incompatible;
unavailability before handshake remains unknown. The standalone probe still closes
stdin after its finite handshake/catalog observation and retains its original
read-only behavior and tests.

Catalog holds at most 50 validated descriptors in memory, tied to the live instance.
It is never SQLite state. Provider and availability remain unknown where Codex
provides no normalized evidence; listing grants no entitlement. Control pages are
at most 20 descriptors with existing bounded model/provider/display strings, well
below the unchanged 64 KiB frame cap. Cursor contains runtime instance ID and bounded
index; old-instance cursors reject. Stopped/failed clears catalog and live identity.
Restart obtains a new catalog rather than restoring old observations.

After ready, continuously consume/validate structured stdout. Known notifications
pass existing normalization, unknown legal notifications retain only bounded
private method/shape summaries (64 methods, saturating counts); no raw transcript.
One partial JSONL frame is bounded to 256 KiB plus 4 KiB scratch. Oversized,
malformed/invalid envelope/payload, correlation contradiction or server request
fails closed. Stderr drains in bounded work units with a private 32 KiB tail,
never serialized/persisted. Ready IO sleeps in poll on stdout/stderr/cancellation;
an idle 60-second deadline permits liveness checks, while an incomplete frame at
deadline fails. Cancellation wakes poll immediately. The core waits on its bounded
eight-message channel; no SQLite polling or busy reconciliation loop exists.
Lifecycle backpressure is bounded and interruptible. Clients never perform IO
against the child or trigger another catalog probe by reading projections.

## Shutdown and restart

Client/graphical Console disappearance changes neither desired nor actual state.
Graceful authority SIGTERM/SIGINT stops/reaps its actual child, records the minimal
expected exit and preserves desired running. Startup reloads desired state and
admits one new process/instance/handshake. Unexpected runtime failure marks failed
and awaits a new explicit start intent or a future authority startup.

SIGKILL authority invokes Linux parent-death containment. Durable desired running
survives; a restarted authority launches a fresh instance. Explicit runtime/stop,
by contrast, commits stopped, then reaps; subsequent authority startup launches
nothing. No shutdown path closes or binds ManagedSessions. Fake-child process tests
and isolated real Codex acceptance cover all three cases, with unrelated process
identity snapshots and no raw protocol recording.

## Sprint 2H implementation note

ADR 0015 replaces plain supervised startup with an immutable inference process
profile, a discovery-only MCP preflight and a freshly attested runtime generation.
Runtime readiness remains only one necessary execution-gate condition.

## Later implementation status

ADR 0016 adds a second runtime profile. A generation started as `codex_local_tools_v1`
does not inherit the local Codex environment and configuration described above: it
runs in an authority-built namespace with a private Codex home and policy, and one
validated credential capability (ADR 0016, hardened in ADR 0018). The authority still
reads no token file in either profile: the credential is opened without read access
and bound by descriptor. The schema is at version 7. The working directory of the
runtime process is still the repository root compiled into the binary, so a binary
moved away from its build tree cannot start a runtime; it fails closed. Packaging has
to replace that before distribution (ADR 0018, residual risks).
Since ADR 0021 the desktop can ask for `runtime/start`, with the default profile, as
part of activating a session; it still launches and supervises nothing.
