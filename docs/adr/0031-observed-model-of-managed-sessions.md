# 0031 — The observed model of managed sessions

Status: **Proposed** on 2026-10-13. Not accepted. It changes no code, no schema, no event
vocabulary and no capability of the window. It authorises no implementation.

It was written by the author of the architecture set and has not been reviewed by anyone else.
It answers DN-09 (`docs/architecture/22-DECISIONS-AND-OPEN-QUESTIONS.md`). Acceptance needs:

- the owner's choice among the options of §3;
- a review of the exact text by someone who did not write it;
- because it changes what the authority's classifier accepts and what it emits, the safety
  contract, evidence and review that 10 §6 requires for every extension of the managed plane.

Date: 2026-10-13

## 1. Context

### 1.1 What is built `[R]`

- The adapter reads `model` and `modelProvider` from the answer to `thread/start` and
  `thread/resume`. It normalises them into `observed_model`, an `AgentModelDescriptorV1`
  (`src-tauri/src/adapters/codex/normalize.rs:197-211`, `:425-430`). Its rules:
  - A missing or non-string `model` gives `None`.
  - A string outside the descriptor's bounds fails the observation.
  - An invalid provider becomes `null`.
- ADR 0014 then says: "Provider model/policy/configuration fields remain observations and are
  discarded." Nothing of it reaches the control plane, SQLite, the event log or the window.
- The window's `AuthorityView` v1 carries `model_count` only (ADR 0019; `authority/view.rs:33`).
- The managed turn's safety classifier tolerates a fixed list of notifications and turns any
  other into `inference_isolation_violation` (`adapters/codex/safety.rs:1108`, `:1203-1220`).
  `model/rerouted` is not on the list.
- The event store has 28 event types, vocabulary number 1, schema version 7 (ADR 0017). Payloads
  hold "identifiers, enumerations, revisions, timestamps, counts and SHA-256 fingerprints".

### 1.2 What the protocol says `[D]`

From the Codex App Server schema and source at tag `rust-v0.159.0`, the version the authority's
codec names (`safety.rs:818`). Read on 2026-10-13; not run.

- `ThreadStartResponse` and `ThreadResumeResponse` carry `model`, `modelProvider`,
  `reasoningEffort` and `serviceTier`.
- `Turn` has no model field. The model is reported per **thread**, not per turn. This answers
  OQ-10 from documentation.
- `model/rerouted` (`ModelReroutedNotification`) carries `threadId`, `turnId`, `fromModel`,
  `toModel` and `reason`. `reason` is the closed set `highRiskCyberActivity`. The notification
  is **not** marked experimental (`common.rs:1995`), so a client with `experimentalApi: false`
  is not excluded from it by the protocol's own gating. Whether and when Codex sends it is not
  known (OQ-50).

### 1.3 What follows

1. Today the Console cannot show the model of a managed session, though the adapter holds it.
2. Today a reroute, if Codex sends one during a managed turn, fails that turn closed as an
   isolation violation. This is safe, but it reports a provider's safety reroute as a breach of
   isolation. That is wrong for the user and noisy for the audit.
3. "The model of a turn" can only mean the session's reported model, unless a reroute names that
   turn.

## 2. Decision drivers

- **AD-16:** a model is shown as used only when the provider reported it. A default, a catalogue
  entry or a configuration value is never substituted.
- **SG-12 (10 §2) stays as written.** This ADR adds no field a client can send.
- **The classifier must not get weaker.** Tolerating `model/rerouted` must not let any other
  notification through. A malformed or mismatched reroute must still fail closed.
- **Strict contracts.** Every new field is versioned, bounded and validated on both sides.
  Unknown fields stay rejected.
- **Durable data stays minimal (ADR 0003).** Model identifiers are provider identifiers, not
  prompt or output text. They are still new durable data, and a durable choice needs a reason.

## 3. Options

| Option | Where the observation lives                                                                                                                                                     | Survives                                     | Schema and vocabulary                                                                                                         | History (FR-MOD-09)                                          |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| **A**  | **Transient attachment state.** Kept in memory beside the attachment, like the session's runtime state. Projected to the window while attached                                  | Until detach, runtime stop or authority exit | `AuthorityView` (or its session section) gains an optional, versioned `observed_model` per attached session. No SQLite change | None                                                         |
| **B**  | **A durable column of `managed_turns`** (`observed_model_id`, `observed_provider_id`)                                                                                           | The turn row                                 | Schema version 8; the turn record's contract grows; turn events' payloads grow                                                | Per turn, but session grain written as if it were turn grain |
| **C**  | **A versioned event**: `authority.managed_session.model_observed` when the session-grain report first arrives or changes; `authority.managed_turn.model_rerouted` for a reroute | The event log                                | Event vocabulary 2 (ADR 0017's rule: recorded number never decreases); new payload types; no table change                     | Yes, as events                                               |

**B is rejected.** It writes a session-grain fact into a turn row, which is the conflation AD-16
forbids. It also changes the canonical turn table for a fact that is not authority state.

## 4. Proposal

### 4.1 The observation

`ManagedModelObservationV1`, built by the adapter. All fields are bounded as in ADR 0008 unless
stated.

| Field                 | Type                                               | Rule                                                                                        |
| --------------------- | -------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `schema_version`      | 1                                                  | Unknown fields rejected                                                                     |
| `grain`               | `session` · `turn`                                 | `turn` only from a reroute                                                                  |
| `source`              | `thread_start` · `thread_resume` · `turn_rerouted` | Closed                                                                                      |
| `model_id`            | Opaque, 1 to 256 bytes, or `null`                  | `null` means "not reported", never "default"                                                |
| `provider_id`         | Nullable                                           | As `AgentModelDescriptorV1.provider_id`                                                     |
| `reasoning_effort`    | Opaque, 1 to 64 bytes, or `null`                   | From `reasoningEffort`; not read today                                                      |
| `from_model_id`       | As `model_id`                                      | Only for `turn_rerouted`                                                                    |
| `reason`              | `high_risk_cyber_activity` · `unrecognised`        | Only for `turn_rerouted`; the provider's value mapped to a closed word, its text never kept |
| `managed_turn_id`     | Present only for `turn` grain                      | The authority's turn whose `codex_turn_id` equals the notice's `turnId`                     |
| `runtime_instance_id` | As today                                           | The generation that reported it                                                             |
| `observed_at`         | RFC 3339 UTC                                       | The authority's clock                                                                       |

**Missing, malformed and unknown reports.**

| Case                                                 | Result                                                                                               |
| ---------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `model` missing or not a string in the thread answer | `model_id: null` (as built). The window shows "not reported"                                         |
| `model` a string outside the bounds                  | The answer fails as a protocol error (as built)                                                      |
| `model` not in the current catalogue                 | Kept as reported. The window adds "not in this runtime's list". Never corrected to a catalogue entry |
| `reasoningEffort` missing or not a string            | `reasoning_effort: null`                                                                             |

### 4.2 `model/rerouted`

The classifier gains one case, accepted **only** when all of these hold:

1. A managed turn is running on the session.
2. `threadId` equals the binding's thread.
3. `turnId` equals the running turn's `codex_turn_id`.
4. `fromModel` and `toModel` are strings within bounds.
5. The object has no fields beyond the five of the schema.

Then it yields a `turn`-grain observation and the turn continues. In every other case, it is
`inference_isolation_violation` as today. An unknown `reason` value is mapped to `unrecognised`;
it does not fail the turn. The notification carries no tool, command or content, so tolerating it
adds no path for model-visible capability.

The session-grain observation is then updated to `toModel`, with `source` kept as
`turn_rerouted`, because Codex's notice says the reroute applies from that turn. Whether later
turns stay rerouted is not documented (OQ-50); the window says "reported for this turn" for the
rerouted turn and keeps the session-grain line marked with its source.

### 4.3 Where it lives

**Recommended: A now, C only if the owner wants a history of model changes.**

- **Under A:**
  - The `AuthorityView` revision rises.
  - The window receives, per attached session, the latest session-grain observation and the
    reroutes of the turns it currently shows.
  - Nothing is durable. On detach, runtime stop or authority exit, the observation is dropped,
    and the window says "not attached".
- **Under C, in addition:**
  - Two event types are added, and the event vocabulary becomes 2. A build with vocabulary 1
    refuses a store that has recorded 2 (ADR 0017), so a downgrade after the first such event
    is refused. That is the existing rule, stated here so it is not a surprise.
  - Payloads hold identifiers, the closed `reason` word and timestamps only.
  - The ADR 0014 sentence "remain observations and are discarded" is amended for `model` and
    `modelProvider` only.
  - Retention follows the event log's.

### 4.4 Catalogue (related, same projection change)

- `catalogue_partial`: the handshake's single `model/list` returns `nextCursor`. When it is not
  `null`, the authority records that the list is incomplete, and the projection carries a
  boolean `catalogue_complete`.
- Replacement marking: `upgrade` / `upgradeInfo.model` / `retirementAt` may be carried as
  identifiers and a date. `upgradeCopy` and `migrationMarkdown` are provider text and are never
  carried.

### 4.5 Audit

- **Under A:** no audit record, as for every adapter observation today (ADR 0014).
- **Under C:** the events are the record.
- **Under either:** a reroute that fails the new checks is audited exactly as an isolation
  violation is today.

### 4.6 What this ADR does not do

- It adds **nothing** a client can send. Thread and turn start parameters stay as ADR 0014 and
  0015 fix them.
- **SG-12's text is unchanged.** Its text names executable, arguments, environment, workspace
  override and tool. The absence of a model field rests on ADR 0014, 0015 and the codec.
  Whether to make "a model or effort" explicit in SG-12 is a separate change, proposed here only
  as a question for the security review (31 §5, C14).
- **Managed model selection is DN-42**, a separate proposal with its own security review (12
  §5.5).
- **Usage and rate-limit figures are DN-43** (12 §6). They are not carried by this ADR.

## 5. Consequences

- **Under A:** F6a can show "Model: X (reported for this session)" for an attached managed
  session, and can show a reroute instead of failing the turn. FR-MOD-09 stays unmet for the
  managed plane.
- **Under C:** FR-MOD-09 is met for reported changes. A vocabulary change becomes part of the
  store's compatibility.
- **Under any option:** no test runs Codex. Fixtures are built from the schema (12 §8;
  TS-MOD-01 to 10).

## 6. Evidence still missing

| Question | Why it matters                                                                           |
| -------- | ---------------------------------------------------------------------------------------- |
| OQ-50    | Whether Codex sends `model/rerouted` to this client, and what later turns report         |
| OQ-10    | Answered from documentation; a fixture from a real run would confirm it (not authorised) |
