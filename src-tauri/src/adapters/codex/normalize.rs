use super::{CodecError, Result, fail, protocol::*, wire::*};
use crate::contracts::{
    agent_adapter::{
        AdapterErrorV1, AgentModelDescriptorV1, ManagedAgentSessionObservationV1,
        ManagedAgentSessionState, ModelProviderId,
    },
    identities::{AgentRuntimeInstanceId, ManagedSessionId},
};
use serde_json::{Value, json};

/// Caller-owned mapping only. This module never allocates a canonical session.
#[derive(Debug, Clone)]
pub struct SessionBinding {
    pub managed_session_id: ManagedSessionId,
    pub runtime_instance_id: AgentRuntimeInstanceId,
    pub thread_id: CodexThreadId,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcome {
    InProgress,
    Completed,
    Failed,
    Interrupted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    UserMessage,
    AgentMessage,
    CommandExecution,
    FileChange,
    McpToolCall,
    Plan,
    ReasoningSummary,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStage {
    Started,
    Completed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    AgentMessage,
    CommandOutput,
    ReasoningSummary,
    ToolProgress,
    Plan,
}
#[derive(Debug, Clone)]
pub struct ItemObservation {
    pub thread_id: CodexThreadId,
    pub turn_id: CodexTurnId,
    pub item_id: CodexItemId,
    pub kind: ItemKind,
    pub stage: ItemStage,
    pub surface: Surface,
    pub status: Option<String>,
    /// Explicitly selected metadata, not a complete wire item. Raw reasoning is absent.
    pub detail: Value,
}
#[derive(Debug, Clone)]
pub struct ApprovalObservation {
    pub request_id: CodexRequestId,
    pub kind: ApprovalKind,
    pub thread_id: CodexThreadId,
    pub turn_id: CodexTurnId,
    pub item_id: CodexItemId,
    pub surface: Surface,
    /// Vendor metadata is untrusted. It never represents Wyss authorization.
    pub vendor_params: Value,
}
#[derive(Debug, Clone)]
pub struct CatalogModel {
    pub model: AgentModelDescriptorV1,
    /// Selected Codex catalog metadata; hidden/default/efforts are not entitlement.
    pub codex: Value,
}
/// Internal normalization values, not EventEnvelope, UI IPC or durable records.
/// No Serialize implementation intentionally exposes this entire vendor boundary.
#[derive(Debug, Clone)]
pub enum CodexObservation {
    Session {
        thread_id: CodexThreadId,
        state: ManagedAgentSessionState,
        common: Option<ManagedAgentSessionObservationV1>,
    },
    Turn {
        thread_id: CodexThreadId,
        turn_id: CodexTurnId,
        outcome: TurnOutcome,
        common: Option<ManagedAgentSessionObservationV1>,
        error: Option<AdapterErrorV1>,
    },
    Item(ItemObservation),
    Content {
        thread_id: CodexThreadId,
        turn_id: CodexTurnId,
        item_id: Option<CodexItemId>,
        kind: ContentKind,
        text: String,
        detail: Value,
    },
    Approval(ApprovalObservation),
    ApprovalResolved {
        thread_id: CodexThreadId,
        request_id: CodexRequestId,
    },
    ModelCatalog {
        models: Vec<CatalogModel>,
        next_cursor: Option<String>,
    },
    UnknownNotification {
        method: String,
        vendor_params: Option<Value>,
        surface: Surface,
    },
    UnsupportedServerRequest {
        request_id: CodexRequestId,
        method: String,
        vendor_params: Option<Value>,
        surface: Surface,
    },
    SuppressedPrivateReasoning,
    Error(AdapterErrorV1),
    Acknowledged,
}
fn contract_error(_: crate::contracts::agent_adapter::AdapterContractError) -> CodecError {
    CodecError {
        code: "normalized_contract_bounds",
    }
}
fn thread(p: &serde_json::Map<String, Value>) -> Result<CodexThreadId> {
    CodexThreadId::parse(field_text(p, "threadId", 256)?)
}
fn turn(p: &serde_json::Map<String, Value>) -> Result<CodexTurnId> {
    CodexTurnId::parse(field_text(p, "turnId", 256)?)
}
fn item(p: &serde_json::Map<String, Value>) -> Result<CodexItemId> {
    CodexItemId::parse(field_text(p, "itemId", 256)?)
}
fn check_binding(binding: Option<&SessionBinding>, thread: &CodexThreadId) -> Result<()> {
    if binding.is_some_and(|b| &b.thread_id != thread) {
        return fail("session_mapping_mismatch");
    }
    Ok(())
}
fn common_session(
    binding: Option<&SessionBinding>,
    thread: &CodexThreadId,
    state: ManagedAgentSessionState,
    runtime_state: &str,
    model: Option<AgentModelDescriptorV1>,
) -> Result<Option<ManagedAgentSessionObservationV1>> {
    check_binding(binding, thread)?;
    binding.map(|b|ManagedAgentSessionObservationV1::from_value(json!({
        "schema_version":1,"managed_session_id":b.managed_session_id,"adapter_id":"codex",
        "runtime_instance_id":b.runtime_instance_id,"runtime_session_id":thread.as_str(),"state":state,
        "runtime_state":runtime_state,"observed_model":model,"terminal_id":null,
    })).map_err(contract_error)).transpose()
}
fn model(id: &str, name: Option<&str>, provider: Option<&str>) -> Result<AgentModelDescriptorV1> {
    let provider = provider.and_then(|p| ModelProviderId::parse(p).ok());
    AgentModelDescriptorV1::from_value(json!({"schema_version":1,"provider_id":provider,"model_id":id,"display_name":name,"availability":"unknown"})).map_err(contract_error)
}
fn safe_error(runtime: bool) -> Result<AdapterErrorV1> {
    AdapterErrorV1::from_value(json!({"schema_version":1,"code":if runtime {"runtime_failed"}else{"protocol_error"},"operation":null,"diagnostic":{"code":"codex.request_failed","summary":if runtime {"Codex reported failed work."}else{"Codex reported a protocol request failure."}}})).map_err(contract_error)
}
fn session_observation(
    t: &Value,
    binding: Option<&SessionBinding>,
    fallback_model: Option<&str>,
    fallback_provider: Option<&str>,
) -> Result<CodexObservation> {
    let p = object(t)?;
    let id = CodexThreadId::parse(field_text(p, "id", 256)?)?;
    let runtime_state = field_text(object(&p["status"])?, "type", 128)?;
    let state = match runtime_state {
        "idle" => ManagedAgentSessionState::Idle,
        "systemError" => ManagedAgentSessionState::Failed,
        "active" => {
            let flags = array(&p["status"]["activeFlags"])?;
            if flags
                .iter()
                .any(|f| f != "waitingOnApproval" && f != "waitingOnUserInput")
            {
                ManagedAgentSessionState::Unknown
            } else if flags.iter().any(|f| f == "waitingOnApproval") {
                ManagedAgentSessionState::WaitingForApproval
            } else if flags.iter().any(|f| f == "waitingOnUserInput") {
                ManagedAgentSessionState::WaitingForInput
            } else {
                ManagedAgentSessionState::Running
            }
        }
        _ => ManagedAgentSessionState::Unknown,
    };
    let observed_model = p
        .get("model")
        .and_then(Value::as_str)
        .or(fallback_model)
        .map(|m| {
            model(
                m,
                None,
                p.get("modelProvider")
                    .and_then(Value::as_str)
                    .or(fallback_provider),
            )
        })
        .transpose()?;
    let common = common_session(binding, &id, state, runtime_state, observed_model)?;
    Ok(CodexObservation::Session {
        thread_id: id,
        state,
        common,
    })
}
fn turn_observation(
    thread_id: CodexThreadId,
    t: &Value,
    binding: Option<&SessionBinding>,
) -> Result<CodexObservation> {
    let p = object(t)?;
    let turn_id = CodexTurnId::parse(field_text(p, "id", 256)?)?;
    let status = field_text(p, "status", 128)?;
    let (outcome, state) = match status {
        "inProgress" => (TurnOutcome::InProgress, ManagedAgentSessionState::Running),
        "completed" => (TurnOutcome::Completed, ManagedAgentSessionState::Idle),
        "failed" => (TurnOutcome::Failed, ManagedAgentSessionState::Failed),
        "interrupted" => (
            TurnOutcome::Interrupted,
            ManagedAgentSessionState::Interrupted,
        ),
        _ => return fail("invalid_enum"),
    };
    let common = common_session(binding, &thread_id, state, status, None)?;
    // The vendor error object is not copied into the safe common error.
    let error = if outcome == TurnOutcome::Failed {
        Some(safe_error(true)?)
    } else {
        None
    };
    Ok(CodexObservation::Turn {
        thread_id,
        turn_id,
        outcome,
        common,
        error,
    })
}
fn item_observation(
    p: &serde_json::Map<String, Value>,
    stage: ItemStage,
) -> Result<CodexObservation> {
    let i = object(&p["item"])?;
    let item_id = CodexItemId::parse(field_text(i, "id", 256)?)?;
    let vendor_kind = field_text(i, "type", 128)?;
    let (kind, detail) = match vendor_kind {
        "agentMessage" => (
            ItemKind::AgentMessage,
            json!({"text":i["text"],"phase":i.get("phase")}),
        ),
        "userMessage" => (
            ItemKind::UserMessage,
            json!({"content_count":array(&i["content"])?.len()}),
        ),
        "reasoning" => (
            ItemKind::ReasoningSummary,
            json!({"summary":i.get("summary").cloned().unwrap_or(json!([]))}),
        ),
        "plan" => (ItemKind::Plan, json!({"text":i["text"]})),
        "commandExecution" => (
            ItemKind::CommandExecution,
            json!({"command":i["command"],"cwd":i["cwd"],"exit_code":i.get("exitCode")}),
        ),
        "fileChange" => (
            ItemKind::FileChange,
            json!({"changes":array(&i["changes"])?.iter().map(|c|json!({"path":c["path"],"kind":c["kind"]})).collect::<Vec<_>>()}),
        ),
        "mcpToolCall" => (
            ItemKind::McpToolCall,
            json!({"server":i["server"],"tool":i["tool"]}),
        ),
        // Unknown item payload is not projected; bounded original remains in WireMessage.
        _ => (ItemKind::Unknown, json!({"vendor_type":vendor_kind})),
    };
    Ok(CodexObservation::Item(ItemObservation {
        thread_id: thread(p)?,
        turn_id: turn(p)?,
        item_id,
        kind,
        stage,
        surface: if kind == ItemKind::Plan {
            Surface::Experimental
        } else if kind == ItemKind::Unknown {
            Surface::Unknown
        } else {
            Surface::Stable
        },
        status: i.get("status").and_then(Value::as_str).map(str::to_owned),
        detail,
    }))
}

pub fn normalize_inbound(
    message: &WireMessage,
    binding: Option<&SessionBinding>,
) -> Result<CodexObservation> {
    if message.direction() != Direction::ServerToClient {
        return fail("wrong_direction_or_kind");
    }
    if message.kind() == WireKind::ErrorResponse {
        return Ok(CodexObservation::Error(safe_error(false)?));
    }
    let method = message.method().ok_or(CodecError {
        code: "response_context_required",
    })?;
    let surface = method_surface(method);
    if surface == Surface::PrivateReasoning && message.kind() == WireKind::Notification {
        return Ok(CodexObservation::SuppressedPrivateReasoning);
    }
    if message.kind() == WireKind::ServerRequest {
        if let Some(kind) = approval_kind(method) {
            let p = params(message)?;
            let thread_id = thread(p)?;
            check_binding(binding, &thread_id)?;
            return Ok(CodexObservation::Approval(ApprovalObservation {
                request_id: message.id().unwrap().clone(),
                kind,
                thread_id,
                turn_id: turn(p)?,
                item_id: item(p)?,
                surface: message.surface(),
                vendor_params: message.params().unwrap().clone(),
            }));
        }
        return Ok(CodexObservation::UnsupportedServerRequest {
            request_id: message.id().unwrap().clone(),
            method: method.to_owned(),
            vendor_params: message.params().cloned(),
            surface,
        });
    }
    if surface != Surface::Stable {
        return Ok(CodexObservation::UnknownNotification {
            method: method.to_owned(),
            vendor_params: message.params().cloned(),
            surface,
        });
    }
    let p = params(message)?;
    if method != "thread/started" {
        check_binding(binding, &thread(p)?)?;
    }
    match method {
        "thread/started" => session_observation(&p["thread"], binding, None, None),
        // Validated lifecycle notices are transient metadata, never attachment proof.
        "thread/archived" | "thread/unarchived" => Ok(CodexObservation::UnknownNotification {
            method: method.to_owned(),
            vendor_params: None,
            surface,
        }),
        "turn/started" | "turn/completed" => turn_observation(thread(p)?, &p["turn"], binding),
        "item/started" => item_observation(p, ItemStage::Started),
        "item/completed" => item_observation(p, ItemStage::Completed),
        "serverRequest/resolved" => Ok(CodexObservation::ApprovalResolved {
            thread_id: thread(p)?,
            request_id: CodexRequestId::from_value(&p["requestId"])?,
        }),
        "item/agentMessage/delta"
        | "item/commandExecution/outputDelta"
        | "item/reasoning/summaryTextDelta"
        | "item/mcpToolCall/progress" => {
            let kind = match method {
                "item/agentMessage/delta" => ContentKind::AgentMessage,
                "item/commandExecution/outputDelta" => ContentKind::CommandOutput,
                "item/mcpToolCall/progress" => ContentKind::ToolProgress,
                _ => ContentKind::ReasoningSummary,
            };
            Ok(CodexObservation::Content {
                thread_id: thread(p)?,
                turn_id: turn(p)?,
                item_id: Some(item(p)?),
                kind,
                text: field_text(
                    p,
                    if kind == ContentKind::ToolProgress {
                        "message"
                    } else {
                        "delta"
                    },
                    MAX_CONTENT_BYTES,
                )?
                .to_owned(),
                detail: json!({"summary_index_decimal":p.get("summaryIndex").and_then(Value::as_i64).map(|n|n.to_string())}),
            })
        }
        "turn/plan/updated" => Ok(CodexObservation::Content {
            thread_id: thread(p)?,
            turn_id: turn(p)?,
            item_id: None,
            kind: ContentKind::Plan,
            text: p
                .get("explanation")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            detail: json!({"plan":p["plan"]}),
        }),
        _ => fail("unsupported_normalization"),
    }
}
pub fn normalize_response(
    message: &WireMessage,
    id: &CodexRequestId,
    operation: Operation,
    binding: Option<&SessionBinding>,
) -> Result<CodexObservation> {
    validate_response(message, id, operation)?;
    if message.kind() == WireKind::ErrorResponse {
        return Ok(CodexObservation::Error(safe_error(false)?));
    }
    let p = object(message.result().unwrap())?;
    match operation {
        Operation::ThreadStart | Operation::ThreadResume => session_observation(
            &p["thread"],
            binding,
            p["model"].as_str(),
            p["modelProvider"].as_str(),
        ),
        Operation::TurnStart => {
            let b = binding.ok_or(CodecError {
                code: "session_mapping_required",
            })?;
            turn_observation(b.thread_id.clone(), &p["turn"], binding)
        }
        Operation::ModelList => {
            let mut models = Vec::new();
            for v in array(&p["data"])? {
                let m = object(v)?;
                models.push(CatalogModel {model:model(field_text(m,"model",256)?,Some(field_text(m,"displayName",128)?),None)?,codex:json!({
                    "id":m["id"],"hidden":m["hidden"],"isDefault":m["isDefault"],"defaultReasoningEffort":m["defaultReasoningEffort"],"supportedReasoningEfforts":m["supportedReasoningEfforts"],"inputModalities":m.get("inputModalities").cloned().unwrap_or(json!(["text","image"]))
                })});
            }
            Ok(CodexObservation::ModelCatalog {
                models,
                next_cursor: p
                    .get("nextCursor")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        }
        _ => Ok(CodexObservation::Acknowledged),
    }
}
