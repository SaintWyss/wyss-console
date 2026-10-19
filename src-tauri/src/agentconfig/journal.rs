//! The journal of changes (14 §3, 11 §5.5): one line per fact, appended and flushed, with
//! no content of any entry and no value. It is what the History view shows, what an undo
//! is built from, and what start-up repair reads after a write was cut short.
//!
//! An apply writes an `intent` before it touches the provider's file and an `outcome`
//! after. If the intent cannot be written, nothing is done: a privileged operation fails
//! closed when its auditable record cannot be made (ADR 0003).
use super::change::{Operation, Outcome, Prior, Risk};
use super::fault::{Faults, Injected, Step};
use super::fsutil::{StoreError, private_dir};
use super::inventory::SourceKey;
use super::model::ProviderId;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const FILE: &str = "journal.jsonl";
/// 14 §3: ten thousand entries or a hundred and eighty days.
pub const MOST_ENTRIES: usize = 10_000;
pub const MOST_AGE_SECONDS: u64 = 180 * 24 * 3600;
const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Intent,
    Outcome,
}

/// One line. Hashes are SHA-256 in hex; `item` is the inventory's opaque identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub v: u32,
    /// Shared by the intent and the outcome of one apply.
    pub id: String,
    pub at: u64,
    pub phase: Phase,
    pub provider: ProviderId,
    pub source: SourceKey,
    pub operation: Operation,
    pub item: String,
    pub risk: Risk,
    /// The file before, and the file the plan produces (intent) or that was found (outcome).
    pub h0: String,
    pub h1: Option<String>,
    pub outcome: Option<Outcome>,
    /// The restoration record this change made or used, if any.
    pub record: Option<String>,
    /// The entry this one undoes, if it is an undo.
    pub undoes: Option<String>,
    /// For a switch: what its key held before, so the undo is exact. A closed word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior: Option<Prior>,
}

/// An apply as the History view shows it: both lines folded into one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub id: String,
    pub at: u64,
    pub provider: ProviderId,
    pub source: SourceKey,
    pub operation: Operation,
    pub item: String,
    pub risk: Risk,
    pub h0: String,
    pub h1: Option<String>,
    /// `None` while the apply is in flight or its end was never recorded.
    pub outcome: Option<Outcome>,
    pub record: Option<String>,
    pub undoes: Option<String>,
    pub prior: Option<Prior>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    /// The journal could not be written: the caller must not go on.
    NotRecorded,
    Crashed,
}

pub struct Journal {
    dir: PathBuf,
    faults: Faults,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

pub fn new_id() -> String {
    let mut bytes = [0_u8; 16];
    // Without randomness an id would repeat; the zero id is refused by `append`.
    let _ = getrandom::fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Journal {
    pub fn open(state: &Path, faults: Faults) -> Result<Self, StoreError> {
        Ok(Self {
            dir: private_dir(&state.join("journal"))?,
            faults,
        })
    }

    fn path(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    /// Appends one line and flushes it. A line is one `write`, so a crash leaves either
    /// all of it or a torn last line, which readers skip.
    pub fn append(&self, entry: &Entry, step: Step) -> Result<(), JournalError> {
        if entry.id.is_empty() || entry.id.bytes().all(|b| b == b'0') {
            return Err(JournalError::NotRecorded);
        }
        let mut line = serde_json::to_vec(entry).map_err(|_| JournalError::NotRecorded)?;
        line.push(b'\n');
        match self.faults.check(step) {
            Err(Injected::Failed) => return Err(JournalError::NotRecorded),
            Err(Injected::Crashed) => return Err(JournalError::Crashed),
            Ok(()) => (),
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(self.path())
            .map_err(|_| JournalError::NotRecorded)?;
        file.write_all(&line)
            .and_then(|()| file.sync_all())
            .map_err(|_| JournalError::NotRecorded)
    }

    /// Every line that parses, oldest first. A line that does not is skipped and counted:
    /// a torn last line is what a crash leaves, and is not an error.
    pub fn entries(&self) -> (Vec<Entry>, usize) {
        let Ok(text) = fs::read(self.path()) else {
            return (Vec::new(), 0);
        };
        let (mut entries, mut damaged) = (Vec::new(), 0);
        for line in text.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            match serde_json::from_slice::<Entry>(line) {
                Ok(entry) if entry.v == VERSION => entries.push(entry),
                _ => damaged += 1,
            }
        }
        (entries, damaged)
    }

    /// Applies folded: newest first.
    pub fn changes(&self) -> Vec<Change> {
        fold(&self.entries().0)
    }

    pub fn change(&self, id: &str) -> Option<Change> {
        self.changes().into_iter().find(|change| change.id == id)
    }

    /// A page of changes, newest first, after `cursor` (the id of the last one seen).
    pub fn history(&self, cursor: Option<&str>, limit: usize) -> (Vec<Change>, Option<String>) {
        let all = self.changes();
        let start = cursor
            .and_then(|id| all.iter().position(|change| change.id == id))
            .map_or(0, |index| index + 1);
        let limit = limit.clamp(1, 50);
        let page: Vec<Change> = all.iter().skip(start).take(limit).cloned().collect();
        let next = (start + page.len() < all.len())
            .then(|| page.last().map(|change| change.id.clone()))
            .flatten();
        (page, next)
    }

    /// Drops what is past the bounds, oldest first, by writing a new file beside and
    /// renaming it over. An apply still in flight (an intent with no outcome) is kept.
    pub fn prune(&self, now: u64) -> Result<usize, StoreError> {
        let (entries, _) = self.entries();
        let open: std::collections::HashSet<&str> = entries
            .iter()
            .filter(|e| e.phase == Phase::Intent)
            .filter(|e| {
                !entries
                    .iter()
                    .any(|o| o.id == e.id && o.phase == Phase::Outcome)
            })
            .map(|e| e.id.as_str())
            .collect();
        let too_old = |e: &Entry| {
            now.saturating_sub(e.at) > MOST_AGE_SECONDS && !open.contains(e.id.as_str())
        };
        let mut kept: Vec<&Entry> = entries.iter().filter(|e| !too_old(e)).collect();
        while kept.len() > MOST_ENTRIES {
            match kept.iter().position(|e| !open.contains(e.id.as_str())) {
                Some(index) => {
                    kept.remove(index);
                }
                None => break,
            }
        }
        let dropped = entries.len() - kept.len();
        if dropped == 0 {
            return Ok(0);
        }
        let mut text = Vec::new();
        for entry in kept {
            serde_json::to_writer(&mut text, entry).map_err(|_| StoreError::Io)?;
            text.push(b'\n');
        }
        super::fsutil::write_atomic(&self.dir, FILE, &text)?;
        Ok(dropped)
    }

    /// Applies whose end was never recorded: the application stopped in the middle.
    pub fn unfinished(&self) -> Vec<Entry> {
        let (entries, _) = self.entries();
        entries
            .iter()
            .filter(|e| e.phase == Phase::Intent)
            .filter(|e| {
                !entries
                    .iter()
                    .any(|o| o.id == e.id && o.phase == Phase::Outcome)
            })
            .cloned()
            .collect()
    }
}

fn fold(entries: &[Entry]) -> Vec<Change> {
    let mut changes: Vec<Change> = Vec::new();
    for entry in entries {
        match changes.iter_mut().find(|change| change.id == entry.id) {
            Some(change) if entry.phase == Phase::Outcome => {
                change.outcome = entry.outcome;
                change.h1 = entry.h1.clone().or_else(|| change.h1.clone());
                change.record = entry.record.clone().or_else(|| change.record.clone());
            }
            Some(_) => (),
            None => changes.push(Change {
                id: entry.id.clone(),
                at: entry.at,
                provider: entry.provider,
                source: entry.source,
                operation: entry.operation,
                item: entry.item.clone(),
                risk: entry.risk,
                h0: entry.h0.clone(),
                h1: entry.h1.clone(),
                outcome: entry.outcome,
                record: entry.record.clone(),
                undoes: entry.undoes.clone(),
                prior: entry.prior,
            }),
        }
    }
    changes.reverse();
    changes
}
