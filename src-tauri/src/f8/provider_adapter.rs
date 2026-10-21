//! Explicit native Codex JSON-RPC boundary. Normalized fixture frames are never accepted as
//! native messages. Provider identifiers are correlated within this one owned connection.
//! Local-tools requires the existing namespace/credential attestor and is refused here until
//! that attestor is supplied; JSON declarations cannot substitute for host containment.
use super::engine::{Attachment, Containment, StopState, Worker};
use crate::{
    adapters::codex::{
        self as codex, ClientInfo, ClientOperation, CodexObservation, CodexRequestId,
        CodexThreadId, CodexTurnId, Direction, Operation, ProtocolState, SessionBinding,
        TurnOutcome, WireKind, WireMessage, safety,
    },
    contracts::{
        control::{ControlError as E, ControlResult, ModelOffer, unique_json},
        identities::{AgentRuntimeInstanceId, ManagedSessionId},
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    time::Instant,
};

#[derive(Clone)]
enum Pending {
    Initialize,
    Config,
    Hooks,
    Requirements,
    Models,
    Binding {
        operation: String,
        method: Operation,
        expected: Option<CodexThreadId>,
    },
    Archive {
        operation: String,
        thread: CodexThreadId,
    },
    Unarchive {
        operation: String,
        thread: CodexThreadId,
    },
    Turn {
        managed: String,
    },
    Interrupt,
}

pub struct NativeCodexWorker<W: Worker> {
    worker: W,
    state: ProtocolState,
    pending: BTreeMap<String, Pending>,
    next_request: u64,
    session: ManagedSessionId,
    instance: AgentRuntimeInstanceId,
    attachment: Attachment,
    catalog: PathBuf,
    version: String,
    binding: Option<SessionBinding>,
    turn: Option<(String, Option<CodexTurnId>)>,
    config_verified: bool,
    hooks_verified: bool,
    ready: bool,
    preflight_pending: bool,
    proof: bool,
    known_mcp: Option<BTreeSet<String>>,
    normalized: VecDeque<Vec<u8>>,
    local: Option<crate::adapters::codex::probe::isolation::F8Attestation>,
    proposals: BTreeMap<String, Value>,
    approvals: BTreeMap<String, NativeApproval>,
    seen_requests: BTreeSet<String>,
    seen_items: BTreeSet<String>,
    #[cfg(test)]
    critical_fault: Option<NativeCriticalFault>,
}
#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum NativeCriticalFault {
    Backpressure,
    PrefixThenFail,
    AfterWriteFail,
    ReapStuck,
}
struct NativeApproval {
    message: WireMessage,
    action: super::operations::ProviderAction,
    fingerprint: String,
    offered: safety::Offered,
    decision: Option<String>,
    consumed: bool,
    ended: bool,
}
fn codec<T>(r: std::result::Result<T, codex::CodecError>) -> ControlResult<T> {
    r.map_err(|_| E::InvalidRequest)
}
impl<W: Worker> NativeCodexWorker<W> {
    /// `version` must come from the launcher's explicit version probe, never provider JSON.
    /// This wrapper performs its own initialize/config/hooks checks before reporting ready.
    pub fn inference(
        worker: W,
        session: ManagedSessionId,
        instance: AgentRuntimeInstanceId,
        attachment: Attachment,
        catalog: PathBuf,
        version: String,
    ) -> ControlResult<Self> {
        Self::inference_retaining(worker, session, instance, attachment, catalog, version)
            .map_err(|(error, _worker)| error)
    }
    /// Preserve the owned worker on validation or initialize-write failure so the
    /// supervisor can contain and reap it through its bounded lifecycle.
    pub fn inference_retaining(
        worker: W,
        session: ManagedSessionId,
        instance: AgentRuntimeInstanceId,
        attachment: Attachment,
        catalog: PathBuf,
        version: String,
    ) -> std::result::Result<Self, (E, W)> {
        Self::retaining(
            worker, session, instance, attachment, catalog, version, None,
        )
    }
    #[allow(dead_code)] // Inactive trusted host entry; installed protocol 1 never selects it.
    pub(crate) fn local_tools_retaining(
        worker: W,
        session: ManagedSessionId,
        instance: AgentRuntimeInstanceId,
        attachment: Attachment,
        catalog: PathBuf,
        version: String,
        proof: crate::adapters::codex::probe::isolation::F8Attestation,
    ) -> std::result::Result<Self, (E, W)> {
        Self::retaining(
            worker,
            session,
            instance,
            attachment,
            catalog,
            version,
            Some(proof),
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn retaining(
        worker: W,
        session: ManagedSessionId,
        instance: AgentRuntimeInstanceId,
        attachment: Attachment,
        catalog: PathBuf,
        version: String,
        local: Option<crate::adapters::codex::probe::isolation::F8Attestation>,
    ) -> std::result::Result<Self, (E, W)> {
        if attachment.profile
            != if local.is_some() {
                "codex_local_tools_v1"
            } else {
                "codex_inference_v1"
            }
            || local.as_ref().is_some_and(|p| {
                p.workspace().to_str() != Some(attachment.workspace.as_str()) || !p.holds()
            })
            || attachment.revision == 0
            || version.is_empty()
            || version.len() > 128
            || !version
                .split('.')
                .all(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
            || !catalog.is_absolute()
        {
            return Err((E::InvalidRequest, worker));
        }
        let mut this = Self {
            worker,
            state: Default::default(),
            pending: BTreeMap::new(),
            next_request: 0,
            session,
            instance,
            attachment,
            catalog,
            version,
            binding: None,
            turn: None,
            config_verified: false,
            hooks_verified: false,
            ready: false,
            preflight_pending: false,
            proof: false,
            known_mcp: None,
            normalized: VecDeque::new(),
            local,
            proposals: BTreeMap::new(),
            approvals: BTreeMap::new(),
            seen_requests: BTreeSet::new(),
            seen_items: BTreeSet::new(),
            #[cfg(test)]
            critical_fault: None,
        };
        let request = ClientOperation::Initialize {
            client_info: ClientInfo {
                name: "wyss-console".into(),
                title: None,
                version: env!("CARGO_PKG_VERSION").into(),
            },
            experimental_api: false,
        };
        if let Err(error) = this.request(request, Pending::Initialize) {
            return Err((error, this.worker));
        }
        Ok(this)
    }
    #[cfg(test)]
    pub(crate) fn fixture_critical_fault(&mut self, fault: Option<NativeCriticalFault>) {
        self.critical_fault = fault;
    }
    fn id(&mut self) -> ControlResult<CodexRequestId> {
        if self.pending.len() >= 16 {
            return Err(E::CapacityExceeded);
        }
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or(E::CapacityExceeded)?;
        Ok(CodexRequestId::String(format!("f8-{}", self.next_request)))
    }
    fn enqueue(&mut self, message: WireMessage, pending: Pending) -> ControlResult<()> {
        codec(self.state.outbound(&message))?;
        let Some(CodexRequestId::String(id)) = message.id() else {
            return Err(E::InvalidRequest);
        };
        let id = id.clone();
        let bytes = codec(message.encode())?;
        self.worker.send(&bytes)?;
        self.pending.insert(id, pending);
        Ok(())
    }
    fn request(&mut self, operation: ClientOperation, pending: Pending) -> ControlResult<()> {
        let id = self.id()?;
        self.enqueue(codec(operation.encode(id))?, pending)
    }
    fn query(&mut self, method: &str, params: Value, pending: Pending) -> ControlResult<()> {
        let id = self.id()?;
        self.enqueue(codec(WireMessage::request(id, method, params))?, pending)
    }
    fn response(&mut self, message: &WireMessage) -> ControlResult<Vec<Value>> {
        let Some(CodexRequestId::String(id)) = message.id() else {
            return Err(E::CorrelationError);
        };
        let pending = self.pending.remove(id).ok_or(E::CorrelationError)?;
        if message.kind() == WireKind::ErrorResponse {
            return Err(E::ProviderRejected);
        }
        let request_id = CodexRequestId::String(id.clone());
        match pending {
            Pending::Initialize => {
                codec(self.state.initialize_response(message))?;
                if let Some(local) = &self.local
                    && message.result().and_then(|r| r["codexHome"].as_str()) != local.home.to_str()
                {
                    return Err(E::GateDenied);
                }
                let initialized = codec(codex::initialized_notification())?;
                codec(self.state.outbound(&initialized))?;
                self.worker.send(&codec(initialized.encode())?)?;
                self.query(
                    "config/read",
                    json!({"cwd":self.attachment.workspace,"includeLayers":false}),
                    Pending::Config,
                )?;
                Ok(Vec::new())
            }
            Pending::Config => {
                codec(codex::validate_response(
                    message,
                    &request_id,
                    Operation::ConfigRead,
                ))?;
                let result = message.result().ok_or(E::InvalidRequest)?;
                if self.local.is_some() {
                    codec(safety::verify_local_tools_config(result, &self.catalog))?;
                } else {
                    codec(safety::verify_config(result, &self.catalog))?;
                }
                let configured = codec(safety::mcp_names(result, false))?;
                if self
                    .known_mcp
                    .as_ref()
                    .is_some_and(|known| !configured.is_subset(known))
                {
                    return Err(E::GateDenied);
                }
                codec(safety::mcp_names(result, true))?;
                if self.known_mcp.is_none() {
                    self.known_mcp = Some(configured);
                }
                self.config_verified = true;
                if self.local.is_some() {
                    self.query("configRequirements/read", json!({}), Pending::Requirements)?;
                    return Ok(Vec::new());
                }
                self.query(
                    "hooks/list",
                    json!({"cwds":[self.attachment.workspace]}),
                    Pending::Hooks,
                )?;
                Ok(Vec::new())
            }
            Pending::Requirements => {
                codec(codex::validate_response(
                    message,
                    &request_id,
                    Operation::ConfigRequirementsRead,
                ))?;
                codec(safety::verify_requirements(
                    message.result().ok_or(E::InvalidRequest)?,
                ))?;
                self.host_proven()?;
                self.query(
                    "hooks/list",
                    json!({"cwds":[self.attachment.workspace]}),
                    Pending::Hooks,
                )?;
                Ok(Vec::new())
            }
            Pending::Hooks => {
                codec(codex::validate_response(
                    message,
                    &request_id,
                    Operation::HooksList,
                ))?;
                codec(safety::zero_external_hooks(
                    message.result().ok_or(E::InvalidRequest)?,
                ))?;
                self.hooks_verified = true;
                if self.preflight_pending {
                    self.preflight_pending = false;
                    self.proof = true;
                } else {
                    self.request(
                        ClientOperation::ModelList {
                            limit: Some(50),
                            cursor: None,
                            include_hidden: false,
                        },
                        Pending::Models,
                    )?;
                }
                Ok(Vec::new())
            }
            Pending::Models => {
                let observation = codec(codex::normalize_response(
                    message,
                    &request_id,
                    Operation::ModelList,
                    self.binding.as_ref(),
                ))?;
                let CodexObservation::ModelCatalog {
                    models,
                    next_cursor,
                } = observation
                else {
                    return Err(E::InvalidRequest);
                };
                if models.len() > 50 {
                    return Err(E::CapacityExceeded);
                }
                let offers: Vec<ModelOffer> = models
                    .iter()
                    .map(|m| ModelOffer {
                        is_default: m.codex["isDefault"].as_bool().unwrap_or(false),
                        default_effort: m.codex["defaultReasoningEffort"]
                            .as_str()
                            .map(str::to_owned),
                        efforts: m.codex["supportedReasoningEfforts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|v| v["reasoningEffort"].as_str().map(str::to_owned))
                            .collect(),
                        replaced_by: m.codex["replacedBy"].as_str().map(str::to_owned),
                        retires_at: m.codex["retirementAt"].as_str().map(str::to_owned),
                    })
                    .collect();
                let models: Vec<_> = models.into_iter().map(|m| m.model).collect();
                // Match the engine's snapshot admission before publishing readiness:
                // a one-frame quantum must never expose Ready for a catalogue that
                // the following quantum would reject.
                if models.iter().any(|model| model.validate().is_err())
                    || offers.iter().any(|offer| !offer.valid())
                {
                    return Err(E::InvalidRequest);
                }
                let mut notices = Vec::new();
                if !self.ready {
                    if !self.config_verified || !self.hooks_verified {
                        return Err(E::GateDenied);
                    }
                    self.ready = true;
                    notices.push(
                        json!({"kind":"runtime_ready","observed_version":self.version,
                        "workspace":self.attachment.workspace,"profile":self.attachment.profile,
                        "revision":self.attachment.revision}),
                    );
                }
                // The engine admits model snapshots only after this launch's
                // durable readiness edge. Both still require validated discovery.
                notices.push(json!({"kind":"models","models":models,"offers":offers,"complete":next_cursor.is_none()}));
                Ok(notices)
            }
            Pending::Binding {
                operation,
                method,
                expected,
            } => {
                codec(codex::validate_response(message, &request_id, method))?;
                let result = message.result().ok_or(E::InvalidRequest)?;
                if self.local.is_some() {
                    codec(safety::verify_local_tools_thread(result))?;
                    if result["thread"]["cliVersion"].as_str() != Some(self.version.as_str()) {
                        return Err(E::GateDenied);
                    }
                } else {
                    codec(safety::verify_resume(result))?;
                }
                if result["cwd"].as_str() != Some(self.attachment.workspace.as_str())
                    || result["thread"]["ephemeral"] != false
                {
                    return Err(E::GateDenied);
                }
                let thread = codec(CodexThreadId::parse(
                    result["thread"]["id"].as_str().ok_or(E::InvalidRequest)?,
                ))?;
                if expected.as_ref().is_some_and(|id| id != &thread) {
                    return Err(E::CorrelationError);
                }
                if method == Operation::ThreadStart {
                    let id = self.id()?;
                    self.enqueue(
                        codec(codex::materialization_request(id, &thread, false))?,
                        Pending::Archive { operation, thread },
                    )?;
                    Ok(Vec::new())
                } else {
                    self.binding_confirmed(operation, thread)
                }
            }
            Pending::Archive { operation, thread } => {
                codec(codex::validate_response(
                    message,
                    &request_id,
                    Operation::ThreadArchive,
                ))?;
                let id = self.id()?;
                self.enqueue(
                    codec(codex::materialization_request(id, &thread, true))?,
                    Pending::Unarchive { operation, thread },
                )?;
                Ok(Vec::new())
            }
            Pending::Unarchive { operation, thread } => {
                codec(codex::validate_response(
                    message,
                    &request_id,
                    Operation::ThreadUnarchive,
                ))?;
                let result = message.result().ok_or(E::InvalidRequest)?;
                if result["thread"]["id"].as_str() != Some(thread.as_str())
                    || result["thread"]["cwd"].as_str() != Some(self.attachment.workspace.as_str())
                    || result["thread"]["ephemeral"] != false
                {
                    return Err(E::GateDenied);
                }
                self.binding_confirmed(operation, thread)
            }
            Pending::Turn { managed } => {
                // A turn/start result can carry the same item list as a lifecycle
                // notification. Do not let normalization erase executable surfaces
                // that the inference profile would reject in a notification.
                if safety::external_action_candidate(
                    "turn/started",
                    message.result().ok_or(E::InvalidRequest)?,
                ) {
                    return Err(E::GateDenied);
                }
                let observation = codec(codex::normalize_response(
                    message,
                    &request_id,
                    Operation::TurnStart,
                    self.binding.as_ref(),
                ))?;
                self.translate(observation, Some(&managed))
            }
            Pending::Interrupt => {
                codec(codex::validate_response(
                    message,
                    &request_id,
                    Operation::TurnInterrupt,
                ))?;
                Ok(Vec::new()) // RPC acknowledgement never proves cancellation completed.
            }
        }
    }
    fn binding_confirmed(
        &mut self,
        operation: String,
        thread: CodexThreadId,
    ) -> ControlResult<Vec<Value>> {
        self.binding = Some(SessionBinding {
            managed_session_id: self.session.clone(),
            runtime_instance_id: self.instance.clone(),
            thread_id: thread.clone(),
        });
        Ok(vec![
            json!({"kind":"binding_confirmed","operation_id":operation,"provider_thread_id":thread.as_str()}),
        ])
    }
    fn translate(
        &mut self,
        observation: CodexObservation,
        expected: Option<&str>,
    ) -> ControlResult<Vec<Value>> {
        match observation {
            CodexObservation::Turn {
                turn_id, outcome, ..
            } => {
                let (managed, provider) = self.turn.as_mut().ok_or(E::CorrelationError)?;
                if expected.is_some_and(|e| e != managed)
                    || provider.as_ref().is_some_and(|p| p != &turn_id)
                {
                    return Err(E::CorrelationError);
                }
                // Both turn/start's response and turn/started report the same
                // lifecycle edge. Publish it once; the durable writer correctly
                // refuses a second Running -> Running transition.
                if outcome == TurnOutcome::InProgress && provider.as_ref() == Some(&turn_id) {
                    return Ok(Vec::new());
                }
                *provider = Some(turn_id.clone());
                let notice = match outcome {
                    TurnOutcome::InProgress => {
                        json!({"kind":"turn_running","managed_turn_id":managed,"provider_turn_id":turn_id.as_str()})
                    }
                    TurnOutcome::Completed => {
                        json!({"kind":"turn_completed","managed_turn_id":managed,"provider_turn_id":turn_id.as_str()})
                    }
                    TurnOutcome::Failed => json!({"kind":"turn_failed","managed_turn_id":managed}),
                    TurnOutcome::Interrupted => {
                        json!({"kind":"turn_interrupted","managed_turn_id":managed})
                    }
                };
                if outcome != TurnOutcome::InProgress {
                    self.turn = None;
                    self.proposals.clear();
                    self.approvals.clear();
                    self.seen_items.clear();
                    self.proof = false;
                }
                Ok(vec![notice])
            }
            CodexObservation::Content { turn_id, text, .. } => {
                let (managed, provider) = self.turn.as_ref().ok_or(E::CorrelationError)?;
                if provider.as_ref() != Some(&turn_id) {
                    return Err(E::CorrelationError);
                }
                Ok(vec![
                    json!({"kind":"content","managed_turn_id":managed,"provider_turn_id":turn_id.as_str(),"text":text}),
                ])
            }
            CodexObservation::Item(_) | CodexObservation::SuppressedPrivateReasoning => {
                Ok(Vec::new())
            }
            // The inference classifier has already accepted these metadata-only
            // notifications. The normalizer also checks any stable thread identity.
            CodexObservation::UnknownNotification { .. } => Ok(Vec::new()),
            CodexObservation::Session { .. }
                if self.pending.values().any(|p| {
                    matches!(
                        p,
                        Pending::Binding { .. }
                            | Pending::Archive { .. }
                            | Pending::Unarchive { .. }
                    )
                }) =>
            {
                Ok(Vec::new())
            }
            _ => Err(E::InvalidRequest),
        }
    }
    fn outbound(&mut self, frame: &[u8]) -> ControlResult<()> {
        if !self.ready {
            return Err(E::InvalidTransition);
        }
        let value = unique_json(frame)?;
        match value["method"].as_str() {
            Some("turn/start") => {
                if !self.proof {
                    return Err(E::GateDenied);
                }
                self.proof = false;
                if self.turn.is_some() {
                    return Err(E::ActiveTurnExists);
                }
                let managed = value["turn"].as_str().ok_or(E::InvalidRequest)?.to_owned();
                let text = value["input"].as_str().ok_or(E::InvalidRequest)?.to_owned();
                let binding = self.binding.as_ref().ok_or(E::BindingNotFound)?;
                let operation = ClientOperation::TurnStart {
                    thread_id: binding.thread_id.clone(),
                    text,
                };
                self.request(
                    operation,
                    Pending::Turn {
                        managed: managed.clone(),
                    },
                )?;
                self.turn = Some((managed, None));
                Ok(())
            }
            Some("turn/interrupt") => {
                let binding = self.binding.as_ref().ok_or(E::BindingNotFound)?;
                let turn_id = self
                    .turn
                    .as_ref()
                    .and_then(|(_, p)| p.clone())
                    .ok_or(E::InvalidTransition)?;
                self.request(
                    ClientOperation::TurnInterrupt {
                        thread_id: binding.thread_id.clone(),
                        turn_id,
                    },
                    Pending::Interrupt,
                )
            }
            Some("thread/start" | "thread/resume") => {
                if !self.proof {
                    return Err(E::GateDenied);
                }
                self.proof = false;
                if self.turn.is_some() {
                    return Err(E::ActiveTurnExists);
                }
                let operation = value["operation_id"]
                    .as_str()
                    .ok_or(E::InvalidRequest)?
                    .to_owned();
                if value["workspace"].as_str() != Some(self.attachment.workspace.as_str()) {
                    return Err(E::GateDenied);
                }
                let resume = value["method"] == "thread/resume";
                let method = if resume {
                    Operation::ThreadResume
                } else {
                    Operation::ThreadStart
                };
                let mut params = json!({"cwd":self.attachment.workspace,"approvalPolicy":"never",
                    "sandbox":"read-only","config":codec(safety::thread_config(self.known_mcp.as_ref().ok_or(E::GateDenied)?))?});
                if let Some(local) = &self.local {
                    params["approvalPolicy"] = json!("untrusted");
                    params["sandbox"] = json!("workspace-write");
                    params["config"] = codec(safety::local_tools_thread_config(
                        self.known_mcp.as_ref().ok_or(E::GateDenied)?,
                        &local.roots,
                    ))?;
                }
                let expected = if resume {
                    Some(codec(CodexThreadId::parse(
                        value["thread_id"].as_str().ok_or(E::InvalidRequest)?,
                    ))?)
                } else {
                    None
                };
                if let Some(thread) = &expected {
                    params["threadId"] = json!(thread);
                    params["excludeTurns"] = json!(true);
                } else {
                    params["ephemeral"] = json!(false);
                }
                self.query(
                    if resume {
                        "thread/resume"
                    } else {
                        "thread/start"
                    },
                    params,
                    Pending::Binding {
                        operation,
                        method,
                        expected,
                    },
                )
            }
            Some("model/list") => self.request(
                ClientOperation::ModelList {
                    limit: Some(50),
                    cursor: None,
                    include_hidden: false,
                },
                Pending::Models,
            ),
            _ => Err(E::Unsupported),
        }
    }
}
impl<W: Worker> Worker for NativeCodexWorker<W> {
    fn preflight(&mut self, context: &Attachment) -> ControlResult<()> {
        if context != &self.attachment || !self.ready {
            return Err(E::GateDenied);
        }
        self.host_proven()?;
        if self.proof {
            return Ok(());
        }
        if !self.preflight_pending {
            self.preflight_pending = true;
            self.query(
                "config/read",
                json!({"cwd":self.attachment.workspace,"includeLayers":false}),
                Pending::Config,
            )?;
        }
        Err(E::RuntimeOperationInProgress)
    }
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.outbound(frame)
    }
    fn send_critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.critical(frame)
    }
    fn flush(&mut self) -> ControlResult<()> {
        self.worker.flush()
    }
    fn inbound(&mut self, max_frames: usize) -> ControlResult<Vec<Vec<u8>>> {
        if max_frames == 0 {
            return Ok(Vec::new());
        }
        if !self.normalized.is_empty() {
            return Ok(self
                .normalized
                .drain(..max_frames.min(self.normalized.len()))
                .collect());
        }
        let frames = self.worker.inbound(max_frames.min(16))?;
        for frame in frames {
            let message = codec(WireMessage::parse(&frame, Direction::ServerToClient))?;
            let events = if matches!(
                message.kind(),
                WireKind::SuccessResponse | WireKind::ErrorResponse
            ) {
                self.response(&message)?
            } else {
                // Existing inference isolation classifier rejects requests, tools, hooks,
                // MCP and unknown executable surfaces before the normalizer sees them.
                let local_notice = if self.local.is_some() {
                    codec(safety::local_notification(&message))?
                } else {
                    codec(safety::notification(&message))?
                };
                if !self.ready {
                    // The pinned fixture emits connection metadata before initialize's
                    // response. It cannot attest readiness, binding or executable work.
                    if matches!(
                        message.method(),
                        Some(
                            "remoteControl/status/changed"
                                | "account/updated"
                                | "account/rateLimits/updated"
                        )
                    ) {
                        continue;
                    }
                    return Err(E::InvalidTransition);
                }
                // Some validated metadata methods are deliberately not normalized into
                // stable observations. They still belong to this connection's thread,
                // including the materialization interval before binding is published.
                let expected_thread = self.binding.as_ref().map(|b| &b.thread_id).or_else(|| {
                    self.pending.values().find_map(|p| match p {
                        Pending::Archive { thread, .. } | Pending::Unarchive { thread, .. } => {
                            Some(thread)
                        }
                        Pending::Binding {
                            expected: Some(thread),
                            ..
                        } => Some(thread),
                        _ => None,
                    })
                });
                if let Some(expected) = expected_thread
                    && let Some(thread) = message.params().and_then(|p| p.get("threadId"))
                    && thread.as_str() != Some(expected.as_str())
                {
                    return Err(E::CorrelationError);
                }
                if self.local.is_some()
                    && matches!(message.method(), Some("turn/started" | "turn/completed"))
                {
                    self.terminal_items(&message)?;
                }
                if self.local.is_some()
                    && (message.kind() == WireKind::ServerRequest
                        || matches!(
                            local_notice.as_ref().map(|n| &n.kind),
                            Some(
                                safety::NoticeKind::CommandProposed(_)
                                    | safety::NoticeKind::FileProposed(_)
                                    | safety::NoticeKind::ApprovalRequested(_)
                                    | safety::NoticeKind::Output { .. }
                                    | safety::NoticeKind::ItemEnded { .. }
                                    | safety::NoticeKind::DiffUpdated
                            )
                        )
                        || message.method() == Some("serverRequest/resolved"))
                {
                    self.mediate(&message, local_notice)?
                } else {
                    let observation =
                        codec(codex::normalize_inbound(&message, self.binding.as_ref()))?;
                    self.translate(observation, None)?
                }
            };
            for value in events {
                // One native response can expand into models plus runtime-ready.
                // Retain the bounded remainder rather than faulting a healthy worker
                // when the scheduler supplies a one-frame quantum.
                if self.normalized.len() >= 32 {
                    return Err(E::CapacityExceeded);
                }
                self.normalized
                    .push_back(serde_json::to_vec(&value).map_err(|_| E::InternalError)?);
            }
        }
        Ok(self
            .normalized
            .drain(..max_frames.min(self.normalized.len()))
            .collect())
    }
    fn has_exited(&mut self) -> bool {
        self.worker.has_exited()
    }
    fn begin_stop(&mut self, now: Instant) {
        if self.local.is_some() {
            self.proof = false;
            self.ready = false;
            self.approvals.clear();
            self.proposals.clear();
        }
        self.worker.begin_stop(now);
    }
    fn poll_stop(&mut self, now: Instant) -> StopState {
        #[cfg(test)]
        if matches!(self.critical_fault, Some(NativeCriticalFault::ReapStuck)) {
            let _ = self.worker.poll_stop(now);
            return StopState::Stuck;
        }
        self.worker.poll_stop(now)
    }
    fn descendants(&self) -> usize {
        self.worker.descendants()
    }
    fn containment(&mut self) -> Containment {
        self.worker.containment()
    }
    fn pid_for_diagnostics(&self) -> Option<i32> {
        self.worker.pid_for_diagnostics()
    }
}

#[path = "native_approvals.rs"]
mod native_approvals;

#[cfg(test)]
#[path = "native_process_tests.rs"]
pub(crate) mod native_process_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };
    #[derive(Default)]
    struct Fixture {
        sent: Arc<Mutex<Vec<Vec<u8>>>>,
        inbound: VecDeque<Vec<u8>>,
        reject_send: bool,
    }
    impl Worker for Fixture {
        fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
            if self.reject_send {
                return Err(E::TransportError);
            }
            self.sent.lock().unwrap().push(frame.to_vec());
            Ok(())
        }
        fn inbound(&mut self, maximum: usize) -> ControlResult<Vec<Vec<u8>>> {
            Ok((0..maximum)
                .filter_map(|_| self.inbound.pop_front())
                .collect())
        }
    }
    fn worker() -> NativeCodexWorker<Fixture> {
        NativeCodexWorker::inference(
            Fixture::default(),
            ManagedSessionId::parse("00000000-0000-4000-8000-000000000001").unwrap(),
            AgentRuntimeInstanceId::parse("00000000-0000-4000-8000-000000000002").unwrap(),
            Attachment {
                workspace: "/work".into(),
                profile: "codex_inference_v1".into(),
                revision: 1,
            },
            "/private/catalog.json".into(),
            "0.159.0".into(),
        )
        .unwrap()
    }
    #[test]
    fn native_boundary_refuses_normalized_fixture_and_uncorrelated_rpc() {
        let mut w = worker();
        w.worker.inbound.push_back(br#"{"kind":"runtime_ready","observed_version":"0.159.0","workspace":"/work","profile":"codex_inference_v1","revision":1}"#.to_vec());
        assert!(w.inbound(16).is_err());
        let mut w = worker();
        w.worker
            .inbound
            .push_back(br#"{"id":"foreign","result":{}}"#.to_vec());
        assert!(w.inbound(16).is_err());
        assert!(!w.ready);
    }
    #[test]
    fn initialization_alone_cannot_publish_readiness() {
        let mut w = worker();
        w.worker.inbound.push_back(br#"{"id":"f8-1","result":{"userAgent":"fixture","codexHome":"/private","platformFamily":"unix","platformOs":"linux"}}"#.to_vec());
        assert!(w.inbound(16).unwrap().is_empty());
        assert!(!w.ready);
        let sent = w.worker.sent.lock().unwrap();
        let config: Value = serde_json::from_slice(sent.last().unwrap()).unwrap();
        assert_eq!(config["method"], "config/read");
        drop(sent);
        w.worker
            .inbound
            .push_back(br#"{"id":"f8-2","result":{"config":{}}}"#.to_vec());
        assert!(w.inbound(16).is_err());
        assert!(!w.ready);
    }
    fn bound() -> NativeCodexWorker<Fixture> {
        let mut w = worker();
        w.binding = Some(SessionBinding {
            managed_session_id: w.session.clone(),
            runtime_instance_id: w.instance.clone(),
            thread_id: CodexThreadId::parse("thread-A").unwrap(),
        });
        w.ready = true;
        w.turn = Some((
            "00000000-0000-4000-8000-000000000003".into(),
            Some(CodexTurnId::parse("turn-A").unwrap()),
        ));
        w
    }
    #[test]
    fn native_cross_thread_and_cross_turn_messages_are_refused() {
        for (thread, turn) in [("thread-B", "turn-A"), ("thread-A", "turn-B")] {
            let mut w = bound();
            w.worker.inbound.push_back(
                serde_json::to_vec(&json!({"method":"item/agentMessage/delta", "params":{
                "threadId":thread,"turnId":turn,"itemId":"item-A","delta":"foreign"}}))
                .unwrap(),
            );
            assert!(w.inbound(16).is_err());
        }
    }
    #[test]
    fn native_content_translates_with_authority_managed_turn() {
        let mut w = bound();
        w.worker.inbound.push_back(br#"{"method":"item/agentMessage/delta","params":{"threadId":"thread-A","turnId":"turn-A","itemId":"item-A","delta":"hello"}}"#.to_vec());
        let events = w.inbound(16).unwrap();
        let event: Value = serde_json::from_slice(&events[0]).unwrap();
        assert_eq!(event["kind"], "content");
        assert_eq!(
            event["managed_turn_id"],
            "00000000-0000-4000-8000-000000000003"
        );
        assert_eq!(event["text"], "hello");
    }
    #[test]
    fn inference_rejects_native_tool_or_approval_surfaces() {
        let mut w = bound();
        w.worker.inbound.push_back(br#"{"method":"item/commandExecution/requestApproval","id":8,"params":{"threadId":"thread-A","turnId":"turn-A","itemId":"item-A"}}"#.to_vec());
        assert!(w.inbound(16).is_err());
        assert_eq!(
            w.send_critical(b"approval item-A accept"),
            Err(E::Unsupported)
        );
    }
    #[test]
    fn native_materialization_metadata_is_consumed_and_thread_fenced() {
        for method in ["thread/archived", "thread/unarchived"] {
            let mut w = bound();
            w.worker.inbound.push_back(
                serde_json::to_vec(&json!({
                    "method":method,"params":{"threadId":"thread-A"}
                }))
                .unwrap(),
            );
            assert!(w.inbound(1).unwrap().is_empty());
            w.worker.inbound.push_back(
                serde_json::to_vec(&json!({
                    "method":method,"params":{"threadId":"thread-B"}
                }))
                .unwrap(),
            );
            assert!(w.inbound(1).is_err());
        }
        let mut w = bound();
        w.binding = None;
        w.pending.insert(
            "archive".into(),
            Pending::Archive {
                operation: "operation".into(),
                thread: CodexThreadId::parse("thread-A").unwrap(),
            },
        );
        w.worker.inbound.push_back(
            br#"{"method":"thread/archived","params":{"threadId":"thread-B"}}"#.to_vec(),
        );
        assert_eq!(w.inbound(1), Err(E::CorrelationError));
    }
    #[test]
    fn native_metadata_consumption_does_not_allow_unknown_methods() {
        let mut w = bound();
        w.worker
            .inbound
            .push_back(br#"{"method":"unknown/effect","params":{"threadId":"thread-A"}}"#.to_vec());
        assert!(w.inbound(1).is_err());
    }
    #[test]
    fn native_readiness_expansion_respects_one_frame_budget_without_loss() {
        let mut w = worker();
        w.config_verified = true;
        w.hooks_verified = true;
        w.pending.clear();
        w.pending.insert("models".into(), Pending::Models);
        w.worker
            .inbound
            .push_back(br#"{"id":"models","result":{"data":[],"nextCursor":null}}"#.to_vec());
        let first = w.inbound(1).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&first[0]).unwrap()["kind"],
            "runtime_ready"
        );
        let second = w.inbound(1).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&second[0]).unwrap()["kind"],
            "models"
        );
        assert!(w.inbound(1).unwrap().is_empty());
    }
    #[test]
    fn native_invalid_model_offers_never_publish_readiness() {
        for excessive_efforts in [true, false] {
            let mut w = worker();
            w.config_verified = true;
            w.hooks_verified = true;
            w.pending.clear();
            w.pending.insert("models".into(), Pending::Models);
            let efforts: Vec<_> = (0..if excessive_efforts { 17 } else { 1 })
                .map(|index| json!({"reasoningEffort":format!("effort{index}"),"description":"Fixture"}))
                .collect();
            w.worker.inbound.push_back(serde_json::to_vec(&json!({
                "id":"models","result":{"data":[{
                    "id":"fixture","model":"fixture","displayName":"Fixture",
                    "description":"Fixture","hidden":false,"isDefault":true,
                    "defaultReasoningEffort":if excessive_efforts { "effort0" } else { "invalid effort" },
                    "supportedReasoningEfforts":efforts
                }],"nextCursor":null}
            })).unwrap());
            assert_eq!(w.inbound(1), Err(E::InvalidRequest));
            assert!(!w.ready);
            assert!(w.normalized.is_empty());
            assert!(w.inbound(1).unwrap().is_empty());
        }
    }
    #[test]
    fn native_turn_response_cannot_hide_an_executed_action() {
        let mut w = bound();
        w.pending.insert(
            "turn".into(),
            Pending::Turn {
                managed: "00000000-0000-4000-8000-000000000003".into(),
            },
        );
        w.worker.inbound.push_back(
            serde_json::to_vec(&json!({
                "id":"turn","result":{"turn":{"id":"turn-A","status":"completed",
                "items":[{"id":"action","type":"commandExecution","command":"true",
                "cwd":"/work","commandActions":[],"status":"completed","exitCode":0}]}}
            }))
            .unwrap(),
        );
        assert_eq!(w.inbound(1), Err(E::GateDenied));
        assert!(w.turn.is_some());
    }
    #[test]
    fn native_turn_start_response_and_notification_publish_one_running_edge() {
        for response_first in [true, false] {
            let mut w = bound();
            w.turn.as_mut().unwrap().1 = None;
            w.pending.insert(
                "start".into(),
                Pending::Turn {
                    managed: "00000000-0000-4000-8000-000000000003".into(),
                },
            );
            let turn = json!({"id":"turn-A","status":"inProgress","items":[]});
            let response =
                serde_json::to_vec(&json!({"id":"start","result":{"turn":turn}})).unwrap();
            let notice = serde_json::to_vec(
                &json!({"method":"turn/started","params":{"threadId":"thread-A","turn":turn}}),
            )
            .unwrap();
            if response_first {
                w.worker.inbound.extend([response, notice]);
            } else {
                w.worker.inbound.extend([notice, response]);
            }
            let frames = w.inbound(16).unwrap();
            assert_eq!(frames.len(), 1);
            assert_eq!(
                serde_json::from_slice::<Value>(&frames[0]).unwrap()["kind"],
                "turn_running"
            );
        }
    }
    #[test]
    fn retaining_constructor_returns_owned_worker_for_every_failure() {
        for invalid_profile in [true, false] {
            let w = worker();
            let mut fixture = w.worker;
            let identity = Arc::clone(&fixture.sent);
            fixture.reject_send = !invalid_profile;
            let mut attachment = w.attachment;
            if invalid_profile {
                attachment.profile = "unsupported".into();
            }
            match NativeCodexWorker::inference_retaining(
                fixture, w.session, w.instance, attachment, w.catalog, w.version,
            ) {
                Ok(_) => panic!("expected refusal"),
                Err((error, owned)) => {
                    assert_eq!(
                        error,
                        if invalid_profile {
                            E::InvalidRequest
                        } else {
                            E::TransportError
                        }
                    );
                    assert!(Arc::ptr_eq(&identity, &owned.sent));
                }
            }
        }
    }
}
