# ADR 0010 — Codex live probe process boundary

Status: Accepted
Date: 2026-10-06

## Decision recorded before implementation

Add an explicitly invoked Rust developer probe, independent of Tauri. Discover
only `codex` using absolute PATH directories in order, checking a regular executable
and canonicalizing symlinks. Ignore empty/relative PATH entries. No caller-supplied
executable, argv, environment, PID or workspace operation is accepted. One owner
runs `codex --version`, then a separate owner runs exactly `codex app-server`.
No shell is invoked. The cwd is this repository's root, determined from the crate
manifest. Inherit the normal local environment; Wyss reads PATH for discovery but
does not enumerate secrets, read authentication/configuration files, initiate login,
set provider overrides or modify configuration.

The probe can construct only initialize, initialized and model/list. There is no
public general-purpose send/spawn API. Initialize uses explicit application client
identity/version and `experimentalApi: false`, with no other opt-ins. A fresh UUID
prefix produces distinct string request IDs per probe/operation. Reuse the 2B
ProtocolState, encoder, wire validator and model normalizer. Initialize must succeed
before initialized, which must precede model/list. Request one page with limit 50,
cursor null and includeHidden false. Never create/resume/read a thread, execute or
interrupt a turn, send prompts, respond to approvals or grant permissions.

## Transport, bounds and ownership

Linux-first implementation uses existing nix/std IO facilities: nonblocking stdin,
stdout and stderr, one synchronous coordinator and poll deadlines. No async runtime,
worker threads, task-per-frame or message queues exist. Blocking this explicit CLI
is permitted; no Tauri/UI runtime calls it. Each owner guards one exact Child,
including failures while configuring pipes. Only that unreaped child's ID/handle
may be signaled; unrelated Codex processes are never attached or controlled.

Stdout is strictly JSONL protocol. An incremental reader holds one partial frame
(maximum 262,144 bytes excluding its newline) and one 4,096-byte scratch buffer.
It checks capacity before appending, rejects excess rather than truncating, and
passes complete frames to the existing codec. Empty/non-JSON frames, unterminated
EOF and invalid envelopes reject; no startup banner recovery or relaxed parser.
Multiple frames in one read and UTF-8 split across reads retain their bytes.

Stderr is independently drained in bounded work units (at most 16 × 4,096 bytes per
pump), with a private 32 KiB tail. Only consumed-byte counts survive into the report;
raw tail contents are never printed or serialized and are cleared on shutdown.
Version stdout is at most 4 KiB; retained UTF-8 version text is one nonempty,
control-free line of at most 128 bytes. An optional numeric dotted suffix is
informational, never the compatibility rule. Version exit status is checked.

Timeouts: version output 5 seconds; each protocol write and each matching response
10 seconds (including startup for initialize); final EOF/notification drain 1 second.
The fixed plan has two outstanding/completed request identities in total and one
request in flight. ID type/value must match exactly. Unrelated responses and duplicate
completions reject, including during final drain. Notifications may interleave;
retain bounded method/count summaries (64 distinct methods, 1,024 messages) and
the first structural key summary per method. Each structural summary holds at most
16 ASCII key names of at most 64 bytes, a validated method of at most 128 bytes,
category and emission-field presence. One rejected-frame summary may survive. No
params, result, error contents or emission timestamp values enter these diagnostics.
Their existence never establishes capabilities. Any server request stops the probe
without answering, regardless of subtype or whether it arrives during final drain.

## Compatibility and safe report

`CodexLiveProbeReportV1` is an in-memory, serialize-only developer result with its
own schema version, Unix-millisecond timestamp, fixed schema source revision,
executable/version/cwd, phase results, fresh request IDs, owned PID, compatibility,
bounded normalized catalog metadata, notification counts, fixed diagnostic codes,
optional numeric vendor error code and shutdown facts. It is not a shared UI DTO,
canonical session, EventEnvelope, audit log or persisted probe history.

Compatibility uses the Sprint 2A enum:

- compatible: handshake succeeded and observed frames matched the existing codec;
- degraded: handshake succeeded but optional catalog/configuration/provider access
  failed or transport availability was insufficient to finish the probe;
- incompatible: actual framing/envelope/required payload mismatch or correlation
  contradiction; version differences alone are insufficient;
- unknown: missing executable, spawn/version/environment failure, timeout/early exit
  before handshake, or an unclassified initialize rejection.

Model/list RPC error responses are degraded after a successful handshake. Generic
numeric codes cannot prove authentication versus configuration/provider causes;
therefore retain only the numeric code and `runtime_error`, without English-message
matching or invented authentication certainty. Raw provider messages/data never
become user diagnostics. A malformed catalog or interleaved frame remains a codec
incompatibility, distinct from an ordinary catalog error.

Confirmed evidence is limited to structured stdio and initialization; model_list
observation is added only after successful validation/normalization. No descriptor
upgrades untested session/tools/approvals/skills capabilities. Models keep unknown
provider/availability through the 2B normalizer; listing is not entitlement and this
page describes only the current runtime/provider configuration. Default/hidden and
reasoning-effort metadata are bounded selected fields, not a global model catalog.
No capability grants authority. Observed does not mean enforced. An approval request
is not authorization. No managed session identity is allocated.

## Shutdown

Stop sending, close stdin and drain final frames for at most one second. The child
owner then allows up to one second for EOF exit, sends SIGTERM only to its exact
unreaped child if necessary, waits 500 ms, escalates to Child::kill (SIGKILL) if
necessary and polls/reaps for up to two seconds. Pipes/tail are released. On errors
or panic, Drop closes stdin and uses kill/wait as a final reap guard. No worker can
remain blocked on inherited pipes because IO is nonblocking and no workers exist.
The final kill/wait guard prioritizes reaping; like std process operations, its
completion still depends on the OS kernel. No process supervisor or crash-surviving
runtime is promised.

## Verification and current result

Hermetic protocol tests use synthetic in-memory peer messages. Linux process tests
use standard local coreutils (`cat`, `printf`, `head`, `sleep`, `false`), never Codex,
network or credentials. They cover bounds, deadlines, stderr floods, natural EOF,
SIGTERM resistance, kill/reap, isolation, correlation and the outbound allowlist.
Normal test commands do not discover/start Codex. The separate example is explicit;
absence prints unknown/runtime_unavailable and exits 0, while an installed failed
probe exits 1. Compatible/degraded completed probes exit 0 only with cleanup and
without unexpected server requests.

The first live attempt initialized successfully but rejected an interleaved
notification. A subsequent bounded structural diagnostic conclusively identified
`emittedAtMs` on `remoteControl/status/changed`, with keys `emittedAtMs`, `method`,
`params`. ADR 0009 records the correction of the original 2B schema omission,
including evidence that the field existed in the pinned source. Unknown fields
still reject; no general validation relaxation was introduced.

The corrected 2026-10-06 probe of Codex 0.159.0 completed initialize/initialized
and model/list (limit 50, cursor null, includeHidden false): seven models, no next
cursor, compatibility compatible. Interleaved `remoteControl/status/changed` and
`account/updated` each carried the same legal three-key envelope with `emittedAtMs`.
Only structural summaries survived; no raw payload was retained as an artifact.
Evidence confirms only structured stdio, initialization and model-list observation,
not entitlement or untested agent capabilities. Owned PID 213482 exited through
EOF, status 0, and was reaped without signals. Repository-content snapshots and
the four unrelated Codex processes were unchanged. No server request was answered.

Deferred: frontend exposure, network transports, live managed threads/turns, authority,
authentication management, approval execution, persistent reports, Event Bus and all
agent work. ADR 0002's external authority milestone remains in force.
