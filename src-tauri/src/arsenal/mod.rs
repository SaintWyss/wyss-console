//! Arsenal: the capability library. It is offline. It reads the account's skill folders through
//! the configuration engine's reader, copies what the owner imports into its own private database,
//! and starts nothing. Import, presets and selections never change a provider's files or a session.
//! The window reaches it through `commands`.

pub mod analysis;
pub mod commands;
pub mod groups;
pub mod store;
pub mod sync;
pub mod terminal;

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use store::{Error, Store};

/// The Arsenal state the desktop manages. Opening the database may fail; the failure is kept and
/// reported by every command, so the window can say so instead of showing an empty library.
pub struct Arsenal {
    store: Result<Arc<Mutex<Store>>, Error>,
    /// The library's folder, when the library opened. Holds `groups.json` beside the database.
    directory: Option<PathBuf>,
    /// The details of the last scan, by identity. Read by `arsenal_candidate`, replaced by each scan.
    details: Arc<Mutex<BTreeMap<String, commands::CandidateDetail>>>,
    /// Synchronization with the owner's other computers, when the library opened and it is not off.
    sync: Option<sync::Handle>,
}

impl Arsenal {
    pub fn start() -> Self {
        let opened = open();
        let directory = opened.as_ref().ok().map(|(_, directory)| directory.clone());
        let store = opened.map(|(store, _)| Arc::new(Mutex::new(store)));
        if let (Ok(store), Some(directory)) = (&store, &directory) {
            let mut state = groups::load(directory);
            if !state.presets_migrated {
                let migrated = store
                    .lock()
                    .is_ok_and(|mut guard| guard.migrate_presets().is_ok());
                // A failure is not reported here: the migration is tried again at the next start.
                if migrated {
                    state.presets_migrated = true;
                    let _ = groups::save(directory, &state);
                }
            }
        }
        let sync = match (&store, sync::Paths::from_env()) {
            (Ok(store), Some(paths)) => Some(sync::start(store.clone(), paths)),
            _ => None,
        };
        Self {
            store,
            directory,
            details: Arc::new(Mutex::new(BTreeMap::new())),
            sync,
        }
    }

    /// The owner's group corrections, by identity.
    // Called by the group commands.
    #[allow(dead_code)]
    pub(super) fn groups(&self) -> Result<BTreeMap<String, String>, Error> {
        let directory = self.directory.as_ref().ok_or(Error::Storage)?;
        Ok(groups::load(directory).groups)
    }

    /// Puts a capability of the library in a group. Runs under the store's lock, so two changes
    /// cannot overwrite each other's file.
    // Called by the group commands.
    #[allow(dead_code)]
    pub(super) fn set_group(&self, identity: &str, group_id: &str) -> Result<(), Error> {
        if !groups::identity_ok(identity) || !groups::group_ok(group_id) {
            return Err(Error::Invalid);
        }
        let directory = self.directory.clone().ok_or(Error::Storage)?;
        self.with_store(|store| {
            if store.current(identity)?.is_none() {
                return Err(Error::NotFound);
            }
            let mut state = groups::load(&directory);
            if state.groups.len() >= groups::MAX_GROUPS && !state.groups.contains_key(identity) {
                return Err(Error::Invalid);
            }
            state
                .groups
                .insert(identity.to_owned(), group_id.to_owned());
            groups::save(&directory, &state).map_err(|_| Error::Storage)
        })
    }

    /// Takes a capability out of its group. Removing a capability with no group changes nothing.
    // Called by the group commands.
    #[allow(dead_code)]
    pub(super) fn clear_group(&self, identity: &str) -> Result<(), Error> {
        let directory = self.directory.clone().ok_or(Error::Storage)?;
        self.with_store(|_| {
            let mut state = groups::load(&directory);
            if state.groups.remove(identity).is_some() {
                groups::save(&directory, &state).map_err(|_| Error::Storage)?;
            }
            Ok(())
        })
    }

    /// Synchronization's status; `enabled: false` when it does not run here.
    pub(super) fn sync_status(&self) -> sync::Status {
        self.sync
            .as_ref()
            .map(sync::Handle::status)
            .unwrap_or_default()
    }

    /// Asks synchronization to run soon: after a change to the library, or when the owner asks.
    pub(super) fn nudge_sync(&self) {
        if let Some(sync) = &self.sync {
            sync.nudge();
        }
    }

    /// The last scan's details, shared with the work that replaces them.
    fn details_handle(&self) -> Arc<Mutex<BTreeMap<String, commands::CandidateDetail>>> {
        self.details.clone()
    }

    pub(super) fn candidate_detail(&self, id: &str) -> Option<commands::CandidateDetail> {
        self.details.lock().ok()?.get(id).cloned()
    }

    /// The shared store for a Native start, which rebuilds its exclusion off the window's thread.
    pub(crate) fn launch_handle(&self) -> Option<Arc<Mutex<Store>>> {
        self.store.clone().ok()
    }

    /// The shared store, for work that runs off the window's thread and locks it there.
    fn handle(&self) -> Result<Arc<Mutex<Store>>, Error> {
        self.store.clone()
    }

    fn with_store<T>(
        &self,
        action: impl FnOnce(&mut Store) -> Result<T, Error>,
    ) -> Result<T, Error> {
        match &self.store {
            Ok(store) => {
                let mut guard = store.lock().map_err(|_| Error::Storage)?;
                action(&mut guard)
            }
            Err(error) => Err(*error),
        }
    }
}

/// The library for reading only, as the terminal host needs it to plan a delegated agent's skills
/// while the window may be writing. Never creates the file or its schema.
pub fn open_read_only() -> Result<Store, Error> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute())
                .map(|home| home.join(".local/share"))
        })
        .ok_or(Error::Storage)?;
    let path = data
        .join("wyss-console")
        .join("arsenal")
        .join("arsenal.sqlite");
    if !fs::symlink_metadata(&path)
        .map_err(|_| Error::Storage)?
        .is_file()
    {
        return Err(Error::Storage);
    }
    Store::open(Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

/// `wyss-console/arsenal` under `XDG_DATA_HOME` (or `~/.local/share`), in a folder only its owner
/// can open. The database file itself is created 0600. Returns the store and that folder.
fn open() -> Result<(Store, PathBuf), Error> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute())
                .map(|home| home.join(".local/share"))
        })
        .ok_or(Error::Storage)?;
    let directory = data.join("wyss-console").join("arsenal");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)
        .map_err(|_| Error::Storage)?;
    if !fs::symlink_metadata(&directory)
        .map_err(|_| Error::Storage)?
        .is_dir()
    {
        return Err(Error::Storage);
    }
    let path = directory.join("arsenal.sqlite");
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)
        .map_err(|_| Error::Storage)?;
    Ok((Store::open(Connection::open(path)?)?, directory))
}
