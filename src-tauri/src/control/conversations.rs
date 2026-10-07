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
    control::*,
    identities::{AgentRuntimeInstanceId, ManagedSessionId},
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
    attachments: BTreeMap<String, AgentRuntimeInstanceId>,
}
impl Conversations {
    pub fn invalidate_admission(&self) {
        if let Some(p) = &self.pending {
            p.admission.store(false, Ordering::Release)
        }
    }
    pub fn status(
        &self,
        store: &Store,
        runtime: &Runtime,
        id: &ManagedSessionId,
    ) -> ControlResult<ResultValue> {
        let mut s = store.binding_status(id)?;
        if s.binding.is_some()
            && store.gate_session(id)?.control_state == ControlState::Prepared
            && let Some(live) = runtime.status().runtime_instance_id
            && runtime.status().actual_state == ActualState::Ready
            && self.attachments.get(&id.as_str().to_ascii_lowercase()) == Some(&live)
        {
            s.state = BindingState::BoundAttached;
            s.runtime_instance_id = Some(live);
        }
        Ok(ResultValue::BindingStatus(s))
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
            .retain(|_, id| status.runtime_instance_id.as_ref() == Some(id));
        if !self
            .attachments
            .contains_key(&session.as_str().to_ascii_lowercase())
            && self.attachments.len() >= MAX_ATTACHMENTS
        {
            return Err(ControlError::CapacityExceeded);
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
            store.fail_binding(&reservation, true)?;
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
                            id.clone(),
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
