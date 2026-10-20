//! Canonical, versioned, domain-separated digests for B1 content and records.
//!
//! Digests are computed over an explicit binary encoding, never over JSON text, so key order
//! and whitespace cannot change them. Each message starts with a domain tag, the encoding
//! version and a purpose label, so a digest made for one purpose cannot be replayed as another.
//!
//! Encoding version 1:
//! - `bytes(x)` is the u64 big-endian length of `x`, then `x`.
//! - integers are fixed-width big-endian. A task UUID is its 16 raw bytes. A SHA-256 value is
//!   its 32 raw bytes. An optional value is `0` when absent, or `1` followed by the value.
//! - content is `0` for a tombstone, `1` followed by mode, size and content hash for a regular
//!   file, or `2` followed by the target bytes for a symlink.
//!
//! [`raw_sha256`] is the plain SHA-256 of content bytes, so an owner can reproduce it with
//! standard tools. Everything else is a binding digest. A digest is evidence that specific bytes
//! were hashed. It is not authorization, and it is not proof that content was inspected or
//! admitted. Those decisions belong to the selection and the admission step.
use super::{
    B1Error, B1ErrorCode, B1Result, GitOid, MAX_ENTRIES, MAX_INPUT_BYTES, Sha256Hex, ensure,
    path::B1RelPath,
    records::{B1AdmissionRecordV1, B1ExportRecordV1, ExportOperation},
    source::{B1OverlayEntryV1, ChangeOperation, ChangeOrigin, OverlayContent},
};
use crate::contracts::identities::B1TaskId;
use sha2::{Digest, Sha256};

pub const ENCODING_VERSION: u8 = 1;
const DOMAIN_TAG: &[u8] = b"wyss-console.b1.digest\0";

#[derive(Debug, Clone, Copy)]
enum Purpose {
    ContentBinding,
    SourceManifest,
    OverlayManifest,
    AdmissionRecord,
    ExportRecord,
}

impl Purpose {
    const fn label(self) -> &'static [u8] {
        match self {
            Self::ContentBinding => b"content-binding",
            Self::SourceManifest => b"source-manifest",
            Self::OverlayManifest => b"overlay-manifest",
            Self::AdmissionRecord => b"admission-record",
            Self::ExportRecord => b"export-record",
        }
    }
}

/// Operation as it appears in a digest. Renames are a distinct tag, never a delete plus add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryOperation {
    Add,
    Modify,
    Delete,
    Rename,
}

impl EntryOperation {
    const fn tag(self) -> u8 {
        match self {
            Self::Add => 1,
            Self::Modify => 2,
            Self::Delete => 3,
            Self::Rename => 4,
        }
    }
}

impl From<ChangeOperation> for EntryOperation {
    fn from(operation: ChangeOperation) -> Self {
        match operation {
            ChangeOperation::Add => Self::Add,
            ChangeOperation::Modify => Self::Modify,
            ChangeOperation::Delete => Self::Delete,
            ChangeOperation::Rename => Self::Rename,
        }
    }
}

impl From<ExportOperation> for EntryOperation {
    fn from(operation: ExportOperation) -> Self {
        match operation {
            ExportOperation::Add => Self::Add,
            ExportOperation::Modify => Self::Modify,
            ExportOperation::Delete => Self::Delete,
        }
    }
}

/// Content as it appears in a digest, borrowed from its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestContent<'a> {
    Tombstone,
    Regular {
        mode: u32,
        size: u64,
        sha256: &'a Sha256Hex,
    },
    Symlink {
        target: &'a [u8],
    },
}

impl<'a> DigestContent<'a> {
    pub fn of(content: Option<&'a OverlayContent>) -> Self {
        match content {
            None => Self::Tombstone,
            Some(OverlayContent::Regular {
                content_sha256,
                size,
                mode,
            }) => Self::Regular {
                mode: *mode,
                size: *size,
                sha256: content_sha256,
            },
            Some(OverlayContent::Symlink { target }) => Self::Symlink {
                target: target.as_bytes(),
            },
        }
    }
}

/// One entry of a manifest. Paths must be supplied in strictly ascending byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestEntry<'a> {
    pub path: &'a B1RelPath,
    pub operation: EntryOperation,
    pub content: DigestContent<'a>,
}

/// Accumulates one versioned message. Fields are appended in the fixed order of the grammar.
struct Encoder(Vec<u8>);

impl Encoder {
    fn new(purpose: Purpose) -> Self {
        let mut encoder = Self(Vec::with_capacity(256));
        encoder.0.extend_from_slice(DOMAIN_TAG);
        encoder.0.push(ENCODING_VERSION);
        encoder.bytes(purpose.label());
        encoder
    }

    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.u64(value.len() as u64);
        self.0.extend_from_slice(value);
    }

    fn uuid(&mut self, text: &str) -> B1Result {
        let parsed = uuid::Uuid::parse_str(text)
            .map_err(|_| B1Error::new("task_id", B1ErrorCode::FieldInvalid))?;
        self.0.extend_from_slice(parsed.as_bytes());
        Ok(())
    }

    fn sha(&mut self, digest: &Sha256Hex) -> B1Result {
        let raw = digest
            .to_raw()
            .ok_or(B1Error::new("sha256", B1ErrorCode::DigestInvalid))?;
        self.0.extend_from_slice(&raw);
        Ok(())
    }

    fn optional_oid(&mut self, oid: Option<&GitOid>) {
        match oid {
            None => self.u8(0),
            Some(oid) => {
                self.u8(1);
                self.bytes(oid.as_str().as_bytes());
            }
        }
    }

    fn content(&mut self, content: DigestContent<'_>) -> B1Result {
        match content {
            DigestContent::Tombstone => self.u8(0),
            DigestContent::Regular { mode, size, sha256 } => {
                self.u8(1);
                self.u32(mode);
                self.u64(size);
                self.sha(sha256)?;
            }
            DigestContent::Symlink { target } => {
                self.u8(2);
                self.bytes(target);
            }
        }
        Ok(())
    }

    fn finish(self) -> Sha256Hex {
        Sha256Hex::from_raw(Sha256::digest(&self.0).into())
    }
}

fn ensure_count(count: usize) -> B1Result {
    ensure(count <= MAX_ENTRIES, "entries", B1ErrorCode::LimitExceeded)
}

/// Plain SHA-256 of content bytes, which an owner can reproduce with standard tools.
pub fn raw_sha256(bytes: &[u8]) -> Sha256Hex {
    Sha256Hex::from_raw(Sha256::digest(bytes).into())
}

/// Verifies supplied bytes against their declared size and hash. Size is checked before
/// hashing, so an oversized input is refused without being hashed.
pub fn verify_content(bytes: &[u8], expected_sha256: &Sha256Hex, expected_size: u64) -> B1Result {
    let actual_size = bytes.len() as u64;
    ensure(
        actual_size <= MAX_INPUT_BYTES,
        "size",
        B1ErrorCode::LimitExceeded,
    )?;
    ensure(
        actual_size == expected_size,
        "size",
        B1ErrorCode::ContentSizeMismatch,
    )?;
    ensure(
        raw_sha256(bytes) == *expected_sha256,
        "sha256",
        B1ErrorCode::ContentDigestMismatch,
    )
}

/// Binds one content record to its task, generation and path.
pub fn content_binding(
    task_id: &B1TaskId,
    generation: u64,
    path: &B1RelPath,
    content: DigestContent<'_>,
) -> B1Result<Sha256Hex> {
    let mut encoder = Encoder::new(Purpose::ContentBinding);
    encoder.uuid(task_id.as_str())?;
    encoder.u64(generation);
    encoder.bytes(path.as_bytes());
    encoder.content(content)?;
    Ok(encoder.finish())
}

fn check_ascending<'a>(paths: impl Iterator<Item = &'a B1RelPath>) -> B1Result {
    let mut previous: Option<&B1RelPath> = None;
    for path in paths {
        ensure(
            previous.is_none_or(|last| last < path),
            "entries",
            B1ErrorCode::NonCanonicalOrder,
        )?;
        previous = Some(path);
    }
    Ok(())
}

/// Digest of a source or snapshot manifest for one task, generation and base object.
pub fn source_manifest(
    task_id: &B1TaskId,
    generation: u64,
    base: &GitOid,
    entries: &[ManifestEntry<'_>],
) -> B1Result<Sha256Hex> {
    ensure_count(entries.len())?;
    check_ascending(entries.iter().map(|entry| entry.path))?;
    let mut encoder = Encoder::new(Purpose::SourceManifest);
    encoder.uuid(task_id.as_str())?;
    encoder.u64(generation);
    encoder.bytes(base.as_str().as_bytes());
    encoder.u64(entries.len() as u64);
    for entry in entries {
        encoder.bytes(entry.path.as_bytes());
        encoder.u8(entry.operation.tag());
        encoder.content(entry.content)?;
    }
    Ok(encoder.finish())
}

fn origin_tag(origin: ChangeOrigin) -> u8 {
    match origin {
        ChangeOrigin::Staged => 1,
        ChangeOrigin::Unstaged => 2,
        ChangeOrigin::Untracked => 3,
    }
}

/// Digest of the owner's approved overlay. It binds every field the owner reviewed, including
/// origin and the expected base object of each path.
pub fn overlay_manifest(
    task_id: &B1TaskId,
    base: &GitOid,
    policy_sha256: &Sha256Hex,
    entries: &[B1OverlayEntryV1],
) -> B1Result<Sha256Hex> {
    ensure_count(entries.len())?;
    check_ascending(entries.iter().map(|entry| &entry.path))?;
    let mut encoder = Encoder::new(Purpose::OverlayManifest);
    encoder.uuid(task_id.as_str())?;
    encoder.bytes(base.as_str().as_bytes());
    encoder.sha(policy_sha256)?;
    encoder.u64(entries.len() as u64);
    for entry in entries {
        encoder.bytes(entry.path.as_bytes());
        encoder.u8(EntryOperation::from(entry.operation).tag());
        encoder.u8(origin_tag(entry.origin));
        encoder.optional_oid(entry.expected_base_oid.as_ref());
        match &entry.rename_from {
            None => encoder.u8(0),
            Some(from) => {
                encoder.u8(1);
                encoder.bytes(from.as_bytes());
            }
        }
        encoder.content(DigestContent::of(entry.content.as_ref()))?;
        encoder.sha(&entry.reviewed_sha256)?;
    }
    Ok(encoder.finish())
}

/// Digest of an admission record. Every field is covered, so a changed count or byte total
/// changes the digest that the quarantine receipt must reproduce.
pub fn admission_record(record: &B1AdmissionRecordV1) -> B1Result<Sha256Hex> {
    let mut encoder = Encoder::new(Purpose::AdmissionRecord);
    encoder.uuid(record.task_id.as_str())?;
    encoder.uuid(record.selection_id.as_str())?;
    encoder.u64(record.generation);
    encoder.u64(record.record_revision);
    encoder.bytes(record.base_commit_oid.as_str().as_bytes());
    encoder.sha(&record.policy_sha256)?;
    encoder.sha(&record.approved_overlay_sha256)?;
    encoder.sha(&record.admitted_tree_sha256)?;
    encoder.u64(record.admitted_bytes);
    encoder.u64(record.admitted_entries);
    Ok(encoder.finish())
}

/// Digest of an export record, covering every exported entry in the order it is stored.
pub fn export_record(record: &B1ExportRecordV1) -> B1Result<Sha256Hex> {
    ensure_count(record.entries.len())?;
    let mut encoder = Encoder::new(Purpose::ExportRecord);
    encoder.uuid(record.task_id.as_str())?;
    encoder.u64(record.generation);
    encoder.sha(&record.base_admission_sha256)?;
    encoder.u64(record.entries.len() as u64);
    for entry in &record.entries {
        encoder.bytes(entry.path.as_bytes());
        encoder.u8(EntryOperation::from(entry.operation).tag());
        encoder.content(DigestContent::of(entry.content.as_ref()))?;
    }
    encoder.sha(&record.exporter_sha256)?;
    encoder.sha(&record.source_tree_sha256)?;
    Ok(encoder.finish())
}
