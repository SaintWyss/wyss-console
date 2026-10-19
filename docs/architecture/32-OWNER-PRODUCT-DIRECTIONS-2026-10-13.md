# 32 — Owner's final-product directions (2026-10-13)

Status: **product requirements and planning directions, NOT implementation authorisation**.
Source: the owner's explicit choices in the 2026-10-13 Wyss Console planning conversation.
The owner authorised **documentation consolidation on the planning branch only**.
Nothing in this record accepts an ADR, lifts DN-16/DN-24, authorises a provider experiment,
permits writing the owner's configuration or merging PR #2. No code or tests were run for this record.

## 1. Authority and terminology

- **Owner-selected**: the owner chose the specific alternative in conversation. It determines
  target product behaviour, not the solution's safety or feasibility.
- **Delegated planning candidate**: on 2026-10-13 the owner asked the planning assistant to
  take the previously recommended alternatives without repetitive questionnaires, except
  for choices with meaningful owner-dependent trade-offs. These are defaults for designing,
  not technical proof or formal ADR acceptance.
- **Open / conditional**: no solution may be silently selected while important facts or trade-offs
  are unavailable. Where evidence is required, write a falsifiable gate, not a promise.
- **Built/observed** is never inferred from an owner preference.
- **Full product** means the personal product's accepted final scope; NOT an MVP and NOT a
  licence to omit features from its acceptance criteria.

This document is the canonical record of the later conversation decisions, which have not yet
been incorporated into [22](22-DECISIONS-AND-OPEN-QUESTIONS.md)'s detailed alternatives.
Until all design contracts agree, differences are tracked in
[33](33-CROSS-DOMAIN-RECONCILIATION.md), not silently flattened.

## 2. Owner-selected directions

| ID | Decision for final product | Technical caveat / next document |
| --- | --- | --- |
| DN-06 | **D**: Wyss is the policy/authorisation/execution controller; Hermes is intelligence/planning, strictly behind a boundary | OS-level containment and no direct effectful Hermes tools must be evidenced; 17 §6, OQ-55–57 |
| DN-07 | **A**: personal product, not offered or supported as a public distribution | ADR 0027 remains; no licence, release or repository publication authorised; DN-49/51 not applicable while scope holds |
| DN-08 | **A**: a separate managed runtime per session; bounded concurrent OL3 work | Isolation is not proved by a process boundary; concurrency/security ADR and measurements required |
| DN-09 | **C**: live observations plus versioned durable events of reported model changes and reroutes | ADR 0031 remains Proposed; provider observation is not proof of each turn's effective model |
| DN-10 | **C (new)**: static effective-config inference plus optional verification from an already-running compatible Codex session | Do not launch a provider solely to inspect; unknown remains 'not verified' |
| DN-11 | **A**: products named Wyss Console and Wyss Agent; internal services are not a separate Wyss CLI product | No rename or new component authorised |
| DN-12 | **A**: native dialog to author/edit executable hook commands, with review | Not a security boundary by itself; writing is still blocked |
| DN-13 | **A**: GUI can use a closed set of validated provider-native configuration operations | No arbitrary page-supplied command, arguments or process invocation before authorisation |
| DN-14 | **C**: use provider-official separated-configuration mechanisms to preserve existing sign-in; otherwise use ordinary profiles with an explicit limitation | Never read, duplicate or re-home auth credentials |
| DN-15 | **OL3**: supervised coordinated multi-agent pipelines, including OL0–OL2; no scheduled unattended OL4 automation | Explicit scope includes orchestration, not mere handoff; see 34 |
| DN-20 | **B**: evaluate omitted user controls against effective configuration at every start; native reconfirmation when excluded set or provider version changes | Provider/system-required controls are not optional; memory-only generated material only if verified |
| DN-22 | **B**: correct security claim AND target separated configuration and terminal web views with differentiated capabilities | ADR 0030 Proposed; neither view guarantees an OS-user boundary |
| DN-32 | **A**: global Console, Sessions, Configuration, Profiles, Agents, Approvals, History, Settings; Devices when remote exists; Sessions grouped by project | Actual visual design remains unapproved |
| DN-33 | **C**: disabled contextual actions explain gate; absent whole areas do not advertise unfinished functionality | No permission to ship a final product incomplete |
| DN-34 | **C**: dark, light and high-contrast themes, including terminal colour accessibility | Contrast tests for all themes required |
| DN-35 | **C**: desktop notifications opt-in per category, default off | No sensitive content in OS notification payload or lock-screen text; Android push is separate |
| DN-36 | **D**: reconnect to surviving sessions; if dead, restore layout and show individual/batch opt-in restart through the ordinary launcher | Layout persistence is never authority to launch an agent |
| DN-37 | **B**: default 30-day configurable review reminder for held entries | Never delete, deactivate or alter an entry by age |
| DN-38 | **D (new)**: integrated visual model/effort selector in native sessions where a verified safe provider mechanism exists | No silent terminal command injection or provider config mutation; show limitations explicitly |
| DN-39 | **C**: leader chord plus safe configurable direct shortcuts with conflict detection | Verify KDE, terminal inputs and IME before assigning real chords |
| DN-40 | **C**: new terminals adapt their scrollback preference when global memory budget is reached; never trim existing retained output merely because of this threshold | NOT a hard RAM bound; specify upper memory and process bounds separately; performance evidence required |
| DN-42 | **C**: managed Codex model/effort selection at session start and per turn | Separate safety ADR, catalogue validation, provider tests, external review |
| DN-43 | **D**: historical usage and rate-limit metrics where reported by provider | Separate sensitive metadata store policy; no fabricated tokens or price conversion |
| DN-44 | **B**: authorised second PC and Android may get full remote terminal input, default off, explicit revocable privilege | Remote terminal.write == the host user's shell/account access; does not respect narrower config grants |
| DN-45 | **B**: Android can submit a config-change request that is revalidated and finally confirmed natively on the host | Cannot remotely satisfy a host-only confirmation; direct writes blocked by L1/L2/L5 and DN-31 |
| DN-46 | **B**: optional, explicitly enabled user-service lingering so work may survive Linux logout | screen-lock/close/logout/suspend/power-off distinguished; lingering is not magic crash recovery |
| DN-47 | **B**: faithfully restore interactive terminal screen on reattachment, potentially via host-owned emulator state | Parser isolation, bounded memory, fidelity, replay and hostile escape sequences unverified |
| DN-48 | **A**: dedicated native Android client with platform-keystore/presence capabilities | No mobile pairing/remote security claims before tests |

**DN-05 remains OPEN.** The owner expressly rejected prematurely committing to RA3/VPN:
the preferred experience is transparent local LAN and, away from home, direct encrypted P2P
where possible, with a relay fallback; no manual VPN operation per connection. Reuse of
Sarmiento's inventory/discovery/infrastructure may be studied but not required. The
remote transport, gateway security, costs, relay ownership, hosting and compatibility are
**not chosen**. The host service shape HS1 remains a design candidate, highly aligned with
DN-46/47, not an accepted ADR. See [35](35-HOST-REMOTE-AND-MOBILE-DELTA.md).

## 3. Delegated planning defaults (NOT new ADR approvals)

The owner authorised use of recommendations without asking again on each question.
Preserve them as **candidates** and do not treat them as verified implementation contracts.

| ID | Planning assumption | Qualification |
| --- | --- | --- |
| DN-02 | Launch native agents through the owner's interactive shell with controlled direct-spawn fallback | Launcher ADR, shell recipes, consent and identity-check evidence pending |
| DN-03 | Investigate isolation I4 first, I1 second, I2 last | Managed Inference independent of native hooks remains unverified; no experiments authorised |
| DN-04 | Add dependencies only when the specific phase proves need and licences/performance are reviewed | Does not approve any dependency installation |
| DN-19 | No Codex hook toggles before G-INF; show state and reason | Preserve original safety property; no silent weakening |
| DN-23 | Version-sensitive, tiered provider evidence; verified native writers preferred | No blanket supported-version ranges for dangerous kept forms |
| DN-41 | Measure transport M23 first; if it fails, study notification+pull and event-driven wait together | No performance measurement or changed transport authorised |
| DN-50 | No network update checks by default for personal deployment; local/package updates only | No update server; do not conflate with verified dependency security updates |

DN-49 and DN-51 concern public distribution and become **not applicable under DN-07=A**,
rather than falsely approved. Preserve their historical alternatives for a future scope change.

## 4. Previously recorded owner constraints continue unchanged

The decisions on DN-01, DN-21, DN-25–30 and DN-31 as recorded in [22] and
[27](27-PROTECTED-RESTORATION-VAULT.md) remain authoritative. In particular:

- User-owned entries are not deleted merely due to age or storage pressure.
- Recoverable content has no place in indefinite logs, event stores, diagnostics or analytics;
  named encrypted vault exceptions remain Proposed, with precise consent and threat model.
- Provider-native writers are preferred. Direct writes stay blocked where concurrency,
  transaction, filesystem, replication and snapshot conditions have not been verified.
- DN-31 is unresolved. An ancestor marker or an ignore rule is not proof of non-replication;
  filesystem snapshots are a separate disclosure channel and not automatically acceptable.
- L1, L2, L5 and blocker B1 remain open. No declaration of zero data loss is permitted.
- No provider executable probes, experiments in throwaway homes, background deployment,
  tests, packaging, or implementation are authorised by this documentation work.
- No ADR status changes, accepted architecture, or publication of private components
  are authorised. Any technical acceptance is a later separate decision.

## 5. Product acceptance direction

Before any final-product work can be authorised, every requirement must trace to a design
contract, failure modes, a test and a pass/fail acceptance criterion; provider and OS claims
need the labelled level of evidence. Independent review must cover all material revisions
after both existing REJECT verdicts. Owner approval of the full plan is separate and last.

The proposed elaborations live in [33](33-CROSS-DOMAIN-RECONCILIATION.md),
[34](34-OL3-ORCHESTRATION-CONTRACT.md),
[35](35-HOST-REMOTE-AND-MOBILE-DELTA.md) and
[36](36-PLANNING-CLOSURE-AND-VERIFICATION-GATES.md).

## 6. Later owner permission: isolated verification stage

**Later on 2026-10-13**, after the docs-only consolidation, the owner
answered **yes** to a *stage* of bounded technical verification
in isolated/disposable environments and to obtaining an independent
architecture review. This supersedes the earlier blanket prohibition
only for that bounded verification stage. It does **not** constitute
a per-run grant to open personal configuration/credentials or to
modify the owner's desktop/phone, and it does not authorise
implementation, actual product code change, service deployment,
accepted ADR, public distribution or PR merge.
The accessible execution container was used solely for a
disposable filesystem name-exchange check recorded in
[41](41-STATIC-TRACEABILITY-AND-ISOLATED-FILESYSTEM-CHECK.md).
Source-reading evidence was recorded in [39](39-REMOTE-CANDIDATE-SOURCE-EVIDENCE.md)
and [40](40-HERMES-BOUNDARY-STATIC-EVIDENCE.md).
When an isolated provider setup or native Android/VM environment
is actually available, each higher-risk probe still needs a named
target, version, allowed data, exact procedure and rollback before execution.

## 7. IR-01–15 REJECT overlays do not revoke the owner's product goals

The post-owner review
[reviews/2026-10-13-independent-post-owner-review.md](reviews/2026-10-13-independent-post-owner-review.md)
is a **REJECT** of the proposed implementation
contracts, not a rejection of the selected
functionality DN-08, DN-15, DN-44, DN-46 or DN-48.
New [42](42-POST-REJECT-CORRECTIVE-PLAN.md)
states **fail-closed temporary blockers** for
native OL3, managed concurrency, uncontrolled
Hermes shell, absent-host LP4 and sensitive
configuration mutation. Existing same-UID
privileges cannot be treated as separate
security principals.

The owner asked to be consulted only when
a meaningful product/security trade-off
cannot be resolved by technical evidence.
Safe defaults pending independent review
do not preapprove a permanent reduction in
final-product scope, unattended phone control
or unauthenticated mobile fallbacks.

