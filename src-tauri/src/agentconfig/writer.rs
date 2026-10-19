//! Writing a provider's file (07 §5.4): a new file beside it, flushed, renamed over the old
//! one, the directory flushed. The file is reached through a directory descriptor that was
//! judged on the way down, so a link put in the place of a directory after the judgement is
//! not followed, and the rename happens in that directory and no other.
//!
//! The new file gets the old one's mode. If any step before the rename fails, the old file
//! is as it was and the new one is removed. A step that "crashes" leaves exactly what a
//! death would leave, for the next start to repair.
use super::fault::{Faults, Injected, Step};
use super::place::Parent;
use nix::{
    errno::Errno,
    fcntl::{AtFlags, Flock, FlockArg, OFlag, openat, renameat},
    sys::stat::{FchmodatFlags, FileStat, Mode, fchmodat, fstatat},
    unistd::{UnlinkatFlags, fsync, geteuid, unlinkat, write},
};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::ffi::OsStrExt,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteError {
    /// The disk refused; the old file is as it was.
    Failed,
    /// The file is not the one that was read: someone replaced or changed it.
    Replaced,
    /// The process died here (injected): nothing was cleaned up.
    Crashed,
    /// The file is not this user's to write.
    NotOwner,
    /// The rename happened and flushing the directory did not: the new file is in place,
    /// and whether it survives a power cut is not known.
    AfterRename,
}

/// What names a file on disk at a moment: if it differs a moment later, it was replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    dev: u64,
    ino: u64,
    size: i64,
    mtime: (i64, i64),
    mode: u32,
    uid: u32,
}

fn identity(stat: &FileStat) -> Identity {
    Identity {
        dev: stat.st_dev,
        ino: stat.st_ino,
        size: stat.st_size,
        mtime: (stat.st_mtime, stat.st_mtime_nsec),
        mode: stat.st_mode,
        uid: stat.st_uid,
    }
}

/// The identity of the file in its directory, without following a link.
pub fn identity_of(parent: &Parent) -> Result<Identity, WriteError> {
    fstatat(
        Some(parent.dir.as_raw_fd()),
        parent.name.as_os_str(),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map(|stat| identity(&stat))
    .map_err(|_| WriteError::Failed)
}

impl Identity {
    pub fn is_owned_by_this_user(&self) -> bool {
        self.uid == geteuid().as_raw()
    }
}

/// Whether the names beside the file include a conflict copy of it, in the form the
/// replication tool uses: `<stem>.sync-conflict-<date>-<time>-<id>.<extension>`.
pub fn conflict_copy_beside(names: &[OsString], file: &OsString) -> bool {
    let file = file.to_string_lossy();
    let stem = file.split('.').next().unwrap_or_default();
    names.iter().any(|name| {
        let name = name.to_string_lossy();
        !stem.is_empty() && name.starts_with(&format!("{stem}.sync-conflict-"))
    })
}

/// Replaces the file with `bytes`. `expected` is what the file was when it was last read:
/// if it is not that any more at the moment of the rename, nothing is replaced.
pub fn replace(
    parent: &Parent,
    bytes: &[u8],
    expected: Identity,
    faults: &Faults,
) -> Result<(), WriteError> {
    if !expected.is_owned_by_this_user() {
        return Err(WriteError::NotOwner);
    }
    let dirfd = parent.dir.as_raw_fd();
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).map_err(|_| WriteError::Failed)?;
    // The pattern the start-up cleanup knows (`remove_interrupted`).
    let temporary = format!(
        ".{}.{}.tmp",
        parent.name.to_string_lossy(),
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let temporary = OsString::from(temporary);

    let step = |step: Step| -> Result<(), WriteError> {
        faults.check(step).map_err(|injected| match injected {
            Injected::Failed => WriteError::Failed,
            Injected::Crashed => WriteError::Crashed,
        })
    };
    // A failure cleans up; a crash does not.
    let cleanup = |error: WriteError| {
        if error != WriteError::Crashed {
            let _ = unlinkat(
                Some(dirfd),
                temporary.as_os_str(),
                UnlinkatFlags::NoRemoveDir,
            );
        }
        error
    };

    step(Step::TemporaryCreated)?;
    let created = openat(
        Some(dirfd),
        temporary.as_os_str(),
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(|_| WriteError::Failed)?;
    // SAFETY: `openat` returned this descriptor just now and nothing else owns it.
    let file = unsafe { OwnedFd::from_raw_fd(created) };
    // The old file's permissions, not the process's umask.
    fchmodat(
        Some(dirfd),
        temporary.as_os_str(),
        Mode::from_bits_truncate(expected.mode & 0o7777),
        FchmodatFlags::FollowSymlink,
    )
    .map_err(|_| cleanup(WriteError::Failed))?;

    step(Step::TemporaryWritten).map_err(cleanup)?;
    let mut written = 0;
    while written < bytes.len() {
        match write(&file, &bytes[written..]) {
            Ok(0) | Err(_) => return Err(cleanup(WriteError::Failed)),
            Ok(count) => written += count,
        }
    }
    step(Step::TemporaryFlushed).map_err(cleanup)?;
    fsync(file.as_raw_fd()).map_err(|_| cleanup(WriteError::Failed))?;

    // The last look before the rename: still the file that was read.
    match identity_of(parent) {
        Ok(now) if now == expected => (),
        Ok(_) | Err(_) => return Err(cleanup(WriteError::Replaced)),
    }
    step(Step::Renamed).map_err(cleanup)?;
    renameat(
        Some(dirfd),
        temporary.as_os_str(),
        Some(dirfd),
        parent.name.as_os_str(),
    )
    .map_err(|_| cleanup(WriteError::Failed))?;
    step(Step::DirectoryFlushed).map_err(|error| match error {
        WriteError::Failed => WriteError::AfterRename,
        other => other,
    })?;
    fsync(dirfd).map_err(|_| WriteError::AfterRename)
}

/// An advisory lock on one target, held for the length of an apply and released if the
/// holder dies (07 §5.5). Two windows, or two processes, apply one after the other.
pub struct TargetLock {
    _held: Flock<std::fs::File>,
}

pub fn lock_target(locks: &Path, target: &Path) -> Result<TargetLock, WriteError> {
    use sha2::{Digest, Sha256};
    let name: String = Sha256::digest(target.as_os_str().as_bytes())[..12]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(locks.join(format!("{name}.lock")))
        .map_err(|_| WriteError::Failed)?;
    let started = Instant::now();
    let mut file = file;
    loop {
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(held) => return Ok(TargetLock { _held: held }),
            Err((back, Errno::EWOULDBLOCK)) if started.elapsed() < Duration::from_secs(2) => {
                file = back;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return Err(WriteError::Failed),
        }
    }
}

/// Removes leftovers of an interrupted write beside `name` in the directory: only files
/// with the exact pattern of `replace`.
pub fn remove_leftovers(parent: &Parent) -> usize {
    let Ok(dir) = fs::read_dir(format!("/proc/self/fd/{}", parent.dir.as_raw_fd())) else {
        return 0;
    };
    let prefix = format!(".{}.", parent.name.to_string_lossy());
    let mut removed = 0;
    for entry in dir.flatten() {
        let name = entry.file_name();
        let text = name.to_string_lossy();
        let ours = text.starts_with(&prefix)
            && text.ends_with(".tmp")
            && text.len() == prefix.len() + 16 + 4
            && text[prefix.len()..prefix.len() + 16]
                .bytes()
                .all(|b| b.is_ascii_hexdigit());
        if ours
            && unlinkat(
                Some(parent.dir.as_raw_fd()),
                name.as_os_str(),
                UnlinkatFlags::NoRemoveDir,
            )
            .is_ok()
        {
            removed += 1;
        }
    }
    removed
}
