//! Admission, workspace, export, quarantine and failure records (doc 52 §3).
//!
//! These are typed evidence shapes. Producing the real evidence (capture, launch, freeze,
//! export and publication) is out of scope for this contract. Record validation checks
//! shape; the digest, admission and lifecycle modules compute and cross-check bindings.
//! None of these data records attests that a real sandbox or publication exists.
use super::{
    B1ErrorCode, B1Result, GitOid, MAX_ENTRIES, MAX_TOTAL_CONTENT_BYTES, SchemaV1, Sha256Hex,
    counter, ensure,
    lifecycle::B1TaskState,
    path::{B1RelPath, validate_case_distinct, validate_destination_set},
    source::{B1SourceSelectionV1, OverlayContent},
};
use crate::contracts::{
    UtcTimestamp, WireUuid,
    identities::{B1SelectionId, B1TaskId},
};
use serde::{Deserialize, Serialize};

/// Binds a generation to the owner selection that produced it. Only trusted admission writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1AdmissionRecordV1 {
    pub schema_version: SchemaV1,
    pub task_id: B1TaskId,
    pub selection_id: B1SelectionId,
    pub generation: u64,
    pub record_revision: u64,
    pub base_commit_oid: GitOid,
    pub policy_sha256: Sha256Hex,
    pub approved_overlay_sha256: Sha256Hex,
    pub admitted_tree_sha256: Sha256Hex,
    pub admitted_bytes: u64,
    pub admitted_entries: u64,
}

impl B1AdmissionRecordV1 {
    pub fn validate(&self) -> B1Result {
        counter("generation", self.generation)?;
        counter("record_revision", self.record_revision)?;
        ensure(
            self.admitted_bytes <= MAX_TOTAL_CONTENT_BYTES,
            "admitted_bytes",
            B1ErrorCode::LimitExceeded,
        )?;
        ensure(
            self.admitted_entries <= MAX_ENTRIES as u64,
            "admitted_entries",
            B1ErrorCode::LimitExceeded,
        )
    }

    /// Confirms this record was produced from `selection`, not from a different task or base.
    pub fn bind_selection(&self, selection: &B1SourceSelectionV1) -> B1Result {
        ensure(
            self.task_id == selection.task_id
                && self.selection_id == selection.selection_id
                && self.policy_sha256 == selection.policy_sha256,
            "selection",
            B1ErrorCode::AdmissionUnauthorized,
        )?;
        ensure(
            self.base_commit_oid == selection.base_commit_oid,
            "base_commit_oid",
            B1ErrorCode::BaseOidInvalid,
        )
    }
}

/// Launch evidence. It proves a task-specific writable object set, not inode identity alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1WorkspaceReceiptV1 {
    pub schema_version: SchemaV1,
    pub task_id: B1TaskId,
    pub generation: u64,
    /// The admitted tree used to materialize this workspace, not a caller-chosen label.
    pub source_manifest_sha256: Sha256Hex,
    pub task_fs_id: WireUuid,
    pub mount_policy_sha256: Sha256Hex,
    pub preexec_profile_sha256: Sha256Hex,
    pub runtime_fingerprint_sha256: Sha256Hex,
    pub checked_at: UtcTimestamp,
}

impl B1WorkspaceReceiptV1 {
    pub fn validate(&self) -> B1Result {
        counter("generation", self.generation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportOperation {
    Add,
    Modify,
    Delete,
}

/// One exported node, as inert typed data. Symlink targets are never followed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1ExportEntryV1 {
    pub path: B1RelPath,
    pub operation: ExportOperation,
    pub content: Option<OverlayContent>,
}

impl B1ExportEntryV1 {
    fn validate(&self) -> B1Result {
        let shape_is_valid = match self.operation {
            ExportOperation::Add | ExportOperation::Modify => self.content.is_some(),
            ExportOperation::Delete => self.content.is_none(),
        };
        ensure(shape_is_valid, "operation", B1ErrorCode::ExportNodeUnsafe)?;
        match &self.content {
            Some(content) => content.validate(&self.path),
            None => Ok(()),
        }
    }
}

/// Exporter output. It is produced only after verified quiescence and is always re-validated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1ExportRecordV1 {
    pub schema_version: SchemaV1,
    pub task_id: B1TaskId,
    pub generation: u64,
    pub base_admission_sha256: Sha256Hex,
    pub entries: Vec<B1ExportEntryV1>,
    pub exporter_sha256: Sha256Hex,
    pub source_tree_sha256: Sha256Hex,
}

impl B1ExportRecordV1 {
    pub fn validate(&self) -> B1Result {
        counter("generation", self.generation)?;
        ensure(
            self.entries.len() <= MAX_ENTRIES,
            "entries",
            B1ErrorCode::LimitExceeded,
        )?;
        ensure(
            self.entries
                .windows(2)
                .all(|pair| pair[0].path < pair[1].path),
            "entries",
            B1ErrorCode::NonCanonicalOrder,
        )?;
        let mut total_bytes = 0u64;
        for entry in &self.entries {
            entry.validate()?;
            if let Some(content) = &entry.content {
                total_bytes += content.budget_bytes();
            }
        }
        ensure(
            total_bytes <= MAX_TOTAL_CONTENT_BYTES,
            "entries",
            B1ErrorCode::LimitExceeded,
        )?;
        validate_case_distinct(self.entries.iter().map(|entry| &entry.path))?;
        validate_destination_set(
            self.entries
                .iter()
                .filter(|entry| entry.operation != ExportOperation::Delete)
                .map(|entry| &entry.path),
        )
    }
}

/// Security advisory for owner review. Classification never grants execution or merge safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskFlag {
    AgentInstruction,
    AgentConfiguration,
    EditorConfiguration,
    #[serde(rename = "github_workflow")]
    GitHubWorkflow,
    GitHooks,
    BuildScript,
    PackageManifest,
    GitMetadata,
    EnvironmentLoader,
    Executable,
    Binary,
}

/// Flags a path by its components at any depth, comparing ASCII case-insensitively so that
/// case-folding destinations cannot hide a delayed-execution surface.
///
/// `executable` and `binary` are facts the caller must supply from the mode and the bytes.
/// This pure contract does not inspect content, so it cannot detect binary files itself.
pub fn classify_entry(path: &B1RelPath, executable: bool, binary: bool) -> Vec<RiskFlag> {
    let mut flags = Vec::new();
    for component in path.components() {
        match component.to_ascii_lowercase().as_slice() {
            b".github" => flags.push(RiskFlag::GitHubWorkflow),
            b".vscode" => flags.push(RiskFlag::EditorConfiguration),
            b".claude" | b".codex" => flags.push(RiskFlag::AgentConfiguration),
            b".husky" => flags.push(RiskFlag::GitHooks),
            _ => {}
        }
    }
    if let Some(base) = path.components().last() {
        match base.to_ascii_lowercase().as_slice() {
            b"agents.md" | b"claude.md" => flags.push(RiskFlag::AgentInstruction),
            b"makefile" => flags.push(RiskFlag::BuildScript),
            b"package.json" => flags.push(RiskFlag::PackageManifest),
            b".gitmodules" | b".gitattributes" | b".lfsconfig" => {
                flags.push(RiskFlag::GitMetadata);
            }
            b".envrc" => flags.push(RiskFlag::EnvironmentLoader),
            _ => {}
        }
    }
    if executable {
        flags.push(RiskFlag::Executable);
    }
    if binary {
        flags.push(RiskFlag::Binary);
    }
    flags.sort();
    flags.dedup();
    flags
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineStatus {
    Pending,
}

/// Quarantine result receipt. It is the only way a task reports success, and it is final B1
/// success only. It never authorizes integration into the source worktree or `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1QuarantineReceiptV1 {
    pub schema_version: SchemaV1,
    pub task_id: B1TaskId,
    pub expected_generation: u64,
    pub source_base_oid: GitOid,
    pub admission_sha256: Sha256Hex,
    pub export_sha256: Sha256Hex,
    pub result_store_ref: String,
    pub cas_revision: u64,
    pub result_tree_oid: GitOid,
    pub risk_flags: Vec<RiskFlag>,
    pub status: QuarantineStatus,
}

/// The only accepted quarantine ref for a task generation. It is derived from identity, so a
/// receipt cannot point at a ref belonging to a different task or to the source repository.
pub fn quarantine_ref(task_id: &B1TaskId, generation: u64) -> String {
    format!("refs/b1-quarantine/{}/{generation}", task_id.as_str())
}

impl B1QuarantineReceiptV1 {
    pub fn validate(&self) -> B1Result {
        counter("expected_generation", self.expected_generation)?;
        counter("cas_revision", self.cas_revision)?;
        ensure(
            self.result_tree_oid.format() == self.source_base_oid.format(),
            "result_tree_oid",
            B1ErrorCode::OidInvalid,
        )?;
        ensure(
            self.result_store_ref == quarantine_ref(&self.task_id, self.expected_generation),
            "result_store_ref",
            B1ErrorCode::ReceiptInvalid,
        )?;
        ensure(
            self.risk_flags.windows(2).all(|pair| pair[0] < pair[1]),
            "risk_flags",
            B1ErrorCode::NonCanonicalOrder,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRequired {
    None,
    OwnerReview,
    OperatorRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStatus {
    Verified,
    Unverified,
}

/// Failure report. It has no free-text field, so secret bytes, auth headers, private paths
/// and raw hostile content cannot be carried into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1FailureV1 {
    pub schema_version: SchemaV1,
    pub task_id: B1TaskId,
    /// The terminal state reached. Only failure or rejection states are valid here.
    pub stage: B1TaskState,
    pub stable_error_code: B1ErrorCode,
    pub action_required: ActionRequired,
    pub cleanup_status: CleanupStatus,
    pub correlation_id: WireUuid,
}

impl B1FailureV1 {
    pub fn validate(&self) -> B1Result {
        let consistent = match self.stage {
            B1TaskState::FailedCleanupUnverified => {
                self.cleanup_status == CleanupStatus::Unverified
                    && self.action_required == ActionRequired::OperatorRecovery
            }
            B1TaskState::FailedClean | B1TaskState::Rejected | B1TaskState::BlockedUnsafe => {
                self.cleanup_status == CleanupStatus::Verified
            }
            _ => false,
        };
        ensure(consistent, "stage", B1ErrorCode::ReceiptInvalid)
    }
}
