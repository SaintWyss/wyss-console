//! The optional local metrics history of docs/contracts/m3-model-usage-history.md.
//!
//! A separate SQLite database in its own private directory, disabled until the owner opts in and
//! until the contract is accepted (`gate::METRICS_POLICY_ACCEPTED`). It never shares a file, a
//! transaction or a migration with the control authority, the configuration journal or the vault,
//! and it cannot fail a Managed turn: `Recorder` turns every storage failure into a *state*.
//!
//! What a record can hold is closed by construction. There is no free-text field: identifiers are
//! bounded, charset-checked and refused (never truncated) when they look like a secret; everything
//! else is an enum or a number. Unknown stays NULL, never zero. Prompts, output, reasoning, paths,
//! commands, diffs, account names, raw provider payloads and cost estimates have no column.
use super::{
    gate,
    usage::{Completeness, LimitObservation, Provider, TurnUsage},
};
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub const SCHEMA_VERSION: u8 = 1;
pub const MAX_RETENTION_SECS: i64 = 30 * 24 * 3600;
pub const MAX_RECORDS: i64 = 10_000;
pub const MAX_RECORD_BYTES: usize = 1024;
pub const DB_LIMIT_BYTES: u64 = 12 * 1024 * 1024;
pub const TOTAL_LIMIT_BYTES: u64 = 24 * 1024 * 1024;
pub const EXPORT_LIMIT_BYTES: u64 = 12 * 1024 * 1024;
const PAGE_SIZE: u64 = 4096;
const DIR: &str = "m3-history";
const DB: &str = "history.sqlite3";
const MARKER: &str = "ENABLED";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryError {
    /// The privacy contract is not accepted: nothing opens, nothing is written.
    NotAccepted,
    /// The owner did not opt in.
    NotEnabled,
    UnsafeDirectory,
    InvalidRecord,
    RecordTooLarge,
    SecretLooking,
    Quota,
    Corrupt,
    Storage,
    ExportLimit,
}
impl HistoryError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotAccepted => "not_accepted",
            Self::NotEnabled => "not_enabled",
            Self::UnsafeDirectory => "unsafe_directory",
            Self::InvalidRecord => "invalid_record",
            Self::RecordTooLarge => "record_too_large",
            Self::SecretLooking => "secret_looking",
            Self::Quota => "quota",
            Self::Corrupt => "corrupt",
            Self::Storage => "storage",
            Self::ExportLimit => "export_limit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    TurnUsage,
    Limit,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    ProviderUsageEvent,
    ProviderLimitEvent,
    ProviderStatus,
    ProviderCatalogue,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Fresh,
    Late,
    Stale,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Complete {
    Complete,
    Partial,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitWindowName {
    FiveHour,
    SevenDay,
    SevenDayOpus,
    SevenDaySonnet,
    Overage,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitStatusName {
    Allowed,
    AllowedWarning,
    Rejected,
}

macro_rules! text_of {
    ($t:ty { $($v:ident => $s:literal),+ $(,)? }) => {
        impl $t {
            pub const fn text(self) -> &'static str { match self { $(Self::$v => $s),+ } }
            pub fn parse(s: &str) -> Option<Self> { match s { $($s => Some(Self::$v),)+ _ => None } }
        }
    };
}
text_of!(Kind { TurnUsage => "turn_usage", Limit => "limit" });
text_of!(Provenance { ProviderUsageEvent => "provider_usage_event", ProviderLimitEvent => "provider_limit_event", ProviderStatus => "provider_status", ProviderCatalogue => "provider_catalogue" });
text_of!(Freshness { Fresh => "fresh", Late => "late", Stale => "stale" });
text_of!(Complete { Complete => "complete", Partial => "partial", Unknown => "unknown" });
text_of!(LimitWindowName { FiveHour => "five_hour", SevenDay => "seven_day", SevenDayOpus => "seven_day_opus", SevenDaySonnet => "seven_day_sonnet", Overage => "overage" });
text_of!(LimitStatusName { Allowed => "allowed", AllowedWarning => "allowed_warning", Rejected => "rejected" });

/// Identifier charset of the contract: `[A-Za-z0-9._:-]+`.
fn ident_ok(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// Refuses an identifier that could be carrying a credential. The charset allows it, so the
/// content check is the second wall: key prefixes, tokens, JWTs, key blocks and long opaque runs.
pub fn looks_secret(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    const PREFIXES: [&str; 14] = [
        "sk-",
        "sk_",
        "pk-",
        "pk_",
        "rk_",
        "ghp_",
        "gho_",
        "ghs_",
        "github_pat_",
        "xox",
        "akia",
        "asia",
        "eyj",
        "bearer",
    ];
    if PREFIXES.iter().any(|p| {
        lower.starts_with(p) || lower.contains(&format!(":{p}")) || lower.contains(&format!("-{p}"))
    }) {
        return true;
    }
    if lower.contains("begin") && lower.contains("key") {
        return true;
    }
    // A long run with no separator that mixes cases and digits is opaque secret material, not an
    // identifier a person or a provider names things with (UUIDs have hyphens; message ids are
    // short and prefixed).
    s.split(['.', '_', ':', '-']).any(|run| {
        run.len() >= 32
            && run.bytes().any(|b| b.is_ascii_digit())
            && run.bytes().any(|b| b.is_ascii_uppercase())
            && run.bytes().any(|b| b.is_ascii_lowercase())
    })
}

/// One observation. Constructed only through `Record::turn_usage` / `Record::limit`, which
/// validate every field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Record {
    pub schema_version: u8,
    pub provider: &'static str,
    pub provider_build: String,
    pub session_id: String,
    pub runtime_id: String,
    pub generation: i64,
    pub turn_id: String,
    pub origin_event_id: String,
    pub kind: Kind,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub limit_window: Option<LimitWindowName>,
    pub limit_status: Option<LimitStatusName>,
    /// Parts per thousand, as the provider's fraction reported it.
    pub limit_utilization_permille: Option<i64>,
    pub limit_resets_at: Option<i64>,
    pub provenance: Provenance,
    pub completeness: Complete,
    pub freshness: Freshness,
    pub event_at: i64,
    pub received_at: i64,
}

/// Where an observation came from, as the authority knows it.
pub struct Origin<'a> {
    pub provider: Provider,
    pub provider_build: &'a str,
    pub session_id: &'a str,
    pub runtime_id: &'a str,
    pub generation: u64,
}

fn nn(v: Option<u64>) -> Result<Option<i64>, HistoryError> {
    v.map(|x| i64::try_from(x).map_err(|_| HistoryError::InvalidRecord))
        .transpose()
}
const fn provider_name(p: Provider) -> &'static str {
    match p {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
    }
}

impl Record {
    fn base(
        o: &Origin,
        turn_id: &str,
        origin_event_id: &str,
        kind: Kind,
        provenance: Provenance,
        now: i64,
    ) -> Result<Self, HistoryError> {
        let build_ok = !o.provider_build.is_empty()
            && o.provider_build.len() <= 64
            && o.provider_build
                .split('.')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if !build_ok
            || !ident_ok(o.session_id, 64)
            || !ident_ok(o.runtime_id, 64)
            || !ident_ok(turn_id, 128)
            || !ident_ok(origin_event_id, 128)
        {
            return Err(HistoryError::InvalidRecord);
        }
        if [o.session_id, o.runtime_id, turn_id, origin_event_id]
            .iter()
            .any(|s| looks_secret(s))
        {
            return Err(HistoryError::SecretLooking);
        }
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            provider: provider_name(o.provider),
            provider_build: o.provider_build.to_owned(),
            session_id: o.session_id.to_owned(),
            runtime_id: o.runtime_id.to_owned(),
            generation: i64::try_from(o.generation).map_err(|_| HistoryError::InvalidRecord)?,
            turn_id: turn_id.to_owned(),
            origin_event_id: origin_event_id.to_owned(),
            kind,
            input_tokens: None,
            output_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            reasoning_tokens: None,
            limit_window: None,
            limit_status: None,
            limit_utilization_permille: None,
            limit_resets_at: None,
            provenance,
            completeness: Complete::Unknown,
            freshness: Freshness::Fresh,
            event_at: now,
            received_at: now,
        })
    }

    /// A finished turn's usage as the ledger holds it. The SDK's cost estimate and per-model
    /// bookkeeping are deliberately not carried over.
    pub fn turn_usage(o: &Origin, t: &TurnUsage, now: i64) -> Result<Self, HistoryError> {
        let mut r = Self::base(
            o,
            &t.turn_id,
            &t.turn_id,
            Kind::TurnUsage,
            Provenance::ProviderUsageEvent,
            now,
        )?;
        r.input_tokens = nn(t.total.input)?;
        r.output_tokens = nn(t.total.output)?;
        r.cache_creation_tokens = nn(t.total.cache_creation)?;
        r.cache_read_tokens = nn(t.total.cache_read)?;
        r.reasoning_tokens = nn(t.total.reasoning)?;
        r.completeness = match t.completeness {
            Completeness::Complete => Complete::Complete,
            Completeness::ProviderError | Completeness::Interrupted => Complete::Partial,
            Completeness::Unreported | Completeness::Running => Complete::Unknown,
        };
        r.validate()?;
        Ok(r)
    }

    pub fn limit(
        o: &Origin,
        turn_id: &str,
        l: &LimitObservation,
        now: i64,
    ) -> Result<Self, HistoryError> {
        let window = format!("{:?}", l.window);
        let mut r = Self::base(
            o,
            turn_id,
            &format!("limit:{}:{}", window.to_ascii_lowercase(), l.observed_at),
            Kind::Limit,
            Provenance::ProviderLimitEvent,
            now,
        )?;
        r.limit_window = Some(match l.window {
            super::usage::LimitWindow::FiveHour => LimitWindowName::FiveHour,
            super::usage::LimitWindow::SevenDay => LimitWindowName::SevenDay,
            super::usage::LimitWindow::SevenDayOpus => LimitWindowName::SevenDayOpus,
            super::usage::LimitWindow::SevenDaySonnet => LimitWindowName::SevenDaySonnet,
            super::usage::LimitWindow::Overage => LimitWindowName::Overage,
        });
        r.limit_status = Some(match l.status {
            super::usage::LimitStatus::Allowed => LimitStatusName::Allowed,
            super::usage::LimitStatus::AllowedWarning => LimitStatusName::AllowedWarning,
            super::usage::LimitStatus::Rejected => LimitStatusName::Rejected,
        });
        r.limit_utilization_permille = l
            .utilization
            .filter(|u| u.is_finite() && (0.0..=1.0).contains(u))
            .map(|u| (u * 1000.0).round() as i64);
        r.limit_resets_at = nn(l.resets_at)?;
        r.completeness = Complete::Partial;
        r.validate()?;
        Ok(r)
    }

    /// The canonical encoding the size bound is measured on.
    pub fn canonical(&self) -> Result<Vec<u8>, HistoryError> {
        serde_json::to_vec(self).map_err(|_| HistoryError::InvalidRecord)
    }

    pub fn validate(&self) -> Result<(), HistoryError> {
        let nonneg = |v: Option<i64>| v.is_none_or(|x| x >= 0);
        if self.schema_version != SCHEMA_VERSION
            || self.generation < 0
            || self.event_at < 0
            || self.received_at < 0
            || ![
                self.input_tokens,
                self.output_tokens,
                self.cache_creation_tokens,
                self.cache_read_tokens,
                self.reasoning_tokens,
                self.limit_resets_at,
            ]
            .into_iter()
            .all(nonneg)
            || self
                .limit_utilization_permille
                .is_some_and(|u| !(0..=1000).contains(&u))
            || (self.kind == Kind::Limit) != self.limit_window.is_some()
            || (self.kind == Kind::Limit) != self.limit_status.is_some()
            || (self.kind == Kind::TurnUsage
                && (self.limit_resets_at.is_some() || self.limit_utilization_permille.is_some()))
        {
            return Err(HistoryError::InvalidRecord);
        }
        // Every field is judged here, whoever built the record and whatever was changed in it since:
        // the store trusts nothing a caller could have set.
        let build_ok = !self.provider_build.is_empty()
            && self.provider_build.len() <= 64
            && self
                .provider_build
                .split('.')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if !matches!(self.provider, "claude" | "codex")
            || !build_ok
            || !ident_ok(&self.session_id, 64)
            || !ident_ok(&self.runtime_id, 64)
            || !ident_ok(&self.turn_id, 128)
            || !ident_ok(&self.origin_event_id, 128)
        {
            return Err(HistoryError::InvalidRecord);
        }
        // Provenance is a function of the kind, and so is the shape of the origin event.
        let consistent = match self.kind {
            Kind::TurnUsage => {
                self.provenance == Provenance::ProviderUsageEvent
                    && self.origin_event_id == self.turn_id
            }
            Kind::Limit => {
                self.provenance == Provenance::ProviderLimitEvent
                    && self.origin_event_id.starts_with("limit:")
            }
        };
        if !consistent {
            return Err(HistoryError::InvalidRecord);
        }
        if [
            &self.session_id,
            &self.runtime_id,
            &self.turn_id,
            &self.origin_event_id,
        ]
        .iter()
        .any(|s| looks_secret(s))
        {
            return Err(HistoryError::SecretLooking);
        }
        if self.canonical()?.len() > MAX_RECORD_BYTES {
            return Err(HistoryError::RecordTooLarge);
        }
        Ok(())
    }

    /// Derived, never supplied: provider, generation, origin event and kind.
    pub fn dedupe_key(&self) -> String {
        let mut h = Sha256::new();
        for part in [
            self.provider.as_bytes(),
            &self.generation.to_be_bytes(),
            self.origin_event_id.as_bytes(),
            self.kind.text().as_bytes(),
        ] {
            h.update((part.len() as u64).to_be_bytes());
            h.update(part);
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ---- the store -------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Inserted {
    New,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub records: i64,
    pub oldest_event_at: Option<i64>,
    pub newest_event_at: Option<i64>,
    pub db_bytes: u64,
    pub total_bytes: u64,
    /// Rows deleted by retention since the store was created: pruning is never silent.
    pub pruned_total: i64,
    pub retention_days: i64,
    pub max_records: i64,
}

/// The bounds the store enforces. The contract's numbers are the default; tests use smaller ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub retention_secs: i64,
    pub max_records: i64,
    pub db_bytes: u64,
    pub total_bytes: u64,
    pub export_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            retention_secs: MAX_RETENTION_SECS,
            max_records: MAX_RECORDS,
            db_bytes: DB_LIMIT_BYTES,
            total_bytes: TOTAL_LIMIT_BYTES,
            export_bytes: EXPORT_LIMIT_BYTES,
        }
    }
}

pub struct History {
    dir: PathBuf,
    conn: Connection,
    limits: Limits,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS records (
  id INTEGER PRIMARY KEY,
  dedupe TEXT NOT NULL UNIQUE CHECK(length(dedupe)=64),
  schema_version INTEGER NOT NULL CHECK(schema_version=1),
  provider TEXT NOT NULL CHECK(provider IN ('claude','codex')),
  provider_build TEXT NOT NULL CHECK(length(provider_build) BETWEEN 1 AND 64),
  session_id TEXT NOT NULL CHECK(length(session_id) BETWEEN 1 AND 64),
  runtime_id TEXT NOT NULL CHECK(length(runtime_id) BETWEEN 1 AND 64),
  generation INTEGER NOT NULL CHECK(generation>=0),
  turn_id TEXT NOT NULL CHECK(length(turn_id) BETWEEN 1 AND 128),
  origin_event_id TEXT NOT NULL CHECK(length(origin_event_id) BETWEEN 1 AND 128),
  kind TEXT NOT NULL CHECK(kind IN ('turn_usage','limit')),
  input_tokens INTEGER CHECK(input_tokens IS NULL OR input_tokens>=0),
  output_tokens INTEGER CHECK(output_tokens IS NULL OR output_tokens>=0),
  cache_creation_tokens INTEGER CHECK(cache_creation_tokens IS NULL OR cache_creation_tokens>=0),
  cache_read_tokens INTEGER CHECK(cache_read_tokens IS NULL OR cache_read_tokens>=0),
  reasoning_tokens INTEGER CHECK(reasoning_tokens IS NULL OR reasoning_tokens>=0),
  limit_window TEXT CHECK(limit_window IS NULL OR limit_window IN ('five_hour','seven_day','seven_day_opus','seven_day_sonnet','overage')),
  limit_status TEXT CHECK(limit_status IS NULL OR limit_status IN ('allowed','allowed_warning','rejected')),
  limit_utilization_permille INTEGER CHECK(limit_utilization_permille IS NULL OR limit_utilization_permille BETWEEN 0 AND 1000),
  limit_resets_at INTEGER CHECK(limit_resets_at IS NULL OR limit_resets_at>=0),
  provenance TEXT NOT NULL CHECK(provenance IN ('provider_usage_event','provider_limit_event','provider_status','provider_catalogue')),
  completeness TEXT NOT NULL CHECK(completeness IN ('complete','partial','unknown')),
  freshness TEXT NOT NULL CHECK(freshness IN ('fresh','late','stale')),
  event_at INTEGER NOT NULL CHECK(event_at>=0),
  received_at INTEGER NOT NULL CHECK(received_at>=0)
) STRICT;
CREATE INDEX IF NOT EXISTS records_event_at ON records(event_at);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL) STRICT;
INSERT OR IGNORE INTO meta VALUES('schema_version',1),('pruned_total',0);
";

fn me() -> u32 {
    nix::unistd::geteuid().as_raw()
}

/// The private directory of the store: ours, mode 0700, not a link, and no other entry kinds.
fn private_dir(parent: &Path, create: bool) -> Result<PathBuf, HistoryError> {
    let pm = fs::symlink_metadata(parent).map_err(|_| HistoryError::UnsafeDirectory)?;
    if !pm.is_dir() || pm.file_type().is_symlink() || pm.uid() != me() || pm.mode() & 0o022 != 0 {
        return Err(HistoryError::UnsafeDirectory);
    }
    let dir = parent.join(DIR);
    if create {
        match fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(HistoryError::UnsafeDirectory),
        }
    }
    let m = fs::symlink_metadata(&dir).map_err(|_| HistoryError::UnsafeDirectory)?;
    if !m.is_dir() || m.file_type().is_symlink() || m.uid() != me() || m.mode() & 0o077 != 0 {
        return Err(HistoryError::UnsafeDirectory);
    }
    Ok(dir)
}

fn sizes(dir: &Path) -> (u64, u64) {
    let len = |n: &str| {
        fs::symlink_metadata(dir.join(n))
            .map(|m| m.len())
            .unwrap_or(0)
    };
    let db = len(DB);
    (
        db,
        db + len("history.sqlite3-journal")
            + len("history.sqlite3-wal")
            + len("history.sqlite3-shm"),
    )
}

impl History {
    /// Opt in: creates the private directory and the marker. Refused while the contract is not
    /// accepted.
    pub fn enable(parent: &Path) -> Result<(), HistoryError> {
        if !gate::metrics_history_blockers().is_empty() {
            return Err(HistoryError::NotAccepted);
        }
        Self::enable_unchecked(parent)
    }
    /// Without the policy gate: tests only.
    #[cfg(test)]
    pub(crate) fn enable_with_gate(parent: &Path) -> Result<(), HistoryError> {
        Self::enable_unchecked(parent)
    }
    fn enable_unchecked(parent: &Path) -> Result<(), HistoryError> {
        let dir = private_dir(parent, true)?;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(dir.join(MARKER))
            .map(|_| ())
            .map_err(|_| HistoryError::Storage)
    }
    pub fn is_enabled(parent: &Path) -> bool {
        private_dir(parent, false)
            .is_ok_and(|d| fs::symlink_metadata(d.join(MARKER)).is_ok_and(|m| m.is_file()))
    }

    pub fn open(parent: &Path) -> Result<Self, HistoryError> {
        if !gate::metrics_history_blockers().is_empty() {
            return Err(HistoryError::NotAccepted);
        }
        Self::open_unchecked(parent, Limits::default())
    }

    /// Without the policy gate: tests only.
    #[cfg(test)]
    pub(crate) fn open_with_gate(parent: &Path) -> Result<Self, HistoryError> {
        Self::open_unchecked(parent, Limits::default())
    }

    /// Without the policy gate and with chosen bounds: tests only.
    #[cfg(test)]
    pub(crate) fn open_with(parent: &Path, limits: Limits) -> Result<Self, HistoryError> {
        Self::open_unchecked(parent, limits)
    }

    fn open_unchecked(parent: &Path, limits: Limits) -> Result<Self, HistoryError> {
        // A directory that exists but is not safe is an attack to report; no directory is "off".
        let dir = match private_dir(parent, false) {
            Ok(d) => d,
            Err(_) if fs::symlink_metadata(parent.join(DIR)).is_err() => {
                return Err(HistoryError::NotEnabled);
            }
            Err(e) => return Err(e),
        };
        if !fs::symlink_metadata(dir.join(MARKER)).is_ok_and(|m| m.is_file()) {
            return Err(HistoryError::NotEnabled);
        }
        let path = dir.join(DB);
        if let Ok(m) = fs::symlink_metadata(&path)
            && (!m.is_file() || m.file_type().is_symlink() || m.uid() != me() || m.nlink() != 1)
        {
            return Err(HistoryError::UnsafeDirectory);
        }
        match Self::open_db(&dir, &path, limits) {
            Ok(h) => Ok(h),
            Err(HistoryError::Corrupt) => {
                // Only this optional store is quarantined; nothing else is touched.
                let aside = dir.join(format!("{DB}.corrupt"));
                let _ = fs::remove_file(&aside);
                let _ = fs::rename(&path, &aside);
                for side in ["-journal", "-wal", "-shm"] {
                    let _ = fs::remove_file(dir.join(format!("{DB}{side}")));
                }
                Err(HistoryError::Corrupt)
            }
            Err(e) => Err(e),
        }
    }

    fn open_db(dir: &Path, path: &Path, limits: Limits) -> Result<Self, HistoryError> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| HistoryError::Corrupt)?;
        let max_pages = limits.db_bytes / PAGE_SIZE;
        conn.execute_batch(&format!(
            "PRAGMA page_size=4096; PRAGMA journal_mode=TRUNCATE; PRAGMA synchronous=FULL; PRAGMA max_page_count={max_pages}; PRAGMA trusted_schema=OFF;"
        ))
        .map_err(|_| HistoryError::Corrupt)?;
        let ok: String = conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|_| HistoryError::Corrupt)?;
        if ok != "ok" {
            return Err(HistoryError::Corrupt);
        }
        conn.execute_batch(SCHEMA)
            .map_err(|_| HistoryError::Corrupt)?;
        // The schema must be exactly ours: a foreign table is a different file.
        let tables: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT IN ('records','meta')", [], |r| r.get(0))
            .map_err(|_| HistoryError::Corrupt)?;
        if tables != 0 {
            return Err(HistoryError::Corrupt);
        }
        Ok(Self {
            dir: dir.to_owned(),
            conn,
            limits,
        })
    }

    /// Inserts one record atomically with its dedupe marker, after retention. A duplicate is
    /// ignored, not counted again; a write that cannot fit within the quota is rolled back.
    pub fn insert(&mut self, rec: &Record, now: i64) -> Result<Inserted, HistoryError> {
        rec.validate()?;
        if sizes(&self.dir).1 > self.limits.total_bytes {
            return Err(HistoryError::Quota);
        }
        let (dir, limits) = (self.dir.clone(), self.limits);
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| HistoryError::Storage)?;
        let key = rec.dedupe_key();
        let dup: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM records WHERE dedupe=?1)",
                [&key],
                |r| r.get(0),
            )
            .map_err(|_| HistoryError::Storage)?;
        if dup {
            return Ok(Inserted::Duplicate);
        }
        // Retention first: age, then count. Every deletion is counted.
        let aged = tx
            .execute(
                "DELETE FROM records WHERE event_at < ?1",
                [now - limits.retention_secs],
            )
            .map_err(|_| HistoryError::Storage)? as i64;
        let count: i64 = tx
            .query_row("SELECT count(*) FROM records", [], |r| r.get(0))
            .map_err(|_| HistoryError::Storage)?;
        let excess = (count + 1 - limits.max_records).max(0);
        let trimmed = if excess > 0 {
            tx.execute(
                "DELETE FROM records WHERE id IN (SELECT id FROM records ORDER BY id ASC LIMIT ?1)",
                [excess],
            )
            .map_err(|_| HistoryError::Storage)? as i64
        } else {
            0
        };
        if aged + trimmed > 0 {
            tx.execute(
                "UPDATE meta SET value=value+?1 WHERE key='pruned_total'",
                [aged + trimmed],
            )
            .map_err(|_| HistoryError::Storage)?;
        }
        tx.execute(
            "INSERT INTO records (dedupe,schema_version,provider,provider_build,session_id,runtime_id,generation,turn_id,origin_event_id,kind,
              input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,reasoning_tokens,limit_window,limit_status,
              limit_utilization_permille,limit_resets_at,provenance,completeness,freshness,event_at,received_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)",
            params![
                key, rec.schema_version, rec.provider, rec.provider_build, rec.session_id, rec.runtime_id, rec.generation, rec.turn_id,
                rec.origin_event_id, rec.kind.text(), rec.input_tokens, rec.output_tokens, rec.cache_creation_tokens, rec.cache_read_tokens,
                rec.reasoning_tokens, rec.limit_window.map(LimitWindowName::text), rec.limit_status.map(LimitStatusName::text),
                rec.limit_utilization_permille, rec.limit_resets_at, rec.provenance.text(), rec.completeness.text(), rec.freshness.text(),
                rec.event_at, rec.received_at
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::DiskFull => HistoryError::Quota,
            _ => HistoryError::Storage,
        })?;
        // The combined on-disk bound is checked inside the transaction, where the journal exists.
        if sizes(&dir).1 > limits.total_bytes {
            return Err(HistoryError::Quota);
        }
        tx.commit().map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::DiskFull => {
                HistoryError::Quota
            }
            _ => HistoryError::Storage,
        })?;
        Ok(Inserted::New)
    }

    pub fn status(&self) -> Result<Status, HistoryError> {
        let (db_bytes, total_bytes) = sizes(&self.dir);
        let (records, oldest, newest): (i64, Option<i64>, Option<i64>) = self
            .conn
            .query_row(
                "SELECT count(*),min(event_at),max(event_at) FROM records",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| HistoryError::Storage)?;
        let pruned_total: i64 = self
            .conn
            .query_row("SELECT value FROM meta WHERE key='pruned_total'", [], |r| {
                r.get(0)
            })
            .map_err(|_| HistoryError::Storage)?;
        Ok(Status {
            records,
            oldest_event_at: oldest,
            newest_event_at: newest,
            db_bytes,
            total_bytes,
            pruned_total,
            retention_days: self.limits.retention_secs / 86_400,
            max_records: self.limits.max_records,
        })
    }

    /// Records in insertion order after `after_id` (0 for the first page), at most `limit`.
    pub fn page(&self, after_id: i64, limit: u32) -> Result<Vec<(i64, Record)>, HistoryError> {
        let limit = i64::from(limit.clamp(1, 500));
        let mut q = self
            .conn
            .prepare(
                "SELECT id,provider,provider_build,session_id,runtime_id,generation,turn_id,origin_event_id,kind,input_tokens,output_tokens,
                 cache_creation_tokens,cache_read_tokens,reasoning_tokens,limit_window,limit_status,limit_utilization_permille,limit_resets_at,
                 provenance,completeness,freshness,event_at,received_at FROM records WHERE id>?1 ORDER BY id ASC LIMIT ?2",
            )
            .map_err(|_| HistoryError::Storage)?;
        let rows = q
            .query_map(params![after_id, limit], |r| {
                let text = |i: usize| -> String { r.get(i).unwrap_or_default() };
                let opt = |i: usize| -> Option<String> { r.get(i).ok().flatten() };
                let provider = match text(1).as_str() {
                    "claude" => "claude",
                    _ => "codex",
                };
                Ok((
                    r.get::<_, i64>(0)?,
                    Record {
                        schema_version: SCHEMA_VERSION,
                        provider,
                        provider_build: text(2),
                        session_id: text(3),
                        runtime_id: text(4),
                        generation: r.get(5)?,
                        turn_id: text(6),
                        origin_event_id: text(7),
                        kind: Kind::parse(&text(8)).unwrap_or(Kind::TurnUsage),
                        input_tokens: r.get(9)?,
                        output_tokens: r.get(10)?,
                        cache_creation_tokens: r.get(11)?,
                        cache_read_tokens: r.get(12)?,
                        reasoning_tokens: r.get(13)?,
                        limit_window: opt(14).and_then(|s| LimitWindowName::parse(&s)),
                        limit_status: opt(15).and_then(|s| LimitStatusName::parse(&s)),
                        limit_utilization_permille: r.get(16)?,
                        limit_resets_at: r.get(17)?,
                        provenance: Provenance::parse(&text(18))
                            .unwrap_or(Provenance::ProviderStatus),
                        completeness: Complete::parse(&text(19)).unwrap_or(Complete::Unknown),
                        freshness: Freshness::parse(&text(20)).unwrap_or(Freshness::Stale),
                        event_at: r.get(21)?,
                        received_at: r.get(22)?,
                    },
                ))
            })
            .map_err(|_| HistoryError::Storage)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| HistoryError::Storage)
    }

    /// Streams the allowlisted schema as JSON lines: a header that says what the export holds and
    /// what retention removed, then one row per record. It aborts cleanly at the size limit.
    pub fn export(&self, out: &mut dyn Write) -> Result<u64, HistoryError> {
        let status = self.status()?;
        let limit = self.limits.export_bytes;
        let mut written = 0u64;
        let mut put = |out: &mut dyn Write, line: &[u8]| -> Result<(), HistoryError> {
            if written + line.len() as u64 + 1 + 64 > limit {
                let _ = out.write_all(b"{\"export_aborted\":\"size_limit\"}\n");
                return Err(HistoryError::ExportLimit);
            }
            out.write_all(line)
                .and_then(|()| out.write_all(b"\n"))
                .map_err(|_| HistoryError::Storage)?;
            written += line.len() as u64 + 1;
            Ok(())
        };
        let header = serde_json::json!({"export":"wyss_m3_history","schema_version":SCHEMA_VERSION,"records":status.records,
            "retention_days":status.retention_days,"max_records":status.max_records,"pruned_total":status.pruned_total,
            "contains":"provider-reported usage and limit metadata only"});
        put(
            out,
            &serde_json::to_vec(&header).map_err(|_| HistoryError::Storage)?,
        )?;
        let mut after = 0;
        loop {
            let page = self.page(after, 200)?;
            let Some((last, _)) = page.last() else { break };
            after = *last;
            for (_, rec) in &page {
                put(out, &rec.canonical()?)?;
            }
        }
        Ok(written)
    }

    /// Deletes this store only: the database, its journal sidecars and the marker.
    pub fn reset(parent: &Path) -> Result<u32, HistoryError> {
        let dir = private_dir(parent, false)?;
        let mut removed = 0;
        for name in [
            DB,
            "history.sqlite3-journal",
            "history.sqlite3-wal",
            "history.sqlite3-shm",
            "history.sqlite3.corrupt",
            MARKER,
        ] {
            match fs::symlink_metadata(dir.join(name)) {
                Ok(m) if m.is_file() => {
                    fs::remove_file(dir.join(name)).map_err(|_| HistoryError::Storage)?;
                    removed += 1;
                }
                Ok(_) => return Err(HistoryError::UnsafeDirectory),
                Err(_) => (),
            }
        }
        // The directory is ours alone; an entry nobody here created is left where it is.
        let _ = fs::remove_dir(&dir);
        Ok(removed)
    }
}

// ---- failure isolation -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
pub enum SinkState {
    Off,
    Recording,
    /// A storage failure disabled writes. Managed work is unaffected; the history is partial.
    Partial(&'static str),
}

/// What the host records into. It never returns an error to its caller: a failure turns writes
/// off and is reported as `Partial`, so a full disk or a locked database cannot fail or delay a
/// turn, an approval, a cancellation or a recovery.
pub struct Recorder {
    store: Option<History>,
    state: SinkState,
    accepted: u64,
    dropped: u64,
}
impl Recorder {
    pub fn off() -> Self {
        Self {
            store: None,
            state: SinkState::Off,
            accepted: 0,
            dropped: 0,
        }
    }
    pub fn with(store: History) -> Self {
        Self {
            store: Some(store),
            state: SinkState::Recording,
            accepted: 0,
            dropped: 0,
        }
    }
    pub fn state(&self) -> SinkState {
        self.state
    }
    pub fn counts(&self) -> (u64, u64) {
        (self.accepted, self.dropped)
    }
    pub fn record(&mut self, rec: &Record, now: i64) {
        let Some(store) = self
            .store
            .as_mut()
            .filter(|_| self.state == SinkState::Recording)
        else {
            self.dropped += 1;
            return;
        };
        match store.insert(rec, now) {
            Ok(_) => self.accepted += 1,
            Err(
                e @ (HistoryError::InvalidRecord
                | HistoryError::RecordTooLarge
                | HistoryError::SecretLooking),
            ) => {
                // This one observation is refused; the store is fine.
                self.dropped += 1;
                let _ = e;
            }
            Err(e) => {
                self.dropped += 1;
                self.state = SinkState::Partial(e.code());
            }
        }
    }
    pub fn store(&self) -> Option<&History> {
        self.store.as_ref()
    }
}

/// Records handed from the event path to a writer thread through a bounded queue. The event path
/// never takes a lock the writer holds, never touches SQLite and never waits: `offer` either puts
/// the record in the queue or counts it as dropped and says so (`Partial("queue_full")`). A slow
/// or stuck disk therefore costs history, never a turn, a cancellation or a reap.
pub struct AsyncRecorder {
    tx: std::sync::mpsc::SyncSender<Job>,
    shared: std::sync::Arc<std::sync::Mutex<Shared>>,
    overflowed: std::sync::atomic::AtomicU64,
    writer: std::sync::Mutex<Option<std::thread::JoinHandle<Recorder>>>,
}
struct Shared {
    state: SinkState,
    accepted: u64,
    dropped: u64,
}
enum Job {
    Record(Box<Record>, i64),
    /// Answers once every earlier record has been written (tests and shutdown, never the event path).
    Flush(std::sync::mpsc::SyncSender<()>),
    Stop,
}
/// Records the queue holds before it starts dropping.
pub const QUEUE_CAPACITY: usize = 256;

impl AsyncRecorder {
    pub fn spawn(recorder: Recorder) -> Self {
        Self::spawn_with(recorder, QUEUE_CAPACITY, None)
    }
    /// `hold`: a receiver the writer waits on before each record (a test makes the disk "slow").
    pub(crate) fn spawn_with(
        recorder: Recorder,
        capacity: usize,
        hold: Option<std::sync::mpsc::Receiver<()>>,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Job>(capacity);
        let shared = std::sync::Arc::new(std::sync::Mutex::new(Shared {
            state: recorder.state(),
            accepted: 0,
            dropped: 0,
        }));
        let seen = shared.clone();
        let writer = std::thread::Builder::new()
            .name("m3-history-writer".into())
            .spawn(move || {
                let mut recorder = recorder;
                while let Ok(job) = rx.recv() {
                    match job {
                        Job::Record(rec, now) => {
                            if let Some(hold) = &hold {
                                let _ = hold.recv();
                            }
                            recorder.record(&rec, now);
                            if let Ok(mut s) = seen.lock() {
                                let (accepted, dropped) = recorder.counts();
                                s.state = recorder.state();
                                s.accepted = accepted;
                                s.dropped = dropped;
                            }
                        }
                        Job::Flush(ack) => {
                            let _ = ack.send(());
                        }
                        Job::Stop => break,
                    }
                }
                recorder
            })
            .expect("a thread can be started");
        Self {
            tx,
            shared,
            overflowed: 0.into(),
            writer: std::sync::Mutex::new(Some(writer)),
        }
    }

    /// Never blocks. A full queue (or a writer that is gone) drops the record and is counted.
    pub fn offer(&self, rec: Record, now: i64) {
        use std::sync::mpsc::TrySendError;
        match self.tx.try_send(Job::Record(Box::new(rec), now)) {
            Ok(()) => (),
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.overflowed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    /// What the sink is doing, truthfully: a storage failure or an overflowing queue is `Partial`.
    pub fn state(&self) -> SinkState {
        let s = self
            .shared
            .lock()
            .map_or(SinkState::Partial("poisoned"), |s| s.state);
        match (
            s,
            self.overflowed.load(std::sync::atomic::Ordering::Relaxed),
        ) {
            (SinkState::Recording, n) if n > 0 => SinkState::Partial("queue_full"),
            (state, _) => state,
        }
    }
    /// (records the store accepted, records dropped for any reason, including a full queue).
    pub fn counts(&self) -> (u64, u64) {
        let (accepted, dropped) = self
            .shared
            .lock()
            .map_or((0, 0), |s| (s.accepted, s.dropped));
        (
            accepted,
            dropped + self.overflowed.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Waits until everything offered before this call has been written. Not for the event path.
    pub fn flush(&self) {
        let (ack, done) = std::sync::mpsc::sync_channel(1);
        if self.tx.send(Job::Flush(ack)).is_ok() {
            let _ = done.recv();
        }
    }

    /// Ends the writer and hands back the recorder (and its store). Waits for the queue to drain.
    pub fn finish(&self) -> Option<Recorder> {
        let _ = self.tx.send(Job::Stop);
        let handle = self.writer.lock().ok()?.take()?;
        handle.join().ok()
    }
}

#[cfg(test)]
mod tests;
