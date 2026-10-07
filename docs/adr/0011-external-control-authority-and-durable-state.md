# ADR 0011 — External control authority and durable state

Status: Accepted
Date: 2026-10-06

## Preimplementation decision

`wyss-control` is an independently started Linux binary in the existing package.
Tauri does not start it, call it or own its lifecycle. Only this authority allocates
canonical managed-session IDs and commits prepared/closed control state. These
states are separate from runtime-observed lifecycle. Prepared grants no execution.

Use private XDG runtime/state product directories, a Unix socket with same-UID
SO_PEERCRED validation, and strict length-prefixed JSON contracts. Eight clients,
eight queued core requests, one SQLite writer, no async framework. Connection
presence/disconnect never changes canonical state. A bounded Rust client is the
future integration boundary; no frontend command or capability is added.

Use exact rusqlite 0.40.2, default features disabled, bundled SQLite only; nix socket
support and time formatting reuse existing dependencies. No system installation.
SQLite state, append-only EventEnvelope audit and durable intent receipts commit
atomically. Stable stream identity and sequences survive authority restart; process
instance IDs change. Failed audit append rolls back the state mutation. Intent retry
returns the original result; conflicting reuse rejects. No agent/runtime is spawned.

Only hello, status, prepare, get, list, close and audit/read exist. No execution,
prompt, approval, permission, lease or generic process method exists. Future runtime
supervision must belong here, behind deliberately defined authority gates, before
any managed work. Human PTYs remain Console-local. This does not yet authorize a
first prompt.

## Process and peer boundary

The binary accepts no CLI arguments and inherits XDG/HOME location policy only;
there is no configurable executable, database path, argv, PID, signal or environment
in its control API. It never calls the Codex probe or launches a runtime. Console
continues owning human PTYs. This binary is independently invoked, not a Tauri
sidecar or installed service. Cargo default-run remains the desktop binary.

Linux SO_PEERCRED via nix 0.28.0 checks peer UID against authority effective UID
before frames are processed; the client also checks the server UID. Client instance
UUID is one process-local informational identity, reused on reconnect, not identity
proof. Authority instance UUID is fresh on startup and is the audit writer instance.
Same-UID validation is a local access boundary, not protection from a compromised
same-user process. Runtime output/capability claims grant no authority.

Runtime: `$XDG_RUNTIME_DIR/wyss-console/control.sock`. XDG_RUNTIME_DIR must exist,
be absolute, owned by effective UID and mode 0700. No fallback. Product directory
0700 and socket 0600. Reject symlink components, unexpected owners and writable
ancestors; root-owned sticky temporary parents are allowed for isolated tests.
Unix pathname length must fit Linux's sockaddr_un. State: `$XDG_STATE_HOME/wyss-console/control.sqlite3`,
or `$HOME/.local/state/wyss-console/control.sqlite3` if unset. The state root must
be absolute/user-owned and not writable by other users. New product directories
are private; existing unsafe paths are rejected rather than silently chmodded.

Runtime and state each retain an exclusive advisory lock on private `control.lock`
files. This prevents a second writer even with different runtime directories.
Existing socket recovery only removes an owned socket that refuses a nonblocking
connection, after an inode/device check. Active/indeterminate sockets fail closed.
Regular files and symlinks are never unlinked. Graceful cleanup removes only the
socket inode created by this process. Lock files remain inert; no runtime identity
or authorization is inferred from their existence. A hard kill leaves a stale
socket that the next owner safely recovers.

## Protocol v1 and resource bounds

Wire framing is u32 big-endian length plus one UTF-8 JSON object, 1–65,536 bytes.
Length is checked before payload allocation; partial/truncated frames, invalid
UTF-8/JSON and duplicate members at any depth reject. Request has exactly
`schema_version`, UUID `request_id`, `method`, typed `params`. Unknown core/parameter
fields and unknown versions reject. Each connection first performs authority/hello
with protocol version 1 and client instance UUID. Successful hello returns protocol,
service/storage versions, authority instance, durable stream/head and session counts.
It establishes compatibility, not permissions. Repeat hello on a connection rejects.

Response has schema version, exact request ID and authority instance, plus exactly
one typed result or `{code}` error. No raw Rust/SQLite diagnostic chains. Client
checks request correlation, instance continuity and expected result variant.
The exact method allowlist is authority/hello, authority/status,
managedSession/prepare, managedSession/get, managedSession/list,
managedSession/close and audit/read. There is no shutdown or execution operation.

At most eight connection workers and eight queued core requests; one request per
connection is outstanding. Excess clients are closed; full core queue returns
capacity_exceeded. Core alone owns SQLite. It never holds storage while writing
client sockets. Idle listener waits on poll, core waits on recv; no database polling.
Frame IO is nonblocking with absolute deadlines: hello 3 s, subsequent reads 5 s,
response writes 3 s; core reply wait 3 s and SQLite busy timeout 1 s. IO cancellation
checks at most every 50 ms using sleeping poll, not a spin loop. A timeout may make
a committed result ambiguous; the durable intent receipt resolves retry safely.
Clients reconnect after idle timeout. List/audit pages accept limits 1–50 (below
100 and sized for the 64 KiB frame), ordered by creation/audit sequence. Nullable
cursors mean start; a full page may return its last sequence as a continuation.
No filtering or unlimited transcript cache exists.

SIGINT/SIGTERM are blocked before worker creation and consumed using nix signalfd.
Shutdown stops accepting, cancels bounded socket work, joins workers, drains/aborts
queued core requests and closes SQLite before releasing locks/socket. No protocol
client can stop the authority. Unexpected hard kill relies on committed SQLite WAL
recovery; active runtime recovery is deferred because no runtime exists.

## Canonical storage and transitions

Schema v1 has only authority_metadata, managed_sessions, control_events and
processed_intents. Explicit migration accepts an empty v0 database, opens verified
v1, and rejects higher/unknown/corrupt or altered schemas. It checks integrity,
foreign keys, exact schema objects and stored control contracts; no deletion,
automatic repair or empty in-memory fallback occurs. Initial migration downgrade/
upgrade is exercised only on isolated test databases, including seeded databases;
there is no production downgrade API.

rusqlite 0.40.2 uses default-features=false, bundled only (libsqlite3-sys 0.38.2).
The [published manifest](https://github.com/rusqlite/rusqlite/blob/v0.40.2/Cargo.toml)
does not declare an MSRV; maintainer policy is release-time stable. A local isolated
Rust 1.90.0 dependency build and execution verified this exact bundled integration.
No global toolchain or system package was changed. Existing nix gains socket and
time gains formatting. No ORM, async runtime, HTTP or SDK is added.

SQLite: foreign_keys ON, WAL, synchronous FULL, busy timeout 1 s. Database/lock files
are created 0600 with no-follow checks; existing files must be owned, private,
regular and single-linked. Existing journal/WAL/SHM paths are validated. SQLite
sidecars inherit the private database mode inside the 0700 state directory, verified
by tests. The single writer uses IMMEDIATE transactions. Closed records remain
queryable; durable retention/compaction is deferred, not an in-memory list.

Authority allocates a fresh ManagedSessionId on prepare and records only adapter ID,
runtime kind, prepared state, revision 1, authority UTC timestamps and creation order.
No runtime ID, process, thread, model or grants exist. Intentional close changes
prepared to closed, revision 2. expected_revision rejects stale requests. Closing
an already closed record at its current revision is a no-op with a durable receipt.
Retries do not advance revision. Runtime-observed ManagedAgentSessionState remains
an independent contract. ConsoleSessionId and TerminalId cannot substitute in typed
client methods; wire UUID text alone cannot prove an identity domain.

Mutating intent_id is independent of request_id. Receipts store a canonical typed
method/parameter representation with normalized UUID case and the original session
result; no hashing dependency. Same intent and payload returns the original committed
result even after disconnect/restart; changed method/payload returns intent_conflict.
The original projection can be older than current state: clients read get/status
and compare authority revision rather than treating a retry as a new transition.

## EventEnvelope audit and fail-closed commit

One persisted stream UUID survives authority restart. Full u64 sequence semantics
use decimal text internally (fixed-width 20 digits for SQLite ordering), canonical
positive decimal strings on EventEnvelope wire, and a null head before any event.
Checked exhaustion fails closed. Sequence allocation/advance is inside the transaction;
rollback leaves no committed gap. This is not a global ordering across other streams.

Only authority.managed_session.prepared and authority.managed_session.closed are
produced. EventEnvelope v1 is unchanged: source.component=wyss_control,
source.instance_id=current process UUID, session_id=authority ManagedSessionId,
correlation_id=intent_id, causation_id=null, authority-recorded UTC timestamp,
durable/internal/none and payload version 1. Payload is an exact allowlist of
adapter_id, runtime_kind, control_state and revision: explicitly minimized metadata.
No prompt, output, terminal bytes, provider frame, stderr, auth or environment content.
Labels are not a secret detector. No subscriptions, Event Bus or generic event API.

A single transaction performs intent check, state mutation, checked sequence append,
validated audit insertion and durable receipt. Audit/receipt/storage failure rolls
back everything. Audit has unique event ID and stream+sequence, and triggers reject
UPDATE/DELETE. Normal API exposes no historical mutation. Read returns validated
EventEnvelope values ordered by this stream and sequence.

Disconnect, EOF, client crash or last-client disappearance causes zero canonical
transition and no durable connection event. Reconnection reads authority state.
Process tests prove unchanged state/revision/head, ambiguous-response retry, distinct
concurrent allocations, graceful and hard-kill restart with stable stream/session,
new process instance and continued sequence. No control state grants execution.

The separate isolated live acceptance also exercised the actual binary and every
developer-client operation: client disappearance preserved prepared revision 1 and
audit sequence 1; retry reused the same session; intentional close produced revision
2/sequence 2; a new authority instance recovered the same stream and closed record,
then allocated sequence 3. Both processes were reaped and sockets removed. Repository
contents and existing Codex processes were unchanged; no managed runtime was launched.

## Deferred boundary

Future Console intents must be evaluated here before an authority-owned adapter/
runtime is started; Console must not launch Codex then ask for authority afterward.
Runtime supervision, action gates, permission/lease semantics and approval mediation
remain required before managed work. No first prompt is authorized by Sprint 2D.
Installation/autostart, remote transport, policies, retention and runtime recovery
are deferred. Current human terminal UX and the separate read-only Codex developer
probe remain unchanged.

## Implementation status

Sprint 2E adds authority-owned runtime process supervision and schema v2 as specified
in ADR 0012. The authority/client boundary and durable session ownership remain in
force. Runtime readiness still grants no managed-work permission.
