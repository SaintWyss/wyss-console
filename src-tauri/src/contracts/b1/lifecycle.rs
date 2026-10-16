//! Fail-closed B1 task lifecycle (doc 52 §4).
//!
//! `PENDING` is reachable only through [`B1TaskLifecycle::publish_pending`], which checks
//! cross-record evidence. The generic [`B1TaskLifecycle::advance`] refuses it. Failure states
//! are reachable only from the stages where doc 52 allows them, and terminal states never
//! leave.
use super::{
    B1Error, B1ErrorCode, B1Result, Sha256Hex,
    records::{B1AdmissionRecordV1, B1ExportRecordV1, B1QuarantineReceiptV1, B1WorkspaceReceiptV1},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum B1TaskState {
    Created,
    Selecting,
    Captured,
    Admitted,
    Materialized,
    Confined,
    Running,
    Quiescing,
    Frozen,
    Exporting,
    Validated,
    Pending,
    Rejected,
    FailedClean,
    FailedCleanupUnverified,
    BlockedUnsafe,
}

impl B1TaskState {
    /// `PENDING` is final B1 success. It is not integration and is not a resumable state.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Pending
                | Self::Rejected
                | Self::FailedClean
                | Self::FailedCleanupUnverified
                | Self::BlockedUnsafe
        )
    }

    /// The only legal edges. `Validated -> Pending` is listed so the table is complete, but
    /// [`B1TaskLifecycle::advance`] never takes it without evidence.
    pub const fn can_transition_to(self, next: Self) -> bool {
        use B1TaskState::*;
        matches!(
            (self, next),
            (Created, Selecting)
                | (Selecting, Captured | Rejected)
                | (Captured, Admitted | Rejected)
                | (Admitted, Materialized | Rejected)
                | (Materialized, Confined | FailedClean | BlockedUnsafe)
                | (Confined, Running | FailedClean | BlockedUnsafe)
                | (Running, Quiescing)
                | (Quiescing, Frozen | FailedCleanupUnverified)
                | (Frozen, Exporting | FailedClean)
                | (Exporting, Validated | Rejected)
                | (Validated, Pending | Rejected)
        )
    }
}

/// Typed evidence required for `PENDING`. Every record is validated and cross-checked.
#[derive(Debug, Clone, Copy)]
pub struct B1PendingEvidence<'a> {
    pub admission: &'a B1AdmissionRecordV1,
    pub admission_sha256: &'a Sha256Hex,
    pub workspace: &'a B1WorkspaceReceiptV1,
    pub export: &'a B1ExportRecordV1,
    pub export_sha256: &'a Sha256Hex,
    pub quarantine: &'a B1QuarantineReceiptV1,
}

impl B1PendingEvidence<'_> {
    fn check(&self) -> B1Result {
        self.admission.validate()?;
        self.workspace.validate()?;
        self.export.validate()?;
        self.quarantine.validate()?;
        let task = &self.admission.task_id;
        let generation = self.admission.generation;
        let consistent = self.workspace.task_id == *task
            && self.export.task_id == *task
            && self.quarantine.task_id == *task
            && self.workspace.generation == generation
            && self.export.generation == generation
            && self.quarantine.expected_generation == generation
            && self.export.base_admission_sha256 == *self.admission_sha256
            && self.quarantine.admission_sha256 == *self.admission_sha256
            && self.quarantine.export_sha256 == *self.export_sha256
            && self.quarantine.source_base_oid == self.admission.base_commit_oid;
        if consistent {
            Ok(())
        } else {
            Err(B1Error::new(
                "pending_evidence",
                B1ErrorCode::PendingEvidenceMismatch,
            ))
        }
    }
}

/// Owned, single-writer state. It is deliberately not `Copy` or `Clone`, so a validated state cannot be forked.
#[derive(Debug, PartialEq, Eq)]
pub struct B1TaskLifecycle {
    state: B1TaskState,
}

impl Default for B1TaskLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl B1TaskLifecycle {
    pub const fn new() -> Self {
        Self {
            state: B1TaskState::Created,
        }
    }

    pub const fn state(&self) -> B1TaskState {
        self.state
    }

    /// Advances along a legal edge. Refused edges leave the state unchanged.
    pub fn advance(&mut self, next: B1TaskState) -> B1Result {
        if next == B1TaskState::Pending {
            return Err(B1Error::new("state", B1ErrorCode::InvalidTransition));
        }
        if !self.state.can_transition_to(next) {
            return Err(B1Error::new("state", B1ErrorCode::InvalidTransition));
        }
        self.state = next;
        Ok(())
    }

    /// Publishes the pending result. It is valid only from `VALIDATED` with matching evidence.
    /// On any failure the state is unchanged.
    pub fn publish_pending(&mut self, evidence: &B1PendingEvidence<'_>) -> B1Result {
        if self.state != B1TaskState::Validated {
            return Err(B1Error::new("state", B1ErrorCode::InvalidTransition));
        }
        evidence.check()?;
        self.state = B1TaskState::Pending;
        Ok(())
    }
}
