# Agent runtime surfaces — 2026-10-06

Retrieved: 2026-10-06. Primary official documentation only. This is documentation
research. The Sprint 2C subsection separately records a read-only installed-runtime
probe; no authentication material was read by Wyss.

## Codex

[Official App Server documentation](https://developers.openai.com/codex/app-server)
(currently redirects to ChatGPT Learn) documents bidirectional JSON-RPC, default
JSONL stdio, initialization, thread creation/resume, turns/items, streamed output,
model catalog and approval requests/responses. Turn interruption ends current work;
it does not necessarily close a reusable thread. Schemas can be generated for a
specific installed CLI version. Experimental methods/fields require opt-in;
WebSocket is explicitly experimental/unsupported for production in this source.
The page also lists Unix-socket transport. Sprint 2A models the stdio integration
candidate; it does not implement every published transport.

[OpenAI token-sharing App Server guidance](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server)
warns that a model catalog is not itself an entitlement check. Hence catalog
visibility and effective selection/access remain separate contract concepts.

Uncertain: configured feature gates, account access, coverage of every tool/subagent
and the experimental surface's future stability. Installed version/probe facts are
recorded separately below.
No optimistic compatibility range or local capability claim is inferred.

### Sprint 2B: generated protocol verification

Retrieved again: 2026-10-06. Wire source pinned to official
[openai/codex revision 0b863c69](https://github.com/openai/codex/tree/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol).
This is source inspection, not a locally installed release or compatibility claim.

- [rpc.rs](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/src/rpc.rs): no `jsonrpc` header; request IDs are string or signed i64; requests may carry optional W3C `trace`. Direction must distinguish client/server requests. Default documented stdio is newline-delimited JSON; the offline codec accepts one complete JSON value and implements no reader or transport.
- [ClientRequest schema](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/schema/json/ClientRequest.json): initialize/clientInfo, thread start/resume, turn start/interrupt and model/list. Initialization response is in [v1/InitializeResponse](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/schema/json/v1/InitializeResponse.json); later operations use v2 response schemas. A v2 payload is not the JSON-RPC header/version and does not require experimental opt-in by itself.
- [ServerNotification schema](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/schema/json/ServerNotification.json): separate thread, turn and item identities; final turn status is completed/failed/interrupted. Stable deltas include agent messages, command output and explicit reasoning summaries. Raw reasoning text is excluded from Wyss normalization. Plan updates are distinct from experimental plan items/deltas.
- [ServerRequest schema](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/schema/json/ServerRequest.json): separate command, file-change and permission approval requests. Command decisions are accept, acceptForSession, decline, cancel, or verified structured execpolicy/network-policy amendments. File changes support the four string decisions. Permission responses use a granted permission profile, not a command decision; 2B encodes only the verified empty-grant subset. Resolution notifications correlate the original request ID.
- [ModelListResponse schema](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/schema/json/v2/ModelListResponse.json): catalog id/model, displayName, hidden/isDefault and reasoning-effort metadata. Reasoning effort is now nonempty opaque text, not a closed hardcoded enum. The catalog does not provide a provider field or establish entitlement; use unknown provider/access. A thread response can explicitly report a different modelProvider.

Example/schema differences: pinned schemas require `startedAtMs` on approval and
item-start notifications, `completedAtMs` on item completion, and additional Thread
metadata that abbreviated documentation examples omit. The current command approval
schema has no `availableDecisions` field. Use verified response decision shapes,
not an assumed list copied from examples. `grantRoot` is explicitly unstable.
These are wire-shape refinements, with no conflict with the frozen Wyss architecture.

[Protocol registration](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/src/protocol/common.rs)
and field annotations in
[thread.rs](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/src/protocol/v2/thread.rs)
and [turn.rs](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/src/protocol/v2/turn.rs)
identify experimental methods/fields. The codec tags selected known experimental
surfaces and preserves unknown ones without claiming stable support. Outbound typed
operations stay minimal; initialization opts out of experimental API in normal fixtures.
No complete experimental method registry is claimed. Authentication APIs are excluded.

The 2B corpus contains synthetic schema-derived shapes, deliberately malformed cases
and explicitly labeled forward-compatibility examples. No fixture is captured traffic.
Fifty-seven stable examples were additionally checked against the pinned generated schemas
using the already installed Ajv dependency; integer precision/range is verified in Rust,
not JavaScript schema validation (i64 extrema fixtures are excluded from that check). Generated schemas are not vendored into the repository.
Offline fixtures do not establish installed version, handshake or compatibility;
see the separate Sprint 2C observations below.

### Sprint 2C: current schema and installed-runtime probe

Retrieved: 2026-10-06. Current official source inspected at
[0b47040d](https://github.com/openai/codex/tree/0b47040dc9eb08d9c37d51e0824870ce5b27101b/codex-rs/app-server-protocol).
[v1 initialization](https://github.com/openai/codex/blob/0b47040dc9eb08d9c37d51e0824870ce5b27101b/codex-rs/app-server-protocol/src/protocol/v1.rs)
retains clientInfo and optional capabilities with experimental API false by default.
Gateway OAuth, attestation, MCP extensions and notification suppression are optional
and not enabled. The [model/list parameter schema](https://github.com/openai/codex/blob/0b47040dc9eb08d9c37d51e0824870ce5b27101b/codex-rs/app-server-protocol/schema/json/v2/ModelListParams.json)
provides optional cursor, uint32 limit and includeHidden. The official App Server
page still documents default newline-delimited JSON over stdio. No frozen wire or
normalized semantics were changed to match an installed version.

Installed executable: canonical standalone Codex binary, version `codex-cli 0.159.0`.
The [official release RPC source](https://github.com/openai/codex/blob/rust-v0.159.0/codex-rs/app-server-protocol/src/rpc.rs)
and [release initialize source](https://github.com/openai/codex/blob/rust-v0.159.0/codex-rs/app-server-protocol/src/protocol/v1.rs)
were also inspected. Source differences are informational, not compatibility proof.

The original Sprint 2B codec omitted official notification-envelope metadata.
A bounded live diagnostic identified the rejected field as `emittedAtMs` on
`remoteControl/status/changed`, with top-level keys `emittedAtMs`, `method`, `params`.
This field already existed in the pinned Sprint 2B source; it is not new protocol
drift in Codex 0.159.0. Verified official sources:

- [Pinned Rust envelope](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/src/protocol/common.rs):
  `ServerNotificationEnvelope` flattens the notification and declares optional
  `emitted_at_ms: Option<i64>`, camelCase, absent-compatible decoding.
- [Current Rust envelope](https://github.com/openai/codex/blob/0b47040dc9eb08d9c37d51e0824870ce5b27101b/codex-rs/app-server-protocol/src/protocol/common.rs)
  retains that signed i64 contract. Explicit null decodes to absence; non-null
  integers may span the full signed i64 range.
- [Pinned generated TypeScript envelope](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server-protocol/schema/typescript/ServerNotificationEnvelope.ts)
  and [current generated envelope](https://github.com/openai/codex/blob/0b47040dc9eb08d9c37d51e0824870ce5b27101b/codex-rs/app-server-protocol/schema/typescript/ServerNotificationEnvelope.ts)
  expose optional `emittedAtMs`, documented as Unix emission time in milliseconds.
- [Pinned producer](https://github.com/openai/codex/blob/0b863c69f50335acd92164aab971cb58d298c2fe/codex-rs/app-server/src/outgoing_message.rs)
  and [current producer](https://github.com/openai/codex/blob/0b47040dc9eb08d9c37d51e0824870ce5b27101b/codex-rs/app-server/src/outgoing_message.rs)
  populate the field when emitting app-server notifications.

The codec now accepts exactly that optional server-notification field, validates
its upstream type/range and preserves it only as vendor metadata. Other unknown
envelope keys and placement on requests/responses/client notifications still reject.
It does not change normalized observations, EventEnvelope v1 or canonical ordering.
Focused fixtures are synthetic/schema-derived, not retained live frames.

Corrected live result: fixed app-server spawn, correlated initialize success,
initialized sent, experimental false; model/list limit 50/includeHidden false
returned seven models with no next cursor. `remoteControl/status/changed` and
`account/updated` each arrived once with the legal three-key envelope and emission
metadata present. Only key/method/presence summaries were retained, not raw params
or timestamp values. Compatibility is compatible for this exercised surface.
Model provider and availability remain unknown; listing does not prove entitlement
or describe all possible configured providers. No unexercised capability is confirmed.

Owned child PID 213482 exited by EOF, status 0, and was reaped without escalation.
Git-content snapshots matched before/after; four pre-existing Codex PIDs remained
unchanged. No workspace work, authentication inspection, configuration override,
thread, turn, prompt, tool operation or approval response was performed by Wyss.
These are local observations, not a guarantee about every runtime/configuration.
See ADR 0010 for limits and shutdown.

## Claude Code / Agent SDK

[Official Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview)
distinguishes the agent harness from the direct model Client SDK. Python and
TypeScript SDKs operate Claude Code with tools, hooks, sessions, subagents,
skills/plugins and permissions. An SDK embedding is not proof that execution is
in the same process: this SDK operates the Claude Code binary.

[Programmatic CLI operation](https://code.claude.com/docs/en/headless) documents
`-p`, JSON results and streaming JSON, including session/result metadata.
[Session documentation](https://code.claude.com/docs/en/agent-sdk/sessions) uses
`query()` with explicit resume/fork options. A completed query can leave a resumable
conversation; it must not automatically map to closed.

[Permissions](https://code.claude.com/docs/en/agent-sdk/permissions) are evaluated
through runtime modes, rules, hooks and callbacks. Callback visibility depends on
configuration and does not prove all actions pass through Wyss. Represent coverage
per capability; approval response is a request, not external authority.

Important current difference from earlier assumptions:
[TypeScript V2 session API documentation](https://code.claude.com/docs/en/agent-sdk/typescript-v2-preview)
now labels that API removed and directs integrations to `query()`. Do not encode
`unstable_v2_*` APIs into normalized contracts.

Uncertain: installed SDK/CLI pairing, enabled configuration, exact callback and
background-work coverage, authentication/product eligibility for a concrete
integration. No integration or credential decision is made in 2A.

## OpenCode

[Public server documentation](https://opencode.ai/docs/server/) describes a headless
HTTP server, OpenAPI 3.1, health/version, sessions/status/children/abort, messages,
providers, agents and SSE. Some tools endpoints are explicitly experimental.
[Public SDK documentation](https://opencode.ai/docs/sdk/) describes
`@opencode-ai/sdk`, a generated JS/TS HTTP client; client creation and combined
server/client startup are different operations.

The separately published [v2 embedded SDK](https://opencode.ai/v2/docs/build/sdk)
describes `@opencode/sdk`, an owned embedded host using an in-memory HTTP router
and asynchronous event streams, distinct from the network client.
[v2 HTTP API documentation](https://opencode.ai/v2/docs/api) explicitly labels its
selected-instance HTTP surface experimental. These package/API families are not
interchangeable; public documentation does not certify a stable release matrix.

Uncertain: which release/package/API family should be pinned for a real adapter,
installed version, complete operation/event compatibility and experimental route
stability. Probe a selected release later. Never infer structured telemetry from
OpenCode's terminal text or call every documented v2 API stable.

## Consequences for Sprint 2A

Use vendor-neutral semantic capabilities with independent observe/request support,
conditions and provenance. Preserve runtime/provider/transport separation and opaque
vendor IDs/state. Model version/probe evidence instead of hardcoded vendor matrices.
Fixtures are synthetic contract examples, including synthetic confirmed probes;
they are not facts about installed runtimes. Contract validation establishes shape
and consistency, not trust, complete telemetry, enforcement or authorization.

No new network/process mechanism is required. ADR 0002's external-authority
milestone still precedes actual managed-agent execution.
