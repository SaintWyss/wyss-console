//! The projection of one durable event, and of a window of them.
use crate::contracts::{
    Sequence, UtcTimestamp, WireUuid,
    control::{
        ApprovalEffect, ApprovalKind, ApprovalReason, ControlError, DesiredState, ExecutionScope,
        RevocationReason, RuntimeDiagnostic, TurnReason,
    },
    identities::{ManagedApprovalId, ManagedSessionId, ManagedTurnId},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

pub(super) const EVENTS_SCHEMA_VERSION: u8 = 1;
/// Events one window holds at most. It is the authority's own page bound.
pub const EVENTS_PAGE: u64 = 50;
/// Longest runtime version shown. Anything else is left out.
const VERSION_MAX: usize = 32;

/// What part of the product an event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventCategory {
    Session,
    Runtime,
    Turn,
    Approval,
    Lease,
    /// An event of a kind this build does not know. Nothing of it is shown but that.
    Authority,
}

/// What was recorded. A closed vocabulary: a kind that is not in it is `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    SessionPrepared,
    SessionClosed,
    SessionBound,
    SessionAttached,
    BindingIndeterminate,
    BindingAbandoned,
    RuntimeDesired,
    RuntimeReady,
    RuntimeExited,
    TurnAdmitted,
    TurnRunning,
    TurnCancelRequested,
    TurnCompleted,
    TurnInterrupted,
    TurnFailed,
    TurnIndeterminate,
    ApprovalRequested,
    ApprovalAccepted,
    ApprovalConfirmed,
    ApprovalDeclined,
    ApprovalCancelled,
    ApprovalExpired,
    ApprovalInvalidated,
    ApprovalIndeterminate,
    LeaseGranted,
    LeaseRenewed,
    LeaseRevoked,
    LeaseExpired,
    Other,
}

/// How an event reads at a glance. Derived from its kind and its recorded reason only:
/// nothing here is a judgement about how serious it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Normal,
    /// It ended some other way than asked for, or its outcome is not known.
    Attention,
    /// The authority recorded a failure.
    Failure,
    /// The authority recorded that an isolation or safety rule was what ended it.
    Security,
}

/// The few facts of an event that cross, each from a closed vocabulary or a timestamp.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EventFacts {
    /// Why a turn ended, or why its cancel was asked for.
    pub turn_reason: Option<TurnReason>,
    /// What kind of action an approval was about. Never the action.
    pub approval_kind: Option<ApprovalKind>,
    pub approval_reason: Option<ApprovalReason>,
    /// What became of the one answer an accepted approval sends.
    pub approval_effect: Option<ApprovalEffect>,
    /// What the runtime was asked to be.
    pub desired: Option<DesiredState>,
    /// How the runtime's process ended, and whether the authority meant it to.
    pub exit: Option<RuntimeDiagnostic>,
    pub expected: Option<bool>,
    pub runtime_version: Option<String>,
    /// What a lease allowed. The lease itself is not named.
    pub scopes: Option<Vec<ExecutionScope>>,
    pub expires_at: Option<UtcTimestamp>,
    pub revocation: Option<RevocationReason>,
}

/// One durable event, as a person may read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventRow {
    /// Its position in the stream. An opaque decimal string to whoever receives it.
    pub sequence: Sequence,
    pub at: UtcTimestamp,
    pub category: EventCategory,
    pub kind: EventKind,
    pub disposition: Disposition,
    /// It is a lease event, or isolation or safety is what it records.
    pub security: bool,
    pub session: Option<ManagedSessionId>,
    pub turn: Option<ManagedTurnId>,
    pub approval: Option<ManagedApprovalId>,
    pub facts: EventFacts,
}

/// A window of the stream, newest first and without a hole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventsPage {
    pub schema_version: u8,
    /// The stream these events belong to. Positions of two streams are unrelated.
    pub stream: WireUuid,
    /// The durable head read before the events: the window is at least this new.
    pub head: Option<Sequence>,
    pub events: Vec<EventRow>,
    /// Hand this back to read the window before this one. None when there is none: the
    /// stream begins in this window, or what lies before it is not there to read.
    pub older: Option<Sequence>,
    /// The authority did not return the events this window was to begin with: history
    /// before its oldest event is unavailable.
    pub gap: bool,
}

fn pick<T: DeserializeOwned>(value: &Value) -> Option<T> {
    serde_json::from_value(value.clone()).ok()
}

fn classify(event_type: &str) -> (EventCategory, EventKind) {
    use EventCategory::{Approval, Authority, Lease, Runtime, Session, Turn};
    use EventKind::*;
    match event_type {
        "authority.managed_session.prepared" => (Session, SessionPrepared),
        "authority.managed_session.closed" => (Session, SessionClosed),
        "authority.managed_session.runtime_bound" => (Session, SessionBound),
        "authority.managed_session.runtime_attached" => (Session, SessionAttached),
        "authority.managed_session.runtime_operation_indeterminate" => {
            (Session, BindingIndeterminate)
        }
        "authority.managed_session.runtime_operation_abandoned" => (Session, BindingAbandoned),
        "authority.runtime.desired_state_changed" => (Runtime, RuntimeDesired),
        "authority.runtime.instance_ready" => (Runtime, RuntimeReady),
        "authority.runtime.instance_exited" => (Runtime, RuntimeExited),
        "authority.managed_turn.admitted" => (Turn, TurnAdmitted),
        "authority.managed_turn.running" => (Turn, TurnRunning),
        "authority.managed_turn.cancel_requested" => (Turn, TurnCancelRequested),
        "authority.managed_turn.completed" => (Turn, TurnCompleted),
        "authority.managed_turn.interrupted" => (Turn, TurnInterrupted),
        "authority.managed_turn.failed" => (Turn, TurnFailed),
        "authority.managed_turn.indeterminate" => (Turn, TurnIndeterminate),
        "authority.managed_approval.requested" => (Approval, ApprovalRequested),
        "authority.managed_approval.accepted" => (Approval, ApprovalAccepted),
        "authority.managed_approval.confirmed" => (Approval, ApprovalConfirmed),
        "authority.managed_approval.declined" => (Approval, ApprovalDeclined),
        "authority.managed_approval.cancelled" => (Approval, ApprovalCancelled),
        "authority.managed_approval.expired" => (Approval, ApprovalExpired),
        "authority.managed_approval.invalidated" => (Approval, ApprovalInvalidated),
        "authority.managed_approval.indeterminate" => (Approval, ApprovalIndeterminate),
        "authority.execution_lease.granted" => (Lease, LeaseGranted),
        "authority.execution_lease.renewed" => (Lease, LeaseRenewed),
        "authority.execution_lease.revoked" => (Lease, LeaseRevoked),
        "authority.execution_lease.expired" => (Lease, LeaseExpired),
        _ => (Authority, Other),
    }
}

/// The facts of a known kind, picked one by one. A field that is not what this build
/// knows it as is left out; nothing else of the payload is looked at.
fn facts(kind: EventKind, payload: &Value) -> EventFacts {
    use EventKind::*;
    let mut facts = EventFacts::default();
    match kind {
        RuntimeDesired => facts.desired = pick(&payload["desired_state"]),
        RuntimeReady => {
            facts.runtime_version = payload["runtime_version"]
                .as_str()
                .filter(|v| {
                    !v.is_empty()
                        && v.len() <= VERSION_MAX
                        && v.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
                })
                .map(str::to_owned);
        }
        RuntimeExited => {
            facts.exit = pick(&payload["exit_kind"]);
            facts.expected = payload["expected"].as_bool();
            facts.desired = pick(&payload["desired_state"]);
        }
        TurnCancelRequested | TurnCompleted | TurnInterrupted | TurnFailed | TurnIndeterminate => {
            facts.turn_reason = pick(&payload["reason"]);
        }
        ApprovalRequested
        | ApprovalAccepted
        | ApprovalConfirmed
        | ApprovalDeclined
        | ApprovalCancelled
        | ApprovalExpired
        | ApprovalInvalidated
        | ApprovalIndeterminate => {
            facts.approval_kind = pick(&payload["kind"]);
            facts.approval_reason = pick(&payload["reason"]);
            facts.approval_effect = pick(&payload["effect"]);
        }
        LeaseGranted | LeaseRenewed | LeaseRevoked | LeaseExpired => {
            facts.scopes = pick(&payload["scopes"]);
            facts.expires_at = pick(&payload["expires_at"]);
            facts.revocation = pick(&payload["revocation_reason"]);
        }
        SessionPrepared | SessionClosed | SessionBound | SessionAttached | BindingIndeterminate
        | BindingAbandoned | TurnAdmitted | TurnRunning | Other => (),
    }
    facts
}

fn disposition(kind: EventKind, facts: &EventFacts) -> Disposition {
    use EventKind::*;
    let isolation = facts.turn_reason == Some(TurnReason::InferenceIsolationViolation)
        || facts.approval_reason == Some(ApprovalReason::IsolationViolation)
        || matches!(
            facts.exit,
            Some(
                RuntimeDiagnostic::InferenceIsolationViolation | RuntimeDiagnostic::SafetyUnproven
            )
        );
    if isolation {
        return Disposition::Security;
    }
    match kind {
        TurnFailed => Disposition::Failure,
        RuntimeExited if facts.expected != Some(true) => Disposition::Failure,
        TurnInterrupted if facts.turn_reason != Some(TurnReason::UserCancel) => {
            Disposition::Attention
        }
        TurnIndeterminate
        | BindingIndeterminate
        | ApprovalCancelled
        | ApprovalExpired
        | ApprovalInvalidated
        | ApprovalIndeterminate => Disposition::Attention,
        _ => Disposition::Normal,
    }
}

/// Reduces one envelope, serialized as the authority sent it, to a row. The envelope was
/// validated when it was received; what is checked here is that it belongs to `stream`.
pub(super) fn project(envelope: &Value, stream: &WireUuid) -> Result<EventRow, ControlError> {
    let same = envelope["stream_id"]
        .as_str()
        .is_some_and(|id| id.eq_ignore_ascii_case(stream.as_str()));
    let (Some(sequence), Some(at), Some(event_type), true) = (
        pick::<Sequence>(&envelope["sequence"]),
        pick::<UtcTimestamp>(&envelope["timestamp"]),
        envelope["event_type"].as_str(),
        same,
    ) else {
        return Err(ControlError::CorrelationError);
    };
    let (category, kind) = classify(event_type);
    let payload = &envelope["payload"];
    let facts = facts(kind, payload);
    let disposition = disposition(kind, &facts);
    // An unknown kind names nothing but where it is: its payload is not looked at.
    let known = kind != EventKind::Other;
    Ok(EventRow {
        sequence,
        at,
        category,
        kind,
        disposition,
        security: category == EventCategory::Lease || disposition == Disposition::Security,
        session: pick(&envelope["session_id"]),
        turn: known
            .then(|| pick(&payload["managed_turn_id"]))
            .flatten()
            .filter(|_| matches!(category, EventCategory::Turn | EventCategory::Approval)),
        approval: known
            .then(|| pick(&payload["managed_approval_id"]))
            .flatten()
            .filter(|_| category == EventCategory::Approval),
        facts,
    })
}
