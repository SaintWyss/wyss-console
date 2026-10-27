//! Finite dependency scheduler over the existing protocol-2 Managed authority.
//! This module is a client of `wyss-control`, never an execution authority of its own.
use super::journal::{
    CancellationOutcome, WorkspaceClaimBinding, WorkspaceImportClaim, WorkspaceSettlementIntent,
};
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
use sha2::Digest;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
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
    authority_store_id: String,
    authority_epoch: String,
    attachment_revision: u64,
    lease_id: String,
    lease_revision: u64,
    workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct GuardedStartContext {
    pub coordinator_store_id: String,
    pub run_id: String,
    pub plan_revision: u64,
    pub plan_digest: String,
    pub step_id: String,
    pub attempt_id: String,
    pub model_id: Option<String>,
    pub effect: EffectClass,
    pub handoff_receiver: bool,
    pub claim: WorkspaceClaimBinding,
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
    fn initialize_workspace_guard(
        &self,
        _coordinator_store_id: &str,
        _claims: &[WorkspaceImportClaim],
    ) -> Result<(), BridgeFault> {
        Err(BridgeFault::Unknown)
    }
    fn acquire_workspace_claim(
        &self,
        _coordinator_store_id: &str,
        _run_id: &str,
        _plan_revision: u64,
        _plan_digest: &str,
        _workspace_root: &Path,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        Err(BridgeFault::Unknown)
    }
    fn set_workspace_gate(
        &self,
        _claim: &WorkspaceClaimBinding,
        _gate: &str,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        Err(BridgeFault::Unknown)
    }
    fn workspace_claim_status(
        &self,
        _claim: &WorkspaceClaimBinding,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        Err(BridgeFault::Unknown)
    }
    fn settle_workspace_claim(
        &self,
        _claim: &WorkspaceClaimBinding,
        _workspace_root: &Path,
        _intent: &WorkspaceSettlementIntent,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        Err(BridgeFault::Unknown)
    }
    fn acknowledge_workspace_settlement(
        &self,
        _claim: &WorkspaceClaimBinding,
        _gate: &str,
        _coordinator_revision: u64,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        Err(BridgeFault::Unknown)
    }
    fn release_workspace_claim(
        &self,
        _claim: &WorkspaceClaimBinding,
        _coordinator_revision: u64,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        Err(BridgeFault::Unknown)
    }
    fn release_unused_workspace_claim(
        &self,
        _claim: &WorkspaceClaimBinding,
        _coordinator_revision: u64,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        Err(BridgeFault::Unknown)
    }
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
    /// Capability negotiation is only a gate. A `true` result does not authorize an effect;
    /// implementations must still consume the authority-owned claim and one-use receipt in
    /// `start_guarded`. The production F8 bridge stays closed until protocol-2 implements it.
    fn supports_workspace_guard_v1(&self) -> bool {
        false
    }
    /// M6 must not fall back to ordinary `TurnStart`: the F8 implementation must atomically
    /// validate the durable workspace claim, pinned root, freshness, approval and receipt.
    #[allow(clippy::too_many_arguments)] // Keeps the existing scheduler boundary explicit.
    fn start_guarded(
        &self,
        _admission: &Admission,
        _context: &GuardedStartContext,
        _intent_id: &str,
        _prompt: &str,
        _workspace_root: &Path,
        _workspace_id: &str,
        _workspace_revision: &str,
    ) -> Result<StartedTurn, BridgeFault> {
        Err(BridgeFault::Unknown)
    }
    /// Owner input is not proof of effect outcome or root identity. Only the authority may
    /// produce the evidence that permits settlement and eventual claim release.
    fn verify_workspace_settlement(
        &self,
        _workspace_root: &Path,
        _decision: ReconcileDecision,
    ) -> Result<(), BridgeFault> {
        Err(BridgeFault::Unknown)
    }
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
    fn initialize_workspace_guard(
        &self,
        coordinator_store_id: &str,
        claims: &[WorkspaceImportClaim],
    ) -> Result<(), BridgeFault> {
        let imported = claims
            .iter()
            .map(|claim| {
                serde_json::json!({
                    "claim_id": claim.claim_id,
                    "run_id": claim.run_id,
                    "claim_generation": claim.claim_generation,
                    "source_revision": claim.source_revision
                })
            })
            .collect::<Vec<_>>();
        let mut client = self.client()?;
        client
            .call_workspace_guard(
                "orchestration/claims/import",
                serde_json::json!({
                    "coordinator_store_id": coordinator_store_id,
                    "snapshot_revision": 1,
                    "claims": imported
                }),
            )
            .map_err(map_read_fault)?;
        Ok(())
    }

    fn acquire_workspace_claim(
        &self,
        coordinator_store_id: &str,
        run_id: &str,
        plan_revision: u64,
        plan_digest: &str,
        workspace_root: &Path,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        let mut client = self.client()?;
        let capability = client
            .call_workspace_guard("authority/capabilities", serde_json::json!({}))
            .map_err(map_read_fault)?;
        if capability["guard_ready"] != true || capability["managed_execution_enabled"] != true {
            return Err(BridgeFault::Denied);
        }
        let authority_store_id = capability["authority_store_id"]
            .as_str()
            .ok_or(BridgeFault::Unknown)?
            .to_owned();
        let authority_epoch = capability["authority_epoch"]
            .as_str()
            .ok_or(BridgeFault::Unknown)?
            .to_owned();
        let root = client
            .call_workspace_guard(
                "orchestration/root/observe",
                serde_json::json!({"locator": workspace_root}),
            )
            .map_err(map_read_fault)?;
        let root_binding_id = root["root_binding_id"]
            .as_str()
            .ok_or(BridgeFault::Unknown)?
            .to_owned();
        let root_observation_revision = root["root_observation_revision"]
            .as_u64()
            .ok_or(BridgeFault::Unknown)?;
        let claim = client
            .call_workspace_guard(
                "orchestration/claim/acquire",
                serde_json::json!({
                    "coordinator_store_id": coordinator_store_id,
                    "run_id": run_id,
                    "plan_revision": plan_revision,
                    "plan_digest": plan_digest,
                    "root_binding_id": root_binding_id
                }),
            )
            .map_err(map_read_fault)?;
        if claim["root_binding_id"] != root_binding_id || claim["phase"] != "held" {
            return Err(BridgeFault::Unknown);
        }
        Ok(WorkspaceClaimBinding {
            authority_store_id,
            authority_epoch,
            root_binding_id,
            claim_id: claim["claim_id"]
                .as_str()
                .ok_or(BridgeFault::Unknown)?
                .to_owned(),
            claim_generation: claim["claim_generation"]
                .as_u64()
                .ok_or(BridgeFault::Unknown)?,
            claim_revision: claim["claim_revision"]
                .as_u64()
                .ok_or(BridgeFault::Unknown)?,
            gate_revision: claim["gate_revision"]
                .as_u64()
                .ok_or(BridgeFault::Unknown)?,
            root_observation_revision,
            phase: claim["phase"]
                .as_str()
                .ok_or(BridgeFault::Unknown)?
                .to_owned(),
            gate_state: claim["gate_state"]
                .as_str()
                .ok_or(BridgeFault::Unknown)?
                .to_owned(),
            settlement_id: claim["settlement_id"].as_str().map(str::to_owned),
            coordinator_revision: claim["coordinator_revision"]
                .as_u64()
                .ok_or(BridgeFault::Unknown)?,
        })
    }

    fn set_workspace_gate(
        &self,
        claim: &WorkspaceClaimBinding,
        gate: &str,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        if !matches!(gate, "open" | "paused" | "stopped") {
            return Err(BridgeFault::Denied);
        }
        let mut client = self.client()?;
        if client.authority().as_str() != claim.authority_epoch {
            return Err(BridgeFault::Denied);
        }
        let value = client
            .call_workspace_guard(
                "orchestration/claim/gate",
                serde_json::json!({
                    "claim_id": claim.claim_id,
                    "expected_claim_revision": claim.claim_revision,
                    "expected_gate_revision": claim.gate_revision,
                    "gate": gate
                }),
            )
            .map_err(map_read_fault)?;
        if value["claim_id"] != claim.claim_id
            || value["root_binding_id"] != claim.root_binding_id
            || value["phase"] == "quarantined"
            || value["gate_state"] != gate
        {
            return Err(BridgeFault::Unknown);
        }
        Ok(WorkspaceClaimBinding {
            phase: value["phase"]
                .as_str()
                .ok_or(BridgeFault::Unknown)?
                .to_owned(),
            gate_state: gate.to_owned(),
            claim_revision: value["claim_revision"]
                .as_u64()
                .ok_or(BridgeFault::Unknown)?,
            gate_revision: value["gate_revision"]
                .as_u64()
                .ok_or(BridgeFault::Unknown)?,
            settlement_id: value["settlement_id"].as_str().map(str::to_owned),
            coordinator_revision: value["coordinator_revision"]
                .as_u64()
                .ok_or(BridgeFault::Unknown)?,
            ..claim.clone()
        })
    }

    fn workspace_claim_status(
        &self,
        claim: &WorkspaceClaimBinding,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        let mut client = self.client()?;
        let capability = client
            .call_workspace_guard("authority/capabilities", serde_json::json!({}))
            .map_err(map_read_fault)?;
        if capability["authority_store_id"] != claim.authority_store_id
            || capability["authority_epoch"] != claim.authority_epoch
            || client.authority().as_str() != claim.authority_epoch
        {
            return Err(BridgeFault::Denied);
        }
        let value = client
            .call_workspace_guard(
                "orchestration/claim/status",
                serde_json::json!({"claim_id": claim.claim_id}),
            )
            .map_err(map_read_fault)?;
        workspace_claim_from_value(claim, &value)
    }

    fn settle_workspace_claim(
        &self,
        claim: &WorkspaceClaimBinding,
        workspace_root: &Path,
        intent: &WorkspaceSettlementIntent,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        let current = self.workspace_claim_status(claim)?;
        if current.phase == "verified"
            && current.settlement_id.as_deref() == Some(&intent.observation_id)
            && current.claim_revision == claim.claim_revision.saturating_add(1)
            && current.gate_revision == claim.gate_revision.saturating_add(1)
        {
            return Ok(current);
        }
        if current != *claim || claim.phase != "unsettled" {
            return Err(BridgeFault::Denied);
        }
        let mut client = self.client()?;
        if client.authority().as_str() != claim.authority_epoch {
            return Err(BridgeFault::Denied);
        }
        let prepared = client
            .call_workspace_guard(
                "orchestration/verification/prepare",
                serde_json::json!({
                    "observation_id": intent.observation_id,
                    "claim_id": claim.claim_id,
                    "claim_generation": claim.claim_generation,
                    "expected_claim_revision": claim.claim_revision,
                    "root_binding_id": claim.root_binding_id,
                    "attempt_id": intent.attempt_id,
                    "locator": workspace_root,
                    "reviewed_digest": intent.reviewed_digest
                }),
            )
            .map_err(map_read_fault)?;
        if prepared["observation_id"] != intent.observation_id
            || prepared["claim_id"] != claim.claim_id
            || prepared["claim_generation"] != claim.claim_generation
            || prepared["claim_revision"] != claim.claim_revision
            || prepared["root_binding_id"] != claim.root_binding_id
            || prepared["content_digest"] != intent.reviewed_digest
        {
            return Err(BridgeFault::Unknown);
        }
        let commit = serde_json::json!({
            "claim_id": claim.claim_id,
            "claim_generation": claim.claim_generation,
            "expected_claim_revision": claim.claim_revision,
            "observation_id": intent.observation_id,
            "attempt_id": intent.attempt_id,
            "locator": workspace_root,
            "decision": intent.decision,
            "reviewed_digest": intent.reviewed_digest
        });
        let answer = client.call_workspace_guard("orchestration/verification/commit", commit);
        match answer {
            Ok(value) => {
                let updated = workspace_claim_from_value(claim, &value)?;
                if updated.phase != "verified"
                    || updated.settlement_id.as_deref() != Some(&intent.observation_id)
                    || updated.claim_revision != claim.claim_revision.saturating_add(1)
                    || updated.gate_revision != claim.gate_revision.saturating_add(1)
                    || updated.gate_state != "stopped"
                {
                    return Err(BridgeFault::Unknown);
                }
                Ok(updated)
            }
            Err(_) => {
                // F8 may have committed and lost only its reply. Resolve the same immutable
                // observation by claim status; never create another settlement identifier.
                let updated = self.workspace_claim_status(claim)?;
                if updated.phase == "verified"
                    && updated.settlement_id.as_deref() == Some(&intent.observation_id)
                    && updated.claim_revision == claim.claim_revision.saturating_add(1)
                    && updated.gate_revision == claim.gate_revision.saturating_add(1)
                    && updated.gate_state == "stopped"
                {
                    Ok(updated)
                } else {
                    Err(BridgeFault::Unknown)
                }
            }
        }
    }

    fn acknowledge_workspace_settlement(
        &self,
        claim: &WorkspaceClaimBinding,
        gate: &str,
        coordinator_revision: u64,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        if claim.phase != "verified"
            || claim.settlement_id.is_none()
            || coordinator_revision == 0
            || coordinator_revision <= claim.coordinator_revision
            || !matches!(gate, "open" | "paused" | "stopped")
        {
            return Err(BridgeFault::Denied);
        }
        let mut client = self.client()?;
        if client.authority().as_str() != claim.authority_epoch {
            return Err(BridgeFault::Denied);
        }
        let value = client
            .call_workspace_guard(
                "orchestration/claim/gate",
                serde_json::json!({
                    "claim_id": claim.claim_id,
                    "expected_claim_revision": claim.claim_revision,
                    "expected_gate_revision": claim.gate_revision,
                    "gate": gate,
                    "settlement_id": claim.settlement_id,
                    "coordinator_revision": coordinator_revision
                }),
            )
            .map_err(map_read_fault)?;
        let updated = workspace_claim_from_value(claim, &value)?;
        if updated.phase != "held"
            || updated.gate_state != gate
            || updated.coordinator_revision != coordinator_revision
            || updated.claim_revision != claim.claim_revision.saturating_add(1)
            || updated.gate_revision != claim.gate_revision.saturating_add(1)
        {
            return Err(BridgeFault::Unknown);
        }
        Ok(updated)
    }

    fn release_workspace_claim(
        &self,
        claim: &WorkspaceClaimBinding,
        coordinator_revision: u64,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        let settlement_id = claim.settlement_id.as_deref().ok_or(BridgeFault::Denied)?;
        if !matches!(claim.phase.as_str(), "verified" | "released")
            || coordinator_revision == 0
            || (claim.phase == "verified" && coordinator_revision <= claim.coordinator_revision)
            || (claim.phase == "released" && coordinator_revision != claim.coordinator_revision)
        {
            return Err(BridgeFault::Denied);
        }
        let mut client = self.client()?;
        if client.authority().as_str() != claim.authority_epoch {
            return Err(BridgeFault::Denied);
        }
        let value = client
            .call_workspace_guard(
                "orchestration/claim/release",
                serde_json::json!({
                    "claim_id": claim.claim_id,
                    "claim_generation": claim.claim_generation,
                    "expected_claim_revision": claim.claim_revision,
                    "settlement_id": settlement_id,
                    "coordinator_revision": coordinator_revision
                }),
            )
            .map_err(map_read_fault)?;
        let updated = workspace_claim_from_value(claim, &value)?;
        if updated.phase != "released"
            || updated.gate_state != "stopped"
            || updated.settlement_id.as_deref() != Some(settlement_id)
            || updated.coordinator_revision != coordinator_revision
        {
            return Err(BridgeFault::Unknown);
        }
        Ok(updated)
    }

    fn release_unused_workspace_claim(
        &self,
        claim: &WorkspaceClaimBinding,
        coordinator_revision: u64,
    ) -> Result<WorkspaceClaimBinding, BridgeFault> {
        if claim.phase != "held"
            || claim.settlement_id.is_some()
            || coordinator_revision == 0
            || coordinator_revision <= claim.coordinator_revision
        {
            return Err(BridgeFault::Denied);
        }
        let mut client = self.client()?;
        if client.authority().as_str() != claim.authority_epoch {
            return Err(BridgeFault::Denied);
        }
        let value = client
            .call_workspace_guard(
                "orchestration/claim/release-unused",
                serde_json::json!({
                    "claim_id": claim.claim_id,
                    "claim_generation": claim.claim_generation,
                    "expected_claim_revision": claim.claim_revision,
                    "expected_gate_revision": claim.gate_revision,
                    "coordinator_revision": coordinator_revision
                }),
            )
            .map_err(map_read_fault)?;
        let updated = workspace_claim_from_value(claim, &value)?;
        if updated.phase != "released"
            || updated.gate_state != "stopped"
            || updated.settlement_id.is_some()
            || updated.coordinator_revision != coordinator_revision
        {
            return Err(BridgeFault::Unknown);
        }
        Ok(updated)
    }

    fn supports_workspace_guard_v1(&self) -> bool {
        let Ok(mut client) = self.client() else {
            return false;
        };
        client
            .call_workspace_guard("authority/capabilities", serde_json::json!({}))
            .is_ok_and(|value| value["ready"] == true)
    }

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
        let capability = client
            .call_workspace_guard("authority/capabilities", serde_json::json!({}))
            .map_err(map_read_fault)?;
        let authority_store_id = capability["authority_store_id"]
            .as_str()
            .ok_or(BridgeFault::Unknown)?
            .to_owned();
        let authority_epoch = capability["authority_epoch"]
            .as_str()
            .ok_or(BridgeFault::Unknown)?
            .to_owned();
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
        let active_lease = match client
            .call(
                Operation::LeaseActive(LeaseActive {
                    session_id: session.clone(),
                }),
                Target::None,
            )
            .map_err(map_read_fault)?
            .value
        {
            ResultValue::ActiveLease(Some(lease)) if lease.session_id == session => lease,
            ResultValue::ActiveLease(None) => return Err(BridgeFault::Denied),
            _ => return Err(BridgeFault::Unknown),
        };
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
            authority_store_id,
            authority_epoch,
            attachment_revision: session_binding.binding_revision,
            lease_id: active_lease.lease_id.as_str().to_owned(),
            lease_revision: active_lease.revision,
            workspace_root: PathBuf::from(session_binding.workspace_root),
        })
    }

    fn start_guarded(
        &self,
        admission: &Admission,
        context: &GuardedStartContext,
        intent_id: &str,
        prompt: &str,
        workspace_root: &Path,
        workspace_id: &str,
        expected_workspace_revision: &str,
    ) -> Result<StartedTurn, BridgeFault> {
        if prompt.is_empty()
            || prompt.len() > 8192
            || workspace_root != admission.workspace_root
            || workspace_id
                != workspace_identity(workspace_root).map_err(|_| BridgeFault::Denied)?
            || expected_workspace_revision
                != workspace_revision(workspace_root).map_err(|_| BridgeFault::Denied)?
            || context.claim.authority_store_id != admission.authority_store_id
            || context.claim.authority_epoch != admission.authority_epoch
            || context.claim.claim_revision == 0
            || context.claim.gate_revision == 0
            || context.claim.root_observation_revision == 0
            || context.claim.phase != "held"
            || context.claim.gate_state != "open"
        {
            return Err(BridgeFault::Denied);
        }
        let effect_class = match context.effect {
            EffectClass::ManagedInference => {
                crate::f8::guard_protocol::StartEffect::ManagedInference
            }
            EffectClass::ManagedLocalTools => {
                crate::f8::guard_protocol::StartEffect::ManagedLocalTools
            }
            _ => return Err(BridgeFault::Denied),
        };
        let model_profile = match effect_class {
            crate::f8::guard_protocol::StartEffect::ManagedInference => {
                crate::f8::guard_protocol::StartProfile::CodexInferenceV1
            }
            crate::f8::guard_protocol::StartEffect::ManagedLocalTools => {
                crate::f8::guard_protocol::StartProfile::CodexLocalToolsV1
            }
        };
        let purpose = if context.handoff_receiver {
            crate::f8::guard_protocol::StartPurpose::HandoffFreshReceiver
        } else {
            crate::f8::guard_protocol::StartPurpose::PipelineStep
        };
        let tuple = crate::f8::guard_protocol::StartTuple {
            authority_store_id: context.claim.authority_store_id.clone(),
            authority_epoch: context.claim.authority_epoch.clone(),
            coordinator_store_id: context.coordinator_store_id.clone(),
            run_id: context.run_id.clone(),
            plan_revision: context.plan_revision,
            plan_digest: context.plan_digest.clone(),
            step_id: context.step_id.clone(),
            attempt_id: context.attempt_id.clone(),
            intent_id: intent_id.to_owned(),
            effect_class,
            prompt_digest: format!("{:x}", sha2::Sha256::digest(prompt.as_bytes())),
            model_id: context.model_id.clone(),
            model_profile,
            root_binding_id: context.claim.root_binding_id.clone(),
            claim_id: context.claim.claim_id.clone(),
            claim_generation: context.claim.claim_generation,
            claim_revision: context.claim.claim_revision,
            root_observation_revision: context.claim.root_observation_revision,
            dispatch_gate_revision: context.claim.gate_revision,
            session_id: admission.session_id.clone(),
            session_revision: admission.session_revision,
            attachment_revision: admission.attachment_revision,
            runtime_instance_id: admission.runtime_id.clone(),
            runtime_generation: admission.generation,
            lease_id: admission.lease_id.clone(),
            lease_revision: admission.lease_revision,
            approval_reference: None,
            purpose,
        };
        let tuple_digest = tuple.digest().map_err(|_| BridgeFault::Denied)?;
        let mut client = self.client()?;
        if client.authority().as_str() != admission.authority_id
            || client.authority().as_str() != context.claim.authority_epoch
        {
            return Err(BridgeFault::Denied);
        }
        let prepared = client
            .call_workspace_guard(
                "orchestration/start/prepare",
                serde_json::json!({"tuple": tuple}),
            )
            .map_err(map_effect_fault)?;
        if prepared["tuple_digest"] != tuple_digest {
            return Err(BridgeFault::Unknown);
        }
        let receipt_id = prepared["receipt_id"]
            .as_str()
            .ok_or(BridgeFault::Unknown)?
            .to_owned();
        let receipt_revision = prepared["receipt_revision"]
            .as_u64()
            .ok_or(BridgeFault::Unknown)?;
        let committed = client
            .call_workspace_guard(
                "orchestration/start/commit",
                serde_json::json!({
                    "tuple": tuple,
                    "receipt_id": receipt_id,
                    "receipt_revision": receipt_revision,
                    "prompt": prompt
                }),
            )
            .map_err(map_effect_fault)?;
        if committed["kind"] != "turn" || committed["value"].as_object().is_none() {
            return Err(BridgeFault::Unknown);
        }
        Ok(StartedTurn {
            turn_id: committed["value"]["managed_turn_id"]
                .as_str()
                .ok_or(BridgeFault::Unknown)?
                .to_owned(),
            revision: committed["value"]["revision"]
                .as_u64()
                .ok_or(BridgeFault::Unknown)?,
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

fn workspace_claim_from_value(
    original: &WorkspaceClaimBinding,
    value: &serde_json::Value,
) -> Result<WorkspaceClaimBinding, BridgeFault> {
    if value["claim_id"] != original.claim_id
        || value["root_binding_id"] != original.root_binding_id
        || value["claim_generation"] != original.claim_generation
    {
        return Err(BridgeFault::Unknown);
    }
    let phase = value["phase"].as_str().ok_or(BridgeFault::Unknown)?;
    let gate_state = value["gate_state"].as_str().ok_or(BridgeFault::Unknown)?;
    let settlement_id = match &value["settlement_id"] {
        serde_json::Value::Null => None,
        serde_json::Value::String(value) => Some(value.clone()),
        _ => return Err(BridgeFault::Unknown),
    };
    Ok(WorkspaceClaimBinding {
        claim_revision: value["claim_revision"]
            .as_u64()
            .ok_or(BridgeFault::Unknown)?,
        gate_revision: value["gate_revision"]
            .as_u64()
            .ok_or(BridgeFault::Unknown)?,
        phase: phase.to_owned(),
        gate_state: gate_state.to_owned(),
        settlement_id,
        coordinator_revision: value["coordinator_revision"]
            .as_u64()
            .ok_or(BridgeFault::Unknown)?,
        ..original.clone()
    })
}

struct ActivePlan {
    plan: OrchestrationPlanV1,
    canonical_root: PathBuf,
    handoff_receiver: bool,
}

pub struct Coordinator {
    journal: Arc<RunJournal>,
    authority: Arc<dyn ManagedAuthority>,
    coordinator_store_id: String,
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
        let recovered = journal
            .recover_after_restart(now)
            .map_err(|_| CoordinatorError::JournalUnavailable)?;
        let coordinator_store_id = journal
            .coordinator_store_id()
            .map_err(|_| CoordinatorError::JournalUnavailable)?;
        let import_snapshot = journal
            .import_snapshot()
            .map_err(|_| CoordinatorError::JournalUnavailable)?;
        // Initialization is deliberately retriable. Until the F8 import fence confirms this
        // exact immutable snapshot, capability negotiation remains false and no turn can start.
        let guard_initialized = authority
            .initialize_workspace_guard(&coordinator_store_id, &import_snapshot)
            .is_ok();
        let coordinator = Self {
            journal,
            authority,
            coordinator_store_id,
            plans: Mutex::new(HashMap::new()),
            admission: Mutex::new(()),
            started: Mutex::new(HashMap::new()),
            round_robin: Mutex::new(0),
        };
        // If restart recovery durably cancelled a run with no persisted attempts, let F8
        // release only the held claim for which it independently proves no receipt or attempt
        // exists. A failed or ambiguous release remains in the journal for an idempotent retry.
        if guard_initialized {
            for run in recovered {
                let _ = coordinator.release_terminal_unused_claim(&run);
            }
        }
        Ok(coordinator)
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
                let current = std::fs::canonicalize(&active.plan.workspace_root)
                    .map_err(|_| CoordinatorError::WorkspaceConflict)?;
                if current != active.canonical_root {
                    // A symlink alias moved since its claim was admitted. Do not
                    // mistake its new target for the original protected workspace.
                    return Err(CoordinatorError::WorkspaceConflict);
                }
                if canonical.starts_with(&active.canonical_root)
                    || active.canonical_root.starts_with(&canonical)
                {
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
        if self.authority.supports_workspace_guard_v1() {
            let binding = self
                .authority
                .acquire_workspace_claim(
                    &self.coordinator_store_id,
                    &run.run_id,
                    plan.revision,
                    &review.digest,
                    Path::new(&plan.workspace_root),
                )
                .map_err(|fault| match fault {
                    BridgeFault::Busy => CoordinatorError::ResourceBusy,
                    BridgeFault::Denied => CoordinatorError::WorkspaceConflict,
                    BridgeFault::Unavailable | BridgeFault::Unknown => {
                        CoordinatorError::AuthorityUnavailable
                    }
                })?;
            self.journal
                .bind_workspace_claim(&run.run_id, &binding)
                .map_err(map_journal)?;
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
                    canonical_root: canonical,
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
        self.set_guarded_gate(run_id, "paused")?;
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
        self.set_guarded_gate(run_id, "open")?;
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
            let expired = self
                .journal
                .set_run_state(
                    &current.run_id,
                    current.revision,
                    RunState::Expired,
                    Some(FailureCode::PlanExpired),
                    now,
                )
                .map_err(map_journal)?;
            self.release_terminal_unused_claim(&expired)?;
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
            // F8's gate CAS is the stop linearization point. A delayed scheduler tick may
            // still observe Running locally, but F8 will reject its guarded reservation.
            self.set_guarded_gate(run_id, "stopped")?;
            self.journal
                .set_run_state(run_id, run.revision, RunState::Cancelling, None, now)
                .map_err(map_journal)?;
        } else {
            self.set_guarded_gate(run_id, "stopped")?;
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
        let (plan, claimed_root) = self
            .plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(run_id)
            .map(|p| (p.plan.clone(), p.canonical_root.clone()))
            .ok_or(CoordinatorError::InvalidTransition)?;
        let current_root = std::fs::canonicalize(&plan.workspace_root)
            .map_err(|_| CoordinatorError::WorkspaceConflict)?;
        if current_root != claimed_root
            || workspace_identity(&current_root).map_err(|_| CoordinatorError::WorkspaceConflict)?
                != plan.workspace_id
        {
            return Err(CoordinatorError::WorkspaceConflict);
        }
        let actual =
            workspace_revision(&current_root).map_err(|_| CoordinatorError::WorkspaceConflict)?;
        if actual != reviewed_workspace_revision {
            return Err(CoordinatorError::WorkspaceConflict);
        }
        let run = self.get(run_id)?;
        let step = run
            .steps
            .iter()
            .find(|step| step.step_id == step_id)
            .ok_or(CoordinatorError::UnknownRun)?;
        if step.state != StepState::Verifying {
            return Err(CoordinatorError::InvalidTransition);
        }
        self.settle_step_with_authority(
            run_id,
            step_id,
            StepState::Verifying,
            StepState::Succeeded,
            None,
            &current_root,
            &actual,
            ReconcileDecision::ConfirmedCompleted,
            now,
        )?;
        self.finalize_workspace_settlement(run_id, now)
    }

    #[allow(clippy::too_many_arguments)]
    fn settle_step_with_authority(
        &self,
        run_id: &str,
        step_id: &str,
        expected_step_state: StepState,
        next_step_state: StepState,
        failure: Option<FailureCode>,
        workspace_root: &Path,
        reviewed_digest: &str,
        decision: ReconcileDecision,
        now: u64,
    ) -> Result<CoordinatorSnapshot, CoordinatorError> {
        let mut claim = self
            .journal
            .workspace_claim_binding(run_id)
            .map_err(map_journal)?
            .ok_or(CoordinatorError::AuthorityUnavailable)?;
        let observed = self
            .authority
            .workspace_claim_status(&claim)
            .map_err(|_| CoordinatorError::AuthorityUnavailable)?;
        if observed != claim {
            // F8 may have sealed a drifted locator/topology independently of M6.
            // Mirror only the same claim's closed gate CAS; this grants no dispatch.
            if observed.phase != claim.phase || observed.gate_state != "stopped" {
                return Err(CoordinatorError::WorkspaceConflict);
            }
            self.journal
                .update_workspace_claim_gate(run_id, &claim, &observed)
                .map_err(map_journal)?;
            claim = observed;
        }
        if claim.phase != "unsettled" {
            return Err(CoordinatorError::InvalidTransition);
        }
        let run = self.get(run_id)?;
        let step = run
            .steps
            .iter()
            .find(|step| step.step_id == step_id)
            .ok_or(CoordinatorError::UnknownRun)?;
        let attempt_id = step
            .attempt_id
            .as_deref()
            .ok_or(CoordinatorError::InvalidTransition)?;
        let decision_text = match decision {
            ReconcileDecision::ConfirmedCompleted => "confirmed_completed",
            ReconcileDecision::ConfirmedNoEffect => "confirmed_no_effect",
            ReconcileDecision::ConfirmedCancelled => "confirmed_cancelled",
            ReconcileDecision::Unknown => return Err(CoordinatorError::InvalidTransition),
        };
        let intent = match self
            .journal
            .workspace_settlement_intent(run_id)
            .map_err(map_journal)?
        {
            Some(intent)
                if intent.attempt_id == attempt_id
                    && intent.decision == decision_text
                    && intent.reviewed_digest == reviewed_digest =>
            {
                intent
            }
            Some(_) => return Err(CoordinatorError::ReviewStale),
            None => WorkspaceSettlementIntent {
                observation_id: Uuid::new_v4().to_string(),
                attempt_id: attempt_id.to_owned(),
                decision: decision_text.to_owned(),
                reviewed_digest: reviewed_digest.to_owned(),
            },
        };
        self.journal
            .begin_workspace_settlement(run_id, step_id, expected_step_state, &claim, &intent)
            .map_err(map_journal)?;
        let verified = self
            .authority
            .settle_workspace_claim(&claim, workspace_root, &intent)
            .map_err(|fault| match fault {
                BridgeFault::Busy => CoordinatorError::ResourceBusy,
                BridgeFault::Denied => CoordinatorError::WorkspaceConflict,
                BridgeFault::Unavailable | BridgeFault::Unknown => {
                    CoordinatorError::AuthorityUnavailable
                }
            })?;
        self.journal
            .settle_workspace_step(
                run_id,
                step_id,
                expected_step_state,
                next_step_state,
                failure,
                &claim,
                &verified,
                &intent,
                now,
            )
            .map_err(map_journal)
    }

    fn finalize_workspace_settlement(
        &self,
        run_id: &str,
        now: u64,
    ) -> Result<CoordinatorSnapshot, CoordinatorError> {
        let mut run = self.get(run_id)?;
        if run.state == RunState::NeedsReview {
            run = self.close_reconciled_run(run_id, now)?;
        } else if run.state == RunState::Pausing
            && !run.steps.iter().any(|step| {
                matches!(
                    step.state,
                    StepState::Starting | StepState::Running | StepState::Verifying
                )
            })
        {
            run = self
                .journal
                .set_run_state(run_id, run.revision, RunState::Paused, None, now)
                .map_err(map_journal)?;
        } else if run.state == RunState::Cancelling {
            run = self.cancel_unstarted(run_id, now)?;
        } else {
            self.finish_if_done(run_id, now)?;
            run = self.get(run_id)?;
        }

        let Some(claim) = self
            .journal
            .workspace_claim_binding(run_id)
            .map_err(map_journal)?
        else {
            return Ok(run);
        };
        if claim.phase != "verified" {
            return Ok(run);
        }
        if run.state.terminal() {
            let released = self
                .authority
                .release_workspace_claim(&claim, run.revision)
                .map_err(|_| CoordinatorError::AuthorityUnavailable)?;
            self.journal
                .record_workspace_claim_release(run_id, &claim, &released, run.revision)
                .map_err(map_journal)?;
        } else {
            let gate = match run.state {
                RunState::Paused => "paused",
                RunState::Pausing | RunState::Cancelling | RunState::NeedsReview => "stopped",
                _ => "open",
            };
            let acknowledged = self
                .authority
                .acknowledge_workspace_settlement(&claim, gate, run.revision)
                .map_err(|_| CoordinatorError::AuthorityUnavailable)?;
            self.journal
                .update_workspace_claim_gate(run_id, &claim, &acknowledged)
                .map_err(map_journal)?;
        }
        self.get(run_id)
    }

    fn release_terminal_unused_claim(
        &self,
        run: &CoordinatorSnapshot,
    ) -> Result<(), CoordinatorError> {
        if !run.state.terminal() || run.steps.iter().any(|step| step.attempt_id.is_some()) {
            return Ok(());
        }
        let Some(claim) = self
            .journal
            .workspace_claim_binding(&run.run_id)
            .map_err(map_journal)?
        else {
            return Ok(());
        };
        if claim.phase != "held" || claim.settlement_id.is_some() {
            return Ok(());
        }
        let released = self
            .authority
            .release_unused_workspace_claim(&claim, run.revision)
            .map_err(|_| CoordinatorError::AuthorityUnavailable)?;
        self.journal
            .record_workspace_claim_release(&run.run_id, &claim, &released, run.revision)
            .map_err(map_journal)
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
                let root = workspace_root
                    .map(PathBuf::from)
                    .or_else(|| plan.map(|plan| PathBuf::from(plan.workspace_root)))
                    .ok_or(CoordinatorError::WorkspaceConflict)?;
                let actual =
                    workspace_revision(&root).map_err(|_| CoordinatorError::WorkspaceConflict)?;
                if actual != expected {
                    return Err(CoordinatorError::WorkspaceConflict);
                }
                self.settle_step_with_authority(
                    run_id,
                    step_id,
                    StepState::NeedsReview,
                    StepState::Succeeded,
                    None,
                    &root,
                    &actual,
                    decision,
                    now,
                )?;
                self.finalize_workspace_settlement(run_id, now)
            }
            ReconcileDecision::ConfirmedNoEffect => {
                let plan = self
                    .plans
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(run_id)
                    .map(|p| p.plan.clone());
                let root = workspace_root
                    .map(PathBuf::from)
                    .or_else(|| plan.map(|plan| PathBuf::from(plan.workspace_root)))
                    .ok_or(CoordinatorError::WorkspaceConflict)?;
                let actual =
                    workspace_revision(&root).map_err(|_| CoordinatorError::WorkspaceConflict)?;
                self.settle_step_with_authority(
                    run_id,
                    step_id,
                    StepState::NeedsReview,
                    StepState::Failed,
                    Some(FailureCode::OwnerReviewRequired),
                    &root,
                    &actual,
                    decision,
                    now,
                )?;
                self.finalize_workspace_settlement(run_id, now)
            }
            ReconcileDecision::ConfirmedCancelled => {
                let plan = self
                    .plans
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(run_id)
                    .map(|p| p.plan.clone());
                let root = workspace_root
                    .map(PathBuf::from)
                    .or_else(|| plan.map(|plan| PathBuf::from(plan.workspace_root)))
                    .ok_or(CoordinatorError::WorkspaceConflict)?;
                let expected = reviewed_workspace_revision.ok_or(CoordinatorError::ReviewStale)?;
                let actual =
                    workspace_revision(&root).map_err(|_| CoordinatorError::WorkspaceConflict)?;
                if actual != expected {
                    return Err(CoordinatorError::WorkspaceConflict);
                }
                self.settle_step_with_authority(
                    run_id,
                    step_id,
                    StepState::NeedsReview,
                    StepState::Cancelled,
                    Some(FailureCode::OwnerReviewRequired),
                    &root,
                    &actual,
                    decision,
                    now,
                )?;
                self.finalize_workspace_settlement(run_id, now)
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

    /// Keep the durable F8 gate authoritative. The local journal is only a CAS mirror; if F8
    /// changes and its response is lost, the stale local revision prevents later dispatch.
    fn set_guarded_gate(&self, run_id: &str, gate: &str) -> Result<(), CoordinatorError> {
        let Some(expected) = self
            .journal
            .workspace_claim_binding(run_id)
            .map_err(map_journal)?
        else {
            // No F8-owned claim means this run was never admitted through guarded Managed
            // execution. Its scheduler lane remains closed by the missing-claim check.
            return Ok(());
        };
        if gate == "open" && expected.phase != "held" {
            return Err(CoordinatorError::InvalidTransition);
        }
        let updated = self
            .authority
            .set_workspace_gate(&expected, gate)
            .map_err(|fault| match fault {
                BridgeFault::Busy => CoordinatorError::ResourceBusy,
                BridgeFault::Denied => CoordinatorError::InvalidTransition,
                BridgeFault::Unavailable | BridgeFault::Unknown => {
                    CoordinatorError::AuthorityUnavailable
                }
            })?;
        self.journal
            .update_workspace_claim_gate(run_id, &expected, &updated)
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
                if !self.authority.supports_workspace_guard_v1() {
                    // No ordinary TurnStart fallback. Leave the M6 claim held and the step
                    // waiting until F8 can provide the frozen atomic guard contract.
                    if row.state == StepState::Eligible {
                        self.journal
                            .set_step_state(
                                &run.run_id,
                                &step.step_id,
                                row.state,
                                StepState::WaitingResource,
                                Some(FailureCode::TrustBoundaryBlocked),
                                now,
                            )
                            .map_err(map_journal)?;
                    }
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
                let Some(claim) = self
                    .journal
                    .workspace_claim_binding(&run.run_id)
                    .map_err(map_journal)?
                else {
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
                };
                let attempt_id = Uuid::new_v4().to_string();
                let intent_id = Uuid::new_v4().to_string();
                let guarded_context = GuardedStartContext {
                    coordinator_store_id: self.coordinator_store_id.clone(),
                    run_id: run.run_id.clone(),
                    plan_revision: run.plan_revision,
                    plan_digest: run.plan_digest.clone(),
                    step_id: step.step_id.clone(),
                    attempt_id: attempt_id.clone(),
                    model_id: step.model_id.clone(),
                    effect: step.effect,
                    handoff_receiver,
                    claim: claim.clone(),
                };
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
                match self.authority.start_guarded(
                    &admission,
                    &guarded_context,
                    &intent_id,
                    &step.prompt,
                    &admission.workspace_root,
                    &run.workspace_id,
                    &run.current_workspace_revision,
                ) {
                    Ok(started) => {
                        self.journal
                            .acknowledge_guarded_attempt(
                                &run.run_id,
                                &step.step_id,
                                &attempt_id,
                                &started.turn_id,
                                started.revision,
                                &claim,
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
        if state.terminal() {
            let terminal = self.get(run_id)?;
            self.release_terminal_unused_claim(&terminal)?;
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
            let terminal = self
                .journal
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
                .map_err(map_journal)?;
            self.release_terminal_unused_claim(&terminal)?;
            Ok(terminal)
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
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
        path::Path,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    const SESSION: &str = "11111111-1111-4111-8111-111111111111";
    const SECOND_SESSION: &str = "12121212-1212-4212-8212-121212121212";
    const AUTHORITY: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";
    const RUNTIME: &str = "99999999-9999-4999-8999-999999999999";
    const TURN: &str = "88888888-8888-4888-8888-888888888888";

    struct FakeAuthority {
        root: PathBuf,
        root_pin: fs::File,
        second_root: Mutex<Option<PathBuf>>,
        second_root_pin: Mutex<Option<fs::File>>,
        starts: AtomicUsize,
        guarded_workspace: AtomicBool,
        cancels: AtomicUsize,
        inspect_fault: Mutex<Option<BridgeFault>>,
        fresh_receiver_fault: Mutex<Option<BridgeFault>>,
        source_fault: Mutex<Option<BridgeFault>>,
        start_fault: Mutex<Option<BridgeFault>>,
        cancel_fault: Mutex<Option<BridgeFault>>,
        observation: Mutex<TurnObservation>,
        settlement_evidence: Mutex<Option<ReconcileDecision>>,
        claims: Mutex<HashMap<String, WorkspaceClaimBinding>>,
    }
    impl FakeAuthority {
        fn new(root: PathBuf) -> Self {
            Self {
                root_pin: fs::File::open(&root).unwrap(),
                root,
                second_root: Mutex::new(None),
                second_root_pin: Mutex::new(None),
                starts: AtomicUsize::new(0),
                guarded_workspace: AtomicBool::new(true),
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
                settlement_evidence: Mutex::new(None),
                claims: Mutex::new(HashMap::new()),
            }
        }

        fn authorize_settlement(&self, decision: ReconcileDecision) {
            *self.settlement_evidence.lock().unwrap() = Some(decision);
        }

        fn set_second_root(&self, path: PathBuf) {
            *self.second_root.lock().unwrap() = Some(path.clone());
            *self.second_root_pin.lock().unwrap() = Some(fs::File::open(path).unwrap());
        }

        fn pin_matches(&self, path: &Path) -> Result<(), BridgeFault> {
            let current = fs::metadata(path).map_err(|_| BridgeFault::Unknown)?;
            let primary_matches = self
                .root_pin
                .metadata()
                .is_ok_and(|pinned| pinned.dev() == current.dev() && pinned.ino() == current.ino());
            let secondary_matches = self
                .second_root_pin
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|pin| pin.metadata().ok())
                .is_some_and(|pinned| {
                    pinned.dev() == current.dev() && pinned.ino() == current.ino()
                });
            if primary_matches || secondary_matches {
                Ok(())
            } else {
                Err(BridgeFault::Denied)
            }
        }
    }
    impl ManagedAuthority for FakeAuthority {
        fn initialize_workspace_guard(
            &self,
            _coordinator_store_id: &str,
            _claims: &[WorkspaceImportClaim],
        ) -> Result<(), BridgeFault> {
            Ok(())
        }

        fn acquire_workspace_claim(
            &self,
            _coordinator_store_id: &str,
            _run_id: &str,
            _plan_revision: u64,
            _plan_digest: &str,
            workspace_root: &Path,
        ) -> Result<WorkspaceClaimBinding, BridgeFault> {
            self.pin_matches(workspace_root)?;
            let binding = WorkspaceClaimBinding {
                authority_store_id: Uuid::new_v4().to_string(),
                authority_epoch: Uuid::new_v4().to_string(),
                root_binding_id: Uuid::new_v4().to_string(),
                claim_id: Uuid::new_v4().to_string(),
                claim_generation: 1,
                claim_revision: 1,
                gate_revision: 1,
                root_observation_revision: 1,
                phase: "held".into(),
                gate_state: "open".into(),
                settlement_id: None,
                coordinator_revision: 0,
            };
            self.claims
                .lock()
                .unwrap()
                .insert(binding.claim_id.clone(), binding.clone());
            Ok(binding)
        }

        fn set_workspace_gate(
            &self,
            claim: &WorkspaceClaimBinding,
            gate: &str,
        ) -> Result<WorkspaceClaimBinding, BridgeFault> {
            if !matches!(gate, "open" | "paused" | "stopped")
                || matches!(claim.phase.as_str(), "released" | "quarantined")
                || (gate == "open" && claim.phase != "held")
            {
                return Err(BridgeFault::Denied);
            }
            let mut updated = claim.clone();
            if updated.gate_state != gate {
                updated.claim_revision += 1;
                updated.gate_revision += 1;
                updated.gate_state = gate.to_owned();
            }
            self.claims
                .lock()
                .unwrap()
                .insert(updated.claim_id.clone(), updated.clone());
            Ok(updated)
        }

        fn workspace_claim_status(
            &self,
            claim: &WorkspaceClaimBinding,
        ) -> Result<WorkspaceClaimBinding, BridgeFault> {
            self.claims
                .lock()
                .unwrap()
                .get(&claim.claim_id)
                .cloned()
                .ok_or(BridgeFault::Unknown)
        }

        fn settle_workspace_claim(
            &self,
            claim: &WorkspaceClaimBinding,
            workspace_root: &Path,
            intent: &WorkspaceSettlementIntent,
        ) -> Result<WorkspaceClaimBinding, BridgeFault> {
            self.pin_matches(workspace_root)?;
            if workspace_revision(workspace_root).map_err(|_| BridgeFault::Denied)?
                != intent.reviewed_digest
            {
                return Err(BridgeFault::Denied);
            }
            let expected_decision = match intent.decision.as_str() {
                "confirmed_completed" => ReconcileDecision::ConfirmedCompleted,
                "confirmed_no_effect" => ReconcileDecision::ConfirmedNoEffect,
                "confirmed_cancelled" => ReconcileDecision::ConfirmedCancelled,
                _ => return Err(BridgeFault::Denied),
            };
            if self.settlement_evidence.lock().unwrap().take() != Some(expected_decision) {
                return Err(BridgeFault::Denied);
            }
            let mut claims = self.claims.lock().unwrap();
            let current = claims.get(&claim.claim_id).ok_or(BridgeFault::Unknown)?;
            if current != claim || claim.phase != "unsettled" {
                return Err(BridgeFault::Denied);
            }
            let mut updated = claim.clone();
            updated.phase = "verified".to_owned();
            updated.gate_state = "stopped".to_owned();
            updated.claim_revision += 1;
            updated.gate_revision += 1;
            updated.settlement_id = Some(intent.observation_id.clone());
            claims.insert(updated.claim_id.clone(), updated.clone());
            Ok(updated)
        }

        fn acknowledge_workspace_settlement(
            &self,
            claim: &WorkspaceClaimBinding,
            gate: &str,
            coordinator_revision: u64,
        ) -> Result<WorkspaceClaimBinding, BridgeFault> {
            if claim.phase != "verified" || coordinator_revision <= claim.coordinator_revision {
                return Err(BridgeFault::Denied);
            }
            let mut claims = self.claims.lock().unwrap();
            if claims.get(&claim.claim_id) != Some(claim) {
                return Err(BridgeFault::Denied);
            }
            let mut updated = claim.clone();
            updated.phase = "held".to_owned();
            updated.gate_state = gate.to_owned();
            updated.coordinator_revision = coordinator_revision;
            updated.claim_revision += 1;
            updated.gate_revision += 1;
            claims.insert(updated.claim_id.clone(), updated.clone());
            Ok(updated)
        }

        fn release_workspace_claim(
            &self,
            claim: &WorkspaceClaimBinding,
            coordinator_revision: u64,
        ) -> Result<WorkspaceClaimBinding, BridgeFault> {
            let mut claims = self.claims.lock().unwrap();
            let current = claims.get(&claim.claim_id).ok_or(BridgeFault::Unknown)?;
            if current.phase == "released" && current.coordinator_revision == coordinator_revision {
                return Ok(current.clone());
            }
            if current != claim
                || claim.phase != "verified"
                || coordinator_revision <= claim.coordinator_revision
                || claim.settlement_id.is_none()
            {
                return Err(BridgeFault::Denied);
            }
            let mut updated = claim.clone();
            updated.phase = "released".to_owned();
            updated.gate_state = "stopped".to_owned();
            updated.coordinator_revision = coordinator_revision;
            updated.claim_revision += 1;
            updated.gate_revision += 1;
            claims.insert(updated.claim_id.clone(), updated.clone());
            Ok(updated)
        }

        fn release_unused_workspace_claim(
            &self,
            claim: &WorkspaceClaimBinding,
            coordinator_revision: u64,
        ) -> Result<WorkspaceClaimBinding, BridgeFault> {
            if claim.phase != "held"
                || claim.settlement_id.is_some()
                || coordinator_revision <= claim.coordinator_revision
            {
                return Err(BridgeFault::Denied);
            }
            let mut claims = self.claims.lock().unwrap();
            if claims.get(&claim.claim_id) != Some(claim) {
                return Err(BridgeFault::Denied);
            }
            let mut updated = claim.clone();
            updated.phase = "released".to_owned();
            updated.gate_state = "stopped".to_owned();
            updated.coordinator_revision = coordinator_revision;
            updated.claim_revision += 1;
            updated.gate_revision += 1;
            claims.insert(updated.claim_id.clone(), updated.clone());
            Ok(updated)
        }

        fn supports_workspace_guard_v1(&self) -> bool {
            self.guarded_workspace.load(Ordering::SeqCst)
        }

        fn start_guarded(
            &self,
            admission: &Admission,
            context: &GuardedStartContext,
            intent_id: &str,
            prompt: &str,
            workspace_root: &Path,
            workspace_id: &str,
            expected_workspace_revision: &str,
        ) -> Result<StartedTurn, BridgeFault> {
            self.pin_matches(workspace_root)?;
            if workspace_id
                != workspace_identity(workspace_root).map_err(|_| BridgeFault::Denied)?
                || expected_workspace_revision
                    != workspace_revision(workspace_root).map_err(|_| BridgeFault::Denied)?
                || admission.workspace_root != workspace_root
            {
                return Err(BridgeFault::Denied);
            }
            let started = self.start(admission, intent_id, prompt)?;
            let mut claim = context.claim.clone();
            if claim.phase != "held" || claim.gate_state != "open" {
                return Err(BridgeFault::Denied);
            }
            claim.phase = "unsettled".to_owned();
            claim.claim_revision += 1;
            self.claims
                .lock()
                .unwrap()
                .insert(claim.claim_id.clone(), claim);
            Ok(started)
        }

        fn verify_workspace_settlement(
            &self,
            workspace_root: &Path,
            decision: ReconcileDecision,
        ) -> Result<(), BridgeFault> {
            self.pin_matches(workspace_root)?;
            let mut evidence = self.settlement_evidence.lock().unwrap();
            if *evidence == Some(decision) {
                *evidence = None;
                Ok(())
            } else {
                Err(BridgeFault::Unknown)
            }
        }

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
                authority_store_id: Uuid::new_v4().to_string(),
                authority_epoch: AUTHORITY.into(),
                attachment_revision: 1,
                lease_id: Uuid::new_v4().to_string(),
                lease_revision: 1,
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
    fn guarded_pause_and_uncertain_stop_keep_authority_claim_closed() {
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

        coordinator.pause(&run.run_id, 101).unwrap();
        let paused = journal
            .workspace_claim_binding(&run.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            (paused.phase.as_str(), paused.gate_state.as_str()),
            ("held", "paused")
        );
        // An exact gate replay must be idempotent and release the journal mutex before
        // consulting the persisted binding on the zero-row CAS path.
        coordinator.set_guarded_gate(&run.run_id, "paused").unwrap();
        coordinator.resume(&run.run_id, 102).unwrap();
        coordinator.tick(103).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        let started = journal
            .workspace_claim_binding(&run.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            (started.phase.as_str(), started.gate_state.as_str()),
            ("unsettled", "open")
        );

        *authority.cancel_fault.lock().unwrap() = Some(BridgeFault::Unknown);
        assert_eq!(
            coordinator.stop(&run.run_id, 104).unwrap().state,
            RunState::NeedsReview
        );
        let stopped = journal
            .workspace_claim_binding(&run.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            (stopped.phase.as_str(), stopped.gate_state.as_str()),
            ("unsettled", "stopped")
        );
        coordinator.tick(105).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            journal.workspace_claim_binding(&run.run_id).unwrap(),
            Some(stopped)
        );
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
        assert!(journal.claimed_workspaces().unwrap().is_empty());

        // A separate run that never persisted an attempt is cancelled after a coordinator
        // restart; its unused F8 claim is released only through F8's no-attempt guard.
        let second_draft = plan(
            &root,
            vec![step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![])],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        let second = coordinator
            .submit(second_draft.clone(), review(&second_draft), 100)
            .unwrap();
        coordinator.pause(&second.run_id, 111).unwrap();
        let restarted = Coordinator::new(journal.clone(), authority.clone(), 112).unwrap();
        assert_eq!(
            restarted.get(&second.run_id).unwrap().state,
            RunState::Cancelled
        );
        assert_eq!(
            restarted.resume(&second.run_id, 113).unwrap_err(),
            CoordinatorError::InvalidTransition
        );
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        assert!(journal.claimed_workspaces().unwrap().is_empty());
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
        authority.set_second_root(root_b.clone());

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
        authority.set_second_root(root_b.clone());

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
        authority.set_second_root(root_b.clone());
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
        authority.authorize_settlement(ReconcileDecision::ConfirmedCompleted);
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
        authority.set_second_root(root_b.clone());
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
    fn moved_workspace_symlink_cannot_lose_original_parent_claim() {
        let original = root();
        let another = root();
        let child = original.join("child");
        fs::create_dir(&child).unwrap();
        let link = std::env::temp_dir().join(format!("wyss-orch-alias-{}", Uuid::new_v4()));
        symlink(&original, &link).unwrap();
        let (coordinator, _journal, authority) = coordinator(&original);
        authority.set_second_root(child.clone());
        let alias_plan = plan(
            &link,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let accepted = coordinator
            .submit(alias_plan.clone(), review(&alias_plan), 100)
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
        fs::remove_file(&link).unwrap();
        symlink(&another, &link).unwrap();
        let mut nested_step = step("cccccccc-cccc-4ccc-8ccc-cccccccccccc", vec![]);
        nested_step.managed_session_id = Some(SECOND_SESSION.into());
        let nested = plan(
            &child,
            vec![nested_step],
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        );
        assert_eq!(
            coordinator
                .submit(nested.clone(), review(&nested), 103)
                .unwrap_err(),
            CoordinatorError::WorkspaceConflict
        );
        assert_eq!(
            coordinator
                .verify_step(
                    &accepted.run_id,
                    &alias_plan.steps[0].step_id,
                    &workspace_revision(&another).unwrap(),
                    104,
                )
                .unwrap_err(),
            CoordinatorError::WorkspaceConflict
        );
        assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
        fs::remove_file(link).unwrap();
        fs::remove_dir_all(original).unwrap();
        fs::remove_dir_all(another).unwrap();
    }

    #[test]
    fn restart_cannot_reconcile_equal_content_replacement_without_live_f8_identity() {
        let root = root();
        let moved = root.with_extension("moved");
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
        let step_id = &draft.steps[0].step_id;
        let attempt = coordinator.get(&run.run_id).unwrap().steps[0]
            .attempt_id
            .clone()
            .unwrap();
        journal
            .mark_attempt_unknown(&run.run_id, step_id, &attempt, 102)
            .unwrap();
        authority.authorize_settlement(ReconcileDecision::ConfirmedCompleted);
        let reviewed_revision = workspace_revision(&root).unwrap();
        drop(coordinator);

        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::metadata(&moved).unwrap().permissions()).unwrap();
        let restarted = Coordinator::new(journal.clone(), authority, 103).unwrap();
        let result = restarted.reconcile(
            &run.run_id,
            step_id,
            ReconcileDecision::ConfirmedCompleted,
            Some(root.to_str().unwrap()),
            Some(&reviewed_revision),
            104,
        );
        let claims = journal.claimed_workspaces().unwrap();
        let blocked_and_claimed = result.is_err() && claims.iter().any(|(id, _)| id == &run.run_id);

        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&moved).unwrap();
        assert!(
            blocked_and_claimed,
            "reconciliation must retain the unresolved claim unless live F8 proves the original root"
        );
    }

    #[test]
    fn production_f8_guard_capability_stays_closed_without_atomic_protocol_support() {
        let root = root();
        let authority = F8Authority::new(root.join("runtime"));
        assert!(!authority.supports_workspace_guard_v1());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_workspace_guard_blocks_before_attempt_and_retains_claim() {
        let root = root();
        let (coordinator, journal, authority) = coordinator(&root);
        authority.guarded_workspace.store(false, Ordering::SeqCst);
        let draft = plan(
            &root,
            vec![step("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", vec![])],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        let accepted = coordinator
            .submit(draft.clone(), review(&draft), 100)
            .unwrap();
        coordinator.tick(101).unwrap();

        let blocked = coordinator.get(&accepted.run_id).unwrap();
        assert_eq!(authority.starts.load(Ordering::SeqCst), 0);
        assert_eq!(blocked.steps[0].state, StepState::WaitingResource);
        assert_eq!(
            blocked.steps[0].failure,
            Some(FailureCode::TrustBoundaryBlocked)
        );
        assert!(blocked.steps[0].attempt_id.is_none());
        assert_eq!(journal.claimed_workspaces().unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn owner_confirmation_without_authority_evidence_never_releases_ambiguous_claim() {
        for decision in [
            ReconcileDecision::ConfirmedCompleted,
            ReconcileDecision::ConfirmedNoEffect,
            ReconcileDecision::ConfirmedCancelled,
        ] {
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
            coordinator.tick(101).unwrap();
            let attempt = coordinator.get(&accepted.run_id).unwrap().steps[0]
                .attempt_id
                .clone()
                .unwrap();
            journal
                .mark_attempt_unknown(&accepted.run_id, &draft.steps[0].step_id, &attempt, 102)
                .unwrap();

            let result = coordinator.reconcile(
                &accepted.run_id,
                &draft.steps[0].step_id,
                decision,
                Some(&draft.workspace_root),
                Some(&workspace_revision(&root).unwrap()),
                103,
            );
            assert_eq!(result.unwrap_err(), CoordinatorError::WorkspaceConflict);
            assert_eq!(journal.claimed_workspaces().unwrap().len(), 1);
            assert_eq!(
                coordinator.get(&accepted.run_id).unwrap().steps[0].state,
                StepState::NeedsReview
            );
            assert_eq!(authority.starts.load(Ordering::SeqCst), 1);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn parent_child_workspace_claims_conflict_in_both_admission_orders() {
        for parent_first in [true, false] {
            let root = root();
            let child = root.join("child");
            fs::create_dir(&child).unwrap();
            let (coordinator, _journal, authority) = coordinator(&root);
            authority.set_second_root(child.clone());
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
                .clone()
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
                // Exercise the authority-backed settlement path. A journal-only step update
                // would manufacture success without F8 proof and must retain the claim.
                authority.authorize_settlement(ReconcileDecision::ConfirmedCompleted);
                coordinator
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
                authority.authorize_settlement(ReconcileDecision::ConfirmedCompleted);
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
        authority.authorize_settlement(ReconcileDecision::ConfirmedCompleted);
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
            .clone()
            .unwrap();
        journal
            .mark_attempt_unknown(&run.run_id, &draft.steps[0].step_id, &attempt, 103)
            .unwrap();
        let revised = workspace_revision(&root).unwrap();
        authority.authorize_settlement(ReconcileDecision::ConfirmedCompleted);
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
        authority.authorize_settlement(ReconcileDecision::ConfirmedCompleted);
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
