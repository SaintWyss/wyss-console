use crate::contracts::control::{ControlError as E, ControlResult};
use nix::{
    fcntl::{Flock, FlockArg},
    unistd::geteuid,
};
use std::{
    env,
    fs::{self, DirBuilder, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub(crate) struct Paths {
    pub runtime: PathBuf,
    pub state: PathBuf,
}
fn err(_: impl std::fmt::Debug) -> E {
    E::UnsafePath
}
/// No symlink component and no ancestor writable by another UID, except root-owned
/// sticky temporary parents used by isolated tests. Final XDG root remains owned/private.
fn ancestors(path: &Path) -> ControlResult<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(E::UnsafePath);
    }
    for p in path.ancestors() {
        let m = fs::symlink_metadata(p).map_err(err)?;
        if !m.is_dir()
            || m.file_type().is_symlink()
            || (m.uid() != 0 && m.uid() != geteuid().as_raw())
        {
            return Err(E::UnsafePath);
        }
        if m.mode() & 0o022 != 0 && !(m.uid() == 0 && m.mode() & 0o1000 != 0) {
            return Err(E::UnsafePath);
        }
    }
    Ok(())
}
fn owned_dir(path: &Path, private: bool) -> ControlResult<()> {
    ancestors(path)?;
    let m = fs::symlink_metadata(path).map_err(err)?;
    if m.uid() != geteuid().as_raw() || (private && m.mode() & 0o777 != 0o700) {
        return Err(E::UnsafePath);
    }
    Ok(())
}
fn create_private(path: &Path) -> ControlResult<()> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(err(e)),
    }
    owned_dir(path, true)
}
pub(crate) fn runtime_product(root: &Path, create: bool) -> ControlResult<PathBuf> {
    owned_dir(root, true)?;
    let p = root.join("wyss-console");
    if create {
        create_private(&p)?;
    } else {
        owned_dir(&p, true)?;
    }
    Ok(p)
}
impl Paths {
    pub fn from_environment() -> ControlResult<Self> {
        let runtime = env::var_os("XDG_RUNTIME_DIR").ok_or(E::UnsafePath)?;
        owned_dir(Path::new(&runtime), true)?;
        let state = match env::var_os("XDG_STATE_HOME") {
            Some(s) => PathBuf::from(s),
            None => {
                let home = PathBuf::from(env::var_os("HOME").ok_or(E::UnsafePath)?);
                owned_dir(&home, false)?;
                for p in [home.join(".local"), home.join(".local/state")] {
                    if !p.exists() {
                        create_private(&p)?;
                    } else {
                        owned_dir(&p, false)?;
                    }
                }
                home.join(".local/state")
            }
        };
        Self::new(Path::new(&runtime), &state)
    }
    pub fn new(runtime: &Path, state: &Path) -> ControlResult<Self> {
        let runtime = runtime_product(runtime, true)?;
        // XDG_STATE_HOME itself may be a normal 0755 user directory; the product is private.
        if !state.exists() {
            let parent = state.parent().ok_or(E::UnsafePath)?;
            ancestors(parent)?;
            create_private(state)?;
        }
        owned_dir(state, false)?;
        let state = state.join("wyss-console");
        create_private(&state)?;
        Ok(Self { runtime, state })
    }
    pub fn lock(&self) -> ControlResult<(Flock<File>, Flock<File>)> {
        let runtime = lock_file(&self.runtime.join("control.lock"))?;
        let state = lock_file(&self.state.join("control.lock"))?;
        Ok((runtime, state))
    }
}
/// A private directory of this user, opened as itself: no link is followed to reach it,
/// and what is checked last is the descriptor that is returned.
pub(crate) fn private_dir(path: &Path) -> ControlResult<File> {
    owned_dir(path, true)?;
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(err)?;
    let m = dir.metadata().map_err(err)?;
    if !m.is_dir() || m.uid() != geteuid().as_raw() || m.mode() & 0o777 != 0o700 {
        return Err(E::UnsafePath);
    }
    Ok(dir)
}
pub(crate) fn validate_file(path: &Path) -> ControlResult<()> {
    let m = fs::symlink_metadata(path).map_err(err)?;
    if !m.is_file()
        || m.file_type().is_symlink()
        || m.uid() != geteuid().as_raw()
        || m.mode() & 0o777 != 0o600
        || m.nlink() != 1
    {
        return Err(E::UnsafePath);
    }
    Ok(())
}
pub(crate) fn private_file(path: &Path) -> ControlResult<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(err)?;
    validate_file(path)?;
    Ok(f)
}
fn lock_file(path: &Path) -> ControlResult<Flock<File>> {
    Flock::lock(private_file(path)?, FlockArg::LockExclusiveNonblock).map_err(|_| E::AlreadyRunning)
}
