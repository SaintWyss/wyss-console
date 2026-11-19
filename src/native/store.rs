//! The Console's own small files for native sessions: private directory, atomic writes,
//! bounded reads that do not follow a link.
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub(super) enum AtomicWriteError {
    BeforeRename(io::Error),
    AfterRename(io::Error),
}
impl AtomicWriteError {
    fn into_io(self) -> io::Error {
        match self {
            Self::BeforeRename(error) | Self::AfterRename(error) => error,
        }
    }
}

/// Makes `path` (and what is missing above it) a directory only the owner can enter.
pub fn private_dir(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() {
        return Err(io::Error::other("not a directory"));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes `bytes` to a new file beside `path` (mode 0600), flushes it, renames it over `path`,
/// and synchronizes the containing directory. A successful return means both file contents
/// and the rename are behind a durability barrier; a reader sees old or new, never a partial.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_staged(path, bytes).map_err(AtomicWriteError::into_io)
}

pub(super) fn write_atomic_staged(path: &Path, bytes: &[u8]) -> Result<(), AtomicWriteError> {
    write_atomic_staged_with(path, bytes, |dir| fs::File::open(dir)?.sync_all())
}

fn write_atomic_staged_with(
    path: &Path,
    bytes: &[u8],
    sync_directory: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), AtomicWriteError> {
    let dir = path
        .parent()
        .ok_or_else(|| AtomicWriteError::BeforeRename(io::Error::other("no parent")))?;
    let tmp = dir.join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&tmp);
        return Err(AtomicWriteError::BeforeRename(error));
    }
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(AtomicWriteError::BeforeRename(error));
    }
    sync_directory(dir).map_err(AtomicWriteError::AfterRename)
}

/// The file's bytes, or `None` when it is absent, a link, not a regular file, owned by
/// someone else, or longer than `most`.
pub fn read_bounded(path: &Path, most: u64) -> Option<Vec<u8>> {
    use std::os::unix::fs::MetadataExt;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.uid() != nix::unistd::geteuid().as_raw() || meta.len() > most {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(most).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_replacement_returns_only_after_the_new_name_is_visible() {
        let root = std::env::temp_dir().join(format!(
            "wyss-native-store-{}",
            uuid::Uuid::new_v4().simple()
        ));
        private_dir(&root).unwrap();
        let path = root.join("state.json");
        write_atomic(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(read_bounded(&path, 16).as_deref(), Some(b"new".as_slice()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_sync_failure_is_reported_after_visible_atomic_replacement() {
        let root = std::env::temp_dir().join(format!(
            "wyss-native-store-stage-{}",
            uuid::Uuid::new_v4().simple()
        ));
        private_dir(&root).unwrap();
        let path = root.join("state.json");
        write_atomic(&path, b"old").unwrap();

        let result = write_atomic_staged_with(&path, b"new", |_| {
            Err(io::Error::other("synthetic directory fsync failure"))
        });
        assert!(matches!(result, Err(AtomicWriteError::AfterRename(_))));
        assert_eq!(read_bounded(&path, 16).as_deref(), Some(b"new".as_slice()));
        fs::remove_dir_all(root).unwrap();
    }
}
