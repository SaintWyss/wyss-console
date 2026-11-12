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
    pub(super) fn groups(&self) -> Result<BTreeMap<String, String>, Error> {
        let directory = self.directory.as_ref().ok_or(Error::Storage)?;
        Ok(groups::load(directory).groups)
    }

    /// Puts a capability of the library in a group. Runs under the store's lock, so two changes
    /// cannot overwrite each other's file.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arsenal-groups-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        dir
    }

    /// An Arsenal whose library is `store`, with its group corrections kept in `directory`.
    fn open_with(store: Store, directory: PathBuf) -> Arsenal {
        Arsenal {
            store: Ok(Arc::new(Mutex::new(store))),
            directory: Some(directory),
            details: Arc::new(Mutex::new(BTreeMap::new())),
            sync: None,
        }
    }

    /// Imports one skill named `name` under `identity` into `store`.
    fn skill(store: &mut Store, identity: &str, name: &str) {
        store
            .import(&store::NewCapability {
                identity: identity.to_owned(),
                provider: "claude_code".to_owned(),
                source: "claude_user_skills".to_owned(),
                kind: "skill".to_owned(),
                name: name.to_owned(),
                description: String::new(),
                origin: format!("~/.claude/skills/{name}"),
                files: vec![("SKILL.md".to_owned(), b"# skill".to_vec())],
            })
            .unwrap();
    }

    fn id(n: u32) -> String {
        format!("{n:064x}")
    }

    fn memory() -> Store {
        Store::open(Connection::open_in_memory().unwrap()).unwrap()
    }

    #[test]
    fn a_group_is_set_changed_and_cleared_in_the_file_beside_the_library() {
        let directory = scratch("set");
        let mut store = memory();
        skill(&mut store, &id(1), "frontend");
        let arsenal = open_with(store, directory.clone());
        assert!(arsenal.groups().unwrap().is_empty());

        arsenal.set_group(&id(1), "design").unwrap();
        assert_eq!(
            arsenal.groups().unwrap(),
            BTreeMap::from([(id(1), "design".to_owned())])
        );
        // The correction lives in groups.json beside the library, not in its database.
        assert_eq!(
            groups::load(&directory).groups,
            BTreeMap::from([(id(1), "design".to_owned())])
        );

        arsenal.set_group(&id(1), "review").unwrap();
        assert_eq!(
            arsenal.groups().unwrap(),
            BTreeMap::from([(id(1), "review".to_owned())])
        );

        arsenal.clear_group(&id(1)).unwrap();
        assert!(arsenal.groups().unwrap().is_empty());
        // Clearing a capability that has no correction changes nothing and is not an error.
        arsenal.clear_group(&id(1)).unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_group_names_a_capability_the_library_holds_and_a_valid_group() {
        let directory = scratch("refuse");
        let mut store = memory();
        skill(&mut store, &id(1), "frontend");
        let arsenal = open_with(store, directory.clone());
        assert_eq!(
            arsenal.set_group(&id(2), "design"),
            Err(Error::NotFound),
            "a capability the library does not hold"
        );
        assert_eq!(
            arsenal.set_group("not-an-identity", "design"),
            Err(Error::Invalid)
        );
        assert_eq!(
            arsenal.set_group(&id(1), "Design Group"),
            Err(Error::Invalid)
        );
        assert_eq!(arsenal.set_group(&id(1), ""), Err(Error::Invalid));
        assert!(arsenal.groups().unwrap().is_empty());
        assert!(!directory.join("groups.json").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_new_group_is_refused_once_the_file_holds_the_most_corrections() {
        let directory = scratch("cap");
        let full = groups::State {
            schema_version: 1,
            presets_migrated: true,
            groups: (0..u32::try_from(groups::MAX_GROUPS).unwrap())
                .map(|n| (id(n), "design".to_owned()))
                .collect(),
        };
        groups::save(&directory, &full).unwrap();
        let mut store = memory();
        skill(&mut store, &id(0), "already-corrected");
        skill(&mut store, &id(u32::MAX), "newcomer");
        let arsenal = open_with(store, directory.clone());
        assert_eq!(
            arsenal.set_group(&id(u32::MAX), "design"),
            Err(Error::Invalid)
        );
        // A capability that already has a correction can still change it.
        arsenal.set_group(&id(0), "review").unwrap();
        assert_eq!(
            groups::load(&directory)
                .groups
                .get(&id(0))
                .map(String::as_str),
            Some("review")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_library_that_did_not_open_keeps_no_group_corrections() {
        let arsenal = Arsenal {
            store: Err(Error::Storage),
            directory: None,
            details: Arc::new(Mutex::new(BTreeMap::new())),
            sync: None,
        };
        assert_eq!(arsenal.set_group(&id(1), "design"), Err(Error::Storage));
        assert_eq!(arsenal.clear_group(&id(1)), Err(Error::Storage));
        assert_eq!(arsenal.groups(), Err(Error::Storage));
    }
}
