use nix::unistd::geteuid;
use std::{fs::{self, DirBuilder}, os::unix::fs::{DirBuilderExt, MetadataExt}, path::{Path, PathBuf}};
use super::store::StoreError;

/// No symlink component and no ancestor writable by another UID, except root-owned
/// sticky temporary parents used by isolated tests. Final XDG root remains owned/private.
pub(crate) fn ancestors(path: &Path) -> Result<(), StoreError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(StoreError::Unsafe);
    }
    for p in path.ancestors() {
        let m = fs::symlink_metadata(p).map_err(|_| StoreError::Unsafe)?;
        if !m.is_dir()
            || m.file_type().is_symlink()
            || (m.uid() != 0 && m.uid() != geteuid().as_raw())
        {
            return Err(StoreError::Unsafe);
        }
        if m.mode() & 0o022 != 0 && !(m.uid() == 0 && m.mode() & 0o1000 != 0) {
            return Err(StoreError::Unsafe);
        }
    }
    Ok(())
}
pub(crate) fn owned_dir(path: &Path, private: bool) -> Result<(), StoreError> {
    ancestors(path)?;
    let m = fs::symlink_metadata(path).map_err(|_| StoreError::Unsafe)?;
    if m.uid() != geteuid().as_raw() || (private && m.mode() & 0o777 != 0o700) {
        return Err(StoreError::Unsafe);
    }
    Ok(())
}
pub(crate) fn create_private(path: &Path) -> Result<(), StoreError> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(StoreError::Unsafe),
    }
    owned_dir(path, true)
}
/// `root` and then each of `below`, created 0700 where missing and checked either way.
pub(crate) fn descend(root: &Path, below: &[&str], private: bool) -> Result<PathBuf, StoreError> {
    // The XDG root itself may be missing on a clean account, and so may `~/.local`.
    let mut missing = Vec::new();
    let mut existing = root;
    while !existing.exists() {
        missing.push(existing);
        existing = existing.parent().ok_or(StoreError::Unsafe)?;
    }
    ancestors(existing).map_err(|_| StoreError::Unsafe)?;
    for path in missing.into_iter().rev() {
        create_private(path).map_err(|_| StoreError::Unsafe)?;
    }
    owned_dir(root, false).map_err(|_| StoreError::Unsafe)?;
    let mut path = root.to_owned();
    for name in below {
        path.push(name);
        if path.exists() {
            owned_dir(&path, private).map_err(|_| StoreError::Unsafe)?;
        } else {
            create_private(&path).map_err(|_| StoreError::Unsafe)?;
        }
    }
    Ok(path)
}
