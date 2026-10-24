# M2 contract: Native parity and configuration closure

Status: frozen engineering contract, 2026-10-23. Source baseline: verified
`origin/main` at `def69fe35ddfe8b696b65a12d2f2babe296c8247`.

This contract turns the accepted product requirements into test oracles for M2. It
does not reopen accepted ADRs. A command-line parse check proves syntax only; it does
not prove provider resolution, account entitlement, reload, or effective use.

## Boundary

M2 closes Native Claude Code/Codex behavior and configuration work under
FP-SES-01/AC-FP-SES-01, FP-CFG-01..05/AC-FP-CFG-01..05, and the Native portion of
FP-MOD-01/AC-FP-MOD-01. It retains the canonical FR-NAT, FR-CFG, FR-HRS, FR-MOD and
FR-SEC identifiers mapped below. Native model/effort requests are in scope here.

Managed model/effort selection, Claude Managed, authority protocol extensions,
Managed capability changes, and concurrency above the accepted limit belong to M3/M5.
Codex per-session hook/skill exclusions, Codex project skills, Codex hook mutation,
and project trust writes remain unsupported and must stay visible as such.

Tests and automated GUI runs use disposable configuration roots, temporary state and
fixture providers. No real provider prompt, live configuration mutation, host or
authority restart, credential change, migration, or deployment is authorized by this
contract.

## Frozen oracles

### M2-01 — Native launch and provider fidelity

**Requirements:** FP-SES-01/AC-FP-SES-01; FR-NAT-01..04, FR-NAT-11; ADR0035.

- **Positive:** For each installed catalogue entry, a fixture launch receives the
  exact closed executable, argv words, environment and selected working directory;
  provider-owned prompts and CLI behavior remain available. The UI labels the plane
  Native and shows the resolved executable/version, actual directory and profile
  effects before launch. For an effectful profile, the launch shell resolves the
  provider command after its startup files, checks that binary's supported
  `--version`, and executes that same resolved path with the reviewed argv.
- **Negative:** Cancel, missing executable, untrusted/changed project, changed
  provider version, or an unsupported profile field starts no child. No request can
  supply an executable, arbitrary argv, environment or path. No global provider file
  is swapped to emulate a session profile. If shell resolution reports an unsupported
  command form or the resolved build differs from preview, the effectful profile is
  refused before the provider receives profile values.
- **Evidence:** Rust fake-process/recipe tests plus native UI/controller tests; record
  provider version and the exact supported CLI/configuration interface separately
  from any claim about consumption.
- **Owners:** `src-tauri/src/native/{catalogue,recipe,service,commands}.rs`,
  `src-tauri/src/host/{protocol,core}.rs`, `src/lib/features/native/`,
  `tests/native*.test.ts`.

### M2-02 — Profile inheritance, precedence and version pinning

**Requirements:** FP-CFG-03/AC-FP-CFG-03; FR-NAT-03; ADR0035/ADR0048.

- **Positive:** Resolve parents before children; nearer scalar values win and lists
  combine by their documented rule. Selection is explicit session choice, then the
  selected project's default, then As configured. A launch records the resolved
  profile revision; the preview token binds the complete inherited resolution and
  launch plan. The Native view tracks the selected profile and each inherited parent
  revision. A direct or inherited edit invalidates the old preview, hides its details,
  disables Start and requests one fresh preview for the changed chain. Only the latest
  matching response may be displayed or used to start. A stale token is refused.
  Restart validates and starts the replacement before stopping the old session;
  failed validation or spawn leaves the old session usable. Later edits/default
  changes do not relabel or alter a running session. Saved versions and import/export
  preserve the accepted bounds.
- **Negative:** Missing parent/version, cycle, depth overflow, provider mismatch, or
  any unsupported field prevents launch with a specific reason. Import never grants
  a capability. Codex hook/skill exclusions remain refusals. A changed profile,
  inherited profile or selected default between preview and start/restart cannot use
  the earlier preview token on an updated host. An older asynchronous preview response
  cannot replace a newer response. A failed preview keeps Start disabled and exposes
  an explicit retry; unrelated state changes do not retry automatically. A legacy host
  without the token feature may continue non-effort Native profiles with an explicit
  warning, but may apply the latest saved profile instead of the revision shown in
  preview; this path does not satisfy exact M2 preview/revision binding. An effort-
  bearing profile is refused unless the host can preserve it.
- **Evidence:** Rust profile/store/service tests and Native view tests cover direct and
  inherited profile edits while navigating between Workspace and Profiles, stale
  Start gating, latest-response ordering, explicit retry without reactive request
  loops, and use of the refreshed token. Fake protocol-1 HostLink integration tests
  cover non-effort legacy preview/start forwarding and refusal of inherited effort.
  Native restart tests cover concurrent sessions with different revisions, changed
  defaults and restart.
- **Owners:** `src-tauri/src/native/{profile,profiles,service,registry}.rs`,
  `src/lib/features/native/`, `tests/native*.test.ts`.

### M2-03 — Native model and reasoning-effort requests

**Requirements:** FP-SES-01/AC-FP-SES-01; Native FP-MOD-01/AC-FP-MOD-01;
FR-NAT-03, FR-MOD-03, FR-MOD-05, FR-MOD-07, FR-MOD-10, FR-MOD-12,
FR-MOD-14; ADR0043/ADR0048.

- **Positive:** Keep configured file values, profile-requested values, provider-
  reported values and actually observed/effective values distinct, with source and
  scope. Claude receives its documented `--model`/`--effort`; Codex receives `-m`
  and its documented per-process `-c model_reasoning_effort=...` override. A request
  changes no global file. If no provider report exists, show “not reported/unknown”.
- **Negative:** Reject malformed model/effort words and unsupported Claude effort
  levels before spawning. An older host without `profile_effort` refuses an
  effort-bearing profile; a profile without effort remains compatible. Never infer
  entitlement, effective use, or a reroute from the requested/configured value. A
  legacy host without the M2 plan-token feature refuses an effort-bearing profile;
  non-effort profiles remain launch-compatible with a visible preview-binding warning.
- **Evidence:** Exact argv/override fixtures, old/new host protocol tests, UI fact
  separation tests, and read-only local version/help evidence. Syntax evidence is
  not provider-consumption evidence. This oracle grants no Managed selection fields.
- **Owners:** `src-tauri/src/native/{profile,commands,service}.rs`,
  `src-tauri/src/host/{protocol,core}.rs`, `src/lib/features/native/`,
  `src/lib/contracts/native.ts`, `tests/native*.test.ts`.

### M2-04 — Real configuration inventory and provenance

**Requirements:** FP-CFG-01/AC-FP-CFG-01; FR-CFG-01..04, FR-CFG-12..14,
FR-CFG-16; FR-HRS-01, FR-HRS-06, FR-HRS-12.

- **Positive:** Inventory only the provider sources and scopes accepted for that
  provider. Each value separately reports persisted, provider-resolved and
  session-effective state, source, scope, provenance and freshness. External changes
  refresh or mark the inventory stale; a bounded rescan recovers watcher overflow.
- **Negative:** Malformed, unreadable, linked, generated, provider-owned or unknown
  formats are reported as such and do not suppress other sources or become writable.
  Secret values never reach the page, diagnostics, journal or events. No provider is
  launched to populate inventory.
- **Evidence:** Isolated source fixtures for valid, conflicting, malformed, replaced,
  missing, unreadable and secret-marker inputs; assert exact source labels and no
  content leakage.
- **Owners:** `src-tauri/src/agentconfig/{inventory,reader,claude,codex,opencode,redact}.rs`,
  `src/lib/features/configuration/`, `src/lib/contracts/configuration.ts`,
  configuration tests.

### M2-05 — Hooks, skills, MCP, plugins and unsupported capabilities

**Requirements:** FP-CFG-02/AC-FP-CFG-02, FP-CFG-05/AC-FP-CFG-05;
FR-HRS-02..10, FR-HRS-12; FR-CFG-14, FR-CFG-19; ADR0041/0042/0044/0047/0050.

- **Positive:** Each provider/action pair uses only its accepted native switch or
  reviewed writer. Claude hooks/skills and supported Claude project files, Codex
  skills/MCP switches, and OpenCode inventory/plugin behavior retain their
  provider-specific semantics. Executable hook/MCP changes use the native editor and
  confirmation.
- **Negative:** Codex hooks, unsupported per-session Codex hook/skill exclusions,
  Codex project skills, project trust writes, unknown plugin switches, and formats
  without a safe writer are read-only/refused with a reason. No cross-provider
  capability is inferred from a similarly named feature.
- **Evidence:** Provider × operation × scope fixtures assert each supported and
  refused cell, capability permission manifests, and native confirmation behavior.
- **Owners:** `src-tauri/src/agentconfig/{claude,codex,opencode,codex_plan,author,codex_author,project}.rs`,
  `src-tauri/src/configuration.rs`, `src/lib/features/configuration/`,
  configuration tests and capability manifests.

### M2-06 — Global, project and session boundaries

**Requirements:** FP-CFG-01..03/AC-FP-CFG-01..03; FR-CFG-05, FR-CFG-11,
FR-NAT-03; ADR0035/0047/0050.

- **Positive:** Global administration previews global file effects; project
  authoring is limited to the currently selected approved project; a native session
  uses its pinned profile and provider-supported per-session overrides. Switching
  projects rebuilds inventory and clears plans for the prior project.
- **Negative:** A project outside the approved home boundary, a provider/credential
  directory, link, or changed project identity is refused. A session profile never
  writes to global settings; an A plan or Undo cannot write into B.
- **Evidence:** Temporary home tree with account/project fixtures, two profiles and
  two sessions; switch A→B during plan/apply and verify exact bytes and records in
  both scopes.
- **Owners:** `src-tauri/src/agentconfig/{project,roots,service,changes,writer}.rs`,
  `src-tauri/src/native/{profile,service}.rs`, native/configuration UI tests.

### M2-07 — Safe authoring, plans, apply and Undo

**Requirements:** FP-CFG-02/AC-FP-CFG-02; FR-CFG-05..10, FR-HRS-03..05,
FR-HRS-07, FR-HRS-09; FR-SEC-01..06, FR-SEC-10; ADR0034/0042/0044/0045/0047/0050.

- **Positive:** Read → draft → validate → review → required native confirmation →
  identity/revision recheck → journal intent → accepted writer → read-back → outcome.
  Undo restores exact bytes/absence/metadata only when its kind-specific expected
  postimage holds. Diverged restoration follows its accepted per-kind rule; no
  universal exact-Undo promise is made.
- **Negative:** Cancel, stale plan, outside writer, symlink/hardlink, ACL/ownership
  failure, malformed format, disk full, journal/vault failure or project switch
  produces no unauthorized overwrite. Multi-file partial progress is reported per
  file; unsupported atomicity is never claimed.
- **Evidence:** Fault-injected plan/apply/Undo tests at each write/journal/vault
  boundary; races with outside writers; exact before/after bytes, identity, mode,
  journal and recovery assertions.
- **Owners:** `src-tauri/src/agentconfig/{changes,plan,writer,journal,restore,vault,fsutil,acl}.rs`,
  native dialog and configuration UI/controller tests.

### M2-08 — Imported templates and encrypted restoration

**Requirements:** FP-CFG-04/AC-FP-CFG-04; FR-CFG-15..17, FR-SEC-08, FR-SEC-10;
ADR0034/0045/0046/0050.

- **Positive:** Instruction, hook and MCP template import is domain/version checked,
  decrypts only in a native surface, labels content untrusted, and prefills a form or
  editor. It produces no write until the user completes a normal plan and required
  native confirmation. Applied instruction Undo is always sealed; record restoration
  follows its accepted entry-kind contract.
- **Negative:** Wrong passphrase/key, wrong domain, tampering, malformed/oversized
  payload, kind mismatch, invalid provider data, or cancellation changes nothing and
  reveals no plaintext to the page or logs. A missing key never overwrites an
  existing key or the only recovery copy.
- **Evidence:** Disposable-vault round trips and domain-confusion/tamper/fault tests;
  native editor/form → plan → confirm → apply → Undo GUI evidence. Real provider
  consumption is a separate result.
- **Owners:** `src-tauri/src/agentconfig/{config_template,instruction_template,vault,restore,changes}.rs`,
  `src-tauri/src/configuration.rs`, `src/lib/features/configuration/`, tests.
- **Separate owner decision OD-M2-01:** choose whether to remove ADR0034's disclosed
  private-plaintext fallback for entries classified secret-free when Secret Service
  is unavailable. Until that policy extension is separately accepted, preserve ADR0034
  exactly; do not silently prohibit or replace its fallback. This owner decision does
  not block independent M2 engineering and remains a release gate if the all-sealed
  final target is required.

### M2-09 — Provider-version capability validation

**Requirements:** FP-CFG-05/AC-FP-CFG-05; FR-CFG-14, FR-CFG-19,
FR-MOD-10, FR-MOD-14; ADR0035/0048.

- **Positive:** Every exposed action is tied to provider, observed CLI build or
  justified version range, mode/profile, scope, operation, format, evidence source,
  date, limits and reload semantics. A provider upgrade invalidates affected
  evidence without changing files. The current local CLI version is reported as an
  observation, not a general support claim.
- **Negative:** An unsupported/unknown version leaves uncertain configuration
  read-only and shows a reason; Native CLI launch may continue with an explicit
  warning if its closed launcher contract remains valid. Imported capability data
  cannot add a path, command, schema or execution permission; no silent fallback.
- **Evidence:** The manifest records exact build, checked date, operations, interface,
  format, scope, reload semantics, source and limitations. Version-bound capability
  fixtures, upgrade/invalidation tests, malformed/imported-manifest refusal tests and
  a read-only local version/help record establish syntax only. No inference or config
  mutation is used to qualify syntax; exact-build provider resolution/entitlement and
  effective use remain separate evidence.
- **Owners:** `src-tauri/src/native/{catalogue,profile,service}.rs`,
  `src-tauri/src/agentconfig/{inventory,service}.rs`, provider matrix and UI status.

### M2-10 — External changes, failures, concurrency and recovery

**Requirements:** FP-CFG-01..04/AC-FP-CFG-01..04; FR-CFG-04, FR-CFG-07..10,
FR-CFG-17; ADR0034/0045/0047/0050.

- **Positive:** External replacement invalidates plans; watcher overflow triggers a
  bounded rescan. Concurrent in-app applies/project changes serialize according to
  the accepted writer lock. Interrupted intents recover from journal plus disk
  identity/hash; ambiguous results are retained and labelled for review.
- **Negative:** A stale plan, outside writer, symlink/ACL change, missing vault key,
  missing or corrupt journal, failed read-back, interrupted write or occupied create
  destination never triggers a blind retry or force restore. Recovery preserves
  unexplained files/records.
- **Evidence:** Deterministic races and crash/fault injection at each transaction
  step; missing-key, stale-plan, ACL, outside-writer and recovery-restart fixtures.
- **Owners:** `src-tauri/src/agentconfig/{changes,writer,journal,restore,vault,project}.rs`
  and recovery/concurrency tests.

### M2-11 — Native GUI end-to-end workflows

**Requirements:** FP-SES-01, FP-CFG-01..04 and their ACs; FR-NAT-01..04,
FR-CFG-06, FR-HRS-04/09, FR-SEC-01..03; journeys JF-02, JF-03 and JF-06..08.

- **Positive:** In the native desktop, select Claude/Codex Native, project and
  profile; inspect preview; launch a fixture provider; inspect account/project/session
  configuration; author, import a template, review, confirm, apply and Undo. The GUI
  distinguishes requested, persisted, resolved and observed facts and lists the exact
  unsupported cells.
- **Negative:** Dialog cancellation, stale project/version, unsupported profile,
  malformed inventory and failed apply leave fixture files/processes unchanged or
  explicitly recoverable. No provider prompt is auto-answered.
- **Evidence:** The 2026-10-23 isolated Tauri runs in
  `docs/acceptance/evidence/m2-gui-2026-10-23/` use private HOME/XDG roots, a private
  display/bus/keyring and fixture CLIs. Saved screenshots and SHA-256 checks cover
  preview, chooser/plan cancellation, fixture start/restart, instruction authoring,
  sealed template export/import, native confirmations and Undo; run records assert no
  leftover processes. Web component tests and fake HostLink Rust integration tests
  are recorded separately. Real provider workflow evidence remains separate and
  requires explicit authorization.
- **Owners:** `src/lib/features/{native,configuration,models}/`, desktop command
  bridges, native dialogs, GUI harness and the Rust owners named above.

### M2-12 — Preservation, resource costs and regressions

**Requirements:** FP-NFR-03/AC-FP-NFR-03; FR-NAT-11, FR-CFG-07..10,
FR-SEC-03..06, FR-SEC-10; ADR0034/0035/0040.

- **Positive:** Build/test/rehearsal uses isolated stores and fixture providers; the
  existing installation, host, authority, sessions, provider configuration,
  credentials and provider history remain untouched. Record configured inventory,
  profile, journal and restoration ceilings plus guard/recovery timeouts; retain
  existing regression coverage.
- **Negative:** No automatic paid inference, global configuration rewrite, live
  process restart/signal, migration, secret export, public telemetry or unreviewed
  binary deployment. A test that attempts an undeclared path or provider command
  fails closed.
- **Evidence:** Targeted Rust, TypeScript/Svelte, configuration/security/recovery and
  disposable GUI suites; repository diff; explicit evidence that no live provider
  mutation or inference was run. Exclude dangerous lifecycle and paid tests.
- **Owners:** All M2 owners above; validation scripts and acceptance record.

## Delivery interpretation

Engineering can be reported complete only when every applicable oracle has passing
reproducible fixture evidence. Release/adoption remains blocked while authorized real
provider GUI acceptance or OD-M2-01 is unresolved. The final delivery must label each
unsupported capability and separate configured, requested, provider-resolved and
actually observed/effective values. Do not merge or deploy automatically.
