# Wyss Console

Desktop client for local terminals and, progressively, managed-agent control.
Linux-first, local-first. Current validated status: **Sprint 2I authority-mediated commands and file changes** using Tauri 2,
Rust, Svelte, TypeScript and Vite. Real local human shells run in bounded PTYs
and render through xterm.js, with native shell input, ANSI colors and resizing. EventEnvelope contracts remain independent
of transient terminal bytes. Architecture decisions are in [docs/adr](docs/adr).

## Development

Requires stable Rust, Node 26+, npm 12+ and the
[Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).

```sh
npm ci
npm run desktop:dev
```

`npm run dev` runs the frontend alone; application IPC requires the desktop host.
Development and native-run commands keep Cargo/XDG caches inside ignored `.cache/`.

## Validation

```sh
npm run check
npm run check:rust
npm run test:pty
```

Frontend commands: `format:check`, `lint`, `typecheck`, `test`, `build`.
Rust commands: `rust:fmt`, `rust:check`, `rust:test`, `rust:clippy`.
`npm run format` applies frontend/config/document formatting.

## Production build

```sh
npm run desktop:build
npm run desktop:run
```

Produces `src-tauri/target/release/wyss-console`. Distribution packaging is deferred.

## Interactive terminal

The application opens one local account-shell terminal automatically. **New Terminal**
creates an independent session/tab, up to 16 retained sessions (the backend also
limits PTY resources to 16). Each tab keeps its shell, screen and scrollback when hidden;
background output continues to be consumed. Type normally; keyboard input is transported
to that session's shell. The active terminal fits the window; hidden terminals retain
previous dimensions until activated.

`exit` retains the exited tab and final screen without respawning. A tab's close control
releases only its local resource; closing the final tab leaves an empty workspace.
New Terminal starts again explicitly. These are local human-terminal sessions, not
managed-agent sessions. Tabs, identities and scrollback are not persisted; restart
creates fresh shells. Tab arrows/Home/End move tab focus; Enter/Space activate.
No global terminal shortcuts are intercepted.

TERM is scoped to each child as `xterm-256color`. Scrollback is 5,000 volatile lines.
Output uses bounded byte batches acknowledged after xterm processing. Input is capped
at 64 KiB queued and 4 KiB per IPC write; an excessive paste fails visibly instead of
buffering indefinitely. Selection/paste use default WebView behavior, without added
clipboard permissions. External terminal hyperlinks are disabled.

Human terminal survival across Console process crash is not guaranteed at this stage.
Graceful application exit closes its local terminals. ADRs 0005–0007 record backend
ownership, renderer flow and local session ownership. No agents, Event Bus or persistence exist.

## Agent adapter contracts

Validated Rust/TypeScript v1 contracts describe agent adapters, capabilities,
version/compatibility evidence, observed managed-session state, models and safe
errors. Shared synthetic JSON fixtures prove classification and semantic round-trip
parity. They do not claim installed-runtime compatibility. See ADR 0008 and
[official runtime-surface research](docs/research/agent-runtime-surfaces-2026-10-06.md).
No agent SDK, authority or agent UI is connected. Existing local
terminal behavior is unchanged; managed-agent execution requires external authority.

## Codex protocol boundary

An offline Rust codec validates and normalizes a bounded subset of Codex App Server
messages using schema-derived synthetic fixtures. It includes initialization ordering,
thread/turn/item lifecycle, approvals and model catalogs. Shared common projections
are checked in Rust and TypeScript. Codex is not usable through the Console UI. See ADR 0009 for codec limits.

## Read-only Codex compatibility probe

The explicit developer harness discovers Codex, reads its version, starts a private
app-server, initializes with experimental API disabled, requests one model catalog
page and reaps its child. **This read-only probe executes no managed work.** No threads,
turns, prompts, approval responses, login or configuration changes are sent.

```sh
node scripts/tool.mjs cargo build --locked --manifest-path src-tauri/Cargo.toml --example codex_live_probe
./src-tauri/target/debug/examples/codex_live_probe
```

Build and execution are separate so the probe inherits your normal environment,
without the Cargo helper's project-local XDG overrides. It prints a safe JSON summary;
no raw stderr/protocol traffic is printed. Missing Codex is a reported unavailable
outcome (exit 0); an installed failed/incompatible probe exits 1. Normal tests do not
require or launch Codex. Linux process tests require standard coreutils.

Current local observation (2026-10-06): Codex 0.159.0 completed the read-only probe,
including legal interleaved notification metadata and a seven-model catalog page
with no next cursor. Compatibility is confirmed only for the exercised protocol
surface; listing is not entitlement. The owned app-server exited cleanly and was
reaped. See ADRs 0009–0010 and the research note. Managed execution still requires
external authority.

## External control authority

`wyss-control` independently owns prepared/closed managed-session identities,
canonical control state, durable intent receipts and a minimal EventEnvelope audit.
**Prepared is not execution permission.** Explicit, leased managed turns are
available through the authority developer client, with no product chat UI:
inference-only by default, and with authority-mediated commands and file changes
in the isolated local-tools profile.
Human terminal tabs remain Console-local; the read-only Codex probe above remains
a separate developer tool. The desktop does not start or connect to the authority.

Build the independent binary and developer client:

```sh
node scripts/tool.mjs cargo build --locked --manifest-path src-tauri/Cargo.toml --bin wyss-control --example control_client
```

Start independently, with a valid private XDG runtime directory:

```sh
./src-tauri/target/debug/wyss-control
```

From another terminal:

```sh
./src-tauri/target/debug/examples/control_client status
./src-tauri/target/debug/examples/control_client prepare
./src-tauri/target/debug/examples/control_client list
./src-tauri/target/debug/examples/control_client audit
```

The client also supports `hello`, `get UUID`, `close UUID revision`, and
`prepare intent-UUID` for durable retry. Close commits session/lease control state and cancels any active managed turn.
Prepared sessions create no Codex thread/turn. Stop authority using Ctrl+C or SIGTERM;
a normal control client cannot shut it down.

Socket: `$XDG_RUNTIME_DIR/wyss-console/control.sock` (0700 directory, 0600 socket).
Missing/unsafe runtime directories fail closed. State: `$XDG_STATE_HOME/wyss-console/control.sqlite3`,
fallback `$HOME/.local/state/wyss-console/control.sqlite3`. Product state is private.
SQLite WAL/FULL commits state, audit and intent receipts atomically. Do not delete
WAL/SHM files or change/copy an active canonical database casually. Client EOF
changes no state. Authority restart preserves committed state/stream/sequence.
Same-UID SO_PEERCRED is a local boundary, not protection from compromised same-user
processes. Maximum eight connected clients; idle connections expire after five
seconds. Session/audit pages are bounded to fifty entries. See ADR 0011.

`npm run rust:test` includes hermetic authority unit tests and actual process tests
using isolated temporary XDG directories. They do not require or launch Codex.
No production state is created by the test suite.

## Authority-owned Codex runtime

`wyss-control` can supervise one shared, long-lived `codex app-server`, independently
of Console clients. Durable desired running/stopped is separate from observed
starting/initializing/ready/stopping/failed. Schema v1–v5 upgrades transactionally to
v6, preserving sessions, leases, bindings, operation journal, turns, receipts, stream,
audit and runtime desired state. Migration creates no runtime desired state,
managed turns or managed approvals; startup reconciles an existing desired-running record. Durable
conversation bindings use CodexThreadId; runtime attachment remains transient.

The developer client exposes the new normalized control methods:

```sh
./src-tauri/target/debug/examples/control_client runtime/status
./src-tauri/target/debug/examples/control_client runtime/start
./src-tauri/target/debug/examples/control_client runtime/model/list
./src-tauri/target/debug/examples/control_client runtime/stop
```

Start/stop also accept an intent UUID for durable retries. Each new intent commits
its desired state/revision/audit before reconciliation; retry returns the original
receipt. Status observes readiness asynchronously. The Rust client supports catalog
pages of at most twenty descriptors with instance-scoped cursors. The catalog is
bounded to one fifty-model page, transient and refreshed for each new process;
listing is not provider entitlement. A rejected catalog fails safely without auth
remediation. No raw Codex frames/stderr/auth/environment are exposed or persisted.

The shared 2C handshake sends only initialize, initialized and model/list. Stdout
continues to be validated/drained after readiness and stderr remains bounded.
The inference profile answers no server request; protocol failure reaps the child. Linux
parent-death SIGKILL plus parent-identity checking prevents an orphan App Server
after authority crash. Graceful authority shutdown reaps the child but preserves
desired running; restart obtains a fresh AgentRuntimeInstanceId and catalog.
Explicit runtime/stop persists stopped and suppresses startup reconciliation.
Unexpected runtime failure does not create an automatic restart loop; a new start
intent admits one recovery attempt.

**Runtime process readiness is not execution authorization.** Runtime supervision
creates no Codex threads or turns, submits no prompts, and answers no approvals or
permission requests.
Tauri has no runtime command/capability or authority connection. Human terminal
behavior and the standalone read-only probe remain unchanged. See ADR 0012.

Rust tests include seeded v1 migration, idempotency/audit rollback, deterministic
fake App Server IO/failure/allowlist checks, graceful restart, explicit stop and
Linux SIGKILL containment. Normal tests use isolated XDG directories and a fake
executable, never the user's Codex runtime or production authority state.

### Isolated local-tools runtime profile

A runtime generation has exactly one safety profile for its whole life. The default
`codex_inference_v1` offers the model no tool. `codex_local_tools_v1` is requested
explicitly and names the one workspace it serves:

```sh
./src-tauri/target/debug/examples/control_client runtime/start/local-tools /abs/workspace
```

Changing profile or workspace needs `runtime/stop` first; a start that would change
either on a live generation is refused before anything durable changes. The profile
is not durable state: after an authority restart a desired-running runtime comes
back as inference, the least capable profile.

The local-tools generation runs inside an unprivileged `bwrap` namespace whose argv
is fixed by the authority and built without a shell or any client text:

- `/etc/codex` is replaced by an authority-owned policy whose managed requirement
  `unified_exec = false` removes the persistent command tool and `write_stdin`.
  Passing that flag as plain configuration is not enough: the runtime overrides it.
- `CODEX_HOME` is a private authority directory. The user's Codex home, and
  `~/.codex`, are hidden behind an empty tmpfs: no user configuration, rules, hooks or
  MCP servers. Only the pinned release directory returns, read-only.
- One credential capability crosses: the user's `auth.json`, bound read-write
  because the runtime refreshes its token in place. It is never read, copied or
  logged, and a symlinked, foreign-owned or group/world-accessible file is refused.
  A token refresh by the isolated runtime therefore writes the user's own credential
  file; narrowing that is deferred.
- Every existing project policy layer (`.codex`) from the project root down to the
  workspace is masked read-only, and every thread forces the project root and the
  workspace `untrusted`. The authority creates nothing in the workspace.
- The model catalog is the authority's normalized presentation, with direct tools
  and without code mode, sub-agents or experimental tools; it keeps the patch tool.

Readiness is reported only after the runtime itself proves the profile: its private
home in the initialize answer, the managed requirement through
`configRequirements/read`, every reflected clamp with only the command tool
enabled, no MCP server, no hook, and workspace-only writes (`/tmp` and `$TMPDIR`
excluded, no network). Every thread must come back with approval policy
`untrusted` and that sandbox. Anything unproven is `safety_unproven`, and the same
attestation runs again before each conversation and turn; project policy that
appears in an unmasked layer afterwards stops further work. The host must provide
`/usr/bin/bwrap` and an existing `/etc/codex` directory; the authority never
creates either.

A session attaches as `local_tools_v1` only for the generation's workspace, and
the `command_execute` and `file_change` gate actions are denied under any other
profile. In this profile the authority mediates the provider's command and
file-change approval requests, as described under
[Tool and approval mediation](#tool-and-approval-mediation); nothing runs without
its Accept. A session bound under one profile keeps its Codex thread in that
profile's home.

## Execution leases and internal gate

Execution is denied by default. A prepared ManagedSession has no permission until
an explicit `executionLease/grant` commits a durable lease and audit. Runtime
startup, client reconnection and authority restart never grant or renew a lease.
Lease operations themselves create **zero Codex threads or turns**, submit no
prompts, and answer no approvals or permission requests. A grant alone never executes work.

Schema v3 adds only execution leases and a unique active-lease index; migration
preserves sessions, runtime desired state, receipts and exact audit bytes, and
creates zero leases. Each lease has its own ExecutionLeaseId and revision, explicit
nonempty scopes (`session_create`, `session_resume`, `turn_start`,
`command_execute`, `file_change`), and an authority-time TTL of 1–3600 seconds.
No scope implies another scope.

Schema v6 rebuilds `execution_leases` so its stored scope encoding is bounded by the
exact canonical maximum (80 characters, all five scopes) and adds
`managed_approvals`: control metadata of one command or file-change approval. A row
holds identities, the admitted lease, state, revision, decision, external-effect
marker, deadlines, an authority-computed SHA-256 action fingerprint and bounded
counts. It never holds a command, output, path, diff or file content. At most one
approval is pending, a provider item is mediated at most once, and a schema trigger
refuses any change that is not a legal single-revision transition; only `pending` is
nonterminal. Startup invalidates an undecided approval and marks an accepted one
without a confirmed outcome `indeterminate`; no provider response is ever replayed.

The typed Rust client and developer client support `executionLease/grant`,
`executionLease/get`, `executionLease/active`, `executionLease/renew` and
`executionLease/revoke`. Grant takes session ID, expected session revision, TTL,
comma-separated scopes and intent UUID. Renew takes lease ID, expected lease
revision, TTL and intent UUID; revoke takes lease ID, revision and intent UUID.
Get/active take the corresponding lease/session ID. Renewal preserves scopes and
uses authority NOW + TTL. Original intent retries return their original receipt,
including the original expiry; UUID case and scope order are normalized.

`executionGate/status` takes session ID and one scope, returning an informational
reason only. It is not an authorization token. Execution reevaluates
the internal gate immediately before the authority-owned side effect. It requires
prepared session, active unexpired scoped lease, matching runtime metadata,
desired running, actual ready, live runtime identity, compatible runtime and
healthy canonical storage. A valid lease with stopped runtime still denies;
runtime readiness without a lease also denies.

The single authority writer expires overdue leases at startup and through a
deadline-driven scheduler. Get returns effective state without writing or inventing
a revision; active never returns an overdue lease. Gates compare UTC time even if
expiry persistence is delayed. Storage/audit failure latches execution closed.
Closing a session atomically expires/revokes its lease before closing the session.
Runtime stop leaves leases unchanged. Graceful/hard authority restart preserves
unexpired lease identity, revision and exact deadline, without extending it.
See [ADR 0013](docs/adr/0013-execution-gates-leases-and-revocation.md).

## Managed conversation binding

The authority implementation adds gated conversation create/resume, schema v4,
immutable ManagedSessionId-to-CodexThreadId bindings and an external-effect journal.
Runtime attachment is transient and restart never automatically resumes a thread.
Conversation create/resume itself sends no turn or prompt. Turns require the additional
profiles and turn_start gate described below. Create and resume answer no approval or
permission request. Tauri/human terminal integration is unchanged.

Empty Codex 0.159.0 threads are materialized durably without a turn through an
internal stable start→archive→unarchive persistence barrier. Create leaves the
binding **bound_detached**. Explicit resume requires the separate `session_resume`
lease scope before attachment; authority/runtime restart never resumes implicitly.
No experimental API or history mode is enabled. The compatibility barrier, journal
ambiguity and zero-turn boundary are documented in
[ADR 0014](docs/adr/0014-managed-codex-session-binding.md).

## Inference-only managed turns

Sprint 2H enables the first authority-owned text inference through the typed Rust client
and developer harness. It requires **all three** of an attested `codex_inference_v1`
process, a current `inference_only_v1` attachment, and an allowed internal `turn_start`
execution gate. The compatible runtime is currently exactly Codex 0.159.0 with
experimentalApi=false. No client can supply runtime args, tools, permissions or turn
configuration overrides.

Each generation first runs a discovery-only App Server to read effective MCP names,
then reaps it and starts the final child with every discovered MCP server disabled
before thread creation. Fixed process and thread overrides disable external tool
surfaces. Backend model metadata can register tools independently of those overrides,
so the authority also launches the final child with its own normalized model catalog
(direct tool mode, no experimental, multi-agent or patch tools); with it the provider
receives zero model-visible tools. A catalog that cannot be proven safe keeps the
runtime from starting. config/read and hooks/list must verify the safety preconditions, including
zero enabled executable external hooks. Restoring user/project/plugin/managed hooks
or incompatible configuration makes inference unavailable; Wyss never edits them.
Explicit resume verifies the stored thread/workspace, readOnly sandbox and never
approval policy before publishing its transient attachment profile.

Control operations are `managedSession/turn/start`, `managedSession/turn/status`,
`managedSession/turn/events` and `managedSession/turn/cancel`. The developer start
command takes session ID, expected session revision and intent UUID, and reads the
prompt from stdin. Only one exact text input of 1–8192 UTF-8 bytes is accepted.
Wyss persists its authority-computed SHA-256 fingerprint and byte length, never the
prompt, assistant output or reasoning. Codex-owned conversation history may retain
its legitimate content. Start returns a committed starting receipt asynchronously;
status observes later running/terminal revisions. Retrying the same intent returns
its original receipt and never resends the prompt.

At most one managed turn is starting/running globally. ManagedTurnId is distinct
from CodexTurnId. Durable terminal states are completed, interrupted, failed and
indeterminate. Assistant deltas and visible reasoning summaries use a separate
memory-only stream; raw/private reasoning is suppressed. Per-turn buffers retain
at most 256 events/256 KiB, the last eight turns, with UTF-8-safe splitting and
explicit gaps. Event pages contain at most fifty events within control-frame bounds.
Authority restart preserves canonical status but loses transient output.

Cancellation remains available after lease expiry/revocation. The authority interrupts
on explicit cancel, admitted-lease expiry/revocation or session close. Renewal of the
same lease may extend execution; a replacement never transfers a running admission.
Failure to confirm interruption within the bounded deadline contains/reaps the runtime.
Client disconnect has no turn effect. Runtime/authority restart clears both profiles
and attachments, preserves durable bindings, and never replays a prompt. Nonterminal
turns recovered after a crash become indeterminate.

**In the inference profile, tool, command, file, MCP, web, subagent and
approval/permission execution is disabled.** Any observed external action or server
request fails closed and contains the runtime; no request is answered.
The standalone read-only probe, human terminals and Tauri capabilities are unchanged.
See [ADR 0015](docs/adr/0015-first-managed-turn-and-streaming.md).

## Tool and approval mediation

In a `codex_local_tools_v1` generation the model is offered exactly two tools, a
one-shot command and a patch, and neither does anything on its own. The provider
announces the action and asks; the authority turns that into one pending
`ManagedApproval`; the action runs only after the authority committed an Accept and
wrote exactly one answer. There is at most one active managed turn and one pending
approval, and no approval queue.

```sh
./src-tauri/target/debug/examples/control_client runtime/start/local-tools /abs/workspace
./src-tauri/target/debug/examples/control_client executionLease/grant SESSION 1 600 session_create,session_resume,turn_start,command_execute,file_change INTENT
./src-tauri/target/debug/examples/control_client managedTurn/approval/pending TURN
./src-tauri/target/debug/examples/control_client managedTurn/approval/status APPROVAL
./src-tauri/target/debug/examples/control_client managedTurn/approval/status APPROVAL 0 0
./src-tauri/target/debug/examples/control_client managedTurn/approval/respond APPROVAL REVISION accept INTENT
```

`pending` returns the approval a turn is waiting on. `status` returns its canonical
metadata and, only while the authority still holds the provider request, the action
itself: the complete command with its directory, or one file of the change set
(`status APPROVAL FILE_INDEX OFFSET`) with its complete diff in contiguous 8 KiB
pages. `respond` takes the approval, its expected revision, `accept`, `decline` or
`cancel`, and an intent UUID. There is no generic provider response, provider request
identity or raw provider method. The same intent returns its original receipt from any
connection and never a second provider answer; another payload under the same intent
is `intent_conflict`; a client disconnect decides nothing.

An approval exists only for a complete, bounded action that is provably inside the
session workspace:

- Command: 1–16 KiB, working directory up to 4 KiB, up to 64 parsed actions, and a
  presentation that fits one control frame. Codex wraps the model's command as
  `/usr/bin/bash -lc '<command>'`; that wrapped text is what is shown and approved.
- File change: every path, change kind, move destination and diff; up to 64 files,
  128 KiB per diff and 256 KiB in total. Nothing shown for approval is truncated. A
  request without its complete change set cannot be accepted and is refused after ten
  seconds. A persistent `grantRoot` is never honored.
- Confinement: the working directory and every source and destination must extend the
  canonical workspace component by component. No string-prefix comparison, no `..`,
  no symbolic link anywhere, no hard-linked file, no mount point, no `.codex`, `.git`
  or `.agents` component. The proof is repeated immediately before every Accept.

Anything out of bounds or not provably confined never becomes an approval: the
provider is refused and the turn stops with `action_rejected`.

Accept is an intent the authority revalidates in full: the exact runtime generation,
profile, attachment, turn, provider item, held request, fingerprint and confinement;
a fresh attestation of the isolated profile by the runtime; and, in the committing
transaction, the pending state, revision, deadline, session, turn and the ORIGINAL
lease of the turn with the `command_execute` or `file_change` scope. A replacement
lease never transfers authority. The deadline is the stricter of five minutes and the
lease expiry at the time the approval opened. Decision, dispatch marker, audit and
receipt commit before the one provider answer is written.

Decline and cancel need no execution authority. A refused command is answered with
provider `cancel`, which Codex 0.159.0 offers instead of `decline` and which ends the
turn; a declined file change is answered with `decline` and keeps its turn. Session
grants, exec-policy amendments, network-policy amendments and permission grants are
never sent.

Turn cancel, lease revocation or expiry, session close and shutdown terminalize the
approval before any provider frame, refuse the held request and stop the turn. If the
runtime is lost while an approval is pending it is `invalidated`. If an accepted
answer cannot be proven delivered, or the runtime is lost before the provider reported
the accepted item, the approval is `indeterminate` and the runtime is contained.
Nothing is resent and no action is replayed, including after an authority restart.

The per-turn transient stream additionally reports `command_proposed`,
`file_change_proposed`, `approval_required`, `approval_resolved`, `command_started`,
`file_change_started`, `command_output_delta` and the completed/failed events, under
the same bounds as assistant output. Commands, output, paths and diffs never enter
SQLite, its WAL/SHM, the audit stream or intent receipts: storage holds a SHA-256
fingerprint and bounded counts. Codex keeps its own conversation history in the
private runtime home.

Everything else stays an isolation violation in this profile too, never answered and
contained: hooks, MCP, web and network tools, plugins, apps, skills, dynamic tools,
sub-agents, collaboration, request-user-input, permission escalation, write-stdin,
terminal interaction, a second concurrent request, and any command or file effect the
authority did not accept.

Known limits: the user's `auth.json` is shared read-write with the isolated runtime
(narrowing it is deferred); `bwrap` and a host `/etc/codex` directory are required;
the profile does not survive an authority restart; compatibility is pinned to Codex
0.159.0; and an independent review of this sprint is pending. See
[ADR 0016](docs/adr/0016-tool-and-approval-mediation.md).
