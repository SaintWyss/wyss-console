//! Content is transient; durable turn records contain control metadata only.
use super::{
    Sequence, UtcTimestamp, WireUuid,
    control::{ControlError as E, ControlResult, timestamp, valid_revision},
    identities::{AgentRuntimeInstanceId, ExecutionLeaseId, ManagedSessionId, ManagedTurnId},
};
use crate::adapters::codex::{CodexThreadId, CodexTurnId};
use serde::{Deserialize, Serialize};

pub const MAX_PROMPT_BYTES: usize = 8192;
pub fn validate_prompt(s: &str) -> ControlResult<()> {
    if s.is_empty() || s.len() > MAX_PROMPT_BYTES {
        Err(E::InvalidRequest)
    } else {
        Ok(())
    }
}
// Deliberately no Debug: prompts must not enter diagnostics through derived formatting.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnStart {
    pub intent_id: WireUuid,
    pub managed_session_id: ManagedSessionId,
    pub expected_session_revision: u64,
    pub prompt: String,
}
impl std::fmt::Debug for TurnStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TurnStart(<sensitive>)")
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnGet {
    pub managed_turn_id: ManagedTurnId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnEvents {
    pub managed_turn_id: ManagedTurnId,
    pub after_sequence: Option<Sequence>,
    pub limit: u16,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnCancel {
    pub intent_id: WireUuid,
    pub managed_turn_id: ManagedTurnId,
    pub expected_revision: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Starting,
    Running,
    Completed,
    Interrupted,
    Failed,
    Indeterminate,
}
impl TurnState {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Starting | Self::Running)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnReason {
    Completed,
    UserCancel,
    LeaseExpired,
    LeaseRevoked,
    SessionClosed,
    RuntimeStopped,
    AuthorityShutdown,
    AuthorityRestarted,
    RuntimeFailed,
    ProviderFailed,
    DispatchUnknown,
    InferenceIsolationViolation,
    InterruptTimeout,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedTurnRecord {
    pub managed_turn_id: ManagedTurnId,
    pub managed_session_id: ManagedSessionId,
    pub codex_thread_id: CodexThreadId,
    pub codex_turn_id: Option<CodexTurnId>,
    pub admitted_lease_id: ExecutionLeaseId,
    pub admitted_lease_revision: u64,
    pub runtime_instance_id: AgentRuntimeInstanceId,
    pub state: TurnState,
    pub revision: u64,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub terminal_at: Option<UtcTimestamp>,
    pub terminal_reason: Option<TurnReason>,
}
impl ManagedTurnRecord {
    pub fn validate(&self) -> ControlResult<()> {
        valid_revision(self.revision)?;
        valid_revision(self.admitted_lease_revision)?;
        if timestamp(&self.created_at)? > timestamp(&self.updated_at)?
            || self.state.terminal() != self.terminal_at.is_some()
            || self.state.terminal() != self.terminal_reason.is_some()
            || self
                .terminal_at
                .as_ref()
                .is_some_and(|v| v != &self.updated_at)
            || (self.state == TurnState::Running && self.codex_turn_id.is_none())
        {
            return Err(E::StorageIncompatible);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentProfile {
    InferenceOnlyV1,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedTurnStatus {
    pub turn: ManagedTurnRecord,
    pub stream_earliest_sequence: Option<Sequence>,
    pub stream_latest_sequence: Option<Sequence>,
    pub stream_retained: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    TurnStarted,
    AssistantMessageDelta,
    AssistantMessageCompleted,
    ReasoningSummaryDelta,
    ReasoningSummaryPart,
    TurnCompleted,
    TurnInterrupted,
    TurnFailed,
    StreamGap,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedTurnStreamEventV1 {
    pub schema_version: u8,
    pub managed_turn_id: ManagedTurnId,
    pub managed_session_id: ManagedSessionId,
    pub codex_turn_id: Option<CodexTurnId>,
    pub sequence: Sequence,
    pub kind: StreamKind,
    pub text: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnStreamPage {
    pub events: Vec<ManagedTurnStreamEventV1>,
    pub earliest_sequence: Option<Sequence>,
    pub latest_sequence: Option<Sequence>,
    pub next_sequence: Option<Sequence>,
    pub dropped_events: u64,
    pub gap: bool,
    pub terminal: bool,
    pub retained: bool,
}
