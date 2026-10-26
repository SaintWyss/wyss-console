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
        Sequence, WireUuid,
        control::{
            ActualState, AuditRead, BindingState, ControlError as E, ControlState, Empty,
            ExecutionScope, GateReason, Get, LeaseActive, MAX_PAGE, Operation, ResultValue,
            RuntimeSafetyProfile, RuntimeTarget, TurnCancel, TurnGet, TurnStart,
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
    /// Authority-backed check used only for an explicitly reviewed Managed receiver.
    /// Implementations that cannot prove a complete creation-to-head history fail closed.
    fn verify_fresh_receiver(&self, _session_id: &str) -> Result<(), BridgeFault> {
        Err(BridgeFault::Unknown)
    }
    /// Freshness for Managed source provenance. Native source identity is owned by wyss-host
    /// and is not inferred from this Managed authority.
    fn verify_managed_source(
        &self,
        _session_id: &str,
        _generation: u64,
        _workspace_root: &std::path::Path,
    ) -> Result<(), BridgeFault> {
        Err(BridgeFault::Unknown)
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
        let runtime_root = runtime_root.into();
        Self {
            connector: SocketConnector::new(runtime_root.clone()),
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

    /// Proves a prepared Managed Codex session was created on this authority and has no
    /// durable turn-admission event in its complete bounded creation-to-head history.
    fn verify_fresh_receiver(&self, session_id: &str) -> Result<(), BridgeFault> {
        let snapshot = self.inspect_fresh_receiver_snapshot(session_id);
        require_atomic_fresh_start(snapshot)
    }

    /// Reads all currently supported F8 evidence for the receiver. A clean snapshot is not
    /// sufficient to authorize a later turn/start: F8 has no atomic fresh-only precondition
    /// or consumable preparation receipt, and its session revision does not advance when a
    /// turn is admitted. The caller must keep this admission blocked until that contract
    /// exists. This check is still useful to classify known-used receivers as denied.
    fn inspect_fresh_receiver_snapshot(&self, session_id: &str) -> Result<(), BridgeFault> {
        const MAX_RECEIVER_HISTORY: u64 = 2_000;
        let session = ManagedSessionId::parse(session_id).map_err(|_| BridgeFault::Denied)?;
        let mut client = self.client()?;
        let initial = status_snapshot(&mut client)?;
        let record = session_record(&mut client, session.clone())?;
        if record.control_state != ControlState::Prepared
            || record.runtime_kind != crate::contracts::agent_adapter::AgentRuntimeKind::Codex
        {
            return Err(BridgeFault::Denied);
        }
        let created = record.created_sequence.get();
        let head = initial.last_sequence.map_or(0, Sequence::get);
        if created == 0 || created > head || head.saturating_sub(created) > MAX_RECEIVER_HISTORY {
            return Err(BridgeFault::Unknown);
        }
        let mut cursor = created - 1;
        let mut prepared_receipt_seen = false;
        while cursor < head {
            let after_sequence = if cursor == 0 {
                None
            } else {
                Some(
                    cursor
                        .to_string()
                        .parse::<Sequence>()
                        .map_err(|_| BridgeFault::Unknown)?,
                )
            };
            let answer = client
                .call(
                    Operation::AuditRead(AuditRead {
                        stream_id: initial.stream_id.clone(),
                        after_sequence,
                        limit: MAX_PAGE,
                    }),
                    Target::None,
                )
                .map_err(map_read_fault)?;
            let ResultValue::Audit { events, .. } = answer.value else {
                return Err(BridgeFault::Unknown);
            };
            if events.is_empty() {
                return Err(BridgeFault::Unknown);
            }
            for event in events {
                let value = serde_json::to_value(event).map_err(|_| BridgeFault::Unknown)?;
                let sequence = value["sequence"]
                    .as_str()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or(BridgeFault::Unknown)?;
                let next_cursor = next_receiver_sequence(cursor, sequence, head)?;
                inspect_receiver_event(
                    &value,
                    sequence,
                    created,
                    &session,
                    initial.authority_instance_id.as_str(),
                    initial.stream_id.as_str(),
                    &mut prepared_receipt_seen,
                )?;
                cursor = next_cursor;
            }
        }
        if !prepared_receipt_seen || cursor != head {
            return Err(BridgeFault::Unknown);
        }
        let final_status = status_snapshot(&mut client)?;
        if final_status.authority_instance_id != initial.authority_instance_id
            || final_status.stream_id != initial.stream_id
            || final_status.last_sequence != initial.last_sequence
        {
            return Err(BridgeFault::Unknown);
        }
        Ok(())
    }

    fn verify_managed_source(
        &self,
        session_id: &str,
        generation: u64,
        workspace_root: &std::path::Path,
    ) -> Result<(), BridgeFault> {
        let session = ManagedSessionId::parse(session_id).map_err(|_| BridgeFault::Denied)?;
        let mut client = self.client()?;
        let record = session_record(&mut client, session.clone())?;
        if record.control_state != ControlState::Prepared
            || record.runtime_kind != crate::contracts::agent_adapter::AgentRuntimeKind::Codex
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
        if binding.state != BindingState::BoundAttached
            || binding
                .binding
                .as_ref()
                .is_none_or(|value| std::path::Path::new(&value.workspace_root) != workspace_root)
        {
            return Err(BridgeFault::Denied);
        }
        let attached_instance = binding.runtime_instance_id.ok_or(BridgeFault::Denied)?;
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
            generation: observed_generation,
            identity: Some(identity),
            ..
        } = runtime.projection
        else {
            return Err(BridgeFault::Unknown);
        };
        if observed_session != session
            || observed_generation != generation
            || status.actual_state != ActualState::Ready
            || status.desired_state != crate::contracts::control::DesiredState::Running
            || status.runtime_instance_id.as_ref() != Some(&attached_instance)
            || identity.managed_session_id != session
            || identity.runtime_instance_id != attached_instance
            || identity.generation != generation
        {
            return Err(BridgeFault::Denied);
        }
        Ok(())
    }
}

impl ManagedAuthority for F8Authority {
    fn verify_fresh_receiver(&self, session_id: &str) -> Result<(), BridgeFault> {
        F8Authority::verify_fresh_receiver(self, session_id)
    }

    fn verify_managed_source(
        &self,
        session_id: &str,
        generation: u64,
        workspace_root: &std::path::Path,
    ) -> Result<(), BridgeFault> {
        F8Authority::verify_managed_source(self, session_id, generation, workspace_root)
    }

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

fn status_snapshot(
    client: &mut F8Client,
) -> Result<crate::contracts::control::AuthorityStatus, BridgeFault> {
    match client
        .call(Operation::Status(Empty {}), Target::None)
        .map_err(map_read_fault)?
        .value
    {
        ResultValue::Status(status)
            if status.authority_instance_id == *client.authority()
                && status.protocol_version == 2 =>
        {
            Ok(status)
        }
        _ => Err(BridgeFault::Unknown),
    }
}

fn session_record(
    client: &mut F8Client,
    session: ManagedSessionId,
) -> Result<crate::contracts::control::ManagedSessionRecord, BridgeFault> {
    match client
        .call(
            Operation::Get(Get {
                managed_session_id: session.clone(),
            }),
            Target::None,
        )
        .map_err(map_read_fault)?
        .value
    {
        ResultValue::Session(record) if record.managed_session_id == session => Ok(record),
        _ => Err(BridgeFault::Unknown),
    }
}

fn inspect_receiver_event(
    value: &serde_json::Value,
    sequence: u64,
    created: u64,
    session: &ManagedSessionId,
    authority_instance_id: &str,
    stream_id: &str,
    prepared_receipt_seen: &mut bool,
) -> Result<(), BridgeFault> {
    let event_type = value["event_type"].as_str().ok_or(BridgeFault::Unknown)?;
    let event_session = value["session_id"].as_str();
    if value["stream_id"].as_str() != Some(stream_id)
        || value["source"]["component"].as_str() != Some("wyss_control")
        || value["source"]["instance_id"].as_str() != Some(authority_instance_id)
    {
        return Err(BridgeFault::Unknown);
    }
    if sequence == created {
        *prepared_receipt_seen = event_type == "authority.managed_session.prepared"
            && event_session == Some(session.as_str())
            && value["correlation_id"].as_str().is_some();
        if !*prepared_receipt_seen {
            return Err(BridgeFault::Unknown);
        }
    }
    if sequence > created && event_type.starts_with("authority.managed_turn.") {
        if event_session.is_none() {
            return Err(BridgeFault::Unknown);
        }
        if event_session == Some(session.as_str()) {
            return Err(BridgeFault::Denied);
        }
    }
    Ok(())
}

fn next_receiver_sequence(
    cursor: u64,
    sequence: u64,
    observed_head: u64,
) -> Result<u64, BridgeFault> {
    if sequence <= cursor || sequence > observed_head || sequence != cursor.saturating_add(1) {
        return Err(BridgeFault::Unknown);
    }
    Ok(sequence)
}

/// Converts the current-history result into the stronger proof required for dispatch.
/// `turn/start` currently cannot atomically consume a fresh-only receipt, so a clean history
/// remains unproven even though the snapshot check itself succeeded.
fn require_atomic_fresh_start(snapshot: Result<(), BridgeFault>) -> Result<(), BridgeFault> {
    snapshot?;
    Err(BridgeFault::Unknown)
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
    handoff_receiver: bool,
}

pub struct Coordinator {
    journal: Arc<RunJournal>,
    authority: Arc<dyn ManagedAuthority>,
    plans: Mutex<HashMap<String, ActivePlan>>,
    admission: Mutex<()>,
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
            admission: Mutex::new(()),
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
        // Serialize claim admission through insertion of the volatile plan. SQLite's
        // equality-only workspace claim is not sufficient for nested roots.
        let _admission = self
            .admission
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let canonical = std::fs::canonicalize(&plan.workspace_root)
            .map_err(|_| CoordinatorError::WorkspaceConflict)?;
        let identity = workspace_identity(&canonical).map_err(|_| CoordinatorError::InvalidPlan)?;
        let revision = workspace_revision(&canonical).map_err(|_| CoordinatorError::InvalidPlan)?;
        if identity != plan.workspace_id || revision != plan.workspace_revision {
            return Err(CoordinatorError::WorkspaceConflict);
        }
        let claims = self.journal.claimed_workspaces().map_err(map_journal)?;
        let plans = self.plans.lock().unwrap_or_else(PoisonError::into_inner);
        for (run_id, _identity) in claims {
            if let Some(active) = plans.get(&run_id) {
                if active.plan.plan_id == plan.plan_id {
                    continue; // The journal separately validates exact idempotent replay.
                }
                let existing = std::fs::canonicalize(&active.plan.workspace_root)
                    .map_err(|_| CoordinatorError::WorkspaceConflict)?;
                if canonical.starts_with(&existing) || existing.starts_with(&canonical) {
                    return Err(CoordinatorError::WorkspaceConflict);
                }
            } else {
                // A recovered claim still protects an ambiguous effect, but its root
                // is deliberately not persisted. Never infer non-overlap from digests.
                return Err(CoordinatorError::WorkspaceConflict);
            }
        }
        drop(plans);
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
            .insert(
                run.run_id.clone(),
                ActivePlan {
                    plan,
                    handoff_receiver: false,
                },
            );
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
        // A retry must carry the same reviewed bytes and confirmation before idempotency is
        // considered. Validate against the original confirmation time so an accepted retry
        // remains idempotent after envelope expiry without accepting a mutated body.
        if envelope.digest()? != envelope.content_digest {
            return Err(CoordinatorError::ReviewStale);
        }
        envelope.validate(confirmation.confirmed_at_unix)?;
        confirmation.validate_for(&envelope, confirmation.confirmed_at_unix)?;
        if confirmation.handoff_id == envelope.handoff_id
            && confirmation.revision == envelope.revision
            && confirmation.reviewed_digest == envelope.content_digest
            && let Some(existing) = self
                .journal
                .handoff_run(
                    &envelope.handoff_id,
                    envelope.revision,
                    &envelope.content_digest,
                    confirmation.confirmed_at_unix,
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
        if envelope.source.plane == super::HandoffPlane::Managed {
            match self.authority.verify_managed_source(
                &envelope.source.session_id,
                envelope.source.session_generation,
                &source_root,
            ) {
                Ok(()) => (),
                Err(BridgeFault::Denied | BridgeFault::Busy) => {
                    return Err(CoordinatorError::WorkspaceConflict);
                }
                Err(BridgeFault::Unavailable) => {
                    return Err(CoordinatorError::AuthorityUnavailable);
                }
                Err(BridgeFault::Unknown) => {
                    return Err(CoordinatorError::SourceFreshnessUnproven);
                }
            }
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
        match self.authority.verify_fresh_receiver(&managed_session_id) {
            Ok(()) => (),
            Err(BridgeFault::Denied | BridgeFault::Busy) => {
                return Err(CoordinatorError::ReceiverNotFresh);
            }
            Err(BridgeFault::Unavailable) => {
                return Err(CoordinatorError::AuthorityUnavailable);
            }
            Err(BridgeFault::Unknown) => {
                return Err(CoordinatorError::ReceiverFreshnessUnproven);
            }
        }
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
        if let Some(active) = self
            .plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&run.run_id)
        {
            active.handoff_receiver = true;
        }
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
    pub fn has_background_work(&self, now: u64) -> Result<bool, CoordinatorError> {
        let runs = self.list()?;
        let plans = self.plans.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(runs.iter().any(|run| {
            matches!(
                run.state,
                RunState::Ready | RunState::Running | RunState::Pausing | RunState::Cancelling
            ) || (run.state == RunState::Paused
                && plans
                    .get(&run.run_id)
                    .is_some_and(|active| now < active.plan.expires_at_unix))
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
        let expires_at = self
            .plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(run_id)
            .map(|active| active.plan.expires_at_unix)
            .ok_or(CoordinatorError::InvalidTransition)?;
        if now >= expires_at {
            self.expire_paused(&run, now)?;
            return Err(CoordinatorError::Expired);
        }
        self.journal
            .set_run_state(run_id, run.revision, RunState::Running, None, now)
            .map_err(map_journal)
    }

    fn expire_paused(&self, run: &CoordinatorSnapshot, now: u64) -> Result<(), CoordinatorError> {
        if run
            .steps
            .iter()
            .any(|step| step.state == StepState::Verifying)
        {
            for step in &run.steps {
                if step.state == StepState::Verifying {
                    self.journal
                        .set_step_state(
                            &run.run_id,
                            &step.step_id,
                            StepState::Verifying,
                            StepState::NeedsReview,
                            Some(FailureCode::PlanExpired),
                            now,
                        )
                        .map_err(map_journal)?;
                }
            }
            let current = self.get(&run.run_id)?;
            if current.state == RunState::Paused {
                self.journal
                    .set_run_state(
                        &current.run_id,
                        current.revision,
                        RunState::NeedsReview,
                        Some(FailureCode::PlanExpired),
                        now,
                    )
                    .map_err(map_journal)?;
            }
            return Ok(());
        }
        for step in &run.steps {
            if !step.state.terminal() {
                self.journal
                    .set_step_state(
                        &run.run_id,
                        &step.step_id,
                        step.state,
                        StepState::Cancelled,
                        Some(FailureCode::PlanExpired),
                        now,
                    )
                    .map_err(map_journal)?;
            }
        }
        let current = self.get(&run.run_id)?;
        if current.state == RunState::Paused {
            self.journal
                .set_run_state(
                    &current.run_id,
                    current.revision,
                    RunState::Expired,
                    Some(FailureCode::PlanExpired),
                    now,
                )
                .map_err(map_journal)?;
        }
        Ok(())
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
            // A review resolves uncertainty; it never grants a new dispatch.
            // The owner must submit a fresh reviewed plan to continue.
            self.close_reconciled_run(run_id, now)
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
        // Never close a run while any completed-but-unverified effect remains.
        // Legacy in-memory runs may contain more than one Verifying step.
        if run.steps.iter().any(|step| {
            matches!(
                step.state,
                StepState::NeedsReview
                    | StepState::Verifying
                    | StepState::Starting
                    | StepState::Running
            )
        }) {
            return Ok(run);
        }
        // Reconciliation is a safety closure, not a new owner authorization. Every
        // remaining never-dispatched step is explicitly cancelled, regardless of
        // whether the request to pause or stop survived as a durable run state.
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
                self.journal
                    .set_step_state(
                        run_id,
                        &step.step_id,
                        step.state,
                        StepState::Cancelled,
                        Some(FailureCode::OwnerReviewRequired),
                        now,
                    )
                    .map_err(map_journal)?;
            }
        }
        let run = self.get(run_id)?;
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
        } else {
            // Completed reviewed effects never implicitly authorize a new turn.
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

    /// A terminal/paused review fence expires independently of other running work.
    /// An executed effect never becomes an unreviewed "cancelled" effect.
    fn expire_inactive(&self, run: &CoordinatorSnapshot, now: u64) -> Result<(), CoordinatorError> {
        if run.state == RunState::Paused {
            return self.expire_paused(run, now);
        }
        if !matches!(
            run.state,
            RunState::Running | RunState::Pausing | RunState::Cancelling
        ) {
            return Ok(());
        }
        if run
            .steps
            .iter()
            .any(|step| step.state == StepState::Verifying)
        {
            for step in &run.steps {
                if step.state == StepState::Verifying {
                    self.journal
                        .set_step_state(
                            &run.run_id,
                            &step.step_id,
                            StepState::Verifying,
                            StepState::NeedsReview,
                            Some(FailureCode::PlanExpired),
                            now,
                        )
                        .map_err(map_journal)?;
                }
            }
            return Ok(());
        }
        if run.state == RunState::Cancelling {
            self.cancel_unstarted(&run.run_id, now)?;
            return Ok(());
        }
        for step in &run.steps {
            if !step.state.terminal() {
                self.journal
                    .set_step_state(
                        &run.run_id,
                        &step.step_id,
                        step.state,
                        StepState::Cancelled,
                        Some(FailureCode::PlanExpired),
                        now,
                    )
                    .map_err(map_journal)?;
            }
        }
        let current = self.get(&run.run_id)?;
        if matches!(current.state, RunState::Running | RunState::Pausing) {
            self.journal
                .set_run_state(
                    &current.run_id,
                    current.revision,
                    RunState::Expired,
                    Some(FailureCode::PlanExpired),
                    now,
                )
                .map_err(map_journal)?;
        }
        Ok(())
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
        // Deadlines are per run, not blocked by the global Managed execution slot.
        // Service all inactive/verification/paused deadlines before polling the live turn.
        for run in &ordered {
            if now >= plan_deadline(&self.plans, &run.run_id).unwrap_or(0)
                && !run
                    .steps
                    .iter()
                    .any(|step| matches!(step.state, StepState::Starting | StepState::Running))
            {
                self.expire_inactive(run, now)?;
            }
        }
        let ordered = ordered
            .iter()
            .map(|run| self.get(&run.run_id))
            .collect::<Result<Vec<_>, _>>()?;
        // Exactly one current Managed turn is allowed globally. Poll it before considering a
        // new dispatch; another run can never race it into the authority.
        if let Some((run, step)) = ordered.iter().find_map(|run| {
            run.steps
                .iter()
                .find(|s| matches!(s.state, StepState::Starting | StepState::Running))
                .map(|step| (run, step))
        }) {
            if now >= plan_deadline(&self.plans, &run.run_id).unwrap_or(0) {
                return self.expire_active(run, step, now);
            }
            return self.poll_active(run, step, now);
        }
        // Verification fences only the run/workspace that owns the completed effect.
        // A global return here would also starve unrelated workspaces and their deadlines.
        let mut dispatched = false;
        for run in ordered {
            if run.state == RunState::Paused {
                if now >= plan_deadline(&self.plans, &run.run_id).unwrap_or(0) {
                    self.expire_paused(&self.get(&run.run_id)?, now)?;
                }
                continue;
            }
            if !matches!(
                run.state,
                RunState::Running | RunState::Pausing | RunState::Cancelling
            ) {
                continue;
            }
            // A prior effect in this run requires owner verification before any more
            // effects can dispatch, even for dependency-free DAG steps. This fence
            // is per run; independent workspace deadlines and work still progress.
            if run
                .steps
                .iter()
                .any(|step| step.state == StepState::Verifying)
            {
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
            let handoff_receiver = self
                .plans
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&run.run_id)
                .is_some_and(|active| active.handoff_receiver);
            for step in &plan.steps {
                // Do not dispatch from the snapshot taken before an earlier admission result.
                // In particular, a mandatory denial cancels later eligible rows immediately.
                self.stop_after_mandatory_failure(&run.run_id, now)?;
                let current_run = self.get(&run.run_id)?;
                if current_run.state != RunState::Running {
                    break;
                }
                let Some(row) = current_run
                    .steps
                    .iter()
                    .find(|row| row.step_id == step.step_id)
                else {
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
                if handoff_receiver {
                    let receiver = step
                        .managed_session_id
                        .as_deref()
                        .ok_or(CoordinatorError::InvalidPlan)?;
                    match self.authority.verify_fresh_receiver(receiver) {
                        Ok(()) => (),
                        Err(BridgeFault::Denied | BridgeFault::Busy) => {
                            self.journal
                                .set_step_state(
                                    &run.run_id,
                                    &step.step_id,
                                    row.state,
                                    StepState::Blocked,
                                    Some(FailureCode::ManagedSessionUnavailable),
                                    now,
                                )
                                .map_err(map_journal)?;
                            self.stop_after_mandatory_failure(&run.run_id, now)?;
                            self.finish_if_done(&run.run_id, now)?;
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
                            let review_run = self.get(&run.run_id)?;
                            self.journal
                                .set_run_state(
                                    &run.run_id,
                                    review_run.revision,
                                    RunState::NeedsReview,
                                    Some(FailureCode::AuthorityUnavailable),
                                    now,
                                )
                                .map_err(map_journal)?;
                            break;
                        }
                    }
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
                        self.stop_after_mandatory_failure(&run.run_id, now)?;
                        self.finish_if_done(&run.run_id, now)?;
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
                        self.stop_after_mandatory_failure(&run.run_id, now)?;
                        self.finish_if_done(&run.run_id, now)?;
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
    use crate::orchestration::{
        DependencyV1, HandoffConfirmationV1, HandoffEnvelopeV1, HandoffPlane, HandoffSourceV1,
        HandoffTargetV1, PlanLimitsV1, PlanReviewV1, StopPolicy,
    };
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
        inspect_fault: Mutex<Option<BridgeFault>>,
        fresh_receiver_fault: Mutex<Option<BridgeFault>>,
        source_fault: Mutex<Option<BridgeFault>>,
        start_fault: Mutex<Option<BridgeFault>>,
        cancel_fault: Mutex<Option<BridgeFault>>,
        observation: Mutex<TurnObservation>,
    }
    impl FakeAuthority {
        fn new(root: PathBuf) -> Self {
            Self {
                root,
                second_root: Mutex::new(None),
                starts: AtomicUsize::new(0),
                cancels: AtomicUsize::new(0),
                inspect_fault: Mutex::new(None),
                fresh_receiver_fault: Mutex::new(None),
                source_fault: Mutex::new(None),
                start_fault: Mutex::new(None),
                cancel_fault: Mutex::new(None),
                observation: Mutex::new(TurnObservation {
                    state: TurnState::Running,
                    reason: None,
                    revision: 1,
                }),
            }
        }
    }
    impl ManagedAuthority for FakeAuthority {
        fn verify_fresh_receiver(&self, _session_id: &str) -> Result<(), BridgeFault> {
            if let Some(fault) = self.fresh_receiver_fault.lock().unwrap().take() {
                return Err(fault);
            }
            Ok(())
        }

        fn verify_managed_source(
            &self,
            _session_id: &str,
            _generation: u64,
            _workspace_root: &std::path::Path,
        ) -> Result<(), BridgeFault> {
            if let Some(fault) = self.source_fault.lock().unwrap().take() {
                return Err(fault);
            }
            Ok(())
        }

        fn inspect(
            &self,
            session_id: &str,
            _effect: EffectClass,
            _model: Option<&str>,
        ) -> Result<Admission, BridgeFault> {
            if let Some(fault) = self.inspect_fault.lock().unwrap().take() {
                return Err(fault);
            }
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
            if let Some(fault) = *self.cancel_fault.lock().unwrap() {
                return Err(fault);
            }
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

    fn handoff(root: &Path) -> (HandoffEnvelopeV1, HandoffConfirmationV1) {
        let workspace_id = workspace_identity(root).unwrap();
        let workspace_revision = workspace_revision(root).unwrap();
        let mut envelope = HandoffEnvelopeV1 {
            schema_version: 1,
            handoff_id: "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into(),
            revision: 1,
            source: HandoffSourceV1 {
                host_id: "local".into(),
                session_id: SESSION.into(),
                session_generation: 1,
                plane: HandoffPlane::Native,
                project_id: "test-project".into(),
                workspace_id: workspace_id.clone(),
                workspace_revision: workspace_revision.clone(),
                workspace_root: root.to_string_lossy().into_owned(),
            },
            target: HandoffTargetV1 {
                host_id: "local".into(),
                project_id: "test-project".into(),
                workspace_id,
                workspace_revision,
                workspace_root: root.to_string_lossy().into_owned(),
                provider: ProviderLane::ManagedCodex,
                effect: EffectClass::ManagedInference,
                managed_session_id: Some(SESSION.into()),
                native_profile_id: None,
                profile_revision: None,
            },
            objective: "review this small, owner-selected handoff".into(),
            selected_context_items: vec![],
            selected_file_references: vec![],
            open_questions: vec![],
            created_at_unix: 100,
            expires_at_unix: 300,
            content_digest: String::new(),
        };
        envelope.seal().unwrap();
        let confirmation = HandoffConfirmationV1 {
            handoff_id: envelope.handoff_id.clone(),
            revision: envelope.revision,
            reviewed_digest: envelope.content_digest.clone(),
            confirmed_at_unix: 120,
        };
        (envelope, confirmation)
    }

    #[test]
    fn fresh_receiver_evidence_requires_creation_receipt_and_rejects_prior_turns() {
        let session = ManagedSessionId::parse(SESSION).unwrap();
        let mut prepared_seen = false;
        let prepared = serde_json::json!({
            "sequence": "42",
            "stream_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "event_type": "authority.managed_session.prepared",
            "session_id": SESSION,
            "correlation_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "source": {
                "component": "wyss_control",
                "instance_id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
            }
        });
        inspect_receiver_event(
            &prepared,
            42,
            42,
            &session,
            "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            &mut prepared_seen,
        )
        .unwrap();
        assert!(prepared_seen);

        let prior_turn = serde_json::json!({
            "sequence": "43",
            "stream_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "event_type": "authority.managed_turn.admitted",
            "session_id": SESSION,
            "source": {
                "component": "wyss_control",
                "instance_id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
            }
        });
        assert_eq!(
            inspect_receiver_event(
                &prior_turn,
                43,
                42,
                &session,
                "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                &mut prepared_seen
            ),
            Err(BridgeFault::Denied)
        );

        let mut no_receipt = false;
        let malformed_creation = serde_json::json!({
            "sequence": "42",
            "stream_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "event_type": "authority.managed_session.prepared",
            "session_id": SESSION,
            "correlation_id": null,
            "source": {
                "component": "wyss_control",
                "instance_id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
            }
        });
        assert_eq!(
            inspect_receiver_event(
                &malformed_creation,
                42,
                42,
                &session,
                "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                &mut no_receipt
            ),
            Err(BridgeFault::Unknown)
        );

        let mut missing_identity = false;
        let unscoped_turn = serde_json::json!({
            "sequence": "43",
            "stream_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "event_type": "authority.managed_turn.completed",
            "session_id": null,
            "source": {
                "component": "wyss_control",
                "instance_id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
            }
        });
        assert_eq!(
            inspect_receiver_event(
                &unscoped_turn,
                43,
                42,
                &session,
                "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                &mut missing_identity
            ),
            Err(BridgeFault::Unknown)
        );

        let mut cross_authority = false;
        assert_eq!(
            inspect_receiver_event(
                &prepared,
                42,
                42,
                &session,
                "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                &mut cross_authority
            ),
            Err(BridgeFault::Unknown)
        );

        // The current F8 read evidence proves the receiver is unused at the observed head,
        // but turn/start cannot consume that fact atomically. It is not dispatch authority.
        assert_eq!(
            require_atomic_fresh_start(Ok(())),
            Err(BridgeFault::Unknown)
        );
        assert_eq!(
            require_atomic_fresh_start(Err(BridgeFault::Denied)),
            Err(BridgeFault::Denied)
        );

        assert_eq!(next_receiver_sequence(42, 43, 43), Ok(43));
        assert_eq!(
            next_receiver_sequence(42, 44, 44),
            Err(BridgeFault::Unknown),
            "a skipped event refuses the snapshot"
        );
        assert_eq!(
            next_receiver_sequence(42, 43, 42),
            Err(BridgeFault::Unknown),
            "a page beyond the observed head terminates fail-closed instead of rereading"
        );
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
    fn mandatory_admission_refusal_cancels_independent_steps_before_dispatch() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        *authority.inspect_fault.lock().unwrap() = Some(BridgeFault::Denied);
        let first = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let second = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        let draft = plan(
            &root,
            vec![step(first, vec![]), step(second, vec![])],
            "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        );
        let accepted = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();

        coordinator.tick(101).unwrap();

        let stopped = coordinator.get(&accepted.run_id).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        assert_eq!(stopped.steps[0].state, StepState::Blocked);
        assert_eq!(stopped.steps[1].state, StepState::Cancelled);
        assert_eq!(stopped.state, RunState::Failed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_handoff_admission_fails_closed_when_receiver_is_not_fresh() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        *authority.fresh_receiver_fault.lock().unwrap() = Some(BridgeFault::Denied);
        let (envelope, confirmation) = handoff(&root);

        assert_eq!(
            coordinator
                .submit_handoff(envelope, confirmation, 120)
                .unwrap_err(),
            CoordinatorError::ReceiverNotFresh
        );
        assert!(coordinator.list().unwrap().is_empty());
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_handoff_admission_fails_closed_when_freshness_is_unproven() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        *authority.fresh_receiver_fault.lock().unwrap() = Some(BridgeFault::Unknown);
        let (envelope, confirmation) = handoff(&root);

        assert_eq!(
            coordinator
                .submit_handoff(envelope, confirmation, 120)
                .unwrap_err(),
            CoordinatorError::ReceiverFreshnessUnproven
        );
        assert!(coordinator.list().unwrap().is_empty());
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_managed_source_admission_fails_closed_without_fresh_identity_evidence() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        *authority.source_fault.lock().unwrap() = Some(BridgeFault::Unknown);
        let (mut envelope, confirmation) = handoff(&root);
        envelope.source.plane = super::super::HandoffPlane::Managed;
        // This changes reviewed bytes; seal again and bind a matching explicit confirmation.
        envelope.seal().unwrap();
        let confirmation = HandoffConfirmationV1 {
            handoff_id: envelope.handoff_id.clone(),
            revision: envelope.revision,
            reviewed_digest: envelope.content_digest.clone(),
            confirmed_at_unix: confirmation.confirmed_at_unix,
        };

        assert_eq!(
            coordinator
                .submit_handoff(envelope, confirmation, 120)
                .unwrap_err(),
            CoordinatorError::SourceFreshnessUnproven
        );
        assert!(coordinator.list().unwrap().is_empty());
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn handoff_receiver_is_revalidated_before_effectful_dispatch() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let (envelope, confirmation) = handoff(&root);
        let run = coordinator
            .submit_handoff(envelope, confirmation, 120)
            .unwrap();
        // A same-UID caller may create work after review. The backend repeats authority
        // history validation at dispatch and blocks the stale receiver without starting.
        *authority.fresh_receiver_fault.lock().unwrap() = Some(BridgeFault::Denied);

        coordinator.tick(121).unwrap();

        let after = coordinator.get(&run.run_id).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        assert_eq!(after.steps[0].state, StepState::Blocked);
        assert_eq!(
            after.steps[0].failure,
            Some(FailureCode::ManagedSessionUnavailable)
        );
        assert_eq!(after.state, RunState::Blocked);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn handoff_replay_rejects_mutated_body_and_changed_confirmation_time() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let (envelope, confirmation) = handoff(&root);
        let first = coordinator
            .submit_handoff(envelope.clone(), confirmation.clone(), 120)
            .unwrap();

        let mut mutated = envelope.clone();
        mutated.objective.push_str(" with changed bytes");
        assert_eq!(
            coordinator
                .submit_handoff(mutated, confirmation.clone(), 121)
                .unwrap_err(),
            CoordinatorError::ReviewStale
        );
        let mut changed_confirmation = confirmation;
        changed_confirmation.confirmed_at_unix += 1;
        assert_eq!(
            coordinator
                .submit_handoff(envelope, changed_confirmation, 121)
                .unwrap_err(),
            CoordinatorError::ReviewStale
        );
        assert_eq!(coordinator.list().unwrap().len(), 1);
        assert_eq!(coordinator.get(&first.run_id).unwrap().run_id, first.run_id);
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn paused_plan_keeps_daemon_alive_past_idle_and_resumes_on_reconnect() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let draft = plan(
            &root,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let accepted = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        let paused = coordinator.pause(&accepted.run_id, 110).unwrap();
        assert_eq!(paused.state, RunState::Paused);

        // 61 seconds without a window request exceeds the daemon idle threshold. The same
        // live coordinator remains reachable because its approved finite plan is still valid.
        assert!(coordinator.has_background_work(171).unwrap());
        coordinator.tick(171).unwrap();
        assert_eq!(
            coordinator.get(&accepted.run_id).unwrap().state,
            RunState::Paused
        );

        coordinator.resume(&accepted.run_id, 172).unwrap();
        coordinator.tick(173).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        assert!(matches!(
            coordinator.get(&accepted.run_id).unwrap().steps[0].state,
            StepState::Starting | StepState::Running
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn paused_plan_expires_and_restart_recovery_does_not_resume_it() {
        let root = root();
        let (coordinator, journal, authority) = coordinator(&root);
        let draft = plan(
            &root,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let accepted = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.pause(&accepted.run_id, 110).unwrap();
        assert!(!coordinator.has_background_work(400).unwrap());
        assert_eq!(
            coordinator.resume(&accepted.run_id, 400).unwrap_err(),
            CoordinatorError::Expired
        );
        let expired = coordinator.get(&accepted.run_id).unwrap();
        assert_eq!(expired.state, RunState::Expired);
        assert_eq!(expired.failure, Some(FailureCode::PlanExpired));

        // A separate run that is still within its envelope becomes NeedsReview after a
        // coordinator restart; in-memory prompts are never reconstructed from SQLite.
        let second_draft = plan(
            &root,
            vec![step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![])],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        let second = coordinator
            .submit(second_draft.clone(), review(&second_draft), 100)
            .unwrap();
        coordinator.pause(&second.run_id, 111).unwrap();
        let restarted = Coordinator::new(journal, authority.clone(), 112).unwrap();
        assert_eq!(
            restarted.get(&second.run_id).unwrap().state,
            RunState::NeedsReview
        );
        assert_eq!(
            restarted.resume(&second.run_id, 113).unwrap_err(),
            CoordinatorError::InvalidTransition
        );
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
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
    fn verifying_workspace_does_not_starve_another_workspaces_expired_deadline() {
        let root_a = root();
        let root_b = root();
        let (coordinator, _journal, authority) = coordinator(&root_a);
        *authority.second_root.lock().unwrap() = Some(root_b.clone());

        let mut first = plan(
            &root_a,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        first.expires_at_unix = 140;
        first.limits.max_total_seconds = 40;
        let a = coordinator
            .submit(first.clone(), review(&first), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        *authority.observation.lock().unwrap() = TurnObservation {
            state: TurnState::Completed,
            reason: None,
            revision: 2,
        };
        coordinator.tick(102).unwrap();
        assert_eq!(
            coordinator.get(&a.run_id).unwrap().steps[0].state,
            StepState::Verifying
        );

        let mut second_step = step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![]);
        second_step.managed_session_id = Some(SECOND_SESSION.into());
        let mut second = plan(
            &root_b,
            vec![second_step],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        second.expires_at_unix = 120;
        second.limits.max_total_seconds = 20;
        let b = coordinator
            .submit(second.clone(), review(&second), 103)
            .unwrap();
        coordinator.tick(120).unwrap();
        assert_eq!(coordinator.get(&b.run_id).unwrap().state, RunState::Expired);
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            coordinator.get(&a.run_id).unwrap().steps[0].state,
            StepState::Verifying
        );

        coordinator.tick(140).unwrap();
        let expired_review = coordinator.get(&a.run_id).unwrap();
        assert_eq!(expired_review.state, RunState::NeedsReview);
        assert_eq!(expired_review.steps[0].state, StepState::NeedsReview);
        assert_eq!(
            expired_review.steps[0].failure,
            Some(FailureCode::PlanExpired)
        );
        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    }

    #[test]
    fn verifying_workspace_does_not_block_independent_serial_dispatch() {
        let root_a = root();
        let root_b = root();
        let (coordinator, _journal, authority) = coordinator(&root_a);
        *authority.second_root.lock().unwrap() = Some(root_b.clone());
        let first = plan(
            &root_a,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let a = coordinator
            .submit(first.clone(), review(&first), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        *authority.observation.lock().unwrap() = TurnObservation {
            state: TurnState::Completed,
            reason: None,
            revision: 2,
        };
        coordinator.tick(102).unwrap();

        let mut second_step = step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![]);
        second_step.managed_session_id = Some(SECOND_SESSION.into());
        let second = plan(
            &root_b,
            vec![second_step],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        let b = coordinator
            .submit(second.clone(), review(&second), 103)
            .unwrap();
        coordinator.tick(104).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 2);
        assert_eq!(
            coordinator.get(&a.run_id).unwrap().steps[0].state,
            StepState::Verifying
        );
        assert!(matches!(
            coordinator.get(&b.run_id).unwrap().steps[0].state,
            StepState::Starting | StepState::Running
        ));
        coordinator.tick(105).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 2);
        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    }

    #[test]
    fn completed_effect_paused_at_expiry_stays_needs_review_with_claim() {
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
            state: TurnState::Completed,
            reason: None,
            revision: 2,
        };
        coordinator.tick(102).unwrap();
        assert_eq!(
            coordinator.pause(&run.run_id, 103).unwrap().state,
            RunState::Paused
        );
        coordinator.tick(400).unwrap();
        let held = coordinator.get(&run.run_id).unwrap();
        assert_eq!(held.state, RunState::NeedsReview);
        assert_eq!(held.steps[0].state, StepState::NeedsReview);
        assert_eq!(held.steps[0].failure, Some(FailureCode::PlanExpired));

        let mut other = plan(
            &root,
            vec![step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![])],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        other.expires_at_unix = 600;
        other.limits.max_total_seconds = 500;
        assert_eq!(
            coordinator
                .submit(other.clone(), review(&other), 401)
                .unwrap_err(),
            CoordinatorError::WorkspaceConflict
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stop_does_not_cancel_or_release_a_completed_unverified_effect() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let draft = plan(
            &root,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bb8-bbbbbbbbbbbb",
        );
        let run = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        *authority.observation.lock().unwrap() = TurnObservation {
            state: TurnState::Completed,
            reason: None,
            revision: 2,
        };
        coordinator.tick(102).unwrap();
        let stopped = coordinator.stop(&run.run_id, 103).unwrap();
        assert_eq!(stopped.state, RunState::Cancelling);
        assert_eq!(stopped.steps[0].state, StepState::Verifying);
        let next = plan(
            &root,
            vec![step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![])],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        assert_eq!(
            coordinator
                .submit(next.clone(), review(&next), 104)
                .unwrap_err(),
            CoordinatorError::WorkspaceConflict
        );
        let reviewed = coordinator
            .verify_step(
                &run.run_id,
                &draft.steps[0].step_id,
                &workspace_revision(&root).unwrap(),
                105,
            )
            .unwrap();
        assert_eq!(reviewed.state, RunState::Cancelled);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn active_managed_turn_does_not_starve_another_workspaces_expired_review() {
        let root_a = root();
        let root_b = root();
        let (coordinator, _journal, authority) = coordinator(&root_a);
        *authority.second_root.lock().unwrap() = Some(root_b.clone());
        let mut first = plan(
            &root_a,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        first.expires_at_unix = 120;
        first.limits.max_total_seconds = 20;
        let a = coordinator
            .submit(first.clone(), review(&first), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        *authority.observation.lock().unwrap() = TurnObservation {
            state: TurnState::Completed,
            reason: None,
            revision: 2,
        };
        coordinator.tick(102).unwrap();

        let mut second_step = step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![]);
        second_step.managed_session_id = Some(SECOND_SESSION.into());
        let second = plan(
            &root_b,
            vec![second_step],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        let b = coordinator
            .submit(second.clone(), review(&second), 103)
            .unwrap();
        coordinator.tick(104).unwrap();
        *authority.observation.lock().unwrap() = TurnObservation {
            state: TurnState::Running,
            reason: None,
            revision: 2,
        };
        coordinator.tick(120).unwrap();
        assert_eq!(
            coordinator.get(&a.run_id).unwrap().state,
            RunState::NeedsReview
        );
        assert_eq!(
            coordinator.get(&a.run_id).unwrap().steps[0].state,
            StepState::NeedsReview
        );
        assert!(matches!(
            coordinator.get(&b.run_id).unwrap().steps[0].state,
            StepState::Starting | StepState::Running
        ));
        assert_eq!(authority.starts.load(Ordering::SeqCst), 2);
        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    }

    #[test]
    fn parent_child_workspace_claims_conflict_in_both_admission_orders() {
        for parent_first in [true, false] {
            let root = root();
            let child = root.join("child");
            fs::create_dir(&child).unwrap();
            let (coordinator, _journal, authority) = coordinator(&root);
            *authority.second_root.lock().unwrap() = Some(child.clone());
            let parent = plan(
                &root,
                vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            );
            let mut child_step = step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![]);
            child_step.managed_session_id = Some(SECOND_SESSION.into());
            let nested = plan(
                &child,
                vec![child_step],
                "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
            );
            let (first, second) = if parent_first {
                (&parent, &nested)
            } else {
                (&nested, &parent)
            };
            let accepted = coordinator
                .submit(first.clone(), review(first), 100)
                .unwrap();
            coordinator.tick(101).unwrap();
            *authority.observation.lock().unwrap() = TurnObservation {
                state: TurnState::Completed,
                reason: None,
                revision: 2,
            };
            coordinator.tick(102).unwrap();
            assert_eq!(
                coordinator.get(&accepted.run_id).unwrap().steps[0].state,
                StepState::Verifying
            );
            assert_eq!(
                coordinator
                    .submit(second.clone(), review(second), 103)
                    .unwrap_err(),
                CoordinatorError::WorkspaceConflict
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn crash_at_both_owner_confirmation_commit_boundaries_is_recoverable() {
        for verified_before_crash in [false, true] {
            let root = root();
            let (coordinator, journal, authority) = coordinator(&root);
            let draft = plan(
                &root,
                vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            );
            let run = coordinator
                .submit(draft.clone(), review(&draft), 100)
                .unwrap();
            coordinator.tick(101).unwrap();
            let step_id = draft.steps[0].step_id.as_str();
            let attempt = coordinator.get(&run.run_id).unwrap().steps[0]
                .attempt_id
                .unwrap();
            // A real attempted effect became ambiguous; the owner began confirming it.
            journal
                .mark_attempt_unknown(&run.run_id, step_id, &attempt, 102)
                .unwrap();
            journal
                .set_step_state(
                    &run.run_id,
                    step_id,
                    StepState::NeedsReview,
                    StepState::Verifying,
                    Some(FailureCode::OwnerReviewRequired),
                    103,
                )
                .unwrap();
            let revision = workspace_revision(&root).unwrap();
            if verified_before_crash {
                journal
                    .verify_step(&run.run_id, step_id, &revision, 104)
                    .unwrap();
            }
            drop(coordinator);
            let restarted = Coordinator::new(journal.clone(), authority.clone(), 105).unwrap();
            if verified_before_crash {
                assert_eq!(
                    restarted.get(&run.run_id).unwrap().state,
                    RunState::Completed
                );
            } else {
                let held = restarted.get(&run.run_id).unwrap();
                assert_eq!(held.state, RunState::NeedsReview);
                assert_eq!(held.steps[0].state, StepState::NeedsReview);
                assert_eq!(journal.claimed_workspaces().unwrap().len(), 1);
                let root_text = root.to_string_lossy().into_owned();
                let resolved = restarted
                    .reconcile(
                        &run.run_id,
                        step_id,
                        ReconcileDecision::ConfirmedCompleted,
                        Some(&root_text),
                        Some(&revision),
                        106,
                    )
                    .unwrap();
                assert_eq!(resolved.state, RunState::Completed);
            }
            assert!(journal.claimed_workspaces().unwrap().is_empty());
            assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn stop_with_ambiguous_cancel_cannot_resume_after_owner_reconciliation() {
        let root = root();
        let (coordinator, journal, authority) = coordinator(&root);
        let draft = plan(
            &root,
            vec![
                step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![]),
                step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![]),
            ],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let run = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        *authority.cancel_fault.lock().unwrap() = Some(BridgeFault::Unknown);
        let stopped = coordinator.stop(&run.run_id, 102).unwrap();
        assert_eq!(stopped.state, RunState::NeedsReview);
        assert_eq!(stopped.steps[0].state, StepState::NeedsReview);
        assert_eq!(stopped.steps[1].state, StepState::Eligible);
        let revised = workspace_revision(&root).unwrap();
        let resolved = coordinator
            .reconcile(
                &run.run_id,
                &draft.steps[0].step_id,
                ReconcileDecision::ConfirmedCompleted,
                Some(&draft.workspace_root),
                Some(&revised),
                103,
            )
            .unwrap();
        assert_eq!(resolved.state, RunState::Cancelled);
        assert_eq!(resolved.steps[0].state, StepState::Succeeded);
        assert_eq!(resolved.steps[1].state, StepState::Cancelled);
        assert!(journal.claimed_workspaces().unwrap().is_empty());
        coordinator.tick(104).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn paused_ambiguous_effect_cannot_implicitly_resume_after_reconciliation() {
        let root = root();
        let (coordinator, journal, authority) = coordinator(&root);
        let draft = plan(
            &root,
            vec![
                step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![]),
                step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![]),
            ],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let run = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        assert_eq!(
            coordinator.pause(&run.run_id, 102).unwrap().state,
            RunState::Pausing
        );
        let attempt = coordinator.get(&run.run_id).unwrap().steps[0]
            .attempt_id
            .unwrap();
        journal
            .mark_attempt_unknown(&run.run_id, &draft.steps[0].step_id, &attempt, 103)
            .unwrap();
        let revised = workspace_revision(&root).unwrap();
        let resolved = coordinator
            .reconcile(
                &run.run_id,
                &draft.steps[0].step_id,
                ReconcileDecision::ConfirmedCompleted,
                Some(&draft.workspace_root),
                Some(&revised),
                104,
            )
            .unwrap();
        assert_eq!(resolved.state, RunState::Cancelled);
        assert_eq!(resolved.steps[1].state, StepState::Cancelled);
        coordinator.tick(105).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        assert!(journal.claimed_workspaces().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn independent_steps_in_same_run_wait_for_owner_verification() {
        let root = root();
        let (coordinator, _journal, authority) = coordinator(&root);
        let draft = plan(
            &root,
            vec![
                step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![]),
                step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![]),
            ],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let run = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        *authority.observation.lock().unwrap() = TurnObservation {
            state: TurnState::Completed,
            reason: None,
            revision: 2,
        };
        coordinator.tick(102).unwrap();
        assert_eq!(
            coordinator.get(&run.run_id).unwrap().steps[0].state,
            StepState::Verifying
        );
        // No filesystem changes is not equivalent to reviewed effects.
        for now in [103, 104, 105] {
            coordinator.tick(now).unwrap();
            assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
            let waiting = coordinator.get(&run.run_id).unwrap();
            assert_eq!(waiting.steps[0].state, StepState::Verifying);
            assert_ne!(waiting.steps[1].state, StepState::Verifying);
        }
        coordinator
            .verify_step(
                &run.run_id,
                &draft.steps[0].step_id,
                &workspace_revision(&root).unwrap(),
                106,
            )
            .unwrap();
        coordinator.tick(107).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 2);
        fs::remove_dir_all(root).unwrap();
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
