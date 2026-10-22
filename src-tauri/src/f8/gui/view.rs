//! What the window is told about managed runtimes of protocol 2.
//!
//! Every payload names the authority instance it was read from, so a window that sees the
//! instance change drops every fence, cursor and handle it held. Nothing here names a
//! control method, a process, a path of the runtime or a provider identity: the window gets
//! the fence it must hand back (the expected observation, which grants nothing) and the
//! lifecycle the authority recorded. Content (a prompt, an answer, an action under
//! approval) never prints through `Debug`.
use crate::{
    authority::{Connection, Fault},
    contracts::{
        Sequence, UtcTimestamp, WireUuid,
        binding::ObservedModel,
        control::{
            ActualState, AttachmentProfile, BindingState, ControlError, ControlState, DesiredState,
            ExecutionLease, ExecutionScope, GateReason, LeaseState, ManagedSessionRecord,
            RevocationReason, RuntimeDiagnostic, RuntimeSafetyProfile,
        },
        identities::{
            AgentRuntimeInstanceId, ExecutionLeaseId, ManagedApprovalId, ManagedSessionId,
            ManagedTurnId,
        },
    },
    events::EventsPage,
    f8::protocol::RuntimeFence,
    sessions::CatalogueEntry,
    turns::{ApprovalNotice, TurnEvent, TurnSummary},
};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u8 = 1;
/// Sessions one reading shows. The authority holds at most 128 over its lifetime.
pub const SESSION_PAGE: usize = 25;
/// Turns of one session shown with it.
pub const TURN_PAGE: usize = 8;

/// Why a call did not succeed, in product terms. Closed: no protocol text crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum F8Failure {
    /// This desktop was not started with the protocol-2 interface. Nothing was sent.
    Inactive,
    /// No protocol-2 authority answered, or it stopped answering. The outcome may be unknown.
    Unavailable,
    /// The peer speaks another protocol or storage version, or broke the protocol.
    Incompatible,
    /// The request is not one this desktop sends. Decided here, before anything is asked.
    Invalid,
    NotFound,
    /// What the window saw is not what the authority holds: a revision, a generation or a
    /// runtime instance changed. Read again before deciding again.
    Stale,
    /// The state does not permit this.
    NotAllowed,
    NotBound,
    /// The runtime is not running, not ready or not the session's.
    RuntimeUnavailable,
    /// A managed turn is running. The runtime it runs on is not stopped under it.
    TurnActive,
    WorkspaceInvalid,
    AlreadyBound,
    /// The execution gate denied it: no usable lease for this action.
    PermissionDenied,
    /// A lease for the session is already active.
    LeaseActive,
    /// The authority is doing something that excludes this, or is at capacity.
    Busy,
    /// The agent runtime refused.
    Rejected,
    /// The authority could not tell whether the runtime acted.
    Uncertain,
    /// The intent was already used for something else.
    Conflict,
    /// The user did not allow it in the confirmation window of the desktop, or that window
    /// could not be shown. Nothing was sent to the authority.
    NotConfirmed,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct F8Error {
    pub code: F8Failure,
    /// Nothing was refused: sending the same intent again is safe and settles the outcome.
    pub retry: bool,
    /// Milliseconds until the link tries again, while it waits.
    pub retry_in_ms: Option<u64>,
    #[serde(skip)]
    pub cause: Option<ControlError>,
}
impl F8Error {
    pub const fn new(code: F8Failure) -> Self {
        Self {
            code,
            retry: matches!(code, F8Failure::Unavailable | F8Failure::Busy),
            retry_in_ms: None,
            cause: None,
        }
    }
    pub(crate) fn of(error: ControlError) -> Self {
        use ControlError as E;
        use F8Failure as F;
        let code = match error {
            E::TransportError
            | E::Timeout
            | E::TruncatedFrame
            | E::StorageUnavailable
            | E::AuthorityUnavailable
            | E::AlreadyRunning
            | E::UnsafePath
            | E::PeerRejected => F::Unavailable,
            E::ProtocolVersionUnsupported
            | E::StorageIncompatible
            | E::CorrelationError
            | E::FrameTooLarge
            | E::HandshakeRequired
            | E::UnsupportedMethod => F::Incompatible,
            E::InvalidRequest | E::Unsupported => F::Invalid,
            E::SessionNotFound | E::TurnNotFound | E::ApprovalNotFound | E::LeaseNotFound => {
                F::NotFound
            }
            E::StaleRevision => F::Stale,
            E::InvalidTransition => F::NotAllowed,
            E::BindingNotFound => F::NotBound,
            E::BindingAlreadyExists => F::AlreadyBound,
            E::WorkspaceInvalid => F::WorkspaceInvalid,
            E::GateDenied => F::PermissionDenied,
            E::LeaseAlreadyActive => F::LeaseActive,
            E::ActiveTurnExists => F::TurnActive,
            E::RuntimeOperationInProgress | E::CapacityExceeded => F::Busy,
            E::ProviderRejected => F::Rejected,
            E::ExternalEffectUnknown | E::BindingIndeterminate => F::Uncertain,
            E::IntentConflict => F::Conflict,
            E::InternalError => F::Internal,
        };
        Self {
            cause: Some(error),
            ..Self::new(code)
        }
    }
}
impl std::fmt::Display for F8Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.code)
    }
}
impl std::error::Error for F8Error {}

// ---------------------------------------------------------------- link and authority

#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    pub state: Connection,
    pub fault: Option<Fault>,
    pub retry_in_ms: Option<u64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct AuthorityView {
    pub instance_id: WireUuid,
    pub service_version: String,
    pub protocol_version: u8,
    pub storage_schema_version: u8,
    pub stream_id: WireUuid,
    pub last_sequence: Option<Sequence>,
    pub prepared_count: u64,
    pub closed_count: u64,
}
/// What the desktop knows about the interface and its link. `active: false` is the
/// installed configuration: nothing in the window may offer an action then.
#[derive(Debug, Clone, Serialize)]
pub struct StateView {
    pub schema_version: u8,
    pub active: bool,
    pub link: LinkView,
    pub authority: Option<AuthorityView>,
    /// Counts the times a greeting found another authority instance than the one before.
    /// A change means every fence and handle the window holds is stale.
    pub epoch: u64,
}

// ---------------------------------------------------------------- runtime and sessions

/// The expected observation a mutation is asked against. It grants nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FenceView {
    pub session: ManagedSessionId,
    pub generation: u64,
    pub instance: Option<AgentRuntimeInstanceId>,
}
impl FenceView {
    pub fn fence(&self) -> RuntimeFence {
        RuntimeFence {
            managed_session_id: self.session.clone(),
            generation: self.generation,
            runtime_instance_id: self.instance.clone(),
        }
    }
    pub fn of(fence: &RuntimeFence) -> Self {
        Self {
            session: fence.managed_session_id.clone(),
            generation: fence.generation,
            instance: fence.runtime_instance_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeView {
    pub desired: DesiredState,
    pub actual: ActualState,
    /// Revision of the desired state: what a start or a stop is asked against.
    pub revision: u64,
    /// The generation counter, as far as a number the window can hold exactly goes.
    pub generation: u64,
    /// The counter is at its end: the authority will start no further launch for this session.
    pub exhausted: bool,
    /// The identity published once the process was attested; null before and after.
    pub instance: Option<AgentRuntimeInstanceId>,
    pub version: Option<String>,
    pub profile: Option<RuntimeSafetyProfile>,
    pub workspace: Option<String>,
    pub model_count: u16,
    pub diagnostic: Option<RuntimeDiagnostic>,
    /// The closed budget key that ended it, when a resource budget did.
    pub budget_failure: Option<String>,
    pub fence: FenceView,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionRow {
    pub id: ManagedSessionId,
    pub state: ControlState,
    pub revision: u64,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub position: Sequence,
    pub runtime: RuntimeView,
}
impl SessionRow {
    pub fn of(record: ManagedSessionRecord, runtime: RuntimeView) -> Self {
        Self {
            id: record.managed_session_id,
            state: record.control_state,
            revision: record.revision,
            created_at: record.created_at,
            updated_at: record.updated_at,
            position: record.created_sequence,
            runtime,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionsView {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub head: Option<Sequence>,
    /// Newest first.
    pub sessions: Vec<SessionRow>,
    /// Cursor of the next older page, when older sessions exist.
    pub older: Option<Sequence>,
    /// How many sessions the authority holds and how many a lifetime allows.
    pub capacity: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionReceipt {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub id: ManagedSessionId,
}

#[derive(Debug, Clone, Serialize)]
pub struct BindingView {
    pub state: BindingState,
    pub workspace: Option<String>,
    pub mode: Option<AttachmentProfile>,
    /// The operation an indeterminate create left, to abandon it.
    pub operation: Option<WireUuid>,
    /// What the runtime itself reported for the thread, only while attached.
    pub observed_model: Option<ObservedModel>,
    pub attached_instance: Option<AgentRuntimeInstanceId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeaseView {
    pub id: ExecutionLeaseId,
    pub state: LeaseState,
    pub scopes: Vec<ExecutionScope>,
    pub revision: u64,
    pub issued_at: UtcTimestamp,
    pub expires_at: UtcTimestamp,
    pub revocation: Option<RevocationReason>,
}
impl LeaseView {
    pub fn of(lease: ExecutionLease) -> Self {
        Self {
            id: lease.lease_id,
            state: lease.state,
            scopes: lease.scopes,
            revision: lease.revision,
            issued_at: lease.issued_at,
            expires_at: lease.expires_at,
            revocation: lease.revocation_reason,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct GateRow {
    pub action: ExecutionScope,
    pub reason: GateReason,
}

/// A turn as the authority holds it, with the runtime launch it was admitted on.
#[derive(Debug, Clone, Serialize)]
pub struct TurnRow {
    pub turn: TurnSummary,
    /// The generation and instance it ran on, from the immutable record. Null for a turn
    /// that predates protocol 2.
    pub generation: Option<u64>,
    /// The launch is the one that is attested and live now.
    pub live: bool,
}

#[derive(Clone, Serialize)]
pub struct SessionDetail {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub session: SessionRow,
    pub binding: BindingView,
    pub lease: Option<LeaseView>,
    pub gates: Vec<GateRow>,
    /// Newest first.
    pub turns: Vec<TurnRow>,
    /// Older turns exist that this desktop did not look for; the history has them.
    pub turns_partial: bool,
    /// The approval the newest running turn waits on, with its action.
    pub approval: Option<ApprovalDetail>,
}
impl std::fmt::Debug for SessionDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionDetail(<sensitive>)")
    }
}

/// One reading of a turn: its record, the stream events after the cursor, and the approval
/// it waits on.
#[derive(Clone, Serialize)]
pub struct TurnReading {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub turn: TurnRow,
    pub events: Vec<TurnEvent>,
    pub next: Option<Sequence>,
    pub more: bool,
    pub gap: bool,
    pub approval: Option<ApprovalNotice>,
}
impl std::fmt::Debug for TurnReading {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TurnReading(<sensitive>)")
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct TurnReceipt {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub id: ManagedTurnId,
}
#[derive(Debug, Clone, Serialize)]
pub struct LeaseReceipt {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub lease: LeaseView,
}
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeReceipt {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub session: ManagedSessionId,
    pub desired: DesiredState,
    pub revision: u64,
    pub generation: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct BindingReceipt {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub session: ManagedSessionId,
    pub workspace: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct DecisionReceipt {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub id: ManagedApprovalId,
    pub state: crate::contracts::control::ApprovalState,
}
#[derive(Debug, Clone, Serialize)]
pub struct AbandonReceipt {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub session: ManagedSessionId,
}

/// The models the runtime of one launch listed. Offered is not entitled; a model is never
/// selected here, because protocol 2 has no request that selects one.
#[derive(Debug, Clone, Serialize)]
pub struct ModelsView {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub fence: FenceView,
    pub version: String,
    pub complete: Option<bool>,
    pub entries: Vec<CatalogueEntry>,
    /// What the session's thread reported it uses, while attached.
    pub observed: Option<ObservedModel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditView {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub page: EventsPage,
}
#[derive(Debug, Clone, Serialize)]
pub struct WatchView {
    pub schema_version: u8,
    pub authority: WireUuid,
    pub stream: WireUuid,
    /// The durable head after waiting. Equal to the cursor when nothing was committed.
    pub head: Option<Sequence>,
    pub changed: bool,
}

/// Supported decisions are transient capabilities of this connection, not stored history.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalDetail {
    pub approval: crate::turns::ApprovalSummary,
    pub workspace: Option<String>,
    pub command: Option<crate::turns::CommandView>,
    pub files: Option<u16>,
    pub file: Option<crate::turns::FileView>,
    pub decisions: Vec<crate::contracts::control::ApprovalDecision>,
}
impl std::fmt::Debug for ApprovalDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalDetail(<sensitive>)")
    }
}
impl ApprovalDetail {
    pub(super) fn of(
        status: crate::contracts::approval::ApprovalStatus,
        workspace: Option<String>,
        decisions: Vec<crate::contracts::control::ApprovalDecision>,
    ) -> Self {
        let d = crate::turns::ApprovalDetail::of(status, workspace);
        Self {
            approval: d.approval,
            workspace: d.workspace,
            command: d.command,
            files: d.files,
            file: d.file,
            decisions,
        }
    }
}
