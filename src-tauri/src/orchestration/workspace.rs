//! Bounded, read-only workspace identity and revision snapshots.
//! Only digests cross the journal boundary; path and file bytes remain transient.
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::{self, OpenOptions, ReadDir},
    io::Read,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_ENTRIES: usize = 20_000;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_DURATION: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    InvalidRoot,
    TooLarge,
    ChangedDuringRead,
    Io,
}

pub fn workspace_identity(root: &Path) -> Result<String, SnapshotError> {
    let canonical = fs::canonicalize(root).map_err(|_| SnapshotError::InvalidRoot)?;
    let metadata = fs::symlink_metadata(&canonical).map_err(|_| SnapshotError::InvalidRoot)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SnapshotError::InvalidRoot);
    }
    Ok(hex(&Sha256::digest(canonical.as_os_str().as_bytes())))
}

/// Hashes regular files, directory names and modes, and symlink targets without following
/// links. `.git` metadata is excluded because it is control metadata outside the workspace.
/// A filesystem race or a workspace exceeding the fixed budget fails closed.
pub fn workspace_revision(root: &Path) -> Result<String, SnapshotError> {
    let root = fs::canonicalize(root).map_err(|_| SnapshotError::InvalidRoot)?;
    let metadata = fs::symlink_metadata(&root).map_err(|_| SnapshotError::InvalidRoot)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SnapshotError::InvalidRoot);
    }
    let deadline = Instant::now() + MAX_DURATION;
    let mut digest = Sha256::new();
    let mut stack = vec![(root.clone(), PathBuf::new(), 0usize)];
    let mut entries_seen = 0usize;
    let mut bytes_seen = 0u64;
    while let Some((directory, relative, depth)) = stack.pop() {
        if depth > MAX_DEPTH || Instant::now() > deadline {
            return Err(SnapshotError::TooLarge);
        }
        let entries = sorted_entries(fs::read_dir(&directory).map_err(|_| SnapshotError::Io)?)?;
        let before = fs::symlink_metadata(&directory).map_err(|_| SnapshotError::Io)?;
        if !before.is_dir() || before.file_type().is_symlink() {
            return Err(SnapshotError::ChangedDuringRead);
        }
        for entry in entries {
            entries_seen += 1;
            if entries_seen > MAX_ENTRIES || Instant::now() > deadline {
                return Err(SnapshotError::TooLarge);
            }
            let name = entry.file_name();
            let path = entry.path();
            let child_relative = relative.join(&name);
            let meta = fs::symlink_metadata(&path).map_err(|_| SnapshotError::Io)?;
            if name == OsStr::new(".git") && meta.is_dir() {
                continue;
            }
            hash_field(&mut digest, child_relative.as_os_str().as_bytes());
            digest.update((meta.mode() & 0o7777).to_le_bytes());
            if meta.is_dir() {
                digest.update(*b"d");
                stack.push((path, child_relative, depth + 1));
            } else if meta.file_type().is_symlink() {
                digest.update(*b"l");
                let target = fs::read_link(&path).map_err(|_| SnapshotError::Io)?;
                hash_field(&mut digest, target.as_os_str().as_bytes());
            } else if meta.is_file() {
                digest.update(*b"f");
                if meta.len() > MAX_BYTES.saturating_sub(bytes_seen) {
                    return Err(SnapshotError::TooLarge);
                }
                let mut file = OpenOptions::new()
                    .read(true)
                    .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
                    .open(&path)
                    .map_err(|_| SnapshotError::ChangedDuringRead)?;
                let opened = file.metadata().map_err(|_| SnapshotError::Io)?;
                if !opened.is_file() || !same_file(&meta, &opened) {
                    return Err(SnapshotError::ChangedDuringRead);
                }
                let mut buffer = [0u8; 64 * 1024];
                let mut size = 0u64;
                loop {
                    if Instant::now() > deadline {
                        return Err(SnapshotError::TooLarge);
                    }
                    let read = file.read(&mut buffer).map_err(|_| SnapshotError::Io)?;
                    if read == 0 {
                        break;
                    }
                    size = size.saturating_add(read as u64);
                    bytes_seen = bytes_seen.saturating_add(read as u64);
                    if bytes_seen > MAX_BYTES {
                        return Err(SnapshotError::TooLarge);
                    }
                    digest.update(&buffer[..read]);
                }
                let after = file.metadata().map_err(|_| SnapshotError::Io)?;
                if size != meta.len() || !same_file(&opened, &after) || meta.len() != opened.len() {
                    return Err(SnapshotError::ChangedDuringRead);
                }
                digest.update(size.to_le_bytes());
            } else {
                // Sockets, devices, FIFOs and other special files are not workspace inputs.
                return Err(SnapshotError::InvalidRoot);
            }
        }
        let after = fs::symlink_metadata(&directory).map_err(|_| SnapshotError::Io)?;
        if !same_file(&before, &after) {
            return Err(SnapshotError::ChangedDuringRead);
        }
    }
    Ok(hex(&digest.finalize()))
}

fn sorted_entries(read_dir: ReadDir) -> Result<Vec<fs::DirEntry>, SnapshotError> {
    let mut entries = read_dir
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SnapshotError::Io)?;
    entries.sort_by(|a, b| a.file_name().as_bytes().cmp(b.file_name().as_bytes()));
    Ok(entries)
}

fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.mode() == b.mode()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

fn hash_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    fn root() -> PathBuf {
        let path = std::env::temp_dir().join(format!("wyss-orch-ws-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn identity_is_canonical_and_revision_detects_tracked_and_untracked_content_changes() {
        let root = root();
        fs::write(root.join("tracked"), b"one").unwrap();
        let identity = workspace_identity(&root).unwrap();
        let before = workspace_revision(&root).unwrap();
        fs::write(root.join("tracked"), b"two").unwrap();
        let after = workspace_revision(&root).unwrap();
        assert_ne!(before, after);
        fs::write(root.join("untracked"), b"one").unwrap();
        assert_ne!(after, workspace_revision(&root).unwrap());
        assert_eq!(identity, workspace_identity(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_hashes_symlink_targets_without_following_them_and_ignores_git_metadata() {
        let source_root = root();
        let outside_root = root();
        let outside = outside_root.join("outside");
        fs::write(&outside, b"private target contents").unwrap();
        symlink(&outside, source_root.join("link")).unwrap();
        fs::create_dir(source_root.join(".git")).unwrap();
        fs::write(source_root.join(".git/index"), b"one").unwrap();
        let before = workspace_revision(&source_root).unwrap();
        fs::write(source_root.join(".git/index"), b"two").unwrap();
        assert_eq!(before, workspace_revision(&source_root).unwrap());
        fs::remove_file(source_root.join("link")).unwrap();
        symlink(source_root.join("elsewhere"), source_root.join("link")).unwrap();
        assert_ne!(before, workspace_revision(&source_root).unwrap());
        fs::remove_dir_all(source_root).unwrap();
        fs::remove_dir_all(outside_root).unwrap();
    }
}
