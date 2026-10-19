//! What the Console's own private stores need from the file system: a directory only this
//! user can enter, and a file that is either wholly written or not there (14 §3).
//!
//! The provider's files are not written with these: they are written by `mutate.rs`,
//! through descriptors judged by the same rules as the reads.
use nix::{
    fcntl::{OFlag, openat, renameat},
    sys::stat::{Mode, fstat},
    unistd::{fsync, geteuid, write},
};
use std::{
    ffi::OsStr,
    fs,
    os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// The directory exists and someone else could enter or change it.
    Unsafe,
    /// The disk refused.
    Io,
}

/// A directory of this user's, mode 0700, made if it is not there. One that exists with
/// any other owner or a looser mode is refused, not repaired: someone else may have had it.
pub fn private_dir(path: &Path) -> Result<PathBuf, StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| StoreError::Io)?;
    }
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(StoreError::Io),
    }
    // `symlink_metadata`: a link in the place of the directory is not followed.
    let meta = fs::symlink_metadata(path).map_err(|_| StoreError::Io)?;
    use std::os::unix::fs::MetadataExt;
    if !meta.is_dir() || meta.uid() != geteuid().as_raw() || meta.permissions().mode() & 0o077 != 0
    {
        return Err(StoreError::Unsafe);
    }
    Ok(path.to_owned())
}

/// Writes `bytes` as `name` in `dir`: a new file under a name nothing else uses, flushed,
/// renamed over, the directory flushed. The file is 0600. If a step fails the old file
/// is as it was and the new one is gone.
pub fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
    let handle = fs::File::open(dir).map_err(|_| StoreError::Io)?;
    let dirfd = handle.as_fd();
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).map_err(|_| StoreError::Io)?;
    let temporary = format!(
        ".{name}.{}.tmp",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let created = openat(
        Some(dirfd.as_raw_fd()),
        OsStr::new(&temporary),
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(|_| StoreError::Io)?;
    // SAFETY: `openat` returned this descriptor just now and nothing else owns it.
    let file = unsafe { OwnedFd::from_raw_fd(created) };
    let cleanup = || {
        let _ = nix::unistd::unlinkat(
            Some(dirfd.as_raw_fd()),
            OsStr::new(&temporary),
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        );
    };
    let mut written = 0;
    while written < bytes.len() {
        match write(&file, &bytes[written..]) {
            Ok(0) | Err(_) => {
                cleanup();
                return Err(StoreError::Io);
            }
            Ok(count) => written += count,
        }
    }
    if fsync(file.as_raw_fd()).is_err()
        || renameat(
            Some(dirfd.as_raw_fd()),
            OsStr::new(&temporary),
            Some(dirfd.as_raw_fd()),
            OsStr::new(name),
        )
        .is_err()
    {
        cleanup();
        return Err(StoreError::Io);
    }
    // The rename is only durable once the directory is.
    fsync(dirfd.as_raw_fd()).map_err(|_| StoreError::Io)?;
    let _ = fstat(file.as_raw_fd());
    Ok(())
}

/// Removes the leftovers of writes that were interrupted, by their exact name pattern.
pub fn remove_interrupted(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let ours = name.starts_with('.')
            && name.ends_with(".tmp")
            && name.rsplit('.').nth(1).is_some_and(|random| {
                random.len() == 16 && random.bytes().all(|b| b.is_ascii_hexdigit())
            });
        if ours
            && entry.file_type().is_ok_and(|kind| kind.is_file())
            && fs::remove_file(entry.path()).is_ok()
        {
            removed += 1;
        }
    }
    removed
}
