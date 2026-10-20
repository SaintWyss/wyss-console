//! Exclusive private per-launch staging directories. Inactive; nothing here starts a provider.
//!
//! One directory per runtime instance, created with `mkdirat(..., 0700)` which fails if the name
//! exists in any form (directory, file, symlink), then pinned by descriptor. Files are created
//! `O_EXCL | O_NOFOLLOW` and made read-only. Cleanup removes only entries this value created and
//! only after re-checking, by descriptor, that each is still the same regular single-link file or
//! directory it made, so a replaced entry (symlink, hardlink, another launch's file) is never
//! deleted and one launch can never overwrite or clean another's catalogue.
use super::protocol::canonical;
use crate::contracts::{
    control::{ControlError as E, ControlResult},
    identities::AgentRuntimeInstanceId,
};
use nix::{
    fcntl::{OFlag, openat},
    sys::stat::{FileStat, Mode, fstat, fstatat, mkdirat},
    unistd::{UnlinkatFlags, unlinkat},
};
use std::{
    io::Write,
    os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
    path::Path,
};

const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILES: usize = 8;

fn io(_: impl std::fmt::Debug) -> E {
    E::StorageUnavailable
}
fn uid() -> u32 {
    nix::unistd::getuid().as_raw()
}
fn is_dir(s: &FileStat) -> bool {
    s.st_mode & libc_s_ifmt() == libc_s_ifdir()
}
fn is_reg(s: &FileStat) -> bool {
    s.st_mode & libc_s_ifmt() == libc_s_ifreg()
}
const fn libc_s_ifmt() -> u32 {
    nix::libc::S_IFMT
}
const fn libc_s_ifdir() -> u32 {
    nix::libc::S_IFDIR
}
const fn libc_s_ifreg() -> u32 {
    nix::libc::S_IFREG
}

/// An existing directory, owned by the current user, mode 0700, which holds launch directories.
pub struct StagingRoot {
    fd: OwnedFd,
}
impl StagingRoot {
    pub fn open(path: &Path) -> ControlResult<Self> {
        let fd = openat(
            None,
            path,
            OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_RDONLY | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| E::UnsafePath)?;
        // SAFETY: a fresh descriptor returned by openat and owned by nobody else.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let st = fstat(fd.as_raw_fd()).map_err(io)?;
        if !is_dir(&st) || st.st_uid != uid() || st.st_mode & 0o077 != 0 {
            return Err(E::UnsafePath);
        }
        Ok(Self { fd })
    }
    /// Creates `<root>/<instance>` exclusively. An existing name, whatever it is, refuses.
    pub fn create(&self, instance: &AgentRuntimeInstanceId) -> ControlResult<LaunchStaging> {
        let name = instance.as_str().to_owned();
        canonical(&name)?;
        mkdirat(
            Some(self.fd.as_raw_fd()),
            name.as_str(),
            Mode::from_bits_truncate(0o700),
        )
        .map_err(|e| {
            if e == nix::errno::Errno::EEXIST {
                E::AlreadyRunning
            } else {
                E::StorageUnavailable
            }
        })?;
        let dir = openat(
            Some(self.fd.as_raw_fd()),
            name.as_str(),
            OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_RDONLY | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(io)?;
        // SAFETY: a fresh descriptor returned by openat and owned by nobody else.
        let dir = unsafe { OwnedFd::from_raw_fd(dir) };
        let st = fstat(dir.as_raw_fd()).map_err(io)?;
        if !is_dir(&st) || st.st_uid != uid() || st.st_mode & 0o077 != 0 {
            return Err(E::UnsafePath);
        }
        Ok(LaunchStaging {
            root: self.fd.try_clone().map_err(io)?,
            dir,
            name,
            ino: (st.st_dev, st.st_ino),
            files: Vec::new(),
        })
    }
}

pub struct LaunchStaging {
    root: OwnedFd,
    dir: OwnedFd,
    name: String,
    ino: (u64, u64),
    files: Vec<(String, (u64, u64))>,
}
impl LaunchStaging {
    fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 64
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    }
    /// Writes a new read-only file. An existing entry of that name (file, symlink, hardlink)
    /// refuses; nothing is ever overwritten or followed.
    pub fn write_file(&mut self, name: &str, bytes: &[u8]) -> ControlResult<()> {
        if !Self::valid_name(name) || bytes.len() > MAX_FILE_BYTES || self.files.len() >= MAX_FILES
        {
            return Err(E::InvalidRequest);
        }
        let fd = openat(
            Some(self.dir.as_raw_fd()),
            name,
            OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_WRONLY | OFlag::O_CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(|e| {
            if e == nix::errno::Errno::EEXIST || e == nix::errno::Errno::ELOOP {
                E::AlreadyRunning
            } else {
                E::StorageUnavailable
            }
        })?;
        // SAFETY: a fresh descriptor returned by openat and owned by nobody else.
        let mut file = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        let st = fstat(file.as_raw_fd()).map_err(io)?;
        if !is_reg(&st) || st.st_nlink != 1 || st.st_uid != uid() {
            return Err(E::UnsafePath);
        }
        self.files.push((name.to_owned(), (st.st_dev, st.st_ino)));
        file.write_all(bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        nix::sys::stat::fchmod(file.as_raw_fd(), Mode::from_bits_truncate(0o400)).map_err(io)?;
        Ok(())
    }
    /// Identity and size of a file this launch created, or `None` if it is not (or no longer) it.
    pub fn identity(&self, name: &str) -> ControlResult<(u64, u64, i64)> {
        let (_, expected) = self
            .files
            .iter()
            .find(|(n, _)| n == name)
            .ok_or(E::InvalidRequest)?;
        let st = fstatat(
            Some(self.dir.as_raw_fd()),
            name,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(io)?;
        if !is_reg(&st) || st.st_nlink != 1 || (st.st_dev, st.st_ino) != *expected {
            return Err(E::UnsafePath);
        }
        Ok((st.st_dev, st.st_ino, st.st_size))
    }
    /// Removes exactly what this launch created, verifying identity first. Any mismatch leaves
    /// the foreign entry untouched and reports `UnsafePath`; nothing outside is ever removed.
    pub fn cleanup(mut self) -> ControlResult<()> {
        let mut outcome = Ok(());
        for (name, ino) in std::mem::take(&mut self.files) {
            let same = fstatat(
                Some(self.dir.as_raw_fd()),
                name.as_str(),
                nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .is_ok_and(|s| is_reg(&s) && s.st_nlink == 1 && (s.st_dev, s.st_ino) == ino);
            if same {
                let _ = unlinkat(
                    Some(self.dir.as_raw_fd()),
                    name.as_str(),
                    UnlinkatFlags::NoRemoveDir,
                );
            } else {
                outcome = Err(E::UnsafePath);
            }
        }
        let same_dir = fstatat(
            Some(self.root.as_raw_fd()),
            self.name.as_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .is_ok_and(|s| is_dir(&s) && (s.st_dev, s.st_ino) == self.ino);
        if same_dir {
            // Fails (ENOTEMPTY) rather than removing anything it did not create.
            if unlinkat(
                Some(self.root.as_raw_fd()),
                self.name.as_str(),
                UnlinkatFlags::RemoveDir,
            )
            .is_err()
            {
                outcome = Err(E::UnsafePath);
            }
        } else {
            outcome = Err(E::UnsafePath);
        }
        let _ = self.dir.as_fd();
        outcome
    }
}

#[cfg(test)]
#[path = "staging_tests.rs"]
mod tests;
