//! The cooperative, exclusive terminal input claim (contract §3).
//!
//! One claim per host epoch and terminal, held in two ways at once: a table of this process
//! (the Console's windows share one process) and an `flock` on a private file (other Console
//! processes). Taking input is explicit; a mirror view without the claim is read-only; the claim
//! is released on blur, on detach and when the window or the process ends, because the kernel
//! drops the lock with the process. This coordinates Console clients with each other. It does
//! not stop an older window, a older writer or any other process of the same user from
//! writing to the terminal, and the interface says so.
use nix::fcntl::{Flock, FlockArg};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimError {
    /// Another window of this process or another Console process holds it.
    HeldByOther,
    /// There is no private place to coordinate in, so no claim can be taken.
    Unavailable,
}

struct Held {
    window: String,
    _lock: Flock<fs::File>,
}

pub struct Claims {
    dir: Option<PathBuf>,
    held: Mutex<HashMap<String, Held>>,
}

fn key(epoch: &str, terminal: &str) -> String {
    let digest = Sha256::digest(format!("{epoch}\0{terminal}"));
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

impl Claims {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            held: Mutex::new(HashMap::new()),
        }
    }

    /// `$XDG_RUNTIME_DIR/wyss-console/claims`, which is private to the user.
    pub fn from_environment() -> Self {
        Self::new(
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| p.join("wyss-console").join("claims")),
        )
    }

    fn table(&self) -> std::sync::MutexGuard<'_, HashMap<String, Held>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the claim for `window`. Claiming what the window already holds is a no-op.
    pub fn claim(&self, window: &str, epoch: &str, terminal: &str) -> Result<(), ClaimError> {
        let dir = self.dir.as_ref().ok_or(ClaimError::Unavailable)?;
        crate::native::store::private_dir(dir).map_err(|_| ClaimError::Unavailable)?;
        let key = key(epoch, terminal);
        let mut table = self.table();
        match table.get(&key) {
            Some(held) if held.window == window => return Ok(()),
            Some(_) => return Err(ClaimError::HeldByOther),
            None => {}
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(dir.join(format!("{key}.lock")))
            .map_err(|_| ClaimError::Unavailable)?;
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(lock) => {
                table.insert(
                    key,
                    Held {
                        window: window.to_owned(),
                        _lock: lock,
                    },
                );
                Ok(())
            }
            Err((_, nix::errno::Errno::EWOULDBLOCK)) => Err(ClaimError::HeldByOther),
            Err(_) => Err(ClaimError::Unavailable),
        }
    }

    pub fn release(&self, window: &str, epoch: &str, terminal: &str) {
        let key = key(epoch, terminal);
        let mut table = self.table();
        if table.get(&key).is_some_and(|held| held.window == window) {
            table.remove(&key);
        }
    }

    /// Releases everything a window holds: it blurred, closed or crashed.
    pub fn release_window(&self, window: &str) {
        self.table().retain(|_, held| held.window != window);
    }

    pub fn holds(&self, window: &str, epoch: &str, terminal: &str) -> bool {
        self.table()
            .get(&key(epoch, terminal))
            .is_some_and(|held| held.window == window)
    }

    /// The window of this process that holds the claim, if one does.
    pub fn holder(&self, epoch: &str, terminal: &str) -> Option<String> {
        self.table()
            .get(&key(epoch, terminal))
            .map(|held| held.window.clone())
    }
}
