//! Approval control metadata is durable. The action under approval is transient content.
use super::{
    Sequence, UtcTimestamp, WireUuid,
    control::{ControlError as E, ControlResult, timestamp, valid_revision},
    identities::{
        AgentRuntimeInstanceId, ExecutionLeaseId, ManagedApprovalId, ManagedSessionId,
        ManagedTurnId,
    },
};
use serde::{Deserialize, Serialize};

pub const MAX_COMMAND_BYTES: usize = 16 * 1024;
pub const MAX_CWD_BYTES: usize = 4 * 1024;
pub const MAX_PARSED_ACTIONS: usize = 64;
pub const MAX_PARSED_ACTION_BYTES: usize = 256;
pub const MAX_FILES: usize = 64;
pub const MAX_FILE_DIFF_BYTES: usize = 128 * 1024;
pub const MAX_TOTAL_DIFF_BYTES: usize = 256 * 1024;
pub const MAX_PROVIDER_ITEM_BYTES: usize = 128;
/// Upper bound of a pending approval; the admitted lease expiry may shorten it.
pub const APPROVAL_TTL_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    CommandExecution,
    FileChange,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Accepted,
    Declined,
    Cancelled,
    Expired,
    Invalidated,
    Indeterminate,
}
impl ApprovalState {
    pub fn terminal(self) -> bool {
        self != Self::Pending
    }
}
/// The only decisions a client may request. Each is an intent, never authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Accept,
    Decline,
    Cancel,
}
impl ApprovalDecision {
    pub fn state(self) -> ApprovalState {
        match self {
            Self::Accept => ApprovalState::Accepted,
            Self::Decline => ApprovalState::Declined,
            Self::Cancel => ApprovalState::Cancelled,
        }
    }
}
/// External-effect journal of the one provider response an approval may cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalEffect {
    None,
    Dispatched,
    Confirmed,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalReason {
    ClientDecision,
    ApprovalTimeout,
    LeaseExpired,
    LeaseRevoked,
    TurnCancelled,
    TurnEnded,
    SessionClosed,
    RuntimeLost,
    AuthorityShutdown,
    AuthorityRestarted,
    EffectUnknown,
    Unsupported,
    IsolationViolation,
}
/// Bounded shape of the action. Sizes and counts only: never the command, paths or diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalStructure {
    pub command_bytes: u32,
    pub cwd_bytes: u32,
    pub parsed_actions: u16,
    pub files: u16,
    pub diff_bytes: u32,
}
impl ApprovalStructure {
    pub fn validate(&self, kind: ApprovalKind) -> ControlResult<()> {
        let command = self.command_bytes != 0 || self.cwd_bytes != 0 || self.parsed_actions != 0;
        let file = self.files != 0 || self.diff_bytes != 0;
        let valid = match kind {
            ApprovalKind::CommandExecution => {
                !file
                    && (1..=MAX_COMMAND_BYTES as u32).contains(&self.command_bytes)
                    && (1..=MAX_CWD_BYTES as u32).contains(&self.cwd_bytes)
                    && usize::from(self.parsed_actions) <= MAX_PARSED_ACTIONS
            }
            // An approval exists only for a complete proposal: at least one file.
            ApprovalKind::FileChange => {
                !command
                    && (1..=MAX_FILES).contains(&usize::from(self.files))
                    && self.diff_bytes as usize <= MAX_TOTAL_DIFF_BYTES
            }
        };
        if valid {
            Ok(())
        } else {
            Err(E::StorageIncompatible)
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedApprovalRecord {
    pub managed_approval_id: ManagedApprovalId,
    pub managed_turn_id: ManagedTurnId,
    pub managed_session_id: ManagedSessionId,
    pub runtime_instance_id: AgentRuntimeInstanceId,
    pub kind: ApprovalKind,
    pub provider_item_id: String,
    pub admitted_lease_id: ExecutionLeaseId,
    pub admitted_lease_revision: u64,
    pub state: ApprovalState,
    pub revision: u64,
    pub action_fingerprint: String,
    pub structure: ApprovalStructure,
    pub decision: Option<ApprovalDecision>,
    pub effect: ApprovalEffect,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub expires_at: UtcTimestamp,
    pub terminal_at: Option<UtcTimestamp>,
    pub terminal_reason: Option<ApprovalReason>,
    /// Audit sequence of the request event: the total order approvals are listed in.
    pub created_sequence: Sequence,
}
pub fn valid_fingerprint(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub fn valid_provider_item(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_PROVIDER_ITEM_BYTES
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b':'))
}
impl ManagedApprovalRecord {
    pub fn validate(&self) -> ControlResult<()> {
        valid_revision(self.revision)?;
        valid_revision(self.admitted_lease_revision)?;
        self.structure.validate(self.kind)?;
        let created = timestamp(&self.created_at)?;
        let updated = timestamp(&self.updated_at)?;
        let expires = timestamp(&self.expires_at)?;
        let decided = match self.state {
            ApprovalState::Pending => None,
            ApprovalState::Accepted => Some(Some(ApprovalDecision::Accept)),
            ApprovalState::Declined => Some(Some(ApprovalDecision::Decline)),
            ApprovalState::Cancelled => Some(Some(ApprovalDecision::Cancel)),
            // Only an accepted approval can have an uncertain provider outcome.
            ApprovalState::Indeterminate => Some(Some(ApprovalDecision::Accept)),
            ApprovalState::Expired | ApprovalState::Invalidated => Some(None),
        };
        let effect_valid = match (self.state, self.effect) {
            (ApprovalState::Accepted, ApprovalEffect::Dispatched | ApprovalEffect::Confirmed) => {
                true
            }
            (ApprovalState::Indeterminate, ApprovalEffect::Unknown) => true,
            (ApprovalState::Accepted | ApprovalState::Indeterminate, _) => false,
            (_, ApprovalEffect::None) => true,
            _ => false,
        };
        let terminal_after_update = match &self.terminal_at {
            Some(at) => timestamp(at)? > updated,
            None => false,
        };
        let ids = [
            self.managed_turn_id.as_str(),
            self.managed_session_id.as_str(),
            self.admitted_lease_id.as_str(),
            self.runtime_instance_id.as_str(),
        ];
        if !valid_fingerprint(&self.action_fingerprint)
            || !valid_provider_item(&self.provider_item_id)
            || created > updated
            || expires <= created
            || expires - created > time::Duration::seconds(APPROVAL_TTL_SECONDS)
            || self.state.terminal() != self.terminal_at.is_some()
            || self.state.terminal() != self.terminal_reason.is_some()
            || terminal_after_update
            || decided.is_some_and(|d| d != self.decision)
            || (self.state == ApprovalState::Pending && self.decision.is_some())
            || !effect_valid
            || ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case(self.managed_approval_id.as_str()))
        {
            return Err(E::StorageIncompatible);
        }
        Ok(())
    }
}
/// Fingerprint-only receipt of one decision intent. Retrying the intent returns it unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalReceipt {
    pub managed_approval_id: ManagedApprovalId,
    pub managed_turn_id: ManagedTurnId,
    pub kind: ApprovalKind,
    pub decision: ApprovalDecision,
    pub state: ApprovalState,
    pub revision: u64,
    pub action_fingerprint: String,
    pub decided_at: UtcTimestamp,
}
impl ApprovalReceipt {
    pub fn validate(&self) -> ControlResult<()> {
        valid_revision(self.revision)?;
        timestamp(&self.decided_at)?;
        if !valid_fingerprint(&self.action_fingerprint) || self.state != self.decision.state() {
            return Err(E::StorageIncompatible);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalPending {
    pub managed_turn_id: ManagedTurnId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalGet {
    pub managed_approval_id: ManagedApprovalId,
    /// Selects one proposed file of a live file approval; its diff is paged from `offset`.
    #[serde(default)]
    pub file_index: Option<u16>,
    #[serde(default)]
    pub offset: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRespond {
    pub intent_id: WireUuid,
    pub managed_approval_id: ManagedApprovalId,
    pub expected_revision: u64,
    pub decision: ApprovalDecision,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Add,
    Delete,
    Update,
    Move,
}
pub const MAX_PATH_BYTES: usize = 1024;
/// One control frame carries at most this much of a diff; the client pages the rest.
pub const DIFF_PAGE_BYTES: usize = 8 * 1024;
// Deliberately no Debug on the presentation types: commands and diffs must not enter
// diagnostics through derived formatting.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandPresentation {
    pub command: String,
    pub cwd: String,
    pub parsed_actions: Vec<String>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilePresentation {
    pub index: u16,
    pub path: String,
    pub kind: FileChangeKind,
    pub move_path: Option<String>,
    pub diff_bytes: u32,
    pub offset: u32,
    pub chunk: String,
    pub next_offset: Option<u32>,
}
/// Memory-only view for the client that must decide. Lost on authority or runtime restart.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalStatus {
    pub approval: ManagedApprovalRecord,
    /// True only while the approval is pending and its transient action is still held.
    pub acceptable: bool,
    pub command: Option<CommandPresentation>,
    pub file_count: Option<u16>,
    pub file: Option<FilePresentation>,
}
impl std::fmt::Debug for ApprovalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalStatus(<sensitive>)")
    }
}
impl ApprovalStatus {
    pub fn validate(&self) -> ControlResult<()> {
        self.approval.validate().map_err(|_| E::InvalidRequest)?;
        let transient = self.command.is_some() || self.file_count.is_some();
        let command_kind = self.approval.kind == ApprovalKind::CommandExecution;
        if (self.acceptable && (self.approval.state != ApprovalState::Pending || !transient))
            || (self.command.is_some() && !command_kind)
            || (self.file_count.is_some() && command_kind)
            || (self.file.is_some() && self.file_count.is_none())
            || self.file_count.is_some_and(|n| usize::from(n) > MAX_FILES)
            || self.command.as_ref().is_some_and(|c| {
                c.command.len() > MAX_COMMAND_BYTES
                    || c.cwd.len() > MAX_CWD_BYTES
                    || c.parsed_actions.len() > MAX_PARSED_ACTIONS
            })
            || self.file.as_ref().is_some_and(|f| {
                f.chunk.len() > DIFF_PAGE_BYTES
                    || f.path.len() > MAX_PATH_BYTES
                    || f.move_path
                        .as_ref()
                        .is_some_and(|p| p.len() > MAX_PATH_BYTES)
                    || f.diff_bytes as usize > MAX_FILE_DIFF_BYTES
            })
        {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }
}
