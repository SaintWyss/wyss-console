//! What the window is told about managed turns. Canonical lifecycle and the text of the
//! assistant's answer, and nothing the product has no use for: no provider identity, no
//! lease, no runtime instance, no reasoning text and no detail of a mediated action.
use crate::contracts::{
    Sequence, UtcTimestamp,
    control::{
        ApprovalKind, ApprovalState, ApprovalStatus, BindingState, ControlState, ManagedTurnStatus,
        ManagedTurnStreamEventV1, StreamKind, TurnReason, TurnState,
    },
    identities::{ManagedSessionId, ManagedTurnId},
};
use serde::Serialize;

pub(super) const VIEW_SCHEMA_VERSION: u8 = 1;
/// Turns of one session the window is given at once, newest first.
pub const TURN_PAGE: usize = 20;

/// One managed turn as the authority holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnSummary {
    pub id: ManagedTurnId,
    pub session: ManagedSessionId,
    pub state: TurnState,
    pub revision: u64,
    pub started_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub ended_at: Option<UtcTimestamp>,
    /// Why it ended, once it has.
    pub reason: Option<TurnReason>,
    /// The authority still holds output of this turn to read. It keeps a few turns, in
    /// memory, and none across its own restart.
    pub output: bool,
    /// Presentation hint: the turn has not ended. Whether it can be cancelled is the
    /// authority's to say.
    pub cancellable: bool,
}
impl TurnSummary {
    pub(super) fn of(status: ManagedTurnStatus) -> Self {
        let turn = status.turn;
        Self {
            cancellable: !turn.state.terminal(),
            id: turn.managed_turn_id,
            session: turn.managed_session_id,
            state: turn.state,
            revision: turn.revision,
            started_at: turn.created_at,
            updated_at: turn.updated_at,
            ended_at: turn.terminal_at,
            reason: turn.terminal_reason,
            output: status.stream_retained,
        }
    }
}

/// What one stream event is, for the product. A mediated action is named by what kind of
/// thing happened to it, never by its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEventKind {
    Started,
    /// A piece of the assistant's answer. The only kind that carries provider text.
    Text,
    MessageEnd,
    /// The model is reasoning. Its reasoning is not part of this projection.
    Thinking,
    Completed,
    Interrupted,
    Failed,
    /// Events before this one were not kept.
    Gap,
    CommandProposed,
    CommandStarted,
    CommandOutput,
    CommandCompleted,
    CommandFailed,
    FileChangeProposed,
    FileChangeStarted,
    FileChangeCompleted,
    FileChangeFailed,
    ApprovalRequired,
    /// `text` is the state the approval ended in.
    ApprovalResolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnEvent {
    /// Position in the turn's stream. Opaque: it is only ever handed back as a cursor.
    pub sequence: Sequence,
    pub kind: TurnEventKind,
    pub text: Option<String>,
}
impl TurnEvent {
    pub(super) fn of(event: ManagedTurnStreamEventV1) -> Self {
        use StreamKind as S;
        use TurnEventKind as K;
        let (kind, text) = match event.kind {
            S::TurnStarted => (K::Started, None),
            S::AssistantMessageDelta => (K::Text, event.text),
            S::AssistantMessageCompleted => (K::MessageEnd, None),
            // Reasoning summaries are the model's reasoning: that it happens is shown,
            // what it says is dropped here and never reaches the window.
            S::ReasoningSummaryDelta | S::ReasoningSummaryPart => (K::Thinking, None),
            S::TurnCompleted => (K::Completed, None),
            S::TurnInterrupted => (K::Interrupted, None),
            S::TurnFailed => (K::Failed, None),
            S::StreamGap => (K::Gap, None),
            // A command, its working directory and its output are the approval's to
            // present, to whoever decides it. Here they are an activity and no more.
            S::CommandProposed => (K::CommandProposed, None),
            S::CommandStarted => (K::CommandStarted, None),
            S::CommandOutputDelta => (K::CommandOutput, None),
            S::CommandCompleted => (K::CommandCompleted, None),
            S::CommandFailed => (K::CommandFailed, None),
            S::FileChangeProposed => (K::FileChangeProposed, None),
            S::FileChangeStarted => (K::FileChangeStarted, None),
            S::FileChangeCompleted => (K::FileChangeCompleted, None),
            S::FileChangeFailed => (K::FileChangeFailed, None),
            S::ApprovalRequired => (K::ApprovalRequired, None),
            // Only a state the contract names is passed on.
            S::ApprovalResolved => (
                K::ApprovalResolved,
                event.text.filter(|text| {
                    serde_json::from_value::<ApprovalState>(serde_json::json!(text)).is_ok()
                }),
            ),
        };
        Self {
            sequence: event.sequence,
            kind,
            text,
        }
    }
}

/// The approval a turn is waiting on: what kind of action, and until when. The action
/// itself is for whoever answers it, and nothing here can answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalNotice {
    pub kind: ApprovalKind,
    pub expires_at: UtcTimestamp,
}
impl ApprovalNotice {
    pub(super) fn of(status: ApprovalStatus) -> Self {
        Self {
            kind: status.approval.kind,
            expires_at: status.approval.expires_at,
        }
    }
}

/// One reading of a turn: its canonical record, read after the events it comes with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnView {
    pub schema_version: u8,
    pub turn: TurnSummary,
    /// Stream events after the cursor that was asked for, in order.
    pub events: Vec<TurnEvent>,
    /// The cursor to ask from next. Unchanged when nothing new was read.
    pub next: Option<Sequence>,
    /// More events are already there to read.
    pub more: bool,
    /// Events before the first one returned were not kept by the authority.
    pub gap: bool,
    /// The approval the turn is waiting on, while it waits.
    pub approval: Option<ApprovalNotice>,
}

/// The authority accepted an intent for this turn. What the turn is now is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnReceipt {
    pub id: ManagedTurnId,
}

/// The session a list of turns belongs to, as the authority holds it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionFacts {
    pub id: ManagedSessionId,
    pub state: ControlState,
    pub revision: u64,
    pub binding: BindingState,
    pub workspace: Option<String>,
}

/// The recent turns of one session, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnList {
    pub schema_version: u8,
    /// The durable head read before the turns: the list is at least this new.
    pub head: Option<Sequence>,
    pub session: SessionFacts,
    pub turns: Vec<TurnSummary>,
    /// Presentation hint: prepared, attached and with no turn of its own running. Whether
    /// a turn may start is the authority's to say.
    pub runnable: bool,
    /// Older history exists that this desktop did not read.
    pub partial: bool,
}
