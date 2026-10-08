# ADR 0016: Tool and approval mediation

Status: Accepted
Date: 2026-10-08

## Decision

Wyss admits two model-initiated actions, each mediated by the external authority: one
one-shot command execution and one file change set at a time. Nothing else the provider
can do is admitted. The inference-only profile of ADR 0015 is unchanged and remains the
default; an action exists only in a runtime generation that was explicitly started
under the isolated `codex_local_tools_v1` profile, for a session attached to that
generation as `local_tools_v1`, on a turn admitted under ADR 0013 and ADR 0015.

Every action is announced by the provider, becomes a durable `ManagedApproval` only
when the authority can present it complete and prove it confined to the session
workspace, and runs only after the authority committed an Accept and wrote exactly one
provider answer. A client decision is an intent. The authority derives and revalidates
everything else. Tauri and human PTYs are unchanged; no desktop capability is added.

## Runtime profile and isolation

`RuntimeSafetyProfile::CodexLocalToolsV1` and `AttachmentProfile::LocalToolsV1` sit
next to `CodexInferenceV1` and `InferenceOnlyV1`. A generation has one profile for its
whole life: `runtime/start` selects local tools explicitly and names the single
canonical workspace the generation serves, and a start that would change profile or
workspace on a live generation is refused before anything durable changes. The profile
is not durable: after an authority restart a desired-running runtime comes back as
inference, the least capable profile, and every attachment has to be resumed
explicitly.

The supported runtime is exactly Codex 0.159.0. The local-tools generation, including
its discovery process, runs inside an unprivileged `bwrap` namespace whose argv is
fixed by the authority and built without a shell or any client, provider or workspace
text:

- **Managed requirements.** `/etc/codex` is replaced by an authority-owned directory
  holding `requirements.toml` with `[features] unified_exec = false`. In 0.159.0 the
  same flag passed as process or thread configuration is overridden by the runtime;
  only a managed requirement removes the persistent command tool, `write_stdin` and
  processes that outlive a turn. The runtime must report that requirement itself
  through `configRequirements/read`.
- **Private home.** `CODEX_HOME` is a private authority directory. The user's Codex
  home and `~/.codex` are hidden behind an empty tmpfs, so no user configuration,
  exec-policy rule, hook or MCP server is loaded; only the pinned release directory
  returns, read-only. No `CODEX_*` or `OPENAI*` variable is inherited.
- **Project policy.** Every existing `.codex` layer from the project root down to the
  workspace is masked read-only, and every thread forces the project root and the
  workspace `untrusted`. A layer that appears later and holds anything stops further
  work.
- **Authority model catalog.** The generation is launched with the authority's
  normalized presentation of the runtime's backend catalog: `tool_mode = direct`, no
  experimental tools, no multi-agent version. Unlike the inference presentation it
  keeps the backend's patch tool type, so the model is offered exactly `exec_command`
  and `apply_patch`.
- **Posture.** Every thread is started and resumed with approval policy `untrusted`
  and a `workspace-write` sandbox without network access, with `/tmp` and `$TMPDIR`
  removed from the writable roots, and must come back reporting exactly that.

Readiness, every conversation, every turn and every Accept require the runtime to
prove this profile again: private home, managed requirement, every reflected clamp
with only the command tool enabled, no MCP server, no enabled hook, the masks still
holding. Anything unproven is `safety_unproven` and ends the generation. The host must
provide `/usr/bin/bwrap` and an existing `/etc/codex` directory; the authority creates
neither.

> **Implementation status (Sprint 2K, ADR 0018).** The namespace was hardened without changing
> this decision. The managed policy is mounted on a private overlay of `/etc`, so a
> host `/etc/codex` is no longer a prerequisite; `bwrap` needs unprivileged overlay
> support instead. The authority's own runtime and state directories (control socket,
> locks, canonical database) are hidden inside the namespace; the generation gets back
> its private home and, read-only, its model catalog. The masked project chain starts
> at the outermost ancestor holding a `.git` marker instead of the nearest, and every
> such ancestor is forced `untrusted` together with the workspace, so a marker that
> appears in the workspace can never shorten what is masked. The proof before each
> conversation, turn and Accept also covers the identity of the bound credential.

### Credential capability

One capability crosses into the namespace: the user's `auth.json`, bound read-write
onto the private home because the runtime refreshes its token in place. The authority
never reads, copies or logs it, and refuses a symlinked, foreign-owned or group- or
world-accessible file. A token refresh by the isolated runtime therefore writes the
user's own credential file. Narrowing this boundary (a dedicated credential, or a
broker that owns the refresh) is deferred to Sprint 2K and is the main residual risk
of this decision.

> **Implementation status (Sprint 2K, ADR 0018).** The read-write bind stays: Codex 0.159.0
> rewrites `auth.json` in place when it refreshes a token, and a mirror or a
> read-only bind would leave the user's own login with a rotated, stale refresh
> token. It is now a narrow, validated capability. The file is opened without
> following a link and without read access; it must be a private regular file of this
> user with exactly one name, in a canonical home directory nobody else can write; and
> the launcher binds that open descriptor, so the inode that enters the namespace is
> the one that was validated, whatever its path names afterwards. Its identity is
> checked again with every profile attestation, and a replaced, widened, re-linked or
> removed credential ends the generation. A command the user accepts can still read
> the file, exactly as under the user's own Codex.

## Mediated surface

Only the thread-capable local-tools process has a mediated surface. On it the
supervisor admits exactly two server requests, `item/commandExecution/requestApproval`
and `item/fileChange/requestApproval`, together with the `commandExecution` and
`fileChange` items they are about, their output and completion, `serverRequest/resolved`
and the turn's applied-diff notice. The request parameter sets are the exact sets of
the pinned protocol; an unknown field is an unknown decision surface.

The following stay isolation violations under local tools exactly as under inference:
hooks, MCP startup or calls, web and network tools, plugins, apps, skills, dynamic
tools, sub-agents, collaboration, request-user-input, permission requests, write-stdin
approvals, terminal interaction, additional permissions, network approval context or
policy amendments, a callback identity of another terminal, an environment other than
the runtime's own `local`, a persistent `grantRoot`, a second request while one is
unanswered, a request for a turn that is not the active one, an item that starts
already executed, a user shell command, and any output, completion or applied diff of
an item the authority did not accept. A command the runtime runs on its own without
asking is one of them. On a violation the stream stops, nothing is ever approved, the
supervisor attempts an interrupt when it is the one that detects it, the owned runtime
is reaped, attachment and profile are cleared, the turn fails and its approval is
invalidated. Nothing is replayed.

The inference profile answers no server request at all.

## ManagedApproval

Schema v6 migrates v1–v5 transactionally, preserving every existing row and audit
byte. It rebuilds `execution_leases` so the stored scope encoding is bounded by the
canonical maximum of the five scopes (`session_create`, `session_resume`,
`turn_start`, `command_execute`, `file_change`) and adds `managed_approvals`.
Migration creates no approval. A row holds
identities, the admitted lease and its revision, the runtime generation, the provider
item, state, revision, decision, external-effect marker, deadlines, an action
fingerprint and bounded counts. It never holds a command, output, path, diff or file
content. `ManagedApprovalId` is its own identity domain. States are `pending`,
`accepted`, `declined`, `cancelled`, `expired`, `invalidated` and `indeterminate`; only
`pending` is nonterminal. A partial unique index allows one pending approval globally,
a provider item is mediated at most once per turn, and a trigger refuses any write
that is not a legal single-revision transition. There is no approval queue: at most
one managed turn is active and at most one approval is pending.

The **action fingerprint** is SHA-256 over a domain tag and length-prefixed fields: for
a command the provider item, the normalized working directory and the exact command;
for a file change the provider item and the complete change set ordered by path, each
with its change kind, move destination and diff. It is computed by the authority from
what it holds in memory, never supplied by a client, and it is the only trace of the
action in storage and audit.

### Command mediation

The command comes from the provider's announcement (`item/started`, in progress) and
its request. Whatever the request restates has to equal the announcement exactly;
otherwise the provider contradicted itself and that is a violation. A command is
approvable only if it is 1 to 16 KiB of UTF-8 without NUL, its working directory is at
most 4 KiB, it has at most 64 parsed actions of at most 256 bytes, and its complete
presentation fits one control frame. The provider's parsed actions are display labels
only; the fingerprint and the decision cover the exact command. Real 0.159.0 wraps the
model's command as `/usr/bin/bash -lc '<command>'`: that wrapped text is the action.

### File mediation

A file approval exists only for a complete change set: every path, change kind, move
destination and diff. The pinned runtime announces the change set before it asks. If
the request arrives first, the authority holds it without an approval, it cannot be
accepted, and it is refused after ten seconds. Once the approval is open the change
set is frozen: only an identical restatement is tolerated. Bounds are 64 files,
128 KiB per diff and 256 KiB in total, and nothing the client is asked to approve is
truncated; the provider frame codec is stricter still (64 KiB per string, 256 KiB per
frame) and an oversized frame is contained. `grantRoot` is never honored.

### Workspace confinement

A working directory, and every source and destination of a change set, must be inside
the canonical workspace of the session. The proof compares no string prefixes and
follows no link. The path is normalized lexically, a `..` component is refused rather
than resolved, the path has to extend the workspace component by component, and every
component that exists must be a real directory on the workspace's own filesystem. A
symbolic link anywhere, a hard-linked file, a mount point, a component named `.codex`,
`.git` or `.agents`, or anything that cannot be inspected makes the proof fail. The
leaf must be what the action needs: an existing directory for a command; an existing
regular file for update, delete and a move source; a regular file or nothing for add
and a move destination, in which case the missing components can only be created
inside. A proposal that is out of bounds, malformed or not provably confined never
becomes an approval: the provider is refused and the turn stops with
`action_rejected`. The proof is repeated immediately before every Accept. The
runtime's own workspace sandbox remains the enforcement against a change after it;
the authority's check is what a human is shown and agrees to.

## Lease authority and deadline

An approval inherits the ORIGINAL execution lease of its turn: the lease identity and
revision admitted with the turn are persisted on the approval. Accepting a command
requires the `command_execute` scope and accepting a file change the `file_change`
scope, on that lease, which must still be present, active and unexpired. A replacement
lease never transfers authority, even with every scope. Renewing the same lease keeps
the approval acceptable but does not move its deadline, which is fixed when the
approval opens as the stricter of five minutes and the lease expiry of that moment.
The deadline is scheduled, not polled. On expiry the approval becomes `expired`, the
provider is refused and the turn stops.

## Decisions

The public decisions are `accept`, `decline` and `cancel`. The authority answers the
provider with at most one of `accept`, `decline` or `cancel`, and only one the request
offers. `acceptForSession`, `acceptWithExecpolicyAmendment`,
`applyNetworkPolicyAmendment` and permission grants are never emitted; an amendment the
provider proposes is ignored.

Real 0.159.0 offers `accept`, `acceptWithExecpolicyAmendment` and `cancel` for a
command, and no `decline`. A refused command therefore maps to provider `cancel`
whether the client said decline or cancel, which also ends the turn. A file request
carries no list and the pinned protocol defines `decline` for it: a declined file
change is answered with `decline` and keeps its turn, a cancelled one with `cancel`. If
a request offers no refusal at all, none is synthesized and the turn is interrupted
instead.

Decline and cancel reduce authority, so they need none: only the exact pending
approval and its still-held provider request. They stay possible after the scope, the
attachment or the lease is gone.

### Accept gate

Immediately before a provider Accept the authority revalidates, in this order:

1. In memory, against the exact approval: healthy authority and storage; the active
   turn, running and not being stopped; the exact runtime generation, live and ready;
   the local-tools profile and its workspace; the `local_tools_v1` attachment of that
   session to that generation and thread; the held provider request, unanswered, for
   that exact item, offering `accept`; the complete action, still fingerprinting to the
   stored value; workspace confinement, proven again.
2. In the supervisor, while it holds the request: the full profile attestation.
3. In one transaction: the approval still pending at the expected revision and before
   its deadline, the session prepared, the turn running under the same admitted lease,
   that lease active, unexpired and carrying the scope.

The transaction commits the decision, the dispatch marker (`effect = dispatched`), a
fingerprint-only audit event and the receipt. Only then is the one answer handed to the
supervisor, after the receipt. Any failure before the commit means zero Accept frames.
The supervisor writes at most one frame per request and marks the request answered
before the write.

Decisions are idempotent by intent: the same intent, approval, revision and decision
returns the original receipt from any connection and never a second frame; another
payload under the same intent is `intent_conflict`. Concurrent decisions are
serialized by the single writer and arbitrated by the transaction, so Accept against
Accept, lease revocation or expiry, turn cancel or session close has exactly one
committed outcome. A refusal wins against an Accept that is still being attested.

## Loss of authority, uncertainty and recovery

Turn cancel, lease revocation or expiry, session close and authority shutdown make
the approval terminal before any provider frame is written, refuse the held request
and stop the turn. Session close stays authoritative: it is not rolled back because
the provider is awkward. If the runtime is lost while an approval is pending the
approval is `invalidated`.

A provider Accept is an external-effect boundary. If the answer cannot be proven
delivered, or the runtime is lost after the dispatch marker and before the provider
reported the item, the approval is `indeterminate`, the turn is terminal and the
runtime is contained. The same holds after an authority restart: an undecided approval
is invalidated and an accepted one without a confirmed outcome is indeterminate.
Nothing is ever resent and no action is ever replayed; a new action needs a new turn
and a new approval. Exactly-once execution by the provider is not claimed. When the
provider reports the accepted item finished, the marker becomes `confirmed`.

## Control protocol

`managedTurn/approval/pending`, `managedTurn/approval/status` and
`managedTurn/approval/respond` are the whole surface. There is no generic provider
response, no provider request identity and no raw provider method. `pending` returns
the approval one turn waits on. `status` returns canonical metadata and, only while
the authority still holds the provider request, the action: the complete command, or
one file of the change set with its complete diff in contiguous 8 KiB pages split on
character boundaries. The action is never read from storage, so a decided approval and
an approval after a restart show metadata only. `respond` takes the approval, its
expected revision, an intent and a decision. A disconnect decides nothing.

## Streaming and privacy

The bounded per-turn transient stream of ADR 0015 is extended, not duplicated, with
`command_proposed`, `file_change_proposed`, `approval_required`, `approval_resolved`
(naming the approval and its terminal state), `command_started` and
`file_change_started` once the Accept reached the provider, `command_output_delta`,
and the completed and failed events. Sequence, page, UTF-8 and retention rules are
unchanged: deltas of at most 4 KiB, at most 256 events and 256 KiB retained per turn,
oldest events dropped with an explicit gap. Output a one-shot command reports only when
it finishes is shown then, once. The stream carries command output and never the
command, the paths or the diff.

Commands, output, paths and diffs live only in authority memory and in transient
control responses. They never enter SQLite, its WAL or SHM, the audit stream, intent
receipts or diagnostics; the action types derive no `Debug`. Codex keeps its own
conversation history in the private home, which is provider state, not Wyss
durability.

## Limits and residual risk

- The credential file is shared read-write with the isolated runtime (deferred to 2K;
  see the status note under Credential capability).
- `bwrap` and a host `/etc/codex` are prerequisites; without them the profile is off
  (since Sprint 2K: `bwrap` with overlay support, and no host directory).
- The profile is not durable across an authority restart, by design.
- The authority cannot stop a command the runtime decides to run without asking; it
  can only detect it and contain the runtime. With the posture above, real 0.159.0
  asked for approval for every command probed, including `ls -la` and
  `cat /etc/hostname`, which it classifies as safe elsewhere. That is an observation
  of the pinned version, not a guarantee the authority enforces.
- Path confinement is checked when the approval opens and again before Accept. A
  change between that check and the provider's write is bounded by the runtime's own
  sandbox, not by the authority.
- One approval at a time: a provider that asks for two at once is contained.
- Compatibility is pinned to 0.159.0; other versions fail closed.
- An independent review of this sprint is pending (carried out in Sprint 2K).

## Acceptance evidence

Deterministic coverage is in the Rust suite: the approval state machine, the Accept
gate, races, deadlines, decision mapping, confinement, presentation, streaming and
privacy as unit tests against a runtime that is a channel, and the same flows through
the real authority binary, the control socket and a scripted provider inside the real
`bwrap` namespace.

On 2026-10-08 the production authority binary, launcher and control protocol were
exercised against real Codex 0.159.0 with the authenticated provider, in an isolated
authority state and a temporary workspace:

| Case                              | Measured                                                                                                                                                                  |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A inference regression            | 1 model request traced, 0 model-visible tools, 0 commands, 0 file mutations, 0 approval responses                                                                         |
| B command accept                  | announced, pending, nothing ran during a 3 s hold, 1 provider Accept, 1 execution, output `WYSS_2I_COMMAND_OK`, completed and confirmed; the retried intent wrote nothing |
| C command decline                 | provider `cancel` 1, executions 0, turn interrupted                                                                                                                       |
| D command cancel                  | provider `cancel` 1, executions 0, turn interrupted                                                                                                                       |
| E file accept                     | complete proposal shown, target absent for 4 s, 1 provider Accept, 1 mutation, exact content                                                                              |
| F file decline                    | provider `decline` 1, mutations 0, the turn went on and completed                                                                                                         |
| G file cancel                     | provider `cancel` 1, mutations 0, turn interrupted                                                                                                                        |
| H missing scope                   | Accept `gate_denied`, provider Accept 0, action 0, approval still pending and then refused                                                                                |
| I lease revoked while pending     | approval `invalidated` (`lease_revoked`), provider `cancel` 1, turn interrupted, later Accept `invalid_transition`, action 0                                              |
| J runtime killed while pending    | approval `invalidated` (`runtime_lost`); new generation wrote 0 answers and started 0 turns; binding preserved and detached; a turn needed an explicit resume             |
| K runtime killed after the Accept | 1 Accept written; approval `indeterminate`, turn indeterminate; new generation wrote 0 Accepts and started 0 turns; the command never completed and left no process       |
| K killed at the response boundary | runtime killed 20 ms after the decision was sent: the receipt committed, at most 1 frame, approval `indeterminate`, nothing resent                                        |
| L forbidden capabilities          | 0 isolation violations over every turn, 0 code-mode helper, 0 process left by the provider, profile re-attested before every turn and Accept                              |
| Privacy                           | 0 occurrences of the command, output and diff sentinels in every Wyss-owned file; both fingerprints present                                                               |

Totals of that run: 11 command proposals, 4 accepted, 1 declined, 4 cancelled, 4
executions started (2 completed, 2 indeterminate after the injected runtime kills); 4 file
proposals, 2 accepted, 1 declined, 1 cancelled, 2 mutations; 6 provider Accept frames
for 6 accepted approvals, 0 duplicates; 0 unauthorized commands, 0 unauthorized file
mutations, 0 replayed actions. The workspace ended with exactly the two approved
files, next to the empty directories the runtime's own sandbox leaves.

Case L counts what the authority can observe: any forbidden surface is a violation,
and there was none. The tool inventory of the local-tools profile itself was not
traced in this run, because the launcher strips the variable that enables the
runtime's native trace; the model reported exactly `exec_command` and `apply_patch`
and reported web search, sub-agents, user input, terminal input and MCP as
unavailable, matching the inventory traced for this profile on 2026-10-07.

Measured facts of the pinned provider that this design depends on: a command request
carries `environmentId: "local"`, `kind: "command"` and `availableDecisions` even with
the experimental API disabled; the model's command arrives wrapped as
`/usr/bin/bash -lc '<command>'`; a one-shot command emits no output deltas and reports
its output with the finished item; the diff of an added file is its raw content; after
`cancel` the provider reports the request resolved, the item declined and the turn
interrupted, in that order; `decline` on a file change keeps the turn running.
