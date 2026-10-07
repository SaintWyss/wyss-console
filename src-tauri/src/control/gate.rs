//! Point-in-time internal admission. This decision is not serializable or reusable.
use super::store::Store;
use crate::contracts::{
    agent_adapter::CompatibilityState, control::*, identities::ManagedSessionId,
};

#[derive(Debug)]
pub(super) enum ExecutionDecision {
    Allowed,
    Denied(GateReason),
}
impl ExecutionDecision {
    pub(super) fn reason(&self) -> GateReason {
        match self {
            Self::Allowed => GateReason::Allowed,
            Self::Denied(r) => *r,
        }
    }
}
pub(super) struct ExecutionGate;
impl ExecutionGate {
    fn base(
        store: &Store,
        session_id: &ManagedSessionId,
        action: ExecutionScope,
        runtime: &RuntimeStatus,
    ) -> ExecutionDecision {
        use GateReason::*;
        let deny = ExecutionDecision::Denied;
        if !store.execution_available {
            return deny(AuthorityUnavailable);
        }
        let session = match store.gate_session(session_id) {
            Ok(s) => s,
            Err(ControlError::SessionNotFound) => return deny(SessionNotFound),
            Err(_) => return deny(AuthorityUnavailable),
        };
        if session.control_state != ControlState::Prepared {
            return deny(SessionNotPrepared);
        }
        let lease = match store.gate_lease(session_id) {
            Ok(Some(l)) => l,
            Ok(None) => return deny(LeaseMissing),
            Err(_) => return deny(AuthorityUnavailable),
        };
        if lease.state != LeaseState::Active {
            return deny(if lease.state == LeaseState::Expired {
                LeaseExpired
            } else {
                LeaseNotActive
            });
        }
        let deadline = match timestamp(&lease.expires_at) {
            Ok(t) => t,
            Err(_) => return deny(AuthorityUnavailable),
        };
        if store.lease_now() >= deadline {
            return deny(LeaseExpired);
        }
        if !lease.scopes.contains(&action) {
            return deny(ScopeNotGranted);
        }
        if session.adapter_id != runtime.adapter_id || session.runtime_kind != runtime.runtime_kind
        {
            return deny(RuntimeMismatch);
        }
        if runtime.desired_state != DesiredState::Running {
            return deny(RuntimeNotRunning);
        }
        if runtime.actual_state != ActualState::Ready {
            return deny(RuntimeNotReady);
        }
        let Some(_) = runtime.runtime_instance_id.as_ref() else {
            return deny(RuntimeIdentityMissing);
        };
        if runtime.compatibility != CompatibilityState::Compatible {
            return deny(RuntimeIncompatible);
        }
        ExecutionDecision::Allowed
    }
    pub(super) fn evaluate(
        store: &Store,
        session: &ManagedSessionId,
        action: ExecutionScope,
        runtime: &RuntimeStatus,
    ) -> ExecutionDecision {
        let base = Self::base(store, session, action, runtime);
        if matches!(base, ExecutionDecision::Allowed) && action == ExecutionScope::TurnStart {
            ExecutionDecision::Denied(GateReason::BindingDetached)
        } else {
            base
        }
    }
    pub(super) fn turn(
        store: &Store,
        session: &ManagedSessionId,
        runtime: &RuntimeStatus,
        binding: &BindingStatus,
    ) -> ExecutionDecision {
        let base = Self::base(store, session, ExecutionScope::TurnStart, runtime);
        if !matches!(base, ExecutionDecision::Allowed) {
            return base;
        }
        use GateReason::*;
        let reason = if binding.binding.is_none() {
            Some(BindingNotFound)
        } else if binding.state != BindingState::BoundAttached {
            Some(BindingDetached)
        } else if binding.runtime_instance_id != runtime.runtime_instance_id {
            Some(AttachmentRuntimeMismatch)
        } else if runtime.runtime_safety_profile != Some(RuntimeSafetyProfile::CodexInferenceV1) {
            Some(RuntimeSafetyProfileMissing)
        } else if binding.attachment_profile != Some(AttachmentProfile::InferenceOnlyV1) {
            Some(InferenceProfileMissing)
        } else {
            match store.active_turn() {
                Ok(Some(_)) => Some(ActiveTurnExists),
                Err(_) => Some(AuthorityUnavailable),
                Ok(None) => None,
            }
        };
        reason.map_or(ExecutionDecision::Allowed, ExecutionDecision::Denied)
    }
    pub(super) fn diagnostic(store: &Store, p: &GateQuery, runtime: &RuntimeStatus) -> GateStatus {
        GateStatus {
            session_id: p.session_id.clone(),
            action: p.action,
            reason: Self::evaluate(store, &p.session_id, p.action, runtime).reason(),
        }
    }
}
