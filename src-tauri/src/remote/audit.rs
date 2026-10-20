//! The gateway's own journal (16 §5.2.6): append-only, bounded, closed words and identifiers.
//! A record has **no field for content**: not terminal bytes, not keystrokes (only a count), not
//! configuration, not a name that came from a file. A full journal refuses new requests instead of
//! dropping a record (`audit_unavailable`).
use super::{Refusal, device::DeviceId, grants::Grant};
use serde::Serialize;

pub const MOST_BYTES: usize = 16 * 1024 * 1024;
pub const KEEP_SECONDS: u64 = 90 * 86_400;

/// How a request ended, as a closed word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Allowed,
    Refused(Refusal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Record {
    pub at: u64,
    pub device: DeviceId,
    pub fingerprint: String,
    pub connection: String,
    pub request_id: String,
    pub grant: Grant,
    pub operation: String,
    pub target: Option<String>,
    /// For input, how many bytes: never which.
    pub bytes: Option<u64>,
    pub outcome: Outcome,
}

/// An identifier the journal will keep: ASCII letters, digits and `. _ - : /`, short. Anything
/// else (a title, a path, a name from a file) is refused at the door, not trimmed.
pub fn identifier_ok(text: &str, most: usize) -> bool {
    !text.is_empty()
        && text.len() <= most
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'/'))
}

impl Record {
    pub fn size(&self) -> usize {
        // A deliberately generous account of what a record weighs, so the bound is never hit late.
        160 + self.fingerprint.len()
            + self.connection.len()
            + self.request_id.len()
            + self.operation.len()
            + self.target.as_ref().map_or(0, String::len)
    }
    pub fn valid(&self) -> bool {
        identifier_ok(&self.connection, 64)
            && identifier_ok(&self.request_id, 64)
            && identifier_ok(&self.operation, 64)
            && self.target.as_deref().is_none_or(|t| identifier_ok(t, 128))
            && self.fingerprint.len() == 64
            && self.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full;

pub trait Journal {
    /// Appends, or says it cannot: the caller then refuses the request.
    fn append(&mut self, record: Record) -> Result<(), Full>;
    /// Whether one more record would be taken now.
    fn has_room(&self) -> bool;
}

/// A journal in memory, with the bounds of the design. A file-backed one waits for the state
/// directory (DN-31) and for the host's persistence decisions.
#[derive(Default)]
pub struct MemoryJournal {
    records: Vec<Record>,
    bytes: usize,
    limit: Option<usize>,
}
impl MemoryJournal {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit: Some(limit),
            ..Self::default()
        }
    }
    pub fn records(&self) -> &[Record] {
        &self.records
    }
    /// Records older than 90 days go; nothing newer ever does.
    pub fn prune(&mut self, now: u64) -> usize {
        let before = self.records.len();
        self.records
            .retain(|r| now.saturating_sub(r.at) < KEEP_SECONDS);
        self.bytes = self.records.iter().map(Record::size).sum();
        before - self.records.len()
    }
}
impl Journal for MemoryJournal {
    fn append(&mut self, record: Record) -> Result<(), Full> {
        let limit = self.limit.unwrap_or(MOST_BYTES);
        if !record.valid() || self.bytes + record.size() > limit {
            return Err(Full);
        }
        self.bytes += record.size();
        self.records.push(record);
        Ok(())
    }
    fn has_room(&self) -> bool {
        self.bytes + 512 <= self.limit.unwrap_or(MOST_BYTES)
    }
}
