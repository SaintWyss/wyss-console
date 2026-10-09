//! Entering a mode: the session steps that may need the one managed runtime stopped.
//!
//! They live with the turn service because stopping the runtime ends whatever turn runs
//! on it, and this is the service that can read whether one does. The steps themselves
//! are the session service's (`sessions::activation`); what is added here is the answer
//! to "is a turn running?", read from the authority through the same connection, right
//! before a stop would be asked for.
//!
//! That reading and the stop are two requests. A turn another client starts between them
//! is ended by the authority as `runtime_stopped`: the core admits a stop under a running
//! turn, and nothing on this side can close that window. What this side guarantees is
//! that it never asks for a stop while it reads a turn as running.
use super::service::TurnService;
use crate::{
    contracts::{WireUuid, identities::ManagedSessionId},
    sessions::{ActivationProgress, AgentMode, SessionError},
};

impl TurnService {
    /// One step towards a session bound to `workspace` and attached in `mode`.
    pub fn activate(
        &self,
        operation: WireUuid,
        session: ManagedSessionId,
        expected_revision: u64,
        workspace: String,
        mode: AgentMode,
    ) -> Result<ActivationProgress, SessionError> {
        self.inner.sessions.activate_in(
            operation,
            session,
            expected_revision,
            workspace,
            mode,
            &|client| self.active(client),
        )
    }

    /// One step towards a bound session attached in `mode`, for the workspace the
    /// authority holds for it.
    pub fn enter(
        &self,
        operation: WireUuid,
        session: ManagedSessionId,
        expected_revision: u64,
        mode: AgentMode,
    ) -> Result<ActivationProgress, SessionError> {
        self.inner
            .sessions
            .enter(operation, session, expected_revision, mode, &|client| {
                self.active(client)
            })
    }
}
