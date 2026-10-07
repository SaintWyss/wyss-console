//! Durable provider identity and transient attachment; no conversational input.
use super::control::{ControlError, ControlResult};
use super::{
    UtcTimestamp, WireUuid,
    identities::{AgentRuntimeInstanceId, ManagedSessionId},
};
use crate::adapters::codex::CodexThreadId;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingCreate {
    pub intent_id: WireUuid,
    pub managed_session_id: ManagedSessionId,
    pub expected_session_revision: u64,
    pub workspace_root: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingResume {
    pub intent_id: WireUuid,
    pub managed_session_id: ManagedSessionId,
    pub expected_session_revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingAbandon {
    pub intent_id: WireUuid,
    pub managed_session_id: ManagedSessionId,
    pub operation_id: WireUuid,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbandonedOperation {
    pub operation_id: WireUuid,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBinding {
    pub managed_session_id: ManagedSessionId,
    pub adapter_id: super::agent_adapter::AgentAdapterId,
    pub runtime_kind: super::agent_adapter::AgentRuntimeKind,
    pub runtime_session_id: CodexThreadId,
    pub workspace_root: String,
    pub binding_revision: u64,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}
impl SessionBinding {
    pub fn validate(&self) -> ControlResult<()> {
        validate_workspace_text(&self.workspace_root)?;
        if self.binding_revision != 1
            || self.adapter_id.as_str() != "codex"
            || self.runtime_kind != super::agent_adapter::AgentRuntimeKind::Codex
            || self.created_at != self.updated_at
        {
            return Err(ControlError::StorageIncompatible);
        }
        Ok(())
    }
}
pub fn validate_workspace_text(s: &str) -> ControlResult<()> {
    // Bounded below existing intent/receipt limits, including JSON escaping.
    if s.is_empty()
        || s.len() > 256
        || s.chars().any(char::is_control)
        || !std::path::Path::new(s).is_absolute()
    {
        return Err(ControlError::WorkspaceInvalid);
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingState {
    Unbound,
    Creating,
    Indeterminate,
    BoundDetached,
    BoundAttached,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingStatus {
    pub managed_session_id: ManagedSessionId,
    pub state: BindingState,
    pub binding: Option<SessionBinding>,
    pub runtime_instance_id: Option<AgentRuntimeInstanceId>,
    pub operation_id: Option<WireUuid>,
}
impl BindingStatus {
    pub fn validate(&self) -> ControlResult<()> {
        if let Some(b) = &self.binding {
            b.validate()?;
            if b.managed_session_id != self.managed_session_id {
                return Err(ControlError::InvalidRequest);
            }
        }
        if self.binding.is_some()
            != matches!(
                self.state,
                BindingState::BoundDetached | BindingState::BoundAttached
            )
            || self.runtime_instance_id.is_some() != (self.state == BindingState::BoundAttached)
        {
            return Err(ControlError::InvalidRequest);
        }
        Ok(())
    }
}
