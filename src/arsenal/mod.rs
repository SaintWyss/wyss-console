//! Arsenal: the capability library. It is offline. It reads the account's skill folders through
//! the configuration engine's reader, copies what the owner imports into its own private database,
//! and starts nothing. Import, presets and selections never change a provider's files or a session.

pub mod analysis;
pub mod environment;
pub mod library;
pub mod role_instructions;
pub mod roles;
pub mod store;
pub mod sync;
pub mod terminal;

use std::fs::{self, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;

use rusqlite::Connection;

use store::{Error, Store};

/// The library for reading only, as the terminal host needs it to plan a delegated agent's skills
/// while the window may be writing. Never creates the file or its schema.
pub fn open_read_only() -> Result<Store, Error> {
    let path = environment::directory().ok_or(Error::Storage)?.join("arsenal.sqlite");
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
pub fn open() -> Result<(Store, PathBuf), Error> {
    let directory = environment::directory().ok_or(Error::Storage)?;
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
