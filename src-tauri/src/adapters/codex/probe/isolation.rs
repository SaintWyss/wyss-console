//! Unprivileged namespace of the local-tools generation. The argv is authority-fixed and
//! built without a shell: private managed requirements, a private Codex home, the user's
//! Codex state hidden except for one credential file, the authority's own control plane
//! hidden, every project policy layer of the workspace masked, and the process started in
//! an empty directory that exists only in the namespace. Nothing here reads, copies or
//! logs credential content.
use super::{ProbeError, ProbeResult};
use crate::adapters::codex::safety;
use std::{
    env, fs,
    io::Write,
    os::{
        fd::{AsRawFd, OwnedFd},
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
/// The largest executable that is sealed into the namespace (a copy held in memory for the
/// life of the launch). Codex 0.159.0 is 274 MiB; this leaves some room and still refuses a
/// file that is not a program.
const EXECUTABLE_IMAGE_LIMIT: u64 = 320 * 1024 * 1024;
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

/// Host-prepared command and exact, owned grant duplicates. No deserialization or
/// raw-FD constructor: only this trusted isolation module can authorize inheritance.
/// The command is private so argv/grant correspondence cannot be changed afterwards.
pub(crate) struct IsolationLaunch {
    command: Command,
    grants: Vec<OwnedFd>,
}
impl IsolationLaunch {
    pub(crate) fn pipe_stdio(&mut self) {
        self.command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
    }

    pub(crate) fn into_parts(self) -> (Command, Vec<OwnedFd>) {
        (self.command, self.grants)
    }

    #[cfg(test)]
    pub(crate) fn fixture_exec_failure(&mut self) {
        unsafe {
            self.command
                .pre_exec(|| Err(std::io::Error::from_raw_os_error(nix::libc::EIO)));
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture_env(&mut self, key: &str, value: &std::ffi::OsStr) {
        self.command.env(key, value);
    }
    #[cfg(test)]
    pub(crate) fn fixture(command: Command, grants: Vec<OwnedFd>) -> Self {
        Self { command, grants }
    }
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
    ///
    /// `cwd` is where the process inside the namespace starts: the authority's own
    /// directory for that. The launcher is told to go there after it has built the
    /// namespace; where the launcher itself is started decides nothing, because it would
    /// otherwise fall back to the user's home once its own directory is hidden.
    pub(super) fn command(
        &self,
        executable: &Path,
        args: Vec<String>,
        cwd: &Path,
        grants: &Grants<'_>,
    ) -> ProbeResult<Command> {
        let home_dir = env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex"));
        let inherited: Vec<_> = env::vars_os().map(|(key, _)| key).collect();
        self.launch(
            executable,
            args,
            cwd,
            grants,
            &user_home()?,
            home_dir,
            &inherited,
        )
    }
    /// Prerequisite for a future F8 local-tools launcher; does not enable that profile.
    /// Unlike the protocol-1 command, F8 applies its blanket CLOEXEC sweep first.
    #[allow(dead_code)]
    pub(super) fn f8_command(
        &self,
        executable: &Path,
        args: Vec<String>,
        cwd: &Path,
        grants: &Grants<'_>,
    ) -> ProbeResult<IsolationLaunch> {
        let home_dir = env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex"));
        let inherited: Vec<_> = env::vars_os().map(|(key, _)| key).collect();
        let mut held = Vec::new();
        let command = self.launch_inner(
            executable,
            args,
            cwd,
            grants,
            &user_home()?,
            home_dir,
            &inherited,
            Some(&mut held),
            Vec::new(),
            None,
        )?;
        Ok(IsolationLaunch {
            command,
            grants: held,
        })
    }

    /// `command` with the process environment made explicit.
    #[allow(clippy::too_many_arguments)]
    fn launch(
        &self,
        executable: &Path,
        args: Vec<String>,
        cwd: &Path,
        grants: &Grants<'_>,
        user: &Path,
        home_dir: Option<PathBuf>,
        inherited: &[std::ffi::OsString],
    ) -> ProbeResult<Command> {
        self.launch_inner(
            executable,
            args,
            cwd,
            grants,
            user,
            home_dir,
            inherited,
            None,
            Vec::new(),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn launch_inner(
        &self,
        executable: &Path,
        args: Vec<String>,
        cwd: &Path,
        grants: &Grants<'_>,
        user: &Path,
        home_dir: Option<PathBuf>,
        inherited: &[std::ffi::OsString],
        mut f8_grants: Option<&mut Vec<OwnedFd>>,
        f8_mounts: Vec<(OwnedFd, PathBuf)>,
        f8_workspace: Option<(OwnedFd, PathBuf)>,
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
        // The working directory is a private directory of this user reached through no
        // link. It is in no project and under nothing of the user's Codex state, and it
        // holds nothing the generation is given: what is mounted there covers only itself.
        owned_dir(cwd)?;
        let given = [
            Some(self.workspace.as_path()),
            Some(self.state.as_path()),
            Some(executable.as_path()),
            grants.catalog,
        ];
        if fs::symlink_metadata(cwd).map_err(unsafe_path)?.mode() & 0o077 != 0
            || fs::canonicalize(cwd).map_err(unsafe_path)? != cwd
            || self
                .project_dirs()?
                .first()
                .is_some_and(|root| cwd.starts_with(root))
            || hidden.iter().any(|h| cwd.starts_with(h))
            || given.into_iter().flatten().any(|g| g.starts_with(cwd))
        {
            return Err(ProbeError::Protocol);
        }
        let mut command = Command::new(BWRAP);
        if f8_grants.is_some() {
            command.arg("--unshare-pid");
        }
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
        if let Some((fd, destination)) = f8_workspace {
            if destination != self.workspace {
                return Err(ProbeError::Protocol);
            }
            // The source is the retained F8 directory object. The path is only the mount
            // location inside the generation namespace; it is never reopened as a source.
            command
                .arg("--bind-fd")
                .arg(fd.as_raw_fd().to_string())
                .arg(destination);
            f8_grants.as_mut().ok_or(ProbeError::Protocol)?.push(fd);
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
            if let Some(held) = f8_grants.as_mut() {
                held.push(own.into());
            } else {
                unsafe {
                    command.pre_exec(move || {
                        if nix::libc::fcntl(own.as_raw_fd(), nix::libc::F_SETFD, 0) == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
        }
        // Last of the namespace, so nothing mounted before can cover it: an empty private
        // directory that exists only here, sealed so it stays empty, and the place the
        // launcher goes to before it becomes the runtime. The host directory of the same
        // name is never the one the process is in.
        command
            .args(["--perms", "0700", "--tmpfs"])
            .arg(cwd)
            .arg("--remount-ro")
            .arg(cwd)
            .arg("--chdir")
            .arg(cwd);
        for (fd, destination) in f8_mounts {
            // Anonymous sealed images have no pathname for bind-fd's source
            // canonicalization. ro-bind-data reads the FD into a private,
            // readonly-mounted file; the credential still uses bind-fd.
            command
                .args([
                    "--perms",
                    if destination == executable {
                        "0555"
                    } else {
                        "0600"
                    },
                    "--ro-bind-data",
                ])
                .arg(fd.as_raw_fd().to_string())
                .arg(destination);
            f8_grants.as_mut().ok_or(ProbeError::Protocol)?.push(fd);
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

/// Live host evidence for exactly one F8 launch. Never serialized or restored
/// from SQLite: every new generation must prepare a fresh value.
pub(crate) struct F8Attestation {
    isolation: Isolation,
    chain: Vec<PathBuf>,
    masked: Vec<PathBuf>,
    pub(crate) roots: Vec<String>,
    pub(crate) home: PathBuf,
    user: PathBuf,
    credential: Option<Credential>,
    workspace_pin: Option<std::fs::File>,
    identities: Vec<(PathBuf, fs::Metadata)>,
    version_launch: Option<IsolationLaunch>,
}
impl F8Attestation {
    pub(crate) fn has_credential(&self) -> bool {
        self.credential.is_some()
    }

    /// Bounded host-side probe of exactly the sealed image used by the final
    /// command. Run during preparation, never from the authority poll loop.
    pub(crate) fn verify_version(&mut self) -> crate::contracts::control::ControlResult<()> {
        use crate::contracts::control::ControlError as E;
        use crate::f8::ownership::{ContainmentPolicy, Grace, PendingOwnedSpawn, Progress};
        use std::{
            io::Read,
            time::{Duration, Instant},
        };
        let mut launch = self.version_launch.take().ok_or(E::GateDenied)?;
        launch.pipe_stdio();
        let mut pending = PendingOwnedSpawn::spawn_isolated(
            launch,
            ContainmentPolicy::DescendantsContained,
            Grace::default(),
        )?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let (mut child, _) = loop {
            if let Some(result) = pending.poll(Instant::now()) {
                break result?;
            }
            if Instant::now() >= deadline {
                pending.cancel();
                // A deadline requests cleanup; only poll's final result proves ownership ended.
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let observed = (|| -> crate::contracts::control::ControlResult<bool> {
            let mut stdout = child.take_stdout().ok_or(E::TransportError)?;
            let flags = unsafe { nix::libc::fcntl(stdout.as_raw_fd(), nix::libc::F_GETFL) };
            if flags < 0
                || unsafe {
                    nix::libc::fcntl(
                        stdout.as_raw_fd(),
                        nix::libc::F_SETFL,
                        flags | nix::libc::O_NONBLOCK,
                    )
                } < 0
            {
                return Err(E::TransportError);
            }
            let mut bytes = Vec::new();
            let mut successful = false;
            loop {
                let mut chunk = [0u8; 129];
                match stdout.read(&mut chunk) {
                    Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => break,
                }
                if bytes.len() > 128 || Instant::now() >= deadline {
                    break;
                }
                if let Some(status) = child.try_wait()? {
                    successful = status.success();
                    // Drain the final bounded bytes after exit before comparing.
                    if let Ok(n) = stdout.read(&mut chunk) {
                        bytes.extend_from_slice(&chunk[..n]);
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(successful && bytes == b"codex-cli 0.159.0\n")
        })();
        child.begin_stop(Instant::now());
        // Do not transfer this probe to the detached reaper and release runtime capacity.
        // An unkillable probe keeps this one preparation thread, its slot and staging held.
        // The authority's bounded shutdown reports incomplete rather than inventing reap.
        while !matches!(child.poll_stop(Instant::now()), Ok(Progress::Reaped(_))) {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !observed? {
            return Err(E::Unsupported);
        }
        Ok(())
    }
    pub(crate) fn workspace(&self) -> &Path {
        &self.isolation.workspace
    }
    pub(crate) fn holds(&self) -> bool {
        self.isolation
            .masks_hold(&self.chain, &self.masked)
            .unwrap_or(false)
            && self.workspace_pin.as_ref().is_none_or(|pin| {
                let Ok(pinned) = pin.metadata() else {
                    return false;
                };
                fs::symlink_metadata(&self.isolation.workspace).is_ok_and(|current| {
                    pinned.is_dir()
                        && current.is_dir()
                        && !current.file_type().is_symlink()
                        && pinned.dev() == current.dev()
                        && pinned.ino() == current.ino()
                })
            })
            && match &self.credential {
                Some(c) => Isolation::same_credential(c, &self.user).unwrap_or(false),
                None => matches!(credential(&self.user), Ok(None)),
            }
            && self.identities.iter().all(|(p, old)| {
                fs::symlink_metadata(p).is_ok_and(|now| {
                    now.dev() == old.dev()
                        && now.ino() == old.ino()
                        && now.mode() == old.mode()
                        && now.uid() == old.uid()
                        && (now.is_dir()
                            || (now.len() == old.len()
                                && now.mtime() == old.mtime()
                                && now.mtime_nsec() == old.mtime_nsec()
                                && now.ctime() == old.ctime()
                                && now.ctime_nsec() == old.ctime_nsec()))
                })
            })
    }
}
impl Isolation {
    /// Trusted host preparation only. The explicit user directory supports an
    /// entirely synthetic fixture; no environment may select grants or argv.
    pub(crate) fn prepare_f8(
        self,
        executable: &Path,
        user: &Path,
        cwd: &Path,
        catalog: &Path,
    ) -> crate::contracts::control::ControlResult<(IsolationLaunch, F8Attestation)> {
        self.prepare_f8_inner(executable, user, cwd, catalog, None)
    }

    /// Prepare a generation whose workspace source is the F8-retained directory object.
    pub(crate) fn prepare_f8_with_workspace_fd(
        self,
        executable: &Path,
        user: &Path,
        cwd: &Path,
        catalog: &Path,
        workspace_fd: std::fs::File,
    ) -> crate::contracts::control::ControlResult<(IsolationLaunch, F8Attestation)> {
        self.prepare_f8_inner(executable, user, cwd, catalog, Some(workspace_fd))
    }

    fn prepare_f8_inner(
        self,
        executable: &Path,
        user: &Path,
        cwd: &Path,
        catalog: &Path,
        workspace_fd: Option<std::fs::File>,
    ) -> crate::contracts::control::ControlResult<(IsolationLaunch, F8Attestation)> {
        use crate::contracts::control::ControlError as E;
        let workspace_pin = workspace_fd
            .as_ref()
            .map(std::fs::File::try_clone)
            .transpose()
            .map_err(|_| E::StorageUnavailable)?;
        let prepare = || -> ProbeResult<_> {
            let executable = fs::canonicalize(executable).map_err(unsafe_path)?;
            let binary = fs::symlink_metadata(&executable).map_err(unsafe_path)?;
            if !binary.is_file()
                || binary.mode() & 0o022 != 0
                || ![0, own_uid()].contains(&binary.uid())
                || binary.mode() & 0o111 == 0
            {
                return Err(ProbeError::Protocol);
            }
            let hidden_default = env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex"));
            let chain = self.project_dirs()?;
            let masked = self.layers()?;
            let roots = self.trust_roots()?;
            let credential = credential(user)?;
            if let Some(pin) = &workspace_pin {
                let pinned = pin.metadata().map_err(unsafe_path)?;
                let current = fs::symlink_metadata(&self.workspace).map_err(unsafe_path)?;
                if !pinned.is_dir()
                    || !current.is_dir()
                    || current.file_type().is_symlink()
                    || pinned.dev() != current.dev()
                    || pinned.ino() != current.ino()
                {
                    return Err(ProbeError::Protocol);
                }
            }
            self.prepare()?;
            let mut mounts = Vec::new();
            for (path, bound, executable) in [
                (executable.clone(), EXECUTABLE_IMAGE_LIMIT, true),
                (catalog.to_path_buf(), 4 * 1024 * 1024, false),
                (self.policy().join("requirements.toml"), 65536, false),
                (self.policy().join("managed_config.toml"), 65536, false),
            ] {
                let image = sealed_file(&path, bound, executable)?;
                if path == catalog {
                    validate_catalog_image(&image)?;
                }
                for (name, expected) in [
                    ("requirements.toml", safety::LOCAL_TOOLS_REQUIREMENTS),
                    ("managed_config.toml", safety::LOCAL_TOOLS_MANAGED_CONFIG),
                ] {
                    if path == self.policy().join(name)
                        && fs::read(format!("/proc/self/fd/{}", image.as_raw_fd()))
                            .map_err(unsafe_path)?
                            != expected.as_bytes()
                    {
                        return Err(ProbeError::Protocol);
                    }
                }
                mounts.push((image, path));
            }
            let probe_mounts = mounts
                .iter()
                .map(|(fd, path)| {
                    // Reopen our own sealed inode for an independent read offset;
                    // duplicate FDs would share the offset consumed by bwrap.
                    let file = fs::OpenOptions::new()
                        .read(true)
                        .custom_flags(nix::libc::O_CLOEXEC)
                        .open(format!("/proc/self/fd/{}", fd.as_raw_fd()))
                        .map_err(unsafe_path)?;
                    Ok((file.into(), path.clone()))
                })
                .collect::<ProbeResult<Vec<_>>>()?;
            let mut probe_grants = Vec::new();
            let mut probe = self.launch_inner(
                &executable,
                vec!["--version".into()],
                cwd,
                &Grants {
                    masked: &masked,
                    catalog: Some(catalog),
                    credential: None,
                },
                user,
                hidden_default.clone(),
                &[],
                Some(&mut probe_grants),
                probe_mounts,
                None,
            )?;
            probe
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LANG", "C.UTF-8")
                .env("CODEX_HOME", self.home());
            let version_launch = IsolationLaunch {
                command: probe,
                grants: probe_grants,
            };
            let mut held = Vec::new();
            let inherited: Vec<_> = env::vars_os().map(|(key, _)| key).collect();
            let mut command = self.launch_inner(
                &executable,
                safety::local_tools_args(&std::collections::BTreeSet::new(), Some(catalog))
                    .map_err(unsafe_path)?,
                cwd,
                &Grants {
                    masked: &masked,
                    catalog: Some(catalog),
                    credential: credential.as_ref(),
                },
                user,
                hidden_default.clone(),
                &inherited,
                Some(&mut held),
                mounts,
                workspace_fd.map(|fd| (fd.into(), self.workspace.clone())),
            )?;
            // The F8 profile starts with a fixed environment. In particular,
            // loader, shell and provider overrides never enter this generation.
            command
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LANG", "C.UTF-8")
                .env("CODEX_HOME", self.home());
            let mut identities = Vec::new();
            for p in [
                &executable,
                catalog,
                &self.workspace,
                &self.state,
                &self.home(),
                &self.policy().join("requirements.toml"),
                &self.policy().join("managed_config.toml"),
            ] {
                identities.push((
                    p.to_path_buf(),
                    fs::symlink_metadata(p).map_err(unsafe_path)?,
                ));
            }
            Ok((
                IsolationLaunch {
                    command,
                    grants: held,
                },
                chain,
                masked,
                roots,
                credential,
                workspace_pin,
                identities,
                version_launch,
            ))
        };
        let (launch, chain, masked, roots, credential, workspace_pin, identities, version_launch) =
            prepare().map_err(|_| E::GateDenied)?;
        let home = self.home();
        Ok((
            launch,
            F8Attestation {
                isolation: self,
                chain,
                masked,
                roots,
                home,
                user: user.to_path_buf(),
                credential,
                workspace_pin,
                identities,
                version_launch: Some(version_launch),
            },
        ))
    }
}

/// Only executable/catalog/policy bytes are copied, never credential contents.
/// A sealed anonymous inode prevents both pathname swaps and in-place mutation
/// after host preparation, including while the spawn coordinator is queued.
fn sealed_file(path: &Path, limit: u64, executable: bool) -> ProbeResult<OwnedFd> {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    let source = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(unsafe_path)?;
    let metadata = source.metadata().map_err(unsafe_path)?;
    if !metadata.is_file()
        || metadata.len() > limit
        || metadata.mode() & 0o022 != 0
        || ![0, own_uid()].contains(&metadata.uid())
    {
        return Err(ProbeError::Protocol);
    }
    let fd = unsafe {
        nix::libc::memfd_create(
            c"wyss-f8-image".as_ptr(),
            nix::libc::MFD_CLOEXEC | nix::libc::MFD_ALLOW_SEALING,
        )
    };
    if fd < 0 {
        return Err(ProbeError::Protocol);
    }
    let mut image = fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    let copied = std::io::copy(&mut (&source).take(limit + 1), &mut image).map_err(unsafe_path)?;
    let after = source.metadata().map_err(unsafe_path)?;
    if after.ctime() != metadata.ctime()
        || after.ctime_nsec() != metadata.ctime_nsec()
        || after.mtime() != metadata.mtime()
        || after.mtime_nsec() != metadata.mtime_nsec()
        || copied != metadata.len()
        || copied > limit
    {
        return Err(ProbeError::Protocol);
    }
    if unsafe { nix::libc::fchmod(fd, if executable { 0o555 } else { 0o400 }) } != 0
        || unsafe {
            nix::libc::fcntl(
                fd,
                nix::libc::F_ADD_SEALS,
                nix::libc::F_SEAL_WRITE
                    | nix::libc::F_SEAL_GROW
                    | nix::libc::F_SEAL_SHRINK
                    | nix::libc::F_SEAL_SEAL,
            )
        } != 0
    {
        return Err(ProbeError::Protocol);
    }
    if unsafe { nix::libc::lseek(fd, 0, nix::libc::SEEK_SET) } != 0 {
        return Err(ProbeError::Protocol);
    }
    Ok(image.into())
}

fn validate_catalog_image(image: &OwnedFd) -> ProbeResult<()> {
    // Validate the exact immutable bytes that bwrap will read, not a pathname
    // that can be replaced between validation and snapshot preparation.
    let path = format!("/proc/self/fd/{}", image.as_raw_fd());
    if fs::metadata(&path).map_err(unsafe_path)?.len() > 4 * 1024 * 1024 {
        return Err(ProbeError::Protocol);
    }
    let raw = fs::read(path).map_err(unsafe_path)?;
    let clamped = safety::local_tools_catalog(&raw).map_err(unsafe_path)?;
    if serde_json::from_slice::<serde_json::Value>(&raw).map_err(unsafe_path)?
        != serde_json::from_slice::<serde_json::Value>(&clamped).map_err(unsafe_path)?
    {
        return Err(ProbeError::Protocol);
    }
    Ok(())
}
