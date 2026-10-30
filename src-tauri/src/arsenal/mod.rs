//! Arsenal: the capability library. It is offline. It reads the account's skill folders through
//! the configuration engine's reader, copies what the owner imports into its own private database,
//! and starts nothing. Import, presets and selections never change a provider's files or a session.
//! The window reaches it through `commands`.

pub mod commands;
pub mod store;

use std::fs::{self, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use store::{Error, Store};

/// The Arsenal state the desktop manages. Opening the database may fail; the failure is kept and
/// reported by every command, so the window can say so instead of showing an empty library.
pub struct Arsenal {
    store: Result<Mutex<Store>, Error>,
}

impl Arsenal {
    pub fn start() -> Self {
        Self {
            store: open().map(Mutex::new),
        }
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

/// `wyss-console/arsenal` under `XDG_DATA_HOME` (or `~/.local/share`), in a folder only its owner
/// can open. The database file itself is created 0600.
fn open() -> Result<Store, Error> {
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
    Store::open(Connection::open(path)?)
}
