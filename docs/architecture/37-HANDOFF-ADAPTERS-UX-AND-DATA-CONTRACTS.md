# 37 — Remaining cross-cutting contracts: handoff, adapters, UX, durable analytics

Status: **paper-only candidate contracts** closing named gaps of 31 rows 14, 15, 16,
17, 18, 21 and 28. This is not a provider experiment or an accepted ADR.
Owner scope: [32](32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md).
Evidence and review gates: [36](36-PLANNING-CLOSURE-AND-VERIFICATION-GATES.md).

## 1. Explicit handoff package, F11a (SM-HANDOFF)

A handoff is a deliberate transfer of *selected context* to a **new** session.
The receiving session never inherits the sender's model-internal conversation,
credential, tool set, project trust or policy approvals. The user's source
session stays intact.

### 1.1 Typed handoff envelope (candidate schema v1)

Fields: `schema_version=1`, `handoff_id`, `source_host_id`,
`source_session_id/generation`, `source_plane`, `source_project_id`,
`target_provider_id/profile_revision`, `target_project_id`,
`user_confirmed_objective`, `selected_context_items[]`,
`selected_file_references[]`, `open_questions[]`,
`workspace_revision`, `created_at`, `expires_at`,
`content_digest`, `owner_confirmation_digest`.
A target may have no access to a file reference; a reference is a label,
not a command nor a capability. Every free-text field has explicit length
and aggregate byte bounds (values selected after UX/resource measurements).
Every source observation is labelled `human-entered`, `provider-reported`
or `provider-output-untrusted`. Escaping/quoting rules apply to the
complete handoff, including markdown-like and tool-looking content.

**Storage**: by default only volatile draft content. If the user wants
the content to survive relaunch, the exact encrypted/consentful
content store and its retention require a separate privacy decision.
The ordinary event history stores only opaque IDs, digests, closed
status words and times, never a handoff's model output.

### 1.2 State machine

```text
DRAFT -> REVIEWING -> CONFIRMED -> PREPARING_RECEIVER -> DELIVERED
  DRAFT/REVIEWING -> DISCARDED
  REVIEWING -> NEEDS_REVISION (source changed) -> REVIEWING
  CONFIRMED/PREPARING_RECEIVER -> EXPIRED | CANCELLED | FAILED
  DELIVERED -> terminal; it does not mean the receiver succeeded
```

The owner must see *exactly the bytes* selected for the receiving
first input plus target identity/profile, current workspace and
authority difference. Confirmation binds the immutable digest.
The target starts under its normal native/managed launcher and
fresh authorisations. If launch fails, no partial received session
is described as delivered. If the source workspace or source
generation changes, the draft is stale and must be reviewed anew.

### 1.3 Tests (planned, not executed)

- **TS-HAND-01:** switching views or opening another agent with the
  same project transfers zero private conversation bytes.
- **TS-HAND-02:** mutate selected context after owner review; digest
  change invalidates consent, receiver starts nothing.
- **TS-HAND-03:** malicious handoff includes terminal escape, fake
  system command, tool grant and relative path escape; receiver gains
  no new ability; payload remains quoted untrusted data.
- **TS-HAND-04:** native→managed and managed→native label the loss of
  tool authority; managed authority does not inherit native approval.
- **TS-HAND-05:** source dies/detaches/changes revision; stale
  handoff is rejected; no plaintext copied to indefinite stores.

## 2. Compatibility adapter for OpenCode and Hermes, F4a/F4b

**No installed binaries were run.** These are data-model contracts,
not claims that current installed versions implement any action.

### 2.1 Capabilities and evidence grades

Use `ProviderCapabilityDescriptorV1` with:
`provider_id`, `provider_build_fingerprint`, `source_schema_revision`,
`capability_id`, `scope(global/project/session/system)`,
`observable_by(file/provider_protocol/CLI)`, `mutability(read-only/native_writer/direct_write/none)`,
`evidence(source_read/doc/observed)`, `evidence_date`,
`exact_version_or_range`, `limitations[]`, `requires_restart`,
`loss_of_data_risk`, `side_effects_class`.
A feature can be shown as **unverified** without enabling its mutation.
Provider identifiers and model identifiers are opaque, never parsed
as paths or shell arguments by the page.

A provider-native change is available only through a recorded,
version-verified typed operation, with fixed executable identity,
bounded fixed argument vocabulary and a secure confirmation.
Unknown version, unknown configuration schema, duplicate scope,
parser conflict or unsupported return round trip -> read-only
and an explicit reason. No direct write where DN-21/DN-31 are blocked.

### 2.2 OpenCode static sources to investigate

Source docs read on 2026-10-13, NOT pinned to a matching installation:
- https://docs.opencode.ai/docs/config/
- https://docs.opencode.ai/docs/skills/
- https://dev.opencode.ai/v2/docs/config/

The public documents describe JSON/JSONC config, multiple global/project
and optional override sources, plus project/global/compatibility skill
directories. They differ by generation (`dev`, `v2`) and cannot
be treated as one observed precedence rule. An adapter must discover
**one exact supported version**, bind the corresponding source
catalogue, handle JSONC round trip and comments, and refuse to
claim a global or effective state based on untested precedence.
The skills also overlap with Claude-compatible directories:
inventory must label their *provider and scope*, never treat one
shared disk file as separately writable independent copies.

**Read contract**: enumerate recognised files without following
unapproved symlinks, read only within configured byte/path limits,
parse JSONC losslessly or refuse mutations, mark dynamic/remote
sources `unknown` until observed, and emit typed metadata.
**Mutation contract**: a provider-native writer only if a real
version-bound command is validated; otherwise no mutation until
the direct write transaction and conflict gates pass. A file is
not safely writable merely because `opencode.jsonc` parses.

**Proposed tests** TS-ADP-OC-01..09:
01 source precedence fixture per pinned version;
02 global/project overrides and unknown remote-managed source;
03 comments/trailing commas round trip byte-for-byte for untouched
fields; 04 invalid syntax and nested symlink refused;
05 overlapping Claude-compatible skills correctly attributed;
06 MCP server records read with secret redaction;
07 plugin hooks never executed by inventory;
08 changed version invalidates write evidence;
09 refresh after external provider write flags conflict, never
overwrites without safe writer protocol.

### 2.3 Hermes as native provider vs Wyss Agent planner

Hermes upstream as a **native provider** (FR-AGT-01) runs with normal
account privileges and does not inherit Wyss Agent policy.
Hermes as an internal **planner for Wyss Agent** (DN-06=D) must
run with no bypassable effectful toolset under a demonstrated OS
boundary; typed plan proposals go through the Wyss policy controller.
Sharing a model name or executable does not merge these two trust
profiles. Install/version/source requirements are in
[17](17-WYSS-AGENT-AND-HERMES.md).

**Proposed tests** TS-ADP-HR-01..05:
01 native TUI terminal resize/input/exit;
02 native config read with version/freshness;
03 effectful planner tools blocked at OS level;
04 malicious proposal cannot forge policy approval;
05 concurrent native/planner instances never share authority or secrets.

## 3. Configuration authoring UX contract: native hook dialog

From DN-12=A and DN-13=A; **writes remain prohibited** until
the provider writer or direct transaction is separately accepted.

A graphical Config view holds non-executable typed metadata. Choosing
"New/Edit hook" opens a **native host-owned dialog** isolated from the
config webview's terminal capabilities (DN-22=B). It shows the
exact provider version, scope, source file fingerprint, event
name, matcher, command preview/editor, timeout, trust source,
potential process privileges, other affected hooks and
a predicted operation kind. A path is displayed only within
the local native review surface, never in remotely persisted logs.

Flow `READ -> DRAFT -> VALIDATE -> REVIEW -> CONFIRM -> RECHECK ->
MUTATE(if unlocked) -> VERIFY -> COMMITTED`; failure states
`UNSUPPORTED / READ_ONLY / STALE / CONFLICT / REJECTED /
WRITE_BLOCKED / ROLLBACK_REQUIRED / UNKNOWN`. Under current
planning gates the flow stops at **WRITE_BLOCKED**; a UI mockup
must not visually impersonate COMMITTED.

For provider-native operations the backend resolves a closed
operation ID and typed operands; no freeform shell fragment from
the webview may become `argv` or environment variables. Arbitrary
hook command authoring is restricted to the native editor.
For operations that need protected restoration, keep only
the separately approved vault content (when accepted), with
no plaintext log/event or unencrypted backup. An action
changing a hook while the provider holds open config files
must handle L1/L2/L5, not promise atomic replacement.

**Planned negative tests TS-CFG-17..20** (previously named but not specified):
- 17: compromised config webview injects executable text into
  native authoring IPC -> backend rejects; native dialog cannot
  be confirmed by the page.
- 18: invalid matcher/event, timeout overflow, missing executable,
  symlinked provider path and unknown version -> visible refusal,
  no provider file change.
- 19: concurrent external edit between review and recheck ->
  STALE/CONFLICT, no unreviewed overwrite; preserve previous
  provider bytes according to the *approved* transaction if any.
- 20: approved provider-native command closed-list check:
  unknown arguments or executable identity are rejected; an
  observed provider-version change invalidates evidence.
These are contracts/test plans, not proofs of safe writes.

## 4. Native Android UX: screen-state sketches

These sketches declare *interaction hierarchy*, not pixel-perfect
visual design, brand identity or approved artwork.
Only non-sensitive metadata is visible before unlock.

```text
MS-01 HOSTS                      MS-04 TERMINAL
+--------------------------+     +--------------------------+
| Wyss      Hosts          |     | Host A  Session #...     |
| Available 2 / Last sync  |     | Read / Input claim      |
| [Desktop] [Connect]      |     | [  bounded screen   ]   |
| [Notebook] [Connect]     |     | [Ctrl] [Esc] [Tab]       |
| [Add trusted host]       |     | [Keyboard] [Detach]     |
+--------------------------+     +--------------------------+

MS-02 PAIR HOST                 MS-05 PIPELINE
+--------------------------+     +--------------------------+
| Scan QR / Manual pairing |     | Run ID / Bounded plan    |
| Compare code on host     |     | Step 1 done             |
| Device key protected     |     | Step 2 waiting approval |
| Grants: Watch (default)  |     | [View] [Pause] [Stop]   |
| [Confirm on host]        |     | Budget / expiry         |
+--------------------------+     +--------------------------+

MS-03 SESSIONS                  MS-06 APPROVAL
+--------------------------+     +--------------------------+
| Grouped by project       |     | Exact managed authority |
| Running / Paused / Lost  |     | operation + scope       |
| Provider / Last event    |     | [Deny] [Step-up approve]|
| [Attach] [Inspect]       |     | Expires in ...          |
+--------------------------+     +--------------------------+

MS-07 NOTIFICATIONS             MS-08 DEVICE GRANTS
+--------------------------+     +--------------------------+
| Category preferences     |     | This device: Watch      |
| Sensitive text: none     |     | Request input privilege |
| [Open after unlock]      |     | Full-account warning    |
+--------------------------+     | [Revoke]                |
                                 +--------------------------+

MS-09 CONFIG PROPOSAL           MS-10 OFFLINE
+--------------------------+     +--------------------------+
| Read-only status         |     | Host unreachable        |
| [Prepare change]         |     | Last known at ...       |
| [Queue on host]          |     | No queued shell input   |
| Waiting local approval   |     | [Retry] [Diagnostics]   |
+--------------------------+     +--------------------------+
```

Every screen needs loading, empty, stale, denied, revoked,
accessibility and small-viewport states, plus explicit display of
the host and transport identity. Native Android design does not
reuse a PWA as a substitute. Real key-storage, OS push and
keyboard semantics remain unverified.

## 5. History of usage and model events (DN-09=C, DN-43=D)

**Candidate schema, separate from protected vault and authority's
trusted core store** (do not assume an accepted DB migration):

`ProviderMetricEventV1`: event ID, local host identifier,
provider opaque ID, model opaque ID (nullable), session opaque ID,
turn ID (nullable and never fabricated), metric kind (closed set:
input tokens, output tokens, cached tokens, limit percentage,
window reset, provider-reported aggregate), metric value with
type/bounds, metric grain (account/session/turn),
provider report timestamp (optional), observed-at timestamp,
source message/type, runtime generation (nullable), completeness,
`reported` vs `inferred` (report only persists).
`ObservedModelEventV1` stays as proposed in ADR 0031 with
session/turn-grain distinctions. Model selection requests are
separate owner intents, never treated as observed model evidence.

No prompt, terminal bytes, provider output, command, credentials,
raw env, absolute path, account subscription details unless
separately opted in, or model's free-text output may be stored.
Metric append is idempotent by origin event ID/generation, and
out-of-order events are handled without double-counting.
Do not sum heterogeneous provider billing units; there is no
exchange rate from subscription usage to API currency.
No guaranteed price reporting; always label unknown as unknown.

**Retention policy candidate:** retain only bounded typed metadata,
with a visible opt-in/on-device history preference, user-driven
export/reset and a future policy for size pressure. Automatic
deletion rules for this **analytics metadata** must be specified
and approved separately; under no circumstance can a usage-history
quota delete protected vault entries, transaction recovery
records or user-selected backups. Until policy accepted, no
persistent analytics store or migration is authorised.

**Planned tests** TS-MET-01..08:
01 provider omission stays unknown;
02 session report not copied as turn-specific fact;
03 reroute references correct running turn/generation;
04 duplicate/late token notification does not inflate history;
05 two provider units never auto-convert to money;
06 selected model not rewritten as actual observation;
07 generated history/export/diagnostics contain no forbidden text;
08 quota/full disk never purges protected entries or masks failure.

## 6. Interface and accessibility acceptance

S15 (agent actions), S16 (remote grants) and S17 (pairing)
need mockups and error-state maps tied to 13 and 16.
Use the Android MS-01..10 above as *state coverage*, not
approved final visual art. Desktop wireframes must preserve
two distinct capability surfaces (config vs terminal) and
identify which plane an action affects.
Apply DN-34 three themes including terminal ANSI colour palette,
DN-35 opt-in notification payload limits and DN-39 keyboard
conflict/focus/IME checks.

**Planned tests** TS-UIX-01..07:
01 theme token contrast pairs; 02 keyboard-only
flow and IME; 03 screen-reader labels and focus
after detach; 04 locked/denied/stale/offline messages;
05 no secret-bearing OS notification; 06 persisted layout
does not trigger launch; 07 at 320 px Android
viewport, all terminal input and approve controls remain
operable without accidental activation.

## 7. Known unavailable evidence

- Pinned installed OpenCode/Hermes and actual provider-specific
  config precedence; public versioned docs are inconsistent.
- Runtime correctness of untrusted ANSI and mobile screen parser.
- Native dialog isolation from config view and real OS effects.
- Contract of protected vault, synced paths and snapshots.
- Usage event provider fields, freshness, retention and resource
  measurements.
- Real visual-design review, accessibility on WebKitGTK and Android,
  implementation and independent security review.

This document is deliberately explicit about those unavailable
facts; no unsupported implementation shortcut may infer them.
