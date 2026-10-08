//! Unprivileged namespace of the local-tools generation. The argv is authority-fixed and
//! built without a shell: private managed requirements, a private Codex home, the user's
//! Codex state hidden except for one credential file, and every project policy layer of
//! the workspace masked. Nothing here reads, copies or logs credential content.
use super::{ProbeError, ProbeResult};
use crate::adapters::codex::safety;
use std::{
    env, fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
};

const BWRAP: &str = "/usr/bin/bwrap";
/// The runtime reads its managed requirements only from here. The authority never creates
/// this host directory; without it the local-tools profile cannot be proven and stays off.
const MANAGED: &str = "/etc/codex";
const MAX_DEPTH: usize = 64;

fn unsafe_path(_: impl std::fmt::Debug) -> ProbeError {
    ProbeError::Protocol
}
fn own_uid() -> u32 {
    nix::unistd::geteuid().as_raw()
}
/// A real directory owned by this user, never a symlink.
fn owned_dir(path: &Path) -> ProbeResult<()> {
    let m = fs::symlink_metadata(path).map_err(unsafe_path)?;
    if !m.is_dir() || m.uid() != own_uid() {
        return Err(ProbeError::Protocol);
    }
    Ok(())
}
fn private_dir(path: &Path) -> ProbeResult<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(unsafe_path(e)),
    }
    owned_dir(path)?;
    if fs::symlink_metadata(path).map_err(unsafe_path)?.mode() & 0o077 != 0 {
        return Err(ProbeError::Protocol);
    }
    Ok(())
}
fn write_private(path: &Path, content: &[u8]) -> ProbeResult<()> {
    let staging = path.with_extension("tmp");
    let _ = fs::remove_file(&staging);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(&staging)
        .map_err(unsafe_path)?;
    file.write_all(content)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&staging, path))
        .map_err(unsafe_path)
}
/// The user's own Codex home: where their configuration, rules, hooks, MCP servers and
/// credential live. Everything in it except the credential is hidden from the generation.
pub(super) fn user_home() -> ProbeResult<PathBuf> {
    match env::var_os("CODEX_HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home)),
        _ => Ok(PathBuf::from(env::var_os("HOME").ok_or(ProbeError::Protocol)?).join(".codex")),
    }
}
/// The one capability that crosses into the namespace: the user's credential file,
/// bound read-write because the runtime refreshes a token in place. A missing file means no
/// capability; an unsafe one (symlink, foreign owner, group or world access) is refused.
fn credential(home: &Path) -> ProbeResult<Option<PathBuf>> {
    let path = home.join("auth.json");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(unsafe_path(e)),
        Ok(m) if m.is_file() && m.uid() == own_uid() && m.mode() & 0o077 == 0 => Ok(Some(path)),
        Ok(_) => Err(ProbeError::Protocol),
    }
}

pub(crate) struct Isolation {
    /// Canonical workspace this generation is isolated for.
    pub workspace: PathBuf,
    /// Private authority directory holding the generation's Codex home and policy.
    pub state: PathBuf,
}
impl Isolation {
    pub(super) fn home(&self) -> PathBuf {
        self.state.join("home")
    }
    fn policy(&self) -> PathBuf {
        self.state.join("etc")
    }
    fn mask(&self) -> PathBuf {
        self.state.join("empty")
    }
    /// Directories that may contribute project policy, from the project root down to the
    /// workspace. The root is the nearest ancestor holding `.git`, else the workspace alone.
    fn project_dirs(&self) -> ProbeResult<Vec<PathBuf>> {
        if !self.workspace.is_absolute()
            || fs::canonicalize(&self.workspace).map_err(unsafe_path)? != self.workspace
        {
            return Err(ProbeError::Protocol);
        }
        let mut dirs = Vec::new();
        for dir in self.workspace.ancestors() {
            dirs.push(dir.to_path_buf());
            if dirs.len() > MAX_DEPTH {
                return Err(ProbeError::Protocol);
            }
            if fs::symlink_metadata(dir.join(".git")).is_ok() {
                dirs.reverse();
                return Ok(dirs);
            }
        }
        Ok(vec![self.workspace.clone()])
    }
    /// Roots whose trust is forced to `untrusted` on every thread of this generation.
    pub(super) fn trust_roots(&self) -> ProbeResult<Vec<String>> {
        let dirs = self.project_dirs()?;
        let mut roots = vec![dirs[0].clone()];
        if dirs.len() > 1 {
            roots.push(self.workspace.clone());
        }
        roots
            .into_iter()
            .map(|p| p.into_os_string().into_string().map_err(unsafe_path))
            .collect()
    }
    /// Project policy layers that exist right now. Each must be a real directory: a file or
    /// a symlink named `.codex` cannot be masked safely and keeps the profile off.
    pub(super) fn layers(&self) -> ProbeResult<Vec<PathBuf>> {
        let mut layers = Vec::new();
        for dir in self.project_dirs()? {
            let layer = dir.join(".codex");
            match fs::symlink_metadata(&layer) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Ok(m) if m.is_dir() => layers.push(layer),
                _ => return Err(ProbeError::Protocol),
            }
        }
        Ok(layers)
    }
    /// True while no project policy escaped the masks: every layer masked at launch is
    /// still there, and a layer that appeared since holds nothing. The runtime's own
    /// sandbox leaves an empty, read-only `.codex` in a workspace it protects; anything
    /// inside an unmasked layer would be policy this namespace does not hide.
    pub(super) fn masks_hold(&self, masked: &[PathBuf]) -> ProbeResult<bool> {
        let current = self.layers()?;
        if masked.iter().any(|layer| !current.contains(layer)) {
            return Ok(false);
        }
        for layer in current.iter().filter(|layer| !masked.contains(layer)) {
            if fs::read_dir(layer).map_err(unsafe_path)?.next().is_some() {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Creates the private home, the managed policy and the empty mask source.
    fn prepare(&self) -> ProbeResult<()> {
        private_dir(&self.state)?;
        for dir in [self.home(), self.policy(), self.mask()] {
            private_dir(&dir)?;
        }
        if fs::read_dir(self.mask())
            .map_err(unsafe_path)?
            .next()
            .is_some()
        {
            return Err(ProbeError::Protocol);
        }
        write_private(
            &self.policy().join("requirements.toml"),
            safety::LOCAL_TOOLS_REQUIREMENTS.as_bytes(),
        )?;
        write_private(
            &self.policy().join("managed_config.toml"),
            safety::LOCAL_TOOLS_MANAGED_CONFIG.as_bytes(),
        )?;
        let mut entries: Vec<_> = fs::read_dir(self.policy())
            .map_err(unsafe_path)?
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        entries.sort();
        if entries != ["managed_config.toml", "requirements.toml"] {
            return Err(ProbeError::Protocol);
        }
        // Mount point of the credential bind. It stays empty: writes go to the bound file.
        let placeholder = self.home().join("auth.json");
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&placeholder)
        {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let m = fs::symlink_metadata(&placeholder).map_err(unsafe_path)?;
                if m.is_file() && m.uid() == own_uid() && m.len() == 0 {
                    Ok(())
                } else {
                    Err(ProbeError::Protocol)
                }
            }
            Err(e) => Err(unsafe_path(e)),
        }
    }
    /// The complete launch of one local-tools process. `masked` is the set of policy layers
    /// hidden for the lifetime of the namespace; the supervisor refuses to work once the
    /// live set differs from it.
    pub(super) fn command(
        &self,
        executable: &Path,
        args: Vec<String>,
        masked: &[PathBuf],
    ) -> ProbeResult<Command> {
        let home_dir = env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex"));
        let inherited: Vec<_> = env::vars_os().map(|(key, _)| key).collect();
        self.launch(
            executable,
            args,
            masked,
            &user_home()?,
            home_dir,
            &inherited,
        )
    }
    /// `command` with the process environment made explicit.
    fn launch(
        &self,
        executable: &Path,
        args: Vec<String>,
        masked: &[PathBuf],
        user: &Path,
        home_dir: Option<PathBuf>,
        inherited: &[std::ffi::OsString],
    ) -> ProbeResult<Command> {
        let bwrap = fs::symlink_metadata(BWRAP).map_err(|_| ProbeError::RuntimeUnavailable)?;
        if !bwrap.is_file() || bwrap.uid() != 0 || bwrap.mode() & 0o022 != 0 {
            return Err(ProbeError::RuntimeUnavailable);
        }
        if !fs::symlink_metadata(MANAGED).is_ok_and(|m| m.is_dir()) {
            return Err(ProbeError::Protocol);
        }
        self.prepare()?;
        let mut hidden: Vec<PathBuf> = Vec::new();
        for dir in [Some(user.to_path_buf()), home_dir].into_iter().flatten() {
            let Ok(dir) = fs::canonicalize(&dir) else {
                continue;
            };
            if dir.is_dir() && !hidden.contains(&dir) {
                hidden.push(dir);
            }
        }
        // Hiding the user's Codex state must not hide what the generation itself needs.
        if hidden
            .iter()
            .any(|h| self.state.starts_with(h) || self.workspace.starts_with(h))
        {
            return Err(ProbeError::Protocol);
        }
        let executable =
            fs::canonicalize(executable).map_err(|_| ProbeError::RuntimeUnavailable)?;
        let mut command = Command::new(BWRAP);
        command
            .args(["--die-with-parent", "--dev-bind", "/", "/", "--ro-bind"])
            .arg(self.policy())
            .arg(MANAGED);
        for dir in &hidden {
            command.arg("--tmpfs").arg(dir);
        }
        if hidden.iter().any(|h| executable.starts_with(h)) {
            // Only the release that contains the pinned executable comes back, read-only.
            let bin = executable.parent().ok_or(ProbeError::RuntimeUnavailable)?;
            let release = if bin.file_name().is_some_and(|n| n == "bin") {
                bin.parent().ok_or(ProbeError::RuntimeUnavailable)?
            } else {
                bin
            };
            if hidden.iter().any(|h| h == release) {
                return Err(ProbeError::Protocol);
            }
            command.arg("--ro-bind").arg(release).arg(release);
        }
        for layer in masked {
            command.arg("--ro-bind").arg(self.mask()).arg(layer);
        }
        if let Some(credential) = credential(user)? {
            command
                .arg("--bind")
                .arg(credential)
                .arg(self.home().join("auth.json"));
        }
        command.arg("--").arg(&executable).args(args);
        // The generation resolves everything from its private home; no inherited override.
        for key in inherited {
            if key
                .to_str()
                .is_none_or(|k| k.starts_with("CODEX_") || k.starts_with("OPENAI"))
            {
                command.env_remove(key);
            }
        }
        command.env("CODEX_HOME", self.home());
        Ok(command)
    }
}
#[cfg(test)]
#[path = "isolation_tests.rs"]
mod tests;
