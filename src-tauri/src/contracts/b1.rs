//! Pure B1 contract foundation (docs 49-52): typed records and fail-closed validation for the
//! isolated-worker pipeline, from owner selection through a quarantined result.
//!
//! This module is data and rules only. It performs no filesystem, Git, process, sandbox,
//! broker or provider work, and nothing here is reachable from control protocol v1 or
//! `METHODS`. The one exception is `capture_fixture`, a synthetic filesystem fixture that is
//! compiled only into test builds and is not a trusted capture service. Validation proves that a record is well formed and internally consistent. It
//! never proves authorization, containment, or that admitted bytes are harmless.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

pub mod admission;
#[cfg(all(test, target_os = "linux"))]
mod capture_fixture;
pub mod digest;
pub mod lifecycle;
pub mod path;
pub mod records;
pub mod source;

#[cfg(test)]
mod tests;

/// Provisional test-profile budgets from doc 52 §7. They are fixed inputs, not measured results.
pub const MAX_ENTRIES: usize = 2048;
pub const MAX_INPUT_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_TOTAL_CONTENT_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_COMPONENT_BYTES: usize = 255;
pub const MAX_PATH_DEPTH: usize = 32;
pub const MAX_LINK_TARGET_BYTES: usize = 4096;
/// Generations and revisions use the signed range that execution leases already use.
pub const MAX_COUNTER: u64 = i64::MAX as u64;

/// Stable diagnostic codes. Errors never carry rejected path, digest or payload bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum B1ErrorCode {
    // Stage codes from doc 52 §4.
    AdmissionUnauthorized,
    BaseOidInvalid,
    SnapshotStale,
    InputKindUnsupported,
    SourceCollision,
    HardlinkOrAliasUnsafe,
    CaptureChanged,
    IsolationUnavailable,
    PreexecVerificationFailed,
    TaskTreeNotQuiesced,
    ExportNodeUnsafe,
    ExportDigestMismatch,
    QuarantineCasConflict,
    QuarantineRecoveryNeeded,
    ProviderAuthUnsupported,
    ProviderEgressUnproven,
    // Pure validation codes introduced by this contract.
    SchemaVersionInvalid,
    FieldInvalid,
    OidInvalid,
    DigestInvalid,
    PathUnsafe,
    PathEncodingNoncanonical,
    PathCollision,
    LinkTargetUnsafe,
    OverlayInvalid,
    NonCanonicalOrder,
    ModeUnsupported,
    LimitExceeded,
    CounterInvalid,
    InvalidTransition,
    PendingEvidenceMismatch,
    ReceiptInvalid,
    UnselectedContent,
    ContentDigestMismatch,
    ContentSizeMismatch,
    SnapshotMismatch,
    StaleGeneration,
}

impl B1ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AdmissionUnauthorized => "ADMISSION_UNAUTHORIZED",
            Self::BaseOidInvalid => "BASE_OID_INVALID",
            Self::SnapshotStale => "SNAPSHOT_STALE",
            Self::InputKindUnsupported => "INPUT_KIND_UNSUPPORTED",
            Self::SourceCollision => "SOURCE_COLLISION",
            Self::HardlinkOrAliasUnsafe => "HARDLINK_OR_ALIAS_UNSAFE",
            Self::CaptureChanged => "CAPTURE_CHANGED",
            Self::IsolationUnavailable => "ISOLATION_UNAVAILABLE",
            Self::PreexecVerificationFailed => "PREEXEC_VERIFICATION_FAILED",
            Self::TaskTreeNotQuiesced => "TASK_TREE_NOT_QUIESCED",
            Self::ExportNodeUnsafe => "EXPORT_NODE_UNSAFE",
            Self::ExportDigestMismatch => "EXPORT_DIGEST_MISMATCH",
            Self::QuarantineCasConflict => "QUARANTINE_CAS_CONFLICT",
            Self::QuarantineRecoveryNeeded => "QUARANTINE_RECOVERY_NEEDED",
            Self::ProviderAuthUnsupported => "PROVIDER_AUTH_UNSUPPORTED",
            Self::ProviderEgressUnproven => "PROVIDER_EGRESS_UNPROVEN",
            Self::SchemaVersionInvalid => "SCHEMA_VERSION_INVALID",
            Self::FieldInvalid => "FIELD_INVALID",
            Self::OidInvalid => "OID_INVALID",
            Self::DigestInvalid => "DIGEST_INVALID",
            Self::PathUnsafe => "PATH_UNSAFE",
            Self::PathEncodingNoncanonical => "PATH_ENCODING_NONCANONICAL",
            Self::PathCollision => "PATH_COLLISION",
            Self::LinkTargetUnsafe => "LINK_TARGET_UNSAFE",
            Self::OverlayInvalid => "OVERLAY_INVALID",
            Self::NonCanonicalOrder => "NON_CANONICAL_ORDER",
            Self::ModeUnsupported => "MODE_UNSUPPORTED",
            Self::LimitExceeded => "LIMIT_EXCEEDED",
            Self::CounterInvalid => "COUNTER_INVALID",
            Self::InvalidTransition => "INVALID_TRANSITION",
            Self::PendingEvidenceMismatch => "PENDING_EVIDENCE_MISMATCH",
            Self::ReceiptInvalid => "RECEIPT_INVALID",
            Self::UnselectedContent => "UNSELECTED_CONTENT",
            Self::ContentDigestMismatch => "CONTENT_DIGEST_MISMATCH",
            Self::ContentSizeMismatch => "CONTENT_SIZE_MISMATCH",
            Self::SnapshotMismatch => "SNAPSHOT_MISMATCH",
            Self::StaleGeneration => "STALE_GENERATION",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct B1Error {
    pub field: &'static str,
    pub code: B1ErrorCode,
}

impl B1Error {
    pub(crate) const fn new(field: &'static str, code: B1ErrorCode) -> Self {
        Self { field, code }
    }
}

impl fmt::Display for B1Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.code.as_str())
    }
}

impl std::error::Error for B1Error {}

pub(crate) type B1Result<T = ()> = Result<T, B1Error>;

pub(crate) fn ensure(condition: bool, field: &'static str, code: B1ErrorCode) -> B1Result {
    if condition {
        Ok(())
    } else {
        Err(B1Error::new(field, code))
    }
}

pub(crate) fn counter(field: &'static str, value: u64) -> B1Result {
    ensure(
        (1..=MAX_COUNTER).contains(&value),
        field,
        B1ErrorCode::CounterInvalid,
    )
}

/// Wire marker for `schema_version: 1`. Any other value is rejected while deserializing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaV1;

impl Serialize for SchemaV1 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(1)
    }
}

impl<'de> Deserialize<'de> for SchemaV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u8::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(serde::de::Error::custom("schema_version")),
        }
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Lowercase hex SHA-256 digest text. Digest domains are assigned by the records that carry them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256Hex(String);

impl Sha256Hex {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Builds the digest of computed bytes. It cannot fail, because the text is always 64
    /// lowercase hex characters.
    pub(crate) fn from_raw(raw: [u8; 32]) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let text = raw
            .iter()
            .flat_map(|byte| {
                [
                    char::from(HEX[usize::from(byte >> 4)]),
                    char::from(HEX[usize::from(byte & 0x0f)]),
                ]
            })
            .collect();
        Self(text)
    }

    pub(crate) fn to_raw(&self) -> Option<[u8; 32]> {
        let bytes = self.0.as_bytes();
        let mut raw = [0u8; 32];
        for (index, pair) in bytes.chunks_exact(2).enumerate() {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            raw[index] = u8::try_from(high * 16 + low).ok()?;
        }
        Some(raw)
    }
}

impl TryFrom<String> for Sha256Hex {
    type Error = B1Error;

    fn try_from(value: String) -> B1Result<Self> {
        ensure(
            is_lower_hex(&value, 64),
            "sha256",
            B1ErrorCode::DigestInvalid,
        )?;
        Ok(Self(value))
    }
}

impl From<Sha256Hex> for String {
    fn from(value: Sha256Hex) -> Self {
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
    pub const fn oid_length(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }
}

/// Lowercase full-length Git object ID. Its length selects the object format; the all-zero
/// value is the null object and is never a valid base or tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GitOid(String);

impl GitOid {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn format(&self) -> ObjectFormat {
        if self.0.len() == ObjectFormat::Sha1.oid_length() {
            ObjectFormat::Sha1
        } else {
            ObjectFormat::Sha256
        }
    }
}

impl TryFrom<String> for GitOid {
    type Error = B1Error;

    fn try_from(value: String) -> B1Result<Self> {
        let valid_length = is_lower_hex(&value, ObjectFormat::Sha1.oid_length())
            || is_lower_hex(&value, ObjectFormat::Sha256.oid_length());
        ensure(
            valid_length && value.bytes().any(|byte| byte != b'0'),
            "oid",
            B1ErrorCode::OidInvalid,
        )?;
        Ok(Self(value))
    }
}

impl From<GitOid> for String {
    fn from(value: GitOid) -> Self {
        value.0
    }
}
