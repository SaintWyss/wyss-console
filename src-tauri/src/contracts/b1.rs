//! Pure B1 contract records: typed identifiers, source selection, overlay
//! entries, lifecycle transitions and pending quarantine receipts.
//!
//! This module performs no I/O and runs no Git, filesystem, sandbox or provider
//! code. Its tests show only that these records validate and transition
//! deterministically. They do not prove owner authorization, safe filesystem
//! capture, OS containment or the absence of exfiltration; those require the
//! reviewed components described in documents 50 to 52.
//!
//! ```compile_fail
//! use wyss_console::contracts::b1::B1TaskId;
//! use wyss_console::contracts::identities::ManagedSessionId;
//! let task = B1TaskId::parse("11111111-1111-4111-8111-111111111111").unwrap();
//! let session: ManagedSessionId = task;
//! ```

use super::{UtcTimestamp, WireUuid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fmt;

pub const SCHEMA_VERSION: u8 = 1;
/// Provisional test-profile budgets from document 52 §7, not measured limits.
pub const MAX_OVERLAY_ENTRIES: usize = 2048;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_COMPONENT_BYTES: usize = 255;
pub const MAX_PATH_DEPTH: usize = 32;
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESULT_STORE_REF: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum B1ErrorCode {
    Identifier,
    SchemaVersion,
    Oid,
    Digest,
    Path,
    Budget,
    PathCollision,
    GitAlias,
    GitMode,
    FileKind,
    SymlinkTarget,
    OverlayShape,
    OverlayOrder,
    ContentMismatch,
    ReviewDigestMismatch,
    ManifestMismatch,
    StaleGeneration,
    IllegalTransition,
    ResultStoreRef,
    RiskFlags,
    CasConflict,
    DuplicatePublication,
}

impl B1ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::SchemaVersion => "schema_version",
            Self::Oid => "oid",
            Self::Digest => "digest",
            Self::Path => "path",
            Self::Budget => "budget",
            Self::PathCollision => "path_collision",
            Self::GitAlias => "git_alias",
            Self::GitMode => "git_mode",
            Self::FileKind => "file_kind",
            Self::SymlinkTarget => "symlink_target",
            Self::OverlayShape => "overlay_shape",
            Self::OverlayOrder => "overlay_order",
            Self::ContentMismatch => "content_mismatch",
            Self::ReviewDigestMismatch => "review_digest_mismatch",
            Self::ManifestMismatch => "manifest_mismatch",
            Self::StaleGeneration => "stale_generation",
            Self::IllegalTransition => "illegal_transition",
            Self::ResultStoreRef => "result_store_ref",
            Self::RiskFlags => "risk_flags",
            Self::CasConflict => "cas_conflict",
            Self::DuplicatePublication => "duplicate_publication",
        }
    }
}

/// Diagnostics name a field and a stable code only; rejected content is never echoed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B1Error {
    pub field: &'static str,
    pub code: B1ErrorCode,
}

impl B1Error {
    pub const fn new(field: &'static str, code: B1ErrorCode) -> Self {
        Self { field, code }
    }
}

impl fmt::Display for B1Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.code.as_str())
    }
}

impl std::error::Error for B1Error {}

pub type B1Result<T> = Result<T, B1Error>;

macro_rules! b1_identity {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(WireUuid);

        impl $name {
            pub fn parse(value: &str) -> B1Result<Self> {
                WireUuid::try_from(value.to_owned())
                    .map(Self)
                    .map_err(|_| B1Error::new($field, B1ErrorCode::Identifier))
            }

            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }
    };
}

b1_identity!(B1TaskId, "task_id");
b1_identity!(B1RepositoryId, "repository_id");
b1_identity!(B1OwnerSelectionId, "owner_selection_id");

/// Lowercase hex SHA-256 text. Constructed only from validated text or a hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256Hex(String);

impl Sha256Hex {
    pub fn of(bytes: &[u8]) -> Self {
        Self::from_hash(Sha256::digest(bytes).as_slice())
    }

    fn from_hash(hash: &[u8]) -> Self {
        Self(hex_lower(hash))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Sha256Hex {
    type Error = B1Error;

    fn try_from(value: String) -> B1Result<Self> {
        if is_lower_hex(&value, 64) {
            Ok(Self(value))
        } else {
            Err(B1Error::new("sha256", B1ErrorCode::Digest))
        }
    }
}

impl From<Sha256Hex> for String {
    fn from(value: Sha256Hex) -> Self {
        value.0
    }
}

/// Lowercase Git object id. Its length is checked against the selected format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GitOid(String);

impl GitOid {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for GitOid {
    type Error = B1Error;

    fn try_from(value: String) -> B1Result<Self> {
        if is_lower_hex(&value, 40) || is_lower_hex(&value, 64) {
            Ok(Self(value))
        } else {
            Err(B1Error::new("oid", B1ErrorCode::Oid))
        }
    }
}

impl From<GitOid> for String {
    fn from(value: GitOid) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectFormat {
    Sha1,
    Sha256,
}

impl ObjectFormat {
    const fn hex_len(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }
}

/// Only the three Git modes B1 can represent. Gitlinks (160000), trees (040000)
/// and any other value fail deserialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum GitFileMode {
    Regular,
    Executable,
    Symlink,
}

impl GitFileMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Regular => "100644",
            Self::Executable => "100755",
            Self::Symlink => "120000",
        }
    }
}

impl TryFrom<String> for GitFileMode {
    type Error = B1Error;

    fn try_from(value: String) -> B1Result<Self> {
        match value.as_str() {
            "100644" => Ok(Self::Regular),
            "100755" => Ok(Self::Executable),
            "120000" => Ok(Self::Symlink),
            _ => Err(B1Error::new("mode", B1ErrorCode::GitMode)),
        }
    }
}

impl From<GitFileMode> for String {
    fn from(value: GitFileMode) -> Self {
        value.as_str().to_owned()
    }
}

/// Canonical relative path. Validated on every construction path, including serde.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct B1Path(String);

impl B1Path {
    pub fn parse(value: &str) -> B1Result<Self> {
        check_path(value).map_err(|code| B1Error::new("path", code))?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for B1Path {
    type Error = B1Error;

    fn try_from(value: String) -> B1Result<Self> {
        Self::parse(&value)
    }
}

impl From<B1Path> for String {
    fn from(value: B1Path) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum B1ChangeOrigin {
    Unstaged,
    Staged,
    Untracked,
}

impl B1ChangeOrigin {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unstaged => "unstaged",
            Self::Staged => "staged",
            Self::Untracked => "untracked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum B1Operation {
    Add,
    Modify,
    Delete,
    Rename,
}

impl B1Operation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Modify => "modify",
            Self::Delete => "delete",
            Self::Rename => "rename",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum B1EntryKind {
    Regular,
    Symlink,
}

impl B1EntryKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Symlink => "symlink",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1BaseEntry {
    pub oid: GitOid,
    pub mode: GitFileMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1Content {
    pub sha256: Sha256Hex,
    pub size: u64,
    pub mode: GitFileMode,
}

/// One explicitly selected change. Its `reviewed_digest` must equal `entry_digest()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1OverlayEntryV1 {
    pub path: B1Path,
    pub change_origin: B1ChangeOrigin,
    pub operation: B1Operation,
    pub entry_kind: B1EntryKind,
    pub base: Option<B1BaseEntry>,
    pub rename_from: Option<B1Path>,
    pub content: Option<B1Content>,
    pub symlink_target: Option<String>,
    pub reviewed_digest: Sha256Hex,
}

impl B1OverlayEntryV1 {
    pub fn entry_digest(&self) -> Sha256Hex {
        let mut canonical = Canonical::new("wyss.b1.overlay-entry.v1");
        canonical.text(self.path.as_str());
        canonical.text(self.change_origin.as_str());
        canonical.text(self.operation.as_str());
        canonical.text(self.entry_kind.as_str());
        match &self.base {
            Some(base) => {
                canonical.text("base");
                canonical.text(base.oid.as_str());
                canonical.text(base.mode.as_str());
            }
            None => canonical.text("base-absent"),
        }
        match &self.rename_from {
            Some(from) => {
                canonical.text("rename-from");
                canonical.text(from.as_str());
            }
            None => canonical.text("rename-none"),
        }
        match &self.content {
            Some(content) => {
                canonical.text("content");
                canonical.text(content.sha256.as_str());
                canonical.number(content.size);
                canonical.text(content.mode.as_str());
            }
            None => canonical.text("content-none"),
        }
        match &self.symlink_target {
            Some(target) => {
                canonical.text("target");
                canonical.text(target);
            }
            None => canonical.text("target-none"),
        }
        canonical.finish()
    }

    pub fn validate(&self) -> B1Result<()> {
        let path = self.path.as_str();
        if !origin_matches_operation(self.change_origin, self.operation, self.base.is_some()) {
            return Err(shape_error());
        }
        match (self.operation, &self.rename_from) {
            (B1Operation::Rename, Some(from)) if from.as_str() != path => {}
            (B1Operation::Rename, _) | (_, Some(_)) => return Err(shape_error()),
            _ => {}
        }
        match (self.operation, &self.content) {
            (B1Operation::Delete, None) => {}
            (B1Operation::Delete, Some(_)) | (_, None) => return Err(shape_error()),
            (_, Some(_)) => {}
        }
        if let Some(base) = &self.base
            && !mode_matches(self.entry_kind, base.mode)
        {
            return Err(B1Error::new("mode", B1ErrorCode::FileKind));
        }
        if let Some(content) = &self.content {
            if !mode_matches(self.entry_kind, content.mode) {
                return Err(B1Error::new("mode", B1ErrorCode::FileKind));
            }
            if content.size > MAX_FILE_BYTES {
                return Err(B1Error::new("content", B1ErrorCode::Budget));
            }
        }
        match (self.entry_kind, &self.content, &self.symlink_target) {
            (B1EntryKind::Symlink, Some(content), Some(target)) => {
                check_symlink_target(path, target)
                    .map_err(|code| B1Error::new("symlink_target", code))?;
                if content.size != target.len() as u64
                    || content.sha256 != Sha256Hex::of(target.as_bytes())
                {
                    return Err(B1Error::new("content", B1ErrorCode::ContentMismatch));
                }
            }
            (B1EntryKind::Regular, _, None) | (B1EntryKind::Symlink, None, None) => {}
            _ => return Err(B1Error::new("symlink_target", B1ErrorCode::SymlinkTarget)),
        }
        if self.entry_digest() != self.reviewed_digest {
            return Err(B1Error::new("reviewed_digest", B1ErrorCode::ReviewDigestMismatch));
        }
        Ok(())
    }
}

/// Checks bytes supplied by the trusted admission component against the admitted record.
pub fn verify_content_bytes(content: &B1Content, bytes: &[u8]) -> B1Result<()> {
    if content.size != bytes.len() as u64 || content.sha256 != Sha256Hex::of(bytes) {
        return Err(B1Error::new("content", B1ErrorCode::ContentMismatch));
    }
    Ok(())
}

/// Validates the whole overlay: shape, canonical order, collisions and budgets.
pub fn validate_overlay(overlay: &[B1OverlayEntryV1]) -> B1Result<()> {
    if overlay.len() > MAX_OVERLAY_ENTRIES {
        return Err(B1Error::new("overlay", B1ErrorCode::Budget));
    }
    for entry in overlay {
        entry.validate()?;
    }
    if overlay
        .windows(2)
        .any(|pair| pair[0].path.as_str() >= pair[1].path.as_str())
    {
        return Err(B1Error::new("overlay", B1ErrorCode::OverlayOrder));
    }
    let mut keys = HashSet::new();
    for entry in overlay {
        if !keys.insert(collision_key(entry.path.as_str())) {
            return Err(B1Error::new("path", B1ErrorCode::PathCollision));
        }
    }
    for entry in overlay {
        if let Some(from) = &entry.rename_from
            && keys.contains(&collision_key(from.as_str()))
        {
            return Err(B1Error::new("rename_from", B1ErrorCode::PathCollision));
        }
        let key = collision_key(entry.path.as_str());
        let components: Vec<&str> = key.split('/').collect();
        for depth in 1..components.len() {
            if keys.contains(&components[..depth].join("/")) {
                return Err(B1Error::new("path", B1ErrorCode::PathCollision));
            }
        }
    }
    let total = overlay
        .iter()
        .filter_map(|entry| entry.content.as_ref())
        .try_fold(0u64, |sum, content| sum.checked_add(content.size));
    if total.is_none_or(|sum| sum > MAX_TOTAL_BYTES) {
        return Err(B1Error::new("overlay", B1ErrorCode::Budget));
    }
    Ok(())
}

/// Digest over the whole overlay list, in its canonical order.
pub fn overlay_sha256(overlay: &[B1OverlayEntryV1]) -> Sha256Hex {
    let mut canonical = Canonical::new("wyss.b1.overlay.v1");
    canonical.number(overlay.len() as u64);
    for entry in overlay {
        canonical.text(entry.entry_digest().as_str());
    }
    canonical.finish()
}

/// Digest over the files that exist after the overlay is applied.
pub fn admitted_content_sha256(overlay: &[B1OverlayEntryV1]) -> Sha256Hex {
    let mut canonical = Canonical::new("wyss.b1.admitted-content.v1");
    let present = overlay.iter().filter(|entry| entry.content.is_some()).count();
    canonical.number(present as u64);
    for entry in overlay {
        if let Some(content) = &entry.content {
            canonical.text(entry.path.as_str());
            canonical.text(content.sha256.as_str());
            canonical.number(content.size);
            canonical.text(content.mode.as_str());
        }
    }
    canonical.finish()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1SourceSelectionV1 {
    pub schema_version: u8,
    pub task_id: B1TaskId,
    pub repository_id: B1RepositoryId,
    pub owner_selection_id: B1OwnerSelectionId,
    pub base_commit: GitOid,
    pub object_format: ObjectFormat,
    pub policy_sha256: Sha256Hex,
    pub issued_at: UtcTimestamp,
    pub overlay: Vec<B1OverlayEntryV1>,
}

impl B1SourceSelectionV1 {
    pub fn validate(&self) -> B1Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(B1Error::new("schema_version", B1ErrorCode::SchemaVersion));
        }
        if self.base_commit.as_str().len() != self.object_format.hex_len() {
            return Err(B1Error::new("base_commit", B1ErrorCode::Oid));
        }
        validate_overlay(&self.overlay)
    }

    /// Digest binding identity, base, policy, owner selection, time and overlay.
    pub fn selection_sha256(&self) -> Sha256Hex {
        let issued: String = self.issued_at.clone().into();
        let mut canonical = Canonical::new("wyss.b1.source-selection.v1");
        canonical.number(u64::from(self.schema_version));
        canonical.text(self.task_id.as_str());
        canonical.text(self.repository_id.as_str());
        canonical.text(self.owner_selection_id.as_str());
        canonical.text(self.base_commit.as_str());
        canonical.text(self.object_format.as_str());
        canonical.text(self.policy_sha256.as_str());
        canonical.text(&issued);
        canonical.text(overlay_sha256(&self.overlay).as_str());
        canonical.finish()
    }
}

/// Written by the trusted admission component after it verifies a selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1AdmissionRecordV1 {
    pub schema_version: u8,
    pub task_id: B1TaskId,
    pub generation: u64,
    pub record_revision: u64,
    pub selection_sha256: Sha256Hex,
    pub overlay_sha256: Sha256Hex,
    pub admitted_content_sha256: Sha256Hex,
}

impl B1AdmissionRecordV1 {
    pub fn validate_against(&self, selection: &B1SourceSelectionV1) -> B1Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(B1Error::new("schema_version", B1ErrorCode::SchemaVersion));
        }
        if self.generation == 0 || self.record_revision == 0 {
            return Err(B1Error::new("admission", B1ErrorCode::StaleGeneration));
        }
        if self.task_id != selection.task_id
            || self.selection_sha256 != selection.selection_sha256()
            || self.overlay_sha256 != overlay_sha256(&selection.overlay)
            || self.admitted_content_sha256 != admitted_content_sha256(&selection.overlay)
        {
            return Err(B1Error::new("admission", B1ErrorCode::ManifestMismatch));
        }
        Ok(())
    }
}

/// Fails closed unless the presented generation is the current one.
pub fn require_generation(current: u64, presented: u64) -> B1Result<()> {
    if current == 0 || presented != current {
        return Err(B1Error::new("generation", B1ErrorCode::StaleGeneration));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum B1State {
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

impl B1State {
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

    const fn before_run(self) -> bool {
        matches!(
            self,
            Self::Created
                | Self::Selecting
                | Self::Captured
                | Self::Admitted
                | Self::Materialized
                | Self::Confined
        )
    }

    /// Only the documented forward chain, cancellation through QUIESCING, and
    /// fail-closed BLOCKED_UNSAFE from any live state. Terminal states never move.
    pub fn transition(self, next: B1State) -> B1Result<B1State> {
        use B1State::*;
        let allowed = !self.is_terminal()
            && match (self, next) {
                (Created, Selecting)
                | (Selecting, Captured)
                | (Captured, Admitted)
                | (Admitted, Materialized)
                | (Materialized, Confined)
                | (Confined, Running)
                | (Running, Quiescing)
                | (Quiescing, Frozen)
                | (Frozen, Exporting)
                | (Exporting, Validated)
                | (Validated, Pending)
                | (Quiescing, FailedClean)
                | (Quiescing, FailedCleanupUnverified) => true,
                (state, Rejected) => state.before_run(),
                (_, BlockedUnsafe) => true,
                _ => false,
            };
        if allowed {
            Ok(next)
        } else {
            Err(B1Error::new("lifecycle", B1ErrorCode::IllegalTransition))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum B1RiskFlag {
    DelayedExecutionDirectory,
    InstructionFile,
    BuildOrHookScript,
    ToolConfiguration,
    ExecutableMode,
    SymlinkEntry,
}

/// Advisory classification for owner review. It is never execution permission.
pub fn classify_risk_flags(overlay: &[B1OverlayEntryV1]) -> Vec<B1RiskFlag> {
    const DELAYED_DIRS: [&str; 5] = [".github", ".vscode", ".claude", ".codex", ".husky"];
    const INSTRUCTION_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];
    const BUILD_FILES: [&str; 2] = ["Makefile", "package.json"];
    const TOOL_FILES: [&str; 4] = [".gitmodules", ".gitattributes", ".lfsconfig", ".envrc"];

    let mut flags = BTreeSet::new();
    for entry in overlay {
        let Some(content) = &entry.content else {
            continue;
        };
        let components: Vec<&str> = entry.path.as_str().split('/').collect();
        let (name, dirs) = components.split_last().expect("validated path is non-empty");
        if dirs
            .iter()
            .any(|dir| DELAYED_DIRS.iter().any(|known| dir.eq_ignore_ascii_case(known)))
        {
            flags.insert(B1RiskFlag::DelayedExecutionDirectory);
        }
        if INSTRUCTION_FILES.iter().any(|known| name.eq_ignore_ascii_case(known)) {
            flags.insert(B1RiskFlag::InstructionFile);
        }
        if BUILD_FILES.iter().any(|known| name.eq_ignore_ascii_case(known)) {
            flags.insert(B1RiskFlag::BuildOrHookScript);
        }
        if TOOL_FILES.iter().any(|known| name.eq_ignore_ascii_case(known)) {
            flags.insert(B1RiskFlag::ToolConfiguration);
        }
        if content.mode == GitFileMode::Executable {
            flags.insert(B1RiskFlag::ExecutableMode);
        }
        if entry.entry_kind == B1EntryKind::Symlink {
            flags.insert(B1RiskFlag::SymlinkEntry);
        }
    }
    flags.into_iter().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum B1ResultStatus {
    Pending,
}

/// Pending result receipt. It records quarantine only, never integration or merge safety.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1QuarantineReceiptV1 {
    pub schema_version: u8,
    pub task_id: B1TaskId,
    pub generation: u64,
    pub source_base_commit: GitOid,
    pub admission_sha256: Sha256Hex,
    pub export_sha256: Sha256Hex,
    pub result_store_ref: String,
    pub cas_revision: u64,
    pub result_tree: GitOid,
    pub risk_flags: Vec<B1RiskFlag>,
    pub status: B1ResultStatus,
}

impl B1QuarantineReceiptV1 {
    pub fn validate(&self) -> B1Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(B1Error::new("schema_version", B1ErrorCode::SchemaVersion));
        }
        if self.generation == 0 {
            return Err(B1Error::new("generation", B1ErrorCode::StaleGeneration));
        }
        if self.cas_revision == 0 {
            return Err(B1Error::new("cas_revision", B1ErrorCode::CasConflict));
        }
        if !is_result_store_ref(&self.result_store_ref) {
            return Err(B1Error::new("result_store_ref", B1ErrorCode::ResultStoreRef));
        }
        if self.risk_flags.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(B1Error::new("risk_flags", B1ErrorCode::RiskFlags));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct B1PublicationExpectation {
    pub generation: u64,
    pub cas_revision: u64,
}

/// In-memory model of publish-once semantics. It is not the Git quarantine publisher.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct B1QuarantineSlot {
    published: Option<B1QuarantineReceiptV1>,
}

impl B1QuarantineSlot {
    pub fn publish(
        &mut self,
        receipt: B1QuarantineReceiptV1,
        expected: B1PublicationExpectation,
    ) -> B1Result<()> {
        receipt.validate()?;
        if self.published.is_some() {
            return Err(B1Error::new("quarantine", B1ErrorCode::DuplicatePublication));
        }
        require_generation(expected.generation, receipt.generation)?;
        if receipt.cas_revision != expected.cas_revision + 1 {
            return Err(B1Error::new("cas_revision", B1ErrorCode::CasConflict));
        }
        self.published = Some(receipt);
        Ok(())
    }

    pub fn published(&self) -> Option<&B1QuarantineReceiptV1> {
        self.published.as_ref()
    }
}

fn shape_error() -> B1Error {
    B1Error::new("overlay", B1ErrorCode::OverlayShape)
}

fn origin_matches_operation(origin: B1ChangeOrigin, operation: B1Operation, has_base: bool) -> bool {
    match operation {
        B1Operation::Add => {
            matches!(origin, B1ChangeOrigin::Staged | B1ChangeOrigin::Untracked) && !has_base
        }
        B1Operation::Modify | B1Operation::Delete | B1Operation::Rename => {
            matches!(origin, B1ChangeOrigin::Staged | B1ChangeOrigin::Unstaged) && has_base
        }
    }
}

const fn mode_matches(kind: B1EntryKind, mode: GitFileMode) -> bool {
    match kind {
        B1EntryKind::Regular => !matches!(mode, GitFileMode::Symlink),
        B1EntryKind::Symlink => matches!(mode, GitFileMode::Symlink),
    }
}

fn check_path(value: &str) -> Result<(), B1ErrorCode> {
    if value.is_empty()
        || value.starts_with('/')
        || value.chars().any(|c| c == '\\' || c.is_control())
    {
        return Err(B1ErrorCode::Path);
    }
    if value.len() > MAX_PATH_BYTES {
        return Err(B1ErrorCode::Budget);
    }
    let components: Vec<&str> = value.split('/').collect();
    if components.len() > MAX_PATH_DEPTH {
        return Err(B1ErrorCode::Budget);
    }
    for component in components {
        if component.is_empty() || component == "." || component == ".." {
            return Err(B1ErrorCode::Path);
        }
        if component.len() > MAX_COMPONENT_BYTES {
            return Err(B1ErrorCode::Budget);
        }
        if is_git_alias(component) {
            return Err(B1ErrorCode::GitAlias);
        }
    }
    Ok(())
}

/// Symlink targets are data resolved relative to the link's directory; they must stay inside the root.
fn check_symlink_target(link: &str, target: &str) -> Result<(), B1ErrorCode> {
    if target.is_empty()
        || target.starts_with('/')
        || target.chars().any(|c| c == '\\' || c.is_control())
    {
        return Err(B1ErrorCode::SymlinkTarget);
    }
    if target.len() > MAX_PATH_BYTES {
        return Err(B1ErrorCode::Budget);
    }
    let mut depth = link.split('/').count() - 1;
    for component in target.split('/') {
        match component {
            "" => return Err(B1ErrorCode::SymlinkTarget),
            "." => {}
            ".." => {
                if depth == 0 {
                    return Err(B1ErrorCode::SymlinkTarget);
                }
                depth -= 1;
            }
            _ => {
                if component.len() > MAX_COMPONENT_BYTES {
                    return Err(B1ErrorCode::Budget);
                }
                if is_git_alias(component) {
                    return Err(B1ErrorCode::GitAlias);
                }
                depth += 1;
            }
        }
    }
    Ok(())
}

/// Case-insensitive, trailing dot and space stripped, so `.GIT.` and `.git ` alias `.git`.
fn is_git_alias(component: &str) -> bool {
    component
        .trim_end_matches(['.', ' '])
        .eq_ignore_ascii_case(".git")
}

fn collision_key(path: &str) -> String {
    path.split('/')
        .map(|component| {
            component
                .trim_end_matches(['.', ' '])
                .to_ascii_lowercase()
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn is_result_store_ref(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_RESULT_STORE_REF
        && matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9')
        && bytes
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Domain-separated, length-prefixed encoding so no two field sequences share a digest input.
struct Canonical(Sha256);

impl Canonical {
    fn new(domain: &str) -> Self {
        let mut canonical = Self(Sha256::new());
        canonical.text(domain);
        canonical
    }

    fn bytes(&mut self, value: &[u8]) {
        self.0.update((value.len() as u64).to_be_bytes());
        self.0.update(value);
    }

    fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn number(&mut self, value: u64) {
        self.bytes(&value.to_be_bytes());
    }

    fn finish(self) -> Sha256Hex {
        Sha256Hex::from_hash(self.0.finalize().as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASK: &str = "11111111-1111-4111-8111-111111111111";
    const REPO: &str = "22222222-2222-4222-8222-222222222222";
    const OWNER: &str = "33333333-3333-4333-8333-333333333333";
    const ISSUED: &str = "2026-10-15T10:00:00Z";

    fn code<T: fmt::Debug>(result: B1Result<T>) -> B1ErrorCode {
        result.unwrap_err().code
    }

    fn oid(c: char) -> GitOid {
        GitOid::try_from(c.to_string().repeat(40)).unwrap()
    }

    fn base(c: char, mode: GitFileMode) -> Option<B1BaseEntry> {
        Some(B1BaseEntry { oid: oid(c), mode })
    }

    fn file(bytes: &[u8]) -> Option<B1Content> {
        Some(B1Content {
            sha256: Sha256Hex::of(bytes),
            size: bytes.len() as u64,
            mode: GitFileMode::Regular,
        })
    }

    fn seal(mut entry: B1OverlayEntryV1) -> B1OverlayEntryV1 {
        entry.reviewed_digest = entry.entry_digest();
        entry
    }

    fn entry(
        path: &str,
        origin: B1ChangeOrigin,
        operation: B1Operation,
        base: Option<B1BaseEntry>,
        content: Option<B1Content>,
    ) -> B1OverlayEntryV1 {
        seal(B1OverlayEntryV1 {
            path: B1Path::parse(path).unwrap(),
            change_origin: origin,
            operation,
            entry_kind: B1EntryKind::Regular,
            base,
            rename_from: None,
            content,
            symlink_target: None,
            reviewed_digest: Sha256Hex::of(b""),
        })
    }

    fn selection(overlay: Vec<B1OverlayEntryV1>) -> B1SourceSelectionV1 {
        B1SourceSelectionV1 {
            schema_version: SCHEMA_VERSION,
            task_id: B1TaskId::parse(TASK).unwrap(),
            repository_id: B1RepositoryId::parse(REPO).unwrap(),
            owner_selection_id: B1OwnerSelectionId::parse(OWNER).unwrap(),
            base_commit: oid('a'),
            object_format: ObjectFormat::Sha1,
            policy_sha256: Sha256Hex::of(b"policy-v1"),
            issued_at: UtcTimestamp::try_from(ISSUED.to_owned()).unwrap(),
            overlay,
        }
    }

    /// Uncommitted work only: no commit exists for any of these changes.
    fn uncommitted_selection() -> B1SourceSelectionV1 {
        let mut rename = entry(
            "a/to.txt",
            B1ChangeOrigin::Unstaged,
            B1Operation::Rename,
            base('e', GitFileMode::Regular),
            file(b"renamed"),
        );
        rename.rename_from = Some(B1Path::parse("a/from.txt").unwrap());
        let rename = seal(rename);
        selection(vec![
            rename,
            entry(
                "notes/todo.md",
                B1ChangeOrigin::Untracked,
                B1Operation::Add,
                None,
                file(b"todo"),
            ),
            entry(
                "old.txt",
                B1ChangeOrigin::Unstaged,
                B1Operation::Delete,
                base('d', GitFileMode::Regular),
                None,
            ),
            entry(
                "src/lib.rs",
                B1ChangeOrigin::Unstaged,
                B1Operation::Modify,
                base('b', GitFileMode::Regular),
                file(b"unstaged edit"),
            ),
            entry(
                "src/main.rs",
                B1ChangeOrigin::Staged,
                B1Operation::Modify,
                base('c', GitFileMode::Regular),
                file(b"staged edit"),
            ),
        ])
    }

    fn receipt() -> B1QuarantineReceiptV1 {
        B1QuarantineReceiptV1 {
            schema_version: SCHEMA_VERSION,
            task_id: B1TaskId::parse(TASK).unwrap(),
            generation: 1,
            source_base_commit: oid('a'),
            admission_sha256: Sha256Hex::of(b"admission"),
            export_sha256: Sha256Hex::of(b"export"),
            result_store_ref: "quarantine-task-1".to_owned(),
            cas_revision: 1,
            result_tree: oid('f'),
            risk_flags: vec![B1RiskFlag::InstructionFile],
            status: B1ResultStatus::Pending,
        }
    }

    #[test]
    fn uncommitted_overlay_validates_and_round_trips() {
        let selection = uncommitted_selection();
        selection.validate().unwrap();
        let json = serde_json::to_string(&selection).unwrap();
        let parsed: B1SourceSelectionV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, selection);
        parsed.validate().unwrap();
    }

    #[test]
    fn admission_record_binds_exactly_the_validated_selection() {
        let selection = uncommitted_selection();
        let record = B1AdmissionRecordV1 {
            schema_version: SCHEMA_VERSION,
            task_id: selection.task_id.clone(),
            generation: 1,
            record_revision: 1,
            selection_sha256: selection.selection_sha256(),
            overlay_sha256: overlay_sha256(&selection.overlay),
            admitted_content_sha256: admitted_content_sha256(&selection.overlay),
        };
        record.validate_against(&selection).unwrap();

        let mut other = selection.clone();
        other.overlay.pop();
        assert_eq!(
            code(record.validate_against(&other)),
            B1ErrorCode::ManifestMismatch
        );

        let mut tampered = record.clone();
        tampered.admitted_content_sha256 = Sha256Hex::of(b"other");
        assert_eq!(
            code(tampered.validate_against(&selection)),
            B1ErrorCode::ManifestMismatch
        );

        let mut zero = record;
        zero.generation = 0;
        assert_eq!(
            code(zero.validate_against(&selection)),
            B1ErrorCode::StaleGeneration
        );
    }

    #[test]
    fn selection_digest_changes_with_every_bound_field() {
        let base_selection = uncommitted_selection();
        let mut policy = base_selection.clone();
        policy.policy_sha256 = Sha256Hex::of(b"policy-v2");
        let mut time = base_selection.clone();
        time.issued_at = UtcTimestamp::try_from("2026-10-15T10:00:01Z".to_owned()).unwrap();
        assert_eq!(base_selection.selection_sha256(), uncommitted_selection().selection_sha256());
        assert_ne!(base_selection.selection_sha256(), policy.selection_sha256());
        assert_ne!(base_selection.selection_sha256(), time.selection_sha256());
    }

    #[test]
    fn unknown_fields_are_rejected_at_both_levels() {
        let selection = uncommitted_selection();
        let mut value = serde_json::to_value(&selection).unwrap();
        value["surprise"] = serde_json::json!(true);
        assert!(serde_json::from_value::<B1SourceSelectionV1>(value).is_err());

        let mut value = serde_json::to_value(&selection).unwrap();
        value["overlay"][0]["extra"] = serde_json::json!("x");
        assert!(serde_json::from_value::<B1SourceSelectionV1>(value).is_err());
    }

    #[test]
    fn schema_version_and_base_oid_shape_are_enforced() {
        let mut selection = uncommitted_selection();
        selection.schema_version = 2;
        assert_eq!(code(selection.validate()), B1ErrorCode::SchemaVersion);

        let mut selection = uncommitted_selection();
        selection.object_format = ObjectFormat::Sha256;
        assert_eq!(code(selection.validate()), B1ErrorCode::Oid);

        assert!(GitOid::try_from("A".repeat(40)).is_err());
        assert!(GitOid::try_from("a".repeat(39)).is_err());
    }

    #[test]
    fn identifiers_are_validated() {
        assert_eq!(code(B1TaskId::parse("not-a-uuid")), B1ErrorCode::Identifier);
        assert!(serde_json::from_str::<B1TaskId>("\"not-a-uuid\"").is_err());
        assert_eq!(B1TaskId::parse(TASK).unwrap().as_str(), TASK);
    }

    #[test]
    fn invalid_paths_are_rejected() {
        for path in [
            "",
            "/etc/passwd",
            "a//b",
            "a/",
            "a/./b",
            "a/../b",
            "a\\b",
            "a\0b",
            "a\nb",
        ] {
            assert_eq!(code(B1Path::parse(path)), B1ErrorCode::Path, "{path:?}");
        }
        assert_eq!(code(B1Path::parse(&"x".repeat(256))), B1ErrorCode::Budget);
        assert_eq!(
            code(B1Path::parse(&vec!["a"; 33].join("/"))),
            B1ErrorCode::Budget
        );
        let long = vec!["abcdefghi"; 460].join("/");
        assert!(long.len() > MAX_PATH_BYTES);
        assert_eq!(code(B1Path::parse(&long)), B1ErrorCode::Budget);
        assert!(serde_json::from_str::<B1Path>("\"a/../b\"").is_err());
    }

    #[test]
    fn git_aliases_are_rejected_but_ordinary_names_pass() {
        for path in [".git", ".GIT", ".git.", ".git ", "x/.git/config", "x/.Git./hooks"] {
            assert_eq!(code(B1Path::parse(path)), B1ErrorCode::GitAlias, "{path:?}");
        }
        for path in [".gitignore", "git", "docs/git-notes.md", ".gitmodules"] {
            assert!(B1Path::parse(path).is_ok(), "{path:?}");
        }
    }

    #[test]
    fn path_collisions_include_case_trailing_dot_prefix_and_rename_source() {
        let a = entry("A.txt", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"1"));
        let b = entry("a.txt", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"2"));
        assert_eq!(code(validate_overlay(&[a, b])), B1ErrorCode::PathCollision);

        let dir = entry("a", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"1"));
        let child = entry("a/b", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"2"));
        assert_eq!(code(validate_overlay(&[dir, child])), B1ErrorCode::PathCollision);

        let trailing = entry("a.txt", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"1"));
        let dotted = entry("a.txt.", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"2"));
        assert_eq!(code(validate_overlay(&[trailing, dotted])), B1ErrorCode::PathCollision);

        let mut rename = entry(
            "z",
            B1ChangeOrigin::Staged,
            B1Operation::Rename,
            base('1', GitFileMode::Regular),
            file(b"z"),
        );
        rename.rename_from = Some(B1Path::parse("m").unwrap());
        let rename = seal(rename);
        let source = entry("m", B1ChangeOrigin::Unstaged, B1Operation::Modify, base('2', GitFileMode::Regular), file(b"m"));
        assert_eq!(code(validate_overlay(&[source, rename])), B1ErrorCode::PathCollision);
    }

    #[test]
    fn overlay_order_is_canonical_and_strict() {
        let b = entry("b", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"b"));
        let a = entry("a", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"a"));
        assert_eq!(code(validate_overlay(&[b, a])), B1ErrorCode::OverlayOrder);
        let a1 = entry("a", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"a"));
        let a2 = entry("a", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"a"));
        assert_eq!(code(validate_overlay(&[a1, a2])), B1ErrorCode::OverlayOrder);
    }

    #[test]
    fn git_modes_outside_the_supported_set_fail_deserialization() {
        for mode in ["160000", "040000", "120001", "100666", ""] {
            assert!(serde_json::from_value::<GitFileMode>(serde_json::json!(mode)).is_err());
        }
        assert_eq!(
            serde_json::from_value::<GitFileMode>(serde_json::json!("100755")).unwrap(),
            GitFileMode::Executable
        );
    }

    #[test]
    fn operation_origin_and_base_combinations_are_enforced() {
        let add_with_base = entry(
            "x",
            B1ChangeOrigin::Staged,
            B1Operation::Add,
            base('1', GitFileMode::Regular),
            file(b"x"),
        );
        assert_eq!(code(add_with_base.validate()), B1ErrorCode::OverlayShape);

        let untracked_modify = entry(
            "x",
            B1ChangeOrigin::Untracked,
            B1Operation::Modify,
            base('1', GitFileMode::Regular),
            file(b"x"),
        );
        assert_eq!(code(untracked_modify.validate()), B1ErrorCode::OverlayShape);

        let delete_with_content = entry(
            "x",
            B1ChangeOrigin::Unstaged,
            B1Operation::Delete,
            base('1', GitFileMode::Regular),
            file(b"x"),
        );
        assert_eq!(code(delete_with_content.validate()), B1ErrorCode::OverlayShape);

        let modify_without_base = entry(
            "x",
            B1ChangeOrigin::Unstaged,
            B1Operation::Modify,
            None,
            file(b"x"),
        );
        assert_eq!(code(modify_without_base.validate()), B1ErrorCode::OverlayShape);

        let mut rename_no_source = entry(
            "x",
            B1ChangeOrigin::Staged,
            B1Operation::Rename,
            base('1', GitFileMode::Regular),
            file(b"x"),
        );
        rename_no_source.rename_from = None;
        assert_eq!(code(seal(rename_no_source).validate()), B1ErrorCode::OverlayShape);

        let mut rename_self = entry(
            "x",
            B1ChangeOrigin::Staged,
            B1Operation::Rename,
            base('1', GitFileMode::Regular),
            file(b"x"),
        );
        rename_self.rename_from = Some(B1Path::parse("x").unwrap());
        assert_eq!(code(seal(rename_self).validate()), B1ErrorCode::OverlayShape);
    }

    #[test]
    fn file_kind_must_match_git_mode() {
        let mut regular_as_symlink = entry(
            "x",
            B1ChangeOrigin::Untracked,
            B1Operation::Add,
            None,
            Some(B1Content {
                sha256: Sha256Hex::of(b"x"),
                size: 1,
                mode: GitFileMode::Symlink,
            }),
        );
        regular_as_symlink.symlink_target = None;
        assert_eq!(code(seal(regular_as_symlink).validate()), B1ErrorCode::FileKind);

        let mut symlink_as_regular = entry(
            "docs/link",
            B1ChangeOrigin::Untracked,
            B1Operation::Add,
            None,
            file(b"../README.md"),
        );
        symlink_as_regular.entry_kind = B1EntryKind::Symlink;
        symlink_as_regular.symlink_target = Some("../README.md".to_owned());
        assert_eq!(code(seal(symlink_as_regular).validate()), B1ErrorCode::FileKind);
    }

    fn symlink(path: &str, target: &str) -> B1OverlayEntryV1 {
        let mut entry = entry(
            path,
            B1ChangeOrigin::Untracked,
            B1Operation::Add,
            None,
            Some(B1Content {
                sha256: Sha256Hex::of(target.as_bytes()),
                size: target.len() as u64,
                mode: GitFileMode::Symlink,
            }),
        );
        entry.entry_kind = B1EntryKind::Symlink;
        entry.symlink_target = Some(target.to_owned());
        seal(entry)
    }

    #[test]
    fn symlink_targets_must_stay_inside_the_root() {
        assert!(symlink("docs/link", "../README.md").validate().is_ok());
        assert!(symlink("docs/link", "sub/../other.md").validate().is_ok());

        assert_eq!(code(symlink("docs/link", "/etc/passwd").validate()), B1ErrorCode::SymlinkTarget);
        assert_eq!(code(symlink("lnk", "../outside").validate()), B1ErrorCode::SymlinkTarget);
        assert_eq!(code(symlink("docs/link", "../../outside").validate()), B1ErrorCode::SymlinkTarget);
        assert_eq!(code(symlink("docs/link", "a//b").validate()), B1ErrorCode::SymlinkTarget);
        assert_eq!(code(symlink("docs/link", "x/.git/config").validate()), B1ErrorCode::GitAlias);
    }

    #[test]
    fn symlink_content_digest_must_match_its_target() {
        let mut tampered = symlink("docs/link", "../README.md");
        tampered.symlink_target = Some("../OTHER.md".to_owned());
        assert_eq!(code(seal(tampered).validate()), B1ErrorCode::ContentMismatch);
    }

    #[test]
    fn reviewed_digest_detects_tampering_after_review() {
        let mut changed_size = entry(
            "src/lib.rs",
            B1ChangeOrigin::Unstaged,
            B1Operation::Modify,
            base('1', GitFileMode::Regular),
            file(b"abc"),
        );
        changed_size.content.as_mut().unwrap().size += 1;
        assert_eq!(code(changed_size.validate()), B1ErrorCode::ReviewDigestMismatch);

        let mut changed_digest = entry(
            "src/lib.rs",
            B1ChangeOrigin::Unstaged,
            B1Operation::Modify,
            base('1', GitFileMode::Regular),
            file(b"abc"),
        );
        changed_digest.reviewed_digest = Sha256Hex::of(b"not the review");
        assert_eq!(code(changed_digest.validate()), B1ErrorCode::ReviewDigestMismatch);
    }

    #[test]
    fn content_bytes_must_match_size_and_digest() {
        let content = B1Content {
            sha256: Sha256Hex::of(b"approved bytes"),
            size: 14,
            mode: GitFileMode::Regular,
        };
        verify_content_bytes(&content, b"approved bytes").unwrap();
        assert_eq!(
            code(verify_content_bytes(&content, b"approved bytez")),
            B1ErrorCode::ContentMismatch
        );
        assert_eq!(
            code(verify_content_bytes(&content, b"approved bytes!")),
            B1ErrorCode::ContentMismatch
        );
    }

    #[test]
    fn budgets_limit_file_size_entry_count_and_total_bytes() {
        let mut oversized = entry(
            "big.bin",
            B1ChangeOrigin::Untracked,
            B1Operation::Add,
            None,
            file(b"x"),
        );
        oversized.content.as_mut().unwrap().size = MAX_FILE_BYTES + 1;
        assert_eq!(code(seal(oversized).validate()), B1ErrorCode::Budget);

        let big_entries: Vec<B1OverlayEntryV1> = (0..9)
            .map(|index| {
                let mut e = entry(
                    &format!("f{index}"),
                    B1ChangeOrigin::Untracked,
                    B1Operation::Add,
                    None,
                    file(b"x"),
                );
                e.content.as_mut().unwrap().size = MAX_FILE_BYTES;
                seal(e)
            })
            .collect();
        assert_eq!(code(validate_overlay(&big_entries)), B1ErrorCode::Budget);

        let many: Vec<B1OverlayEntryV1> = (0..=MAX_OVERLAY_ENTRIES)
            .map(|index| entry(&format!("n{index:05}"), B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"x")))
            .collect();
        assert_eq!(code(validate_overlay(&many)), B1ErrorCode::Budget);
    }

    #[test]
    fn canonical_encoding_is_length_prefixed() {
        let mut left = Canonical::new("wyss.b1.test");
        left.text("ab");
        left.text("c");
        let mut right = Canonical::new("wyss.b1.test");
        right.text("a");
        right.text("bc");
        assert_ne!(left.finish(), right.finish());
    }

    #[test]
    fn lifecycle_follows_the_documented_chain_to_pending() {
        use B1State::*;
        let chain = [
            Created, Selecting, Captured, Admitted, Materialized, Confined, Running, Quiescing,
            Frozen, Exporting, Validated, Pending,
        ];
        let mut state = chain[0];
        for next in &chain[1..] {
            state = state.transition(*next).unwrap();
        }
        assert_eq!(state, Pending);
    }

    #[test]
    fn illegal_lifecycle_transitions_fail_closed() {
        use B1State::*;
        for (from, to) in [
            (Created, Running),
            (Selecting, Running),
            (Confined, Exporting),
            (Frozen, Pending),
            (Exporting, Pending),
            (Running, Rejected),
            (Frozen, FailedClean),
            (Pending, Rejected),
            (Rejected, Selecting),
            (FailedClean, BlockedUnsafe),
        ] {
            assert_eq!(
                code(from.transition(to)),
                B1ErrorCode::IllegalTransition,
                "{from:?} -> {to:?}"
            );
        }
    }

    #[test]
    fn cancellation_and_unsafe_states_are_reachable_without_skipping_quiescence() {
        use B1State::*;
        assert_eq!(Running.transition(Quiescing).unwrap(), Quiescing);
        assert_eq!(Quiescing.transition(FailedCleanupUnverified).unwrap(), FailedCleanupUnverified);
        assert_eq!(Quiescing.transition(FailedClean).unwrap(), FailedClean);
        assert_eq!(Running.transition(BlockedUnsafe).unwrap(), BlockedUnsafe);
        assert_eq!(Created.transition(Rejected).unwrap(), Rejected);
        assert_eq!(Confined.transition(Rejected).unwrap(), Rejected);
        assert_eq!(code(Running.transition(Rejected)), B1ErrorCode::IllegalTransition);
    }

    #[test]
    fn risk_flags_are_advisory_and_deterministic() {
        let overlay = vec![
            entry("AGENTS.md", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"a")),
            entry(".github/workflows/ci.yml", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"c")),
            entry("package.json", B1ChangeOrigin::Unstaged, B1Operation::Modify, base('1', GitFileMode::Regular), file(b"p")),
            entry("src/main.rs", B1ChangeOrigin::Staged, B1Operation::Modify, base('2', GitFileMode::Regular), file(b"s")),
        ];
        let flags = classify_risk_flags(&overlay);
        assert_eq!(
            flags,
            vec![
                B1RiskFlag::DelayedExecutionDirectory,
                B1RiskFlag::InstructionFile,
                B1RiskFlag::BuildOrHookScript,
            ]
        );
        assert!(classify_risk_flags(&[entry("src/lib.rs", B1ChangeOrigin::Untracked, B1Operation::Add, None, file(b"x"))]).is_empty());
    }

    #[test]
    fn quarantine_receipt_is_pending_and_bounded() {
        receipt().validate().unwrap();

        let mut value = serde_json::to_value(receipt()).unwrap();
        value["status"] = serde_json::json!("published");
        assert!(serde_json::from_value::<B1QuarantineReceiptV1>(value).is_err());

        for store_ref in ["../escape", "/tmp/x", "Upper", "", "a/b", &"a".repeat(65)] {
            let mut bad = receipt();
            bad.result_store_ref = store_ref.to_owned();
            assert_eq!(code(bad.validate()), B1ErrorCode::ResultStoreRef, "{store_ref:?}");
        }

        let mut unsorted = receipt();
        unsorted.risk_flags = vec![B1RiskFlag::SymlinkEntry, B1RiskFlag::ExecutableMode];
        assert_eq!(code(unsorted.validate()), B1ErrorCode::RiskFlags);

        let mut duplicated = receipt();
        duplicated.risk_flags = vec![B1RiskFlag::InstructionFile, B1RiskFlag::InstructionFile];
        assert_eq!(code(duplicated.validate()), B1ErrorCode::RiskFlags);
    }

    #[test]
    fn quarantine_publishes_once_with_cas_and_generation_checks() {
        let expected = B1PublicationExpectation { generation: 1, cas_revision: 0 };
        let mut slot = B1QuarantineSlot::default();

        let mut stale = receipt();
        stale.generation = 2;
        assert_eq!(code(slot.publish(stale, expected)), B1ErrorCode::StaleGeneration);

        let mut wrong_cas = receipt();
        wrong_cas.cas_revision = 5;
        assert_eq!(code(slot.publish(wrong_cas, expected)), B1ErrorCode::CasConflict);
        assert!(slot.published().is_none());

        slot.publish(receipt(), expected).unwrap();
        assert_eq!(slot.published(), Some(&receipt()));

        assert_eq!(
            code(slot.publish(receipt(), expected)),
            B1ErrorCode::DuplicatePublication
        );
    }

    #[test]
    fn errors_name_the_field_and_code_without_echoing_content() {
        let error = B1Path::parse("secret/../x").unwrap_err();
        let text = error.to_string();
        assert_eq!(text, "path: path");
        assert!(!text.contains("secret"));
    }
}
