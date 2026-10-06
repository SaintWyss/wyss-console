# ADR 0009 — Codex adapter protocol boundary

Status: Accepted
Date: 2026-10-06

## Decision recorded before implementation

Keep the Codex wire codec, vendor identifiers and protocol normalization in Rust,
independent of Tauri. No raw provider JSON goes to Svelte. Reuse Sprint 2A session,
model and safe error DTOs; test their projected outputs with both Rust and TypeScript.
Codex-specific observations remain internal Rust values until a UI consumer needs
an additional common projection. No second EventEnvelope or event runtime is added.

Implement only initialization, thread start/resume, text turn start, turn interrupt,
model list, selected lifecycle/content notifications and distinct approval requests.
The offline codec does not launch, connect, authenticate, probe or execute work.
Compatibility of an installed runtime remains unknown.

Codex omits the `jsonrpc` header. Envelope direction is explicit: structure alone
cannot distinguish client requests from server requests. Request IDs retain string
versus signed i64 identity; raw wire numbers never pass through JavaScript. Thread,
turn and item IDs are bounded opaque vendor strings, never Wyss UUID identities.
A caller supplies any future ManagedSessionId mapping; the codec allocates none.

Initialization requires initialize -> successful matching response -> initialized.
Other outbound operations are rejected before that sequence completes. The helper
tracks this ordering only, not a live connection or pending-request registry.

Unknown notifications and requests retain bounded vendor data in Rust, without
inferred semantics. Unknown server requests never produce automatic responses.
Uncorrelated responses retain their envelope until a caller supplies operation
context. Known experimental methods/items/fields are tagged separately, with
experimental outbound initialization disabled by default. Unknown is not stable.
Readable reasoning summaries and plans are distinct from agent messages; private
reasoning content is excluded from normalization.

Approval requests are runtime requests, not Wyss authorization. Command, file-change
and permission requests remain distinct. Response encoders are offline only and
accept verified vendor decision shapes. No permission engine or approval UI exists.
Capability is not authority; observed does not mean enforced. Model catalog entries
use unknown availability and unknown provider unless explicitly supplied by runtime
metadata; listing is not entitlement. Provider errors become fixed safe diagnostics,
without copying raw message/data into common errors.

## Source and implementation scope

The source is official generated schemas and protocol Rust source pinned to
`openai/codex@0b863c69f50335acd92164aab971cb58d298c2fe`, retrieved 2026-10-06.
See the runtime research note for URLs, example discrepancies and stability notes.
Synthetic fixtures describe this source revision, not captured or installed traffic.

Deferred: transport, runtime probes, generic adapter trait, full vendor schema,
agent execution, authority, EventEnvelope producers, persistence and agent UI.

## Executable boundary and limits

`WireMessage::parse` rejects duplicate JSON members, trailing second values,
ambiguous envelope combinations and unknown envelope fields (including a `jsonrpc`
header). It preserves absent versus null params and optional request trace. Vendor
IDs remain opaque strings rather than assuming UUID generation policy. String IDs
must be nonblank/control-free, at most 256 UTF-8 bytes. Request IDs retain signed
integers, including negative values and both i64 extrema; empty/control-bearing
string IDs reject. No wire JSON is parsed by JavaScript.

Limits are adapter restrictions, not claims about Codex's maximum capacity:
262,144 bytes per complete frame; depth 16 with root at zero; 8,192 value nodes;
256 members/elements per object/array; 256-byte keys; 65,536 bytes per content string;
128-byte ASCII method names (letters, digits, slash, dot, underscore, hyphen);
4,096-byte vendor error messages. Rejected oversize content is never truncated into
a fabricated valid event. There is no aggregation, queue, transcript or persistence.

Known payload validation checks correlation identities, required core fields, useful
item shapes, turn statuses, decision subtypes and catalog structure. It is deliberately
not a complete clone of every nested Codex schema: unconsumed vendor structures are
bounded and remain private to this integration boundary. Outbound turn input supports
text only. Unknown item details remain in the original bounded wire message; their
projection retains only type and identity. Unknown notifications/server requests retain
bounded params in Rust. There is no automatic response path.

Response payloads need caller-supplied request ID and operation context; generic
valid response envelopes can be retained before that context is known. There is no
pending-request map. A turn/start response has no thread ID, so future correlation
must retain the original request's thread mapping. Optional `SessionBinding` carries
caller-owned ManagedSessionId/runtime-instance/thread identities. A mismatching
thread rejects; absence never allocates or guesses a Wyss identity. Thread start/
resume observations project reusable thread state; successful turn completion maps
to idle, failed/interrupted remain distinct. Unknown thread states/flags map to
unknown, with original vendor state available in the wire object.

Internal `CodexObservation` values are not a second shared event envelope and have
no whole-value serialization implementation. Existing common session, model and
safe error DTOs are the only shared projections needed now, verified by the same
expected fixture outputs in Rust and TypeScript. Future UI must introduce a deliberate
common projection for additional item/content/approval detail; raw wire JSON and
vendor-only parameters must not cross that boundary.

Explicit user-visible reasoning summaries are projected; raw reasoning text/content
is suppressed. Plan items/deltas, process output, dynamic tool calls and selected
experimental fields are distinguishable from stable coverage. The ordering helper
gates known experimental outbound fields unless initialization explicitly opted in;
this is protocol negotiation, not authorization. It is not an exhaustive upstream
feature-gating implementation. No experimental capability is advertised as confirmed.

Approval encoders preserve original request identity and subtype. Command/file
string decisions and command-specific policy amendment shapes are codec-only.
Permission response encoding is deliberately limited to an empty grant. Additional
permission grants, full nested permission validation and live approval lifecycles
remain deferred. Request-resolution observations retain thread and request IDs.

Catalog model projections always use unknown availability. Catalog provider remains
unknown; explicit thread modelProvider is used only if it fits ModelProviderId,
otherwise stays unknown. Hidden/default/effort/modalities stay Codex metadata.
Projection beyond common DTO limits fails explicitly rather than silently coercing.
Raw provider error codes/messages/data remain vendor-only; safe errors use fixed
summaries, never English-message matching. Contract validation cannot detect secrets
inside arbitrary text; future consumers must still minimize and redact content.

The static descriptor fixture describes conditional translation coverage only, with
static declared evidence, null runtime/protocol versions and unknown compatibility.
No installed-runtime support or entitlement is asserted.

Implementation status: offline codec, normalization, ordering guard and synthetic
fixture tests. Terminal behavior, IPC, capabilities and EventEnvelope v1 are unchanged.

## Sprint 2C schema-completeness correction — 2026-10-06

The original Sprint 2B codec omitted an official server-notification envelope field,
`emittedAtMs`. It was already present at the pinned revision
`0b863c69f50335acd92164aab971cb58d298c2fe`; this was a codec omission, not evidence
that Codex 0.159.0 introduced incompatible metadata. The current revision
`0b47040dc9eb08d9c37d51e0824870ce5b27101b` retains the same contract.

Official `common.rs` defines `ServerNotificationEnvelope` with a flattened notification
and `emitted_at_ms: Option<i64>`, camelCase serialization and backward-compatible
absence. The generated TypeScript envelope exposes optional `emittedAtMs`; the
app-server producer populates it. See the research note for pinned/current sources.

Only server-to-client notifications now accept this exact optional field. Non-null
values must be signed i64 integers, including both extrema; absent/null mean unknown.
The vendor wire object preserves its original representation on round-trip. It is
provider emission-time metadata, not a Wyss timestamp, stream sequence, causal order,
request identity or authority evidence. Normalized observations and EventEnvelope v1
are unchanged. Client notifications, requests and responses gain no additional fields;
all unverified top-level keys still reject. The architectural decision stays Accepted.
