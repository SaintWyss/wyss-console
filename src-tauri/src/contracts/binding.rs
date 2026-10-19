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
/// Where a model the runtime reported was read: the answer to starting or resuming the thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedModelSource {
    ThreadStart,
    ThreadResume,
}
/// The model a managed runtime itself reported for a session's thread (ADR 0031, option A):
/// transient attachment state, kept in memory while attached and gone with the attachment.
/// It is what the provider said, never a default, a catalogue entry or a configured value; a
/// report that is missing or out of bounds is no observation at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedModel {
    pub model_id: String,
    pub provider_id: Option<String>,
    pub reasoning_effort: Option<String>,
    pub source: ObservedModelSource,
}
impl ObservedModel {
    const MOST_ID: usize = 256;
    const MOST_WORD: usize = 64;
    fn word_ok(text: &str, most: usize) -> bool {
        !text.is_empty() && text.len() <= most && text.chars().all(|c| c.is_ascii_graphic())
    }
    /// From the answer to `thread/start` or `thread/resume`; `None` when nothing usable.
    pub fn from_thread_answer(
        answer: &serde_json::Value,
        source: ObservedModelSource,
    ) -> Option<Self> {
        let model_id = answer.get("model")?.as_str()?;
        if !Self::word_ok(model_id, Self::MOST_ID) {
            return None;
        }
        let word = |key: &str| {
            answer
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|v| Self::word_ok(v, Self::MOST_WORD))
                .map(str::to_owned)
        };
        Some(Self {
            model_id: model_id.to_owned(),
            provider_id: word("modelProvider"),
            reasoning_effort: word("reasoningEffort"),
            source,
        })
    }
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
    #[serde(default)]
    pub attachment_profile: Option<super::turn::AttachmentProfile>,
    /// What the runtime reported for the thread, while attached.
    #[serde(default)]
    pub observed_model: Option<ObservedModel>,
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

#[cfg(test)]
mod observed_model_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_what_the_runtime_reported_is_an_observation() {
        let source = ObservedModelSource::ThreadStart;
        let seen = ObservedModel::from_thread_answer(
            &json!({"model": "gpt-x", "modelProvider": "openai", "reasoningEffort": "high"}),
            source,
        )
        .unwrap();
        assert_eq!(seen.model_id, "gpt-x");
        assert_eq!(seen.provider_id.as_deref(), Some("openai"));
        assert_eq!(seen.reasoning_effort.as_deref(), Some("high"));
        // Nothing reported, or not a usable word: no observation, never a default.
        for answer in [
            json!({}),
            json!({"model": null}),
            json!({"model": 7}),
            json!({"model": ""}),
            json!({"model": "a b"}),
            json!({"model": "x".repeat(257)}),
            json!({"model": "bad\nid"}),
        ] {
            assert_eq!(
                ObservedModel::from_thread_answer(&answer, source),
                None,
                "{answer}"
            );
        }
        // The extras are dropped one by one, not the whole report.
        let partial = ObservedModel::from_thread_answer(
            &json!({"model": "m", "modelProvider": "a b", "reasoningEffort": 3}),
            ObservedModelSource::ThreadResume,
        )
        .unwrap();
        assert_eq!(
            (partial.provider_id, partial.reasoning_effort),
            (None, None)
        );
        assert_eq!(partial.source, ObservedModelSource::ThreadResume);
    }

    #[test]
    fn a_status_from_an_older_authority_has_no_report() {
        let text = r#"{"managed_session_id":"11111111-1111-4111-8111-111111111111","state":"unbound","binding":null,"runtime_instance_id":null,"operation_id":null}"#;
        let status: BindingStatus = serde_json::from_str(text).unwrap();
        assert!(status.observed_model.is_none());
    }
}
