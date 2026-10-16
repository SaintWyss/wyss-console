//! Owner-selected source identity and exact overlay entries (doc 52 §3).
//!
//! A selection never carries a repository path. The source location is not an authority, and
//! only the reviewed base object plus explicitly listed overlay entries may be admitted.
use super::{
    B1ErrorCode, B1Result, GitOid, MAX_ENTRIES, MAX_INPUT_BYTES, MAX_TOTAL_CONTENT_BYTES,
    ObjectFormat, SchemaV1, Sha256Hex, ensure,
    path::{B1LinkTarget, B1RelPath, validate_case_distinct, validate_destination_set},
};
use crate::contracts::{
    UtcTimestamp,
    identities::{B1PolicyId, B1RepoId, B1SelectionId, B1TaskId},
};
use serde::{Deserialize, Serialize};

pub const REGULAR_MODE: u32 = 0o100644;
pub const EXECUTABLE_MODE: u32 = 0o100755;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOperation {
    Add,
    Modify,
    Delete,
    Rename,
}

/// Which working set the owner selected the bytes from. Staged and unstaged versions of one
/// path are distinct selections, never merged. Untracked files may only be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOrigin {
    Staged,
    Unstaged,
    Untracked,
}

/// New content for a changed path. Gitlinks, special files and every unlisted mode are
/// unrepresentable, so they fail while deserializing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OverlayContent {
    Regular {
        content_sha256: Sha256Hex,
        size: u64,
        mode: u32,
    },
    Symlink {
        target: B1LinkTarget,
    },
}

impl OverlayContent {
    pub(super) fn validate(&self, path: &B1RelPath) -> B1Result {
        match self {
            Self::Regular { size, mode, .. } => {
                ensure(
                    matches!(*mode, REGULAR_MODE | EXECUTABLE_MODE),
                    "mode",
                    B1ErrorCode::ModeUnsupported,
                )?;
                ensure(*size <= MAX_INPUT_BYTES, "size", B1ErrorCode::LimitExceeded)
            }
            Self::Symlink { target } => target.validate_for(path),
        }
    }

    /// Bytes this content adds to the admitted total. Symlinks count their target text.
    pub(super) fn budget_bytes(&self) -> u64 {
        match self {
            Self::Regular { size, .. } => *size,
            Self::Symlink { target } => target.as_bytes().len() as u64,
        }
    }
}

/// One exact, owner-reviewed change relative to the pinned base.
///
/// Operation shape (enforced by [`B1OverlayEntryV1::validate`]):
/// - `add`: no base object, new content, origin `staged` or `untracked`.
/// - `modify`: base object, new content, origin `staged` or `unstaged`.
/// - `delete`: base object, no content (tombstone), origin `staged` or `unstaged`.
/// - `rename`: a destination with new content, `rename_from` a different source path, and
///   the source's base object. The source must also appear as a `delete` in the same overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1OverlayEntryV1 {
    pub path: B1RelPath,
    pub operation: ChangeOperation,
    pub origin: ChangeOrigin,
    pub expected_base_oid: Option<GitOid>,
    pub rename_from: Option<B1RelPath>,
    pub content: Option<OverlayContent>,
    /// Digest of the owner review record covering this entry. Provenance only.
    pub reviewed_sha256: Sha256Hex,
}

impl B1OverlayEntryV1 {
    pub fn validate(&self, format: ObjectFormat) -> B1Result {
        if let Some(oid) = &self.expected_base_oid {
            ensure(
                oid.format() == format,
                "expected_base_oid",
                B1ErrorCode::OidInvalid,
            )?;
        }
        let origin_is_valid = match self.operation {
            ChangeOperation::Add => self.origin != ChangeOrigin::Unstaged,
            ChangeOperation::Modify | ChangeOperation::Delete | ChangeOperation::Rename => {
                self.origin != ChangeOrigin::Untracked
            }
        };
        ensure(origin_is_valid, "origin", B1ErrorCode::OverlayInvalid)?;
        let shape_is_valid = match self.operation {
            ChangeOperation::Add => {
                self.expected_base_oid.is_none()
                    && self.rename_from.is_none()
                    && self.content.is_some()
            }
            ChangeOperation::Modify => {
                self.expected_base_oid.is_some()
                    && self.rename_from.is_none()
                    && self.content.is_some()
            }
            ChangeOperation::Delete => {
                self.expected_base_oid.is_some()
                    && self.rename_from.is_none()
                    && self.content.is_none()
            }
            ChangeOperation::Rename => {
                self.expected_base_oid.is_some()
                    && self.content.is_some()
                    && self
                        .rename_from
                        .as_ref()
                        .is_some_and(|from| from != &self.path)
            }
        };
        ensure(shape_is_valid, "operation", B1ErrorCode::OverlayInvalid)?;
        if let Some(content) = &self.content {
            content.validate(&self.path)?;
        }
        Ok(())
    }
}

/// Validates a complete overlay: canonical order, per-entry shape, budgets, collisions, and
/// rename pairing. Entries must be strictly ascending by raw path bytes, so a reordered or
/// duplicated list is refused rather than silently normalized.
///
/// Any two distinct paths that are equal after ASCII case folding are refused, including a
/// delete paired with an add. A case-only replacement therefore cannot slip past a
/// case-insensitive materializer. Deletes otherwise free their destination, so deleting `a`
/// and adding `a/b` remains a valid replacement.
pub fn validate_overlay(entries: &[B1OverlayEntryV1], format: ObjectFormat) -> B1Result {
    ensure(
        entries.len() <= MAX_ENTRIES,
        "admitted_overlay",
        B1ErrorCode::LimitExceeded,
    )?;
    ensure(
        entries.windows(2).all(|pair| pair[0].path < pair[1].path),
        "admitted_overlay",
        B1ErrorCode::NonCanonicalOrder,
    )?;
    let mut total_bytes = 0u64;
    for entry in entries {
        entry.validate(format)?;
        if let Some(content) = &entry.content {
            total_bytes = total_bytes
                .checked_add(content.budget_bytes())
                .filter(|total| *total <= MAX_TOTAL_CONTENT_BYTES)
                .ok_or(super::B1Error::new(
                    "admitted_overlay",
                    B1ErrorCode::LimitExceeded,
                ))?;
        }
    }
    validate_case_distinct(entries.iter().map(|entry| &entry.path))?;
    // Deletions free their destination, so a delete of `a` may be followed by an add of `a/b`.
    // Every other destination must be free of file/directory conflicts.
    validate_destination_set(
        entries
            .iter()
            .filter(|entry| entry.operation != ChangeOperation::Delete)
            .map(|entry| &entry.path),
    )?;
    for entry in entries
        .iter()
        .filter(|entry| entry.operation == ChangeOperation::Rename)
    {
        let source = entry.rename_from.as_ref().and_then(|from| {
            entries
                .binary_search_by(|candidate| candidate.path.cmp(from))
                .ok()
                .map(|index| &entries[index])
        });
        ensure(
            source.is_some_and(|source| {
                source.operation == ChangeOperation::Delete
                    && source.expected_base_oid == entry.expected_base_oid
            }),
            "rename_from",
            B1ErrorCode::OverlayInvalid,
        )?;
    }
    Ok(())
}

/// Owner's explicit selection for one task. Deserialization proves shape only; call
/// [`B1SourceSelectionV1::validate`] before use, as the other contracts in this crate do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B1SourceSelectionV1 {
    pub schema_version: SchemaV1,
    pub task_id: B1TaskId,
    pub selection_id: B1SelectionId,
    pub repo_id: B1RepoId,
    pub base_commit_oid: GitOid,
    pub repo_object_format: ObjectFormat,
    pub admitted_overlay: Vec<B1OverlayEntryV1>,
    pub policy_id: B1PolicyId,
    pub policy_sha256: Sha256Hex,
    pub issued_at: UtcTimestamp,
}

impl B1SourceSelectionV1 {
    pub fn validate(&self) -> B1Result {
        ensure(
            self.base_commit_oid.format() == self.repo_object_format,
            "base_commit_oid",
            B1ErrorCode::BaseOidInvalid,
        )?;
        validate_overlay(&self.admitted_overlay, self.repo_object_format)
    }
}
