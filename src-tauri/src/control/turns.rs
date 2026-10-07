//! Core-owned single active turn; provider IO remains exclusively in the supervisor.
use super::{
    conversations::Conversations,
    gate::{ExecutionDecision, ExecutionGate},
    runtime::{ConversationFailure, Runtime, RuntimeCommand, TurnCommand},
    store::{Store, turns::StartFingerprint},
    turn_stream::Streams,
};
use crate::{
    adapters::codex::{
        CodexTurnId, TurnOutcome,
        safety::{NoticeKind, TurnNotice},
    },
    contracts::{
        control::*,
        identities::{AgentRuntimeInstanceId, ManagedTurnId},
    },
};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
struct Active {
    turn: ManagedTurnRecord,
    admission: Arc<AtomicBool>,
    provisional: VecDeque<TurnNotice>,
    provisional_bytes: usize,
    cancel: Option<TurnReason>,
    interrupt_sent: bool,
    provider_started: bool,
    deadline: Option<Instant>,
}
#[derive(Default)]
pub(super) struct Turns {
    active: Option<Active>,
    streams: Streams,
}
impl Turns {
    pub fn start(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        conversations: &Conversations,
        p: &TurnStart,
    ) -> ControlResult<ResultValue> {
        let fingerprint = StartFingerprint::compute(p)?;
        if let Some(t) = store.turn_receipt(&fingerprint)? {
            return Ok(ResultValue::Turn(t));
        }
        let binding = conversations.projection(store, runtime, &p.managed_session_id)?;
        match ExecutionGate::turn(store, &p.managed_session_id, &runtime.status(), &binding) {
            ExecutionDecision::Allowed => (),
            ExecutionDecision::Denied(GateReason::ActiveTurnExists) => {
                return Err(ControlError::ActiveTurnExists);
            }
            _ => return Err(ControlError::GateDenied),
        }
        let b = binding.binding.ok_or(ControlError::BindingNotFound)?;
        let lease = store
            .gate_lease(&p.managed_session_id)?
            .ok_or(ControlError::GateDenied)?;
        let instance = runtime
            .status()
            .runtime_instance_id
            .ok_or(ControlError::GateDenied)?;
        let t = store.reserve_turn(&fingerprint, &b, &lease, &instance)?;
        let admission = Arc::new(AtomicBool::new(true));
        self.active = Some(Active {
            turn: t.clone(),
            admission: admission.clone(),
            provisional: VecDeque::new(),
            provisional_bytes: 0,
            cancel: None,
            interrupt_sent: false,
            provider_started: false,
            deadline: None,
        });
        let command = RuntimeCommand::Start(TurnCommand {
            managed_turn_id: t.managed_turn_id.clone(),
            thread_id: t.codex_thread_id.clone(),
            workspace: b.workspace_root,
            prompt: p.prompt.clone(),
            expires_at: lease.expires_at,
            admission,
        });
        if !runtime.turn_command(command) {
            self.lost(store, TurnReason::DispatchUnknown)?;
            runtime.contain(store, RuntimeDiagnostic::TransportFailed)?;
            return Err(ControlError::ExternalEffectUnknown);
        }
        // Async committed admission receipt. Status observes subsequent running/terminal revisions.
        Ok(ResultValue::Turn(t))
    }
    pub fn status(&self, store: &Store, p: &TurnGet) -> ControlResult<ResultValue> {
        let turn = store.turn(&p.managed_turn_id)?;
        let page = self.streams.page(
            &turn,
            &TurnEvents {
                managed_turn_id: p.managed_turn_id.clone(),
                after_sequence: None,
                limit: 1,
            },
        );
        Ok(ResultValue::TurnStatus(ManagedTurnStatus {
            turn,
            stream_earliest_sequence: page.earliest_sequence,
            stream_latest_sequence: page.latest_sequence,
            stream_retained: page.retained,
        }))
    }
    pub fn events(&self, store: &Store, p: &TurnEvents) -> ControlResult<ResultValue> {
        let t = store.turn(&p.managed_turn_id)?;
        Ok(ResultValue::TurnEvents(self.streams.page(&t, p)))
    }
    pub fn cancel(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        p: &TurnCancel,
    ) -> ControlResult<ResultValue> {
        let t = store.cancel_turn_receipt(p, TurnReason::UserCancel)?;
        if self
            .active
            .as_ref()
            .is_some_and(|a| a.turn.managed_turn_id == p.managed_turn_id)
        {
            self.request_cancel(runtime, TurnReason::UserCancel);
        }
        Ok(ResultValue::Turn(t))
    }
    fn request_cancel(&mut self, runtime: &mut Runtime, reason: TurnReason) {
        let Some(a) = self.active.as_mut() else {
            return;
        };
        a.admission.store(false, Ordering::Release);
        if a.cancel.is_none() {
            a.cancel = Some(reason);
            a.deadline = Some(Instant::now() + Duration::from_secs(5));
        }
        if !a.interrupt_sent
            && a.provider_started
            && let Some(turn_id) = a.turn.codex_turn_id.clone()
        {
            a.interrupt_sent = true;
            if !runtime.turn_command(RuntimeCommand::Interrupt {
                managed_turn_id: a.turn.managed_turn_id.clone(),
                thread_id: a.turn.codex_thread_id.clone(),
                turn_id,
            }) {
                a.deadline = Some(Instant::now());
            }
        }
    }
    pub fn shutdown(&mut self, runtime: &mut Runtime) {
        self.request_cancel(runtime, TurnReason::AuthorityShutdown);
    }
    pub fn active(&self) -> bool {
        self.active.is_some()
    }
    pub fn wait(&self) -> Option<Duration> {
        self.active
            .as_ref()
            .and_then(|a| a.deadline)
            .map(|d| d.saturating_duration_since(Instant::now()))
    }
    /// Called on state-change/deadline wakeups, never via busy SQLite polling.
    pub fn enforce(&mut self, store: &mut Store, runtime: &mut Runtime) -> ControlResult<bool> {
        let Some(a) = self.active.as_ref() else {
            return Ok(false);
        };
        if a.deadline.is_some_and(|d| Instant::now() >= d) {
            runtime.contain(store, RuntimeDiagnostic::TransportFailed)?;
            self.lost(store, TurnReason::InterruptTimeout)?;
            return Ok(true);
        }
        if let Some(reason) = a.cancel {
            // turn/start acknowledges enqueueing before the provider is interruptible.
            // A correlated turn/started activates the one pending reductive request.
            self.request_cancel(runtime, reason);
            return Ok(false);
        }
        let reason = if !store.execution_available {
            Some(TurnReason::RuntimeFailed)
        } else if store
            .gate_session(&a.turn.managed_session_id)?
            .control_state
            != ControlState::Prepared
        {
            Some(TurnReason::SessionClosed)
        } else {
            let lease = store.turn_lease(&a.turn)?;
            if timestamp(&lease.expires_at)? <= store.lease_now()
                || lease.state == LeaseState::Expired
            {
                Some(TurnReason::LeaseExpired)
            } else if lease.state != LeaseState::Active {
                Some(TurnReason::LeaseRevoked)
            } else {
                None
            }
        };
        if let Some(r) = reason {
            self.request_cancel(runtime, r);
        }
        Ok(false)
    }
    pub fn accepted(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        instance: &AgentRuntimeInstanceId,
        id: &ManagedTurnId,
        result: Result<CodexTurnId, ConversationFailure>,
    ) -> ControlResult<()> {
        if !self.active.as_ref().is_some_and(|a| {
            a.turn.managed_turn_id == *id && a.turn.runtime_instance_id == *instance
        }) {
            return Ok(());
        }
        match result {
            Ok(provider) => {
                let t = store.transition_turn(id, Some(&provider), TurnState::Running, None)?;
                let a = self.active.as_mut().unwrap();
                a.turn = t.clone();
                self.streams.push(&t, StreamKind::TurnStarted, None);
                let notices = std::mem::take(&mut a.provisional);
                a.provisional_bytes = 0;
                for n in notices {
                    self.notice(store, instance, n)?;
                }
                if let Some(reason) = self.active.as_ref().and_then(|a| a.cancel) {
                    self.request_cancel(runtime, reason);
                }
            }
            Err(ConversationFailure::Rejected) => {
                self.finish(store, TurnState::Failed, TurnReason::ProviderFailed)?
            }
            Err(ConversationFailure::NotDispatched) => {
                let reason = self
                    .active
                    .as_ref()
                    .and_then(|a| a.cancel)
                    .unwrap_or(TurnReason::LeaseExpired);
                self.finish(store, TurnState::Interrupted, reason)?
            }
            Err(ConversationFailure::Unknown) => self.lost(store, TurnReason::DispatchUnknown)?,
        }
        Ok(())
    }
    pub fn notice(
        &mut self,
        store: &mut Store,
        instance: &AgentRuntimeInstanceId,
        n: TurnNotice,
    ) -> ControlResult<()> {
        let Some(a) = self.active.as_mut() else {
            return Ok(());
        };
        if a.turn.runtime_instance_id != *instance || a.turn.codex_thread_id != n.thread {
            return Ok(());
        }
        if a.turn.codex_turn_id.is_none() {
            let bytes = match &n.kind {
                NoticeKind::Delta(_, s) => s.len(),
                _ => 0,
            };
            if a.provisional.len() >= 32 || a.provisional_bytes + bytes > 128 * 1024 {
                return Err(ControlError::CapacityExceeded);
            }
            a.provisional_bytes += bytes;
            a.provisional.push_back(n);
            return Ok(());
        }
        if a.turn.codex_turn_id.as_ref() != Some(&n.turn) {
            return Ok(());
        }
        match n.kind {
            NoticeKind::Started => a.provider_started = true,
            NoticeKind::Delta(kind, text) => self.streams.push(&a.turn, kind, Some(&text)),
            NoticeKind::MessageCompleted => {
                self.streams
                    .push(&a.turn, StreamKind::AssistantMessageCompleted, None)
            }
            NoticeKind::SummaryPart => {
                self.streams
                    .push(&a.turn, StreamKind::ReasoningSummaryPart, None)
            }
            NoticeKind::Terminal(outcome) => {
                let (state, reason) = match outcome {
                    TurnOutcome::Completed => (TurnState::Completed, TurnReason::Completed),
                    TurnOutcome::Interrupted => (
                        TurnState::Interrupted,
                        a.cancel.unwrap_or(TurnReason::UserCancel),
                    ),
                    TurnOutcome::Failed => (TurnState::Failed, TurnReason::ProviderFailed),
                    TurnOutcome::InProgress => return Ok(()),
                };
                self.finish(store, state, reason)?;
            }
        }
        Ok(())
    }
    fn finish(
        &mut self,
        store: &mut Store,
        state: TurnState,
        reason: TurnReason,
    ) -> ControlResult<()> {
        if let Some(a) = &self.active {
            let t = match store.transition_turn(&a.turn.managed_turn_id, None, state, Some(reason))
            {
                Ok(t) => t,
                Err(e) => {
                    // Storage uncertainty is terminal for in-memory admission. The durable
                    // nonterminal record is conservatively reconciled on the next startup.
                    store.execution_available = false;
                    if let Some(a) = self.active.take() {
                        a.admission.store(false, Ordering::Release);
                    }
                    return Err(e);
                }
            };
            let kind = match state {
                TurnState::Completed => StreamKind::TurnCompleted,
                TurnState::Interrupted => StreamKind::TurnInterrupted,
                _ => StreamKind::TurnFailed,
            };
            self.streams.push(&t, kind, None);
        }
        if let Some(a) = self.active.take() {
            a.admission.store(false, Ordering::Release);
        }
        Ok(())
    }
    pub fn lost(&mut self, store: &mut Store, reason: TurnReason) -> ControlResult<()> {
        self.finish(
            store,
            if reason == TurnReason::InferenceIsolationViolation {
                TurnState::Failed
            } else {
                TurnState::Indeterminate
            },
            reason,
        )
    }
}
