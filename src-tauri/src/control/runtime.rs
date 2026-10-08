//! Authority-core projection and reconciliation. The core remains the sole durable writer.
use super::{server::CoreMessage, store::Store};
pub(super) use crate::adapters::codex::probe::supervised::{
    ConversationCommand, ConversationFailure, MaterializationStage, Observation, ProviderDecision,
    RuntimeCommand, TurnCommand,
};
use crate::{
    adapters::codex::probe::{
        isolation::Isolation,
        supervised::{LaunchProfile, Worker},
    },
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
    /// Private authority directory that receives the normalized model catalog.
    catalog_dir: std::path::PathBuf,
    /// Private authority directory of the isolated generation's home and policy.
    isolation_dir: std::path::PathBuf,
    /// Workspace of the local-tools profile requested by the latest explicit start. Memory
    /// only: after an authority restart a desired-running runtime comes back as inference.
    requested: Option<String>,
    /// Workspace the current generation was launched for, if it is a local-tools one.
    generation: Option<String>,
}
impl Runtime {
    pub fn owns_generation(&self, id: &AgentRuntimeInstanceId) -> bool {
        self.attempt.as_ref() == Some(id)
    }
    pub fn new(
        store: &Store,
        catalog_dir: &std::path::Path,
        isolation_dir: &std::path::Path,
    ) -> ControlResult<Self> {
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
            catalog_dir: catalog_dir.to_path_buf(),
            isolation_dir: isolation_dir.to_path_buf(),
            requested: None,
            generation: None,
        })
    }
    /// Admission of an explicit start, before anything durable changes. Returns the
    /// canonical workspace of a local-tools request. A generation never changes profile:
    /// while one exists, only a request for that same profile and workspace is accepted.
    pub fn admit_start(&self, p: &RuntimeMutation) -> ControlResult<Option<String>> {
        p.validate(true)?;
        let workspace = p
            .workspace_root
            .as_deref()
            .map(super::store::bindings::canonical_workspace)
            .transpose()?;
        if self.worker.is_some() && workspace != self.generation {
            return Err(ControlError::InvalidTransition);
        }
        Ok(workspace)
    }
    /// Records the profile of a freshly committed start. A replayed intent returns an
    /// older control revision and changes nothing.
    pub fn request(&mut self, workspace: Option<String>, committed: &RuntimeControlRecord) {
        if committed.revision > self.control.as_ref().map_or(0, |r| r.revision) {
            self.requested = workspace;
        }
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
            workspace_root: self
                .safety
                .filter(|p| *p == RuntimeSafetyProfile::CodexLocalToolsV1)
                .and(self.generation.clone()),
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
            self.generation = self.requested.clone();
            let profile = match &self.generation {
                None => LaunchProfile::Inference,
                Some(workspace) => LaunchProfile::LocalTools(Isolation {
                    workspace: workspace.into(),
                    state: self.isolation_dir.clone(),
                }),
            };
            let core = core.clone();
            match Worker::start(
                id,
                self.mcp_clamp.clone(),
                self.catalog_dir.clone(),
                profile,
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
        if self.reclamped || crate::adapters::codex::safety::process_args(&names, None).is_err() {
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
            | Observation::Attested { .. }
            | Observation::SafetyConfigChanged(_) => (),
            Observation::Conversation { methods, .. }
            | Observation::TurnResult { methods, .. }
            | Observation::Delivered { methods, .. }
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
                // Readiness is reported only after the attestation of this exact profile.
                self.safety = Some(if self.generation.is_some() {
                    RuntimeSafetyProfile::CodexLocalToolsV1
                } else {
                    RuntimeSafetyProfile::CodexInferenceV1
                });
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
        self.generation = None;
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

#[cfg(test)]
impl Runtime {
    /// A ready generation without a process: the test reads every command the core
    /// dispatches. `workspace` selects the local-tools profile.
    pub(super) fn ready_for_test(
        store: &mut Store,
        instance: &AgentRuntimeInstanceId,
        workspace: Option<&str>,
    ) -> (
        Self,
        std::sync::mpsc::Receiver<RuntimeCommand>,
        std::os::unix::net::UnixStream,
    ) {
        let target = RuntimeTarget::codex();
        store
            .apply(&Operation::RuntimeStart(RuntimeMutation {
                intent_id: super::store::uuid(),
                adapter_id: target.adapter_id,
                runtime_kind: target.runtime_kind,
                expected_revision: None,
                safety_profile: workspace.map(|_| RuntimeSafetyProfile::CodexLocalToolsV1),
                workspace_root: workspace.map(str::to_owned),
            }))
            .expect("desired running");
        let directory = std::env::temp_dir();
        let mut runtime = Self::new(store, &directory, &directory).expect("runtime");
        let (worker, commands, wake) = Worker::detached();
        runtime.worker = Some(worker);
        runtime.state = ActualState::Ready;
        runtime.attempt = Some(instance.clone());
        runtime.live = Some(instance.clone());
        runtime.generation = workspace.map(str::to_owned);
        runtime.set_profile_for_test(Some(if workspace.is_some() {
            RuntimeSafetyProfile::CodexLocalToolsV1
        } else {
            RuntimeSafetyProfile::CodexInferenceV1
        }));
        (runtime, commands, wake)
    }
    pub(super) fn set_profile_for_test(&mut self, profile: Option<RuntimeSafetyProfile>) {
        self.safety = profile;
    }
    pub(super) fn set_instance_for_test(&mut self, instance: &AgentRuntimeInstanceId) {
        self.attempt = Some(instance.clone());
        self.live = Some(instance.clone());
    }
}

pub(super) use crate::adapters::codex::CodexThreadId as ThreadId;
pub(super) fn request_id(s: &str) -> crate::adapters::codex::CodexRequestId {
    crate::adapters::codex::CodexRequestId::String(s.to_owned())
}
