//! Arsenal's database: capabilities, their immutable revisions with the content of every file, and
//! the presets and selections built from them. One SQLite file in the private data directory.
//! Each change is one transaction, so a failed import leaves no partial revision and no pointer to
//! content that is not stored. Triggers refuse to change a stored revision, file or blob.

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agentconfig::catalogue::{MAX_FILE_BYTES, MAX_FILES, MAX_TREE_BYTES, name_ok};

const MAX_PATH_BYTES: usize = 256;
const MAX_ITEMS: usize = 128;
const SCHEMA_VERSION: i64 = 2;
/// Most terminal bindings kept; the oldest go first.
const MAX_BINDINGS: i64 = 500;
const MAX_COMPOSITES: usize = 16;
const SCHEMA: &str = "
CREATE TABLE capability (
    id TEXT PRIMARY KEY,
    identity TEXT NOT NULL UNIQUE,
    provider TEXT NOT NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    current_revision INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE revision (
    capability_id TEXT NOT NULL REFERENCES capability(id),
    number INTEGER NOT NULL,
    tree_hash TEXT NOT NULL,
    origin TEXT NOT NULL,
    description TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (capability_id, number)
);
CREATE TABLE blob (
    hash TEXT PRIMARY KEY,
    content BLOB NOT NULL
);
CREATE TABLE revision_file (
    capability_id TEXT NOT NULL,
    number INTEGER NOT NULL,
    relpath TEXT NOT NULL,
    sha256 TEXT NOT NULL REFERENCES blob(hash),
    size INTEGER NOT NULL,
    PRIMARY KEY (capability_id, number, relpath),
    FOREIGN KEY (capability_id, number) REFERENCES revision(capability_id, number)
);
CREATE TABLE preset (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    items TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE selection (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    provider TEXT NOT NULL,
    items TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TRIGGER revision_fixed BEFORE UPDATE ON revision BEGIN
    SELECT RAISE(ABORT, 'arsenal revisions are immutable');
END;
CREATE TRIGGER revision_kept BEFORE DELETE ON revision BEGIN
    SELECT RAISE(ABORT, 'arsenal revisions are immutable');
END;
CREATE TRIGGER revision_file_fixed BEFORE UPDATE ON revision_file BEGIN
    SELECT RAISE(ABORT, 'arsenal revision files are immutable');
END;
CREATE TRIGGER revision_file_kept BEFORE DELETE ON revision_file BEGIN
    SELECT RAISE(ABORT, 'arsenal revision files are immutable');
END;
CREATE TRIGGER blob_fixed BEFORE UPDATE ON blob BEGIN
    SELECT RAISE(ABORT, 'arsenal content is immutable');
END;
CREATE TRIGGER blob_kept BEFORE DELETE ON blob BEGIN
    SELECT RAISE(ABORT, 'arsenal content is immutable');
END;
";
/// Version 2: a selection names the composites it keeps; a terminal remembers the configuration it
/// started with; a project remembers the selection it last used per provider.
const SCHEMA_V2: &str = "
ALTER TABLE selection ADD COLUMN composites TEXT NOT NULL DEFAULT '[]';
CREATE TABLE terminal_binding (
    session_id TEXT PRIMARY KEY,
    selection_id TEXT NOT NULL,
    selection_name TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_version TEXT,
    project TEXT NOT NULL,
    config_hash TEXT NOT NULL,
    state TEXT NOT NULL,
    reason TEXT NOT NULL,
    plan TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE project_selection (
    project TEXT NOT NULL,
    provider TEXT NOT NULL,
    selection_id TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (project, provider)
);
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    NotFound,
    Busy,
    Corrupt,
    Storage,
    Unsupported,
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                Self::Busy
            }
            Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase) => {
                Self::Corrupt
            }
            _ => Self::Storage,
        }
    }
}

/// The SHA-256 of some bytes, in lowercase hex.
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The identity of a skill tree: its files' paths and digests, sorted by path. Two trees with
/// the same value hold the same content, wherever they were read.
pub fn tree_digest(files: &[(String, Vec<u8>)]) -> String {
    let mut entries: Vec<(&str, String)> = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), digest(bytes)))
        .collect();
    entries.sort();
    let mut hasher = Sha256::new();
    for (path, hash) in entries {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

/// The stable identity of a capability: what it is and where it comes from, never its content.
pub fn identity(provider: &str, kind: &str, source: &str, name: &str) -> String {
    digest(format!("{provider}\0{kind}\0{source}\0{name}").as_bytes())
}

/// One capability as synchronization reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncCapability {
    pub id: String,
    pub identity: String,
    pub provider: String,
    pub source: String,
    pub kind: String,
    pub name: String,
    pub current: i64,
    pub revisions: Vec<SyncRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRevision {
    pub number: i64,
    pub tree_hash: String,
    pub origin: String,
    pub description: String,
}

/// A preset or selection id: the lowercase hyphenated UUID form this store writes.
pub fn uuid_ok(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

/// An identity is 64 lowercase hex digits, with `#n` when one scan holds the name twice. For an
/// account-wide capability it is exactly what `identity` derives from what it is; a project's
/// capability mixes in its project, so only the form is checked.
fn identity_ok(new: &NewCapability) -> bool {
    let (hash, suffix) = new
        .identity
        .split_once('#')
        .map_or((new.identity.as_str(), None), |(hash, n)| (hash, Some(n)));
    let form = hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && suffix
            .is_none_or(|n| !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()));
    if !form {
        return false;
    }
    new.source.contains("_project_")
        || new.identity == identity(&new.provider, &new.kind, &new.source, &new.name)
}

/// What an import copies. `files` are the relative paths and bytes of the whole tree.
pub struct NewCapability {
    pub identity: String,
    pub provider: String,
    pub source: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub origin: String,
    pub files: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Created,
    NewRevision,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Imported {
    pub capability: String,
    pub revision: i64,
    pub outcome: Outcome,
}

/// The capability that an identity names, with its current revision and that revision's digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    pub id: String,
    pub revision: i64,
    pub tree_hash: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub id: String,
    pub provider: String,
    pub source: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub origin: String,
    pub revision: i64,
    pub revisions: i64,
    pub tree_hash: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevisionInfo {
    pub number: i64,
    pub tree_hash: String,
    pub origin: String,
    pub created_at: i64,
    pub files: i64,
    pub bytes: i64,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub sha256: String,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detail {
    pub summary: Summary,
    /// The revision whose files are listed: the current one unless another was asked for.
    pub revision: i64,
    pub revisions: Vec<RevisionInfo>,
    pub files: Vec<FileInfo>,
}

/// One capability in a preset or selection. `revision: None` follows the current revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub capability: String,
    pub revision: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub items: Vec<Item>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetInput {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Selection {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub items: Vec<Item>,
    /// Keys of installed composites whose members this selection keeps exposed.
    pub composites: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionInput {
    pub id: Option<String>,
    pub name: String,
    pub provider: String,
    pub items: Vec<Item>,
    #[serde(default)]
    pub composites: Vec<String>,
}

/// What a terminal started with, as Arsenal recorded it right after the start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub session_id: String,
    pub selection_id: String,
    pub selection_name: String,
    pub provider: String,
    pub provider_version: Option<String>,
    pub project: String,
    pub config_hash: String,
    /// `applied` (the host launched it with this exact configuration) or `not_applied`.
    pub state: String,
    pub reason: String,
    /// The plan as computed at the start, in JSON.
    pub plan: String,
    pub created_at: i64,
}

/// The one connection of the Arsenal database. Every write takes the write lock first, so two
/// writers, in this process or another, run one after the other or the second one reports `Busy`.
pub struct Store {
    db: Connection,
}

const SUMMARY: &str = "SELECT c.id, c.provider, c.source, c.kind, c.name, r.description, r.origin, \
     c.current_revision, (SELECT COUNT(*) FROM revision x WHERE x.capability_id = c.id), \
     r.tree_hash, c.updated_at \
     FROM capability c JOIN revision r ON r.capability_id = c.id AND r.number = c.current_revision";

impl Store {
    /// Opens the database, creating the schema when the file is new. A file of another schema
    /// version, or one that fails SQLite's own check, is refused instead of being used.
    pub fn open(db: Connection) -> Result<Self, Error> {
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version {
            0 => {
                let tx = db.unchecked_transaction()?;
                tx.execute_batch(SCHEMA)?;
                tx.execute_batch(SCHEMA_V2)?;
                tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
                tx.commit()?;
            }
            // A version 1 library keeps every row; the new column starts empty for each selection.
            1 => {
                let tx = db.unchecked_transaction()?;
                tx.execute_batch(SCHEMA_V2)?;
                tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
                tx.commit()?;
            }
            SCHEMA_VERSION => {}
            _ => return Err(Error::Unsupported),
        }
        let check: String = db.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
        if check != "ok" {
            return Err(Error::Corrupt);
        }
        Ok(Self { db })
    }

    /// Stores a skill tree as a new capability, a new revision of a known one, or nothing when the
    /// content is the same as the current revision. Nothing is written unless the whole tree is valid.
    pub fn import(&mut self, new: &NewCapability) -> Result<Imported, Error> {
        validate(new)?;
        let tree = tree_digest(&new.files);
        let now = now();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<(String, i64, String, String)> = tx
            .query_row(
                "SELECT c.id, c.current_revision, r.tree_hash, r.description FROM capability c \
                 JOIN revision r ON r.capability_id = c.id AND r.number = c.current_revision \
                 WHERE c.identity = ?1",
                [&new.identity],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let imported = match current {
            // Same files and nothing new to say about them. A blank description that the scan can
            // now read (a Codex agent's TOML before 2026-11-11) is a new revision, since stored
            // revisions never change.
            Some((id, number, tree_hash, description))
                if tree_hash == tree && (!description.is_empty() || new.description.is_empty()) =>
            {
                Imported {
                    capability: id,
                    revision: number,
                    outcome: Outcome::Unchanged,
                }
            }
            Some((id, number, _, _)) => {
                let next = number.checked_add(1).ok_or(Error::Invalid)?;
                insert_revision(&tx, &id, next, &tree, new, now)?;
                tx.execute(
                    "UPDATE capability SET current_revision = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id, next, now],
                )?;
                Imported {
                    capability: id,
                    revision: next,
                    outcome: Outcome::NewRevision,
                }
            }
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO capability (id, identity, provider, source, kind, name, \
                     current_revision, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
                    params![
                        id,
                        new.identity,
                        new.provider,
                        new.source,
                        new.kind,
                        new.name,
                        now
                    ],
                )?;
                insert_revision(&tx, &id, 1, &tree, new, now)?;
                Imported {
                    capability: id,
                    revision: 1,
                    outcome: Outcome::Created,
                }
            }
        };
        tx.commit()?;
        Ok(imported)
    }

    pub fn list(&self) -> Result<Vec<Summary>, Error> {
        let mut statement = self
            .db
            .prepare(&format!("{SUMMARY} ORDER BY c.name, c.provider, c.source"))?;
        let rows = statement.query_map([], summary_row)?;
        let summaries = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(summaries)
    }

    /// One capability, with its revisions and the files of `number` (the current revision by default).
    pub fn detail(&self, id: &str, number: Option<i64>) -> Result<Detail, Error> {
        let summary = self.summary(id)?;
        let revision = number.unwrap_or(summary.revision);
        let files = self.files(id, revision)?;
        let revisions = self.revisions(id, summary.revision)?;
        Ok(Detail {
            summary,
            revision,
            revisions,
            files,
        })
    }

    /// The stored bytes of one file of one revision, checked against the digest they were stored under.
    pub fn file(&self, id: &str, number: i64, path: &str) -> Result<Vec<u8>, Error> {
        let (hash, content): (String, Option<Vec<u8>>) = self
            .db
            .query_row(
                "SELECT f.sha256, b.content FROM revision_file f \
                 LEFT JOIN blob b ON b.hash = f.sha256 \
                 WHERE f.capability_id = ?1 AND f.number = ?2 AND f.relpath = ?3",
                params![id, number, path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let content = content.ok_or(Error::Corrupt)?;
        if digest(&content) != hash {
            return Err(Error::Corrupt);
        }
        Ok(content)
    }

    /// Makes an existing revision the current one. No revision is created and nothing is copied.
    pub fn restore(&mut self, id: &str, number: i64) -> Result<(), Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !revision_exists(&tx, id, number)? {
            return Err(Error::NotFound);
        }
        tx.execute(
            "UPDATE capability SET current_revision = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, number, now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn current(&self, identity: &str) -> Result<Option<Current>, Error> {
        let current = self
            .db
            .query_row(
                "SELECT c.id, c.current_revision, r.tree_hash, r.description FROM capability c \
                 JOIN revision r ON r.capability_id = c.id AND r.number = c.current_revision \
                 WHERE c.identity = ?1",
                [identity],
                |row| {
                    Ok(Current {
                        id: row.get(0)?,
                        revision: row.get(1)?,
                        tree_hash: row.get(2)?,
                        description: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(current)
    }

    pub fn presets(&self) -> Result<Vec<Preset>, Error> {
        let mut statement = self.db.prepare(
            "SELECT id, name, description, items, created_at, updated_at FROM preset \
             ORDER BY name, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, description, items, created_at, updated_at) = row?;
            Ok(Preset {
                id,
                name,
                description,
                items: decode_items(&items)?,
                created_at,
                updated_at,
            })
        })
        .collect()
    }

    /// Creates a preset, or replaces the name, description and items of `input.id`.
    pub fn save_preset(&mut self, input: &PresetInput) -> Result<Preset, Error> {
        check_text(&input.name, 80, true)?;
        check_text(&input.description, 500, false)?;
        check_items(&input.items)?;
        let items = encode_items(&input.items)?;
        let now = now();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_references(&tx, &input.items)?;
        let id = match &input.id {
            Some(id) => {
                let changed = tx.execute(
                    "UPDATE preset SET name = ?2, description = ?3, items = ?4, updated_at = ?5 \
                     WHERE id = ?1",
                    params![id, input.name.trim(), input.description, items, now],
                )?;
                if changed == 0 {
                    return Err(Error::NotFound);
                }
                id.clone()
            }
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO preset (id, name, description, items, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                    params![id, input.name.trim(), input.description, items, now],
                )?;
                id
            }
        };
        tx.commit()?;
        self.presets()?
            .into_iter()
            .find(|preset| preset.id == id)
            .ok_or(Error::Storage)
    }

    pub fn delete_preset(&mut self, id: &str) -> Result<(), Error> {
        let removed = self.db.execute("DELETE FROM preset WHERE id = ?1", [id])?;
        if removed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    pub fn selections(&self) -> Result<Vec<Selection>, Error> {
        let mut statement = self.db.prepare(
            "SELECT id, name, provider, items, created_at, updated_at, composites FROM selection \
             ORDER BY name, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, provider, items, created_at, updated_at, composites) = row?;
            Ok(Selection {
                id,
                name,
                provider,
                items: decode_items(&items)?,
                composites: serde_json::from_str(&composites).map_err(|_| Error::Corrupt)?,
                created_at,
                updated_at,
            })
        })
        .collect()
    }

    pub fn save_selection(&mut self, input: &SelectionInput) -> Result<Selection, Error> {
        check_text(&input.name, 80, true)?;
        if !is_provider(&input.provider) {
            return Err(Error::Invalid);
        }
        check_items(&input.items)?;
        check_composites(&input.composites)?;
        let items = encode_items(&input.items)?;
        let composites = serde_json::to_string(&input.composites).map_err(|_| Error::Invalid)?;
        let now = now();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_references(&tx, &input.items)?;
        let id =
            match &input.id {
                Some(id) => {
                    let changed = tx.execute(
                    "UPDATE selection SET name = ?2, provider = ?3, items = ?4, updated_at = ?5, \
                     composites = ?6 WHERE id = ?1",
                    params![id, input.name.trim(), input.provider, items, now, composites],
                )?;
                    if changed == 0 {
                        return Err(Error::NotFound);
                    }
                    id.clone()
                }
                None => {
                    let id = uuid::Uuid::new_v4().to_string();
                    tx.execute(
                        "INSERT INTO selection (id, name, provider, items, created_at, updated_at, \
                     composites) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
                        params![
                            id,
                            input.name.trim(),
                            input.provider,
                            items,
                            now,
                            composites
                        ],
                    )?;
                    id
                }
            };
        tx.commit()?;
        self.selections()?
            .into_iter()
            .find(|selection| selection.id == id)
            .ok_or(Error::Storage)
    }

    pub fn delete_selection(&mut self, id: &str) -> Result<(), Error> {
        let removed = self
            .db
            .execute("DELETE FROM selection WHERE id = ?1", [id])?;
        if removed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    /// Creates, for each provider a preset holds, a selection named like the preset, unless the
    /// library already has a selection with that provider and name. The id comes from the preset and
    /// the provider, so two computers holding the same preset agree on it. Returns how many were
    /// created. Existing selections and presets are never changed.
    pub fn migrate_presets(&mut self) -> Result<usize, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let created = create_preset_selections(&tx)?;
        tx.commit()?;
        Ok(created)
    }

    /// Every file of one revision, each checked against the digest it was stored under.
    pub fn revision_content(&self, id: &str, number: i64) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let files = self.files(id, number)?;
        let mut content = Vec::with_capacity(files.len());
        let mut total = 0_usize;
        for file in files {
            let bytes = self.file(id, number, &file.path)?;
            total += bytes.len();
            if total > MAX_TREE_BYTES {
                return Err(Error::Corrupt);
            }
            content.push((file.path, bytes));
        }
        Ok(content)
    }

    /// Every capability with its identity and all its revisions, oldest first, for synchronization.
    pub fn sync_capabilities(&self) -> Result<Vec<SyncCapability>, Error> {
        let mut statement = self.db.prepare(
            "SELECT id, identity, provider, source, kind, name, current_revision FROM capability \
             ORDER BY identity",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(SyncCapability {
                id: row.get(0)?,
                identity: row.get(1)?,
                provider: row.get(2)?,
                source: row.get(3)?,
                kind: row.get(4)?,
                name: row.get(5)?,
                current: row.get(6)?,
                revisions: Vec::new(),
            })
        })?;
        let mut capabilities = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut revisions = self.db.prepare(
            "SELECT number, tree_hash, origin, description FROM revision \
             WHERE capability_id = ?1 ORDER BY number",
        )?;
        for capability in &mut capabilities {
            capability.revisions = revisions
                .query_map([&capability.id], |row| {
                    Ok(SyncRevision {
                        number: row.get(0)?,
                        tree_hash: row.get(1)?,
                        origin: row.get(2)?,
                        description: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        Ok(capabilities)
    }

    /// Adds a revision with this content to the capability its identity names, unless the
    /// capability already holds that content, and makes it current when asked. A new identity
    /// creates the capability. Content that is already stored is never stored twice.
    pub fn import_revision(
        &mut self,
        new: &NewCapability,
        current: bool,
    ) -> Result<Imported, Error> {
        validate(new)?;
        if !identity_ok(new) {
            return Err(Error::Invalid);
        }
        let tree = tree_digest(&new.files);
        let now = now();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let known: Option<(String, i64, String, String, String, String)> = tx
            .query_row(
                "SELECT id, current_revision, provider, source, kind, name FROM capability \
                 WHERE identity = ?1",
                [&new.identity],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()?;
        let imported = match known {
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO capability (id, identity, provider, source, kind, name, \
                     current_revision, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
                    params![
                        id,
                        new.identity,
                        new.provider,
                        new.source,
                        new.kind,
                        new.name,
                        now
                    ],
                )?;
                insert_revision(&tx, &id, 1, &tree, new, now)?;
                Imported {
                    capability: id,
                    revision: 1,
                    outcome: Outcome::Created,
                }
            }
            Some((id, number, provider, source, kind, name)) => {
                // One identity is one capability: content claiming another provider is refused.
                if (provider, source, kind, name)
                    != (
                        new.provider.clone(),
                        new.source.clone(),
                        new.kind.clone(),
                        new.name.clone(),
                    )
                {
                    return Err(Error::Invalid);
                }
                let held: Option<i64> = tx
                    .query_row(
                        "SELECT number FROM revision WHERE capability_id = ?1 AND tree_hash = ?2 \
                         ORDER BY number DESC LIMIT 1",
                        params![id, tree],
                        |row| row.get(0),
                    )
                    .optional()?;
                let (target, added) = match held {
                    Some(target) => (target, false),
                    None => {
                        let last: i64 = tx.query_row(
                            "SELECT MAX(number) FROM revision WHERE capability_id = ?1",
                            [&id],
                            |row| row.get(0),
                        )?;
                        let next = last.checked_add(1).ok_or(Error::Invalid)?;
                        insert_revision(&tx, &id, next, &tree, new, now)?;
                        (next, true)
                    }
                };
                let moved = current && target != number;
                if moved {
                    tx.execute(
                        "UPDATE capability SET current_revision = ?2, updated_at = ?3 WHERE id = ?1",
                        params![id, target, now],
                    )?;
                }
                Imported {
                    capability: id,
                    revision: target,
                    outcome: if added || moved {
                        Outcome::NewRevision
                    } else {
                        Outcome::Unchanged
                    },
                }
            }
        };
        tx.commit()?;
        Ok(imported)
    }

    /// The revision of a capability that holds this content, if any.
    pub fn tree_revision(&self, id: &str, tree: &str) -> Result<Option<i64>, Error> {
        let number = self
            .db
            .query_row(
                "SELECT number FROM revision WHERE capability_id = ?1 AND tree_hash = ?2 \
                 ORDER BY number DESC LIMIT 1",
                params![id, tree],
                |row| row.get(0),
            )
            .optional()?;
        Ok(number)
    }

    /// Writes a preset under a known id: created when new, replaced when it exists. Used by
    /// synchronization, which keeps one id for one preset on every computer.
    pub fn put_preset(&mut self, id: &str, input: &PresetInput) -> Result<(), Error> {
        if !uuid_ok(id) {
            return Err(Error::Invalid);
        }
        check_text(&input.name, 80, true)?;
        check_text(&input.description, 500, false)?;
        check_items(&input.items)?;
        let items = encode_items(&input.items)?;
        let now = now();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_references(&tx, &input.items)?;
        tx.execute(
            "INSERT INTO preset (id, name, description, items, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?5) ON CONFLICT(id) DO UPDATE SET name = excluded.name, \
             description = excluded.description, items = excluded.items, \
             updated_at = excluded.updated_at",
            params![id, input.name.trim(), input.description, items, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Writes a selection under a known id, as `put_preset` does.
    pub fn put_selection(&mut self, id: &str, input: &SelectionInput) -> Result<(), Error> {
        if !uuid_ok(id) {
            return Err(Error::Invalid);
        }
        check_text(&input.name, 80, true)?;
        if !is_provider(&input.provider) {
            return Err(Error::Invalid);
        }
        check_items(&input.items)?;
        check_composites(&input.composites)?;
        let items = encode_items(&input.items)?;
        let composites = serde_json::to_string(&input.composites).map_err(|_| Error::Invalid)?;
        let now = now();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_references(&tx, &input.items)?;
        tx.execute(
            "INSERT INTO selection (id, name, provider, items, created_at, updated_at, composites) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6) ON CONFLICT(id) DO UPDATE SET \
             name = excluded.name, provider = excluded.provider, items = excluded.items, \
             updated_at = excluded.updated_at, composites = excluded.composites",
            params![
                id,
                input.name.trim(),
                input.provider,
                items,
                now,
                composites
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// SQLite's full integrity check: `ok` or the first problem.
    #[cfg(test)]
    pub fn integrity(&self) -> Result<String, Error> {
        Ok(self
            .db
            .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?)
    }

    /// Records what a terminal started with. A session is recorded once; the oldest records go
    /// when there are more than `MAX_BINDINGS`. The project remembers the selection for its provider.
    pub fn bind_terminal(&mut self, binding: &Binding) -> Result<(), Error> {
        self.record_terminal(binding, true)
    }

    /// Recover history idempotently without changing the project's current selection.
    pub fn bind_restarted_terminal(&mut self, binding: &Binding) -> Result<(), Error> {
        self.record_terminal(binding, false)
    }

    fn record_terminal(&mut self, binding: &Binding, remember: bool) -> Result<(), Error> {
        let id_ok = binding.session_id.len() == 32
            && binding
                .session_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !id_ok
            || !is_provider(&binding.provider)
            || !matches!(binding.state.as_str(), "applied" | "not_applied")
            || binding.project.len() > 4096
            || binding.plan.len() > 1024 * 1024
            || binding.reason.chars().count() > 500
        {
            return Err(Error::Invalid);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx
            .query_row(
                "SELECT 1 FROM selection WHERE id = ?1",
                [&binding.selection_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(Error::NotFound);
        }
        let conflict = if remember {
            ""
        } else {
            " ON CONFLICT(session_id) DO NOTHING"
        };
        tx.execute(
            &format!(
                "INSERT INTO terminal_binding (session_id, selection_id, selection_name, provider, \
             provider_version, project, config_hash, state, reason, plan, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11){conflict}"
            ),
            params![
                binding.session_id,
                binding.selection_id,
                binding.selection_name,
                binding.provider,
                binding.provider_version,
                binding.project,
                binding.config_hash,
                binding.state,
                binding.reason,
                binding.plan,
                binding.created_at
            ],
        )?;
        if remember {
            tx.execute(
                "INSERT INTO project_selection (project, provider, selection_id, updated_at) \
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT (project, provider) \
             DO UPDATE SET selection_id = excluded.selection_id, updated_at = excluded.updated_at",
                params![
                    binding.project,
                    binding.provider,
                    binding.selection_id,
                    binding.created_at
                ],
            )?;
        }
        tx.execute(
            "DELETE FROM terminal_binding WHERE session_id IN (SELECT session_id FROM \
             terminal_binding ORDER BY created_at DESC, session_id LIMIT -1 OFFSET ?1)",
            [MAX_BINDINGS],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn bindings(&self) -> Result<Vec<Binding>, Error> {
        let mut statement = self.db.prepare(
            "SELECT session_id, selection_id, selection_name, provider, provider_version, \
             project, config_hash, state, reason, plan, created_at FROM terminal_binding \
             ORDER BY created_at DESC, session_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Binding {
                session_id: row.get(0)?,
                selection_id: row.get(1)?,
                selection_name: row.get(2)?,
                provider: row.get(3)?,
                provider_version: row.get(4)?,
                project: row.get(5)?,
                config_hash: row.get(6)?,
                state: row.get(7)?,
                reason: row.get(8)?,
                plan: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?;
        let bindings = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(bindings)
    }

    /// The selection a project last started a terminal of this provider with, if it still exists.
    pub fn project_selection(
        &self,
        project: &str,
        provider: &str,
    ) -> Result<Option<String>, Error> {
        let id = self
            .db
            .query_row(
                "SELECT p.selection_id FROM project_selection p JOIN selection s \
                 ON s.id = p.selection_id WHERE p.project = ?1 AND p.provider = ?2",
                params![project, provider],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }

    fn summary(&self, id: &str) -> Result<Summary, Error> {
        self.db
            .query_row(&format!("{SUMMARY} WHERE c.id = ?1"), [id], summary_row)
            .optional()?
            .ok_or(Error::NotFound)
    }

    fn files(&self, id: &str, number: i64) -> Result<Vec<FileInfo>, Error> {
        if !revision_exists(&self.db, id, number)? {
            return Err(Error::NotFound);
        }
        let mut statement = self.db.prepare(
            "SELECT relpath, sha256, size FROM revision_file \
             WHERE capability_id = ?1 AND number = ?2 ORDER BY relpath",
        )?;
        let rows = statement.query_map(params![id, number], |row| {
            Ok(FileInfo {
                path: row.get(0)?,
                sha256: row.get(1)?,
                size: row.get(2)?,
            })
        })?;
        let files = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(files)
    }

    fn revisions(&self, id: &str, current: i64) -> Result<Vec<RevisionInfo>, Error> {
        let mut statement = self.db.prepare(
            "SELECT r.number, r.tree_hash, r.origin, r.created_at, \
             (SELECT COUNT(*) FROM revision_file f WHERE f.capability_id = r.capability_id AND f.number = r.number), \
             (SELECT COALESCE(SUM(f.size), 0) FROM revision_file f WHERE f.capability_id = r.capability_id AND f.number = r.number) \
             FROM revision r WHERE r.capability_id = ?1 ORDER BY r.number DESC",
        )?;
        let rows = statement.query_map([id], |row| {
            let number: i64 = row.get(0)?;
            Ok(RevisionInfo {
                number,
                tree_hash: row.get(1)?,
                origin: row.get(2)?,
                created_at: row.get(3)?,
                files: row.get(4)?,
                bytes: row.get(5)?,
                current: number == current,
            })
        })?;
        let revisions = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(revisions)
    }
}

fn revision_exists(db: &Connection, id: &str, number: i64) -> Result<bool, Error> {
    Ok(db
        .query_row(
            "SELECT 1 FROM revision WHERE capability_id = ?1 AND number = ?2",
            params![id, number],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Summary> {
    Ok(Summary {
        id: row.get(0)?,
        provider: row.get(1)?,
        source: row.get(2)?,
        kind: row.get(3)?,
        name: row.get(4)?,
        description: row.get(5)?,
        origin: row.get(6)?,
        revision: row.get(7)?,
        revisions: row.get(8)?,
        tree_hash: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn insert_revision(
    tx: &Transaction<'_>,
    id: &str,
    number: i64,
    tree: &str,
    new: &NewCapability,
    now: i64,
) -> Result<(), Error> {
    tx.execute(
        "INSERT INTO revision (capability_id, number, tree_hash, origin, description, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, number, tree, new.origin, new.description, now],
    )?;
    for (path, bytes) in &new.files {
        let hash = digest(bytes);
        tx.execute(
            "INSERT OR IGNORE INTO blob (hash, content) VALUES (?1, ?2)",
            params![hash, bytes.as_slice()],
        )?;
        tx.execute(
            "INSERT INTO revision_file (capability_id, number, relpath, sha256, size) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                number,
                path,
                hash,
                i64::try_from(bytes.len()).map_err(|_| Error::Invalid)?
            ],
        )?;
    }
    Ok(())
}

/// The folders a capability is read from. Each belongs to one provider, and a project's folders
/// are named apart from the account's, so the same name in two projects never shares an identity.
const SOURCES: [&str; 13] = [
    "claude_user_skills",
    "codex_user_skills",
    "codex_home_skills",
    "claude_project_skills",
    "codex_project_skills",
    "claude_user_agents",
    "codex_user_agents",
    "claude_project_agents",
    "codex_project_agents",
    "claude_user_rules",
    "codex_user_rules",
    "claude_project_rules",
    "codex_project_rules",
];

fn validate(new: &NewCapability) -> Result<(), Error> {
    let owner = if new.source.starts_with("claude_") {
        "claude_code"
    } else {
        "codex"
    };
    if !name_ok(&new.name)
        || !is_provider(&new.provider)
        || new.provider != owner
        || !SOURCES.contains(&new.source.as_str())
        || new.description.chars().count() > 500
        || new.origin.len() > 512
    {
        return Err(Error::Invalid);
    }
    // A skill is a folder with a SKILL.md. An agent or a rule is one file of its own.
    let shape_ok = match new.kind.as_str() {
        "skill" => new.files.iter().any(|(path, _)| path == "SKILL.md"),
        "agent" | "rule" => new.files.len() == 1,
        _ => false,
    };
    if !shape_ok || new.files.is_empty() || new.files.len() > MAX_FILES {
        return Err(Error::Invalid);
    }
    let mut seen = HashSet::new();
    let mut total = 0_usize;
    for (path, bytes) in &new.files {
        if !valid_path(path) || !seen.insert(path.as_str()) || bytes.len() > MAX_FILE_BYTES {
            return Err(Error::Invalid);
        }
        total += bytes.len();
    }
    if total > MAX_TREE_BYTES {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// A relative path of plain components: no empty part, no `.` or `..`, no control character.
fn valid_path(path: &str) -> bool {
    path.len() <= MAX_PATH_BYTES
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part.len() <= 128
                && !part.contains('\\')
                && !part.chars().any(char::is_control)
        })
}

pub(super) fn is_provider(provider: &str) -> bool {
    matches!(provider, "claude_code" | "codex")
}

fn check_text(value: &str, max_chars: usize, required: bool) -> Result<(), Error> {
    let trimmed = value.trim();
    if (required && trimmed.is_empty())
        || value.chars().count() > max_chars
        || value.chars().any(char::is_control)
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// Items are unique by capability and bounded. A pinned revision must be a positive number.
pub(super) fn check_items(items: &[Item]) -> Result<(), Error> {
    if items.len() > MAX_ITEMS {
        return Err(Error::Invalid);
    }
    let mut seen = HashSet::new();
    for item in items {
        if !seen.insert(item.capability.as_str()) || item.revision.is_some_and(|number| number < 1)
        {
            return Err(Error::Invalid);
        }
    }
    Ok(())
}

/// Every item names a capability that exists, and a pinned revision that exists for it.
fn check_references(tx: &Transaction<'_>, items: &[Item]) -> Result<(), Error> {
    for item in items {
        let exists: bool = tx
            .query_row(
                "SELECT 1 FROM capability WHERE id = ?1",
                [&item.capability],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(Error::NotFound);
        }
        let pinned_missing = match item.revision {
            Some(number) => !revision_exists(tx, &item.capability, number)?,
            None => false,
        };
        if pinned_missing {
            return Err(Error::NotFound);
        }
    }
    Ok(())
}

/// Composite keys: a few, unique, short and plain.
pub(super) fn check_composites(keys: &[String]) -> Result<(), Error> {
    let mut seen = HashSet::new();
    if keys.len() > MAX_COMPOSITES
        || keys.iter().any(|key| {
            key.is_empty()
                || key.len() > 128
                || !key.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'@')
                })
                || !seen.insert(key.as_str())
        })
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// Creates the selections a preset of this library implies, one per provider it holds. Returns how
/// many were created.
fn create_preset_selections(tx: &Transaction<'_>) -> Result<usize, Error> {
    let presets = {
        let mut statement = tx.prepare("SELECT id, name, items FROM preset ORDER BY id")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut taken = {
        let mut statement = tx.prepare("SELECT provider, name FROM selection")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| row.map(|(provider, name)| (provider, name.trim().to_owned())))
            .collect::<rusqlite::Result<HashSet<_>>>()?
    };
    let mut created = 0;
    for (preset_id, name, items) in presets {
        // A preset whose items cannot be read is skipped.
        let Ok(items) = decode_items(&items) else {
            continue;
        };
        let name = name.trim().to_owned();
        for provider in ["claude_code", "codex"] {
            let mut kept = Vec::new();
            for item in &items {
                let owner: Option<String> = tx
                    .query_row(
                        "SELECT provider FROM capability WHERE id = ?1",
                        [&item.capability],
                        |row| row.get(0),
                    )
                    .optional()?;
                if owner.as_deref() == Some(provider) {
                    kept.push(item.clone());
                }
            }
            if kept.is_empty() || taken.contains(&(provider.to_owned(), name.clone())) {
                continue;
            }
            let hash =
                digest(format!("wyss-arsenal-preset-v1\0{preset_id}\0{provider}").as_bytes());
            let id = format!(
                "{}-{}-{}-{}-{}",
                &hash[0..8],
                &hash[8..12],
                &hash[12..16],
                &hash[16..20],
                &hash[20..32]
            );
            let kept = encode_items(&kept)?;
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO selection (id, name, provider, items, created_at, updated_at, \
                 composites) VALUES (?1, ?2, ?3, ?4, ?5, ?5, '[]')",
                params![id, name, provider, kept, now()],
            )?;
            if inserted > 0 {
                created += 1;
                taken.insert((provider.to_owned(), name.clone()));
            }
        }
    }
    Ok(created)
}

fn encode_items(items: &[Item]) -> Result<String, Error> {
    serde_json::to_string(items).map_err(|_| Error::Invalid)
}

fn decode_items(text: &str) -> Result<Vec<Item>, Error> {
    serde_json::from_str(text).map_err(|_| Error::Corrupt)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, body: &[u8]) -> NewCapability {
        NewCapability {
            identity: identity("claude_code", "skill", "claude_user_skills", name),
            provider: "claude_code".into(),
            source: "claude_user_skills".into(),
            kind: "skill".into(),
            name: name.into(),
            description: "test".into(),
            origin: format!("~/.claude/skills/{name}"),
            files: vec![
                ("SKILL.md".into(), body.to_vec()),
                ("refs/notes.md".into(), b"notes".to_vec()),
            ],
        }
    }

    fn memory() -> Store {
        Store::open(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "arsenal-{tag}-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        ))
    }

    #[test]
    fn a_description_read_after_the_files_were_stored_becomes_a_revision() {
        let mut store = memory();
        let mut blank = skill("agent", b"same");
        blank.description = String::new();
        let first = store.import(&blank).unwrap();
        let read = skill("agent", b"same");
        let again = store.import(&read).unwrap();
        assert_eq!(again.outcome, Outcome::NewRevision);
        assert_eq!(again.revision, 2);
        let detail = store.detail(&first.capability, None).unwrap();
        assert_eq!(detail.summary.description, "test");
        // Once it has one, the same files are unchanged again, and a blank read never erases it.
        assert_eq!(store.import(&read).unwrap().outcome, Outcome::Unchanged);
        assert_eq!(store.import(&blank).unwrap().outcome, Outcome::Unchanged);
    }

    #[test]
    fn duplicates_do_not_create_revisions_and_changes_do() {
        let mut store = memory();
        let first = store.import(&skill("design", b"one")).unwrap();
        assert_eq!(first.outcome, Outcome::Created);
        let again = store.import(&skill("design", b"one")).unwrap();
        assert_eq!(again.outcome, Outcome::Unchanged);
        assert_eq!(again.capability, first.capability);
        assert_eq!(again.revision, 1);
        let changed = store.import(&skill("design", b"two")).unwrap();
        assert_eq!(changed.outcome, Outcome::NewRevision);
        assert_eq!(changed.revision, 2);
        let detail = store.detail(&first.capability, None).unwrap();
        assert_eq!(detail.summary.revision, 2);
        assert_eq!(detail.revisions.len(), 2);
        assert!(detail.revisions[0].current);
    }

    #[test]
    fn restore_moves_the_pointer_and_reads_old_content_unchanged() {
        let mut store = memory();
        let first = store.import(&skill("design", b"one")).unwrap();
        store.import(&skill("design", b"two")).unwrap();
        store.restore(&first.capability, 1).unwrap();
        let detail = store.detail(&first.capability, None).unwrap();
        assert_eq!(detail.summary.revision, 1);
        assert_eq!(detail.summary.revisions, 2);
        assert_eq!(
            store.file(&first.capability, 1, "SKILL.md").unwrap(),
            b"one"
        );
        assert_eq!(store.restore(&first.capability, 9), Err(Error::NotFound));
    }

    #[test]
    fn invalid_trees_are_refused_and_leave_nothing_behind() {
        let mut store = memory();
        let mut escape = skill("design", b"one");
        escape.files.push(("../escape.md".into(), b"x".to_vec()));
        assert_eq!(store.import(&escape), Err(Error::Invalid));
        let mut no_skill = skill("design", b"one");
        no_skill.files.remove(0);
        assert_eq!(store.import(&no_skill), Err(Error::Invalid));
        let bad_name = skill("../design", b"one");
        assert_eq!(store.import(&bad_name), Err(Error::Invalid));
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn a_failure_in_the_middle_of_an_import_rolls_everything_back() {
        let mut store = memory();
        store
            .db
            .execute_batch(
                "CREATE TRIGGER fail_on_notes BEFORE INSERT ON revision_file \
                 WHEN NEW.relpath = 'refs/notes.md' BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            )
            .unwrap();
        assert_eq!(store.import(&skill("design", b"one")), Err(Error::Storage));
        let counts: (i64, i64, i64) = store
            .db
            .query_row(
                "SELECT (SELECT COUNT(*) FROM capability), (SELECT COUNT(*) FROM revision), \
                 (SELECT COUNT(*) FROM blob)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(counts, (0, 0, 0));
    }

    #[test]
    fn stored_revisions_cannot_be_edited_and_tampered_content_is_detected() {
        let mut store = memory();
        let first = store.import(&skill("design", b"one")).unwrap();
        assert!(
            store
                .db
                .execute("UPDATE revision SET origin = 'x'", [])
                .is_err()
        );
        store.db.execute_batch("DROP TRIGGER blob_fixed;").unwrap();
        store
            .db
            .execute("UPDATE blob SET content = ?1", [b"bad".as_slice()])
            .unwrap();
        assert_eq!(
            store.file(&first.capability, 1, "SKILL.md"),
            Err(Error::Corrupt)
        );
    }

    #[test]
    fn data_survives_reopening_and_a_second_writer_is_refused_while_one_holds_the_lock() {
        let path = temp_path("reopen");
        {
            let mut store = Store::open(Connection::open(&path).unwrap()).unwrap();
            store.import(&skill("design", b"persisted")).unwrap();
        }
        let mut reopened = Store::open(Connection::open(&path).unwrap()).unwrap();
        assert_eq!(reopened.list().unwrap().len(), 1);
        let holder = Connection::open(&path).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE;").unwrap();
        reopened.db.busy_timeout(Duration::from_millis(50)).unwrap();
        assert_eq!(reopened.import(&skill("other", b"x")), Err(Error::Busy));
        holder.execute_batch("ROLLBACK;").unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_skill_from_the_codex_home_is_importable_and_a_codex_system_skill_is_not() {
        let mut store = memory();
        let from = |source: &str| NewCapability {
            identity: identity("codex", "skill", source, "tracer"),
            provider: "codex".into(),
            source: source.into(),
            ..skill("tracer", b"---\nname: tracer\n---\n")
        };
        assert_eq!(
            store.import(&from("codex_home_skills")).unwrap().outcome,
            Outcome::Created
        );
        assert_eq!(
            store.import(&from("codex_system_skills")),
            Err(Error::Invalid)
        );
    }

    #[test]
    fn agents_and_rules_are_one_file_from_their_own_folder_of_their_own_provider() {
        let mut store = memory();
        let agent = |source: &str, provider: &str, files: Vec<(String, Vec<u8>)>| NewCapability {
            identity: identity(provider, "agent", source, "reviewer"),
            provider: provider.into(),
            source: source.into(),
            kind: "agent".into(),
            name: "reviewer".into(),
            description: "Reviews".into(),
            origin: "~/.claude/agents/reviewer.md".into(),
            files,
        };
        let one = vec![("reviewer.md".to_owned(), b"body".to_vec())];
        assert_eq!(
            store
                .import(&agent("claude_user_agents", "claude_code", one.clone()))
                .unwrap()
                .outcome,
            Outcome::Created
        );
        let two = vec![one[0].clone(), ("extra.md".to_owned(), b"x".to_vec())];
        assert_eq!(
            store.import(&agent("claude_user_agents", "claude_code", two)),
            Err(Error::Invalid)
        );
        assert_eq!(
            store.import(&agent("claude_user_agents", "codex", one.clone())),
            Err(Error::Invalid)
        );
        assert_eq!(
            store.import(&agent("opencode_user_agents", "codex", one)),
            Err(Error::Invalid)
        );
    }

    #[test]
    fn a_damaged_or_foreign_file_is_refused_at_open() {
        let path = temp_path("damaged");
        std::fs::write(&path, b"this is not a sqlite database at all, just text").unwrap();
        assert!(Store::open(Connection::open(&path).unwrap()).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn presets_and_selections_persist_and_reject_unknown_references() {
        let mut store = memory();
        let design = store.import(&skill("design", b"one")).unwrap();
        let preset = store
            .save_preset(&PresetInput {
                id: None,
                name: " Frontend ".into(),
                description: "React and TypeScript".into(),
                items: vec![Item {
                    capability: design.capability.clone(),
                    revision: Some(1),
                }],
            })
            .unwrap();
        assert_eq!(preset.name, "Frontend");
        let renamed = store
            .save_preset(&PresetInput {
                id: Some(preset.id.clone()),
                name: "Frontend TS".into(),
                description: String::new(),
                items: vec![],
            })
            .unwrap();
        assert_eq!(renamed.name, "Frontend TS");
        assert!(renamed.items.is_empty());
        assert_eq!(
            store.save_preset(&PresetInput {
                id: None,
                name: "Broken".into(),
                description: String::new(),
                items: vec![Item {
                    capability: "missing".into(),
                    revision: None,
                }],
            }),
            Err(Error::NotFound)
        );
        let selection = store
            .save_selection(&SelectionInput {
                composites: Vec::new(),
                id: None,
                name: "Codex frontend".into(),
                provider: "codex".into(),
                items: vec![Item {
                    capability: design.capability,
                    revision: None,
                }],
            })
            .unwrap();
        assert_eq!(store.selections().unwrap(), vec![selection.clone()]);
        assert_eq!(
            store.save_selection(&SelectionInput {
                composites: Vec::new(),
                id: None,
                name: "Other provider".into(),
                provider: "opencode".into(),
                items: vec![],
            }),
            Err(Error::Invalid)
        );
        store.delete_selection(&selection.id).unwrap();
        store.delete_preset(&renamed.id).unwrap();
        assert!(store.presets().unwrap().is_empty());
        assert_eq!(store.delete_preset(&renamed.id), Err(Error::NotFound));
    }

    #[test]
    fn a_version_1_library_is_upgraded_in_place_and_keeps_its_rows() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db.execute_batch("PRAGMA user_version = 1;").unwrap();
        db.execute(
            "INSERT INTO selection (id, name, provider, items, created_at, updated_at) \
             VALUES ('s1', 'Old', 'codex', '[]', 1, 1)",
            [],
        )
        .unwrap();
        let store = Store::open(db).unwrap();
        let version: i64 = store
            .db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let selections = store.selections().unwrap();
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].name, "Old");
        assert!(selections[0].composites.is_empty());
        assert!(store.bindings().unwrap().is_empty());
    }

    fn binding(session: &str, selection: &str, at: i64) -> Binding {
        Binding {
            session_id: session.to_owned(),
            selection_id: selection.to_owned(),
            selection_name: "Frontend".into(),
            provider: "claude_code".into(),
            provider_version: Some("2.1.314 (Claude Code)".into()),
            project: "/home/u/app".into(),
            config_hash: "a".repeat(64),
            state: "not_applied".into(),
            reason: "unqualified".into(),
            plan: "{}".into(),
            created_at: at,
        }
    }

    #[test]
    fn a_terminal_binding_is_recorded_once_remembers_the_project_and_is_bounded() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let selection = store
            .save_selection(&SelectionInput {
                id: None,
                name: "Frontend".into(),
                provider: "claude_code".into(),
                items: Vec::new(),
                composites: vec!["gsd_core@1.15.0".into()],
            })
            .unwrap();
        assert_eq!(selection.composites, ["gsd_core@1.15.0"]);
        let session = "0123456789abcdef0123456789abcdef";
        store
            .bind_terminal(&binding(session, &selection.id, 10))
            .unwrap();
        // The same session is not recorded twice, and an unknown selection is refused.
        assert!(
            store
                .bind_terminal(&binding(session, &selection.id, 11))
                .is_err()
        );
        assert_eq!(
            store.bind_terminal(&binding(&"b".repeat(32), "nope", 12)),
            Err(Error::NotFound)
        );
        for bad in [
            Binding {
                session_id: "../x".into(),
                ..binding(session, &selection.id, 1)
            },
            Binding {
                state: "verified".into(),
                ..binding(&"c".repeat(32), &selection.id, 1)
            },
            Binding {
                provider: "opencode".into(),
                ..binding(&"d".repeat(32), &selection.id, 1)
            },
        ] {
            assert_eq!(store.bind_terminal(&bad), Err(Error::Invalid));
        }
        assert_eq!(
            store
                .project_selection("/home/u/app", "claude_code")
                .unwrap(),
            Some(selection.id.clone())
        );
        assert_eq!(
            store.project_selection("/home/u/app", "codex").unwrap(),
            None
        );
        for index in 0..MAX_BINDINGS + 5 {
            store
                .bind_terminal(&binding(
                    &format!("{index:032x}"),
                    &selection.id,
                    100 + index,
                ))
                .unwrap();
        }
        let kept = store.bindings().unwrap();
        assert_eq!(kept.len() as i64, MAX_BINDINGS);
        assert_eq!(kept[0].session_id, format!("{:032x}", MAX_BINDINGS + 4));
        // Deleting the selection leaves the record and forgets the project's choice.
        store.delete_selection(&selection.id).unwrap();
        assert_eq!(
            store
                .project_selection("/home/u/app", "claude_code")
                .unwrap(),
            None
        );
    }

    #[test]
    fn composite_keys_are_checked() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        for composites in [
            vec!["a b".to_owned()],
            vec!["x".to_owned(), "x".to_owned()],
            (0..17).map(|i| format!("k{i}")).collect(),
        ] {
            let input = SelectionInput {
                id: None,
                name: "S".into(),
                provider: "codex".into(),
                items: Vec::new(),
                composites,
            };
            assert_eq!(store.save_selection(&input), Err(Error::Invalid));
        }
    }

    /// A version 2 library: one Claude Code and one Codex capability, and the preset Kit holding
    /// both plus an item whose capability no longer exists.
    fn version_2_library(db: Connection) -> Connection {
        db.execute_batch(SCHEMA).unwrap();
        db.execute_batch(SCHEMA_V2).unwrap();
        db.execute_batch("PRAGMA user_version = 2;").unwrap();
        for (id, provider, source) in [
            ("c1", "claude_code", "claude_user_skills"),
            ("c2", "codex", "codex_home_skills"),
        ] {
            db.execute(
                "INSERT INTO capability (id, identity, provider, source, kind, name, \
                 current_revision, created_at, updated_at) \
                 VALUES (?1, ?1, ?2, ?3, 'skill', ?1, 1, 1, 1)",
                params![id, provider, source],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO preset (id, name, description, items, created_at, updated_at) \
             VALUES ('p1', 'Kit', '', ?1, 5, 5)",
            [r#"[{"capability":"c1","revision":null},{"capability":"c2","revision":null},{"capability":"gone","revision":null}]"#],
        )
        .unwrap();
        db
    }

    #[test]
    fn a_version_2_library_turns_each_provider_of_a_preset_into_a_selection() {
        let mut store =
            Store::open(version_2_library(Connection::open_in_memory().unwrap())).unwrap();
        assert_eq!(store.migrate_presets(), Ok(2));
        let selections = store.selections().unwrap();
        assert_eq!(selections.len(), 2);
        for selection in &selections {
            assert_eq!(selection.name, "Kit");
            assert!(uuid_ok(&selection.id));
            assert!(selection.composites.is_empty());
            assert_eq!(selection.items.len(), 1);
        }
        let claude = selections
            .iter()
            .find(|selection| selection.provider == "claude_code")
            .unwrap();
        assert_eq!(claude.items[0].capability, "c1");
        let codex = selections
            .iter()
            .find(|selection| selection.provider == "codex")
            .unwrap();
        assert_eq!(codex.items[0].capability, "c2");
        let presets = store.presets().unwrap();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].name, "Kit");
        assert_eq!(presets[0].updated_at, 5);
        assert_eq!(presets[0].items.len(), 3);
        assert_eq!(presets[0].items[2].capability, "gone");
    }

    #[test]
    fn the_preset_migration_never_touches_an_existing_selection() {
        let db = version_2_library(Connection::open_in_memory().unwrap());
        db.execute(
            "INSERT INTO selection (id, name, provider, items, created_at, updated_at) \
             VALUES ('kept', ' Kit ', 'codex', '[{\"capability\":\"other\",\"revision\":3}]', 7, 7)",
            [],
        )
        .unwrap();
        let mut store = Store::open(db).unwrap();
        // The codex provider already has "Kit", so only the claude_code selection is created.
        assert_eq!(store.migrate_presets(), Ok(1));
        let selections = store.selections().unwrap();
        assert_eq!(selections.len(), 2);
        let kept = selections
            .iter()
            .find(|selection| selection.id == "kept")
            .unwrap();
        assert_eq!(kept.name, " Kit ");
        assert_eq!(kept.created_at, 7);
        assert_eq!(kept.updated_at, 7);
        assert_eq!(
            kept.items,
            vec![Item {
                capability: "other".into(),
                revision: Some(3),
            }]
        );
        let created: Vec<_> = selections
            .iter()
            .filter(|selection| selection.provider == "claude_code")
            .collect();
        assert_eq!(created.len(), 1);
        assert_ne!(created[0].id, "kept");
    }

    #[test]
    fn two_libraries_with_the_same_preset_create_the_same_selection_id() {
        let ids = |db: Connection| {
            let mut store = Store::open(version_2_library(db)).unwrap();
            store.migrate_presets().unwrap();
            let mut ids: Vec<String> = store
                .selections()
                .unwrap()
                .into_iter()
                .map(|selection| selection.id)
                .collect();
            ids.sort();
            ids
        };
        let first = ids(Connection::open_in_memory().unwrap());
        assert_eq!(first.len(), 2);
        assert_eq!(first, ids(Connection::open_in_memory().unwrap()));
    }

    #[test]
    fn running_the_preset_migration_twice_creates_nothing_more() {
        let mut store =
            Store::open(version_2_library(Connection::open_in_memory().unwrap())).unwrap();
        assert_eq!(store.migrate_presets(), Ok(2));
        assert_eq!(store.migrate_presets(), Ok(0));
        assert_eq!(store.selections().unwrap().len(), 2);
    }

    #[test]
    fn the_library_has_no_group_table_and_stays_at_version_2() {
        let store = memory();
        let version: i64 = store
            .db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }
}
