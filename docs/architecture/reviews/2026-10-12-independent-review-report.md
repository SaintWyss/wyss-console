# Independent architecture review, 2026-10-12: the reviewer's report

This file is the reviewer's output, unedited except for one mechanical change: 62 links that
pointed at absolute paths on the reviewing machine were turned into repository-relative paths
in code marks. Nothing else was added, removed or reworded. What was done about each finding is
in [25](../25-INDEPENDENT-REVIEW.md).

Reviewer: a different model from the author's (GPT-6 Luna, maximum reasoning effort), run once
through Codex in a read-only sandbox, with no access to the author's reasoning. It was told to
look for reasons to reject. It read the documents as they stood on `main` at `537eabc` plus the
first draft of ADR 0029, before any correction of this pass. The references it gives to
provider documentation on the web were not re-checked by the author.

---

### Findings

IR-01 | BLOCKER | AREA 1  
Where: 04-TARGET-ARCHITECTURE.md, AD-05 (`docs/architecture/04-TARGET-ARCHITECTURE.md:119`), 13-UX-UI-SPECIFICATION.md, J4 (`docs/architecture/13-UX-UI-SPECIFICATION.md:269`), 14-DATA-PERSISTENCE-AND-RECOVERY.md, §5 (`docs/architecture/14-DATA-PERSISTENCE-AND-RECOVERY.md:92`), 20-VERIFICATION-AND-ACCEPTANCE.md, TS-CONC-01 (`docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md:189`), and ADR 0028 §8; the accurate race is described in 07-CONFIGURATION-ENGINE.md, §5.5 (`docs/architecture/07-CONFIGURATION-ENGINE.md:215`).  
Claim or gap: Several documents still imply a provider write racing Wyss is detected by read-back or recoverable from a backup. TS-CONC-01 tests two Console windows, not a provider process.  
Failure scenario: Wyss checks hash A; the provider writes B; Wyss renames its file C over B; read-back sees C and reports success. The provider’s change is lost, and a pre-write backup contains A, not B.  
Correction: Remove the detection/recovery claims and block writes unless provider activity is excluded by a reliable, provider-cooperative mechanism. If that cannot be guaranteed, make mutation unavailable while the provider may write.  
Confidence: high — the sequence is stated in §5.5 and contradicted by the other cited documents; no race was run.

IR-02 | BLOCKER | AREA 2  
Where: 07-CONFIGURATION-ENGINE.md, §§5.1, 5.4, 9 (`docs/architecture/07-CONFIGURATION-ENGINE.md:144`), 14-DATA-PERSISTENCE-AND-RECOVERY.md, §§3–4 (`docs/architecture/14-DATA-PERSISTENCE-AND-RECOVERY.md:37`), 20-VERIFICATION-AND-ACCEPTANCE.md, TS-REC-01–05 (`docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md:91`), and ADR 0029 §§2, 6.  
Claim or gap: ADR 0029 calls an in-place provider-file replacement “one file and one atomic replacement,” but the Console’s pointer record and journal are separate writes. Its no-orphan claim does not cover those stores.  
Failure scenario: A crash after writing the pointer record but before replacing the provider file leaves a record for an entry that is still active. A crash after replacement but before journaling leaves an applied change without a completed history entry. The proposed record cannot be reconciled safely unless its intent and outcome semantics are defined.  
Correction: Specify a durable write-ahead intent and outcome protocol, including operation ID, source identity, before/after hashes and entry identity; define fsync ordering and recovery for every crash cut. On ambiguous recovery, preserve the record and require explicit resolution.  
Confidence: high — the documents specify separate stores and write ordering but no complete intent/outcome protocol; no fault injection was run.

IR-03 | HIGH | AREA 3  
Where: 11-SECURITY-THREAT-MODEL.md, §§5.4–6 (`docs/architecture/11-SECURITY-THREAT-MODEL.md:174`), 19-IMPLEMENTATION-ROADMAP.md, F2a (`docs/architecture/19-IMPLEMENTATION-ROADMAP.md:150`), 20-VERIFICATION-AND-ACCEPTANCE.md, TS-SEC-07 (`docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md:131`), and ADR 0029 §§1, 4.  
Claim or gap: The older design lets a heuristic decide which hook entries may be persisted, then lets the owner choose an acceptable measured false-negative rate. A labelled corpus cannot establish that arbitrary configuration contains no secrets. ADR 0029 rejects that persistence rule, but remains proposed.  
Failure scenario: A credential embedded in an unfamiliar command, header, or environment value passes the detector; disabling, undo, or export then leaves the credential in Console-managed durable data.  
Correction: Do not persist hook, MCP, or plugin entry content based on a classifier. Accept ADR 0029 or another explicit policy first, then remove classifier-based persistence and export from the requirements, tests, and roadmap. Codex’s current config reference includes bearer-token fields in MCP configuration, illustrating why field-pattern heuristics cannot be treated as a guarantee ([Codex config reference](https://developers.openai.com/codex/config-reference)).  
Confidence: high — the policy conflict and test threshold are explicit; the current Codex reference confirms sensitive values can occur in configuration.

IR-04 | HIGH | AREA 3  
Where: 07-CONFIGURATION-ENGINE.md, §5.4 (`docs/architecture/07-CONFIGURATION-ENGINE.md:207`), 08-RULES-HOOKS-SKILLS-MCP.md, §3.4 (`docs/architecture/08-RULES-HOOKS-SKILLS-MCP.md:106`), 09-PROFILES-AND-ORCHESTRATION.md, §3.1 (`docs/architecture/09-PROFILES-AND-ORCHESTRATION.md:40`), and ADR 0029 §§6–7.  
Claim or gap: ADR 0029 says the Console keeps no content, yet permits a whole-file temporary copy beside the provider file. The roadmap also retains edit/delete restoration records, and profile generation can copy hook entries.  
Failure scenario: A file containing a secret is copied to a temporary sibling, then the process or machine crashes before cleanup. The secret remains on disk and may be captured by snapshots or replication; cleanup only at a later Console start does not make the copy non-durable. A generated profile or undo record can create another content-bearing copy.  
Correction: Treat temporary files and generated profiles as persistence. Remove content-bearing undo/export paths under the no-persistence policy; make profiles that require copied hooks unavailable unless an explicitly approved, crash-safe ephemeral storage design is accepted.  
Confidence: high — the ADR explicitly permits whole-file temporary copies and the older documents specify other content-bearing paths; code was not changed or run.

IR-05 | HIGH | AREA 3  
Where: ADR 0029, §§3 and 5 (`docs/adr/0029-secrets-restoration-data-and-configuration-persistence.md:86`), especially V1–V7.  
Claim or gap: The in-place ladder can work only if an entry is moved to provider-ignored data or neutralized by a provider-supported field. A hook left in the provider’s active hook structure remains executable. The tests are too broad to establish safety per provider version: V2 exercises an event, V4 samples provider rewrites, and V7 checks only V1–V3 on the other computer.  
Failure scenario: A test passes for one hook shape, but another event or handler uses different semantics, or a different provider write path drops the “disabled” data. The Console reports the hook disabled or restorable when it is active or has become unrecoverable.  
Correction: Validate each storage form against every supported hook event and handler shape, all relevant provider write paths, and each supported installation. Tie results to exact versions and configuration forms; otherwise keep per-hook disable unavailable. Claude’s current documentation describes a coarse `disableAllHooks` switch and no general per-hook disable control, so an inert per-entry representation requires direct evidence ([Claude Code Hooks](https://code.claude.com/docs/en/hooks)).  
Confidence: high — the test scope is stated in the ADR; compatibility with actual provider behavior remains unverified.

IR-06 | HIGH | AREA 4  
Where: 07-CONFIGURATION-ENGINE.md, §§5.2–5.4 (`docs/architecture/07-CONFIGURATION-ENGINE.md:185`), 20-VERIFICATION-AND-ACCEPTANCE.md, TS-CFG-06, TS-CFG-16 (`docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md:57`), and src-tauri/src/settings/store.rs (`src-tauri/src/settings/store.rs:276`).  
Claim or gap: The design promises preserved mode and owner while pointing to a settings writer that creates mode-0600 temporary files and renames them; it does not preserve the replaced file’s metadata. The implementation is for private preferences, not provider configuration. Round-trip tests also cover only a narrow TOML comment case, not the claimed JSON, JSONC, and YAML behavior.  
Failure scenario: A rewrite silently changes access permissions or loses ACLs and extended attributes. A parser round trip that changes escaping, ordering, or layout either reformats user data or unexpectedly makes a source read-only.  
Correction: Define a provider-file writer that preserves required metadata or refuses files whose metadata it cannot preserve. Add byte-preservation fixtures for every supported format and edge case, or restrict mutation to edits that preserve every untouched byte.  
Confidence: high — source confirms the settings writer uses mode 0600; the architecture’s broader preservation claims are proposals, not verified behavior.

IR-07 | HIGH | AREA 5  
Where: 07-CONFIGURATION-ENGINE.md, §§4.2–4.3, 5.4 (`docs/architecture/07-CONFIGURATION-ENGINE.md:94`), 08-RULES-HOOKS-SKILLS-MCP.md, §7 (`docs/architecture/08-RULES-HOOKS-SKILLS-MCP.md:180`), 20-VERIFICATION-AND-ACCEPTANCE.md, TS-FS-06 (`docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md:63`), and src-tauri/src/control/paths.rs (`src-tauri/src/control/paths.rs:20`).  
Claim or gap: Path checks walk names and then later operate on names; they do not anchor the whole mutation to checked directory descriptors. Provider config files do not receive the strict `nlink == 1` check used for private control files. TS-FS-06 tests a link replacing the target but not parent substitution, hardlinks, or metadata loss.  
Failure scenario: Between validation and rename, a symlink or ancestor directory changes; the write reaches a different file. A hardlinked source is replaced at one name, silently breaking the user’s expectation that the shared content changed.  
Correction: Anchor reads and renames to validated directory descriptors, re-check device/inode identity at mutation, and refuse hardlinked, ACL-bearing, extended-attribute, or immutable files unless the writer preserves their semantics.  
Confidence: high — the cited path code uses path-based checks and the tests omit these cases; no concurrent filesystem mutation was run.

IR-08 | HIGH | AREA 6  
Where: 04-TARGET-ARCHITECTURE.md, §§2–3 (`docs/architecture/04-TARGET-ARCHITECTURE.md:108`), 11-SECURITY-THREAT-MODEL.md, §4 (`docs/architecture/11-SECURITY-THREAT-MODEL.md:100`), and 24-ARCHITECTURE-REVIEW.md, RV-01 (`docs/architecture/24-ARCHITECTURE-REVIEW.md:76`).  
Claim or gap: A native dialog is a separate consent surface, not a security boundary against a compromised page. The page can send input to the user’s terminal and bypass the configuration command’s dialog.  
Failure scenario: Compromised page code types a configuration edit into a terminal shell; no configuration dialog appears.  
Correction: Describe the dialog as protection against mistakes and defects confined to the configuration view. Do not claim it protects against a compromised page; specify a separate webview and terminal trust boundary. Bind every approval to an immutable plan and source revision.  
Confidence: high — the self-review’s terminal-input scenario matches the architecture’s own trust boundaries; no application code was run.

IR-09 | BLOCKER | AREA 7  
Where: 09-PROFILES-AND-ORCHESTRATION.md, §§3–4 (`docs/architecture/09-PROFILES-AND-ORCHESTRATION.md:25`), 19-IMPLEMENTATION-ROADMAP.md, F3c (`docs/architecture/19-IMPLEMENTATION-ROADMAP.md:246`), and 24-ARCHITECTURE-REVIEW.md, RV-06 (`docs/architecture/24-ARCHITECTURE-REVIEW.md:77`).  
Claim or gap: Claude profiles remove the user source to subtract a merged global hook; whether this also removes user permission rules and skills remains unresolved. F3c does not require ADR 0029’s DN-20 decision.  
Failure scenario: A profile omits the user source to remove one hook and starts without the user’s blocking hooks or permission rules, without a plan-bound security confirmation.  
Correction: Block F3c until provider-specific source-exclusion effects are enumerated. Resolve against the exact source revision; preserve mandatory security controls or show the full omitted-control diff and require explicit native confirmation each time that diff changes. Claude’s settings documentation confirms settings and permission rules have precedence and merge behavior, but does not verify the design’s `--setting-sources` consequences ([Claude Code settings](https://code.claude.com/docs/en/settings)).  
Confidence: high — the unresolved question is explicit and the profile roadmap omits DN-20; exact CLI behavior remains unverified.

IR-10 | HIGH | AREA 8  
Where: 07-CONFIGURATION-ENGINE.md, §§3 and 6 (`docs/architecture/07-CONFIGURATION-ENGINE.md:72`), 13-UX-UI-SPECIFICATION.md, J4 (`docs/architecture/13-UX-UI-SPECIFICATION.md:269`), and 20-VERIFICATION-AND-ACCEPTANCE.md, real-provider acceptance (`docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md:206`).  
Claim or gap: The architecture correctly distinguishes persisted from effective state, but J4 treats read-back as evidence that the provider’s hook list changed. File contents do not prove provider-effective or session-effective state.  
Failure scenario: The file contains the intended disabled state while a running session retains old hook behavior, or the provider ignores the stored entry; the UI calls the result a defect or implies runtime state from file read-back.  
Correction: Show provider/session state as `unknown` unless the provider supplies a runtime observation. Separate “file verified” from “provider reported” and “session observed”; make acceptance require the corresponding live evidence.  
Confidence: high — the contradictory claim is in J4; the design itself admits unknown states.

IR-11 | HIGH | AREA 9  
Where: 10-MANAGED-RUNTIME-AND-SECURITY.md, §§2, 4–5 (`docs/architecture/10-MANAGED-RUNTIME-AND-SECURITY.md:20`), src-tauri/src/adapters/codex/safety.rs (`src-tauri/src/adapters/codex/safety.rs:228`), and ADR 0016.  
Claim or gap: Managed Inference requires zero enabled external hooks and zero MCP servers, so it cannot use the user’s global hooks or MCP settings. Candidate isolation I2 replaces `/etc/codex`; I4 preserves it, but its tests do not establish enforcement across all host and remotely managed requirements. The current reflection check skips `features.hooks` and relies on the live hook list.  
Failure scenario: A user expects global hooks/MCP in Inference, or chooses an isolation that hides a system requirement; the managed session runs with different settings or policy than expected.  
Correction: State that global user hooks and MCP are absent from Inference. Before lifting the gate, test the complete supported matrix of user, system, and remotely managed policy and MCP configurations on the exact runtime; never hide host policy without an explicit accepted decision.  
Confidence: high — the code checks and candidate designs were read; current Codex documentation describes hook configuration in both `hooks.json` and inline config and admin-enforced requirements ([Codex hooks](https://developers.openai.com/codex/hooks), [Codex config reference](https://developers.openai.com/codex/config-reference)).

IR-12 | HIGH | AREA 10  
Where: 07-CONFIGURATION-ENGINE.md, §7 (`docs/architecture/07-CONFIGURATION-ENGINE.md:275`), 15-PERFORMANCE-ENGINEERING.md, M16 (`docs/architecture/15-PERFORMANCE-ENGINEERING.md:15`), and 24-ARCHITECTURE-REVIEW.md, RV-20 (`docs/architecture/24-ARCHITECTURE-REVIEW.md:107`).  
Claim or gap: Per-file limits are proposed, but global watch count, event-queue capacity, concurrent rescan count, and a bounded idle-work budget are not specified. RV-20 proposes 512 watches but does not bound kernel event pressure or rescan contention.  
Failure scenario: Many projects or skill directories exhaust watch limits or generate event storms; overflow triggers repeated whole-source scans and configuration state stays stale or consumes excessive CPU and I/O.  
Correction: Set global caps for watched paths, queued/coalesced events, concurrent scans, and scan frequency. Specify the visible degraded mode and measure idle cost and storm recovery at the cap.  
Confidence: high — the documents leave these limits open; the proposed performance measurements have not been run.

IR-13 | HIGH | AREA 11  
Where: 06-AGENT-ADAPTERS.md, provider specifications and evidence (`docs/architecture/06-AGENT-ADAPTERS.md:38`), 12-MODEL-MANAGEMENT.md (`docs/architecture/12-MODEL-MANAGEMENT.md:1`), and 20-VERIFICATION-AND-ACCEPTANCE.md, §11 (`docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md:206`).  
Claim or gap: Adapter and model behavior depends on provider versions, file sources, precedence, and reload semantics, yet many claims are `[D]`, `[O]`, or `[N]`; the evidence is not a complete per-version contract.  
Failure scenario: A provider adds a config source or changes a schema; Wyss omits a hook/MCP/model setting from inventory or writes an edit that does not mean what the adapter assumes.  
Correction: Maintain a versioned evidence manifest per provider and capability, backed by primary documentation and fixtures for every source and precedence rule. Refuse mutation until that capability has evidence for the exact version.  
Confidence: high — the architecture marks material assumptions unverified; current Codex documentation includes config forms and secret-bearing MCP fields not safely reducible to a narrow inventory without version-specific evidence ([Codex config reference](https://developers.openai.com/codex/config-reference)).

IR-14 | HIGH | AREA 12  
Where: 04-TARGET-ARCHITECTURE.md, AD-03 and AD-12 (`docs/architecture/04-TARGET-ARCHITECTURE.md:117`), 07-CONFIGURATION-ENGINE.md, §§5.2, 8–9 (`docs/architecture/07-CONFIGURATION-ENGINE.md:185`), 08-RULES-HOOKS-SKILLS-MCP.md, SM-ENTRY and §3 (`docs/architecture/08-RULES-HOOKS-SKILLS-MCP.md:22`), and 11-SECURITY-THREAT-MODEL.md, §5 (`docs/architecture/11-SECURITY-THREAT-MODEL.md:116`).  
Claim or gap: These documents still define content-bearing parked/restorable entries, last-seen restore, classifier-based persistence, and export/import. ADR 0029 instead defines metadata-only records, no classifier-based persistence, and a native/in-place/unavailable ladder.  
Failure scenario: An implementer follows the older state machine and stores an entry in Console state, despite ADR 0029’s claimed no-content policy—or follows the ADR and breaks UI states and restore flows that still depend on stored content.  
Correction: Reconcile the entry state machine, storage owner, and operations against one accepted policy. Remove unsupported parked/restore/export states or specify exactly how they work without Console-held content.  
Confidence: high — both sets of rules are explicit; ADR 0029 is still proposed.

IR-15 | HIGH | AREA 12  
Where: 13-UX-UI-SPECIFICATION.md, J3–J5 (`docs/architecture/13-UX-UI-SPECIFICATION.md:267`), 14-DATA-PERSISTENCE-AND-RECOVERY.md, §§3–8 (`docs/architecture/14-DATA-PERSISTENCE-AND-RECOVERY.md:37`), and 16-MULTI-DEVICE-AND-REMOTE.md, configuration portability (`docs/architecture/16-MULTI-DEVICE-AND-REMOTE.md:1`).  
Claim or gap: The user journeys, recovery design, and replication flows still assume content-bearing restoration records and import/export. ADR 0029 withdraws those mechanisms and says an entry removed outside Wyss is not restorable from local content.  
Failure scenario: A user follows the documented restore or cross-computer journey, but the implementation has only metadata and cannot reconstruct the entry—or recovery logic assumes content that the new policy forbids.  
Correction: Rewrite these journeys and recovery transitions after the storage decision. Specify what the user can restore from the provider file, what is permanently unavailable, and what replication can and cannot recover.  
Confidence: high — the documents prescribe incompatible outcomes.

IR-16 | BLOCKER | AREA 12  
Where: 19-IMPLEMENTATION-ROADMAP.md, F2a (`docs/architecture/19-IMPLEMENTATION-ROADMAP.md:150`), 20-VERIFICATION-AND-ACCEPTANCE.md, TS-REC and TS-SEC-07 (`docs/architecture/20-VERIFICATION-AND-ACCEPTANCE.md:91`), 23-TRACEABILITY-MATRIX.md, FR-SEC-08 (`docs/architecture/23-TRACEABILITY-MATRIX.md:108`), and ADR 0029 §8.  
Claim or gap: F2a still tasks a secret classifier, restoration store, undo, export/import, and tests that recover from content-bearing records. ADR 0029 says no mutation should be built until it or another policy is accepted, and would remove or change those requirements.  
Failure scenario: F2a starts against stale tests and ships the design ADR 0029 explicitly rejects.  
Correction: Make acceptance of a reconciled ADR a hard prerequisite for F2a. Replace its tasks, acceptance tests, and traceability rows before planning implementation.  
Confidence: high — roadmap and test IDs directly encode the old design.

IR-17 | BLOCKER | AREA 12  
Where: 09-PROFILES-AND-ORCHESTRATION.md, §3.1 (`docs/architecture/09-PROFILES-AND-ORCHESTRATION.md:74`), 19-IMPLEMENTATION-ROADMAP.md, F3c (`docs/architecture/19-IMPLEMENTATION-ROADMAP.md:246`), and ADR 0029, §7 (`docs/adr/0029-secrets-restoration-data-and-configuration-persistence.md:177`).  
Claim or gap: The profile design places generated settings files under Console state; ADR 0029 forbids that for files containing copied hooks and leaves their existence to DN-20. F3c prerequisites list OQ-02 and OQ-24, not DN-20.  
Failure scenario: A profile writes a secret-bearing generated hook file to disk before the storage and launch decisions are made.  
Correction: Add DN-20 and the launcher security ADR as hard prerequisites to F3c; make the profile unavailable until an approved storage and cleanup design passes crash tests.  
Confidence: high — the conflicting locations and missing prerequisite are explicit.

IR-18 | HIGH | AREA 12  
Where: 21-ADR-RECONCILIATION.md (`docs/architecture/21-ADR-RECONCILIATION.md:1`), 22-DECISIONS-AND-OPEN-QUESTIONS.md (`docs/architecture/22-DECISIONS-AND-OPEN-QUESTIONS.md:1`), and 23-TRACEABILITY-MATRIX.md (`docs/architecture/23-TRACEABILITY-MATRIX.md:108`).  
Claim or gap: The reconciliation and traceability set predates ADR 0029’s proposed changes and does not make its status, decision effects, and replacement tests traceable. The roadmap still treats DN-01 as the governing decision.  
Failure scenario: Developers treat ADR 0029 as accepted or ignore it, with no repository-level mapping that tells them which requirements and tests control.  
Correction: Once the ADR is accepted or revised, update the index, ADR reconciliation, decisions, and traceability matrix together; until then, state explicitly that it authorizes no implementation.  
Confidence: high — ADR 0029 says it is proposed; the reconciliation covers 0001–0028.

IR-19 | MEDIUM | AREA 13  
Where: 16-MULTI-DEVICE-AND-REMOTE.md (`docs/architecture/16-MULTI-DEVICE-AND-REMOTE.md:1`), 17-WYSS-AGENT-AND-HERMES.md (`docs/architecture/17-WYSS-AGENT-AND-HERMES.md:1`), and 18-DISTRIBUTION-AND-EXTENSIBILITY.md (`docs/architecture/18-DISTRIBUTION-AND-EXTENSIBILITY.md:54`).  
Claim or gap: Remote auth and recovery, the Wyss Agent choice, and public distribution are intentionally incomplete. The documents identify future decisions but do not provide an implementable trust, key recovery, update, or support contract.  
Failure scenario: A later phase begins from a roadmap label and invents pairing recovery, remote approval strength, agent compatibility, or update security during implementation.  
Correction: Keep F10, F12, and F13 blocked until their owner decisions, threat models, interface contracts, and acceptance tests are written and reviewed.  
Confidence: high — the documents explicitly defer these designs; no future-phase protocol was inferred.

IR-20 | HIGH | AREA 14  
Where: 07-CONFIGURATION-ENGINE.md, SM-PLAN and IF-CONFIG (`docs/architecture/07-CONFIGURATION-ENGINE.md:226`), 08-RULES-HOOKS-SKILLS-MCP.md, SM-ENTRY (`docs/architecture/08-RULES-HOOKS-SKILLS-MCP.md:22`), 09-PROFILES-AND-ORCHESTRATION.md, IF-PROFILE (`docs/architecture/09-PROFILES-AND-ORCHESTRATION.md:191`), and 13-UX-UI-SPECIFICATION.md, S13 (`docs/architecture/13-UX-UI-SPECIFICATION.md:121`).  
Claim or gap: State machines and interfaces omit important transitions and errors: authorization expiry and native-dialog failure, watcher invalidation during apply, ambiguous recovery, and profile permission/cancellation errors. The UI’s error list does not cover every operation error.  
Failure scenario: A dialog or window closes during authorization, a source changes while applying, or recovery cannot identify an outcome; implementation has no specified transition or user action.  
Correction: Specify guards, terminal states, retries, and recovery for every transition. For each interface, enumerate inputs, outputs, permissions, and all closed error outcomes; make acceptance tests cover each path.  
Confidence: high — the missing transitions and incomplete interface tables are visible in the documents.

### Areas with no finding

None. Findings cover areas 1–14.

### Judgement of the author's self-review (document 24)

- Findings of 24 confirmed independently: RV-01, RV-03, RV-05, RV-06, RV-07, RV-08, RV-10, RV-11, RV-12, RV-13, RV-17, RV-20, RV-22, RV-24, RV-26, RV-27, RV-31, RV-35, RV-43, RV-49, RV-50, RV-53. RV-02 identified a real earlier recovery defect, but its proposed correction does not settle recovery under ADR 0029’s metadata-only records.
- Findings of 24 that are wrong or overstated:
  - RV-04’s concern about claiming session adoption is sound; its account of earlier Claude behavior is marked `[N]` and should not be presented as established.
  - RV-02 is described as corrected, but ADR 0029 changes the recovery record and leaves the new crash cases unspecified.
  - RV-43’s recommendation to set an acceptable classifier miss rate conflicts with ADR 0029 and cannot establish that persisted entries contain no secrets.
  - RV-49’s proposed single research session is useful sequencing, but does not establish behavior across all hook shapes, write paths, machines, and policy sources.
- Resolutions proposed in 24 that would not work:
  - RV-01’s CSP and rendering rules are defense in depth, not an authorization boundary against compromised same-origin code that can write terminal input.
  - RV-03’s atomic exchange and exchange-back still race with later provider writes; it cannot guarantee preservation without provider cooperation.
  - RV-06’s one-time confirmation per profile version can become stale when the user’s permissions or hooks change without a profile version change.
  - RV-07’s one-time path/file identity approval does not itself prevent later path substitution; mutations need descriptor-anchored identity checks.
  - RV-20’s watch-count limit and event filtering do not bound kernel event pressure, overflow recovery work, or concurrent rescans. RV-20b’s stat-based optimization cannot replace a bounded hash strategy.
  - RV-43’s classifier threshold is not an acceptable control for a no-secret-persistence requirement.
- What 24 missed: ADR 0029’s own temporary-file persistence and incomplete metadata recovery (IR-02, IR-04); the narrow in-place compatibility tests (IR-05); the incomplete provider-file writer and path identity guarantees (IR-06, IR-07); Managed Inference’s global config and policy compatibility matrix (IR-11); the stale roadmap and test contracts after ADR 0029 (IR-14–IR-18); and incomplete state-machine/interface failure paths (IR-20).

### Verdict

**REJECT**

- **Terminal — IMPLEMENTATION-READY:** Existing terminal work is implemented, with the review’s terminal concern limited to the security boundary shared with the webview.
- **Native launcher and sessions — BLOCKED BY A DECISION:** Profile effects and plan-bound confirmation need an accepted launcher security contract.
- **Configuration read path — PARTIALLY DESIGNED:** The bounded-read and state model exists, but provider evidence and global watcher limits remain unresolved.
- **Configuration write path — BLOCKED BY A DECISION:** The provider write race has no reliable preservation guarantee.
- **Secrets and restoration — BLOCKED BY A DECISION:** ADR 0029 is proposed and conflicts with the current state, recovery, and test design.
- **Profiles — BLOCKED BY A DECISION:** DN-20 and complete source-exclusion effects must be resolved before F3c.
- **Managed plane and Inference isolation — BLOCKED BY A DECISION:** The isolation choice and full host-policy compatibility contract remain unresolved.
- **Models — PARTIALLY DESIGNED:** The projection is described, but provider/version evidence is incomplete.
- **Remote — BLOCKED BY A DECISION:** Identity, recovery, and remote confirmation contracts are not complete.
- **Wyss Agent — BLOCKED BY A DECISION:** The provider choice and evidence comparison are unfinished.
- **Distribution — BLOCKED BY A DECISION:** Platform, update, licensing, and support decisions remain open.
