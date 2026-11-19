//! Owner-approved accounts that may also write a provider directory. The allowlist lives
//! outside the provider directories and is only writable by the owner. A directory with
//! an additional writer outside the list is treated as read-only.
//!
//! The file is `<state>/configuration/trusted-writers`, one account name a line, `#` for
//! comments. It must be this user's, a regular file, not writable by anybody else; one that
//! is not is ignored whole and nothing is declared.
use nix::unistd::User;
#[cfg(test)]
use nix::unistd::geteuid;
#[cfg(test)]
use std::{
    fs,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

#[cfg(test)]
const MOST_BYTES: u64 = 4096;
#[cfg(test)]
const MOST_NAMES: usize = 16;

#[cfg(test)]
fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b == b'_')
        && name.len() <= 32
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The names in the file, or none when the file is absent or not safe to believe. The
/// folder must be the owner's and closed to others, the file is opened without following a
/// link, and what is judged is what is read: the same descriptor.
#[cfg(test)]
pub fn load(state: &Path) -> Vec<String> {
    let folder = state.join("configuration");
    let me = geteuid().as_raw();
    let Ok(dir) = fs::symlink_metadata(&folder) else {
        return Vec::new();
    };
    if !dir.is_dir() || dir.uid() != me || dir.mode() & 0o022 != 0 {
        return Vec::new();
    }
    let Ok(file) = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
        .open(folder.join("trusted-writers"))
    else {
        return Vec::new();
    };
    let Ok(meta) = file.metadata() else {
        return Vec::new();
    };
    if !meta.is_file() || meta.uid() != me || meta.mode() & 0o022 != 0 || meta.len() > MOST_BYTES {
        return Vec::new();
    }
    let mut text = String::new();
    if file.take(MOST_BYTES).read_to_string(&mut text).is_err() {
        return Vec::new();
    }
    let mut names: Vec<String> = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|name| valid_name(name))
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    names.truncate(MOST_NAMES);
    names
}

/// The uids of those names that exist on this machine; a name that does not is dropped.
pub fn uids(names: &[String]) -> Vec<u32> {
    names
        .iter()
        .filter_map(|name| User::from_name(name).ok().flatten())
        .map(|user| user.uid.as_raw())
        .collect()
}
