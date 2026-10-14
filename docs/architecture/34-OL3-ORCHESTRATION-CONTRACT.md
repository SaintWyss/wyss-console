# 34 — OL3 supervised multi-agent orchestration contract

Status: **candidate final-product design, NOT accepted architecture**. Owner chose DN-15=OL3
and DN-08=A. Ties to [05](05-TERMINAL-AND-NATIVE-SESSIONS.md),
[09](09-PROFILES-AND-ORCHESTRATION.md),
[10](10-MANAGED-RUNTIME-AND-SECURITY.md),
[17](17-WYSS-AGENT-AND-HERMES.md),
[32](32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md) and
[33](33-CROSS-DOMAIN-RECONCILIATION.md).
No orchestrator code exists or was tested for this design.

## 1. Envelope and ownership

The coordinator is a **planner of finite workflows** and an observer of their execution,
not a new source of tool, filesystem or approval authority. It coordinates independent
agents; it cannot change the risk policy that controls a managed turn or a Wyss Agent action.

A pipeline envelope is an immutable, user-reviewed structure:
`pipeline_id`, immutable revision and digest; host/user identity; project root identity
and verified filesystem locator; declared purpose; ordered DAG of task definitions;
allowed agent/provider/profile/model selection per task; task limits and preconditions;
maximum total steps, concurrency, clock duration, available provider usage/cost limits
(only if reliably reported), retry allowance and a hard stop path; per-task
effect classification (proposal only, native/full-account, managed);
approval mode (a finite DAG approval for scheduling only; every native/full-account
step additionally needs per-launch native confirmation and an accepted trust boundary);
explicit expiration and revocation. Include the host and scope of effects in what
the user sees. **No default unlimited budget**; if a quantitative provider budget
cannot be enforced, do not present it as a hard monetary cap and offer an enforceable
proxy or ask the user to narrow the pipeline.

**BLOCKED:** An unrestricted native worker with the user's UID can reach
the same-UID control sockets (accepted ADR 0011); a typed
`native_confirmation` parameter or layer diagram cannot contain
that compromise. Any OL3 native/full-account automated execution
stops with `BLOCKED_TRUST_BOUNDARY` until an OS-level separation
is selected, independently reviewed and validated. This is a
blocker to the final product, not a silent deletion of native
OL3 capability. See [42 §1](42-POST-REJECT-CORRECTIVE-PLAN.md).

Approved envelope != approval of agent-produced tool commands. All existing managed
tool leases and approvals remain enforced by `wyss-control`; a native agent acts
with OS-user privileges once launched and must be labelled accordingly.
An OL3 workflow can remain running after the Console window closes or after KDE logout
(DN-46) only when its finite envelope and the host service allow it; this is NOT
OL4 scheduled unattended autonomous work. The approved pipeline does not authorise
extending itself after its bounds expire.

## 2. Components and identity

- `OrchestrationPlan` (versioned immutable data): DAG, bounds, step templates,
  trust labels, current workspace identity and approval digest.
- `Coordinator`: scheduling, bounded queue, dependency satisfaction, exclusive
  resource claims, stop/cancel, lease tracking, retries. This component is not the
  managed authority.
- `RunJournal`: durable *metadata only* for the workflow, states, terminal IDs,
  provider observation timestamps, outcome, digests and decision origins. No command
  content, provider output, context text, private paths or credentials.
- `StepExecutor` adapters: typed requests to `wyss-control` managed plane and to
  `wyss-host` native launcher. Every adapter must obtain its own launch proof;
  a handoff or pipeline id is never permission to execute by itself.
- `HandoffEnvelope`: explicitly selected data with user-visible provenance,
  source-step ID, target-step ID, revision/hash, byte and time bounds, quoting
  and no implicit authority. If a transfer necessarily persists model output,
  it requires a separate privacy-approved store design; **never put it in RunJournal**.
- `ResourceArbiter`: limits concurrent runtimes, PTYs and jobs; serializes
  incompatible project writes; reports contention and the identity of blockers.

Uniqueness requirements: a step's logical ID never equals its attempt ID; a native
session ID includes host and session generation; a managed runtime includes the
authority's runtime generation and catalogue version; every event binds
pipeline/step/attempt/host. After restarts stale attachments must not be mistaken
for live or approved ones.

### 2.1 Headless coordinator ownership — required by DN-36/46

**The coordinator cannot live only inside the Console UI.** Its pending
and running finite pipelines must remain supervised after the window
closes and, under owner-enabled lingering, after KDE logout. The
candidate is a small separate user service named wyss-orchestrator that
owns scheduler, admission control and metadata journal, talking over
authenticated local IPC to wyss-host and wyss-control.
It is **not** a third product called Wyss CLI, not a privileged system
daemon, not a native PTY owner and not a replacement for
wyss-control's security policy. The UI is only a client.

Alternative co-location in wyss-host may be considered after
security/performance review, but must not expose a terminal
escape parser's attack surface to the managed authority or couple
the scheduler's life to the UI's. The coordinator's own
crash/restart must restore bounded metadata, reconcile live step
attempt IDs with host/authority and pause *ambiguous* work,
not spawn replacements or replay shell input. Startup can be on
demand, but no idle exit while any approved pipeline is active
or waiting on an owner decision. systemd user lingering
must remain explicitly opted in under DN-46.

**Acceptance extension:** TS-ORC-12 must close the window,
log out of KDE under enabled lingering and reattach from Android,
while a bounded two-step pipeline remains in progress. If
the coordinator dies but child agents survive, it must not
mistake a resumed UI for a fresh approval.

## 3. State machines

### 3.1 Pipeline (SM-ORC)

```text
DRAFT -> VALIDATING -> AWAITING_OWNER_APPROVAL -> READY -> RUNNING
  VALIDATING -> REFUSED | NEEDS_REVISION
  AWAITING_OWNER_APPROVAL -> REFUSED | EXPIRED
  READY -> CANCELLED | EXPIRED
  RUNNING -> PAUSING -> PAUSED -> RUNNING
  RUNNING -> CANCELLING -> CANCELLED
  RUNNING -> COMPLETED | FAILED | EXPIRED | NEEDS_REVIEW
  PAUSED -> CANCELLING -> CANCELLED | EXPIRED | NEEDS_REVIEW
  FAILED/COMPLETED/CANCELLED/EXPIRED -> terminal (never re-enter RUNNING)
```

Allowed transitions have these guards:

| Transition | Guard and authority | Failure behaviour |
| --- | --- | --- |
| DRAFT→VALIDATING | Complete bounded DAG, supported typed provider/profile, no cycle | Stay draft, report exact failing field |
| VALIDATING→AWAITING_OWNER_APPROVAL | Current project identity, admission forecast and scope verified | NEEDS_REVISION; no step starts |
| APPROVAL→READY | Local owner reviewed the exact plan revision/digest, limits, native full-account risk and selected approval mode | REFUSED/EXPIRED; no partial authority cached |
| READY→RUNNING | Envelope not expired; resources and ownership leases obtained, first-step checks repeated | Wait bounded, then pause/fail safely |
| RUNNING→PAUSING/PAUSED | Explicit pause or resource/approval demand | No *new* step starts; track already-running native processes separately |
| RUNNING→CANCELLING | Owner stop, violated budget, revoked grant, deadline, unknown identity, safety failure | Revoke launch ability immediately, send permitted cancellation, then reconcile live children |
| RUNNING→COMPLETED | Every required step succeeded and no live outstanding child remains | Partial success is not COMPLETED |
| RUNNING→FAILED/NEEDS_REVIEW | Unrecoverable step failure, external modification, output ambiguity, context tampering | Fail closed for effects, preserve metadata |
| PAUSED→RUNNING | Fresh valid envelope and remaining capacity; pending approvals resolved | No automatic re-authorisation of new scope |

Cancel and shutdown distinguish *stop scheduling* from *terminate processes*.
Stopping new steps is immediate; terminating native agents may require escalation
with visible confirmation, process-group isolation and bounded grace. Never claim
"all stopped" until OS process exit is observed. Managed cancellation follows the
existing authority contract; no direct kill of `wyss-control` to cancel one turn.

### 3.2 Step (SM-ORC-STEP)

```text
PROPOSED -> WAITING_DEPENDENCIES -> ELIGIBLE -> AWAITING_PERMISSION -> QUEUED
QUEUED -> STARTING -> RUNNING -> VERIFYING -> SUCCEEDED
Any nonterminal -> SKIPPED | CANCELLED | BLOCKED | FAILED
RUNNING -> AWAITING_HUMAN (cannot start dependent steps)
VERIFYING -> FAILED | NEEDS_REVIEW
```

A start is exactly-once in the *coordinator journal* but may fail between process
spawn and acknowledgement. Reconciliation must inspect an immutable attempt ID
before retry; **never retry ambiguous effects automatically**. A retry creates a
new attempt with new approval if authority/risk/profile/target scope differs.
Skipped optional steps need explicit dependency semantics; mandatory dependents
cannot complete from missing output.

### 3.3 Human approvals

Distinguish:
1. Reviewing the overall DAG envelope (authorises scheduling of the named steps).
2. Native agent launch (conveys host-account risk, profile and controls left out).
3. Managed turn approval and tool approval by the existing authority.
4. Remote device step-up, which authenticates a remote action but is not native
   confirmation on the host.
5. Wyss Agent policy decision for effectful actions (DN-06=D).

**Required product behaviour, NOT a proven local trust boundary:**
approval of one layer does not *legitimately* grant another layer.
With same-UID Unix sockets, a compromised native/full-account process
can impersonate callers and may request approvals directly. No
component may claim per-layer approval authenticity until a
distinct principal or independently proven native-confirmation
boundary exists. Every native step needs fresh host-local,
per-attempt confirmation after reviewing the actual handoff
input and current controls; whole-DAG consent never substitutes
for it. The native auto-dispatch feature remains BLOCKED while
the same-UID impersonation path exists.

## 4. DAG and execution contract (IF-ORC v1 candidate)

`prepare(plan)`: validate schema, DAG, providers, project and estimates; output
plan digest plus structured failure reasons. No process start.

`submit(approved_plan_digest)`: a *candidate* scheduling
request only; validates a finite DAG and current controls. The
same-UID caller identity does not prove native presence or
authorise a native child. Pending native steps remain
`BLOCKED_TRUST_BOUNDARY` until the missing boundary is accepted;
once safe they require a **separate**, host-owned per-launch
review of exact step, current input, target and profile.
No arbitrary frontend executable/argv/environment.

`inspect(run_id)`: bounded status and statistics, with explicit UNKNOWN/OFFLINE.
A separate, authorised attachment accesses output via `wyss-host`, never through
the metadata event journal.

`pause/resume/stop(run_id, expected_revision)`: revision-checked, idempotent intents.
Stop never skips an already required safety acknowledgement.

`grant_step/deny_step(step_id, attempt_id, expected_generation)`:
requests only. The coordinator's same-UID socket must **never**
treat a field named native_confirmation as physical proof.
Until a real trust boundary is accepted, native launch requests
return `BLOCKED_TRUST_BOUNDARY`; managed requests still obey
the current authority gate. Denies never auto-retry.

`submit_handoff(source, target, selection_digest)`: for human-approved handoff,
review the actual content through a controlled dedicated surface, enforce size limits
and no inherited permissions. Agent-generated URLs, file paths and instructions
are untrusted. Automatic data-only handoff may target only managed or proposal-only
steps whose effects remain enforced by an independent authority.
It cannot trigger a native/full-account step, even inside an
approved DAG; model-provided prompt text can cause executable
effects at the recipient. A native target requires reviewed
actual bytes, per-launch confirmation and an accepted OS boundary.

`observe(event)`: consume only per-component versioned, typed events and reject
unknown identifiers, generations, oversized payloads and out-of-order revisions.
No raw native terminal output is parsed by the coordinator for authority decisions.

### 4.1 Constraints and invariants

- DAG acyclic and finite; fan-out bounded, fan-in explicit, one active attempt per
  step; no orchestration recursion from an agent's own delegator.
- **BLOCKED future requirement DN-08=A / DN-42=C**:
  do not create one managed runtime per session, or send per-turn
  model selection, under current accepted ADRs. ADR 0012 permits
  one shared App Server; ADR 0015 enforces one global active
  managed turn. All managed pipeline steps are **serialized**
  until new separately reviewed/accepted authority ADRs and
  compatibility tests change this. ADR 0031 remains Proposed.
- Native subprocesses retain their native privilege; coordination never
  silently claims they were sandboxed.
- Writes to the same workspace must either use a reviewed single-writer lock and
  conflict policy or run in isolated worktrees with explicit merge/review. A
  per-project lock can *coordinate Console participants* but cannot stop an
  external tool or the user editing files. Detect unexpected changes and pause.
- Resource ceilings apply at scheduler admission and continuously during runs.
  Limit process groups, queue length, log metadata, simultaneous open sessions,
  wall time and spool size. OS enforcement mechanisms not chosen without tests.
- Inferences based on untrusted agent output do not produce privileges or
  guaranteed success. Verify exit status, concrete artefacts and approval.
- The UI reports `queued/active/waiting approval/paused/failed/review needed`
  with distinct labels; no silent partial result.

## 5. Crash, reconnect and degraded behaviour

| Failure | Required behaviour |
| --- | --- |
| Desktop UI closes/reloads | Coordinator/host do not depend on UI process; reconnect by run/session IDs, otherwise show UNKNOWN; never launch a duplicate |
| Host user logs out | Runs continue *only* under owner-enabled lingering; native confirmations may wait for local login |
| Coordinator crashes | Reconstruct metadata journal and query host/authority; ambiguous external step becomes NEEDS_REVIEW, not auto-run |
| Host dies | Mark PTY LOST, but process tree **UNKNOWN / ORPHANS_POSSIBLE** until isolation/scope observation; no restart proposal capable of acting while descendants may still write; preserve journal |
| Authority rejects managed action | Record the exact refusal; pipeline pauses or fails; no native fallback |
| Provider/model unavailable | Queue boundedly or fail, never substitute another model without the plan's explicit substitution rule and confirmation |
| User revokes device | Reject new requests from it, **pause unstarted pipeline work** whose approval originated from the device, mark prior decisions for review; existing native processes may continue with OS-user privileges and must not be reported as revoked/stopped |
| Network drops | Running work on host may continue inside approved finite limits; no queued commands replayed without sequence ACK |
| Clock drift / quota unknown | Enforce local monotonic time where suitable, fail closed on expiry; do not assert monetary caps without reliable counters |
| Secrets in an agent handoff | Reject known prohibited channels; no content persisted in events/usage analytics; surface controlled handoff needs separate reviewed design |

## 6. Why OL3 is not OL4

OL3 can run a **finite, previously reviewed** workflow without the window open,
subject to pending confirmations and quotas. OL4 would create or schedule workflows
from future events without the owner initiating/reviewing that bounded workflow.
There is no OL4 event scheduler, unattended privileged bypass, or self-extending
agent plan in final-product scope. Background persistence (DN-46) alone does not
make a system OL4.

## 7. Acceptance candidates (must later be registered in 20 and 23)

- **TS-ORC-01:** reject cyclic, oversized, unbounded or unsupported plan without launch.
- **TS-ORC-02:** mutated plan after approval changes digest and requires fresh consent.
- **TS-ORC-03:** multi-step implementation→test→review follows dependency, fails
  at a blocking step, never runs dependent review on absent evidence.
- **TS-ORC-04 (BLOCKED pending ADR):** two managed sessions
  do not share attestations, runtime generations, leases,
  catalogues, model observations or write transactions. Under
  current ADR 0012/0015 the expected behaviour is one shared
  App Server and serial managed turns, not concurrent execution.
- **TS-ORC-05:** native output containing prompt-injection, path escape or control-like
  strings yields no scheduler permissions or runnable command.
- **TS-ORC-06:** cancellation halts new dispatch, reports live native process honestly,
  and marks failure if a forced stop cannot be confirmed.
- **TS-ORC-07:** coordinator crash between spawn and ACK reconciles attempt identity;
  never creates a second agent without a fresh user action.
- **TS-ORC-08:** remove/alter hooks after approval; profile exclusion differs;
  must revalidate and require new consent (DN-20).
- **TS-ORC-09:** timeout, resource ceiling and unknown quota each visibly prevent
  further dispatch; no infinite loop or indefinite spool.
- **TS-ORC-10:** phone/watch client cannot create, widen, alter or approve privileged
  plans via metadata-only grants.
- **TS-ORC-11:** managed tool approval stays inside `wyss-control`; coordinator
  or native runtime cannot forge it.
- **TS-ORC-12:** across close/relaunch, logout, network loss and host crash, step
  state reflects reality; no silent resume of a dead process.
- **TS-ORC-13:** prove no prompt/tool output, secret, private path or command body
  reaches durable journal, notification, or usage history.
- **TS-ORC-14:** concurrency/load test of independent managed runtimes on each
  reference PC; attach process/memory/CPU statistics and failure report.
- **TS-ORC-15:** Hermes with direct effectful tools disabled cannot re-enable,
  escape the isolation boundary or issue effects except through Wyss policy.
  This is a **hard gate for DN-06=D**, not an assumption.

All are **specifications**, not tests run. A full run needs the separate
owner authorisation gate DN-24/DN-16 and the appropriate review.

## 8. Completion gates

- Requirements FR-ORC-01/02 and related acceptance matrix are proposed; ensure
  [02](02-REQUIREMENTS.md), [20](20-VERIFICATION-AND-ACCEPTANCE.md), and
  [23](23-TRACEABILITY-MATRIX.md) agree.
- A threat model covers compromised coordinator, compromised native agent,
  malicious handoff, model reroute, confused deputy, split-brain,
  interrupted transaction, and remote input.
- An accepted security ADR must reconcile the managed authority one-runtime
  invariants with DN-08's multi-runtime target; **none is accepted**.
- Hermes effects must be OS-mediated or the integration remains blocked.
- No provider experiment, code or services may run merely because this contract exists.
