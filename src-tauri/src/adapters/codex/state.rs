use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitializationState {
    NotStarted,
    AwaitingInitializeResponse(CodexRequestId),
    AwaitingInitialized,
    Initialized,
    Failed,
    Closed,
}
/// Offline ordering guard. No IO, process ownership, request registry or retries.
#[derive(Debug, Clone)]
pub struct ProtocolState {
    state: InitializationState,
    experimental_enabled: bool,
}
impl Default for ProtocolState {
    fn default() -> Self {
        Self {
            state: InitializationState::NotStarted,
            experimental_enabled: false,
        }
    }
}
impl ProtocolState {
    pub fn state(&self) -> &InitializationState {
        &self.state
    }
    pub fn experimental_enabled(&self) -> bool {
        self.experimental_enabled
    }
    pub fn outbound(&mut self, message: &WireMessage) -> Result<()> {
        if message.direction() != Direction::ClientToServer {
            return fail("wrong_direction_or_kind");
        }
        match (&self.state, message.method()) {
            (InitializationState::NotStarted, Some("initialize")) => {
                self.experimental_enabled = message
                    .params()
                    .and_then(|p| p.get("capabilities"))
                    .and_then(|p| p.get("experimentalApi"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                self.state =
                    InitializationState::AwaitingInitializeResponse(message.id().unwrap().clone());
                Ok(())
            }
            (InitializationState::AwaitingInitialized, Some("initialized")) => {
                self.state = InitializationState::Initialized;
                Ok(())
            }
            (InitializationState::Initialized, Some("initialize" | "initialized")) => {
                fail("invalid_initialization_order")
            }
            (InitializationState::Initialized, _) => {
                if message.surface() == Surface::Experimental && !self.experimental_enabled {
                    return fail("experimental_not_enabled");
                }
                Ok(())
            }
            _ => fail("invalid_initialization_order"),
        }
    }
    pub fn initialize_response(&mut self, message: &WireMessage) -> Result<()> {
        let InitializationState::AwaitingInitializeResponse(id) = &self.state else {
            return fail("invalid_initialization_order");
        };
        validate_response(message, id, Operation::Initialize)?;
        self.state = if message.kind() == WireKind::ErrorResponse {
            InitializationState::Failed
        } else {
            InitializationState::AwaitingInitialized
        };
        Ok(())
    }
    pub fn close(&mut self) {
        self.state = InitializationState::Closed;
    }
}
