//! The authority a desktop build carries, and how it is told from any other file.
use super::{LifecycleError as E, LifecycleResult};
use crate::control::paths::ancestors;
use nix::unistd::geteuid;
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

/// SHA-256 of an authority binary. What a build is pinned to and what is installed are
/// compared as these and never by name, version text or modification time.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Digest([u8; 32]);
impl Digest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
    /// Sixty-four lower-case hexadecimal digits, as the packaging command writes them.
    pub fn parse(hex: &str) -> Option<Self> {
        let hex = hex.as_bytes();
        if hex.len() != 64 {
            return None;
        }
        let mut digest = [0u8; 32];
        for (byte, pair) in digest.iter_mut().zip(hex.as_chunks::<2>().0) {
            let digit = |c: u8| match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                _ => None,
            };
            *byte = digit(pair[0])? << 4 | digit(pair[1])?;
        }
        Some(Self(digest))
    }
}
impl std::fmt::Debug for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

/// An authority binary is a few megabytes. Nothing larger is read as one.
pub(super) const MAX_BINARY: u64 = 256 * 1024 * 1024;

/// Reads a file that has to be an executable nobody else could have written: a regular
/// file, no link, owned by this user (or by root where `system` allows a packaged file),
/// writable by its owner alone. What is checked is the descriptor that is read.
pub(super) fn read_executable(path: &Path, system: bool) -> LifecycleResult<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| E::UnsafePath)?;
    let meta = file.metadata().map_err(|_| E::UnsafePath)?;
    let owner = meta.uid() == geteuid().as_raw() || (system && meta.uid() == 0);
    if !meta.is_file() || !owner || meta.mode() & 0o022 != 0 || meta.len() > MAX_BINARY {
        return Err(E::UnsafePath);
    }
    // A managed file has one name: a second link would let it change under another.
    if !system && meta.nlink() != 1 {
        return Err(E::UnsafePath);
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    File::take(file, MAX_BINARY + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| E::Io)?;
    if bytes.len() as u64 > MAX_BINARY {
        return Err(E::UnsafePath);
    }
    Ok(bytes)
}

/// The authority this build installs: a file, and the digest the build was pinned to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    payload: PathBuf,
    digest: Digest,
}
impl Package {
    /// A payload at an explicit place. The packaged desktop finds its own with
    /// [`Provision::of_build`]; nothing reaches this from the window.
    pub fn at(payload: PathBuf, digest: Digest) -> Self {
        Self { payload, digest }
    }
    pub fn digest(&self) -> Digest {
        self.digest
    }
    /// The payload's bytes, once it is proved to be the pinned authority. It is read one
    /// time: what was verified is what is installed.
    pub fn verified(&self) -> LifecycleResult<Vec<u8>> {
        let parent = self.payload.parent().ok_or(E::PackageInvalid)?;
        ancestors(parent).map_err(|_| E::PackageInvalid)?;
        let bytes = read_executable(&self.payload, true).map_err(|_| E::PackageInvalid)?;
        if Digest::of(&bytes) != self.digest {
            return Err(E::PackageInvalid);
        }
        Ok(bytes)
    }
}

/// What this build can provide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provision {
    /// A packaged build: it installs and starts the authority it carries.
    Packaged(Package),
    /// A build that was not packaged (a development build): it manages no authority and
    /// uses one that is already running.
    Development,
    /// A build that says it is packaged and whose payload cannot be named.
    Invalid,
}
impl Provision {
    /// The authority packaged beside this executable, if the build was pinned to one.
    ///
    /// The payload is found from the running executable and from nowhere else: not from
    /// `PATH`, not from the working directory, not from a name the window sends. A
    /// package lays the two out as `bin/wyss-console` and
    /// `libexec/wyss-console/wyss-control`.
    pub fn of_build(pin: Option<&str>) -> Self {
        let Some(pin) = pin else {
            return Self::Development;
        };
        let located = (|| {
            let digest = Digest::parse(pin)?;
            // The real file behind `/proc/self/exe`, links already resolved.
            let executable = fs::canonicalize(std::env::current_exe().ok()?).ok()?;
            let prefix = executable.parent()?.parent()?;
            Some(Package::at(
                prefix
                    .join("libexec")
                    .join("wyss-console")
                    .join("wyss-control"),
                digest,
            ))
        })();
        located.map_or(Self::Invalid, Self::Packaged)
    }
}
