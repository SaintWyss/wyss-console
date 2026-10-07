//! Authority-core projection and reconciliation. The core remains the sole durable writer.
use super::{server::CoreMessage, store::Store};
pub(super) use crate::adapters::codex::probe::supervised::{
    ConversationCommand, ConversationFailure, MaterializationStage, Observation, RuntimeCommand,
    TurnCommand,
};
use crate::{
    adapters::codex::probe::supervised::Worker,
    contracts::{
        agent_adapter::{AgentModelDescriptorV1, CompatibilityState},
        control::*,
        identities::AgentRuntimeInstanceId,
    },
};
use serde_json::json;
use std::{
    sync::{
        atomic::Ordering,
        mpsc::{SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};

pub(super) struct Runtime {
    control: Option<RuntimeControlRecord>,
    state: ActualState,
    attempt: Option<AgentRuntimeInstanceId>,
    live: Option<AgentRuntimeInstanceId>,
    worker: Option<Worker>,
    version: Option<String>,
    models: Vec<AgentModelDescriptorV1>,
    diagnostic: Option<RuntimeDiagnostic>,
    methods: RuntimeMethods,
    safety: Option<RuntimeSafetyProfile>,
    mcp_clamp: std::collections::BTreeSet<String>,
    reclamped: bool,
}
impl Runtime {
    pub fn owns_generation(&self, id: &AgentRuntimeInstanceId) -> bool {
        self.attempt.as_ref() == Some(id)
    }
    pub fn new(store: &Store) -> ControlResult<Self> {
        Ok(Self {
            control: store.runtime_control()?,
            state: ActualState::Stopped,
            attempt: None,
            live: None,
            worker: None,
            version: None,
            models: Vec::new(),
            diagnostic: None,
            methods: RuntimeMethods::default(),
            safety: None,
            mcp_clamp: Default::default(),
            reclamped: false,
        })
    }
    fn desired(&self) -> DesiredState {
        self.control
            .as_ref()
            .map_or(DesiredState::Stopped, |r| r.desired_state)
    }
    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
            desired_state: self.desired(),
            revision: self.control.as_ref().map_or(0, |r| r.revision),
            actual_state: self.state,
            runtime_instance_id: self.live.clone(),
            runtime_version: self.version.clone(),
            compatibility: match (self.state, self.diagnostic) {
                (ActualState::Ready, _) => CompatibilityState::Compatible,
                (ActualState::Failed, Some(RuntimeDiagnostic::ProtocolFailed)) => {
                    CompatibilityState::Incompatible
                }
                (
                    ActualState::Failed,
                    Some(
                        RuntimeDiagnostic::CatalogRejected
                        | RuntimeDiagnostic::ServerRequest
                        | RuntimeDiagnostic::ChildExited
                        | RuntimeDiagnostic::AuditUnavailable,
                    ),
                ) => CompatibilityState::Degraded,
                _ => CompatibilityState::Unknown,
            },
            model_count: self.models.len() as u16,
            diagnostic: self.diagnostic,
            outbound_methods: self.methods.clone(),
            runtime_safety_profile: self.safety,
        }
    }
    pub fn models(&self, p: &RuntimeModels) -> ControlResult<ResultValue> {
        RuntimeTarget {
            adapter_id: p.adapter_id.clone(),
            runtime_kind: p.runtime_kind,
        }
        .supported()?;
        if self.state != ActualState::Ready {
            return Err(ControlError::InvalidTransition);
        }
        let id = self.live.as_ref().ok_or(ControlError::InternalError)?;
        let start = if let Some(cursor) = &p.cursor {
            if &cursor.runtime_instance_id != id {
                return Err(ControlError::StaleRevision);
            }
            usize::from(cursor.index)
        } else {
            0
        };
        if start > self.models.len() {
            return Err(ControlError::InvalidRequest);
        }
        let end = (start + usize::from(p.limit)).min(self.models.len());
        Ok(ResultValue::RuntimeModels {
            runtime_instance_id: id.clone(),
            models: self.models[start..end].to_vec(),
            next: (end < self.models.len()).then(|| ModelCursor {
                runtime_instance_id: id.clone(),
                index: end as u16,
            }),
        })
    }
    pub fn reconcile(
        &mut self,
        store: &mut Store,
        core: &SyncSender<CoreMessage>,
        startup: bool,
    ) -> ControlResult<()> {
        let control = store.runtime_control()?;
        let changed =
            control.as_ref().map(|r| r.revision) != self.control.as_ref().map(|r| r.revision);
        self.control = control;
        if changed {
            self.reclamped = false;
            self.mcp_clamp.clear();
        }
        if self.desired() == DesiredState::Stopped {
            self.stop(store, true)?;
        } else if self.worker.is_none() && (startup || changed) {
            self.state = ActualState::Starting;
            self.models.clear();
            self.version = None;
            self.diagnostic = None;
            self.methods = RuntimeMethods::default();
            let id = AgentRuntimeInstanceId::parse(&uuid::Uuid::new_v4().to_string())
                .expect("generated identity");
            self.attempt = Some(id.clone());
            let core = core.clone();
            match Worker::start(
                id,
                self.mcp_clamp.clone(),
                move |id, observation, cancel| {
                    let mut message = CoreMessage::Runtime(id.clone(), observation);
                    // Bounded backpressure, interruptible so core shutdown/stop can always join.
                    loop {
                        if cancel.load(Ordering::Acquire) {
                            break;
                        }
                        match core.try_send(message) {
                            Ok(()) | Err(TrySendError::Disconnected(_)) => break,
                            Err(TrySendError::Full(pending)) => {
                                message = pending;
                                thread::sleep(Duration::from_millis(5));
                            }
                        }
                    }
                },
            ) {
                Ok(worker) => self.worker = Some(worker),
                Err(_) => {
                    self.state = ActualState::Failed;
                    self.diagnostic = Some(RuntimeDiagnostic::SpawnFailed);
                    self.attempt = None;
                }
            }
        }
        Ok(())
    }
    /// One safety-only reconciliation per explicit runtime control revision. Never a
    /// crash-retry loop and never requeues a conversation or prompt command.
    pub fn reclamp(
        &mut self,
        store: &mut Store,
        core: &SyncSender<CoreMessage>,
        names: std::collections::BTreeSet<String>,
    ) -> ControlResult<()> {
        self.contain(store, RuntimeDiagnostic::SafetyUnproven)?;
        if self.reclamped || crate::adapters::codex::safety::process_args(&names).is_err() {
            return Ok(());
        }
        self.reclamped = true;
        self.mcp_clamp = names;
        self.reconcile(store, core, true)
    }
    pub fn command(&mut self, c: ConversationCommand) -> bool {
        self.worker
            .as_mut()
            .is_some_and(|w| w.command(RuntimeCommand::Conversation(c)))
    }
    pub fn turn_command(&mut self, c: RuntimeCommand) -> bool {
        self.worker.as_mut().is_some_and(|w| w.command(c))
    }
    pub fn contain(&mut self, store: &mut Store, reason: RuntimeDiagnostic) -> ControlResult<()> {
        self.fail(store, reason)
    }
    pub fn observe(
        &mut self,
        store: &mut Store,
        id: AgentRuntimeInstanceId,
        event: Observation,
    ) -> ControlResult<()> {
        if self.attempt.as_ref() != Some(&id) {
            return Ok(());
        }
        match event {
            Observation::Materialization { .. }
            | Observation::TurnNotice(_)
            | Observation::IsolationViolation
            | Observation::SafetyConfigChanged(_) => (),
            Observation::Conversation { methods, .. }
            | Observation::TurnResult { methods, .. }
            | Observation::InterruptResult { methods, .. } => {
                self.methods = methods;
            }
            Observation::Discovering => self.state = ActualState::Initializing,
            Observation::Spawned => {
                self.live = Some(id);
                self.state = ActualState::Initializing;
            }
            Observation::Ready {
                version,
                models,
                methods,
            } => {
                if self.live.as_ref() != Some(&id) || self.desired() != DesiredState::Running {
                    let _ = self.audit_failed(store);
                    return Ok(());
                }
                let audit=store.runtime_event("authority.runtime.instance_ready",json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":id,"runtime_version":version,"compatibility":"compatible"}));
                if audit.is_err() {
                    // Never publish readiness before its audit commit. Reap even if exit audit also fails.
                    let _ = self.fail(store, RuntimeDiagnostic::AuditUnavailable);
                    return Ok(());
                }
                self.version = Some(version);
                self.models = models;
                self.methods = methods;
                self.state = ActualState::Ready;
                self.safety = Some(RuntimeSafetyProfile::CodexInferenceV1);
            }
            Observation::Exited { reason, methods } => {
                self.methods = methods;
                self.fail(store, reason)?;
            }
        }
        Ok(())
    }
    pub fn audit_failed(&mut self, store: &mut Store) -> ControlResult<()> {
        self.fail(store, RuntimeDiagnostic::AuditUnavailable)
    }
    fn fail(&mut self, store: &mut Store, reason: RuntimeDiagnostic) -> ControlResult<()> {
        let result = self.stop_reason(store, false, reason);
        self.state = ActualState::Failed;
        self.diagnostic = Some(reason);
        result
    }
    pub fn stop(&mut self, store: &mut Store, expected: bool) -> ControlResult<()> {
        self.stop_reason(store, expected, RuntimeDiagnostic::ChildExited)
    }
    fn stop_reason(
        &mut self,
        store: &mut Store,
        expected: bool,
        reason: RuntimeDiagnostic,
    ) -> ControlResult<()> {
        if let Some(mut worker) = self.worker.take() {
            self.state = ActualState::Stopping;
            if let Some(methods) = worker.stop() {
                self.methods = methods;
            }
        }
        let id = self.live.take();
        self.attempt = None;
        self.safety = None;
        self.models.clear();
        self.version = None;
        self.state = ActualState::Stopped;
        self.diagnostic = None;
        if let Some(id) = id {
            store.runtime_event("authority.runtime.instance_exited",json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":id,"exit_kind":reason,"expected":expected,"desired_state":self.desired()}))?;
        }
        Ok(())
    }
}

pub(super) use crate::adapters::codex::CodexThreadId as ThreadId;
pub(super) fn request_id(s: &str) -> crate::adapters::codex::CodexRequestId {
    crate::adapters::codex::CodexRequestId::String(s.to_owned())
}
