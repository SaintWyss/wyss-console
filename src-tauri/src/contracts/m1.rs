//! Pure M1 workspace-store contract (`docs/contracts/m1-workspace.md` §2, ADR 0052): the closed
//! `StoreV1` records, their strongly typed identities, a strict bounded codec and the
//! cross-record validation that makes a decoded store safe to hold.
//!
//! This module is data and rules only. It opens no file, takes no lock, talks to no host,
//! authority or provider, and nothing here is reachable from the control protocol or from any
//! production path: a later increment owns the locked store, migration and launch. A record
//! that validates is well formed and internally consistent. It is never proof of
//! authorization, of a live session, or of an approved Managed profile.
//!
//! Native session and profile IDs (32 lowercase hex) and metadata identities (canonical
//! lowercase UUIDs) are different types, so neither can stand in for the other.
//!
//! ```compile_fail
//! use wyss_console::contracts::m1::scalar::{NativeId, TerminalId};
//! let native = NativeId::parse("0123456789abcdef0123456789abcdef").unwrap();
//! let terminal: TerminalId = native;
//! ```
//!
//! ```compile_fail
//! use wyss_console::contracts::m1::scalar::{ProjectId, WorkspaceId};
//! let project = ProjectId::parse("11111111-1111-4111-8111-111111111111").unwrap();
//! let workspace: WorkspaceId = project;
//! ```
use std::fmt;

pub mod codec;
pub mod records;
pub mod scalar;
mod validate;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

pub use codec::{decode, encode};
pub use records::StoreV1;

/// Store size, checked before any JSON parsing.
pub const MAX_STORE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PROJECTS: usize = 256;
pub const MAX_WORKSPACES_PER_PROJECT: usize = 32;
pub const MAX_BINDINGS_PER_PROJECT: usize = 16;
pub const MAX_ASSOCIATIONS: usize = 2000;
pub const MAX_OPERATIONS: usize = 512;
/// Compacted operation IDs, kept so a retried ID can never launch again.
pub const MAX_CLOSED_OPERATIONS: usize = 4096;
pub const MAX_MIGRATION_ROWS: usize = 4096;
pub const MAX_LEAVES: usize = 16;
/// The root is level 0; no node may sit deeper than this.
pub const MAX_LAYOUT_DEPTH: usize = 6;
pub const MIN_SPLIT_RATIO: f64 = 0.1;
pub const MAX_SPLIT_RATIO: f64 = 0.9;
pub const MAX_TEXT_SCALARS: usize = 64;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_EPOCH_BYTES: usize = 256;

/// The contract's error vocabulary for loading a store: a newer schema, a store that is not
/// valid, and a store past one of its ceilings. The caller shows a read-only recovery screen
/// for any of them; none is repaired and none is replaced by an empty store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum M1ErrorCode {
    UnsupportedSchema,
    InvalidRecord,
    CapacityExceeded,
}

impl M1ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedSchema => "unsupported_schema",
            Self::InvalidRecord => "invalid_record",
            Self::CapacityExceeded => "capacity_exceeded",
        }
    }
}

/// Deterministic diagnostics name a field and a code, never the rejected content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M1Error {
    pub field: &'static str,
    pub code: M1ErrorCode,
}

impl M1Error {
    pub(crate) const fn new(field: &'static str, code: M1ErrorCode) -> Self {
        Self { field, code }
    }
}

impl fmt::Display for M1Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.code.as_str())
    }
}

impl std::error::Error for M1Error {}

pub type M1Result<T = ()> = Result<T, M1Error>;

pub(crate) fn invalid(field: &'static str) -> M1Error {
    M1Error::new(field, M1ErrorCode::InvalidRecord)
}

pub(crate) fn ensure(condition: bool, field: &'static str) -> M1Result {
    if condition {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

pub(crate) fn within(count: usize, ceiling: usize, field: &'static str) -> M1Result {
    if count <= ceiling {
        Ok(())
    } else {
        Err(M1Error::new(field, M1ErrorCode::CapacityExceeded))
    }
}
