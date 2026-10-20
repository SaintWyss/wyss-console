//! Pure, bounded admission over explicitly supplied in-memory source records (docs 50 and 52).
//!
//! Admission is an authorization check, not a capture. It takes the owner's selection, a
//! description of the pinned base tree, and in-memory records that the caller says came from the
//! working set. It refuses anything the selection does not name, and anything whose bytes do not
//! match the selection's size and SHA-256. It then produces an owned, immutable snapshot and
//! admission evidence whose digests are computed here, never copied from input.
//!
//! What this module does not prove:
//! - It reads no filesystem, Git object database or process. Trusted capture of real bytes, with
//!   inode and mount isolation, is future work behind [`TrustedCapture`].
//! - Base object IDs are supplied identities. Git object hashing is not recomputed, because that
//!   needs a SHA-1 dependency this crate does not have.
//! - A correct digest is not proof that bytes were inspected or are harmless. It proves only that
//!   these bytes match the authorized size and hash, and that the owner selected them.
//!
//! Integrity guarantee: no digest in this module is read back from a stored field. The snapshot
//! tree digest hashes the actual bytes of every regular file each time it is computed, and
//! [`verify_snapshot`] additionally refuses an entry whose stored hash disagrees with its bytes.
//! A snapshot whose bytes changed after admission therefore fails verification even when it still
//! carries its original hash text.
use super::{
    B1Error, B1ErrorCode, B1Result, GitOid, MAX_ENTRIES, MAX_INPUT_BYTES, MAX_TOTAL_CONTENT_BYTES,
    SchemaV1, Sha256Hex, counter, digest,
    digest::{DigestContent, EntryOperation, ManifestEntry},
    ensure,
    path::{B1LinkTarget, B1RelPath, validate_case_distinct, validate_destination_set},
    records::B1AdmissionRecordV1,
    source::{
        B1OverlayEntryV1, B1SourceSelectionV1, ChangeOperation, ChangeOrigin, EXECUTABLE_MODE,
        OverlayContent, REGULAR_MODE,
    },
};
use crate::contracts::identities::B1TaskId;
use std::collections::{BTreeMap, BTreeSet};

/// The task a snapshot belongs to. Evidence from another task or generation is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskContext {
    task_id: B1TaskId,
    generation: u64,
}

impl TaskContext {
    pub fn new(task_id: B1TaskId, generation: u64) -> B1Result<Self> {
        counter("generation", generation)?;
        Ok(Self {
            task_id,
            generation,
        })
    }

    pub fn task_id(&self) -> &B1TaskId {
        &self.task_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Object identity of every tracked path in the pinned base commit. In tests this is synthetic.
/// A future trusted capture supplies it from a verified Git object read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseTree {
    base_oid: GitOid,
    objects: BTreeMap<B1RelPath, GitOid>,
}

impl BaseTree {
    pub fn new(base_oid: GitOid, entries: Vec<(B1RelPath, GitOid)>) -> B1Result<Self> {
        ensure(
            entries.len() <= MAX_ENTRIES,
            "base_tree",
            B1ErrorCode::LimitExceeded,
        )?;
        let format = base_oid.format();
        let mut objects = BTreeMap::new();
        for (path, oid) in entries {
            ensure(oid.format() == format, "base_tree", B1ErrorCode::OidInvalid)?;
            if objects.insert(path, oid).is_some() {
                return Err(B1Error::new("base_tree", B1ErrorCode::SourceCollision));
            }
        }
        validate_destination_set(objects.keys())?;
        Ok(Self { base_oid, objects })
    }

    pub fn base_oid(&self) -> &GitOid {
        &self.base_oid
    }

    fn object(&self, path: &B1RelPath) -> Option<&GitOid> {
        self.objects.get(path)
    }
}

/// Content the caller says it supplied. Anything other than a regular file or a symlink is
/// refused without being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedKind {
    Fifo,
    Socket,
    Device,
    Gitlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppliedContent {
    Regular { mode: u32, bytes: Vec<u8> },
    Symlink { target: Vec<u8> },
    Unsupported(UnsupportedKind),
}

/// One in-memory record. Nothing here is authorization: admission decides whether it matches the
/// owner's selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppliedRecord {
    pub path: B1RelPath,
    pub origin: ChangeOrigin,
    pub content: SuppliedContent,
}

/// Immutable content of one snapshot entry. It owns its bytes and shares nothing with the input.
///
/// `sha256` is a claim about `bytes`, never evidence. Every verification recomputes the hash from
/// `bytes` and refuses the entry when the two disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotContent {
    Tombstone,
    Regular {
        mode: u32,
        bytes: Vec<u8>,
        sha256: Sha256Hex,
    },
    Symlink {
        target: Vec<u8>,
    },
}

impl SnapshotContent {
    /// Bytes this content contributes to the admitted total. Symlinks count their target text.
    fn budget(&self) -> u64 {
        match self {
            Self::Tombstone => 0,
            Self::Regular { bytes, .. } => bytes.len() as u64,
            Self::Symlink { target } => target.len() as u64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotEntry {
    path: B1RelPath,
    operation: ChangeOperation,
    origin: ChangeOrigin,
    content: SnapshotContent,
}

impl SnapshotEntry {
    pub fn path(&self) -> &B1RelPath {
        &self.path
    }

    pub fn operation(&self) -> ChangeOperation {
        self.operation
    }

    pub fn origin(&self) -> ChangeOrigin {
        self.origin
    }

    pub fn content(&self) -> &SnapshotContent {
        &self.content
    }
}

/// The product of a successful admission: owned entries plus the record that describes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedSnapshot {
    entries: Vec<SnapshotEntry>,
    record: B1AdmissionRecordV1,
}

impl AdmittedSnapshot {
    pub fn entries(&self) -> &[SnapshotEntry] {
        &self.entries
    }

    pub fn record(&self) -> &B1AdmissionRecordV1 {
        &self.record
    }

    /// Recomputes this snapshot's integrity from its bytes. See [`verify_snapshot`].
    pub fn verify(&self, context: &TaskContext) -> B1Result {
        verify_snapshot(context, &self.record, &self.entries)
    }

    /// Verifies integrity and that the snapshot still equals the owner's selection. See
    /// [`verify_snapshot_against_selection`].
    pub fn verify_against(
        &self,
        context: &TaskContext,
        selection: &B1SourceSelectionV1,
    ) -> B1Result {
        verify_snapshot_against_selection(context, selection, &self.record, &self.entries)
    }
}

/// Interface for a future trusted capture that reads real bytes through an isolated, verified
/// boundary. It is deliberately not implemented here, and no unrestricted host access exists.
pub trait TrustedCapture {
    fn capture(&self, selection: &B1SourceSelectionV1) -> B1Result<Vec<SuppliedRecord>>;
}

fn check_base(overlay: &B1OverlayEntryV1, base: &BaseTree) -> B1Result {
    let mismatch = B1ErrorCode::SnapshotMismatch;
    match overlay.operation {
        ChangeOperation::Add => ensure(base.object(&overlay.path).is_none(), "path", mismatch),
        ChangeOperation::Modify | ChangeOperation::Delete => ensure(
            base.object(&overlay.path) == overlay.expected_base_oid.as_ref(),
            "expected_base_oid",
            mismatch,
        ),
        ChangeOperation::Rename => {
            let source_matches = overlay
                .rename_from
                .as_ref()
                .is_some_and(|from| base.object(from) == overlay.expected_base_oid.as_ref());
            ensure(source_matches, "rename_from", mismatch)?;
            ensure(base.object(&overlay.path).is_none(), "path", mismatch)
        }
    }
}

fn admit_content(
    overlay: &B1OverlayEntryV1,
    supplied: &SuppliedContent,
) -> B1Result<SnapshotContent> {
    let mismatch = B1ErrorCode::SnapshotMismatch;
    match (overlay.content.as_ref(), supplied) {
        (
            Some(OverlayContent::Regular {
                content_sha256,
                size,
                mode,
            }),
            SuppliedContent::Regular {
                mode: supplied_mode,
                bytes,
            },
        ) => {
            ensure(mode == supplied_mode, "mode", mismatch)?;
            digest::verify_content(bytes, content_sha256, *size)?;
            Ok(SnapshotContent::Regular {
                mode: *mode,
                bytes: bytes.clone(),
                sha256: content_sha256.clone(),
            })
        }
        (
            Some(OverlayContent::Symlink { target }),
            SuppliedContent::Symlink { target: supplied },
        ) => {
            ensure(target.as_bytes() == supplied.as_slice(), "target", mismatch)?;
            Ok(SnapshotContent::Symlink {
                target: supplied.clone(),
            })
        }
        _ => Err(B1Error::new("content", mismatch)),
    }
}

fn add_budget(total: u64, content: &SnapshotContent) -> B1Result<u64> {
    total
        .checked_add(content.budget())
        .filter(|sum| *sum <= MAX_TOTAL_CONTENT_BYTES)
        .ok_or(B1Error::new("admitted_bytes", B1ErrorCode::LimitExceeded))
}

/// Digest of the snapshot manifest. Every regular file is hashed here from its actual bytes, so a
/// stored `sha256` can never stand in for them.
fn tree_digest(
    context: &TaskContext,
    base_oid: &GitOid,
    entries: &[SnapshotEntry],
) -> B1Result<Sha256Hex> {
    ensure(
        entries.len() <= MAX_ENTRIES,
        "entries",
        B1ErrorCode::LimitExceeded,
    )?;
    let hashes: Vec<Option<Sha256Hex>> = entries
        .iter()
        .map(|entry| match &entry.content {
            SnapshotContent::Regular { bytes, .. } => Some(digest::raw_sha256(bytes)),
            _ => None,
        })
        .collect();
    let manifest: Vec<ManifestEntry<'_>> = entries
        .iter()
        .zip(&hashes)
        .map(|(entry, hash)| {
            let content = match (&entry.content, hash) {
                (SnapshotContent::Regular { mode, bytes, .. }, Some(sha256)) => {
                    DigestContent::Regular {
                        mode: *mode,
                        size: bytes.len() as u64,
                        sha256,
                    }
                }
                (SnapshotContent::Symlink { target }, _) => DigestContent::Symlink { target },
                _ => DigestContent::Tombstone,
            };
            ManifestEntry {
                path: &entry.path,
                operation: EntryOperation::from(entry.operation),
                content,
            }
        })
        .collect();
    digest::source_manifest(context.task_id(), context.generation(), base_oid, &manifest)
}

/// Admits supplied records against the owner's selection.
///
/// Refusals, in order: wrong task, invalid selection, base mismatch, duplicate or ambiguous
/// supplied paths, unsupported kinds (refused even when selected), oversized input, base
/// identity mismatch, origin or content mismatch, unselected content, and aggregate budget.
pub fn admit(
    context: &TaskContext,
    selection: &B1SourceSelectionV1,
    base: &BaseTree,
    supplied: &[SuppliedRecord],
    record_revision: u64,
) -> B1Result<AdmittedSnapshot> {
    ensure(
        selection.task_id == *context.task_id(),
        "task_id",
        B1ErrorCode::AdmissionUnauthorized,
    )?;
    selection.validate()?;
    ensure(
        base.base_oid() == &selection.base_commit_oid,
        "base_commit_oid",
        B1ErrorCode::BaseOidInvalid,
    )?;
    ensure(
        supplied.len() <= MAX_ENTRIES,
        "supplied",
        B1ErrorCode::LimitExceeded,
    )?;

    let mut by_path: BTreeMap<&B1RelPath, &SuppliedRecord> = BTreeMap::new();
    for record in supplied {
        if by_path.insert(&record.path, record).is_some() {
            return Err(B1Error::new("path", B1ErrorCode::SourceCollision));
        }
    }
    validate_case_distinct(supplied.iter().map(|record| &record.path))?;
    for record in supplied {
        match &record.content {
            SuppliedContent::Unsupported(_) => {
                return Err(B1Error::new("content", B1ErrorCode::InputKindUnsupported));
            }
            SuppliedContent::Regular { bytes, .. } => ensure(
                bytes.len() as u64 <= MAX_INPUT_BYTES,
                "size",
                B1ErrorCode::LimitExceeded,
            )?,
            SuppliedContent::Symlink { .. } => {}
        }
    }

    let mut selected_destinations: BTreeSet<&B1RelPath> = BTreeSet::new();
    let mut entries = Vec::with_capacity(selection.admitted_overlay.len());
    let mut total_bytes = 0u64;
    for overlay in &selection.admitted_overlay {
        check_base(overlay, base)?;
        let content = match overlay.operation {
            ChangeOperation::Delete => {
                ensure(
                    !by_path.contains_key(&overlay.path),
                    "path",
                    B1ErrorCode::SourceCollision,
                )?;
                SnapshotContent::Tombstone
            }
            _ => {
                selected_destinations.insert(&overlay.path);
                let record = by_path
                    .get(&overlay.path)
                    .ok_or(B1Error::new("path", B1ErrorCode::SnapshotMismatch))?;
                ensure(
                    record.origin == overlay.origin,
                    "origin",
                    B1ErrorCode::SnapshotMismatch,
                )?;
                admit_content(overlay, &record.content)?
            }
        };
        total_bytes = add_budget(total_bytes, &content)?;
        entries.push(SnapshotEntry {
            path: overlay.path.clone(),
            operation: overlay.operation,
            origin: overlay.origin,
            content,
        });
    }
    for record in supplied {
        ensure(
            selected_destinations.contains(&record.path),
            "path",
            B1ErrorCode::UnselectedContent,
        )?;
    }

    let tree = tree_digest(context, base.base_oid(), &entries)?;
    let approved_overlay = digest::overlay_manifest(
        context.task_id(),
        base.base_oid(),
        &selection.policy_sha256,
        &selection.admitted_overlay,
    )?;
    let record = B1AdmissionRecordV1 {
        schema_version: SchemaV1,
        task_id: context.task_id().clone(),
        selection_id: selection.selection_id.clone(),
        generation: context.generation(),
        record_revision,
        base_commit_oid: base.base_oid().clone(),
        policy_sha256: selection.policy_sha256.clone(),
        approved_overlay_sha256: approved_overlay,
        admitted_tree_sha256: tree,
        admitted_bytes: total_bytes,
        admitted_entries: entries.len() as u64,
    };
    record.validate()?;
    // Admission must never return a snapshot that verification would refuse.
    verify_snapshot(context, &record, &entries)?;
    Ok(AdmittedSnapshot { entries, record })
}

/// Entry-level integrity, independent of any record: shape, canonical order, path and link
/// rules, modes, per-file size and the claimed hash against the actual bytes. The aggregate
/// budget is checked before any byte is hashed.
fn verify_entries(entries: &[SnapshotEntry]) -> B1Result<u64> {
    ensure(
        entries.len() <= MAX_ENTRIES,
        "entries",
        B1ErrorCode::LimitExceeded,
    )?;
    ensure(
        entries.windows(2).all(|pair| pair[0].path < pair[1].path),
        "entries",
        B1ErrorCode::NonCanonicalOrder,
    )?;
    let mut total = 0u64;
    for entry in entries {
        total = add_budget(total, &entry.content)?;
    }
    validate_case_distinct(entries.iter().map(|entry| &entry.path))?;
    // Deletions free their destination, exactly as in overlay validation.
    validate_destination_set(
        entries
            .iter()
            .filter(|entry| entry.operation != ChangeOperation::Delete)
            .map(|entry| &entry.path),
    )?;
    for entry in entries {
        let overlay = EntryShape::of(entry);
        ensure(overlay.origin_fits, "origin", B1ErrorCode::OverlayInvalid)?;
        ensure(overlay.content_fits, "content", B1ErrorCode::OverlayInvalid)?;
        match &entry.content {
            SnapshotContent::Tombstone => {}
            SnapshotContent::Regular {
                mode,
                bytes,
                sha256,
            } => {
                ensure(
                    matches!(*mode, REGULAR_MODE | EXECUTABLE_MODE),
                    "mode",
                    B1ErrorCode::ModeUnsupported,
                )?;
                ensure(
                    bytes.len() as u64 <= MAX_INPUT_BYTES,
                    "size",
                    B1ErrorCode::LimitExceeded,
                )?;
                ensure(
                    digest::raw_sha256(bytes) == *sha256,
                    "sha256",
                    B1ErrorCode::ContentDigestMismatch,
                )?;
            }
            SnapshotContent::Symlink { target } => {
                B1LinkTarget::from_bytes(target)?.validate_for(&entry.path)?;
            }
        }
    }
    Ok(total)
}

/// What an entry's operation and origin allow, mirroring [`B1OverlayEntryV1::validate`].
struct EntryShape {
    origin_fits: bool,
    content_fits: bool,
}

impl EntryShape {
    fn of(entry: &SnapshotEntry) -> Self {
        let origin_fits = match entry.operation {
            ChangeOperation::Add => entry.origin != ChangeOrigin::Unstaged,
            _ => entry.origin != ChangeOrigin::Untracked,
        };
        let is_tombstone = matches!(entry.content, SnapshotContent::Tombstone);
        let content_fits = (entry.operation == ChangeOperation::Delete) == is_tombstone;
        Self {
            origin_fits,
            content_fits,
        }
    }
}

/// Checks that a record and a set of snapshot entries describe the same admitted tree for the
/// given context. Nothing is taken on trust from stored hash fields: every regular file is
/// re-hashed from its bytes, the totals are recounted, and the tree digest is recomputed.
///
/// Refusals, in order: wrong task, stale generation, invalid record, entry integrity (limits,
/// canonical order, shape, modes, link targets, stored hash against bytes), then totals and the
/// recomputed tree digest against the record.
///
/// This proves the entries are internally consistent and match the record. It does not prove the
/// record matches the owner's selection. Use [`verify_snapshot_against_selection`] for that.
pub fn verify_snapshot(
    context: &TaskContext,
    record: &B1AdmissionRecordV1,
    entries: &[SnapshotEntry],
) -> B1Result {
    ensure(
        record.task_id == *context.task_id(),
        "task_id",
        B1ErrorCode::AdmissionUnauthorized,
    )?;
    ensure(
        record.generation == context.generation(),
        "generation",
        B1ErrorCode::StaleGeneration,
    )?;
    record.validate()?;
    let total = verify_entries(entries)?;
    ensure(
        record.admitted_bytes == total && record.admitted_entries == entries.len() as u64,
        "admitted_bytes",
        B1ErrorCode::SnapshotMismatch,
    )?;
    let tree = tree_digest(context, &record.base_commit_oid, entries)?;
    ensure(
        tree == record.admitted_tree_sha256,
        "admitted_tree_sha256",
        B1ErrorCode::SnapshotMismatch,
    )
}

/// [`verify_snapshot`] plus agreement with the owner's selection: the record must be bound to the
/// selection, its approved-overlay digest must be recomputed from the selection, and every entry
/// must equal the overlay entry it was admitted for in path, operation, origin and content.
pub fn verify_snapshot_against_selection(
    context: &TaskContext,
    selection: &B1SourceSelectionV1,
    record: &B1AdmissionRecordV1,
    entries: &[SnapshotEntry],
) -> B1Result {
    ensure(
        selection.task_id == *context.task_id(),
        "task_id",
        B1ErrorCode::AdmissionUnauthorized,
    )?;
    selection.validate()?;
    verify_snapshot(context, record, entries)?;
    record.bind_selection(selection)?;
    let approved = digest::overlay_manifest(
        context.task_id(),
        &selection.base_commit_oid,
        &selection.policy_sha256,
        &selection.admitted_overlay,
    )?;
    let mismatch = B1ErrorCode::SnapshotMismatch;
    ensure(
        approved == record.approved_overlay_sha256,
        "approved_overlay_sha256",
        mismatch,
    )?;
    ensure(
        entries.len() == selection.admitted_overlay.len(),
        "entries",
        mismatch,
    )?;
    for (entry, overlay) in entries.iter().zip(&selection.admitted_overlay) {
        ensure(
            entry.path == overlay.path
                && entry.operation == overlay.operation
                && entry.origin == overlay.origin,
            "entries",
            mismatch,
        )?;
        let content_matches = match (&entry.content, overlay.content.as_ref()) {
            (SnapshotContent::Tombstone, None) => true,
            (
                SnapshotContent::Regular { mode, bytes, .. },
                Some(OverlayContent::Regular {
                    content_sha256,
                    size,
                    mode: overlay_mode,
                }),
            ) => {
                mode == overlay_mode
                    && bytes.len() as u64 == *size
                    && digest::raw_sha256(bytes) == *content_sha256
            }
            (SnapshotContent::Symlink { target }, Some(OverlayContent::Symlink { target: t })) => {
                target.as_slice() == t.as_bytes()
            }
            _ => false,
        };
        ensure(content_matches, "content", mismatch)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
