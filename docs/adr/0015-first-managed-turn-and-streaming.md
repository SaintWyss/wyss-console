# ADR 0015: First managed turn and structured streaming

Status: Accepted
Date: 2026-10-07

## Decision

Wyss admits inference-only, text-only managed turns through the external authority.
There is at most one starting/running managed turn globally, enforced by the sole
SQLite writer and a partial unique index. A prepared session, a ready runtime, or
a conversation binding alone grants no execution permission. Tauri and human PTYs
are unchanged; no chat UI or desktop capability is added.

ManagedTurnId is an authority-generated UUID domain distinct from ManagedSessionId,
ExecutionLeaseId, AgentRuntimeInstanceId and provider CodexThreadId/CodexTurnId.
The provider turn ID is meaningful within its thread. Turn revisions are separate
from session, binding, lease and runtime revisions. Canonical states are starting,
running, completed, interrupted, failed and indeterminate. The last four are terminal.
Admission, provider acceptance and termination normally produce revisions 1, 2, 3.

## Three necessary admission facts

ExecutionGate(turn_start) requires all previous lease/runtime conditions plus:

- a permanent session/thread binding;
- an explicit current attachment to the exact live runtime generation;
- RuntimeSafetyProfile::CodexInferenceV1;
- AttachmentProfile::InferenceOnlyV1;
- no other active managed turn and healthy canonical storage/audit.

Decisions remain internal and point-in-time. The diagnostic endpoint is informational,
never a serialized capability token. A denied action writes zero turn/start frames.
The supervisor checks the authority-owned admission flag and expiry again immediately
before writing. Revocation after a write triggers interruption; it cannot erase admission.

## Runtime safety preflight

The supported inference runtime is exactly Codex 0.159.0, pinned upstream revision
[687a119f0fcaace47e1f1abcc77cec6c813fd6da](https://github.com/openai/codex/tree/687a119f0fcaace47e1f1abcc77cec6c813fd6da).
Other versions fail closed pending new compatibility evidence. Direct process argv,
never a shell or client-supplied flags, contains fixed `-c key=false` overrides for:

```text
features.apps
features.code_mode
features.code_mode_only
features.context_management
features.current_time_reminder
features.deferred_executor
features.enable_fanout
features.goals
features.hooks
features.image_generation
features.memories
features.multi_agent
features.multi_agent_v2
features.plugins
features.request_permissions_tool
features.shell_snapshot
features.shell_tool
features.standalone_web_search
features.token_budget
features.tool_suggest
features.unified_exec
features.view_image
cloud.skills.enabled
skills.include_instructions
tools.experimental_request_user_input.enabled
tools.update_plan.enabled
```

It also sets `web_search="disabled"`. These keys come from the pinned
[temporary_structured_request implementation](https://github.com/openai/codex/blob/687a119f0fcaace47e1f1abcc77cec6c813fd6da/codex-rs/tui/src/temporary_structured_request.rs).

A discovery-only child may initialize, list models and read config; it cannot create,
resume or turn a thread. Effective MCP names are validated as 1–64 ASCII bare TOML
segments `[A-Za-z0-9_-]`, at most 64 entries. This child is reaped. A fresh process
receives an additional fixed `mcp_servers.NAME.enabled=false` override for every
name before any thread operation. No configuration, auth material or MCP secrets
are copied or persisted.

The final child initializes with experimentalApi=false and obtains its bounded model
catalog, then config/read verifies reflected safety settings and disabled MCP entries.
The two tools.* values are not reflected by 0.159.0 config/read: they remain immutable
process and thread overrides backed by the pinned implementation, rather than being
claimed as reflected evidence. hooks/list must prove zero enabled executable external
hooks of any source. Unknown hook enablement is a rejection. Only after successful
preflight and durable readiness audit does this generation receive CodexInferenceV1.

The earlier thread-only and process-only probes executed a user synchronous
SessionStart command hook. That was a failed isolation boundary. The user explicitly
disabled the four user hooks outside Wyss, retaining a private reversible backup.
Managed policy may still enable the hooks engine; zero executable hook definitions
is independently checked at every safety preflight. Restored hooks cause rejection.
Wyss does not edit the user's hooks, authentication or system policy.

Safety preflight runs again for the stored workspace before create, resume and turn
admission. A newly discovered MCP name clears safety by containing the old child;
one bounded safety-only reconciliation per explicit runtime revision starts a fresh
generation with the complete disable set. It never requeues the rejected operation
or prompt. Further drift fails closed without a restart loop. No idle filesystem
watcher or database polling is introduced; passive drift is discovered at preflight.

config/read is sensitive, bounded and memory-only. Its exact correlated success may
contain up to 512 origins entries (the installed runtime returned 312); all other
object limits remain 256, with unchanged 256 KiB frame, depth, node and string bounds.
The configuration never enters client projections, audit or diagnostics.

## Inference attachment

Explicit session_resume authorization remains required. The supervisor reads effective
workspace configuration, disables all effective MCPs and repeats the fixed isolation
settings in thread/resume.config. Parameters are stored threadId/cwd, excludeTurns=true,
approvalPolicy=never, sandbox=read-only and authority-owned config only. It verifies the
same thread/canonical cwd, returned readOnly sandbox, no network access and no incompatible
active permission profile before publishing InferenceOnlyV1.

Both profiles are transient and tied to the actual runtime generation. Stop, exit,
authority restart or isolation failure clears attachment and safety. Bindings remain.
No automatic resume or prompt replay occurs. Creation retains the 2G persistence barrier
start/archive/unarchive and returns bound_detached, requiring separately gated resume.

## Prompt privacy and durable admission

Only one Text input is accepted: 1–8192 exact UTF-8 bytes, without trimming or truncation.
The authority computes SHA-256 using sha2 0.10.9. Canonical intent comparison includes
method, canonical session/intent identity, expected session revision, digest and byte
length. No client-supplied digest is trusted. No other UserInput variant is encoded.

Schema v5 explicitly migrates v1–v4, preserving existing rows, EventEnvelope bytes,
stream ID and sequence. Migration creates zero turns or provider activity. managed_turns
stores only identities, admitted lease revision, runtime generation, canonical lifecycle,
SHA-256/length, fingerprint-only intent manifest, dispatch marker and timestamps/reason.
The row and immutable receipt are cross-validated on reopen. Future/corrupt schemas
are rejected and failed migration rolls back.

The starting reservation, dispatch marker, admitted audit and fingerprint-only intent
receipt commit atomically before provider write. Start returns that committed starting
receipt asynchronously; status observes later revisions. An ordinary retry returns the
original receipt without re-admission. Different prompt bytes conflict. Provider acceptance
commits the CodexTurnId/running audit; validated terminal lifecycle commits terminal state
and audit. Audit failure contains the runtime and makes execution unavailable.

The only turn/start parameters are:

```json
{
  "threadId": "<durable thread ID>",
  "input": [
    { "type": "text", "text": "<transient prompt>", "textElements": [] }
  ]
}
```

No permission/model/cwd/tool/config overrides are possible in this path. Prompt text may
exist in transient request memory and Codex-owned conversation history. It never enters
Wyss SQLite/WAL/SHM, journal, receipts, events or diagnostics. Database sentinel tests also
exclude transient assistant and summary sentinels from all Wyss-owned durable files.

## Structured transient stream

The supervisor remains the single stdin/stdout owner with one pending RPC, unique exact
request correlation, a one-slot typed command channel and bounded deadlines. Legal turn
notifications may precede the response; the core stages at most 32 provisional notices
and 128 KiB before correlation. Wrong thread/turn/generation cannot mutate a turn.
Late or duplicate RPC responses fail closed. Duplicate lifecycle notifications cannot
repeat a terminal canonical transition.

ManagedTurnStreamEventV1 uses a per-turn positive decimal-string sequence and safe
turn_started, assistant_message_delta/completed, reasoning_summary_delta/part,
turn_completed/interrupted/failed and stream_gap events. Assistant completion is metadata
only. Summary-part metadata has no provider text. Raw item/reasoning/textDelta is dropped;
full reasoning items and provider turn histories are never exposed.

Rings retain at most 256 events and 256 KiB serialized bytes per turn, last eight turns,
with a 2 MiB aggregate cap. Text splits at UTF-8 boundaries into at most 4096-byte deltas.
Pages contain at most 50 events and stay within the 64 KiB control envelope. Dropped events
and eviction are explicit gap/retention metadata. Restart loses stream content, while
canonical status remains durable. No delta is an EventEnvelope.

## Cancellation and authorization loss

Cancel is a reductive idempotent authority intent requiring turn ID/expected revision,
not a valid execution lease. Its audit/receipt commit before interruption. A new cancel
intent for a terminal turn is invalid_transition; original retries return their receipt.
The provider receives only exact stored threadId/turnId through turn/interrupt.

Codex 0.159.0 can acknowledge turn/start before its core is interruptible. Cancellation
therefore remains pending until the exact correlated turn/started notification arrives;
notifications received before the RPC response are staged and applied after correlation.
The original five-second containment deadline is never extended while waiting. This avoids
the pinned provider's early "no active turn to interrupt" rejection without retries,
empty turn IDs or a second prompt. A response-first deterministic regression rejects the
previous behavior and passes with the deferred reductive request.

The interrupt RPC deadline is three seconds; terminal confirmation is bounded to five
seconds. Failure contains/reaps the owned runtime and records a conservative terminal
outcome. Lease expiry or revocation, session close and shutdown wake cancellation. Renewing
the same lease extends its deadline; replacement never transfers a running admission.
Session close commits its established lease/session transaction first, then interrupts;
provider failure does not roll back a committed close. Explicit runtime stop reaps the child
and conservatively terminates the active turn. Desired state retains prior 2E semantics.

The scheduler waits on provider/control/deadline wakeups, with no busy journal/SQLite poll.
Client disconnect has no lifecycle effect. Graceful shutdown attempts bounded interruption
and always reaps. PDEATHSIG still contains the child after authority SIGKILL. Startup marks
any previous nonterminal turn indeterminate with a single durable audit; no thread resume,
turn/start or prompt resend is automatic. Indeterminate is terminal and queryable; a future
new turn requires a new explicit admission. Exactly-once provider execution is not claimed.

## External actions remain forbidden

Any hook/MCP-startup lifecycle, command/file/MCP/web/dynamic-tool/subagent item, non-empty
turn diff or server request is inference_isolation_violation. Unknown notification classes
are rejected unless explicitly classified as benign lifecycle. The owner attempts an
interrupt when the turn identity is known, then contains the runtime and clears profiles
and attachments. Server requests receive zero answers. Durable bindings survive.

The production outbound allowlist is exactly initialize, initialized, model/list,
config/read, hooks/list, thread/start, thread/archive, thread/unarchive, thread/resume,
turn/start and turn/interrupt. hooks/list is exclusively safety preflight, explicitly
approved as the eleventh method. The standalone read-only probe still sends only its
original three methods. No generic sender, approval encoder or permission response is
reachable through managed control operations.

Command/file/MCP/web/skills/apps/plugins/subagent execution and approval mediation remain
disabled. Sprint 2I must deliberately establish authority mediation before replacing this
inference-only boundary. No arbitrary tool-capable safety claim is made here.
