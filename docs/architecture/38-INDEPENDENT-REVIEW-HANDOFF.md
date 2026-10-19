# 38 — Independent-review handoff for final-product planning

**This is the reviewer brief, NOT a review or a positive verdict.**
The author of this file and the other new architecture appendices
cannot independently approve their own work. Earlier independent
verdicts in [25](25-INDEPENDENT-REVIEW.md) remain **REJECT**.
The owner authorised docs-only consolidation on 2026-10-13, not
experiments, implementation, ADR acceptance, or merge.

## 1. Reviewer assignment

Review **Wyss Console's complete personal final-product plan**;
assess the actual proposed system, not its prose or apparent length.
The owner has rejected an MVP and selected all final-product
functional directions of [32](32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md).
Explicitly mark every statement as one of:

- **Accepted baseline**: backed by an already accepted ADR or cited
  source-code fact read from a pinned revision, without inventing runtime proof.
- **Owner product requirement**: a future target, not present behaviour.
- **Candidate design**: a proposal needing an ADR or tests.
- **Verified observation**: only when reproducible evidence exists
  with exact source/version/environment (none was run by this pass).
- **Blocked**: a missing fact or unaccepted risk invalidates adoption.

Do not edit application code, run binaries/providers, read credentials,
alter OS services or approve ADRs. The review may produce a separate
report or PR comment, with no self-certified PASS.

## 2. Reading order and authority

1. [00](00-INDEX.md): documentary hierarchy and accepted baseline.
2. [32](32-OWNER-PRODUCT-DIRECTIONS-2026-10-13.md):
   owner choices, delegated candidates, denied permissions.
3. [33](33-CROSS-DOMAIN-RECONCILIATION.md): conflict register.
4. [34](34-OL3-ORCHESTRATION-CONTRACT.md): OL3 design.
5. [35](35-HOST-REMOTE-AND-MOBILE-DELTA.md): host/remote/mobile.
6. [37](37-HANDOFF-ADAPTERS-UX-AND-DATA-CONTRACTS.md):
   handoff, provider evidence, UI, event history.
7. [02](02-REQUIREMENTS.md), [20](20-VERIFICATION-AND-ACCEPTANCE.md),
   [23](23-TRACEABILITY-MATRIX.md):
   requirement/test/acceptance links.
8. [05](05-TERMINAL-AND-NATIVE-SESSIONS.md),
   [07](07-CONFIGURATION-ENGINE.md),
   [10](10-MANAGED-RUNTIME-AND-SECURITY.md),
   [11](11-SECURITY-THREAT-MODEL.md),
   [12](12-MODEL-MANAGEMENT.md),
   [13](13-UX-UI-SPECIFICATION.md),
   [14](14-DATA-PERSISTENCE-AND-RECOVERY.md),
   [15](15-PERFORMANCE-ENGINEERING.md),
   [16](16-MULTI-DEVICE-AND-REMOTE.md),
   [17](17-WYSS-AGENT-AND-HERMES.md),
   [18](18-DISTRIBUTION-AND-EXTENSIBILITY.md):
   original design and appended owner overlays.
9. [27](27-PROTECTED-RESTORATION-VAULT.md),
   [28](28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md),
   [30](30-TECHNICAL-VALIDATION-PLAN.md):
   unresolved loss/disclosure blockers.
10. [31](31-PLANNING-COVERAGE-AUDIT.md) and
    [36](36-PLANNING-CLOSURE-AND-VERIFICATION-GATES.md):
    historical score and objective stop gates.
11. Every existing **accepted ADR** and all three proposed ADR
    texts 0029/0030/0031; check conflicts and unreconciled clauses.

## 3. Mandatory adversarial questions

### CRITICAL: same-user shell vs authority boundaries

- If a remote Android phone receives LP4 terminal input, can the
  gateway honestly claim to protect config, vault or managed
  approval from that same host account? Identify every false
  security statement and how to correct it.
- If a config webview is compromised, can it call terminal APIs,
  simulate a native confirmation, invoke a provider config command
  or feed executable text into a native editor? Identify which
  mitigations are UI restrictions and which are real OS boundaries.
- Does an OL3 coordinator have any path around `wyss-control`'s
  leases/attestations/one-writer and turn checks? Does the selected
  model per turn affect those checks? Can it issue native shell
  input as a fallback after a managed rejection?
- Can Hermes call tools, network or subprocess effects despite the
  Wyss policy wrapper? If tool isolation is unverified, block
  the design instead of trusting in-process plugins.

### CRITICAL: filesystem writes, replication, vault

- Can an external provider write to an old descriptor or newer
  config version while the Console performs a staged exchange?
  Are L1/L2/L5 really still named as unsolved? Are rollback,
  snapshots, sync ignore rules and cross-mount EXDEV described
  accurately?
- Can any plaintext transaction content reach the owner's
  replicated home, snapshot, logs, diagnostics, crash report,
  swap or analytics? Does the design rely on an actual
  observed exception rather than a symbolic ignore marker?
- What happens when the private vault keyring is locked, missing,
  or rotated? Is recovery opt-in and authenticated? Can quotas
  or a 30-day reminder silently delete held user data?

### HIGH: persistence and remote input

- After KDE logout, service crash, host crash, power cycle,
  network switch and Android unlock, is the process *really*
  alive or merely a saved tab? Does any UI confuse them?
- Can retransmission duplicate a shell keypress or race between
  two phone/desktop writers? Are stream/claim epochs host bound?
- Does a long-lived emulator parse terminal OSC/DCS or hyperlinks
  with full host account rights? What isolation is measured,
  not merely promised? Are current screen, history and
  per-client queues actually bounded?
- Who runs relay/discovery, owns the keys, pays operating costs
  and handles revocation? Can transport identity be confused
  with application-level device authorisation?

### HIGH: data, cost and compatibility

- Are a requested model, the reported thread-grain model and
  an actually rerouted turn incorrectly conflated? Does durable
  usage history collect prompt or output, double-count
  notifications or invent monetary costs?
- For OpenCode, which installed version and docs generation
  control precedence? Are unknown remote/managed sources
  labelled unknown? Can a JSONC edit round-trip comments?
- Does changing profile global exclusions require fresh
  native confirmation? Could a provider version update make
  the old 'approved exclusion' unsafe?
- Does FR-DIST-07 define personal repeatable delivery
  without silently requiring or proposing a public release?
- Are all final-product UX capabilities truly covered by
  state/error screens, tests and accessibility contracts?

## 4. Audit obligations by subsystem

Re-evaluate every one of the **29 subsystem rows** in [31 §2](31-PLANNING-COVERAGE-AUDIT.md),
and its five dimensions: requirement coverage, depth, decisions,
traceability and verification specification. Do not reuse 64.2%
as a current number or count the existence of a test ID as
test execution. Independently check:

1. Every target requirement exists in 02 (including 40 owner-
   final IDs), has its stable design and source, and reaches a
   named test plus pass/fail acceptance in 20/23.
2. Every new test has negative cases, resource limits, source
   independence, fixture/version and failure states.
3. Every new link and identifier is valid; no superseded
   assumption remains presented as an owner decision.
4. Every original public-distribution requirement is either
   conditional or demonstrably needed for the personal product.
5. Every unresolved external fact is a **blocker** when needed
   for a security or reliability claim; never a grade-A assertion.

If there are contradictions between appendices and accepted
ADRs, list both precise passages. Do not modify an accepted
ADR status or quietly rewrite it in another document.

## 5. Output format and pass/fail threshold

Write an independently authored report under
`docs/architecture/reviews/` (or a PR review) containing:

- Final **PASS / PASS-WITH-BLOCKERS / REJECT** verdict
  on *paper readiness only*. Neither PASS variant authorises code.
- Findings table: ID, severity (CRITICAL/HIGH/MEDIUM/LOW),
  affected requirement/ADR/contract, exact file and section,
  observed evidence vs inferred claim, security/user impact,
  reproducible counterexample and minimum fix.
- One table per 29 subsystem rows with the five audit labels,
  explicit source and remaining blocker.
- Flat list of owner-only choices that cannot be decided
  responsibly from documentation.
- List of evidence-only gates requiring later separate
  experiment consent. Never perform them during review.
- Traceability failures that are not just formatting issues.
- Explicit confirmation that no code/test/provider operation
  occurred, and which exact docs/commits were reviewed.

**REJECT** if a critical security bypass is silently described
as contained, a required part of the final product is absent
without an explicit blocked-state and acceptance path, or a
claimed runtime guarantee lacks its necessary proof.
Do not grant PASS because a model filled many pages.

## 6. No owner authorisation implied

This handoff is executable **as a read-only independent document
review** by someone independent of the prior author, provided
the reviewer has legitimate access to this private PR.
No request to run experiments, change credentials, install
packages or merge code is included. Any later verification
and final approval is a separate action by the owner.


## 7. Post-REJECT third-review assignment (2026-10-14)

A first post-owner adversarial report is now at
[reviews/2026-10-13-independent-post-owner-review.md](reviews/2026-10-13-independent-post-owner-review.md).
It returned **REJECT**, enumerated IR-01–15 and explicitly
disclosed its author's overlap with earlier design documents.
It **does not satisfy the independent-third-review gate**.

Read that preserved report first, then
[42](42-POST-REJECT-CORRECTIVE-PLAN.md) and the corrected
33, 34, 35, 37, 16, 20, 23 and 40. Check the actual
accepted ADRs 0011, 0012, 0015 and 0020 and
the original Wyss policy source. A third reviewer
**must not have authored the prior baseline or corrections**;
do not reuse an earlier reviewer identity or claim the
existing Claude report is independent.

Required response: independent PASS/REJECT **on paper
only**, and for each IR-01..15 either:
(a) verified mitigated in *specification only*, with
exact file/section and correct blocked implementation
gate, (b) still defective, with counterexample,
or (c) requiring technical evidence, explicitly NOT RUN.
Verify preservation of TS-CFG-17..20, distinct
TS-CFG-21..24, RC vs historic C, MA vs MS,
and status of current ADR 0012/0015 concurrency.

Focus beyond editorial consistency: whether a
same-UID unrestricted native agent could impersonate
approval APIs even after the proposed corrections;
whether the selected trust boundary changes requested
native-session functionality; whether device revocation
could ever restore host integrity after LP4; whether
a separate PTY/parser process really isolates crashes;
and whether the proposed `LP4-away` fallback violates
the owner's understanding of remote control. **Do not
turn a blocking contradiction into a green badge merely
because the next document calls it BLOCKED**.

No app code, provider runs, services, personal file reads,
new ADR acceptance, PR merge or owner-device tests.
Independently authored report must say which files
and pinned commits were actually reviewed. The
final-product plan stays unapproved despite any
positive paper verdict; the owner has a later,
separate approval gate.
