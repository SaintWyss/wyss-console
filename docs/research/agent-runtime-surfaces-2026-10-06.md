# Agent runtime surfaces — 2026-10-06

Retrieved: 2026-10-06. Primary official documentation only. This is documentation
research, not a local installation inventory, integration test or compatibility
certification. No runtime was launched and no authentication material was read.

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

Uncertain: installed CLI version, exact schema/feature gates, account access,
coverage of every tool/subagent and the experimental surface's future stability.
No optimistic compatibility range or local capability claim is inferred.

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
