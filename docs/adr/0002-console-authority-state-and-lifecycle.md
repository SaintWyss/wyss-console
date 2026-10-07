# 0002 — Console, authority, state and lifecycle

Status: Accepted

Date: 2026-10-06

## Decision

**Wyss Console is a client, not the security authority.** This applies to both
the Svelte frontend and the Rust backend hosted inside the graphical Tauri process.

The frontend renders state, receives input, presents terminals and approval
requests, displays events and sends intents. The local backend adapts IPC,
validates DTOs and manages application-local resources. The UI accesses Tauri
through the dedicated IPC client, never directly throughout feature components.

A future external control authority owns managed-session identity, agent
authorization, security policy, durable permissions, leases, risk gates, bound
approval decisions, canonical transitions and control audit. It has a lifecycle
independent of the graphical Console. Its production name, implementation and full
transport are deferred. No authority process is implemented in Sprint 0A.

The UI sends intents; the deterministic authority decides whether and how a control
transition is permitted. An approval dialog, adapter report or emitted event does
not itself grant authority. Future remote clients use the same control contract.
No always-on network service is required by this decision.

## State ownership

| State                                                | Owner                                     | Rule                                                          |
| ---------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------- |
| Selected tab, focus, scroll, pane layout             | Frontend                                  | Visual state; optional saved preferences are Console-local.   |
| Human-terminal metadata and Console session identity | Console local                             | Restore metadata without assuming a process survives.         |
| Local diagnostics                                    | Console local                             | Kept distinct from canonical control audit.                   |
| Future human-terminal PTY/process resources          | Console local backend                     | May temporarily live in-process during terminal-only sprints. |
| Observed PID                                         | Local observation or authority projection | Never durable agent identity or authorization.                |
| Managed-agent session and process identity           | External authority                        | Console holds references and projections.                     |
| Permissions, leases, approvals and gates             | External authority                        | Never reconstructed from frontend state or local preferences. |
| Model selector draft                                 | Frontend                                  | An unapplied proposal.                                        |
| Model preference                                     | Console local                             | Does not prove selection or actual runtime.                   |
| Selected profile, explicit pin, observed runtime     | External authority                        | Three separate facts for managed sessions.                    |
| Local event sequence                                 | Console local writer                      | Cannot establish agent authority.                             |
| Canonical event sequence and control audit           | External authority                        | Single-writer ordered, durable and versioned.                 |

A Console session is not implicitly promoted to a managed-agent session. Local
and managed session identities remain distinct.

## Crash/restart invariant

> Closing, crashing or restarting the graphical Console must not grant, revoke,
> expand or otherwise change what a managed agent is authorized to do.

Connection presence is not a permission. Disconnecting is not approval, renewal,
revocation or a stop request. On reconnect, Console reads authority-owned state;
it does not restore permissions from caches. The external authority cannot exit
solely because its last graphical client closes while agents or obligations remain.

Explicit stop/revoke operations are separately validated intents. Independent
expiry or security transitions may occur while Console is closed; their cause
belongs to the authority, not the graphical lifecycle. Authority crash recovery
and lease implementation are deferred mechanisms with their own failure semantics.

## Enforcement milestone

This invariant is documented in Sprint 0A, where no managed agents exist.
Before the first managed-agent execution, including terminal/L0 fallback, and
before permissions or approvals are enabled, authority and managed-agent
supervision must be external. Closing, crashing and reconnecting the graphical
client with an active agent must be tested without lifecycle-caused authority changes.

Human-operated terminals in an earlier sprint do not imply managed-agent
containment. A separate process alone does not establish an OS security boundary.

## References

- Wyss architecture research report, sections 12, 13 and 21.
- Accepted pre-implementation DECISION_PACKET, 2026-10-06.

## Sprint 2D implementation status

The independently started `wyss-control` process now owns prepared/closed managed
control records, durable intent receipts and canonical audit. Client disconnect and
authority restart recovery are tested. Console does not spawn or connect to it yet.
Runtime supervision, authorization gates and permissions remain deferred; managed
execution is still unavailable. ADR 0011 records this implementation without
changing the original authority or crash/restart decision.
