//! Where preferences live and how they are written.
//!
//! One small file, `$XDG_CONFIG_HOME/wyss-console/preferences.json`, in a directory only
//! this user can enter. It is read through a descriptor opened without following a link
//! and judged by that descriptor; it is never written in place. A write goes to a name
//! of its own, is flushed, renamed over the old file, and its directory is flushed: a
//! reader, a crash and a second desktop see the old file or the new one.
use super::model::{Preferences, SCHEMA_VERSION};
use crate::control::paths::{ancestors, owned_dir};
use crate::lifecycle::descend;
use nix::unistd::geteuid;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const PRODUCT: &str = "wyss-console";
const FILE: &str = "preferences.json";
/// Where a file that could not be understood is put, so that it can still be looked at.
/// One place: a second such file replaces the first.
const REJECTED: &str = "preferences.invalid.json";
/// A preference file is a few hundred bytes. Nothing larger is read.
pub const MAX_FILE: u64 = 4096;

/// What reading the preferences found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loaded {
    /// A well formed set of this version.
    Stored(Preferences),
    /// Nothing has been saved yet.
    Absent,
    /// A file that is not a preference set (damaged, out of bounds, unknown fields, too
    /// large) was set aside and is not used.
    Recovered,
    /// A file written by a newer Wyss Console. It is left exactly as it is.
    Newer,
    /// A file that is not safely this user's alone: a link, another owner, readable or
    /// writable by others, or with a second name. It is not read.
    Unsafe,
    /// The place of the file is not safely this user's, or could not be reached.
    Unavailable,
}

/// Why a write or a reset did not happen. Nothing was changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// The place is not safely this user's.
    Unsafe,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    /// `$XDG_CONFIG_HOME`.
    root: PathBuf,
}

/// An absolute path with no `..` and no control characters.
fn plain(path: PathBuf) -> Option<PathBuf> {
    let text = path.to_str()?;
    (path.is_absolute()
        && !path.components().any(|c| matches!(c, Component::ParentDir))
        && !text.chars().any(char::is_control))
    .then_some(path)
}

impl Store {
    /// The configuration directory of this user, with the defaults the XDG specification
    /// gives it. `None` where there is no home to speak of.
    pub fn from_environment() -> Option<Self> {
        let named = |variable: &str| std::env::var_os(variable).filter(|v| !v.is_empty());
        let root = match named("XDG_CONFIG_HOME") {
            Some(root) => PathBuf::from(root),
            None => PathBuf::from(named("HOME")?).join(".config"),
        };
        Some(Self { root: plain(root)? })
    }
    /// A store under an explicit configuration directory.
    pub fn at(root: PathBuf) -> Option<Self> {
        Some(Self { root: plain(root)? })
    }
    fn dir(&self) -> PathBuf {
        self.root.join(PRODUCT)
    }
    fn file(&self) -> PathBuf {
        self.dir().join(FILE)
    }

    /// Reads what is saved. Creates nothing; the only thing it changes is moving a file
    /// that cannot be understood out of the way, and removing the temporary files an
    /// interrupted write left.
    pub fn load(&self) -> Loaded {
        let dir = self.dir();
        match fs::symlink_metadata(&dir) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // A configuration directory that is not safely this user's is no place
                // to start from either. One that does not exist yet is a clean account.
                return match fs::symlink_metadata(&self.root) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Loaded::Absent,
                    _ if ancestors(&self.root).is_ok() => Loaded::Absent,
                    _ => Loaded::Unavailable,
                };
            }
            Err(_) => return Loaded::Unavailable,
            Ok(_) => {}
        }
        if owned_dir(&dir, true).is_err() {
            return Loaded::Unavailable;
        }
        remove_leftovers(&dir);
        // A write renames a new file over the old one. A read that opened the old file just
        // before sees a file with no name left, which is a file that was replaced and not
        // a file that is unsafe: it reads again, a few times at most.
        for _ in 0..READ_ATTEMPTS {
            match self.read_once() {
                Attempt::Done(loaded) => return loaded,
                Attempt::Replaced => {}
            }
        }
        Loaded::Unsafe
    }

    fn read_once(&self) -> Attempt {
        let path = self.file();
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Attempt::Done(Loaded::Absent);
            }
            // A link (ELOOP), a directory, a device: not a preference file.
            Err(_) => return Attempt::Done(Loaded::Unsafe),
        };
        let Ok(meta) = file.metadata() else {
            return Attempt::Done(Loaded::Unsafe);
        };
        // Replaced while it was being opened: nothing of it is believed.
        if meta.nlink() == 0 {
            return Attempt::Replaced;
        }
        // Judged by the descriptor that is read.
        if !meta.is_file()
            || meta.uid() != geteuid().as_raw()
            || meta.mode() & 0o777 != 0o600
            || meta.nlink() != 1
        {
            return Attempt::Done(Loaded::Unsafe);
        }
        // And the name still names that file, not another one put there since.
        match fs::symlink_metadata(&path) {
            Ok(named) if named.dev() == meta.dev() && named.ino() == meta.ino() => {}
            _ => return Attempt::Replaced,
        }
        if meta.len() > MAX_FILE {
            return Attempt::Done(self.set_aside());
        }
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        if File::take(file, MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return Attempt::Done(Loaded::Unavailable);
        }
        if bytes.len() as u64 > MAX_FILE {
            return Attempt::Done(self.set_aside());
        }
        Attempt::Done(match parse(&bytes) {
            Parsed::Stored(preferences) => Loaded::Stored(preferences),
            Parsed::Newer => Loaded::Newer,
            Parsed::Rejected => self.set_aside(),
        })
    }

    /// Moves the saved file out of the way without reading or deleting it.
    fn set_aside(&self) -> Loaded {
        let dir = self.dir();
        // The directory was judged by `load`; a failure here only leaves the file where
        // it is, and the next save replaces it.
        if fs::rename(self.file(), dir.join(REJECTED)).is_ok() {
            let _ = sync_dir(&dir);
        }
        Loaded::Recovered
    }

    /// Saves `preferences`, replacing the file whole or not at all.
    pub fn save(&self, preferences: &Preferences) -> Result<(), StoreError> {
        self.save_with(preferences, || Ok(()))
    }

    /// `save`, with a step between the flushed temporary file and the rename. Tests use
    /// it to interrupt a write at the one point where it could matter.
    pub(super) fn save_with(
        &self,
        preferences: &Preferences,
        before_rename: impl FnOnce() -> io::Result<()>,
    ) -> Result<(), StoreError> {
        let dir = descend(&self.root, &[PRODUCT], true).map_err(|_| StoreError::Unsafe)?;
        let mut bytes = serde_json::to_vec_pretty(preferences).map_err(|_| StoreError::Io)?;
        bytes.push(b'\n');
        write_atomic(&dir, FILE, &bytes, before_rename).map_err(|_| StoreError::Io)
    }

    /// Removes the saved preferences and the file set aside, so that the defaults apply.
    /// Nothing else is touched: this directory holds no other state of the product.
    pub fn reset(&self) -> Result<(), StoreError> {
        let dir = self.dir();
        match fs::symlink_metadata(&dir) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(StoreError::Io),
            Ok(_) => {}
        }
        owned_dir(&dir, true).map_err(|_| StoreError::Unsafe)?;
        remove_leftovers(&dir);
        for name in [FILE, REJECTED] {
            // Removing a link removes the link, never what it points at.
            match fs::remove_file(dir.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(StoreError::Io),
            }
        }
        sync_dir(&dir).map_err(|_| StoreError::Io)
    }
}

/// How many times a read that lost a race with a write starts over.
const READ_ATTEMPTS: usize = 5;
enum Attempt {
    Done(Loaded),
    /// The file was replaced while it was being read.
    Replaced,
}

enum Parsed {
    Stored(Preferences),
    Newer,
    Rejected,
}

/// The version is read first and by itself: what a newer version added is not for this one
/// to judge, and what is not a preference set at all is not for it to keep.
fn exact_keys(value: &serde_json::Value, expected: &[&str]) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
}

/// M1-M7 preferences used schema 1, whose exact shape had no theme field. Add only the
/// documented dark default and return the current in-memory shape. Reading remains
/// non-mutating; the next explicit preference update writes schema 2 atomically.
fn migrate_v1(value: &mut serde_json::Value) -> bool {
    if !exact_keys(
        value,
        &["schema_version", "interface", "terminal", "events"],
    ) || !exact_keys(&value["interface"], &["landing_page"])
        || !exact_keys(
            &value["terminal"],
            &["font_size", "scrollback", "cursor_style", "cursor_blink"],
        )
        || !exact_keys(&value["events"], &["time_format", "default_filter"])
    {
        return false;
    }
    let Some(root) = value.as_object_mut() else {
        return false;
    };
    let Some(interface) = root
        .get_mut("interface")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return false;
    };
    interface.insert(
        "theme".to_owned(),
        serde_json::Value::String("dark".to_owned()),
    );
    root.insert(
        "schema_version".to_owned(),
        serde_json::Value::from(SCHEMA_VERSION),
    );
    true
}

fn parse(bytes: &[u8]) -> Parsed {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Parsed::Rejected;
    };
    let Some(version) = value.get("schema_version").and_then(|v| v.as_u64()) else {
        return Parsed::Rejected;
    };
    if version > u64::from(SCHEMA_VERSION) {
        return Parsed::Newer;
    }
    if version == 1 && !migrate_v1(&mut value) {
        return Parsed::Rejected;
    }
    match serde_json::from_value::<Preferences>(value) {
        Ok(preferences) if preferences.validate().is_ok() => Parsed::Stored(preferences),
        _ => Parsed::Rejected,
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);
const TEMP_PREFIX: &str = ".preferences.";
const TEMP_SUFFIX: &str = ".tmp";

fn temp_name() -> String {
    format!(
        "{TEMP_PREFIX}{}.{}{TEMP_SUFFIX}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn write_atomic(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    before_rename: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let temp = dir.join(temp_name());
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        before_rename()?;
        fs::rename(&temp, dir.join(name))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
        return written;
    }
    sync_dir(dir)
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(dir)?
        .sync_all()
}

/// A temporary file younger than this may belong to a write that is still going on, in
/// another desktop of this user. A write takes milliseconds.
const LEFTOVER_AGE: std::time::Duration = std::time::Duration::from_secs(60);

/// Temporary files an interrupted write left, by their exact name pattern: the regular
/// files of this user that are too old to belong to a write that is still going on.
fn remove_leftovers(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let middle = name
            .strip_prefix(TEMP_PREFIX)
            .and_then(|rest| rest.strip_suffix(TEMP_SUFFIX));
        let Some(middle) = middle else { continue };
        let numbers = middle.split('.').collect::<Vec<_>>();
        if numbers.len() != 2
            || !numbers
                .iter()
                .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let path = entry.path();
        if let Ok(meta) = fs::symlink_metadata(&path)
            && meta.is_file()
            && meta.uid() == geteuid().as_raw()
            && meta
                .modified()
                .ok()
                .and_then(|written| written.elapsed().ok())
                .is_some_and(|age| age >= LEFTOVER_AGE)
        {
            let _ = fs::remove_file(path);
        }
    }
}
