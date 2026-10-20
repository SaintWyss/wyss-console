//! Core-owned admission, reply correlation and transient attachment. No provider IO here.
use super::{
    gate::{ExecutionDecision, ExecutionGate},
    runtime::{ConversationCommand, ConversationFailure, Runtime},
    store::{
        Store,
        bindings::{Reservation, canonical_workspace},
    },
};
use crate::contracts::{
    binding::ObservedModel,
    control::*,
    identities::{AgentRuntimeInstanceId, ManagedSessionId, ManagedTurnId},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
};
const MAX_ATTACHMENTS: usize = 128;
struct Pending {
    reservation: Reservation,
    instance: AgentRuntimeInstanceId,
    reply: SyncSender<ControlResult<ResultValue>>,
    admission: Arc<AtomicBool>,
}
#[derive(Default)]
pub(super) struct Conversations {
    pending: Option<Pending>,
    attachments: BTreeMap<String, Attached>,
}
/// A session attached to a generation, and the model that generation reported for it. Both
/// are transient: they are gone with the attachment (ADR 0031, option A).
struct Attached {
    instance: AgentRuntimeInstanceId,
    model: Option<ObservedModel>,
}
#[cfg(test)]
impl Conversations {
    /// Marks a session attached to a generation, as a completed resume would.
    pub(super) fn attach_for_test(
        &mut self,
        session: &ManagedSessionId,
        id: &AgentRuntimeInstanceId,
    ) {
        self.attachments.insert(
            session.as_str().to_ascii_lowercase(),
            Attached {
                instance: id.clone(),
                model: None,
            },
        );
    }
    #[allow(dead_code)]
    pub(super) fn attach_with_model_for_test(
        &mut self,
        session: &ManagedSessionId,
        id: &AgentRuntimeInstanceId,
        model: ObservedModel,
    ) {
        self.attachments.insert(
            session.as_str().to_ascii_lowercase(),
            Attached {
                instance: id.clone(),
                model: Some(model),
            },
        );
    }
    pub(super) fn detach_for_test(&mut self) {
        self.attachments.clear();
    }
}
impl Conversations {
    /// Records a reroute the turn controller accepted: the model in use for this session is now
    /// the one the notice names, marked as reported by a reroute and from which model. Only an
    /// attachment of this very generation is touched. What the thread's own answer reported
    /// (provider, effort) is kept; when it reported no model, the notice's source model stands
    /// in as the base, so nothing is taken from a default or a catalogue.
    pub fn rerouted(
        &mut self,
        instance: &AgentRuntimeInstanceId,
        session: &ManagedSessionId,
        turn: ManagedTurnId,
        reroute: &crate::adapters::codex::safety::Reroute,
    ) {
        let Some(attached) = self
            .attachments
            .get_mut(&session.as_str().to_ascii_lowercase())
            .filter(|a| a.instance == *instance)
        else {
            return;
        };
        let base = attached.model.clone().unwrap_or(ObservedModel {
            model_id: reroute.from.clone(),
            provider_id: None,
            reasoning_effort: None,
            source: crate::contracts::binding::ObservedModelSource::ThreadStart,
            rerouted_from: None,
            reroute_reason: None,
            rerouted_turn: None,
        });
        if let Some(next) = base.rerouted(&reroute.from, &reroute.to, reroute.reason, turn) {
            attached.model = Some(next);
        }
    }
    pub fn invalidate_admission(&self) {
        if let Some(p) = &self.pending {
            p.admission.store(false, Ordering::Release)
        }
    }
    /// Withdraws the admission of the binding request in flight before a mutation that
    /// may take its authority away is committed. The supervisor reads this flag
    /// immediately before it writes to the provider. Returns whether it was withdrawn
    /// here: one that was already withdrawn is not this caller's to give back.
    pub fn suspend_admission(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| p.admission.swap(false, Ordering::AcqRel))
    }
    /// The mutation changed nothing: the request keeps the admission it had.
    pub fn restore_admission(&self) {
        if let Some(p) = &self.pending {
            p.admission.store(true, Ordering::Release)
        }
    }
    pub fn status(
        &self,
        store: &Store,
        runtime: &Runtime,
        id: &ManagedSessionId,
    ) -> ControlResult<ResultValue> {
        let mut s = store.binding_status(id)?;
        // An open operation is in progress only while the core is still waiting for its
        // outcome. One it no longer tracks could not be settled durably: its provider
        // effect is unknown, and it is reported as exactly that until the next start
        // reconciles it. It blocks the session either way.
        if s.state == BindingState::Creating
            && !self.pending.as_ref().is_some_and(|p| {
                s.operation_id.as_ref().is_some_and(|open| {
                    open.as_str()
                        .eq_ignore_ascii_case(p.reservation.operation_id.as_str())
                })
            })
        {
            s.state = BindingState::Indeterminate;
        }
        if s.binding.is_some()
            && store.gate_session(id)?.control_state == ControlState::Prepared
            && let Some(live) = runtime.status().runtime_instance_id
            && runtime.status().actual_state == ActualState::Ready
            && let Some(attached) = self
                .attachments
                .get(&id.as_str().to_ascii_lowercase())
                .filter(|a| a.instance == live)
        {
            s.observed_model = attached.model.clone();
            s.state = BindingState::BoundAttached;
            s.runtime_instance_id = Some(live);
            s.attachment_profile = Some(
                if runtime.status().runtime_safety_profile
                    == Some(RuntimeSafetyProfile::CodexLocalToolsV1)
                {
                    AttachmentProfile::LocalToolsV1
                } else {
                    AttachmentProfile::InferenceOnlyV1
                },
            );
        }
        Ok(ResultValue::BindingStatus(s))
    }
    pub fn projection(
        &self,
        store: &Store,
        runtime: &Runtime,
        id: &ManagedSessionId,
    ) -> ControlResult<BindingStatus> {
        match self.status(store, runtime, id)? {
            ResultValue::BindingStatus(s) => Ok(s),
            _ => Err(ControlError::InternalError),
        }
    }
    pub fn begin(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        op: &Operation,
        reply: SyncSender<ControlResult<ResultValue>>,
    ) {
        let result = self.admit(store, runtime, op, reply.clone());
        if let Err(e) = result {
            if matches!(
                e,
                ControlError::StorageUnavailable | ControlError::StorageIncompatible
            ) {
                store.execution_available = false;
            }
            let _ = reply.try_send(Err(e));
        }
    }
    fn admit(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        op: &Operation,
        reply: SyncSender<ControlResult<ResultValue>>,
    ) -> ControlResult<()> {
        let mut op = op.clone();
        // A committed receipt is authoritative even if its workspace has since disappeared.
        // Only a create payload conflict may be a still-valid alias of the stored canonical path.
        match store.binding_receipt(&op) {
            Ok(Some(r)) => {
                let _ = reply.try_send(Ok(r));
                return Ok(());
            }
            Ok(None) => (),
            Err(ControlError::IntentConflict) if matches!(op, Operation::BindingCreate(_)) => (),
            Err(e) => return Err(e),
        }
        if let Operation::BindingCreate(p) = &mut op {
            p.workspace_root = canonical_workspace(&p.workspace_root)?;
        }
        if let Some(r) = store.binding_receipt(&op)? {
            let _ = reply.try_send(Ok(r));
            return Ok(());
        }
        if self.pending.is_some() {
            return Err(ControlError::RuntimeOperationInProgress);
        }
        let (session, action) = match &op {
            Operation::BindingCreate(p) => (&p.managed_session_id, ExecutionScope::SessionCreate),
            Operation::BindingResume(p) => (&p.managed_session_id, ExecutionScope::SessionResume),
            _ => return Err(ControlError::InvalidRequest),
        };
        if action == ExecutionScope::SessionResume && store.binding(session)?.is_none() {
            return Err(ControlError::BindingNotFound);
        }
        let status = runtime.status();
        // Discard stale attachment metadata; detached sessions have no background polling.
        self.attachments
            .retain(|_, a| status.runtime_instance_id.as_ref() == Some(&a.instance));
        if !self
            .attachments
            .contains_key(&session.as_str().to_ascii_lowercase())
            && self.attachments.len() >= MAX_ATTACHMENTS
        {
            return Err(ControlError::CapacityExceeded);
        }
        // An isolated generation serves exactly the workspace it was launched for.
        if status.runtime_safety_profile == Some(RuntimeSafetyProfile::CodexLocalToolsV1) {
            let workspace = match &op {
                Operation::BindingCreate(p) => Some(p.workspace_root.clone()),
                _ => store.binding(session)?.map(|b| b.workspace_root),
            };
            if workspace != status.workspace_root {
                return Err(ControlError::WorkspaceInvalid);
            }
        }
        if !matches!(
            ExecutionGate::evaluate(store, session, action, &status),
            ExecutionDecision::Allowed
        ) {
            return Err(ControlError::GateDenied);
        }
        let instance = status.runtime_instance_id.ok_or(ControlError::GateDenied)?;
        let expires_at = store
            .gate_lease(session)?
            .ok_or(ControlError::GateDenied)?
            .expires_at;
        let reservation = store.reserve_binding(&op)?;
        // No provider command exists before both reservation and dispatch marker commits.
        store.dispatch_binding(&reservation)?;
        // A new resume must prove attachment afresh, including after provider rejection.
        if action == ExecutionScope::SessionResume {
            self.attachments
                .remove(&session.as_str().to_ascii_lowercase());
        }
        let admission = Arc::new(AtomicBool::new(true));
        let command = ConversationCommand {
            operation_id: reservation.operation_id.clone(),
            request_id: super::runtime::request_id(&reservation.request_id),
            workspace: reservation.workspace.clone(),
            thread_id: reservation.thread_id.clone(),
            expires_at,
            admission: admission.clone(),
        };
        if !runtime.command(command) {
            // Whatever keeps the uncertainty from being recorded closes execution.
            if let Err(e) = store.fail_binding(&reservation, true) {
                store.execution_available = false;
                return Err(e);
            }
            return Err(ControlError::ExternalEffectUnknown);
        }
        self.pending = Some(Pending {
            reservation,
            instance,
            reply,
            admission,
        });
        Ok(())
    }
    pub fn checkpoint(
        &mut self,
        store: &mut Store,
        id: &AgentRuntimeInstanceId,
        operation: &crate::contracts::WireUuid,
        thread: &super::runtime::ThreadId,
        stage: super::runtime::MaterializationStage,
        reply: &SyncSender<bool>,
    ) {
        let result = self
            .pending
            .as_ref()
            .filter(|p| p.instance == *id && p.reservation.operation_id == *operation)
            .ok_or(ControlError::CorrelationError)
            .and_then(|p| store.checkpoint_binding(&p.reservation, thread, stage));
        if result.is_err() {
            store.execution_available = false;
        }
        let _ = reply.try_send(result.is_ok());
    }
    pub fn complete(
        &mut self,
        store: &mut Store,
        runtime: &Runtime,
        id: &AgentRuntimeInstanceId,
        operation: &crate::contracts::WireUuid,
        result: Result<super::runtime::ThreadId, ConversationFailure>,
        model: Option<ObservedModel>,
    ) {
        if !self
            .pending
            .as_ref()
            .is_some_and(|p| p.instance == *id && p.reservation.operation_id == *operation)
        {
            store.execution_available = false;
            return;
        }
        let p = self.pending.take().unwrap();
        let result = match result {
            Ok(thread) => match store.commit_binding(&p.reservation, &thread, id) {
                Ok(b) => {
                    if p.reservation.thread_id.is_some()
                        && runtime.status().runtime_instance_id.as_ref() == Some(id)
                        && store
                            .gate_session(&p.reservation.session_id)
                            .is_ok_and(|s| s.control_state == ControlState::Prepared)
                    {
                        self.attachments.insert(
                            p.reservation.session_id.as_str().to_ascii_lowercase(),
                            Attached {
                                instance: id.clone(),
                                model,
                            },
                        );
                    }
                    Ok(ResultValue::Binding(b))
                }
                Err(e) => {
                    store.execution_available = false;
                    let _ = store.fail_binding(&p.reservation, true);
                    Err(e)
                }
            },
            Err(f) => {
                let uncertain = matches!(f, ConversationFailure::Unknown);
                match store.fail_binding(&p.reservation, uncertain) {
                    Ok(()) => Err(match f {
                        ConversationFailure::Unknown => ControlError::ExternalEffectUnknown,
                        ConversationFailure::Rejected => ControlError::ProviderRejected,
                        ConversationFailure::NotDispatched => ControlError::GateDenied,
                    }),
                    Err(e) => {
                        store.execution_available = false;
                        Err(e)
                    }
                }
            }
        };
        let _ = p.reply.try_send(result);
    }
    pub fn reject_pre_dispatch(
        &mut self,
        store: &mut Store,
        runtime: &Runtime,
        id: &AgentRuntimeInstanceId,
    ) {
        if let Some(p) = &self.pending {
            let operation = p.reservation.operation_id.clone();
            self.complete(
                store,
                runtime,
                id,
                &operation,
                Err(ConversationFailure::NotDispatched),
                None,
            );
        }
        self.attachments.clear();
    }
    pub fn detach(&mut self, store: &mut Store) {
        self.invalidate_admission();
        self.attachments.clear();
        if let Some(p) = self.pending.take() {
            if store.fail_binding(&p.reservation, true).is_err() {
                store.execution_available = false;
            }
            let _ = p.reply.try_send(Err(ControlError::ExternalEffectUnknown));
        }
    }
}
