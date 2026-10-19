//! The restoration store (14 §3, ADR 0034): what the Console keeps so that an entry it took
//! out of a provider's file can be put back exactly.
//!
//! A record is one file in a directory only this user can enter. Its body, the removed entry
//! and where it was, is sealed by the vault. The clear part holds only what the page may
//! already see: an identifier, a redacted label, the time, a state, whether the entry may
//! hold a secret, and how it is protected.
//!
//! There is no way in this module to write a secret-bearing entry unsealed: the plain form
//! exists for an entry classified free of secrets when no key store can be had, and the
//! plan says so before the user confirms. Anything else fails closed.
use super::fault::{Faults, Injected, Step};
use super::fsutil::{StoreError, private_dir, remove_interrupted, write_atomic};
use super::inventory::SourceKey;
use super::journal::{new_id, now};
use super::model::ProviderId;
use super::vault::{
    RecordKind, Sealed, Vault, VaultError, open_with_passphrase, seal_with_passphrase,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const VERSION: u32 = 1;
/// 14 §3: five hundred records or a hundred and eighty days.
pub const MOST_RECORDS: usize = 500;
pub const MOST_AGE_SECONDS: u64 = 180 * 24 * 3600;
const MOST_FILE_BYTES: u64 = 512 * 1024;
const MOST_TEXT: usize = 256 * 1024;
const EXPORT_KIND: &str = "wyss-restoration-export";

/// How far up the removal went: the handler alone, or its group or event if it was the
/// last in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Handler,
    Group,
    Event,
}

/// Where the removed text sat, so that putting it back reproduces the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// It was followed by a sibling: the text ends where that sibling now begins.
    BeforeNext,
    /// It was the last: the text begins where the previous sibling now ends.
    AfterPrevious,
    /// Its container held nothing else (only for a container that stays).
    Alone,
}

/// What is sealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordBody {
    pub provider: ProviderId,
    pub source: SourceKey,
    pub event: String,
    pub matcher: Option<String>,
    pub group: usize,
    pub index: usize,
    pub level: Level,
    pub anchor: Anchor,
    /// The bytes that left the file, with the separator that went with them.
    pub removed_text: String,
    /// Where in the file, after the removal, they go back for an exact restoration.
    pub removed_at: usize,
    /// The removed child on its own, without the separator: a handler, a group, or an
    /// event's member, as `level` says.
    pub unit_text: String,
    /// The handler on its own, to put it in a group that has changed.
    pub handler_text: String,
    /// The file before the removal, and after it.
    pub h0: String,
    pub h1: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protection {
    Sealed,
    /// An entry classified free of secrets, kept as a private file because no key store
    /// could be had. Never a secret-bearing entry.
    PlainNoSecret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Parked,
    /// Put back, or restored elsewhere; kept for the undo of that, then pruned.
    Used,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    v: u32,
    id: String,
    created: u64,
    state: State,
    /// Redacted before it was written: the same text the page shows for the entry.
    label: String,
    secret_bearing: bool,
    protection: Protection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sealed: Option<Sealed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plain: Option<RecordBody>,
}

/// What the page may see of a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub id: String,
    pub created: u64,
    pub state: State,
    pub label: String,
    pub secret_bearing: bool,
    pub protection: Protection,
    /// Parked for longer than the retention: kept until the user decides (14 §3).
    pub past_retention: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreError {
    Vault(VaultError),
    /// A secret-bearing entry was asked to be kept without a key store.
    NoKeyStoreForSecret,
    Missing,
    Malformed,
    Io,
    Unsafe,
    Crashed,
    ImportInvalid,
}

impl From<VaultError> for RestoreError {
    fn from(error: VaultError) -> Self {
        Self::Vault(error)
    }
}
impl From<StoreError> for RestoreError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unsafe => Self::Unsafe,
            StoreError::Io => Self::Io,
        }
    }
}

pub struct RestorationStore {
    dir: PathBuf,
    vault: Vault,
    faults: Faults,
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl RestorationStore {
    pub fn open(state: &Path, vault: Vault, faults: Faults) -> Result<Self, RestoreError> {
        let dir = private_dir(&state.join("restoration"))?;
        remove_interrupted(&dir);
        Ok(Self { dir, vault, faults })
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.rec"))
    }

    /// How an entry would be kept, decided before the user confirms and held to afterwards.
    pub fn protection_for(&self, secret_bearing: bool) -> Result<Protection, RestoreError> {
        if self.vault.available() {
            Ok(Protection::Sealed)
        } else if secret_bearing {
            Err(RestoreError::NoKeyStoreForSecret)
        } else {
            Ok(Protection::PlainNoSecret)
        }
    }

    /// Keeps `body` as a new record. `protection` is what the plan promised: if it cannot be
    /// kept that way it is not kept, and a sealed promise is never kept in the open.
    pub fn park(
        &self,
        label: &str,
        secret_bearing: bool,
        protection: Protection,
        body: &RecordBody,
    ) -> Result<String, RestoreError> {
        if body.removed_text.len() > MOST_TEXT || body.handler_text.len() > MOST_TEXT {
            return Err(RestoreError::Malformed);
        }
        if protection == Protection::PlainNoSecret && secret_bearing {
            return Err(RestoreError::NoKeyStoreForSecret);
        }
        self.park_as(&new_id(), label, secret_bearing, protection, body)
    }

    /// As `park`, under an identifier that was decided before (the journal's intent names
    /// the record it is about to make, so an interrupted apply can find and remove it).
    pub fn park_as(
        &self,
        id: &str,
        label: &str,
        secret_bearing: bool,
        protection: Protection,
        body: &RecordBody,
    ) -> Result<String, RestoreError> {
        if !valid_id(id) {
            return Err(RestoreError::Malformed);
        }
        if protection == Protection::PlainNoSecret && secret_bearing {
            return Err(RestoreError::NoKeyStoreForSecret);
        }
        let id = id.to_owned();
        let plaintext = serde_json::to_vec(body).map_err(|_| RestoreError::Malformed)?;
        let mut file = File {
            v: VERSION,
            id: id.clone(),
            created: now(),
            state: State::Parked,
            label: label.to_owned(),
            secret_bearing,
            protection,
            sealed: None,
            plain: None,
        };
        match protection {
            Protection::Sealed => {
                self.faults
                    .check(Step::RecordSealed)
                    .map_err(|e| self.injected(e))?;
                file.sealed = Some(self.vault.seal(RecordKind::Restoration, &id, &plaintext)?);
            }
            Protection::PlainNoSecret => file.plain = Some(body.clone()),
        }
        self.faults
            .check(Step::RecordWritten)
            .map_err(|e| self.injected(e))?;
        self.write(&file)?;
        Ok(id)
    }

    fn injected(&self, injected: Injected) -> RestoreError {
        match injected {
            Injected::Failed => RestoreError::Io,
            Injected::Crashed => RestoreError::Crashed,
        }
    }

    fn write(&self, file: &File) -> Result<(), RestoreError> {
        let bytes = serde_json::to_vec(file).map_err(|_| RestoreError::Malformed)?;
        write_atomic(&self.dir, &format!("{}.rec", file.id), &bytes)?;
        Ok(())
    }

    fn load(&self, id: &str) -> Result<File, RestoreError> {
        if !valid_id(id) {
            return Err(RestoreError::Missing);
        }
        let path = self.path(id);
        let meta = fs::symlink_metadata(&path).map_err(|_| RestoreError::Missing)?;
        if !meta.is_file() || meta.len() > MOST_FILE_BYTES {
            return Err(RestoreError::Malformed);
        }
        let bytes = fs::read(&path).map_err(|_| RestoreError::Io)?;
        let file: File = serde_json::from_slice(&bytes).map_err(|_| RestoreError::Malformed)?;
        if file.v != VERSION || file.id != id {
            return Err(RestoreError::Malformed);
        }
        Ok(file)
    }

    /// The record's body. A sealed one that does not authenticate is never parsed.
    pub fn read(&self, id: &str) -> Result<RecordBody, RestoreError> {
        let file = self.load(id)?;
        match (file.protection, file.sealed, file.plain) {
            (Protection::Sealed, Some(sealed), None) => {
                let plaintext = self.vault.open(RecordKind::Restoration, id, &sealed)?;
                serde_json::from_slice(&plaintext).map_err(|_| RestoreError::Malformed)
            }
            // A plain record that says it holds a secret is not one this build wrote.
            (Protection::PlainNoSecret, None, Some(body)) if !file.secret_bearing => Ok(body),
            _ => Err(RestoreError::Malformed),
        }
    }

    pub fn mark(&self, id: &str, state: State) -> Result<(), RestoreError> {
        self.faults
            .check(Step::RecordMarked)
            .map_err(|e| self.injected(e))?;
        let mut file = self.load(id)?;
        file.state = state;
        self.write(&file)
    }

    /// Removes the record. A plain one is overwritten first; a sealed one is ciphertext.
    pub fn delete(&self, id: &str) -> Result<(), RestoreError> {
        let file = self.load(id)?;
        let path = self.path(id);
        if file.protection == Protection::PlainNoSecret {
            let length = fs::metadata(&path).map_or(0, |m| m.len());
            let _ = fs::write(&path, vec![0_u8; usize::try_from(length).unwrap_or(0)]);
        }
        fs::remove_file(path).map_err(|_| RestoreError::Io)
    }

    pub fn list(&self) -> Vec<Summary> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let now = now();
        let mut summaries: Vec<Summary> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let id = name.strip_suffix(".rec")?;
                let file = self.load(id).ok()?;
                Some(Summary {
                    past_retention: file.state == State::Parked
                        && now.saturating_sub(file.created) > MOST_AGE_SECONDS,
                    id: file.id,
                    created: file.created,
                    state: file.state,
                    label: file.label,
                    secret_bearing: file.secret_bearing,
                    protection: file.protection,
                })
            })
            .collect();
        summaries.sort_by(|a, b| b.created.cmp(&a.created).then(a.id.cmp(&b.id)));
        summaries
    }

    /// Removes used records past the bounds, oldest first. A parked record is the only copy
    /// of an entry the user took out of his file: it is never removed by age.
    pub fn prune(&self) -> usize {
        let now = now();
        let mut used: Vec<Summary> = self
            .list()
            .into_iter()
            .filter(|s| s.state == State::Used)
            .collect();
        used.sort_by_key(|s| s.created);
        let total = self.list().len();
        let mut removed = 0;
        for (position, record) in used.iter().enumerate() {
            let over_count = total - removed > MOST_RECORDS && position < used.len();
            let over_age = now.saturating_sub(record.created) > MOST_AGE_SECONDS;
            if (over_count || over_age) && self.delete(&record.id).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// The record sealed again under a passphrase, as a file to hand to the user (13).
    pub fn export(&self, id: &str, passphrase: &str, rounds: u32) -> Result<Vec<u8>, RestoreError> {
        let file = self.load(id)?;
        let body = self.read(id)?;
        let plaintext = serde_json::to_vec(&body).map_err(|_| RestoreError::Malformed)?;
        let sealed = seal_with_passphrase(RecordKind::Export, id, &plaintext, passphrase, rounds)?;
        let export = Export {
            kind: EXPORT_KIND.to_owned(),
            v: VERSION,
            id: id.to_owned(),
            label: file.label,
            secret_bearing: file.secret_bearing,
            sealed,
        };
        serde_json::to_vec(&export).map_err(|_| RestoreError::Malformed)
    }

    /// Reads an exported file and keeps it here as a new record, sealed for this computer.
    /// An entry that may hold a secret needs the key store, as anywhere else.
    pub fn import(&self, bytes: &[u8], passphrase: &str) -> Result<String, RestoreError> {
        if bytes.len() as u64 > MOST_FILE_BYTES {
            return Err(RestoreError::ImportInvalid);
        }
        let export: Export =
            serde_json::from_slice(bytes).map_err(|_| RestoreError::ImportInvalid)?;
        if export.kind != EXPORT_KIND || export.v != VERSION || !valid_id(&export.id) {
            return Err(RestoreError::ImportInvalid);
        }
        let plaintext =
            open_with_passphrase(RecordKind::Export, &export.id, &export.sealed, passphrase)?;
        let body: RecordBody =
            serde_json::from_slice(&plaintext).map_err(|_| RestoreError::ImportInvalid)?;
        let protection = self.protection_for(export.secret_bearing)?;
        self.park(&export.label, export.secret_bearing, protection, &body)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    kind: String,
    v: u32,
    id: String,
    label: String,
    secret_bearing: bool,
    sealed: Sealed,
}
