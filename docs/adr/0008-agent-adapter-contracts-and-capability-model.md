# ADR 0008 — Agent adapter contracts and capability model

Status: Accepted
Date: 2026-10-06

## Decision recorded before implementation

Sprint 2A adds validated, versioned wire contracts only. Model providers supply
inference; agent runtimes execute workflows; Wyss adapters translate one runtime
surface into normalized observations and requests. None owns Wyss policy.

Use a capability map keyed by semantic operation, then by `observe` or `request`.
Each access has independent support, conditions and evidence. Omitted entries mean
unknown. Do not expose a `control` level: these surfaces do not establish complete
mediation by Wyss. Capability is not authority. Observed does not mean enforced.

Separate implementation version, observed runtime version, protocol version and
compatibility. Use opaque bounded version text with attributed evidence, not
semver inference. Preserve unknown states and vendor detail. PTY fallback exposes
only raw terminal IO and observed local process status; structured coverage needs
a structured surface. Keep one application crate and existing local validators.

Implement descriptor, session-observation, model and safe error DTOs in Rust and
TypeScript with shared synthetic fixtures. No adapter trait or fake implementation
is needed yet. EventEnvelope v1, terminal behavior and capabilities stay unchanged.

## Executable v1 contract

All four root DTOs have their own required `schema_version: 1`, separate from
EventEnvelope. Unknown versions and unknown core fields/enums reject. Nullable
fields are required on the wire; omission is not null. Rust roots have private
fields and validated serde deserialization. TypeScript parsers validate unknown
input and return detached, readonly-typed values. Nested structures are validated
through their root entrypoint. No runtime integration interface is frozen yet.

### Adapter descriptor

`AgentAdapterDescriptorV1` contains `adapter_id`, `runtime_kind`,
`implementation_version`, `runtime_version`, `protocol_version`, `transport`,
`capabilities`, `compatibility`, `limitations`, `extensions` and schema version.

`AgentAdapterId` and `ModelProviderId` are distinct machine string types:
1–64 ASCII characters, lowercase letter first, lowercase alphanumeric last,
letters/digits/hyphens internally, no consecutive hyphens. These identify an
implementation/runtime family or inference provider, not a running process.
Runtime kinds are `codex`, `claude_code`, `opencode`, `generic_cli`. Transport is
`structured_stdio`, `local_http`, `embedded_sdk` or `terminal_fallback`. Transport
states the selected surface, not every surface a runtime might offer. For example,
a JavaScript SDK wrapping HTTP still represents a local HTTP surface; an embedded
host represents an SDK surface. This vocabulary makes no production compatibility
promise about a particular vendor release.

Versions are bounded opaque strings (128 UTF-8 bytes). Observed runtime/protocol
versions use `{value: string|null, evidence}`. Null means unknown, never a wildcard
compatible version. A confirmed version cannot be null. Implementation version is
separate. Compatibility is `compatible`, `incompatible`, `degraded`, or `unknown`,
with evidence and a nullable reason. Compatible requires declared confirmed
handshake/probe evidence; unknown prohibits confirmed evidence. Other states require
a reason. Probes may demonstrate compatibility without discovering a release
version; compatibility is never calculated from a version string alone.

### Capability negotiation

`capabilities[CapabilityId][observe|request]` contains:

- `support`: `supported`, `unsupported`, `unknown`, `conditional`;
- `evidence`: 1–8 attributed evidence records;
- `conditions`: 0–8 namespaced condition identifiers;
- `reason`: nullable stable reason code.

Observe means the adapter can report a fact with its documented coverage. Request
means it can translate a request; it does not guarantee acceptance, execution,
exclusive mediation, completeness or authority. **Observed does not mean enforced.**
There is deliberately no `control` access. Even confirmed evidence is an adapter's
claim, not a cryptographic attestation or a grant. A future authority must validate
and mediate requests independently of these declarations.

| Capability IDs                            | Allowed access and scope                                                                                    |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `session_create`, `session_resume`        | request: create or reopen a runtime conversation                                                            |
| `session_interrupt`, `session_cancel`     | request: interrupt current work vs cancel a targeted operation; not kill-by-PID or revoke authority         |
| `prompt_submit`                           | request: submit conversation input, without a lowest-common-denominator response API                        |
| `session_status`                          | observe: lifecycle; PTY coverage is process status only                                                     |
| `structured_messages`, `streaming_output` | observe: structured conversation records/deltas; not raw PTY bytes                                          |
| `model_list`                              | observe: catalog, not proof of entitlement or availability                                                  |
| `model_selection`                         | observe effective reported selection / request selection, independently                                     |
| `tool_calls`, `tool_results`              | observe structured invocation/request metadata or result metadata; no promise of pre-execution intervention |
| `approvals`                               | observe runtime approval requests / request a response; not Wyss approval authority                         |
| `subagents`                               | observe declared child work / request child creation; coverage may be partial                               |
| `skills`                                  | observe available skills / request selection; no execution policy                                           |
| `hooks`, `mcp_servers`                    | observe declared runtime configuration/visibility                                                           |
| `token_usage`, `cost_usage`, `latency`    | observe runtime-reported usage/timing; no cost tables, computed prices or trusted audit totals              |
| `working_directory`                       | observe reported cwd / request cwd selection                                                                |
| `terminal_io`                             | observe raw output / request raw input through terminal fallback                                            |

Each access is declared independently; an observation never implies a request.
Missing entries resolve to unknown. Unknown never means supported. Unsupported
and unknown declarations require a reason and no conditions. Conditional requires
at least one condition and a reason; all other support states prohibit conditions.
Positive declarations need at least declared evidence; inconclusive evidence alone
cannot establish support. Unknown cannot carry confirmed evidence. No duplicate
capability list exists: capabilities and access are maps. Producers must emit unique
JSON object member names; parsers accept JSON data, not ambiguous duplicate-member
wire text. Future framing must reject ambiguous input before JSON decoding.

Reason codes: `runtime_version_unknown`, `transport_lacks_structure`,
`requires_configuration`, `runtime_does_not_expose`, `not_implemented_by_adapter`,
`not_probed`, `partial_coverage`, `probe_failed`. Conditions are identifiers for
future documented/probed prerequisites, not executable expressions. A positive
conditional declaration does not mean its condition has been satisfied.

Evidence contains `kind`, `outcome`, nullable `reference`, nullable `observed_at`.
Kinds: static knowledge, runtime handshake, runtime probe, user configuration,
fallback assumption. Outcomes: declared, confirmed, inconclusive. Only a runtime
handshake/probe with an RFC3339 UTC observation time may claim confirmed. References
are informational text (512 bytes), never fetched by these contracts. Configuration
is declarative evidence, not proof of effective runtime behavior.

### PTY fallback and vendor extensions

A pure `terminal_fallback` descriptor may positively declare only `terminal_io`
and observed `session_status`. Structured models/messages/tools/approvals/usage/
subagents remain unsupported or unknown. Evidence or conditions cannot turn PTY
text into structured telemetry. Creating a human PTY is not creating a managed
agent session. A structured adapter may later have a separate presentation terminal;
its descriptor still identifies its structured integration surface.

Extensions are opaque JSON objects under namespaced keys such as `codex.detail`.
Keys have at least two dot-separated lowercase segments with digits, `_` or `-`
after each first letter, maximum 96 bytes. `wyss`, `core`, `normalized` namespaces
are reserved. Unknown valid namespaces are retained, never executed, and cannot
redefine normalized capabilities, state, policy or UI core behavior. Conditions
and safe diagnostic codes use the same namespace syntax.

Bounds: maximum 16 extensions and 16 limitations; limitation text 512 bytes.
Each root accepts at most 2,048 JSON value nodes, depth 12 (root depth zero), 64
members/elements per object/array, 2,048 UTF-8 bytes per JSON string, 128 bytes per
object key and 32,768 aggregate key/string bytes. JSON numbers must be finite;
integers must be JavaScript-safe. This preserves cross-language semantic parity.
Future transports still need frame limits before parsing; these are value limits,
not a network/stream implementation.

### Session, model and error contracts

`ManagedAgentSessionObservationV1` is an observed projection, not canonical
control state. It contains `managed_session_id`, `adapter_id`,
`runtime_instance_id`, nullable opaque `runtime_session_id`, `state`, nullable
`runtime_state`, nullable `observed_model`, nullable presentation `terminal_id`.
`AgentRuntimeInstanceId` is a strong UUID type with a concrete consumer here;
one adapter implementation may describe multiple connections/process instances.
No adapter singleton, PID identity or general process model is introduced.

ManagedSessionId, ConsoleSessionId, TerminalId and runtime instance IDs are distinct
Rust/TypeScript types. Wire UUID text alone cannot prove its identity domain.
Validation rejects wrong field shape and an explicit managed/terminal UUID alias;
allocation/ownership of other cross-domain IDs remains a future authority concern.
Local human session state and behavior are unchanged.

Lifecycle: `unknown`, `creating`, `idle`, `running`, `waiting_for_input`,
`waiting_for_approval`, `interrupted`, `completed`, `failed`, `closed`.
Idle means no active work, not completion of the conversation. Turn completion
normally returns a reusable conversation to idle. Completed is reserved for an
explicitly finished finite managed workflow; closed means resource/session closed.
Interrupted does not imply closure. Waiting-for-approval is a runtime observation,
not a Wyss authorization. An unmapped vendor state must become unknown and retain
its sanitized bounded `runtime_state`; it must not drive a positive transition.
No lifecycle transition engine is implemented. Vendor turn IDs, event details,
retry states, approval semantics and nested agent graphs stay vendor-specific until
a concrete adapter can map them without losing safety-relevant meaning.

`AgentModelDescriptorV1`: nullable `provider_id`, opaque `model_id` (256 bytes),
nullable `display_name` (128 bytes), `availability` = available/unavailable/unknown.
Catalog visibility is existence in a returned catalog; unknown is appropriate when
actual access is unproven. Effective observed selection belongs in the session's
`observed_model`, not a global selected flag. Provider is independent of runtime
vendor. This is no provider implementation, router or cost catalog.

`AdapterErrorV1`: error code, nullable capability operation and nullable diagnostic
with namespaced code and sanitized summary (512 bytes). Codes: unavailable,
unsupported, incompatible_runtime, protocol_error, transport_error,
authentication_required, permission_required, invalid_request, runtime_failed,
interrupted, timeout. Permission-required is a runtime condition, not a grant.
No raw stderr, environment, credentials or arbitrary error chain field is allowed.
Producers must minimize diagnostics; validation cannot recognize secrets in text.
Errors do not authorize retries of potentially completed operations.

## Event provenance and authority milestone

EventEnvelope v1 stays unchanged. Future producers use `source.component` =
`adapter.<adapter_id>`, `source.instance_id` = the adapter producer/writer instance
UUID, and `session_id` = ManagedSessionId when applicable. A payload's
`runtime_instance_id` identifies the observed runtime connection/process instance;
`adapter_id` and opaque runtime session/turn/item identifiers may be included when
needed. Writer and runtime identities are not assumed equal. These are provenance
conventions, never authentication. Stream identity/order remains independent.
No producer, bus, persistence or terminal-byte event implementation is added.

Per ADR 0002, external authority with lifecycle independent from Console is
required **before the first managed-agent execution, including PTY fallback**.
Descriptor support cannot bypass that milestone. WES and the existing Wyss daemon
remain reference/integration candidates, not copied authorities.

## Evidence and deferred mechanisms

See [current official runtime surfaces](../research/agent-runtime-surfaces-2026-10-06.md)
and historical `WYSS-ARCHITECTURE-RESEARCH-REPORT.md` §9 and research block G in the
read-only Downloads material. Historical launch/projection/security APIs are future
work; they do not belong in these descriptive contracts.

Deferred: real adapters, probes, trait/operation APIs, authority, sandbox mediation,
protocol codecs, runtime execution, networking, event production, persistence,
agent UI. Shared fixture claims and probe evidence are explicitly synthetic.
Implementation status: executable Rust/TypeScript roots and shared parity corpus.
