//! The locked, durable, compare-and-swap store file (contract §2).
//!
//! `$XDG_STATE_HOME/wyss-console/projects/v1/store.json` (directory 0700, file 0600, user-owned
//! non-symlink ancestors). One transaction holds an interprocess `flock`, rereads the current
//! file, checks the revision the caller saw, applies the change, validates the whole store and
//! replaces the file with a fsynced same-directory rename. A loser of a race gets
//! `revision_conflict` and refetches; nothing is last-write-wins. A store that is corrupt,
//! newer or oversized is never rewritten, repaired or replaced by an empty one: its exact bytes
//! stay on disk and every mutation is refused.
use crate::native::store::private_dir;
use nix::fcntl::{Flock, FlockArg};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use wyss_console::contracts::m1::{
    M1ErrorCode, MAX_STORE_BYTES, StoreV1, decode, encode, scalar::Counter,
};

const STORE_FILE: &str = "store.json";
const LOCK_FILE: &str = "store.lock";
const TEMP_PREFIX: &str = "store.json.tmp-";
/// How long a transaction waits for another holder of the lock.
const LOCK_WAIT: Duration = Duration::from_secs(5);

/// `$XDG_STATE_HOME/wyss-console/projects/v1`, or `~/.local/state/...`.
pub fn default_dir() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|h| h.join(".local/state"))
        })?;
    Some(state.join("wyss-console").join("projects").join("v1"))
}

/// What reading the file found.
#[derive(Debug, PartialEq)]
pub enum Loaded {
    /// No store yet: offer a migration preview or an explicit empty initialisation.
    Missing,
    Ready(Box<StoreV1>),
    /// Present but not loadable. The bytes are untouched and every mutation is refused.
    Unusable(Unusable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unusable {
    pub code: M1ErrorCode,
    pub field: &'static str,
}

#[derive(Debug)]
pub enum StoreError {
    /// Another writer committed first; the current revision is given.
    RevisionConflict { current: u64 },
    /// The store is absent where a revision was expected, or present where it was to be created.
    StoreState(&'static str),
    Unusable(Unusable),
    /// The change produced a store the contract refuses.
    Invalid(wyss_console::contracts::m1::M1Error),
    /// A failure before the new file replaced the store: nothing changed.
    Io(io::Error),
    /// A failure at or after the replacement: the new store may or may not be the one on disk.
    PublishUnknown(io::Error),
}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// A place in a write where a test can stop it.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Point {
    AfterLock,
    BeforeTempWrite,
    /// After half of the new bytes are in the temporary file.
    MidTempWrite,
    BeforeFileFsync,
    BeforeRename,
    AfterRename,
    BeforeDirFsync,
}

#[cfg(test)]
pub type Faults = std::sync::Arc<dyn Fn(Point) -> io::Result<()> + Send + Sync>;

pub struct Store {
    dir: PathBuf,
    #[cfg(test)]
    faults: Option<Faults>,
}

fn io_other(message: &'static str) -> io::Error {
    io::Error::other(message)
}

/// Every ancestor the user owns must be a real directory: a link in the chain could redirect
/// the store. Ancestors owned by someone else (`/`, `/home`) are the system's and are skipped.
fn check_ancestors(dir: &Path) -> io::Result<()> {
    let me = nix::unistd::geteuid().as_raw();
    for ancestor in dir.ancestors() {
        let meta = fs::symlink_metadata(ancestor)?;
        if meta.uid() != me {
            break;
        }
        if !meta.is_dir() {
            return Err(io_other("a store ancestor is not a plain directory"));
        }
    }
    Ok(())
}

impl Store {
    /// Makes the directory private and checks its ancestors; creates no store file.
    pub fn open(dir: PathBuf) -> io::Result<Self> {
        if !dir.is_absolute() {
            return Err(io_other("the store directory must be absolute"));
        }
        private_dir(&dir)?;
        check_ancestors(&dir)?;
        let meta = fs::symlink_metadata(&dir)?;
        if meta.uid() != nix::unistd::geteuid().as_raw() {
            return Err(io_other("the store directory is not the user's"));
        }
        Ok(Self {
            dir,
            #[cfg(test)]
            faults: None,
        })
    }

    #[cfg(test)]
    pub fn with_faults(mut self, faults: Faults) -> Self {
        self.faults = Some(faults);
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn store_path(&self) -> PathBuf {
        self.dir.join(STORE_FILE)
    }

    #[cfg(test)]
    fn at(&self, point: Point) -> io::Result<()> {
        match &self.faults {
            Some(faults) => faults(point),
            None => Ok(()),
        }
    }

    /// Reads and fully validates the file without taking the lock: the writer replaces it
    /// atomically, so this sees the old store or the new one.
    pub fn read(&self) -> io::Result<Loaded> {
        read_store(&self.store_path())
    }

    fn lock(&self) -> Result<Flock<fs::File>, StoreError> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(self.dir.join(LOCK_FILE))?;
        let deadline = Instant::now() + LOCK_WAIT;
        let mut file = file;
        loop {
            match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
                Ok(lock) => return Ok(lock),
                Err((back, nix::errno::Errno::EWOULDBLOCK)) if Instant::now() < deadline => {
                    file = back;
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err((_, errno)) => return Err(StoreError::Io(io::Error::from(errno))),
            }
        }
    }

    /// Removes temporary files a crashed writer left. Only a holder of the lock writes them.
    fn sweep(&self) {
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    /// Creates the store from `initial` when none exists. A store already there is a
    /// `StoreState` error, so two windows initialising together produce exactly one store.
    pub fn create(&self, initial: StoreV1) -> Result<StoreV1, StoreError> {
        let _lock = self.lock()?;
        #[cfg(test)]
        self.at(Point::AfterLock)?;
        self.sweep();
        match read_store(&self.store_path())? {
            Loaded::Missing => {}
            Loaded::Ready(_) => return Err(StoreError::StoreState("exists")),
            Loaded::Unusable(unusable) => return Err(StoreError::Unusable(unusable)),
        }
        let bytes = encode(&initial).map_err(StoreError::Invalid)?;
        self.publish(&bytes)?;
        Ok(initial)
    }

    /// Applies `change` to the store whose revision is `expected`, bumps the revision and
    /// publishes the result. `change` runs under the lock, on a copy: if it fails nothing is
    /// written. The committed store is returned.
    pub fn transact<E>(
        &self,
        expected: Counter,
        change: impl FnOnce(&mut StoreV1) -> Result<(), E>,
    ) -> Result<Result<StoreV1, E>, StoreError> {
        let _lock = self.lock()?;
        #[cfg(test)]
        self.at(Point::AfterLock)?;
        self.sweep();
        let current = match read_store(&self.store_path())? {
            Loaded::Ready(store) => *store,
            Loaded::Missing => return Err(StoreError::StoreState("missing")),
            Loaded::Unusable(unusable) => return Err(StoreError::Unusable(unusable)),
        };
        if current.revision != expected {
            return Err(StoreError::RevisionConflict {
                current: current.revision.get(),
            });
        }
        let mut next = current;
        if let Err(error) = change(&mut next) {
            return Ok(Err(error));
        }
        next.revision = next
            .revision
            .next()
            .ok_or(StoreError::StoreState("revision_overflow"))?;
        let bytes = encode(&next).map_err(StoreError::Invalid)?;
        self.publish(&bytes)?;
        Ok(Ok(next))
    }

    /// Writes the new bytes beside the store, fsyncs them, renames them over it and fsyncs the
    /// directory: after a crash the file is the complete old store or the complete new one.
    fn publish(&self, bytes: &[u8]) -> Result<(), StoreError> {
        let target = self.store_path();
        let temp = self
            .dir
            .join(format!("{TEMP_PREFIX}{}", uuid::Uuid::new_v4().simple()));
        let result = (|| {
            #[cfg(test)]
            self.at(Point::BeforeTempWrite)?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
                .open(&temp)?;
            let half = bytes.len() / 2;
            file.write_all(&bytes[..half])?;
            #[cfg(test)]
            self.at(Point::MidTempWrite)?;
            file.write_all(&bytes[half..])?;
            #[cfg(test)]
            self.at(Point::BeforeFileFsync)?;
            file.sync_all()?;
            #[cfg(test)]
            self.at(Point::BeforeRename)?;
            fs::rename(&temp, &target)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(StoreError::Io(error));
        }
        // From here on the new store is in place; a failure only leaves its durability unknown.
        (|| {
            #[cfg(test)]
            self.at(Point::AfterRename)?;
            #[cfg(test)]
            self.at(Point::BeforeDirFsync)?;
            fs::File::open(&self.dir)?.sync_all()
        })()
        .map_err(StoreError::PublishUnknown)
    }
}

/// Reads the file as it is: absent, a loaded store, or unusable with the reason.
fn read_store(path: &Path) -> io::Result<Loaded> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Loaded::Missing),
        // A link in the place of the file is not the store.
        Err(error) if error.raw_os_error() == Some(nix::libc::ELOOP) => {
            return Ok(unusable(M1ErrorCode::InvalidRecord, "store"));
        }
        Err(error) => return Err(error),
    };
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != nix::unistd::geteuid().as_raw()
        || meta.permissions().mode() & 0o077 != 0
    {
        return Ok(unusable(M1ErrorCode::InvalidRecord, "store"));
    }
    if meta.len() > MAX_STORE_BYTES as u64 {
        return Ok(unusable(M1ErrorCode::CapacityExceeded, "store"));
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_STORE_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    Ok(match decode(&bytes) {
        Ok(store) => Loaded::Ready(Box::new(store)),
        Err(error) => Loaded::Unusable(Unusable {
            code: error.code,
            field: error.field,
        }),
    })
}

fn unusable(code: M1ErrorCode, field: &'static str) -> Loaded {
    Loaded::Unusable(Unusable { code, field })
}
