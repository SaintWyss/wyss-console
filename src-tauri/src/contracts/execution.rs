//! Permission facts and informational diagnostics. No bearer capability or execution request.
use super::control::{ControlError as E, ControlResult};
use super::{
    UtcTimestamp, WireUuid,
    identities::{ExecutionLeaseId, ManagedSessionId},
};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionScope {
    SessionCreate,
    SessionResume,
    TurnStart,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseState {
    Active,
    Revoked,
    Expired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevocationReason {
    UserRequest,
    SessionClosed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionLease {
    pub lease_id: ExecutionLeaseId,
    pub session_id: ManagedSessionId,
    pub state: LeaseState,
    pub scopes: Vec<ExecutionScope>,
    pub revision: u64,
    pub issued_at: UtcTimestamp,
    pub expires_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub revocation_reason: Option<RevocationReason>,
}
pub fn timestamp(t: &UtcTimestamp) -> ControlResult<OffsetDateTime> {
    OffsetDateTime::parse(&String::from(t.clone()), &Rfc3339).map_err(|_| E::StorageIncompatible)
}
pub fn validate_scopes(scopes: &[ExecutionScope]) -> ControlResult<()> {
    let mut ordered = scopes.to_vec();
    ordered.sort();
    ordered.dedup();
    if scopes.is_empty() || scopes.len() > 3 || ordered.len() != scopes.len() {
        Err(E::InvalidRequest)
    } else {
        Ok(())
    }
}
pub fn validate_ttl(ttl: u32) -> ControlResult<()> {
    if (1..=3600).contains(&ttl) {
        Ok(())
    } else {
        Err(E::InvalidRequest)
    }
}
pub fn valid_revision(r: u64) -> ControlResult<()> {
    if r > 0 && r <= i64::MAX as u64 {
        Ok(())
    } else {
        Err(E::InvalidRequest)
    }
}
impl ExecutionLease {
    pub fn validate(&self) -> ControlResult<()> {
        validate_scopes(&self.scopes)?;
        valid_revision(self.revision)?;
        let mut sorted = self.scopes.clone();
        sorted.sort();
        let issued = timestamp(&self.issued_at)?;
        let expires = timestamp(&self.expires_at)?;
        let updated = timestamp(&self.updated_at)?;
        if sorted != self.scopes
            || expires <= issued
            || expires - updated > time::Duration::hours(1)
            || (self.state == LeaseState::Revoked) != self.revocation_reason.is_some()
            || self
                .lease_id
                .as_str()
                .eq_ignore_ascii_case(self.session_id.as_str())
        {
            return Err(E::StorageIncompatible);
        }
        Ok(())
    }
    pub fn effective_at(&self, now: OffsetDateTime) -> ControlResult<Self> {
        let mut effective = self.clone();
        if effective.state == LeaseState::Active && now >= timestamp(&effective.expires_at)? {
            // Effective projection only: stored revision and timestamp are never fabricated.
            effective.state = LeaseState::Expired;
        }
        Ok(effective)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseGrant {
    pub intent_id: WireUuid,
    pub session_id: ManagedSessionId,
    pub expected_session_revision: u64,
    pub scopes: Vec<ExecutionScope>,
    pub ttl_seconds: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseGet {
    pub lease_id: ExecutionLeaseId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseActive {
    pub session_id: ManagedSessionId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseRenew {
    pub intent_id: WireUuid,
    pub lease_id: ExecutionLeaseId,
    pub expected_revision: u64,
    pub ttl_seconds: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseRevoke {
    pub intent_id: WireUuid,
    pub lease_id: ExecutionLeaseId,
    pub expected_revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateQuery {
    pub session_id: ManagedSessionId,
    pub action: ExecutionScope,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateReason {
    Allowed,
    BindingNotFound,
    BindingDetached,
    AttachmentRuntimeMismatch,
    InferenceProfileMissing,
    RuntimeSafetyProfileMissing,
    ActiveTurnExists,
    SessionNotFound,
    SessionNotPrepared,
    LeaseMissing,
    LeaseNotActive,
    LeaseExpired,
    ScopeNotGranted,
    RuntimeMismatch,
    RuntimeNotRunning,
    RuntimeNotReady,
    RuntimeIdentityMissing,
    RuntimeIncompatible,
    AuthorityUnavailable,
}
/// Informational snapshot only. No operation accepts this value as authorization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateStatus {
    pub session_id: ManagedSessionId,
    pub action: ExecutionScope,
    pub reason: GateReason,
}
