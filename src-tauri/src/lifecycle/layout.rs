//! Where the managed installation lives. Two files, under the user's XDG directories.
use super::{LifecycleError as E, LifecycleResult};
use crate::control::paths::{ancestors, create_private, owned_dir};
use std::{
    env,
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

/// The one unit this product owns in the user's service manager.
pub const UNIT_NAME: &str = "wyss-control.service";
const PRODUCT: &str = "wyss-console";
const BINARY: &str = "wyss-control";

/// The directories of this user the lifecycle works in. Nothing is created by naming them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `$XDG_DATA_HOME`.
    data: PathBuf,
    /// `$XDG_CONFIG_HOME`.
    config: PathBuf,
    /// `$XDG_RUNTIME_DIR`: where the service manager and the control socket are reached.
    runtime: PathBuf,
}

/// An absolute path with no `..` and nothing a unit file could not name.
fn plain(path: PathBuf) -> LifecycleResult<PathBuf> {
    let text = path.to_str().ok_or(E::UnsafePath)?;
    if !path.is_absolute()
        || path.components().any(|c| matches!(c, Component::ParentDir))
        || text.chars().any(char::is_control)
    {
        return Err(E::UnsafePath);
    }
    Ok(path)
}
fn home_or(variable: &str, home: Option<&OsString>, below: &str) -> LifecycleResult<PathBuf> {
    match env::var_os(variable).filter(|value| !value.is_empty()) {
        Some(value) => plain(PathBuf::from(value)),
        None => plain(PathBuf::from(home.ok_or(E::UnsafePath)?).join(below)),
    }
}

impl Layout {
    /// The XDG directories of the process, with the defaults the specification gives them.
    pub fn from_environment() -> LifecycleResult<Self> {
        let home = env::var_os("HOME").filter(|value| !value.is_empty());
        Self::new(
            home_or("XDG_DATA_HOME", home.as_ref(), ".local/share")?,
            home_or("XDG_CONFIG_HOME", home.as_ref(), ".config")?,
            plain(PathBuf::from(
                env::var_os("XDG_RUNTIME_DIR").ok_or(E::UnsafePath)?,
            ))?,
        )
    }
    pub fn new(data: PathBuf, config: PathBuf, runtime: PathBuf) -> LifecycleResult<Self> {
        Ok(Self {
            data: plain(data)?,
            config: plain(config)?,
            runtime: plain(runtime)?,
        })
    }
    pub fn runtime(&self) -> &Path {
        &self.runtime
    }
    /// The directory the managed authority is installed in. Private to this user.
    pub fn binary_dir(&self) -> PathBuf {
        self.data.join(PRODUCT).join("authority")
    }
    /// The one authority binary this desktop manages. A fixed path: the unit names it.
    pub fn binary(&self) -> PathBuf {
        self.binary_dir().join(BINARY)
    }
    /// Where the user's service manager reads this user's units from.
    pub fn unit_dir(&self) -> PathBuf {
        self.config.join("systemd").join("user")
    }
    pub fn unit(&self) -> PathBuf {
        self.unit_dir().join(UNIT_NAME)
    }
    /// The control socket an authority of this runtime directory listens on.
    pub fn socket(&self) -> PathBuf {
        self.runtime.join(PRODUCT).join("control.sock")
    }

    /// The directory of the managed binary, created private where it is missing. Every
    /// directory on the way is this user's or root's, is no link, and is writable by
    /// nobody else; the last two are 0700.
    pub(super) fn prepare_binary_dir(&self) -> LifecycleResult<PathBuf> {
        descend(&self.data, &[PRODUCT, "authority"], true)
    }
    /// The same for the unit directory, which other programs of the user share: it is
    /// required to be this user's and safe, not private.
    pub(super) fn prepare_unit_dir(&self) -> LifecycleResult<PathBuf> {
        descend(&self.config, &["systemd", "user"], false)
    }
}

/// `root` and then each of `below`, created 0700 where missing and checked either way.
pub(crate) fn descend(root: &Path, below: &[&str], private: bool) -> LifecycleResult<PathBuf> {
    // The XDG root itself may be missing on a clean account, and so may `~/.local`.
    let mut missing = Vec::new();
    let mut existing = root;
    while !existing.exists() {
        missing.push(existing);
        existing = existing.parent().ok_or(E::UnsafePath)?;
    }
    ancestors(existing).map_err(|_| E::UnsafePath)?;
    for path in missing.into_iter().rev() {
        create_private(path).map_err(|_| E::UnsafePath)?;
    }
    owned_dir(root, false).map_err(|_| E::UnsafePath)?;
    let mut path = root.to_owned();
    for name in below {
        path.push(name);
        if path.exists() {
            owned_dir(&path, private).map_err(|_| E::UnsafePath)?;
        } else {
            create_private(&path).map_err(|_| E::UnsafePath)?;
        }
    }
    Ok(path)
}
