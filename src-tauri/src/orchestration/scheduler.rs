//! Finite dependency scheduler over the existing protocol-2 Managed authority.
//! This module is a client of `wyss-control`, never an execution authority of its own.
use super::journal::CancellationOutcome;
use super::{
    CoordinatorError, CoordinatorSnapshot, EffectClass, FailureCode, HandoffConfirmationV1,
    HandoffEnvelopeV1, MAX_RUN_SECONDS, OrchestrationPlanV1, PlanLimitsV1, PlanReviewV1,
    ProviderLane, RunJournal, RunState, StepPlanV1, StepSnapshot, StepState, StopPolicy,
    workspace_identity, workspace_revision,
};
use crate::{
    contracts::{
        WireUuid,
        control::{
            ActualState, BindingState, ControlError as E, ControlState, ExecutionScope, GateReason,
            Get, LeaseActive, Operation, ResultValue, RuntimeSafetyProfile, RuntimeTarget,
            TurnCancel, TurnGet, TurnStart,
        },
        identities::{AgentRuntimeInstanceId, ManagedSessionId, ManagedTurnId},
        turn::{TurnReason, TurnState},
    },
    f8::{
        gui::{CallError, Connector, F8Client, SocketConnector},
        protocol::{Projection, RuntimeFence, Target},
    },
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError, atomic::AtomicBool},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeFault {
    Busy,
    Denied,
    Unavailable,
    Unknown,
}

/// A fresh, read-only admission observation. The authority revalidates everything when the
/// subsequent `turn/start` reaches it. The bound path exists only in process memory.
pub struct Admission {
    authority_id: String,
    session_id: String,
    runtime_id: String,
    generation: u64,
    session_revision: u64,
    workspace_root: PathBuf,
}

pub struct StartedTurn {
    pub turn_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnObservation {
    pub state: TurnState,
    pub reason: Option<TurnReason>,
    pub revision: u64,
}

/// Test seams return closed enums only. No prose or provider output can reach the journal.
pub trait ManagedAuthority: Send + Sync + 'static {
    fn inspect(
        &self,
        session_id: &str,
        effect: EffectClass,
        expected_model: Option<&str>,
    ) -> Result<Admission, BridgeFault>;
    fn start(
        &self,
        admission: &Admission,
        intent_id: &str,
        prompt: &str,
    ) -> Result<StartedTurn, BridgeFault>;
    fn observe(&self, turn_id: &str) -> Result<TurnObservation, BridgeFault>;
    fn cancel(&self, step: &StepSnapshot, intent_id: &str) -> Result<(), BridgeFault>;
    fn handoff_start(
        &self,
        admission: &Admission,
        intent_id: &str,
        prompt: &str,
    ) -> Result<StartedTurn, BridgeFault> {
        self.start(admission, intent_id, prompt)
    }
}

/// Concrete bridge. It speaks only the existing F8 protocol and lets its normal authority
/// perform lease, binding, profile, gate, model and approval checks.
pub struct F8Authority {
    connector: SocketConnector,
    cancel: Arc<AtomicBool>,
}
impl F8Authority {
    pub fn new(runtime_root: impl Into<PathBuf>) -> Self {
        Self {
            connector: SocketConnector::new(runtime_root),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn client(&self) -> Result<F8Client, BridgeFault> {
        let stream = self
            .connector
            .connect()
            .map_err(|_| BridgeFault::Unavailable)?;
        F8Client::greet(stream, self.cancel.clone()).map_err(|_| BridgeFault::Unavailable)
    }
}

impl ManagedAuthority for F8Authority {
    fn inspect(
        &self,
        session_id: &str,
        effect: EffectClass,
        expected_model: Option<&str>,
    ) -> Result<Admission, BridgeFault> {
        let session = ManagedSessionId::parse(session_id).map_err(|_| BridgeFault::Denied)?;
        let mut client = self.client()?;
        let session_record = match client
            .call(
                Operation::Get(Get {
                    managed_session_id: session.clone(),
                }),
                Target::None,
            )
            .map_err(map_read_fault)?
            .value
        {
            ResultValue::Session(record) if record.managed_session_id == session => record,
            _ => return Err(BridgeFault::Unknown),
        };
        if session_record.control_state != ControlState::Prepared
            || session_record.runtime_kind
                != crate::contracts::agent_adapter::AgentRuntimeKind::Codex
        {
            return Err(BridgeFault::Denied);
        }
        let binding = match client
            .call(
                Operation::BindingStatus(Get {
                    managed_session_id: session.clone(),
                }),
                Target::None,
            )
            .map_err(map_read_fault)?
            .value
        {
            ResultValue::BindingStatus(binding) if binding.managed_session_id == session => binding,
            _ => return Err(BridgeFault::Unknown),
        };
        if binding.state != BindingState::BoundAttached {
            return Err(BridgeFault::Denied);
        }
        let session_binding = binding.binding.ok_or(BridgeFault::Denied)?;
        let attached_instance = binding.runtime_instance_id.ok_or(BridgeFault::Denied)?;
        let profile = binding.attachment_profile.ok_or(BridgeFault::Denied)?;
        let required_profile = match effect {
            EffectClass::ManagedInference => RuntimeSafetyProfile::CodexInferenceV1,
            EffectClass::ManagedLocalTools => RuntimeSafetyProfile::CodexLocalToolsV1,
            _ => return Err(BridgeFault::Denied),
        };
        let expected_attachment = match required_profile {
            RuntimeSafetyProfile::CodexInferenceV1 => {
                crate::contracts::turn::AttachmentProfile::InferenceOnlyV1
            }
            RuntimeSafetyProfile::CodexLocalToolsV1 => {
                crate::contracts::turn::AttachmentProfile::LocalToolsV1
            }
            _ => return Err(BridgeFault::Denied),
        };
        if profile != expected_attachment {
            return Err(BridgeFault::Denied);
        }
        if expected_model.is_some_and(|expected| {
            binding
                .observed_model
                .as_ref()
                .is_none_or(|model| model.model_id != expected)
        }) {
            return Err(BridgeFault::Denied);
        }
        let runtime = client
            .call(
                Operation::RuntimeStatus(RuntimeTarget::codex()),
                Target::Runtime {
                    session: session.clone(),
                    generation: None,
                    instance: None,
                },
            )
            .map_err(map_read_fault)?;
        let ResultValue::RuntimeStatus(status) = runtime.value else {
            return Err(BridgeFault::Unknown);
        };
        let Projection::Runtime {
            session: observed_session,
            generation,
            identity: Some(identity),
            ..
        } = runtime.projection
        else {
            return Err(BridgeFault::Unknown);
        };
        if observed_session != session
            || status.actual_state != ActualState::Ready
            || status.desired_state != crate::contracts::control::DesiredState::Running
            || status.compatibility
                != crate::contracts::agent_adapter::CompatibilityState::Compatible
            || status.runtime_safety_profile != Some(required_profile)
            || status.runtime_instance_id.as_ref() != Some(&attached_instance)
            || identity.managed_session_id != session
            || identity.runtime_instance_id != attached_instance
            || identity.generation != generation
            || generation == 0
        {
            return Err(BridgeFault::Denied);
        }
        match client
            .call(
                Operation::LeaseActive(LeaseActive {
                    session_id: session.clone(),
                }),
                Target::None,
            )
            .map_err(map_read_fault)?
            .value
        {
            ResultValue::ActiveLease(Some(lease)) if lease.session_id == session => (),
            ResultValue::ActiveLease(None) => return Err(BridgeFault::Denied),
            _ => return Err(BridgeFault::Unknown),
        }
        match client
            .call(
                Operation::GateStatus(crate::contracts::control::GateQuery {
                    session_id: session.clone(),
                    action: ExecutionScope::TurnStart,
                }),
                Target::None,
            )
            .map_err(map_read_fault)?
            .value
        {
            ResultValue::GateStatus(gate)
                if gate.session_id == session && gate.action == ExecutionScope::TurnStart =>
            {
                match gate.reason {
                    GateReason::Allowed => (),
                    GateReason::ActiveTurnExists => return Err(BridgeFault::Busy),
                    _ => return Err(BridgeFault::Denied),
                }
            }
            _ => return Err(BridgeFault::Unknown),
        }
        Ok(Admission {
            authority_id: client.authority().as_str().to_owned(),
            session_id: session.as_str().to_owned(),
            runtime_id: attached_instance.as_str().to_owned(),
            generation,
            session_revision: session_record.revision,
            workspace_root: PathBuf::from(session_binding.workspace_root),
        })
    }

    fn start(
        &self,
        admission: &Admission,
        intent_id: &str,
        prompt: &str,
    ) -> Result<StartedTurn, BridgeFault> {
        let intent = WireUuid::try_from(intent_id.to_owned()).map_err(|_| BridgeFault::Denied)?;
        let session =
            ManagedSessionId::parse(&admission.session_id).map_err(|_| BridgeFault::Denied)?;
        let instance = AgentRuntimeInstanceId::parse(&admission.runtime_id)
            .map_err(|_| BridgeFault::Denied)?;
        let mut client = self.client()?;
        if client.authority().as_str() != admission.authority_id {
            return Err(BridgeFault::Denied);
        }
        let answer = client
            .call(
                Operation::TurnStart(TurnStart {
                    intent_id: intent,
                    managed_session_id: session.clone(),
                    expected_session_revision: admission.session_revision,
                    prompt: prompt.to_owned(),
                }),
                Target::Fenced(RuntimeFence {
                    managed_session_id: session,
                    generation: admission.generation,
                    runtime_instance_id: Some(instance),
                }),
            )
            .map_err(map_effect_fault)?;
        let ResultValue::Turn(turn) = answer.value else {
            return Err(BridgeFault::Unknown);
        };
        Ok(StartedTurn {
            turn_id: turn.managed_turn_id.as_str().to_owned(),
            revision: turn.revision,
        })
    }

    fn observe(&self, turn_id: &str) -> Result<TurnObservation, BridgeFault> {
        let turn = ManagedTurnId::parse(turn_id).map_err(|_| BridgeFault::Denied)?;
        let mut client = self.client()?;
        let answer = client
            .call(
                Operation::TurnStatus(TurnGet {
                    managed_turn_id: turn.clone(),
                }),
                Target::None,
            )
            .map_err(map_read_fault)?;
        let ResultValue::TurnStatus(status) = answer.value else {
            return Err(BridgeFault::Unknown);
        };
        if status.turn.managed_turn_id != turn {
            return Err(BridgeFault::Unknown);
        }
        Ok(TurnObservation {
            state: status.turn.state,
            reason: status.turn.terminal_reason,
            revision: status.turn.revision,
        })
    }

    fn cancel(&self, step: &StepSnapshot, intent_id: &str) -> Result<(), BridgeFault> {
        let session = ManagedSessionId::parse(
            step.managed_session_id
                .as_deref()
                .ok_or(BridgeFault::Denied)?,
        )
        .map_err(|_| BridgeFault::Denied)?;
        let turn =
            ManagedTurnId::parse(step.managed_turn_id.as_deref().ok_or(BridgeFault::Denied)?)
                .map_err(|_| BridgeFault::Denied)?;
        let instance = AgentRuntimeInstanceId::parse(
            step.runtime_instance_id
                .as_deref()
                .ok_or(BridgeFault::Denied)?,
        )
        .map_err(|_| BridgeFault::Denied)?;
        let intent = WireUuid::try_from(intent_id.to_owned()).map_err(|_| BridgeFault::Denied)?;
        let mut client = self.client()?;
        if step.authority_instance_id.as_deref() != Some(client.authority().as_str()) {
            return Err(BridgeFault::Denied);
        }
        client
            .call(
                Operation::TurnCancel(TurnCancel {
                    intent_id: intent,
                    managed_turn_id: turn,
                    expected_revision: step.turn_revision.ok_or(BridgeFault::Denied)?,
                }),
                Target::Fenced(RuntimeFence {
                    managed_session_id: session,
                    generation: step.runtime_generation.ok_or(BridgeFault::Denied)?,
                    runtime_instance_id: Some(instance),
                }),
            )
            .map_err(map_effect_fault)?;
        Ok(())
    }
}

fn map_read_fault(error: CallError) -> BridgeFault {
    match error {
        CallError::Refused(E::ActiveTurnExists | E::CapacityExceeded) => BridgeFault::Busy,
        CallError::Refused(_) | CallError::Local(_) => BridgeFault::Denied,
        CallError::Broken(_) => BridgeFault::Unavailable,
    }
}
fn map_effect_fault(error: CallError) -> BridgeFault {
    match error {
        CallError::Refused(E::ActiveTurnExists | E::CapacityExceeded) => BridgeFault::Busy,
        CallError::Refused(_) | CallError::Local(_) => BridgeFault::Denied,
        CallError::Broken(_) => BridgeFault::Unknown,
    }
}

struct ActivePlan {
    plan: OrchestrationPlanV1,
}

pub struct Coordinator {
    journal: Arc<RunJournal>,
    authority: Arc<dyn ManagedAuthority>,
    plans: Mutex<HashMap<String, ActivePlan>>,
    started: Mutex<HashMap<(String, String), Instant>>,
    round_robin: Mutex<usize>,
}

impl Coordinator {
    pub fn new(
        journal: Arc<RunJournal>,
        authority: Arc<dyn ManagedAuthority>,
        now: u64,
    ) -> Result<Self, CoordinatorError> {
        journal
            .recover_after_restart(now)
            .map_err(|_| CoordinatorError::JournalUnavailable)?;
        Ok(Self {
            journal,
            authority,
            plans: Mutex::new(HashMap::new()),
            started: Mutex::new(HashMap::new()),
            round_robin: Mutex::new(0),
        })
    }

    pub fn submit(
        &self,
        plan: OrchestrationPlanV1,
        review: PlanReviewV1,
        now: u64,
    ) -> Result<CoordinatorSnapshot, CoordinatorError> {
        plan.validate(now)?;
        if !review.matches(&plan) {
            return Err(CoordinatorError::ReviewStale);
        }
        let identity = workspace_identity(std::path::Path::new(&plan.workspace_root))
            .map_err(|_| CoordinatorError::InvalidPlan)?;
        let revision = workspace_revision(std::path::Path::new(&plan.workspace_root))
            .map_err(|_| CoordinatorError::InvalidPlan)?;
        if identity != plan.workspace_id || revision != plan.workspace_revision {
            return Err(CoordinatorError::WorkspaceConflict);
        }
        let mut run = self
            .journal
            .create_run(&plan, &review, now)
            .map_err(map_journal)?;
        if run.state != RunState::Ready {
            return Ok(run);
        }
        run = self
            .journal
            .set_run_state(&run.run_id, run.revision, RunState::Running, None, now)
            .map_err(map_journal)?;
        let mut any_admitted_lane = false;
        for step in &plan.steps {
            if step.provider == ProviderLane::ManagedCodex
                && matches!(
                    step.effect,
                    EffectClass::ManagedInference | EffectClass::ManagedLocalTools
                )
            {
                any_admitted_lane = true;
            } else {
                let snapshot = self
                    .journal
                    .get(&run.run_id)
                    .map_err(map_journal)?
                    .ok_or(CoordinatorError::UnknownRun)?;
                let row = snapshot
                    .steps
                    .iter()
                    .find(|row| row.step_id == step.step_id)
                    .ok_or(CoordinatorError::UnknownRun)?;
                self.journal
                    .set_step_state(
                        &run.run_id,
                        &step.step_id,
                        row.state,
                        StepState::Blocked,
                        Some(
                            if matches!(
                                step.provider,
                                ProviderLane::NativeCodex | ProviderLane::NativeClaude
                            ) {
                                FailureCode::TrustBoundaryBlocked
                            } else {
                                FailureCode::ProviderNotAdmitted
                            },
                        ),
                        now,
                    )
                    .map_err(map_journal)?;
            }
        }
        run = self
            .journal
            .get(&run.run_id)
            .map_err(map_journal)?
            .ok_or(CoordinatorError::UnknownRun)?;
        if !any_admitted_lane {
            run = self
                .journal
                .set_run_state(
                    &run.run_id,
                    run.revision,
                    RunState::Blocked,
                    Some(FailureCode::ProviderNotAdmitted),
                    now,
                )
                .map_err(map_journal)?;
            return Ok(run);
        }
        self.plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(run.run_id.clone(), ActivePlan { plan });
        Ok(run)
    }

    /// Sends a reviewed handoff only to a new Managed Codex turn through the ordinary F8
    /// authority path. Native and unqualified providers remain explicit blocked capabilities.
    pub fn submit_handoff(
        &self,
        envelope: HandoffEnvelopeV1,
        confirmation: HandoffConfirmationV1,
        now: u64,
    ) -> Result<CoordinatorSnapshot, CoordinatorError> {
        if confirmation.handoff_id == envelope.handoff_id
            && confirmation.revision == envelope.revision
            && confirmation.reviewed_digest == envelope.content_digest
            && let Some(existing) = self
                .journal
                .handoff_run(
                    &envelope.handoff_id,
                    envelope.revision,
                    &envelope.content_digest,
                )
                .map_err(map_journal)?
        {
            return Ok(existing);
        }
        confirmation.validate_for(&envelope, now)?;
        if envelope.source.host_id != envelope.target.host_id {
            return Err(CoordinatorError::ProviderBlocked);
        }
        let source_root = PathBuf::from(&envelope.source.workspace_root);
        if workspace_identity(&source_root).map_err(|_| CoordinatorError::WorkspaceConflict)?
            != envelope.source.workspace_id
            || workspace_revision(&source_root).map_err(|_| CoordinatorError::WorkspaceConflict)?
                != envelope.source.workspace_revision
        {
            return Err(CoordinatorError::WorkspaceConflict);
        }
        if envelope.target.provider != ProviderLane::ManagedCodex {
            return Err(match envelope.target.provider {
                ProviderLane::NativeCodex | ProviderLane::NativeClaude => {
                    CoordinatorError::TrustBoundaryBlocked
                }
                ProviderLane::ManagedClaude | ProviderLane::HermesPlanner => {
                    CoordinatorError::ProviderBlocked
                }
                ProviderLane::ManagedCodex => CoordinatorError::InvalidHandoff,
            });
        }
        let managed_session_id = envelope
            .target
            .managed_session_id
            .clone()
            .ok_or(CoordinatorError::InvalidHandoff)?;
        let first_message = envelope.first_message()?;
        let expires_at = envelope
            .expires_at_unix
            .min(now.saturating_add(u64::from(super::MAX_RUN_SECONDS)));
        let duration = expires_at.saturating_sub(now);
        if duration == 0 {
            return Err(CoordinatorError::Expired);
        }
        let plan = OrchestrationPlanV1 {
            schema_version: super::PLAN_SCHEMA_VERSION,
            plan_id: Uuid::new_v4().to_string(),
            revision: 1,
            host_id: envelope.target.host_id.clone(),
            project_id: envelope.target.project_id.clone(),
            workspace_id: envelope.target.workspace_id.clone(),
            workspace_root: envelope.target.workspace_root.clone(),
            workspace_revision: envelope.target.workspace_revision.clone(),
            objective: envelope.objective.clone(),
            created_at_unix: now,
            expires_at_unix: expires_at,
            limits: PlanLimitsV1 {
                max_steps: 1,
                max_parallelism: super::PRODUCTION_PARALLELISM,
                max_total_seconds: duration as u32,
                stop_policy: StopPolicy::CancelCurrentManagedTurn,
            },
            steps: vec![StepPlanV1 {
                step_id: Uuid::new_v4().to_string(),
                dependencies: vec![],
                required: true,
                provider: ProviderLane::ManagedCodex,
                effect: envelope.target.effect,
                managed_session_id: Some(managed_session_id),
                model_id: None,
                prompt: first_message,
                max_attempts: 1,
                max_duration_seconds: super::MAX_STEP_SECONDS.min(duration as u32),
            }],
        };
        let review = PlanReviewV1 {
            plan_id: plan.plan_id.clone(),
            revision: plan.revision,
            digest: plan.digest()?,
        };
        let run = self.submit(plan, review, now)?;
        if self
            .journal
            .record_handoff(&run.run_id, &envelope, &confirmation, now)
            .is_err()
        {
            self.plans
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&run.run_id);
            self.journal
                .set_run_state(
                    &run.run_id,
                    run.revision,
                    RunState::Failed,
                    Some(FailureCode::AuthorityUnavailable),
                    now,
                )
                .map_err(map_journal)?;
            return Err(CoordinatorError::JournalUnavailable);
        }
        self.get(&run.run_id)
    }

    pub fn list(&self) -> Result<Vec<CoordinatorSnapshot>, CoordinatorError> {
        self.journal.list(128).map_err(map_journal)
    }

    /// Whether the daemon still has scheduled work that must continue without a window call.
    pub fn has_background_work(&self) -> Result<bool, CoordinatorError> {
        Ok(self.list()?.iter().any(|run| {
            matches!(
                run.state,
                RunState::Ready | RunState::Running | RunState::Pausing | RunState::Cancelling
            )
        }))
    }

    pub fn get(&self, run_id: &str) -> Result<CoordinatorSnapshot, CoordinatorError> {
        self.journal
            .get(run_id)
            .map_err(map_journal)?
            .ok_or(CoordinatorError::UnknownRun)
    }

    pub fn pause(&self, run_id: &str, now: u64) -> Result<CoordinatorSnapshot, CoordinatorError> {
        let run = self.get(run_id)?;
        if matches!(run.state, RunState::Paused | RunState::Pausing) {
            return Ok(run);
        }
        if run.state != RunState::Running {
            return Err(CoordinatorError::InvalidTransition);
        }
        let next = self
            .journal
            .set_run_state(run_id, run.revision, RunState::Pausing, None, now)
            .map_err(map_journal)?;
        if !next
            .steps
            .iter()
            .any(|step| matches!(step.state, StepState::Starting | StepState::Running))
        {
            return self
                .journal
                .set_run_state(run_id, next.revision, RunState::Paused, None, now)
                .map_err(map_journal);
        }
        Ok(next)
    }

    pub fn resume(&self, run_id: &str, now: u64) -> Result<CoordinatorSnapshot, CoordinatorError> {
        let run = self.get(run_id)?;
        if run.state == RunState::Running {
            return Ok(run);
        }
        if run.state != RunState::Paused {
            return Err(CoordinatorError::InvalidTransition);
        }
        if !self
            .plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(run_id)
        {
            return Err(CoordinatorError::InvalidTransition);
        }
        self.journal
            .set_run_state(run_id, run.revision, RunState::Running, None, now)
            .map_err(map_journal)
    }

    pub fn stop(&self, run_id: &str, now: u64) -> Result<CoordinatorSnapshot, CoordinatorError> {
        let run = self.get(run_id)?;
        if run.state.terminal() || run.state == RunState::NeedsReview {
            return Ok(run);
        }
        let plan = self
            .plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(run_id)
            .map(|p| p.plan.clone())
            .ok_or(CoordinatorError::InvalidTransition)?;
        if run
            .steps
            .iter()
            .any(|step| step.state == StepState::NeedsReview)
        {
            return Ok(run);
        }
        if run.state != RunState::Cancelling {
            self.journal
                .set_run_state(run_id, run.revision, RunState::Cancelling, None, now)
                .map_err(map_journal)?;
        }
        let run = self.get(run_id)?;
        if let Some(active) = run
            .steps
            .iter()
            .find(|step| matches!(step.state, StepState::Starting | StepState::Running))
        {
            if plan.limits.stop_policy == StopPolicy::CancelCurrentManagedTurn
                && !self.request_cancel(run_id, active, now)?
            {
                if active.attempt_id.as_deref().is_some() {
                    self.journal
                        .set_turn_observation(
                            run_id,
                            &active.step_id,
                            active.attempt_id.as_deref().unwrap_or_default(),
                            active.turn_revision.unwrap_or(1),
                            StepState::NeedsReview,
                            Some(FailureCode::CancelUnconfirmed),
                            now,
                        )
                        .map_err(map_journal)?;
                }
                let current = self.get(run_id)?;
                if current.state != RunState::NeedsReview {
                    return self
                        .journal
                        .set_run_state(
                            run_id,
                            current.revision,
                            RunState::NeedsReview,
                            Some(FailureCode::CancelUnconfirmed),
                            now,
                        )
                        .map_err(map_journal);
                }
                return Ok(current);
            }
            return self.get(run_id);
        }
        self.cancel_unstarted(run_id, now)
    }

    pub fn verify_step(
        &self,
        run_id: &str,
        step_id: &str,
        reviewed_workspace_revision: &str,
        now: u64,
    ) -> Result<CoordinatorSnapshot, CoordinatorError> {
        let plan = self
            .plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(run_id)
            .map(|p| p.plan.clone())
            .ok_or(CoordinatorError::InvalidTransition)?;
        let actual = workspace_revision(std::path::Path::new(&plan.workspace_root))
            .map_err(|_| CoordinatorError::WorkspaceConflict)?;
        if actual != reviewed_workspace_revision {
            return Err(CoordinatorError::WorkspaceConflict);
        }
        self.journal
            .verify_step(run_id, step_id, &actual, now)
            .map_err(map_journal)?;
        let run = self.get(run_id)?;
        if run.state == RunState::NeedsReview
            && !run
                .steps
                .iter()
                .any(|step| step.state == StepState::NeedsReview)
        {
            self.journal
                .set_run_state(run_id, run.revision, RunState::Running, None, now)
                .map_err(map_journal)
        } else if run.state == RunState::Pausing
            && !run.steps.iter().any(|step| {
                matches!(
                    step.state,
                    StepState::Starting | StepState::Running | StepState::Verifying
                )
            })
        {
            self.journal
                .set_run_state(run_id, run.revision, RunState::Paused, None, now)
                .map_err(map_journal)
        } else if run.state == RunState::Cancelling {
            self.cancel_unstarted(run_id, now)
        } else {
            Ok(run)
        }
    }

    /// Reconciles a restart/transport ambiguity only by explicit owner evidence. `completed`
    /// moves to review of the live workspace; `failed` closes the run without retry; `unknown`
    /// keeps the workspace claim and the visible needs-review state.
    pub fn reconcile(
        &self,
        run_id: &str,
        step_id: &str,
        decision: ReconcileDecision,
        workspace_root: Option<&str>,
        reviewed_workspace_revision: Option<&str>,
        now: u64,
    ) -> Result<CoordinatorSnapshot, CoordinatorError> {
        let run = self.get(run_id)?;
        if run.state != RunState::NeedsReview {
            return Err(CoordinatorError::InvalidTransition);
        }
        let step = run
            .steps
            .iter()
            .find(|step| step.step_id == step_id)
            .ok_or(CoordinatorError::UnknownRun)?;
        if step.state != StepState::NeedsReview {
            return Err(CoordinatorError::InvalidTransition);
        }
        match decision {
            ReconcileDecision::Unknown => Ok(run),
            ReconcileDecision::ConfirmedCompleted => {
                if step.attempt_id.is_none() {
                    return Err(CoordinatorError::InvalidTransition);
                }
                let plan = self
                    .plans
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(run_id)
                    .map(|p| p.plan.clone());
                let expected = reviewed_workspace_revision.ok_or(CoordinatorError::ReviewStale)?;
                let root = plan
                    .as_ref()
                    .map(|plan| plan.workspace_root.as_str())
                    .or(workspace_root)
                    .ok_or(CoordinatorError::WorkspaceConflict)?;
                if plan.is_none() && workspace_root.is_none() {
                    return Err(CoordinatorError::WorkspaceConflict);
                }
                let root = std::path::Path::new(root);
                let identity =
                    workspace_identity(root).map_err(|_| CoordinatorError::WorkspaceConflict)?;
                let actual =
                    workspace_revision(root).map_err(|_| CoordinatorError::WorkspaceConflict)?;
                if identity != run.workspace_id || actual != expected {
                    return Err(CoordinatorError::WorkspaceConflict);
                }
                self.journal
                    .set_step_state(
                        run_id,
                        step_id,
                        StepState::NeedsReview,
                        StepState::Verifying,
                        Some(FailureCode::OwnerReviewRequired),
                        now,
                    )
                    .map_err(map_journal)?;
                self.journal
                    .verify_step(run_id, step_id, &actual, now)
                    .map_err(map_journal)?;
                self.close_reconciled_run(run_id, now)
            }
            ReconcileDecision::ConfirmedNoEffect => {
                self.journal
                    .set_step_state(
                        run_id,
                        step_id,
                        StepState::NeedsReview,
                        StepState::Failed,
                        Some(FailureCode::OwnerReviewRequired),
                        now,
                    )
                    .map_err(map_journal)?;
                self.close_reconciled_run(run_id, now)
            }
            ReconcileDecision::ConfirmedCancelled => {
                self.journal
                    .set_step_state(
                        run_id,
                        step_id,
                        StepState::NeedsReview,
                        StepState::Cancelled,
                        Some(FailureCode::OwnerReviewRequired),
                        now,
                    )
                    .map_err(map_journal)?;
                self.close_reconciled_run(run_id, now)
            }
        }
    }

    fn close_reconciled_run(
        &self,
        run_id: &str,
        now: u64,
    ) -> Result<CoordinatorSnapshot, CoordinatorError> {
        let run = self.get(run_id)?;
        if run
            .steps
            .iter()
            .any(|step| step.state == StepState::NeedsReview)
        {
            return Ok(run);
        }
        let has_live_plan = self
            .plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(run_id);
        let required_failed = run.steps.iter().any(|step| {
            step.required && matches!(step.state, StepState::Failed | StepState::Blocked)
        });
        let required_cancelled = run
            .steps
            .iter()
            .any(|step| step.required && step.state == StepState::Cancelled);
        let next = if required_failed {
            RunState::Failed
        } else if required_cancelled {
            RunState::Cancelled
        } else if has_live_plan {
            // The in-memory immutable plan still exists, so the owner resolved each ambiguous
            // effect and the ordinary dependency scheduler may continue.
            RunState::Running
        } else {
            // After a process crash the prompts are intentionally gone. Close only the run
            // whose effects the owner has reconciled; never reconstruct or redispatch it.
            RunState::Completed
        };
        let failure = matches!(next, RunState::Failed | RunState::Cancelled)
            .then_some(FailureCode::OwnerReviewRequired);
        self.journal
            .set_run_state(run_id, run.revision, next, failure, now)
            .map_err(map_journal)
    }

    fn request_cancel(
        &self,
        run_id: &str,
        step: &StepSnapshot,
        now: u64,
    ) -> Result<bool, CoordinatorError> {
        let attempt_id = step
            .attempt_id
            .as_deref()
            .ok_or(CoordinatorError::InvalidTransition)?;
        let intent_id = Uuid::new_v4().to_string();
        match self
            .journal
            .begin_cancellation(run_id, &step.step_id, attempt_id, &intent_id, now)
            .map_err(map_journal)?
        {
            CancellationOutcome::Acknowledged => return Ok(true),
            CancellationOutcome::Refused | CancellationOutcome::Unknown => return Ok(false),
            CancellationOutcome::Requested => (),
        }
        match self.authority.cancel(step, &intent_id) {
            Ok(()) => {
                self.journal
                    .finish_cancellation(
                        run_id,
                        &step.step_id,
                        attempt_id,
                        CancellationOutcome::Acknowledged,
                        now,
                    )
                    .map_err(map_journal)?;
                self.started
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&(run_id.to_owned(), step.step_id.clone()));
                Ok(true)
            }
            Err(BridgeFault::Denied | BridgeFault::Busy) => {
                self.journal
                    .finish_cancellation(
                        run_id,
                        &step.step_id,
                        attempt_id,
                        CancellationOutcome::Refused,
                        now,
                    )
                    .map_err(map_journal)?;
                Ok(false)
            }
            Err(BridgeFault::Unavailable | BridgeFault::Unknown) => {
                self.journal
                    .finish_cancellation(
                        run_id,
                        &step.step_id,
                        attempt_id,
                        CancellationOutcome::Unknown,
                        now,
                    )
                    .map_err(map_journal)?;
                Ok(false)
            }
        }
    }

    /// Performs one bounded scheduling observation. Called from the headless daemon loop.
    pub fn tick(&self, now: u64) -> Result<(), CoordinatorError> {
        let runs = self.journal.list(128).map_err(map_journal)?;
        if runs.is_empty() {
            return Ok(());
        }
        let mut cursor = self
            .round_robin
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let start = *cursor % runs.len();
        *cursor = cursor.wrapping_add(1);
        let mut ordered: Vec<_> = (0..runs.len())
            .map(|offset| &runs[(start + offset) % runs.len()])
            .collect();
        ordered.retain(|run| {
            self.plans
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains_key(&run.run_id)
        });
        // Exactly one current Managed turn is allowed globally. Poll it before considering a
        // new dispatch; another run can never race it into the authority.
        if let Some((run, step)) = ordered.iter().find_map(|run| {
            run.steps
                .iter()
                .find(|s| matches!(s.state, StepState::Starting | StepState::Running))
                .map(|step| (*run, step))
        }) {
            if now >= plan_deadline(&self.plans, &run.run_id).unwrap_or(0) {
                return self.expire_active(run, step, now);
            }
            return self.poll_active(run, step, now);
        }
        // A completed turn is not an approved workspace revision. Do not run another step
        // against a workspace whose prior changes the owner has not reviewed yet.
        if ordered.iter().any(|run| {
            run.steps
                .iter()
                .any(|step| step.state == StepState::Verifying)
        }) {
            return Ok(());
        }
        let mut dispatched = false;
        for run in ordered {
            if !matches!(
                run.state,
                RunState::Running | RunState::Pausing | RunState::Cancelling
            ) {
                continue;
            }
            if now >= plan_deadline(&self.plans, &run.run_id).unwrap_or(0) {
                let fresh = self.get(&run.run_id)?;
                if fresh.state == RunState::Cancelling {
                    self.cancel_unstarted(&fresh.run_id, now)?;
                    continue;
                }
                for step in &fresh.steps {
                    if !step.state.terminal() {
                        self.journal
                            .set_step_state(
                                &fresh.run_id,
                                &step.step_id,
                                step.state,
                                StepState::Cancelled,
                                Some(FailureCode::PlanExpired),
                                now,
                            )
                            .map_err(map_journal)?;
                    }
                }
                let fresh = self.get(&run.run_id)?;
                if matches!(fresh.state, RunState::Running | RunState::Pausing) {
                    self.journal
                        .set_run_state(
                            &fresh.run_id,
                            fresh.revision,
                            RunState::Expired,
                            Some(FailureCode::PlanExpired),
                            now,
                        )
                        .map_err(map_journal)?;
                }
                continue;
            }
            self.update_dependencies(&run.run_id, now)?;
            self.stop_after_mandatory_failure(&run.run_id, now)?;
            let run = self.get(&run.run_id)?;
            if run.state == RunState::Pausing {
                if !run.steps.iter().any(|step| {
                    matches!(
                        step.state,
                        StepState::Starting | StepState::Running | StepState::Verifying
                    )
                }) {
                    self.journal
                        .set_run_state(&run.run_id, run.revision, RunState::Paused, None, now)
                        .map_err(map_journal)?;
                }
                continue;
            }
            if run.state == RunState::Cancelling {
                self.cancel_unstarted(&run.run_id, now)?;
                continue;
            }
            self.finish_if_done(&run.run_id, now)?;
            if dispatched {
                continue;
            }
            let run = self.get(&run.run_id)?;
            let Some(plan) = self
                .plans
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&run.run_id)
                .map(|p| p.plan.clone())
            else {
                continue;
            };
            for step in &plan.steps {
                let Some(row) = run.steps.iter().find(|row| row.step_id == step.step_id) else {
                    continue;
                };
                if row.state != StepState::Eligible && row.state != StepState::WaitingResource {
                    continue;
                }
                if !matches!(step.provider, ProviderLane::ManagedCodex)
                    || !matches!(
                        step.effect,
                        EffectClass::ManagedInference | EffectClass::ManagedLocalTools
                    )
                {
                    continue;
                }
                let admission = match self.authority.inspect(
                    step.managed_session_id.as_deref().unwrap_or(""),
                    step.effect,
                    step.model_id.as_deref(),
                ) {
                    Ok(admission) => admission,
                    Err(BridgeFault::Busy) => {
                        if row.state == StepState::Eligible {
                            self.journal
                                .set_step_state(
                                    &run.run_id,
                                    &step.step_id,
                                    row.state,
                                    StepState::WaitingResource,
                                    Some(FailureCode::ResourceBusy),
                                    now,
                                )
                                .map_err(map_journal)?;
                        }
                        continue;
                    }
                    Err(BridgeFault::Denied) => {
                        self.journal
                            .set_step_state(
                                &run.run_id,
                                &step.step_id,
                                row.state,
                                StepState::Blocked,
                                Some(FailureCode::PermissionDenied),
                                now,
                            )
                            .map_err(map_journal)?;
                        continue;
                    }
                    Err(BridgeFault::Unavailable) => {
                        if row.state == StepState::Eligible {
                            self.journal
                                .set_step_state(
                                    &run.run_id,
                                    &step.step_id,
                                    row.state,
                                    StepState::WaitingResource,
                                    Some(FailureCode::AuthorityUnavailable),
                                    now,
                                )
                                .map_err(map_journal)?;
                        }
                        continue;
                    }
                    Err(BridgeFault::Unknown) => {
                        self.journal
                            .set_step_state(
                                &run.run_id,
                                &step.step_id,
                                row.state,
                                StepState::NeedsReview,
                                Some(FailureCode::AuthorityUnavailable),
                                now,
                            )
                            .map_err(map_journal)?;
                        continue;
                    }
                };
                let (identity, current_revision) = match (
                    workspace_identity(&admission.workspace_root),
                    workspace_revision(&admission.workspace_root),
                ) {
                    (Ok(identity), Ok(revision)) => (identity, revision),
                    _ => {
                        self.journal
                            .set_step_state(
                                &run.run_id,
                                &step.step_id,
                                row.state,
                                StepState::NeedsReview,
                                Some(FailureCode::WorkspaceConflict),
                                now,
                            )
                            .map_err(map_journal)?;
                        continue;
                    }
                };
                if identity != plan.workspace_id
                    || identity != run.workspace_id
                    || current_revision != run.current_workspace_revision
                    || identity != step_workspace_identity(&plan)
                {
                    self.journal
                        .set_step_state(
                            &run.run_id,
                            &step.step_id,
                            row.state,
                            StepState::NeedsReview,
                            Some(FailureCode::WorkspaceConflict),
                            now,
                        )
                        .map_err(map_journal)?;
                    continue;
                }
                let attempt_id = Uuid::new_v4().to_string();
                let intent_id = Uuid::new_v4().to_string();
                self.journal
                    .begin_attempt(
                        &run.run_id,
                        &step.step_id,
                        &attempt_id,
                        &intent_id,
                        &admission.authority_id,
                        &admission.runtime_id,
                        admission.generation,
                        now,
                    )
                    .map_err(map_journal)?;
                match self.authority.start(&admission, &intent_id, &step.prompt) {
                    Ok(started) => {
                        self.journal
                            .acknowledge_attempt(
                                &run.run_id,
                                &step.step_id,
                                &attempt_id,
                                &started.turn_id,
                                started.revision,
                                now,
                            )
                            .map_err(map_journal)?;
                        self.started
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .insert((run.run_id.clone(), step.step_id.clone()), Instant::now());
                        dispatched = true;
                        break;
                    }
                    Err(BridgeFault::Busy) => {
                        self.journal
                            .refuse_attempt(
                                &run.run_id,
                                &step.step_id,
                                &attempt_id,
                                FailureCode::ResourceBusy,
                                now,
                            )
                            .map_err(map_journal)?;
                    }
                    Err(BridgeFault::Denied | BridgeFault::Unavailable) => {
                        self.journal
                            .refuse_attempt(
                                &run.run_id,
                                &step.step_id,
                                &attempt_id,
                                FailureCode::PermissionDenied,
                                now,
                            )
                            .map_err(map_journal)?;
                    }
                    Err(BridgeFault::Unknown) => {
                        self.journal
                            .mark_attempt_unknown(&run.run_id, &step.step_id, &attempt_id, now)
                            .map_err(map_journal)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn poll_active(
        &self,
        run: &CoordinatorSnapshot,
        step: &StepSnapshot,
        now: u64,
    ) -> Result<(), CoordinatorError> {
        let attempt = step
            .attempt_id
            .as_deref()
            .ok_or(CoordinatorError::JournalUnavailable)?;
        let Some(turn) = step.managed_turn_id.as_deref() else {
            self.journal
                .mark_attempt_unknown(&run.run_id, &step.step_id, attempt, now)
                .map_err(map_journal)?;
            return Ok(());
        };
        let Some(plan) = self
            .plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&run.run_id)
            .map(|p| p.plan.clone())
        else {
            return Ok(());
        };
        let step_plan = plan
            .steps
            .iter()
            .find(|candidate| candidate.step_id == step.step_id)
            .ok_or(CoordinatorError::UnknownRun)?;
        let timed_out = self
            .started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&(run.run_id.clone(), step.step_id.clone()))
            .is_some_and(|started| {
                started.elapsed() > Duration::from_secs(u64::from(step_plan.max_duration_seconds))
            });
        if timed_out && !self.request_cancel(&run.run_id, step, now)? {
            self.journal
                .set_turn_observation(
                    &run.run_id,
                    &step.step_id,
                    attempt,
                    step.turn_revision.unwrap_or(1),
                    StepState::NeedsReview,
                    Some(FailureCode::CancelUnconfirmed),
                    now,
                )
                .map_err(map_journal)?;
            return Ok(());
        }
        let observation = match self.authority.observe(turn) {
            Ok(observation) => observation,
            Err(BridgeFault::Busy | BridgeFault::Unavailable | BridgeFault::Unknown) => {
                self.journal
                    .set_turn_observation(
                        &run.run_id,
                        &step.step_id,
                        attempt,
                        step.turn_revision.unwrap_or(1),
                        StepState::NeedsReview,
                        Some(FailureCode::DispatchUnknown),
                        now,
                    )
                    .map_err(map_journal)?;
                return Ok(());
            }
            Err(BridgeFault::Denied) => {
                self.journal
                    .set_turn_observation(
                        &run.run_id,
                        &step.step_id,
                        attempt,
                        step.turn_revision.unwrap_or(1),
                        StepState::NeedsReview,
                        Some(FailureCode::ManagedSessionUnavailable),
                        now,
                    )
                    .map_err(map_journal)?;
                return Ok(());
            }
        };
        match observation.state {
            TurnState::Starting | TurnState::Running => {
                if observation.revision > step.turn_revision.unwrap_or(0) {
                    self.journal
                        .set_turn_observation(
                            &run.run_id,
                            &step.step_id,
                            attempt,
                            observation.revision,
                            StepState::Running,
                            None,
                            now,
                        )
                        .map_err(map_journal)?;
                }
            }
            TurnState::Completed => {
                self.journal
                    .set_turn_observation(
                        &run.run_id,
                        &step.step_id,
                        attempt,
                        observation.revision,
                        StepState::Verifying,
                        None,
                        now,
                    )
                    .map_err(map_journal)?;
                self.started
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&(run.run_id.clone(), step.step_id.clone()));
            }
            TurnState::Interrupted
                if run.state == RunState::Cancelling
                    && observation.reason == Some(TurnReason::UserCancel) =>
            {
                self.journal
                    .set_turn_observation(
                        &run.run_id,
                        &step.step_id,
                        attempt,
                        observation.revision,
                        StepState::Cancelled,
                        None,
                        now,
                    )
                    .map_err(map_journal)?;
                self.started
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&(run.run_id.clone(), step.step_id.clone()));
                self.cancel_unstarted(&run.run_id, now)?;
            }
            TurnState::Interrupted | TurnState::Failed => {
                self.journal
                    .set_turn_observation(
                        &run.run_id,
                        &step.step_id,
                        attempt,
                        observation.revision,
                        StepState::Failed,
                        Some(FailureCode::ProviderFailed),
                        now,
                    )
                    .map_err(map_journal)?;
                self.started
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&(run.run_id.clone(), step.step_id.clone()));
                self.update_dependencies(&run.run_id, now)?;
                self.stop_after_mandatory_failure(&run.run_id, now)?;
                self.finish_if_done(&run.run_id, now)?;
            }
            TurnState::Indeterminate => {
                self.journal
                    .set_turn_observation(
                        &run.run_id,
                        &step.step_id,
                        attempt,
                        observation.revision,
                        StepState::NeedsReview,
                        Some(FailureCode::DispatchUnknown),
                        now,
                    )
                    .map_err(map_journal)?;
            }
        }
        Ok(())
    }

    fn expire_active(
        &self,
        run: &CoordinatorSnapshot,
        step: &StepSnapshot,
        now: u64,
    ) -> Result<(), CoordinatorError> {
        let current = self.get(&run.run_id)?;
        if current.state != RunState::Cancelling {
            self.journal
                .set_run_state(
                    &current.run_id,
                    current.revision,
                    RunState::Cancelling,
                    Some(FailureCode::PlanExpired),
                    now,
                )
                .map_err(map_journal)?;
        }
        let current = self.get(&run.run_id)?;
        let active = current
            .steps
            .iter()
            .find(|candidate| candidate.step_id == step.step_id)
            .ok_or(CoordinatorError::UnknownRun)?;
        if !self.request_cancel(&run.run_id, active, now)? {
            let attempt_id = active
                .attempt_id
                .as_deref()
                .ok_or(CoordinatorError::JournalUnavailable)?;
            self.journal
                .set_turn_observation(
                    &run.run_id,
                    &active.step_id,
                    attempt_id,
                    active.turn_revision.unwrap_or(1),
                    StepState::NeedsReview,
                    Some(FailureCode::CancelUnconfirmed),
                    now,
                )
                .map_err(map_journal)?;
            return Ok(());
        }
        self.poll_active(&current, active, now)
    }

    fn update_dependencies(&self, run_id: &str, now: u64) -> Result<(), CoordinatorError> {
        let run = self.get(run_id)?;
        for step in &run.steps {
            if step.state != StepState::WaitingDependencies {
                continue;
            }
            let mut wait = false;
            let mut failed = false;
            for dep in &step.dependencies {
                let parent = run
                    .steps
                    .iter()
                    .find(|row| row.step_id == dep.step_id)
                    .ok_or(CoordinatorError::InvalidPlan)?;
                match parent.state {
                    StepState::Succeeded => {}
                    StepState::Failed
                    | StepState::Blocked
                    | StepState::Cancelled
                    | StepState::Skipped
                        if dep.allow_missing_output => {}
                    StepState::Failed
                    | StepState::Blocked
                    | StepState::Cancelled
                    | StepState::Skipped => failed = true,
                    StepState::NeedsReview => return Ok(()),
                    _ => wait = true,
                }
            }
            if failed {
                self.journal
                    .set_step_state(
                        run_id,
                        &step.step_id,
                        StepState::WaitingDependencies,
                        StepState::Blocked,
                        Some(FailureCode::DependencyFailed),
                        now,
                    )
                    .map_err(map_journal)?;
            } else if !wait {
                self.journal
                    .set_step_state(
                        run_id,
                        &step.step_id,
                        StepState::WaitingDependencies,
                        StepState::Eligible,
                        None,
                        now,
                    )
                    .map_err(map_journal)?;
            }
        }
        Ok(())
    }

    /// A required failure halts the remainder of the immutable pipeline. Direct dependents
    /// already carry DEPENDENCY_FAILED; unrelated queued work is explicitly cancelled and
    /// never dispatched after the required failure.
    fn stop_after_mandatory_failure(&self, run_id: &str, now: u64) -> Result<(), CoordinatorError> {
        let run = self.get(run_id)?;
        if !run.steps.iter().any(|step| {
            step.required && matches!(step.state, StepState::Failed | StepState::Blocked)
        }) {
            return Ok(());
        }
        for step in &run.steps {
            if matches!(
                step.state,
                StepState::Proposed
                    | StepState::WaitingDependencies
                    | StepState::Eligible
                    | StepState::WaitingResource
                    | StepState::AwaitingPermission
                    | StepState::Queued
            ) {
                let next = if step.state == StepState::WaitingDependencies {
                    StepState::Blocked
                } else {
                    StepState::Cancelled
                };
                self.journal
                    .set_step_state(
                        run_id,
                        &step.step_id,
                        step.state,
                        next,
                        (next == StepState::Blocked).then_some(FailureCode::DependencyFailed),
                        now,
                    )
                    .map_err(map_journal)?;
            }
        }
        Ok(())
    }

    fn finish_if_done(&self, run_id: &str, now: u64) -> Result<(), CoordinatorError> {
        let run = self.get(run_id)?;
        if run.steps.iter().any(|step| !step.state.terminal()) {
            return Ok(());
        }
        let state = if run.steps.iter().any(|step| {
            step.required
                && matches!(
                    step.state,
                    StepState::Failed | StepState::Blocked | StepState::Cancelled
                )
        }) {
            if run
                .steps
                .iter()
                .all(|step| matches!(step.state, StepState::Blocked | StepState::Skipped))
            {
                RunState::Blocked
            } else {
                RunState::Failed
            }
        } else {
            RunState::Completed
        };
        if run.state != state {
            self.journal
                .set_run_state(run_id, run.revision, state, None, now)
                .map_err(map_journal)?;
        }
        Ok(())
    }

    fn cancel_unstarted(
        &self,
        run_id: &str,
        now: u64,
    ) -> Result<CoordinatorSnapshot, CoordinatorError> {
        let run = self.get(run_id)?;
        for step in &run.steps {
            if matches!(
                step.state,
                StepState::Proposed
                    | StepState::WaitingDependencies
                    | StepState::Eligible
                    | StepState::WaitingResource
                    | StepState::AwaitingPermission
                    | StepState::Queued
                    | StepState::Verifying
            ) {
                self.journal
                    .set_step_state(
                        run_id,
                        &step.step_id,
                        step.state,
                        StepState::Cancelled,
                        None,
                        now,
                    )
                    .map_err(map_journal)?;
            }
        }
        let run = self.get(run_id)?;
        if !run.steps.iter().any(|step| {
            matches!(
                step.state,
                StepState::Starting | StepState::Running | StepState::Verifying
            )
        }) {
            let expired = run.failure == Some(FailureCode::PlanExpired);
            self.journal
                .set_run_state(
                    run_id,
                    run.revision,
                    if expired {
                        RunState::Expired
                    } else {
                        RunState::Cancelled
                    },
                    expired.then_some(FailureCode::PlanExpired),
                    now,
                )
                .map_err(map_journal)
        } else {
            Ok(run)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileDecision {
    Unknown,
    ConfirmedCompleted,
    ConfirmedNoEffect,
    ConfirmedCancelled,
}

fn plan_deadline(plans: &Mutex<HashMap<String, ActivePlan>>, run_id: &str) -> Option<u64> {
    plans
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(run_id)
        .map(|active| {
            active
                .plan
                .expires_at_unix
                .min(active.plan.created_at_unix.saturating_add(u64::from(
                    active.plan.limits.max_total_seconds.min(MAX_RUN_SECONDS),
                )))
        })
}
fn step_workspace_identity(plan: &OrchestrationPlanV1) -> String {
    plan.workspace_id.clone()
}
fn map_journal(error: super::JournalError) -> CoordinatorError {
    match error {
        super::JournalError::Invalid => CoordinatorError::InvalidPlan,
        super::JournalError::NotFound => CoordinatorError::UnknownRun,
        super::JournalError::Stale => CoordinatorError::ReviewStale,
        super::JournalError::InvalidTransition => CoordinatorError::InvalidTransition,
        super::JournalError::WorkspaceConflict => CoordinatorError::WorkspaceConflict,
        super::JournalError::Capacity => CoordinatorError::ResourceBusy,
        super::JournalError::Storage | super::JournalError::Incompatible => {
            CoordinatorError::JournalUnavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::{DependencyV1, PlanLimitsV1, PlanReviewV1, StopPolicy};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    const SESSION: &str = "11111111-1111-4111-8111-111111111111";
    const SECOND_SESSION: &str = "12121212-1212-4212-8212-121212121212";
    const AUTHORITY: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";
    const RUNTIME: &str = "99999999-9999-4999-8999-999999999999";
    const TURN: &str = "88888888-8888-4888-8888-888888888888";

    struct FakeAuthority {
        root: PathBuf,
        second_root: Mutex<Option<PathBuf>>,
        starts: AtomicUsize,
        cancels: AtomicUsize,
        start_fault: Mutex<Option<BridgeFault>>,
        observation: Mutex<TurnObservation>,
    }
    impl FakeAuthority {
        fn new(root: PathBuf) -> Self {
            Self {
                root,
                second_root: Mutex::new(None),
                starts: AtomicUsize::new(0),
                cancels: AtomicUsize::new(0),
                start_fault: Mutex::new(None),
                observation: Mutex::new(TurnObservation {
                    state: TurnState::Running,
                    reason: None,
                    revision: 1,
                }),
            }
        }
    }
    impl ManagedAuthority for FakeAuthority {
        fn inspect(
            &self,
            session_id: &str,
            _effect: EffectClass,
            _model: Option<&str>,
        ) -> Result<Admission, BridgeFault> {
            let workspace_root = if session_id == SECOND_SESSION {
                self.second_root
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or(BridgeFault::Unavailable)?
            } else {
                self.root.clone()
            };
            Ok(Admission {
                authority_id: AUTHORITY.into(),
                session_id: session_id.into(),
                runtime_id: RUNTIME.into(),
                generation: 1,
                session_revision: 1,
                workspace_root,
            })
        }
        fn start(
            &self,
            _admission: &Admission,
            _intent_id: &str,
            _prompt: &str,
        ) -> Result<StartedTurn, BridgeFault> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            if let Some(fault) = *self.start_fault.lock().unwrap() {
                return Err(fault);
            }
            Ok(StartedTurn {
                turn_id: TURN.into(),
                revision: 1,
            })
        }
        fn observe(&self, _turn_id: &str) -> Result<TurnObservation, BridgeFault> {
            Ok(*self.observation.lock().unwrap())
        }
        fn cancel(&self, _step: &StepSnapshot, _intent_id: &str) -> Result<(), BridgeFault> {
            self.cancels.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn root() -> PathBuf {
        let path = std::env::temp_dir().join(format!("wyss-orch-scheduler-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn step(id: &str, dependencies: Vec<DependencyV1>) -> StepPlanV1 {
        StepPlanV1 {
            step_id: id.into(),
            dependencies,
            required: true,
            provider: ProviderLane::ManagedCodex,
            effect: EffectClass::ManagedInference,
            managed_session_id: Some(SESSION.into()),
            model_id: None,
            prompt: "fixture prompt remains transient".into(),
            max_attempts: 1,
            max_duration_seconds: 60,
        }
    }

    fn plan(root: &Path, steps: Vec<StepPlanV1>, plan_id: &str) -> OrchestrationPlanV1 {
        OrchestrationPlanV1 {
            schema_version: 1,
            plan_id: plan_id.into(),
            revision: 1,
            host_id: "local".into(),
            project_id: "test-project".into(),
            workspace_id: workspace_identity(root).unwrap(),
            workspace_root: root.to_string_lossy().into_owned(),
            workspace_revision: workspace_revision(root).unwrap(),
            objective: "fixture only".into(),
            created_at_unix: 100,
            expires_at_unix: 400,
            limits: PlanLimitsV1 {
                max_steps: 16,
                max_parallelism: 1,
                max_total_seconds: 300,
                stop_policy: StopPolicy::CancelCurrentManagedTurn,
            },
            steps,
        }
    }

    fn review(plan: &OrchestrationPlanV1) -> PlanReviewV1 {
        PlanReviewV1 {
            plan_id: plan.plan_id.clone(),
            revision: plan.revision,
            digest: plan.digest().unwrap(),
        }
    }

    fn coordinator(root: &Path) -> (Coordinator, Arc<RunJournal>, Arc<FakeAuthority>) {
        let journal = Arc::new(RunJournal::open_in_memory().unwrap());
        let authority = Arc::new(FakeAuthority::new(root.to_path_buf()));
        (
            Coordinator::new(journal.clone(), authority.clone(), 99).unwrap(),
            journal,
            authority,
        )
    }

    #[test]
    fn dependency_failure_blocks_descendants_and_halts_remaining_dispatch() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let first = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let second = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        let draft = plan(
            &root,
            vec![
                step(first, vec![]),
                step(
                    second,
                    vec![DependencyV1 {
                        step_id: first.into(),
                        allow_missing_output: false,
                    }],
                ),
            ],
            "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        );
        let accepted = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        *authority.observation.lock().unwrap() = TurnObservation {
            state: TurnState::Failed,
            reason: Some(TurnReason::ProviderFailed),
            revision: 2,
        };
        coordinator.tick(102).unwrap();
        let final_run = coordinator.get(&accepted.run_id).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        assert_eq!(final_run.steps[0].state, StepState::Failed);
        assert_eq!(final_run.steps[1].state, StepState::Blocked);
        assert_eq!(final_run.state, RunState::Failed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ambiguous_start_is_journaled_and_never_retried() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        *authority.start_fault.lock().unwrap() = Some(BridgeFault::Unknown);
        let draft = plan(
            &root,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let run = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        coordinator.tick(102).unwrap();
        let after = coordinator.get(&run.run_id).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        assert_eq!(after.state, RunState::NeedsReview);
        assert_eq!(after.steps[0].state, StepState::NeedsReview);
        assert_eq!(after.steps[0].attempt_count, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reviewed_workspace_change_and_duplicate_workspace_claim_are_refused() {
        let root = root();
        let (coordinator, journal, _authority) = coordinator(&root);
        let draft = plan(
            &root,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let approved = review(&draft);
        fs::write(root.join("changed-after-review"), b"change").unwrap();
        assert_eq!(
            coordinator
                .submit(draft.clone(), approved.clone(), 100)
                .unwrap_err(),
            CoordinatorError::WorkspaceConflict
        );
        fs::remove_file(root.join("changed-after-review")).unwrap();
        journal.create_run(&draft, &approved, 100).unwrap();
        let second = plan(
            &root,
            vec![step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![])],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        assert_eq!(
            journal
                .create_run(&second, &review(&second), 100)
                .unwrap_err(),
            super::super::JournalError::WorkspaceConflict
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancellation_is_an_idempotent_durable_intent() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let draft = plan(
            &root,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let run = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        *authority.observation.lock().unwrap() = TurnObservation {
            state: TurnState::Interrupted,
            reason: Some(TurnReason::UserCancel),
            revision: 2,
        };
        coordinator.stop(&run.run_id, 102).unwrap();
        coordinator.stop(&run.run_id, 103).unwrap();
        assert_eq!(authority.cancels.load(Ordering::SeqCst), 1);
        coordinator.tick(104).unwrap();
        let final_run = coordinator.get(&run.run_id).unwrap();
        assert_eq!(final_run.state, RunState::Cancelled);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn production_dispatch_cap_stays_one_across_distinct_workspaces() {
        let root_a = root();
        let root_b = root();
        let (coordinator, _journal, authority) = coordinator(&root_a);
        *authority.second_root.lock().unwrap() = Some(root_b.clone());

        let first = plan(
            &root_a,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let mut second_step = step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![]);
        second_step.managed_session_id = Some(SECOND_SESSION.into());
        let second = plan(
            &root_b,
            vec![second_step],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        coordinator
            .submit(first.clone(), review(&first), 100)
            .unwrap();
        coordinator
            .submit(second.clone(), review(&second), 100)
            .unwrap();

        coordinator.tick(101).unwrap();
        coordinator.tick(102).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        let runs = coordinator.list().unwrap();
        assert_eq!(
            runs.iter()
                .flat_map(|run| &run.steps)
                .filter(|step| matches!(step.state, StepState::Starting | StepState::Running))
                .count(),
            1
        );
        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    }

    #[test]
    fn expired_finite_budget_prevents_dispatch() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let mut draft = plan(
            &root,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        draft.expires_at_unix = 200;
        draft.limits.max_total_seconds = 100;
        let accepted = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(200).unwrap();
        let expired = coordinator.get(&accepted.run_id).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        assert_eq!(expired.state, RunState::Expired);
        assert_eq!(expired.failure, Some(FailureCode::PlanExpired));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn total_budget_expiry_cancels_the_active_turn_before_closing() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let mut draft = plan(
            &root,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        draft.expires_at_unix = 102;
        draft.limits.max_total_seconds = 2;
        let accepted = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        *authority.observation.lock().unwrap() = TurnObservation {
            state: TurnState::Interrupted,
            reason: Some(TurnReason::UserCancel),
            revision: 2,
        };
        coordinator.tick(102).unwrap();
        let expired = coordinator.get(&accepted.run_id).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        assert_eq!(authority.cancels.load(Ordering::SeqCst), 1);
        assert_eq!(expired.state, RunState::Expired);
        assert_eq!(expired.failure, Some(FailureCode::PlanExpired));
        assert!(expired.steps.iter().all(|step| step.state.terminal()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_dispatch_stays_explicitly_blocked() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let mut native = step(id, vec![]);
        native.provider = ProviderLane::NativeCodex;
        native.effect = EffectClass::NativeFullAccount;
        native.managed_session_id = None;
        let draft = plan(&root, vec![native], "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
        let run = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        assert_eq!(run.state, RunState::Blocked);
        assert_eq!(run.steps[0].state, StepState::Blocked);
        assert_eq!(
            run.steps[0].failure,
            Some(FailureCode::TrustBoundaryBlocked)
        );
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        fs::remove_dir_all(root).unwrap();
    }
}
