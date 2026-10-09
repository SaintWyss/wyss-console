//! Entering a mode: the session steps that may need the one managed runtime stopped.
//!
//! They live with the turn service because stopping the runtime ends whatever turn runs
//! on it, and this is the service that can read whether one does. The steps themselves
//! are the session service's (`sessions::activation`); what is added here is the answer
//! to "is a turn running?", read from the authority through the same connection, right
//! before a stop would be asked for.
//!
//! That reading is a courtesy, not the guard. It and the stop are two requests, and a
//! turn another client starts between them is not this side's to see. The authority
//! refuses a stop while a managed turn is active, on the thread that admits turns; a
//! stop that arrives too late is answered `active_turn_exists` and said here as a
//! running turn. What this side adds is that it does not ask at all when it can tell.
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
