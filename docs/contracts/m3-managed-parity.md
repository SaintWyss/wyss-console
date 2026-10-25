# M3 — Managed provider parity contract

**Frozen:** 2026-10-23 · **Base:** `def69fe35ddfe8b696b65a12d2f2babe296c8247` (`origin/main`)
· **Status:** engineering contract; not a release approval.

This contract translates the existing M3 roadmap and functional requirements into
acceptance oracles. It does not accept ADR0053, alter an accepted authority protocol,
increase F8's runtime capacity, or authorize real-provider turns. Current source and
reproducible evidence override older status prose. PR #22 is a separate, unmerged M2
dependency; this branch is based on `main` and carries no M2 changes.

## Status vocabulary

- **PASS** means the cited runnable test and stored output prove only the named fixture
  or source-level property.
- **NOT_RUN** means the procedure has not been executed.
- **NOT_PROVEN** means available observations do not establish the property.
- **BLOCKED** means the required safety or owner decision prevents qualification.
- Fixture evidence never implies provider, account, subscription, installed-build, or
  release acceptance.

## Oracle index

| Oracle                                          | Existing baseline                                                               | Required outcome                                                                                      | Primary ownership                                                                                                         | Initial disposition                                    |
| ----------------------------------------------- | ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| M3-01 Claude interface, version and feasibility | No Claude Managed adapter; ADR0053 Proposed                                     | Exact official SDK/CLI pair, supported auth/settings route and complete effect inventory              | `docs/adr/0053-*`, `docs/research/claude-managed-feasibility-2026-10-23.md`; `f8/production.rs`, `f8/provider_adapter.rs` | BLOCKED pending evidence and Santiago's ADR acceptance |
| M3-02 Claude Inference                          | Codex-only F8 profiles; no Claude no-tool attestation                           | Claude text turns only, with no executable effect or hidden inherited tool                            | `f8/launcher.rs`, new Claude adapter, `f8/acceptance.rs`                                                                  | BLOCKED pending no-effect and auth evidence            |
| M3-03 Claude Local Tools                        | No Claude effect adapter                                                        | Every complete exact action is held by Wyss authority until current approval and at most one dispatch | `f8/authority.rs`, new Claude adapter, `f8/acceptance.rs`                                                                 | BLOCKED pending independent mediation proof            |
| M3-04 Provider-neutral lifecycle                | F8 lifecycle and GUI exist for Codex                                            | Claude/Codex share visible lifecycle states while retaining provider-specific wire/event contracts    | `f8/{protocol,engine,provider_adapter,gui}`, `src/lib/features/f8/`                                                       | Codex baseline present; parity NOT_PROVEN              |
| M3-05 Identity, leases, recovery and capacity   | F8 identity/generation/lease/receipt invariants; global cap 1                   | Exact runtime/session/generation fences and truthful one-shot recovery; cap remains 1                 | `f8/{authority,operations,engine,registry,allocator}.rs`                                                                  | Codex fixtures exist; Claude NOT_PROVEN                |
| M3-06 Managed model/effort extension            | Current P2 protocol has no caller-controlled selection; ADR0014/0015 exclude it | Separate typed, versioned, generation-bound request, live-disabled until new ADR/protocol acceptance  | Proposed ADR0054; `f8/protocol.rs`, contracts and GUI                                                                     | PROPOSED; no live selection                            |
| M3-07 Provider usage and limits                 | No F8 provider-usage projection                                                 | Show only source-backed provider observations; missing values remain unknown                          | Provider adapters, F8 DTOs and Models/History UI                                                                          | NOT_PROVEN                                             |
| M3-08 Metrics history                           | No separate M3 metrics-history store                                            | Optional local history, default-off, bounded and isolated from audit/recovery                         | Proposed `docs/contracts/m3-model-usage-history.md`; new store and UI                                                     | Owner policy not accepted; persistence disabled        |
| M3-09 GUI lifecycle and provenance              | F8 views currently project Codex                                                | Provider/mode/profile, authority, lease, requested/actual state, usage and recovery are truthful      | `src/lib/features/f8/`, Models, Sessions, Projects                                                                        | Claude integration NOT_IMPLEMENTED                     |
| M3-10 Adversarial security and recovery         | F8 has Codex contract/authority/process fixtures                                | Deny unknown, malformed, stale, cross-scope, bypass and ambiguous actions without replay              | `src-tauri/src/f8/*tests.rs`, `tests/`                                                                                    | Existing Codex evidence is not Claude evidence         |
| M3-11 Performance and resource qualification    | F8 cap 1; partial project-level measurements                                    | Comparable bounded startup, first event, stream/backpressure, cancel, recovery and idle samples       | F8 benches/acceptance, fixture harness                                                                                    | NOT_RUN for M3 comparison                              |
| M3-12 Compatibility and release/rollback        | Codex runtime pinned; Claude absent; M2 PR #22 open                             | Accepted ADRs, provider evidence, review, recovery and explicit rollback; no implicit Native fallback | Compatibility matrix, release record, PR                                                                                  | BLOCKED                                                |

### Disposition after the implementation pass (2026-10-23)

The "Initial disposition" column above is the starting point. The result of the pass, with the exact
status words and the evidence, is in the [acceptance record](../acceptance/m3-engineering-2026-10-23.md):
M3-04 `IMPLEMENTED_AND_TESTED`; M3-01, 02, 03, 07, 09, 10 `IMPLEMENTED_BUT_UNQUALIFIED`; M3-05, 06, 08, 12
`OWNER_APPROVAL_REQUIRED`; M3-11 `PROVIDER_EVIDENCE_REQUIRED` (Codex arm). The V8 store cannot hold a
non-Codex runtime: [ADR0055](../adr/0055-multi-provider-managed-runtime-store.md).

## M3-01 — Claude SDK, protocol, auth and capability discovery

**Requirements:** FP-SES-02/03, FP-CFG-05, FP-SEC-01/03, FR-MAN-01..09, FR-ADP-01..05.
**Baseline:** [ADR0053](../adr/0053-claude-code-managed-integration-boundary.md) is Proposed;
`f8/production.rs` and `f8/provider_adapter.rs` are Codex-specific. Official SDK docs
describe a local process wrapping Claude Code and expose tools, hooks, MCP, plugins,
skills and subagents; exact suitability is not established by documentation alone.

- **Positive:** pin the official distribution, SDK version, bundled CLI version, protocol
  schema, settings/auth path, supported capabilities and freshness; the installed adapter
  reports the exact observed build and refuses unknown builds before launch.
- **Negative:** unknown version, undocumented event/tool, malformed frame, incomplete
  capability inventory, unsupported auth, missing credential refresh semantics or hidden
  inherited effect produces `unverified`/`blocked` and zero launch/turn effect.
- **Unit/integration:** version and schema fixtures; exhaustive typed event parser; config
  source precedence fixture; fake process startup/error/exit/reap. No terminal scraping.
- **GUI:** provider card shows profile-specific `Available`, `Unverified`, `Blocked` or
  `Unsupported`; Launch is disabled unless that exact build/profile has accepted evidence.
- **Real provider:** read-only version/help checks may be recorded; turns, sign-in, refresh,
  account entitlement and installed-build workflow require separate explicit authorization.
- **Failure/recovery:** failed or changed handshake invalidates the generation capability
  record; it does not fall back to Native/Codex or replay a turn.
- **Resources:** process, descriptors, bounded startup deadline and event/frame/queue limits
  are recorded for each pinned build; unknown resource behavior blocks availability.
- **Owner decision:** Santiago accepts ADR0053 only after the evidence report and residual
  risks are reviewable. Supported authentication and real-provider test consent remain
  explicit owner choices.

## M3-02 — Claude Managed Inference (no tools)

**Requirements:** FP-SES-02/03, FP-CFG-05, FP-SEC-01/03, FR-MAN-01..06.
**Baseline:** no Claude adapter. The SDK's documented all-tools deny option makes a
text-only profile a candidate; settings, hooks, MCP, plugins, managed policy, auto-memory,
connectors and credential effects are not thereby proven absent.

- **Positive:** an authority-owned per-session runtime emits bounded text/usage events;
  complete effective tool inventory is empty; no provider-originated command, file,
  network-tool, hook, plugin, MCP or subagent effect is possible outside the model API.
- **Negative:** any inherited or generated tool, hook invocation, managed policy conflict,
  plugin/MCP server, non-text content, oversized frame, unknown event or auth ambiguity
  refuses the profile or fails the turn without effect.
- **Unit/integration:** hidden-tool and callback-bypass fixtures; project/user/local/system
  and managed settings precedence matrix; no-tool proof per exact CLI build; cancellation,
  process reap and restart with no prompt replay.
- **GUI:** mode is Inference; no tool/approval UI is implied; unsupported effects and stale
  attestation explain why Start is unavailable.
- **Real provider:** one authorized low-cost turn and cancellation/recovery/refresh matrix is
  required before release; none is implied by fixture output.
- **Failure/recovery:** after ambiguous acceptance, mark turn indeterminate; preserve receipt;
  never retry the prompt automatically. Restart is explicit and generation-fenced.
- **Resources:** fixed frame, total output, queue, wall-time and process bounds; report
  startup/ready/first-text and RSS/PSS/CPU under fixtures and later comparable real tests.
- **Owner decision:** ADR0053 and authentication route; separate authorization for the
  real turn. If no-effect attestation fails, leave Inference unavailable.

## M3-03 — Claude Managed Local Tools

**Requirements:** FP-SES-02/03, FP-CFG-05, FP-SEC-01/02/03, FR-MAN-01..09, FR-SEC-01..07.
**Baseline:** no Claude adapter. A permission callback is insufficient because documented
permission rules, modes, hooks and configured tools can decide calls before/without that
callback. Accepted ADR0016/0018 residual risks remain in force.

- **Positive:** only authority-owned effect tools are offered. Each invocation carries
  provider/build, session, runtime instance/generation, turn, project/workspace, exact
  complete bounded action, fingerprint, current revision and lease scope. Authority holds
  it for review and one immutable approval receipt admits at most one dispatch.
- **Negative:** built-in tools, callback bypass, auto-approval, successor actions, malformed
  or truncated command/diff, hooks, plugin, MCP or subagent effects, substitution, replay,
  stale generation/lease, cross-project target or duplicate answer produce zero dispatch.
- **Unit/integration:** adversarial fixtures for every bypass path, reordered/duplicate
  answers, race/revoke/deny/cancel, crash before/after dispatch and external side effect.
  Use fake authority tool; do not execute real provider actions.
- **GUI:** exact full action and scope are visible; Accept is disabled on truncation/stale
  state; deny/cancel remains available without a new execution grant.
- **Real provider:** one approved harmless effect and a negative/bypass matrix require
  Santiago's separate authorization and independent review; fixtures do not qualify it.
- **Failure/recovery:** uncertain dispatch becomes indeterminate and is never replayed;
  pending approvals invalidate on process/generation loss. Cancellation does not grant
  additional execution rights.
- **Resources:** one pending approval globally where current authority requires; bounded
  approval count, queue, action size and dispatch duration; cap remains 1 runtime.
- **Owner decision:** ADR0053 acceptance and explicit acceptance of the residual-risk
  statement. Any unmediated effect keeps this profile BLOCKED.

## M3-04 — Provider-neutral Managed lifecycle

**Requirements:** FP-SES-02/03, FP-CTX-04, FP-SEC-03, FR-MAN-01..09.
**Baseline:** F8 runtime/session/turn lifecycle, provider adapter and GUI panels exist for
Codex; event schemas and approval semantics are provider-specific.

- **Positive:** prepare, start, inspect, lease, bind/resume, turn, stream, approval,
  cancel, stop, close, reattach and recover work through a shared user-visible state
  vocabulary while each provider uses its own typed adapter and strict classifier.
- **Negative:** unknown Claude events do not relax Codex parsing; provider ID mismatch,
  fallback to another provider, implicit lease, reconnect replay or UI-only admission
  refuses before effect.
- **Unit/integration:** provider adapter contract suite; unknown/malformed/cross-provider
  protocol corpora; Codex existing `f8` tests and Claude-specific independent fixtures.
- **GUI:** each operation targets captured host/project/provider/mode/session context; late
  responses cannot redirect results to another project or session.
- **Real provider:** per-provider workflow and cancellation evidence required for each
  released profile; no cross-provider inference is allowed.
- **Failure/recovery:** preserve separate provider failure states and immutable identities;
  no automatic resume, restart, lease grant or prompt replay.
- **Resources:** reuse F8 bounds; no new queue or runtime pool; global active-runtime cap
  remains one.
- **Owner decision:** protocol changes that widen accepted authority APIs require a separate
  accepted ADR; this work cannot retire P1 or alter P2 behavior.

## M3-05 — Identity, leases, idempotency, recovery and capacity

**Requirements:** FP-SES-02/03/04, FP-SEC-01/02/03, FR-MAN-03..06.
**Baseline:** F8 has authority/session/runtime/generation identities, leases, receipts,
replay handling and a global capacity of one; those Codex fixtures do not establish Claude.

- **Positive:** every runtime, binding, turn, catalogue, usage event and approval binds the
  exact authority instance, session, runtime instance, generation and provider. The authority
  revalidates lease/revision/workspace immediately before dispatch.
- **Negative:** stale or absent fence, wrong project/session/generation, expired lease,
  capacity overflow or reused conflicting intent refuses before allocation/dispatch.
- **Unit/integration:** crash points across prepare/start/bind/admit/dispatch/confirm;
  idempotency receipt, disk-full/corrupt-store and outside-writer tests; process cleanup
  signals only owned unreaped children.
- **GUI:** requested versus observed state, active lease, pending/indeterminate action and
  recovery option are distinct. Reattachment only observes.
- **Real provider:** restart/resume compatibility is tested only with explicit approved
  test state; provider-owned conversation IDs never replace authority identities.
- **Failure/recovery:** ambiguous accepted effect is indeterminate; no prompt/action replay;
  recovery has a durable receipt and exact successor generation.
- **Resources:** preserve global `MAX_ACTIVE_RUNTIMES = 1`; a stopping/unreaped runtime
  retains its slot; queues and histories stay bounded.
- **Owner decision:** any capacity increase belongs to later qualified work and explicit
  approval; M3 cannot increase concurrency.

## M3-06 — Versioned Managed model and effort authority

**Requirements:** FP-MOD-01, FP-SES-02/03, FP-CFG-05, FP-SEC-01/03, FR-MOD-01..07/10.
**Baseline:** current P2 protocol exposes a provider model catalogue and observed binding
metadata but has no caller-controlled Managed model/effort selection. ADR0014/0015 exclude
selection today; ADR0031 is not an accepted protocol extension.

- **Positive:** an independently versioned typed request identifies model/effort and scope;
  authority validates provider, fresh catalogue revision, capability source, generation,
  entitlement-known state and selected values at admission and immediately before dispatch.
- **Negative:** offered is not entitlement; stale/unknown/retired IDs, unsupported effort,
  unknown authority version, provider mismatch or changed runtime generation refuses; no
  automatic model substitution.
- **Unit/integration:** request/response compatibility and strict unknown-field fixtures;
  stale catalogue race, reroute, restart, cancellation and old-authority tests. Fields remain
  unavailable to production callers while the extension is unapproved.
- **GUI:** separately display Offered, Configured, Requested and Reported-in-use with source,
  freshness, scope and generation. Disable live controls until acceptance.
- **Real provider:** only provider-reported selection and reroute behavior can establish
  honored selection; a catalogue/list is not proof of account availability.
- **Failure/recovery:** cancellation/retry is explicit; no silent fallback. A reroute is
  attached only to its originating turn; recovery never reuses stale catalogues.
- **Resources:** bounded catalogue and event records; no repeated unbounded discovery; keep
  the current runtime cap.
- **Owner decision:** Santiago accepts the Proposed model/effort ADR and authority extension
  before any live request/GUI control is enabled.

## M3-07 — Provider-reported usage, limits and freshness

**Requirements:** FP-MOD-02, FP-SEC-03, FP-NFR-01, FR-MOD-08/09.
**Baseline:** current F8 has no provider usage projection; Native OS process resource samples
are not provider token usage. The Claude SDK documents per-step/per-model provider usage,
but this is not real account/billing authority.

- **Positive:** preserve provider/source/schema/time/scope and distinguish input, output,
  cached input, provider-reported reasoning, turn/session grain, units, limit/reset/freshness
  and completeness. Unknown remains unknown.
- **Negative:** missing/delayed/duplicate/reordered events, mixed units, interrupted turn,
  API outage and SDK cost estimate never become zero usage, subscription entitlement or
  authoritative billing/hard cap.
- **Unit/integration:** typed parser corpus, deduplication by origin event, partial result and
  provider-specific unit fixtures; no inference from prompts or text.
- **GUI:** label source, age, completeness and units; distinguish subscription allowance,
  API billing and provider estimates.
- **Real provider:** a bounded authorized turn is required to verify emitted fields and
  refresh/reset semantics; otherwise show `not reported`/`unknown`.
- **Failure/recovery:** preserve partial observation with an explicit completeness reason;
  no fabricated reconstruction after reconnect.
- **Resources:** bounded in-memory counters/events; no durable data without M3-08 policy.
- **Owner decision:** Santiago approves any interpretation that becomes user-visible billing
  or subscription-limit language.

## M3-08 — Optional metrics history and privacy store

**Requirements:** FP-MOD-02, FP-SEC-03, FP-NFR-01, FR-MOD-08/09.
**Baseline:** no M3 metrics-history store; audit and vault are protected independent stores.

- **Positive:** separate local store, opt-in, default off, with bounded metadata-only schema,
  deduplication, crash-consistent insert/prune/export/reset and disclosed retention/count cap.
- **Negative:** prompts, reasoning, secret values, arbitrary tool output, command/diff content,
  provider credential material, duplicate origin events or vault/recovery rows never enter
  this store. Full disk/corruption cannot alter control receipts or protected records.
- **Unit/integration:** schema/validation, caps, pruning, duplicate event, crash between write
  and index update, disk-full, corruption, export/reset and redaction marker corpus.
- **GUI:** setting is off by default; data scope/retention/export/reset are explicit. Until
  owner approval, production persistence and setting remain unavailable.
- **Real provider:** not needed for store mechanics; authorized provider events are required
  before actual history can be assessed.
- **Failure/recovery:** store errors degrade only history; never fail or replay Managed work.
- **Resources:** proposed maximum 30 days and 10,000 records, whichever is first; bounded
  pages/export and local storage accounting. These numbers remain a proposal until accepted.
- **Owner decision:** Santiago accepts separate privacy/schema/retention contract before
  enabling persistence. No data migration is authorized.

## M3-09 — GUI integration and context safety

**Requirements:** FP-CTX-04, FP-SES-02/03, FP-MOD-01/02, FP-SEC-02/03.
**Baseline:** F8 views include Runtimes, Models, History, Conversation and Approval panels
for Codex; Projects/Models/History surfaces exist independently.

- **Positive:** integrate both providers with Projects/Workspaces, Sessions, Managed turns,
  Models, Approvals, History/Usage and provider capabilities. Show provider/mode/profile,
  exact authority/build, desired/actual state, lease, selected/reported model/effort, usage,
  pending/indeterminate actions and recovery.
- **Negative:** unavailable/unverified profile, unapproved model extension, stale data,
  truncated action, disconnected authority or changed context disables the relevant write or
  launch action. No frontend command can bypass backend typed admission.
- **Unit/integration/GUI:** context-switch-during-request tests; Tauri desktop with isolated
  HOME/XDG, fixture authority/provider; cancel/start/stop/reconnect/approval/history scenarios.
- **Real provider:** visual parity may be fixture-verified; provider claims still require the
  M3-01/02/03/07 real evidence separately.
- **Failure/recovery:** reconnect restores observations only; never creates lease, binds,
  resumes, accepts approval or resubmits prompt.
- **Resources:** lazy/hidden views must not poll unboundedly; measure first paint, event
  projection, multi-window observer cost and subscription teardown.
- **Owner decision:** user-visible profile/usage claims need accepted source definitions;
  no unchecked feature button before qualification.

## M3-10 — Adversarial protocol, approval and recovery campaign

**Requirements:** FP-SES-02/03, FP-CFG-05, FP-SEC-01/02/03, FR-MAN-01..09, FR-ADP-01..05,
FR-SEC-01..07. Reuse the E1–E9 scope in the feasibility record; existing Codex E1–E8
observations are historical and do not qualify Claude.

- **Positive:** fixtures prove each supported profile's allowed effect; negative isolation
  is tested independently from positive workflow.
- **Negative:** unknown frames; malformed/oversized messages; hidden tools; callback bypass;
  hooks; MCP/plugins/subagents; account/project/managed policy precedence; auth/refresh error;
  cross-session/project IDs; wrong generation/lease; duplicate/reordered answers; crashes
  before/after dispatch; ambiguous provider acceptance; cancel/restart race; stale catalogue;
  reroute; unsupported effort; upgrade/downgrade; delayed usage; disk-full/store corruption;
  revocation under load; capacity refusal; owner disconnect/reattach.
- **Unit/integration:** every fixture is deterministic, names the invariant, asserts zero
  forbidden effect and records evidence. Use a fake provider and private sockets; never real
  credentials or paid inference in automated suites.
- **GUI:** denial, stale approval, partial stream, indeterminate action and owner disconnect
  render accurately; cancellation/close remain reachable.
- **Real provider:** only explicitly authorized, isolated tests count; live credentials,
  refresh and provider-side effects are separately enumerated.
- **Failure/recovery:** one durable intent/receipt per accepted operation; no replay after
  ambiguous external effect; retain enough evidence to reconcile safely.
- **Resources:** flood/slow-reader and revocation-under-load tests prove bounded queues and
  no sibling delay at cap 1.
- **Owner decision:** real tests and residual risks receive Santiago's approval; test fixture
  evidence cannot waive either.

## M3-11 — Performance and resource qualification

**Requirements:** FP-NFR-01, FP-SES-04, FP-MOD-02; architecture 15 measurement method.
**Baseline:** F8 project has some idle/storage samples and fixed bounds, but no comparable
M3 provider performance campaign.

- **Positive:** same machine, build, fixture workload and sampling protocol for Codex and
  each qualified Claude profile; report raw samples and p50/p95/p99 for startup/readiness,
  first streamed text, event processing, cancel and recovery plus idle resource cost.
- **Negative:** a failed/slow consumer, oversized event or second concurrent runtime must not
  cause unbounded memory, CPU, FD/process growth, queueing or admission.
- **Tests:** reproducible fixtures with monotonic timing; RSS/PSS, CPU/wakeups, descriptors,
  process count, authority overhead, multi-window observation, backpressure and current cap.
- **GUI:** record view responsiveness and polling/subscription teardown as separate timings.
- **Real provider:** actual latency/cost is NOT_RUN without explicit authorization; do not
  infer it from fixtures.
- **Failure/recovery:** cancellation/reap and restart samples include outcomes and no hidden
  retry. Keep slot reserved until the owned process is reaped.
- **Resource limit:** F8 capacity remains 1; no target justifies weakening existing bounds.
- **Owner decision:** release targets/acceptable deltas require evidence review before release.

## M3-12 — Compatibility, release acceptance and rollback

**Requirements:** FP-SES-01..04, FP-CFG-05, FP-MOD-01/02, FP-SEC-01..03, FP-NFR-01 and
all M3 roadmap acceptance criteria.

- **Positive:** pinned supported versions pass positive/negative fixture suites, independent
  review and authorized provider acceptance. Both Claude profiles and the existing Codex
  profile have separate evidence. Old authority negotiation is explicit.
- **Negative:** unsupported/upgraded/downgraded provider, missing authority feature, unknown
  DTO, stale capability or unapproved ADR disables only the new feature and preserves existing
  Codex histories/control operation; never reinterpret it as Native.
- **Tests:** Rust, frontend, Tauri GUI, boundary, recovery, compatibility, performance and
  rollback suites are listed in the acceptance record with exact commands/results.
- **Real provider:** each released Claude profile requires an authorized real-provider run;
  absent authorization remains `NOT_RUN`, never a pass.
- **Failure/recovery:** rollback disables new launches; retains existing Codex behavior,
  receipts and history; does not stop/restart active host/authorities or kill sessions.
- **Resources:** no migration, package install, host replacement or concurrency change in
  this PR; cost impact is stated per approved real test.
- **Owner decision:** ADR0053, ADR0054, metrics privacy contract and final release/adoption
  approval are independent gates for the features they govern. Do not merge/deploy from this
  engineering branch.

## Release and rollback criteria

Release requires PASS for every applicable oracle, accepted ADR0053 and ADR0054 where used,
accepted metrics privacy policy before history persistence, authorized real-provider evidence
for each released Claude profile, independent security review, exact provider/authority
compatibility, regression evidence and signed owner release decision. A fixture-only build is
not a provider release.

Rollback is a capability/launch gate for new sessions. It preserves current Codex P2 and P1
operation, old receipts and histories; it performs no automatic provider substitution, store
migration, service restart or session termination. Any unresolved external effect stays
indeterminate and requires explicit recovery review.
