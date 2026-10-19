# Final target user journeys and UI contract — Wyss Console

**Status:** Full target design; does not assert that future screens exist. The authoritative vocabulary/layout contract remains architecture [13](../../../architecture/13-UX-UI-SPECIFICATION.md). This supplement defines explicit end-to-end journeys and their negative states for implementation and later acceptance.

## Product interaction model

The console is **terminal-first**. A native or managed badge is text, never color alone. Separate **configured**, **requested**, **reported**, **predicted** and **unknown** facts. No fake switch, loading success, silent fallback or "verified" checkmark without evidence. Every mutation exposes the target/source and an undo/error result.

### Main navigation (target)

| View                | Primary action                                        | Important failure or empty state                 |
| ------------------- | ----------------------------------------------------- | ------------------------------------------------ |
| Console / Workspace | Launch Claude/Codex; attach, split, search PTYs       | No provider installed; host fallback             |
| Sessions            | See real processes, resource use, stop/restart/remove | Lost != exited; disconnected != failed           |
| Configuration       | Inspect scope, toggle verified hook/skill/MCP         | Trust/ACL failure; unsupported provider          |
| Profiles            | Version, clone, import/export launch profiles         | Risky overlay confirmation; incompatible profile |
| Models              | Distinguish installed/configured/selected/observed    | Catalogue is not entitlement; in-use unknown     |
| Handoff             | Review bounded context sent to another agent          | Untrusted text, cancellation, stale source       |
| Credentials         | Secure API key references                             | Wallet locked; provider supports OAuth only      |
| Remote/Devices      | See hosts, pair/revoke devices and grants             | Gateway disabled; revoked; offline               |
| Agents/Approvals    | Inspect managed turns under authority                 | Safety blocked; no native fallback               |
| History/Diagnostics | Safe audit, errors and resource metrics               | Privacy-safe export, missing events              |
| Settings/Help       | Keymap, terminal/privacy preferences, user guidance   | No unsupported capability offered                |

Screens are shown only when actually implemented. Future menu items must be hidden or explicitly disabled with a reason, not decorative clickable controls.

## Core journeys and acceptance contracts

**J-01: Start daily work.** Open app → Console visible; no mutation of provider config → new terminal → keyboard works. Host available shows host-attached status; otherwise a clear nonpersistent fallback.

**J-02: Native agent launch.** Workspace → choose provider/version, project via native picker and profile → launch plan → start → real CLI screen. Show the session as Native, with true lifecycle and never confuse provider prompt with Console trust confirmation.

**J-03: Multiple agents.** Launch Claude Code + Codex + terminal → switch/split/focus → inspect CPU/memory without duplicate counting. Multiple startup failures do not end the other agents.

**J-04: Reconnect persisted PTYs.** Close only GUI → host remains → reopen → same session/scrollback; restore pane layout; label any missing output gap. Host death means Lost/Disconnected, not fabricated active.

**J-05: Configuration inspect and update.** Configuration → scopes and sources → inspect provider-owned values → make planned change → native confirmation if risky → result and undo. Show if live effect is unknown.

**J-06: Provider writes outside Console.** Change watched file in provider → source marked Changed outside → refresh with source revision → existing undo refuses to clobber newer bytes.

**J-07: Profile creation.** Create from template → choose agent-specific flags/overlay → version → import/export round trip → launch with no global settings mutation. Unsupported per-session capability refuses launch, not pretend it applied.

**J-08: Native model choice.** Models → see catalogue/configured value and uncertainty → select for next new session profile → launch → show selected; if actual model not reported, retain "in use unknown".

**J-09: Managed model observation.** Managed authority reports actual model → display reported M5, time/turn and provenance. If unavailable, no default guessed. Managed model selection requires a separately accepted ADR.

**J-10: Context handoff.** Select source agent/session → select allowed artifacts → review redacted bounded package and target permissions → confirm → destination starts via its own profile, not source grants. User cancellation changes nothing.

**J-11: Credentials.** Open protected manager → wallet unlock via OS → store labeled secret securely → bind opaque ID with explicit consent → provider-specific use. Never display secret bytes in general UI or logs.

**J-12: Remote computer.** Host owner enables owner-approved gateway → second computer pairs after local confirmation → receives limited grant → sees sessions and approved terminal capability; revocation immediately ends access. No graphical desktop.

**J-13: Phone.** Physical phone pairs with expiring challenge → read-only status → step-up to type/manage if granted → lock/background hides sensitive content → revoke from host. Unsupported platform not shown as installed.

**J-14: Managed approval.** Read pending managed action → display authority-verified target, policy and risk → confirm/deny via authority contract. A native session is never falsely labelled as managed.

**J-15: Recovery.** Unexpected window/host/provider exit → detect which component died → present Lost/Disconnected/Ended accurately → explicit restart or reconciliation → no automatic duplicate side effects.

**J-16: Install/upgrade/uninstall.** Install supported build as regular user → first run read-only → provider discovery → safe migration → backup/rollback if needed → uninstall prompt for state retention, no hidden network account.

## Responsive and keyboard behavior

- Terminal/pane remains primary and retains focus when overlays close.
- All operations accessible from keyboard, screen reader labels and visible focus.
- Names, hosts, statuses, counts and security states never rely on color, glyph or animation alone.
- Modals must trap focus appropriately and allow safe cancellation; risky native confirmation is not a webview emulation.
- Small screens: controls collapse to a navigable list, terminal remains readable; phone is a dedicated client, not an unverified resized desktop build.
- Empty, loading, partial, offline, error, stale and permission-denied states each have distinct copy.

## Consistent error copy

| Safe code                | User text pattern                                                                            |
| ------------------------ | -------------------------------------------------------------------------------------------- |
| `provider_not_found`     | "The selected agent is not installed on this host."                                          |
| `profile_not_applicable` | "This provider cannot apply that setting per session. Nothing started."                      |
| `source_conflict`        | "Configuration changed outside Wyss Console. Review the latest version before retrying."     |
| `wallet_locked`          | "The credential store is locked. No credential was written."                                 |
| `host_disconnected`      | "Connection to the terminal host was lost; the agent's process state is unknown."            |
| `grant_revoked`          | "This device no longer has access to that host."                                             |
| `safety_unproven`        | "Managed operation is unavailable because the required safety checks could not be verified." |
| `observed_model_unknown` | "Configured model is known; model in use was not reported."                                  |

These are wording candidates; exact error keys must follow existing Rust error vocabulary. Never expose `stderr`, raw JWT/token, stack trace or filesystem secret in a toast.

## Screen acceptance

Screens must be tested in actual GUI for keyboard, correct badges, all negative states, UI navigation under real running providers, operation cancellation, control-plane origin, app restart and memory/CPU. A fixture-only test is not final acceptance.

**F12 note:** Wyss Agent view is not a required standalone product until DN-06 has been accepted after provider comparison. Naming it as a navigation item before then would falsely imply implementation.
