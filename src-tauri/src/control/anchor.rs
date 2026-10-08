//! A small durable witness next to the canonical database: which stream this state
//! directory holds and how far that stream has been seen to go.
//!
//! SQLite cannot tell a database that lost itself, its write-ahead log or the tail of that
//! log from one that never had them: each of those opens as a valid, older or empty store.
//! The witness can. It is written after commits and is only ever a lower bound; nothing is
//! derived from it except the refusal to open a store that is not the one it names or that
//! went backwards. It detects loss and damage, not forgery: whoever can rewrite the database
//! as this user can rewrite the witness too.
//!
//! A head counts as witnessed only behind the whole barrier: the new witness written and
//! synced under a staging name, renamed over the old one, and the state directory synced.
//! Syncing the file makes its content durable, not its name: without the last step a power
//! loss can bring the previous witness back, or none.
use super::{paths, store::Store};
use crate::contracts::{
    WireUuid,
    control::{ControlError as E, ControlResult, unique_json},
};
use nix::{
    fcntl::renameat,
    unistd::{UnlinkatFlags, unlinkat},
};
use serde_json::json;
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 256;
const SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

fn incompatible(_: impl std::fmt::Debug) -> E {
    E::StorageIncompatible
}
fn unavailable(_: impl std::fmt::Debug) -> E {
    E::StorageUnavailable
}
fn beside(db: &Path, suffix: &str) -> ControlResult<PathBuf> {
    let name = db.file_name().ok_or(E::UnsafePath)?.to_string_lossy();
    Ok(db.with_file_name(format!("{name}{suffix}")))
}
fn witness_path(db: &Path) -> PathBuf {
    db.with_extension("anchor")
}

/// What the witness recorded before this start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Witnessed {
    stream: String,
    head: u64,
}
fn read(path: &Path) -> ControlResult<Option<Witnessed>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(E::UnsafePath),
        Ok(_) => paths::validate_file(path)?,
    }
    let mut raw = Vec::new();
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| E::UnsafePath)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(unavailable)?;
    // A witness that cannot be read says nothing about what was here: never a fresh start.
    let v = unique_json(&raw).map_err(incompatible)?;
    let o = v.as_object().ok_or(E::StorageIncompatible)?;
    if raw.len() as u64 > MAX_BYTES || o.len() != 3 || v["schema_version"] != 1 {
        return Err(E::StorageIncompatible);
    }
    let stream = WireUuid::try_from(
        v["stream_id"]
            .as_str()
            .ok_or(E::StorageIncompatible)?
            .to_owned(),
    )
    .map_err(incompatible)?;
    let head = v["head"].as_str().ok_or(E::StorageIncompatible)?;
    let head = if head == "0" {
        0
    } else {
        head.parse::<crate::contracts::Sequence>()
            .map_err(incompatible)?
            .get()
    };
    Ok(Some(Witnessed {
        stream: stream.as_str().to_ascii_lowercase(),
        head,
    }))
}

/// Before the database is opened, and possibly created. A database that is missing or
/// empty is created only in a state directory that shows no trace of an earlier one: no
/// witness and no write-ahead log, shared memory or rollback journal left behind.
pub(crate) fn admit(db: &Path) -> ControlResult<Option<Witnessed>> {
    let witnessed = read(&witness_path(db))?;
    let fresh = match fs::symlink_metadata(db) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => return Err(E::UnsafePath),
        Ok(m) => m.len() == 0,
    };
    if fresh {
        if witnessed.is_some() {
            return Err(E::StorageIncompatible);
        }
        for suffix in SIDECARS {
            if fs::symlink_metadata(beside(db, suffix)?).is_ok() {
                return Err(E::StorageIncompatible);
            }
        }
    }
    Ok(witnessed)
}

pub(crate) struct Anchor {
    /// The state directory, held from the moment it was validated. The witness is replaced
    /// through this descriptor and this descriptor is what is synced, so the directory made
    /// durable is the one the replacement happened in, whatever its path names by then.
    dir: File,
    path: PathBuf,
    stream: String,
    head: u64,
}
impl Anchor {
    /// After the store opened and verified itself. The store has to be the stream the
    /// witness names and must not have gone back behind the head the witness saw.
    pub(crate) fn confirm(
        db: &Path,
        witnessed: Option<Witnessed>,
        store: &Store,
    ) -> ControlResult<Self> {
        let stream = store.status()?.stream_id.as_str().to_ascii_lowercase();
        let head = store.head()?;
        if let Some(w) = &witnessed
            && (w.stream != stream || head < w.head)
        {
            return Err(E::StorageIncompatible);
        }
        let path = witness_path(db);
        let dir = paths::private_dir(path.parent().ok_or(E::UnsafePath)?)?;
        let mut anchor = Self {
            dir,
            path,
            stream,
            head,
        };
        if witnessed.is_none_or(|w| w.head != head) {
            anchor.write(head, File::sync_all)?;
        } else {
            // The witness that was read may be a replacement an earlier run was killed
            // before making durable: nothing is served on a name the directory may lose.
            anchor.dir.sync_all().map_err(unavailable)?;
        }
        Ok(anchor)
    }
    /// Records a committed head. Called after the commit, so the witness never runs ahead
    /// of the database and a crash in between only leaves it lower.
    pub(crate) fn advance(&mut self, head: u64) -> ControlResult<()> {
        self.advance_with(head, File::sync_all)
    }
    /// `sync_dir` is the last step of the barrier. It is a parameter because no real
    /// directory fails to sync on demand, and what follows that failure has to be shown.
    pub(super) fn advance_with(
        &mut self,
        head: u64,
        sync_dir: impl FnOnce(&File) -> std::io::Result<()>,
    ) -> ControlResult<()> {
        if head > self.head {
            self.write(head, sync_dir)?;
        }
        Ok(())
    }
    fn write(
        &mut self,
        head: u64,
        sync_dir: impl FnOnce(&File) -> std::io::Result<()>,
    ) -> ControlResult<()> {
        let content =
            json!({"schema_version": 1, "stream_id": self.stream, "head": head.to_string()})
                .to_string();
        let staging = self.path.with_extension("anchor.tmp");
        let staged = staging.file_name().ok_or(E::UnsafePath)?;
        let witness = self.path.file_name().ok_or(E::UnsafePath)?;
        let dir = Some(self.dir.as_raw_fd());
        // An interrupted replacement leaves its staging file behind, and only that is
        // removed: a private file of this user with one name. Anything else found under
        // that name was not left by an authority and stays as it is.
        match fs::symlink_metadata(&staging) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(E::UnsafePath),
            Ok(_) => {
                paths::validate_file(&staging)?;
                unlinkat(dir, staged, UnlinkatFlags::NoRemoveDir).map_err(unavailable)?;
            }
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&staging)
            .map_err(unavailable)?;
        // Durable before it replaces the previous witness: it is whole or it is the old one.
        let replaced = file
            .write_all(content.as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| Ok(renameat(dir, staged, dir, witness)?));
        if replaced.is_err() {
            // What this call created, and what never became the witness.
            let _ = unlinkat(dir, staged, UnlinkatFlags::NoRemoveDir);
            return Err(E::StorageUnavailable);
        }
        // The new name is in the directory, not in the file. Until the directory is synced
        // the replacement is not recorded, and the head is not counted as witnessed.
        sync_dir(&self.dir).map_err(unavailable)?;
        self.head = head;
        Ok(())
    }
}
