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
    fcntl::{AtFlags, Flock, FlockArg, OFlag, RenameFlags, openat, renameat, renameat2},
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

    // The last look before the swap: still the file that was read.
    match identity_of(parent) {
        Ok(now) if now == expected => (),
        Ok(_) | Err(_) => return Err(cleanup(WriteError::Replaced)),
    }
    step(Step::Renamed).map_err(cleanup)?;
    #[cfg(test)]
    seam::before_swap();
    // The new file takes the old one's place and the old one goes to the temporary name,
    // in one step. A provider that wrote between the look above and here has written to
    // the file that is now at the temporary name: it is found there, and put back.
    match renameat2(
        Some(dirfd),
        temporary.as_os_str(),
        Some(dirfd),
        parent.name.as_os_str(),
        RenameFlags::RENAME_EXCHANGE,
    ) {
        Ok(()) => {
            // A disk cannot refuse a step that is already done; a death here is the case
            // recovery is written for.
            if step(Step::Exchanged) == Err(WriteError::Crashed) {
                return Err(WriteError::Crashed);
            }
            let displaced = fstatat(
                Some(dirfd),
                temporary.as_os_str(),
                AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .map(|stat| identity(&stat));
            if displaced != Ok(expected) {
                // Not the file that was read: put it back, so the other writer's version
                // is the one in place. The exchange undone first; if that cannot be done,
                // the plain rename of the displaced file over the name does the same.
                // Never `cleanup` here: the temporary name holds THEIR file until it is
                // back, and deleting it would lose it.
                let undone = step(Step::SwappedBack).is_ok()
                    && renameat2(
                        Some(dirfd),
                        temporary.as_os_str(),
                        Some(dirfd),
                        parent.name.as_os_str(),
                        RenameFlags::RENAME_EXCHANGE,
                    )
                    .is_ok();
                if undone {
                    // Ours is at the temporary name now.
                    let _ = unlinkat(
                        Some(dirfd),
                        temporary.as_os_str(),
                        UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(WriteError::Replaced);
                }
                return match renameat(
                    Some(dirfd),
                    temporary.as_os_str(),
                    Some(dirfd),
                    parent.name.as_os_str(),
                ) {
                    Ok(()) => Err(WriteError::Replaced),
                    // Neither worked: their file stays at the temporary name, which recovery
                    // does not delete (it keeps what it cannot account for).
                    Err(_) => Err(WriteError::Failed),
                };
            }
            // What was there is now at the temporary name, and is removed.
            let _ = unlinkat(
                Some(dirfd),
                temporary.as_os_str(),
                UnlinkatFlags::NoRemoveDir,
            );
        }
        // A file system that cannot exchange: the plain rename, as before.
        Err(Errno::EINVAL | Errno::ENOSYS | Errno::EOPNOTSUPP) => {
            renameat(
                Some(dirfd),
                temporary.as_os_str(),
                Some(dirfd),
                parent.name.as_os_str(),
            )
            .map_err(|_| cleanup(WriteError::Failed))?;
        }
        Err(_) => return Err(cleanup(WriteError::Failed)),
    }
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

/// The temporary files an interrupted `replace` of this file can leave, by their exact
/// pattern: `.<name>.<16 hex>.tmp`.
pub fn leftovers(parent: &Parent) -> Vec<OsString> {
    let Ok(dir) = fs::read_dir(format!("/proc/self/fd/{}", parent.dir.as_raw_fd())) else {
        return Vec::new();
    };
    let prefix = format!(".{}.", parent.name.to_string_lossy());
    dir.flatten()
        .map(|entry| entry.file_name())
        .filter(|name| {
            let text = name.to_string_lossy();
            text.starts_with(&prefix)
                && text.ends_with(".tmp")
                && text.len() == prefix.len() + 16 + 4
                && text[prefix.len()..prefix.len() + 16]
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
        })
        .collect()
}

/// A leftover's bytes, read without following a link and never more than `limit`.
pub fn read_leftover(parent: &Parent, name: &OsString, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let fd = openat(
        Some(parent.dir.as_raw_fd()),
        name.as_os_str(),
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .ok()?;
    // SAFETY: `openat` returned this descriptor just now and nothing else owns it.
    let file = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

pub fn remove_leftover(parent: &Parent, name: &OsString) -> bool {
    unlinkat(
        Some(parent.dir.as_raw_fd()),
        name.as_os_str(),
        UnlinkatFlags::NoRemoveDir,
    )
    .is_ok()
}

/// Puts a leftover that holds somebody else's newer file back over the target, exchanging
/// the two so that nothing is lost on the way; ours, now at the leftover's name, is removed.
pub fn put_back(parent: &Parent, name: &OsString) -> bool {
    let dirfd = parent.dir.as_raw_fd();
    let swapped = renameat2(
        Some(dirfd),
        name.as_os_str(),
        Some(dirfd),
        parent.name.as_os_str(),
        RenameFlags::RENAME_EXCHANGE,
    )
    .is_ok();
    if swapped {
        let _ = unlinkat(Some(dirfd), name.as_os_str(), UnlinkatFlags::NoRemoveDir);
    }
    swapped
}

/// Keeps what recovery cannot account for under a name no cleanup matches, beside the file.
pub fn keep_leftover(parent: &Parent, name: &OsString, tag: &str) -> bool {
    let dirfd = parent.dir.as_raw_fd();
    let kept = OsString::from(format!("{}.recovered-{tag}", parent.name.to_string_lossy()));
    renameat(Some(dirfd), name.as_os_str(), Some(dirfd), kept.as_os_str()).is_ok()
}

/// A seam for a test to be the provider writing in the instant between the last look and the
/// swap, which no lock can exclude.
#[cfg(test)]
pub(crate) mod seam {
    use std::cell::RefCell;
    thread_local! {
        static BEFORE_SWAP: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }
    pub fn on_before_swap(action: impl FnOnce() + 'static) {
        BEFORE_SWAP.with(|slot| *slot.borrow_mut() = Some(Box::new(action)));
    }
    pub(super) fn before_swap() {
        if let Some(action) = BEFORE_SWAP.with(|slot| slot.borrow_mut().take()) {
            action();
        }
    }
}
