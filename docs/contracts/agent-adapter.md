# Agent adapter and Codex protocol boundary

The adapter contract describes observations and translated requests. It does not
own Wyss policy, canonical Managed state, grants or approval. Current availability
is in [STATUS](../STATUS.md), not the historical time at which a DTO was introduced.

Executable definitions:
[Rust](../../src-tauri/src/contracts/agent_adapter.rs),
[shared validation](../../src-tauri/src/contracts/agent_adapter/validation.rs),
[TypeScript](../../src/lib/contracts/agent-adapter.ts),
[Codex codec](../../src-tauri/src/adapters/codex/wire.rs),
[normalization](../../src-tauri/src/adapters/codex/normalize.rs) and
[explicit probe](../../src-tauri/src/adapters/codex/probe/mod.rs).

## Shared DTO and capability rules

Descriptor, observed-session, model and safe-error roots have required schema
version 1 and required nullable fields. Unknown versions/core fields/enums reject.
Parsers validate unknown data; TypeScript returns detached readonly projections.
An observation is not canonical control state or permission.

Capability entries are keyed by semantic operation and separately by `observe`
and `request`; no `control` access is defined. Missing means unknown. Request
support is not proof of execution, entitlement, exclusive mediation or acceptance.
Capabilities cover session create/resume/interrupt/cancel/status, prompt,
structured/streaming output, models/selection, tool observations, approvals,
subagents, skills, hooks/MCP, usage/timing, cwd and raw terminal I/O with their
individually supported access modes.

| Rule          | Meaning                                                                                                                                                                   |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Support       | supported, unsupported, unknown or conditional. Conditional needs nonempty conditions and reason; unknown/unsupported need reason and no conditions.                      |
| Evidence      | 1–8 attributed records; only a handshake/probe with UTC observation time can claim confirmed. Configuration is declaration, not effective behavior.                       |
| Versions      | Implementation, observed runtime, protocol and compatibility are distinct; bounded opaque text, not semver inference. Unknown version is not a compatible wildcard.       |
| Compatibility | compatible, incompatible, degraded or unknown, with matching evidence and required reason. Positive handshake coverage does not establish unrelated capabilities.         |
| Models        | Listing is visibility, not entitlement. ModelProviderId is distinct from agent runtime; effective selection belongs to the session, not a global catalogue flag.          |
| Lifecycle     | idle is reusable, not completed; interrupted is not closed; waiting for provider approval is not Wyss authorization. Unknown vendor state stays unknown.                  |
| Errors        | Stable safe code and optional minimized diagnostic; no raw stderr, environment, credentials or arbitrary provider error chain. Errors do not authorize uncertain retries. |

Machine adapter/provider IDs are distinct 1–64-character lowercase ASCII
identifiers, with no consecutive hyphens. Runtime instance IDs and local/Managed
UUID domains remain separate; opaque provider thread/turn/item IDs are not Wyss
identities. Evidence references are informational and are never fetched/executed
by validators. Observed does not mean enforced.

Extensions use bounded namespaced keys; reserved core namespaces cannot be
overridden. Unknown valid extensions are retained as data and cannot change core
state, policy or UI behavior. Root validation is bounded to 2,048 value nodes,
depth 12, 64 members/elements per container, 2,048 UTF-8 bytes per value string,
128-byte keys and 32,768 aggregate key/string bytes. Finite numbers and JavaScript-
safe integers preserve cross-language parity. Value bounds do not replace
pre-parse transport framing limits.

A pure terminal-fallback descriptor positively declares only raw terminal I/O
and observed local process status. PTY text does not prove structured messages,
models, usage, skills, approval mediation or creation of a Managed session.

## Codex wire and normalized projections

Provider JSON, request identities and vendor metadata stay in Rust. Common
validated DTOs cross to Svelte; private/raw reasoning content is suppressed.
Visible reasoning summaries, plans, agent messages and unknown item metadata
remain distinguishable. Vendor error messages do not become common diagnostics.

The codec rejects duplicate members, trailing second values, unknown envelope
fields and ambiguous combinations. Direction is explicit; Codex omits `jsonrpc`.
String request IDs and signed i64 IDs retain type/value, including extrema, and
do not pass through JavaScript as raw wire numbers. Absent/null params differ.
Only server notifications admit optional nullable signed-i64 `emittedAtMs`;
it is provider metadata, not stream order, authority or a Wyss timestamp.

| Wire bound         | Limit                                                      |
| ------------------ | ---------------------------------------------------------- |
| Complete frame     | 262,144 bytes.                                             |
| Value structure    | Depth 16, 8,192 nodes, 256 members/elements per container. |
| Keys/content       | 256-byte keys, 65,536-byte content strings.                |
| Method/error text  | 128-byte ASCII methods; 4,096-byte vendor error messages.  |
| Opaque identifiers | Nonblank, control-free strings of at most 256 UTF-8 bytes. |

Oversize data rejects rather than becoming a truncated valid observation.
Unconsumed bounded vendor structures stay private; the codec is not a complete
clone of every upstream schema. Unknown requests/notifications receive no guessed
semantics or automatic responses. Response interpretation needs the original
operation/correlation context; a binding mismatch rejects rather than allocating
an identity. Experimental coverage is distinguished and opt-in, not advertised
as stable from static fixtures.

Initialization is initialize, matching success, initialized before other outbound
operations. Approval subtypes and original request identities remain distinct;
an encoded provider response is not approval by Wyss. Permission encoding at this
codec boundary is limited to an empty grant; effectful mediation belongs to the
qualified authority profile. Catalogue normalization uses unknown availability
and only valid explicit provider metadata.

## Explicit read-only probe

The standalone developer probe discovers a regular executable from absolute PATH
entries, runs version and a separately owned app-server, then only initialize,
initialized and one bounded model/list page (limit 50, no hidden models). It does
not create/resume threads, submit prompts, run/interrupt turns, answer requests,
log in, edit configuration or infer agent capabilities from model visibility.

It uses nonblocking owned pipes/deadlines and validates JSONL through the codec:
one 262,144-byte frame, 4 KiB scratch, private 32 KiB stderr tail. Raw stderr and
payloads are not emitted; only bounded safe metadata/structural summaries survive.
Version text is bounded to one control-free line; each fixed request keeps exact
correlation and only one is in flight. Unrelated/duplicate responses, malformed
frames and any server request stop the probe without a response.

Version has a 5 s deadline; writes/matching responses 10 s; final drain 1 s.
Shutdown closes input, drains, then signals only its unreaped owned child,
escalates if necessary and reaps. Kernel-blocked processes do not gain an
impossible wall-clock guarantee. No other process is attached or cleaned up.

Compatibility concerns only the exercised handshake/catalogue. A successful
handshake with optional catalogue failure is degraded; a framing/correlation
contradiction is incompatible; pre-handshake unavailability/timeout is unknown.
Do not classify authentication from English error text or a generic code.
Normal tests do not discover or start the real provider, and old model counts,
PIDs and live-probe run diaries are not current installed-state evidence.

Managed execution always requires the independent authority and its own profile
gates. The codec, static fixtures, standalone probe and Native CLI are not such
an authority. See [current decisions](../DECISIONS.md) and
[Managed parity](managed-runtime.md).
