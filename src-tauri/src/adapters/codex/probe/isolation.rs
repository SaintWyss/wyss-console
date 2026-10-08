//! Unprivileged namespace of the local-tools generation. The argv is authority-fixed and
//! built without a shell: private managed requirements, a private Codex home, the user's
//! Codex state hidden except for one credential file, the authority's own control plane
//! hidden, and every project policy layer of the workspace masked. Nothing here reads,
//! copies or logs credential content.
use super::{ProbeError, ProbeResult};
use crate::adapters::codex::safety;
use std::{
    env, fs,
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::Command,
};

const BWRAP: &str = "/usr/bin/bwrap";
/// The runtime reads its managed requirements only from here. Inside the namespace `/etc`
/// is a private overlay of the host's, so this mount point never has to exist on the host
/// and nothing of a host `/etc/codex` is visible under it.
const MANAGED: &str = "/etc/codex";
const MAX_DEPTH: usize = 64;
#[cfg(not(test))]
fn managed() -> PathBuf {
    PathBuf::from(MANAGED)
}
#[cfg(test)]
thread_local! {
    /// Lets a test mount the policy where the host has no directory at all.
    static MANAGED_AT: std::cell::RefCell<PathBuf> = std::cell::RefCell::new(PathBuf::from(MANAGED));
}
#[cfg(test)]
fn managed() -> PathBuf {
    MANAGED_AT.with(|at| at.borrow().clone())
}

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
/// bound read-write because the runtime refreshes a token in place. It is held as an open
/// path descriptor of the exact inode that was validated, so what enters the namespace is
/// that inode and never whatever its path names a moment later. No byte of it is read.
pub(crate) struct Credential {
    file: fs::File,
    dev: u64,
    ino: u64,
}
/// A private regular file of this user with exactly one name.
fn private_inode(m: &fs::Metadata) -> bool {
    m.is_file() && m.uid() == own_uid() && m.mode() & 0o077 == 0 && m.nlink() == 1
}
/// Opens the credential of a Codex home without following a link and without read access.
/// The home is resolved once to its canonical directory, which must belong to this user
/// and admit no other writer: nobody else can swap the file under its name. A missing home
/// or file means no capability; an unsafe one (symlink, foreign owner, group or world
/// access, a second hard link) is refused.
fn credential(home: &Path) -> ProbeResult<Option<Credential>> {
    let home = match fs::canonicalize(home) {
        Ok(home) => home,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(unsafe_path(e)),
    };
    let parent = fs::symlink_metadata(&home).map_err(unsafe_path)?;
    if !parent.is_dir() || parent.uid() != own_uid() || parent.mode() & 0o022 != 0 {
        return Err(ProbeError::Protocol);
    }
    let opened = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_PATH | nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(home.join("auth.json"));
    let file = match opened {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(unsafe_path(e)),
    };
    // `try_clone` never returns one of the standard streams, which the child's own stdio
    // replaces: the descriptor number handed to the launcher survives into it.
    let file = file.try_clone().map_err(unsafe_path)?;
    let m = file.metadata().map_err(unsafe_path)?;
    if !private_inode(&m) {
        return Err(ProbeError::Protocol);
    }
    Ok(Some(Credential {
        file,
        dev: m.dev(),
        ino: m.ino(),
    }))
}

pub(crate) struct Isolation {
    /// Canonical workspace this generation is isolated for.
    pub workspace: PathBuf,
    /// Private authority directory holding the generation's Codex home and policy.
    pub state: PathBuf,
    /// The authority's own private directories: control socket, locks and canonical
    /// database. None of it exists inside the namespace.
    pub control: Vec<PathBuf>,
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
    /// Directories that may contribute project policy: from the outermost ancestor that
    /// holds a `.git` marker down to the workspace, or the workspace alone when there is
    /// none. Which marker the runtime takes for the project root is its own business, and
    /// it may change: an interrupted sandbox leaves an empty `.git` in the workspace it was
    /// protecting. The widest reading covers every one of them, so such a marker can never
    /// shorten what is masked.
    pub(super) fn project_dirs(&self) -> ProbeResult<Vec<PathBuf>> {
        if !self.workspace.is_absolute()
            || fs::canonicalize(&self.workspace).map_err(unsafe_path)? != self.workspace
        {
            return Err(ProbeError::Protocol);
        }
        let mut dirs = Vec::new();
        let mut outermost = 1;
        for dir in self.workspace.ancestors() {
            dirs.push(dir.to_path_buf());
            if dirs.len() > MAX_DEPTH {
                return Err(ProbeError::Protocol);
            }
            if fs::symlink_metadata(dir.join(".git")).is_ok() {
                outermost = dirs.len();
            }
        }
        dirs.truncate(outermost);
        dirs.reverse();
        Ok(dirs)
    }
    /// Roots whose trust is forced to `untrusted` on every thread of this generation: the
    /// workspace and every directory above it that could be taken for a project root.
    pub(super) fn trust_roots(&self) -> ProbeResult<Vec<String>> {
        let roots: Vec<PathBuf> = self
            .project_dirs()?
            .into_iter()
            .filter(|dir| *dir == self.workspace || fs::symlink_metadata(dir.join(".git")).is_ok())
            .collect();
        if roots.len() > safety::MAX_TRUST_ROOTS {
            return Err(ProbeError::Protocol);
        }
        roots
            .into_iter()
            .map(|p| p.into_os_string().into_string().map_err(unsafe_path))
            .collect()
    }
    /// Project policy layers of these directories that exist right now. Each must be a
    /// real directory: a file or a symlink named `.codex` cannot be masked safely and
    /// keeps the profile off.
    fn layers_of(dirs: &[PathBuf]) -> ProbeResult<Vec<PathBuf>> {
        let mut layers = Vec::new();
        for dir in dirs {
            let layer = dir.join(".codex");
            match fs::symlink_metadata(&layer) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Ok(m) if m.is_dir() => layers.push(layer),
                _ => return Err(ProbeError::Protocol),
            }
        }
        Ok(layers)
    }
    /// The layers to mask when the namespace is created.
    pub(super) fn layers(&self) -> ProbeResult<Vec<PathBuf>> {
        Self::layers_of(&self.project_dirs()?)
    }
    /// True while no project policy escaped the masks: every layer masked at launch is
    /// still there, and a layer that appeared since holds nothing. `chain` is the project
    /// directory chain the namespace was created for. It stays part of the proof whatever
    /// the tree looks like now, together with every directory a marker that appeared since
    /// launch adds above it. An interrupted sandbox of the runtime leaves empty `.git` and
    /// `.codex` directories in the workspace it protected: an empty layer is no policy,
    /// and anything inside an unmasked layer would be policy this namespace does not hide.
    pub(super) fn masks_hold(&self, chain: &[PathBuf], masked: &[PathBuf]) -> ProbeResult<bool> {
        let mut dirs = chain.to_vec();
        for dir in self.project_dirs()? {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        let current = Self::layers_of(&dirs)?;
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
    /// The credential capability of the user's own Codex home, if there is one.
    pub(super) fn credential(&self) -> ProbeResult<Option<Credential>> {
        credential(&user_home()?)
    }
    /// True while the capability is still the one that was validated: the inode bound into
    /// the namespace is still private and single-named, and the user's credential path
    /// still names that very inode. Its size and times change with every refresh and are
    /// not part of the identity.
    pub(super) fn credential_holds(&self, held: &Credential) -> ProbeResult<bool> {
        Self::same_credential(held, &user_home()?)
    }
    fn same_credential(held: &Credential, home: &Path) -> ProbeResult<bool> {
        let bound = held.file.metadata().map_err(unsafe_path)?;
        if !private_inode(&bound) || bound.dev() != held.dev || bound.ino() != held.ino {
            return Ok(false);
        }
        Ok(matches!(
            credential(home),
            Ok(Some(now)) if now.dev == held.dev && now.ino == held.ino
        ))
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
    /// The complete launch of one local-tools process. The supervisor refuses to work once
    /// the live set of policy layers differs from the masked one.
    pub(super) fn command(
        &self,
        executable: &Path,
        args: Vec<String>,
        grants: &Grants<'_>,
    ) -> ProbeResult<Command> {
        let home_dir = env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex"));
        let inherited: Vec<_> = env::vars_os().map(|(key, _)| key).collect();
        self.launch(
            executable,
            args,
            grants,
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
        grants: &Grants<'_>,
        user: &Path,
        home_dir: Option<PathBuf>,
        inherited: &[std::ffi::OsString],
    ) -> ProbeResult<Command> {
        let bwrap = fs::symlink_metadata(BWRAP).map_err(|_| ProbeError::RuntimeUnavailable)?;
        if !bwrap.is_file() || bwrap.uid() != 0 || bwrap.mode() & 0o022 != 0 {
            return Err(ProbeError::RuntimeUnavailable);
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
        // The control plane is hidden whole. A workspace inside it, or a control directory
        // that is not exactly the private directory the authority owns, cannot be served.
        for dir in &self.control {
            owned_dir(dir)?;
            if fs::canonicalize(dir).map_err(unsafe_path)? != *dir
                || self.workspace.starts_with(dir)
                || hidden.iter().any(|h| dir.starts_with(h))
            {
                return Err(ProbeError::Protocol);
            }
        }
        let executable =
            fs::canonicalize(executable).map_err(|_| ProbeError::RuntimeUnavailable)?;
        let mut command = Command::new(BWRAP);
        command
            .args(["--die-with-parent", "--dev-bind", "/", "/"])
            // A private writable view of `/etc` gives the managed policy its mount point
            // without anything existing, or changing, on the host.
            .args([
                "--overlay-src",
                "/etc",
                "--tmp-overlay",
                "/etc",
                "--ro-bind",
            ])
            .arg(self.policy())
            .arg(managed());
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
        for dir in &self.control {
            command.arg("--tmpfs").arg(dir);
        }
        let within = |path: &Path| self.control.iter().any(|c| path.starts_with(c));
        // Of everything the authority owns, the generation gets back its own home and,
        // read-only, the one catalog file it was launched with.
        if within(&self.home()) {
            command.arg("--bind").arg(self.home()).arg(self.home());
        }
        if let Some(catalog) = grants.catalog.filter(|c| within(c)) {
            command.arg("--ro-bind").arg(catalog).arg(catalog);
        }
        for layer in grants.masked {
            // A layer that is the user's own Codex home is already hidden whole; masking it
            // again would also hide the pinned release that was just brought back.
            if !hidden.contains(layer) {
                command.arg("--ro-bind").arg(self.mask()).arg(layer);
            }
        }
        if let Some(credential) = grants.credential {
            // The launcher binds the descriptor itself, so the inode that enters the
            // namespace is the one that was validated. The command owns its own duplicate:
            // the number in the argv cannot come to name another file before the launch.
            let own = credential.file.try_clone().map_err(unsafe_path)?;
            let fd = own.as_raw_fd();
            command
                .arg("--bind-fd")
                .arg(fd.to_string())
                .arg(self.home().join("auth.json"));
            // SAFETY: after fork, one async-signal-safe syscall on a descriptor this
            // closure keeps open. Only the launcher inherits it; it stays close-on-exec in
            // the authority, so no other child ever receives it.
            unsafe {
                command.pre_exec(move || {
                    if nix::libc::fcntl(own.as_raw_fd(), nix::libc::F_SETFD, 0) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
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
/// What one launch lets into the namespace on top of the fixed profile.
pub(super) struct Grants<'a> {
    /// Project policy layers hidden for the lifetime of the namespace.
    pub masked: &'a [PathBuf],
    /// Authority model catalog of the thread-capable process. Discovery has none.
    pub catalog: Option<&'a Path>,
    /// The validated credential capability, if the user has one.
    pub credential: Option<&'a Credential>,
}
#[cfg(test)]
#[path = "isolation_tests.rs"]
mod tests;
