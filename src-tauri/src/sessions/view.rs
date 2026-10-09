use crate::contracts::{
    Sequence, UtcTimestamp,
    agent_adapter::AgentAdapterId,
    control::{
        BindingState, BindingStatus, ControlError, ControlState, GateReason, ManagedSessionRecord,
    },
    identities::ManagedSessionId,
};
use serde::Serialize;

pub(super) const PAGE_SCHEMA_VERSION: u8 = 2;
/// Sessions in one page of the window. Below the protocol's own page bound.
pub const PAGE_SIZE: usize = 25;

/// One managed session as the authority holds it, with what its binding adds. The provider
/// thread, runtime instance, operation and lease identities stay on this side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionSummary {
    pub id: ManagedSessionId,
    pub adapter: AgentAdapterId,
    pub state: ControlState,
    pub revision: u64,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    /// Where the session sits in creation order. It is the paging cursor.
    pub position: Sequence,
    pub binding: BindingState,
    /// The workspace of its binding, when it has one.
    pub workspace: Option<String>,
    /// Presentation hints: whether offering the action makes sense. The authority decides.
    pub resumable: bool,
    pub closable: bool,
    /// Prepared and without a binding or an attempt at one: a workspace can be bound.
    pub activatable: bool,
    /// Prepared, with an attempt whose outcome the authority could not confirm.
    pub recoverable: bool,
}
impl SessionSummary {
    pub(super) fn of(session: ManagedSessionRecord, binding: BindingStatus) -> Self {
        let prepared = session.control_state == ControlState::Prepared;
        Self {
            id: session.managed_session_id,
            adapter: session.adapter_id,
            state: session.control_state,
            revision: session.revision,
            created_at: session.created_at,
            updated_at: session.updated_at,
            position: session.created_sequence,
            binding: binding.state,
            workspace: binding.binding.map(|b| b.workspace_root),
            resumable: prepared && binding.state == BindingState::BoundDetached,
            closable: prepared,
            activatable: prepared && binding.state == BindingState::Unbound,
            recoverable: prepared && binding.state == BindingState::Indeterminate,
        }
    }
}

/// One page of sessions, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionPage {
    pub schema_version: u8,
    /// The durable head read before the sessions: the page is at least this new.
    pub head: Option<Sequence>,
    pub sessions: Vec<SessionSummary>,
    /// Cursor of the next older page, when older sessions exist.
    pub older: Option<Sequence>,
    /// The bounded walk ended before the newest session: these are not the latest ones.
    pub truncated: bool,
}

/// The authority accepted an intent for this session. What the session is now is read,
/// never inferred from this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionReceipt {
    pub id: ManagedSessionId,
}

/// How far an activation got, as the authority answered it. One call takes one step:
/// the caller asks again, with the same request, until the session is `Ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationStage {
    /// The managed runtime was asked to start, or is starting. Ask again once it settles.
    RuntimeStarting,
    /// The authority is still waiting for the runtime to answer a binding request.
    Creating,
    /// The workspace is bound. The session is not attached yet.
    Bound,
    /// Bound and attached to the running runtime, as the authority reads it now.
    Ready,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ActivationProgress {
    pub stage: ActivationStage,
}

/// Why a call did not succeed, in product terms. No protocol text crosses to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionFailure {
    /// No authority answered, or it stopped answering. The outcome may be unknown.
    Unavailable,
    /// The authority speaks another protocol or storage version.
    Incompatible,
    /// The request was not one this desktop sends. Decided here, before anything is asked.
    Invalid,
    NotFound,
    /// The session is not at the revision the caller saw.
    Changed,
    /// The session's state does not permit this.
    NotAllowed,
    /// The session has no runtime binding to resume.
    NotBound,
    /// The managed runtime is not running, not ready or not the session's.
    RuntimeUnavailable,
    /// The authority refused to start the managed runtime.
    StartRefused,
    /// The authority did not accept the folder as a workspace.
    WorkspaceInvalid,
    /// The session already has a workspace binding that this action did not create.
    AlreadyBound,
    /// The execution gate denied it: no usable lease for this action.
    PermissionDenied,
    /// The authority is doing something that excludes this, or is at capacity.
    Busy,
    /// The agent runtime refused.
    Rejected,
    /// The authority could not tell whether the runtime acted.
    Uncertain,
    /// The intent was already used for something else.
    Conflict,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SessionError {
    pub code: SessionFailure,
    /// Nothing was refused: sending the same intent again is safe and settles the outcome.
    pub retry: bool,
    /// What the authority or the transport said. For tests and diagnostics only.
    #[serde(skip)]
    pub cause: Option<ControlError>,
}
impl SessionError {
    pub const fn new(code: SessionFailure) -> Self {
        Self {
            code,
            retry: matches!(code, SessionFailure::Unavailable | SessionFailure::Busy),
            cause: None,
        }
    }
    pub(crate) fn of(error: ControlError) -> Self {
        use ControlError as E;
        use SessionFailure as F;
        let code = match error {
            E::TransportError
            | E::Timeout
            | E::TruncatedFrame
            | E::StorageUnavailable
            | E::AuthorityUnavailable
            | E::AlreadyRunning
            | E::UnsafePath
            | E::PeerRejected => F::Unavailable,
            E::ProtocolVersionUnsupported | E::StorageIncompatible => F::Incompatible,
            E::SessionNotFound | E::TurnNotFound => F::NotFound,
            E::StaleRevision => F::Changed,
            E::InvalidTransition => F::NotAllowed,
            E::BindingNotFound => F::NotBound,
            E::BindingAlreadyExists => F::AlreadyBound,
            E::WorkspaceInvalid => F::RuntimeUnavailable,
            E::GateDenied | E::LeaseAlreadyActive | E::LeaseNotFound => F::PermissionDenied,
            E::RuntimeOperationInProgress | E::CapacityExceeded | E::ActiveTurnExists => F::Busy,
            E::ProviderRejected => F::Rejected,
            E::ExternalEffectUnknown | E::BindingIndeterminate => F::Uncertain,
            E::IntentConflict => F::Conflict,
            _ => F::Internal,
        };
        Self {
            cause: Some(error),
            ..Self::new(code)
        }
    }
    /// The same failure, where a workspace was being bound: there, a workspace the
    /// authority refused is the folder itself. Every other failure is what it was.
    pub(super) fn binding(self) -> Self {
        match self.cause {
            Some(ControlError::WorkspaceInvalid) => Self {
                code: SessionFailure::WorkspaceInvalid,
                ..self
            },
            _ => self,
        }
    }
    /// Whether nothing was answered: the request may or may not have been taken.
    pub(crate) fn unknown(error: ControlError) -> bool {
        Self::of(error).code == SessionFailure::Unavailable
    }
    /// A denial by the execution gate, named by the authority's own diagnostic.
    pub(crate) fn denied(reason: GateReason) -> Self {
        use GateReason as R;
        use SessionFailure as F;
        let code = match reason {
            R::RuntimeMismatch
            | R::RuntimeNotRunning
            | R::RuntimeNotReady
            | R::RuntimeIdentityMissing
            | R::RuntimeIncompatible => F::RuntimeUnavailable,
            // The session is not attached to the runtime that would run the turn.
            R::BindingDetached
            | R::AttachmentRuntimeMismatch
            | R::InferenceProfileMissing
            | R::LocalToolsProfileMissing
            | R::RuntimeSafetyProfileMissing => F::RuntimeUnavailable,
            R::BindingNotFound => F::NotBound,
            R::ActiveTurnExists => F::Busy,
            R::SessionNotFound => F::NotFound,
            R::SessionNotPrepared => F::NotAllowed,
            R::AuthorityUnavailable => F::Unavailable,
            _ => F::PermissionDenied,
        };
        Self {
            cause: Some(ControlError::GateDenied),
            ..Self::new(code)
        }
    }
}
impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.code)
    }
}
impl std::error::Error for SessionError {}
