# Managed protocol 2: candidate wire contract

Contract revision 1, based on main `2f50390`. **Not enabled, not owner-accepted.** This specifies
request deltas against the exact public `contracts::{control,binding,execution,turn,approval}`
types in this baseline; unspecified keys are forbidden, not extension points. No generic argv,
environment, model/effort selection, prompt forwarding or tools are introduced. Existing typed
turn input and accepted local-tools workspace/profile fields keep their original restrictions.

## Envelope and version negotiation

Hello has exactly `schema_version:2`, `request_id:UUID`, `method:"authority/hello"`, and
`params:{protocol_version:2,client_instance_id:UUID}`. Before hello all other operations refuse
`handshake_required`. A v1 hello refuses `protocol_version_unsupported`; no implicit session.

After hello every request has exactly those four envelope keys plus `authority_instance_id:UUID`
equal to the instance observed in hello. A stale instance refuses `correlation_error`, without
looking up another runtime. Frame cap remains 65,536 bytes, unique JSON keys, canonical UUIDs,
strict finite integers and existing bounds. EOF/oversize never dispatch a partial request.

Responses have exactly `schema_version:2`, `request_id`, `authority_instance_id`, and either
`result:{kind,value}` or `error:{code}`. UUID correlation and the result variant remain mandatory.
Closed error vocabulary is the current `ControlError`; no provider prose. Stale generation/fence
uses `stale_revision`, mismatched session/identity domains `invalid_request`, capacity
`capacity_exceeded`, unknown methods `unsupported_method`. Errors do not carry paths/secrets.

## Exact request deltas

All 29 current method words remain the closed allowlist, with no aliases. Begin with the exact
baseline `Operation` variant parameters and validation; apply **only** the following deltas.

| Methods                                                          | Added/changed mandatory parameters                                                                                                              |
| ---------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `authority/hello`                                                | protocol_version must be 2                                                                                                                      |
| `runtime/status`                                                 | add `managed_session_id:UUID`                                                                                                                   |
| `runtime/start`                                                  | add `managed_session_id:UUID`, `expected_generation:u63`; `expected_revision` becomes a required u63 (0 only for absent row)                    |
| `runtime/stop`                                                   | same as start, plus `expected_runtime_instance_id` (UUID or null); safety_profile/workspace remain forbidden for stop                           |
| `runtime/model/list`                                             | add `managed_session_id:UUID`, `expected_generation:positive_u63`, `expected_runtime_instance_id:UUID`; existing cursor/limit retained          |
| `managedSession/runtime/create`, `managedSession/runtime/resume` | add `runtime_fence:RuntimeFence` with positive generation/non-null instance                                                                     |
| `managedSession/turn/start`, `managedSession/turn/cancel`        | add positive/non-null `runtime_fence`                                                                                                           |
| `executionLease/grant`, `executionLease/renew`                   | add positive/non-null `runtime_fence`                                                                                                           |
| `managedTurn/approval/respond`                                   | add positive/non-null `runtime_fence`                                                                                                           |
| `managedSession/close`, `managedSession/runtime/abandon`         | add `runtime_fence`; zero/null allowed only when no runtime has ever been reserved; stopped prior launch uses last generation and null instance |
| Remaining 15 methods                                             | no parameter delta; after-hello envelope still names the authority                                                                              |

`u63` is an integer in [0, 9223372036854775807], never floating point/string/bool.
`RuntimeFence` is exactly:

```json
{
  "managed_session_id": "00000000-0000-4000-8000-000000000001",
  "generation": 1,
  "runtime_instance_id": "00000000-0000-4000-8000-000000000002"
}
```

The session and non-null instance must be distinct valid UUID domains. A fence asserts an
expected observation; it grants nothing. Resolve lease/turn/approval/operation IDs in the store,
check their session equals the fence, then check the registry's authority/session/generation/live
instance. Never choose the GUI's selected project, a PID, provider request ID alone or the only
current runtime. A null instance means explicitly no live/starting reserved instance, not wildcard.
Existing provider thread identity is globally unique in bindings; collisions across sessions refuse.

New runtime row: first start commits revision 1/generation 1 and allocation identity atomically;
later mutation increments revision by exactly one, each new attempt increments generation by one.
Start while a matching launch is live retains it and its generation (existing explicit-intent
semantics); no duplicate discovery/spawn. Capacity is checked before a new reservation. A failed
attempt consumes its generation. Overflow refuses before durable mutation/spawn.

For exact intent replay, validate syntax and compare canonical payload including fence first.
Return the original receipt with zero side effects even if the old launch is gone; a changed fence
or payload under the same intent ID is `intent_conflict`. New requests require current fences and
all original execution gates immediately before dispatch. Replaying a receipt never reattaches,
regrants or reexecutes work. Preserve the existing active-turn stop safety guard. A new cancel of an already terminal turn
checks its immutable stored session/launch reference and returns the terminal projection without
provider dispatch; it need not find that launch alive. A cancel of an active turn requires the
current live fence and admission checks. No terminal read can restart or reattach a worker.

Read-only historical turn/events/approval reads still name their immutable record IDs; they may
show legacy unassigned history. They cannot resume or admit that history. A model cursor and every
new runtime/attachment observation include the launch identity; stale cursors refuse.

## Result deltas

Hello/status retain all current fields with `protocol_version:2`, `storage_schema_version:8`.
Runtime control results add `managed_session_id` and durable `generation`. Runtime status adds
those fields, `runtime_identity:null|RuntimeIdentity` and `budget_failure:null|budget-key`; null identity
means not published as a live process. `budget-key` is exactly a numeric ceiling key from resources.json
(excluding contract_version); `budget_failure` is non-null only for diagnostic `budget_exceeded`, a new
closed diagnostic word. No arbitrary provider diagnostic text. Runtime models adds non-null `runtime_identity` and preserves the existing model/offer/
completeness/next fields. Existing result kind words do not change.

`RuntimeIdentity` has exactly `schema:2`, `authority_instance_id`, `managed_session_id`,
`runtime_instance_id`, positive `generation`, and `observed_version` (the existing bounded numeric
provider version, never a client claim). Publish only after spawn and successful attestation;
reserved launch metadata may fence pending work but cannot report ready. During initialization,
`runtime_instance_id` is the reserved live instance, while `runtime_identity` stays null and
compatibility unknown. This permits an exact stop of a pending launch without attesting readiness.

New binding-status attachments use the live RuntimeIdentity. Turn/approval status projections add
`runtime_ref:null|RuntimeRef` and `attestation:null|RuntimeIdentity` instead. RuntimeRef has exactly
`schema:2`, `authority_instance_id`, `managed_session_id`, `runtime_instance_id`, and positive
`generation`, taken from immutable allocation/sidecar rows. It survives restart. Attestation includes
observed_version and exists only while that exact launch is attested live; never reconstruct it
from an installed binary or a different generation. A V8 historical record retains runtime_ref with
null attestation; a legacy unassigned record has both null. Permanent bindings and historical
turn/approval records, receipts and stream cursor IDs remain immutable. No legacy generation 1 is invented.
Approval status and non-null pending-approval projections also carry `decisions`, a unique list
of at most three closed words (`accept`, `decline`, `cancel`) offered by that live provider request.
Historical or unavailable requests carry an empty list. These are transient presentation and
admission capabilities, never durable authorization. An unoffered decision refuses before recording
an intent or writing to the provider. Protocol 1 and its approval payloads are unchanged.

Durable leases remain session grants; launch-scoped admission is transient and separate.

This file freezes candidate shapes; production serializers/GUI parsers and their shared fixtures
must be implemented/reviewed together. The contract tests exercise request deltas and routing
oracles only. They do not prove the live protocol, provider isolation or owner acceptance.

## Durable event attribution (vocabulary 2)

The enclosing EventEnvelope stays schema_version 1, with the same sole stream and sequences.
V7→V8 raises metadata event_vocabulary monotonically to at least 2. Existing event/receipt bytes
are untouched; validators accept known historical vocabulary-1 payloads only in their original
shapes. New generation-aware events use payload_schema_version 2; unknown versions refuse.

- `authority.runtime.desired_state_changed`: session_id is the explicit session; payload exactly
  `adapter_id,runtime_kind,desired_state,revision,generation,runtime_instance_id` (instance nullable
  when no live/reserved launch). correlation_id is the mutation intent ID. generation is the
  durable counter after the mutation, including a reserved failed attempt.
- `authority.runtime.instance_ready`: session_id non-null; payload exactly
  `adapter_id,runtime_kind,runtime_instance_id,generation,runtime_version,compatibility`.
  version is the observed bounded numeric version; compatibility is the closed baseline word.
  Append and commit before publishing ready. source.instance_id is the owning authority instance.
- `authority.runtime.instance_exited`: session_id non-null; payload exactly
  `adapter_id,runtime_kind,runtime_instance_id,generation,exit_kind,expected,desired_state,budget_failure`.
  Fixed baseline exit reason words plus `budget_exceeded`, nullable closed budget key and boolean expected; no PID. Append only after owned reap.
- New managed-turn and approval events retain their exact baseline payload fields and add exactly
  `generation`; session_id must equal the joined sidecar's session, runtime_instance_id/generation
  must equal its allocation reference. source.instance_id is the authority writing the event;
  allocation's authority may be historical for a post-restart indeterminate observation.
- Unchanged event types keep payload_schema_version 1 and their exact baseline validation. New
  recovery events about unassigned V7 rows retain the legacy payload shape/version and may never
  admit work. No new runtime event uses null session_id. No per-token audit or secret content.

Validators must join canonical rows/sidecars in the same writer transaction, reject inconsistent
attribution, and account for the unchanged 2 KiB event bound before advancing the stream head.
A post-crash writer must not fabricate a historical exit observation. The old reader refuses V8,
so vocabulary-2 events cannot be interpreted accidentally by the legacy executable.

## Inactive implementation clarifications

`src-tauri/src/f8/protocol.rs` implements strict request parsing/serialization and partial response
codecs, not server activation. Binding-status adds required `runtime_identity` (nullable, present
exactly when bound-attached), checked against its existing session/instance. Model cursors keep the
baseline `{runtime_instance_id,index}` shape, index <=50; they must match the enclosing live identity
and explicit request generation/instance. No cursor can select a launch by itself. Runtime-control
receipts have revision expected+1 and the requested desired state; stop retains generation, start
retains it for the same launch or increments exactly one (first allocation is 1). Exact-intent replay
uses its original request expectations. These clarify the candidate; no approved V1 contract changes.

Audit results are decoded by `f8/audit.rs`: per-event vocabulary-1/2 validation, page correlation
(stream, cursor, limit, exact contiguity, unique event IDs) and stateful `AuditReplay` (sole-writer
authority succession, disjoint session/instance/authority domains, launch attribution, once-only
ready/exit, closed turn/approval transition tables, 128/4096 lifetime limits). All non-audit methods
now have result-kind/record correlation. Binding-create success requires the independently accepted
canonical workspace via `correlate_binding_create`; generic correlation refuses without that context.
Closed-session no-op returns revision 2 unchanged, as in the baseline. Do not use partial codecs to enable a V8 client/server. See the
[implementation evidence](../../acceptance/f8-code-2026-10-20.md) for the remaining boundaries.

## Window integration clarifications, 2026-10-21

Found by driving the protocol through the desktop's own client; none changes a method, a key set or
an error word.

- **Runtime status is not empty.** `runtime/status` reports, for the live launch of the session, the
  safety profile it was asked for (`runtime_safety_profile`) and, for local tools, the one workspace it
  is isolated for (`workspace_root`), exactly as protocol 1's status does; `model_count` is the size of
  the catalogue the attested launch listed (zero until it is ready). A session whose desired state is
  `running` but which has no live launch reports `actual_state: failed` with the closed exit word as
  `diagnostic` (`budget_exceeded` with its `budget_failure` key) **only** when the authority that is
  answering recorded an unexpected `instance_exited` for the current generation. An exit recorded by a
  predecessor authority is history: the runtime is reported `stopped`, not failed.
- **Why a turn or an approval ended.** Work stranded by a launch of this authority that is gone is
  recorded as `runtime_failed` / `runtime_lost`, or as `authority_shutdown` when the authority itself was
  ending. `authority_restarted` is used only for work whose launch belongs to a predecessor.
- **A binding the runtime did not survive.** A create or resume whose runtime was lost while it was
  `reserved`/`dispatching` becomes uncertain (`binding_indeterminate`, operation retained) on the next
  supervisor tick, so it can be abandoned; it is never redispatched.
- **Counters the window cannot hold.** A generation counter at the end of its range is serialized by the
  window's service as the largest integer a JavaScript number holds exactly, with `exhausted: true`; the
  protocol itself is unchanged (u63).
- **Transport.** A frame whose header and body the peer has already sent is read in one supervisor
  quantum (it cost two). A client that asks for nothing for five seconds is dropped; the window's service
  does not reuse a connection that rested more than two seconds. Durable lease and approval expiry is
  recorded at most every 250 ms; reads still project expiry by the clock.
