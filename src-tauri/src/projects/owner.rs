//! What the owners say, and the few effects they can be asked for (contract §1, §3).
//!
//! `Owner` is the only door from the M1 store to the host, the filesystem and the Managed
//! authorities. Reading it never starts, repairs, restarts or falls back to anything: a host
//! that is absent is `HostOffline`, one that is not the host that was observed is
//! `HostChanged`, and neither is retried. Its effects are explicit launches the user asked
//! for; nothing here is reached by selecting a project.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs, io,
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    time::Duration,
};
use wyss_console::contracts::m1::{
    MAX_EPOCH_BYTES, MAX_PATH_BYTES,
    records::{CheckoutKind, ManagedProtocol, ManagedSafetyProfile},
};

/// Longest wait for the host to answer a read or a launch.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerError {
    /// No host answers (or none could be checked). Nothing was sent.
    HostOffline,
    /// The peer is not the host that was observed, or cannot be verified. Nothing was sent.
    HostChanged,
    /// The owner does not have the capability, or the provider is not implemented for it.
    Unsupported,
    /// A Native profile the owner no longer has at that version.
    ProfileUnavailable,
    /// The owner refused the request before doing anything.
    Refused,
}

/// How a launch failed: before anything could have been dispatched, or after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
    NotDispatched(OwnerError),
    /// The request may have reached the owner. It is never retried automatically.
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRow {
    pub terminal_id: String,
    pub running: bool,
    /// Where its shell started, as the host reports it.
    pub cwd: Option<String>,
    /// The Native session this terminal runs, when it runs one.
    pub native_session: Option<String>,
    /// The shell process the host observed for it.
    pub pid: Option<u32>,
}

/// A read of the host through one checked connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostView {
    /// An opaque observation of which host this is: never persisted as authority.
    pub epoch: String,
    pub features: Vec<String>,
    pub terminals: Vec<TerminalRow>,
}

/// A Managed session as its authority holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRow {
    pub protocol: ManagedProtocol,
    pub store_stream_id: String,
    pub session_id: String,
    /// The profile the authority attached the session in, when it is attached. Unknown is
    /// `None`, never a default.
    pub effective_profile: Option<ManagedSafetyProfile>,
    pub attached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootInfo {
    /// The canonical path of the folder, without a link in its last component.
    pub path: String,
    pub device: u64,
    pub inode: u64,
    pub kind: CheckoutKind,
    /// Whether Native sessions may start here.
    pub native_eligible: bool,
    /// Whether the Console may write agent configuration here. The home folder never is.
    pub config_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootError {
    Missing,
    /// A link, not a directory, or not the user's.
    Ineligible,
    /// Another account can use the folder (an ACL or the mode bits), or that could not be told.
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderError {
    /// The name is not one plain basename.
    Name,
    Exists,
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLaunch {
    pub agent: Agent,
    pub path: String,
    pub profile: Option<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLaunched {
    pub session_id: String,
    pub terminal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLaunched {
    pub terminal_id: String,
}

/// The one Managed authority a request is for: its protocol and the identity of its durable
/// stream. A request is only ever made of the authority it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityRef {
    pub protocol: ManagedProtocol,
    pub store_stream_id: String,
}

/// What a Managed authority's own status says. Nothing else is evidence of what it can do: the
/// protocol has no list of supported profiles, and a window's state grants nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityFacts {
    pub protocol_version: u8,
    pub storage_schema_version: u8,
    pub service_version: String,
    pub stream_id: String,
}

/// The authority a Managed launch would go to and the profiles it has shown it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedAdvert {
    pub authority: AuthorityRef,
    pub profiles: Vec<ManagedSafetyProfile>,
}

/// The profiles `facts` support, for the interface of `protocol`; none when the authority is
/// not the one that interface speaks to or cannot be verified as such.
///
/// Inference needs the greeting of that interface's own protocol and storage schema. Local Tools
/// is the privileged profile, so it also needs the authority to be the very build of this
/// desktop, the build whose safety-profile handling is the one that was verified (the same
/// rule protocol 2's own client applies to everything). An older build of the protocol-1
/// authority is offered Inference only; any doubt offers nothing.
pub fn advertised_profiles(
    protocol: ManagedProtocol,
    facts: &AuthorityFacts,
    own_version: &str,
) -> Vec<ManagedSafetyProfile> {
    let (wire, schema) = match protocol {
        ManagedProtocol::P1 => (1, 7),
        ManagedProtocol::P2 => (2, 8),
    };
    if facts.protocol_version != wire || facts.storage_schema_version != schema {
        return Vec::new();
    }
    let same_build = !facts.service_version.is_empty() && facts.service_version == own_version;
    match (protocol, same_build) {
        (_, true) => vec![
            ManagedSafetyProfile::Inference,
            ManagedSafetyProfile::LocalTools,
        ],
        (ManagedProtocol::P1, false) => vec![ManagedSafetyProfile::Inference],
        (ManagedProtocol::P2, false) => Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedLaunch {
    pub agent: Agent,
    /// The workspace root the session is for; a Managed session binds it later, explicitly.
    pub path: String,
    pub requested: ManagedSafetyProfile,
    /// Names the whole user action: the same value sent again is the same action.
    pub intent: String,
    /// The authority this action is for. The owner refuses, before sending, when the authority
    /// that would answer is not this one.
    pub authority: AuthorityRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedLaunched {
    pub protocol: ManagedProtocol,
    pub store_stream_id: String,
    pub session_id: String,
}

/// Where a running shell is right now, and which process that was said of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCwd {
    pub cwd: String,
    pub pid: u32,
    /// The process's start time in clock ticks: a pid reused by another process differs.
    pub start_ticks: u64,
}

/// A read of one terminal's output from a byte position, as the host holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermRead {
    pub data: Vec<u8>,
    pub next: u64,
    pub eof: bool,
    pub truncated: bool,
}

pub trait Owner: Send + Sync {
    /// One checked, non-starting read of the host. `expected` is the epoch observed before;
    /// a different host is `HostChanged`.
    fn host(&self, expected: Option<&str>) -> Result<HostView, OwnerError>;
    /// Sessions of the Managed authorities, P1 and P2, each under its own store identity.
    fn managed(&self) -> Result<Vec<ManagedRow>, OwnerError>;
    /// The authority a Managed launch would go to, read now, and what it has shown it serves.
    /// An authority that cannot be read or verified is an error, never an empty promise.
    fn managed_advert(&self) -> Result<ManagedAdvert, OwnerError>;
    fn root(&self, path: &str) -> Result<RootInfo, RootError>;
    /// Makes `name` inside `parent` exclusively, without Git and without launching anything.
    fn create_folder(&self, parent: &str, name: &str) -> Result<String, FolderError>;
    fn launch_native(
        &self,
        launch: &NativeLaunch,
        epoch: &str,
    ) -> Result<NativeLaunched, LaunchError>;
    fn launch_shell(&self, path: &str, epoch: &str) -> Result<ShellLaunched, LaunchError>;
    fn launch_managed(&self, launch: &ManagedLaunch) -> Result<ManagedLaunched, LaunchError>;
    /// Terminal I/O through the same checked connection: a different host is `HostChanged` and
    /// nothing is sent. `wait_ms` bounds how long a read waits for output.
    fn term_read(
        &self,
        epoch: &str,
        terminal: &str,
        from: u64,
        wait_ms: u64,
    ) -> Result<TermRead, OwnerError>;
    fn term_write(&self, epoch: &str, terminal: &str, data: &[u8]) -> Result<(), OwnerError>;
    fn term_resize(
        &self,
        epoch: &str,
        terminal: &str,
        rows: u16,
        cols: u16,
    ) -> Result<(), OwnerError>;
    /// The directory the shell of a running terminal is in now, read from the process the host
    /// reports for it. A host that is not the one observed is `HostChanged`; a terminal that is
    /// not running, has no observed process or whose directory cannot be read is refused: an
    /// unverifiable directory is never guessed from where the shell started.
    fn shell_cwd(&self, epoch: &str, terminal: &str) -> Result<ShellCwd, OwnerError>;
}

/// The working directory of `pid` through `/proc`, taken only while the process is the same one
/// before and after the read, and the user's own.
pub fn read_cwd(pid: u32) -> Result<ShellCwd, OwnerError> {
    let pid_i = i32::try_from(pid).map_err(|_| OwnerError::Refused)?;
    let before = start_ticks(pid_i).ok_or(OwnerError::Refused)?;
    let proc = PathBuf::from(format!("/proc/{pid}"));
    let meta = fs::metadata(&proc).map_err(|_| OwnerError::Refused)?;
    if meta.uid() != nix::unistd::geteuid().as_raw() {
        return Err(OwnerError::Refused);
    }
    let link = fs::read_link(proc.join("cwd")).map_err(|_| OwnerError::Refused)?;
    if start_ticks(pid_i) != Some(before) {
        return Err(OwnerError::Refused);
    }
    let cwd = link.to_str().ok_or(OwnerError::Refused)?.to_owned();
    // A directory that was removed is shown by the kernel with a suffix: it is not a place.
    if !cwd.starts_with('/')
        || cwd.len() > MAX_PATH_BYTES
        || cwd.chars().any(char::is_control)
        || cwd.ends_with(" (deleted)")
    {
        return Err(OwnerError::Refused);
    }
    Ok(ShellCwd {
        cwd,
        pid,
        start_ticks: before,
    })
}

// ---- the checked connection --------------------------------------------------------------

/// What identifies the process behind a socket: the socket file, the peer's pid and its start
/// time. A host that was replaced differs in at least one. This is a compatibility check for
/// an old host with no instance feature; it does not contain a hostile process of the same user.
fn observe_peer(stream: &UnixStream, socket: &Path) -> Result<String, OwnerError> {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    let credentials = getsockopt(stream, PeerCredentials).map_err(|_| OwnerError::HostChanged)?;
    if credentials.uid() != nix::unistd::geteuid().as_raw() {
        return Err(OwnerError::HostChanged);
    }
    let meta = fs::symlink_metadata(socket).map_err(|_| OwnerError::HostChanged)?;
    let ticks = start_ticks(credentials.pid()).ok_or(OwnerError::HostChanged)?;
    let epoch = format!(
        "h1:{}:{}:{}:{ticks}",
        meta.dev(),
        meta.ino(),
        credentials.pid()
    );
    if epoch.len() > MAX_EPOCH_BYTES {
        return Err(OwnerError::HostChanged);
    }
    Ok(epoch)
}

/// Field 22 of `/proc/<pid>/stat`: the process start time in clock ticks since boot.
pub fn start_ticks(pid: i32) -> Option<u64> {
    parse_start_ticks(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// The command name is in parentheses and may contain spaces and parentheses itself, so the
/// fields are counted from the last closing one.
pub fn parse_start_ticks(stat: &str) -> Option<u64> {
    let after = stat.get(stat.rfind(')')? + 2..)?;
    after.split(' ').nth(19)?.parse().ok()
}

/// The host through its private socket, checked on every connection and never started.
pub struct HostSocket {
    path: PathBuf,
}

impl HostSocket {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn from_environment() -> Option<Self> {
        crate::host::server::socket_path().map(Self::new)
    }

    /// Connects, verifies the peer on this very connection, then sends one request. No retry,
    /// no spawn, no local fallback.
    fn call(&self, expected: Option<&str>, op: &str, params: Value) -> Result<Value, CallFailure> {
        let mut stream = UnixStream::connect(&self.path)
            .map_err(|_| CallFailure::Before(OwnerError::HostOffline))?;
        let _ = stream.set_read_timeout(Some(CALL_TIMEOUT));
        let _ = stream.set_write_timeout(Some(CALL_TIMEOUT));
        let epoch = observe_peer(&stream, &self.path).map_err(CallFailure::Before)?;
        if expected.is_some_and(|expected| expected != epoch) {
            return Err(CallFailure::Before(OwnerError::HostChanged));
        }
        use crate::host::protocol::{Answer, Request, read_frame, write_frame};
        write_frame(
            &mut stream,
            &Request {
                op: op.to_owned(),
                params,
            },
        )
        .map_err(|_| CallFailure::After)?;
        match read_frame::<Answer>(&mut stream).map_err(|_| CallFailure::After)? {
            Answer::Ok(value) => Ok(value),
            Answer::Err(error) => Err(CallFailure::Refused(error.code)),
        }
    }

    /// The epoch of the host that answers now, from a connection of its own.
    pub fn epoch(&self) -> Result<String, OwnerError> {
        let stream = UnixStream::connect(&self.path).map_err(|_| OwnerError::HostOffline)?;
        observe_peer(&stream, &self.path)
    }

    pub fn host_view(&self, expected: Option<&str>) -> Result<HostView, OwnerError> {
        let hello = self
            .call(expected, "hello", Value::Null)
            .map_err(CallFailure::before)?;
        if hello["protocol"] != json!(crate::host::protocol::PROTOCOL) {
            return Err(OwnerError::Unsupported);
        }
        let features = hello["features"]
            .as_array()
            .map(|names| {
                names
                    .iter()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        // The terminals are read on a connection of their own, checked against the epoch just
        // observed, so a host replaced between the two calls is refused rather than mixed.
        let epoch = self.epoch()?;
        // The second observation is held to the expectation too: a host replaced between the two
        // calls is not described with the new one's epoch.
        if expected.is_some_and(|e| e != epoch) {
            return Err(OwnerError::HostChanged);
        }
        let list = self
            .call(Some(&epoch), "term_list", Value::Null)
            .map_err(CallFailure::before)?;
        let terminals = list
            .as_array()
            .ok_or(OwnerError::Unsupported)?
            .iter()
            .filter_map(|row| {
                Some(TerminalRow {
                    terminal_id: row["terminal"]["terminal_id"].as_str()?.to_owned(),
                    running: row["terminal"]["state"] == json!("running"),
                    cwd: row["terminal"]["cwd"].as_str().map(str::to_owned),
                    native_session: row["native"]["session_id"].as_str().map(str::to_owned),
                    pid: row["terminal"]["observed_pid"]
                        .as_u64()
                        .and_then(|pid| u32::try_from(pid).ok()),
                })
            })
            .collect();
        Ok(HostView {
            epoch,
            features,
            terminals,
        })
    }
}

enum CallFailure {
    /// Failed before anything was sent.
    Before(OwnerError),
    /// The owner answered with a refusal word.
    Refused(String),
    /// Sent, and the answer was lost or broken: the effect is unknown.
    After,
}

impl CallFailure {
    fn before(self) -> OwnerError {
        match self {
            Self::Before(error) => error,
            Self::Refused(_) => OwnerError::Refused,
            Self::After => OwnerError::HostOffline,
        }
    }

    fn launch(self) -> LaunchError {
        match self {
            Self::Before(error) => LaunchError::NotDispatched(error),
            Self::Refused(code) => LaunchError::NotDispatched(match code.as_str() {
                "profile_unavailable" | "profile_missing" | "profile_unknown" => {
                    OwnerError::ProfileUnavailable
                }
                _ => OwnerError::Refused,
            }),
            Self::After => LaunchError::Ambiguous,
        }
    }
}

// ---- filesystem roots --------------------------------------------------------------------

/// Looks at a folder without following a link in its last component and without changing it.
pub fn inspect_root(path: &str) -> Result<RootInfo, RootError> {
    let home = std::env::var_os("HOME").and_then(|h| fs::canonicalize(h).ok());
    inspect_root_with(path, home.as_deref(), console_state().as_deref())
}

/// The Console's state directory, where the owner's `configuration/trusted-writers` is.
fn console_state() -> Option<PathBuf> {
    super::store::default_dir()?
        .parent()?
        .parent()
        .map(PathBuf::from)
}

/// `inspect_root` with the home folder named and no account declared, so a test can name its own.
#[cfg(test)]
pub fn inspect_root_in(path: &str, home: Option<&Path>) -> Result<RootInfo, RootError> {
    inspect_root_with(path, home, None)
}

/// `inspect_root` with the home folder and the state directory named. A folder anybody else can
/// write (an ACL, the mode bits, or the ACL it gives what is made in it) is not a root, except
/// for the accounts the owner declared in `<state>/configuration/trusted-writers`, the file the
/// configuration engine already believes. A folder whose ACL cannot be read is not private.
pub fn inspect_root_with(
    path: &str,
    home: Option<&Path>,
    state: Option<&Path>,
) -> Result<RootInfo, RootError> {
    if !path.starts_with('/') || path.len() > MAX_PATH_BYTES || path.contains('\0') {
        return Err(RootError::Ineligible);
    }
    let canonical = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(RootError::Ineligible),
        Ok(_) => fs::canonicalize(path).map_err(|_| RootError::Missing)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(RootError::Missing),
        Err(_) => return Err(RootError::Ineligible),
    };
    let dir = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(&canonical)
        .map_err(|_| RootError::Ineligible)?;
    let meta = dir.metadata().map_err(|_| RootError::Ineligible)?;
    if !meta.is_dir() || meta.uid() != nix::unistd::geteuid().as_raw() {
        return Err(RootError::Ineligible);
    }
    // Who else can write the folder is read from the descriptor just opened, not from the path.
    if crate::agentconfig::folder_is_shared(&dir, meta.mode(), meta.gid(), meta.uid(), state) {
        return Err(RootError::Shared);
    }
    let text = canonical.to_str().ok_or(RootError::Ineligible)?.to_owned();
    if text.len() > MAX_PATH_BYTES || text.chars().any(char::is_control) {
        return Err(RootError::Ineligible);
    }
    let kind = match fs::symlink_metadata(canonical.join(".git")) {
        Ok(git) if git.is_dir() => CheckoutKind::Git,
        Ok(git) if git.is_file() => CheckoutKind::Worktree,
        _ => CheckoutKind::Folder,
    };
    let is_home = home == Some(canonical.as_path());
    Ok(RootInfo {
        device: meta.dev(),
        inode: meta.ino(),
        kind,
        native_eligible: true,
        // Agent configuration is never written to the home folder or to the system's folders.
        config_eligible: !is_home && !system_folder(&canonical),
        path: text,
    })
}

fn system_folder(path: &Path) -> bool {
    path.components().count() <= 2
        || [
            "/etc", "/usr", "/bin", "/sbin", "/lib", "/boot", "/proc", "/sys", "/dev", "/run",
            "/var",
        ]
        .iter()
        .any(|root| path.starts_with(root))
}

/// Creates `name` inside `parent` exclusively: one plain basename, nothing else.
pub fn make_folder(parent: &str, name: &str) -> Result<String, FolderError> {
    let plain = !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\0'])
        && !name.chars().any(char::is_control);
    if !plain {
        return Err(FolderError::Name);
    }
    let parent = inspect_root(parent).map_err(|_| FolderError::Io)?;
    let target = Path::new(&parent.path).join(name);
    match fs::create_dir(&target) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(FolderError::Exists);
        }
        Err(_) => return Err(FolderError::Io),
    }
    target.to_str().map(str::to_owned).ok_or(FolderError::Io)
}

// ---- the real owner ----------------------------------------------------------------------

/// What the real owner needs from the Managed authorities. It has no way to bind, lease or
/// start a turn: a Managed launch only prepares a session, and activation stays with the
/// existing Managed screens.
pub trait ManagedPort: Send + Sync {
    fn sessions(&self) -> Result<Vec<ManagedRow>, OwnerError>;
    fn advertised(&self) -> Result<ManagedAdvert, OwnerError>;
    /// Prepares a session in `launch.authority` and in no other: when the authority that would
    /// answer is a different one nothing is sent (`HostChanged`).
    fn prepare(&self, launch: &ManagedLaunch) -> Result<ManagedLaunched, LaunchError>;
}

/// No Managed authority is reachable. Only tests build an owner without one.
#[cfg(test)]
pub struct NoManaged;

#[cfg(test)]
impl ManagedPort for NoManaged {
    fn sessions(&self) -> Result<Vec<ManagedRow>, OwnerError> {
        Ok(Vec::new())
    }

    fn advertised(&self) -> Result<ManagedAdvert, OwnerError> {
        Err(OwnerError::Unsupported)
    }

    fn prepare(&self, _: &ManagedLaunch) -> Result<ManagedLaunched, LaunchError> {
        Err(LaunchError::NotDispatched(OwnerError::Unsupported))
    }
}

pub struct RealOwner<M: ManagedPort> {
    host: Option<HostSocket>,
    managed: M,
}

impl<M: ManagedPort> RealOwner<M> {
    pub fn new(host: Option<HostSocket>, managed: M) -> Self {
        Self { host, managed }
    }

    fn socket(&self) -> Result<&HostSocket, OwnerError> {
        self.host.as_ref().ok_or(OwnerError::HostOffline)
    }
}

#[derive(Deserialize)]
struct Started {
    session: StartedSession,
    terminal: StartedTerminal,
}
#[derive(Deserialize)]
struct StartedSession {
    id: String,
}
#[derive(Deserialize)]
struct StartedTerminal {
    terminal_id: String,
}

impl<M: ManagedPort> Owner for RealOwner<M> {
    fn host(&self, expected: Option<&str>) -> Result<HostView, OwnerError> {
        self.socket()?.host_view(expected)
    }

    fn managed(&self) -> Result<Vec<ManagedRow>, OwnerError> {
        self.managed.sessions()
    }

    fn managed_advert(&self) -> Result<ManagedAdvert, OwnerError> {
        self.managed.advertised()
    }

    fn root(&self, path: &str) -> Result<RootInfo, RootError> {
        inspect_root(path)
    }

    fn create_folder(&self, parent: &str, name: &str) -> Result<String, FolderError> {
        make_folder(parent, name)
    }

    fn launch_native(
        &self,
        launch: &NativeLaunch,
        epoch: &str,
    ) -> Result<NativeLaunched, LaunchError> {
        let socket = self.socket().map_err(LaunchError::NotDispatched)?;
        // The profile is checked at its current version before anything is started.
        if let Some((id, version)) = &launch.profile {
            let profiles = socket
                .call(Some(epoch), "profiles", Value::Null)
                .map_err(CallFailure::launch)?;
            let current = profiles.as_array().and_then(|list| {
                list.iter()
                    .find(|p| p["id"] == json!(id))
                    .and_then(|p| p["current"].as_u64())
            });
            if current != Some(u64::from(*version)) {
                return Err(LaunchError::NotDispatched(OwnerError::ProfileUnavailable));
            }
        }
        socket
            .call(
                Some(epoch),
                "native_remember_project",
                json!({"path": launch.path}),
            )
            .map_err(CallFailure::launch)?;
        let agent = match launch.agent {
            Agent::ClaudeCode => "claude_code",
            Agent::Codex => "codex",
        };
        let started = socket
            .call(
                Some(epoch),
                "native_start",
                json!({
                    "agent_id": agent,
                    "project": launch.path,
                    "profile_id": launch.profile.as_ref().map(|(id, _)| id),
                    "size": {"rows": 24, "cols": 80},
                }),
            )
            .map_err(CallFailure::launch)?;
        // Dispatched. A reply that cannot be read is ambiguous: no ID, no guess.
        let started: Started =
            serde_json::from_value(started).map_err(|_| LaunchError::Ambiguous)?;
        Ok(NativeLaunched {
            session_id: started.session.id,
            terminal_id: started.terminal.terminal_id,
        })
    }

    fn launch_shell(&self, path: &str, epoch: &str) -> Result<ShellLaunched, LaunchError> {
        let socket = self.socket().map_err(LaunchError::NotDispatched)?;
        let created = socket
            .call(
                Some(epoch),
                "term_create",
                // Scratch has no folder: the host's own default starts it.
                json!({"size": {"rows": 24, "cols": 80},
                       "cwd": (!path.is_empty()).then_some(path), "title": null}),
            )
            .map_err(CallFailure::launch)?;
        let terminal_id = created["terminal_id"]
            .as_str()
            .ok_or(LaunchError::Ambiguous)?
            .to_owned();
        Ok(ShellLaunched { terminal_id })
    }

    fn launch_managed(&self, launch: &ManagedLaunch) -> Result<ManagedLaunched, LaunchError> {
        self.managed.prepare(launch)
    }

    fn term_read(
        &self,
        epoch: &str,
        terminal: &str,
        from: u64,
        wait_ms: u64,
    ) -> Result<TermRead, OwnerError> {
        let answer = self
            .socket()?
            .call(
                Some(epoch),
                "term_read",
                json!({"terminal_id": terminal, "from": from, "wait_ms": wait_ms.min(1000)}),
            )
            .map_err(CallFailure::before)?;
        let data = answer["data"]
            .as_array()
            .ok_or(OwnerError::Unsupported)?
            .iter()
            .map(|b| b.as_u64().and_then(|b| u8::try_from(b).ok()))
            .collect::<Option<Vec<u8>>>()
            .ok_or(OwnerError::Unsupported)?;
        Ok(TermRead {
            data,
            next: answer["next"].as_u64().unwrap_or(from),
            eof: answer["eof"] == json!(true),
            truncated: answer["truncated"] == json!(true),
        })
    }

    fn term_write(&self, epoch: &str, terminal: &str, data: &[u8]) -> Result<(), OwnerError> {
        self.socket()?
            .call(
                Some(epoch),
                "term_write",
                json!({"terminal_id": terminal, "data": data}),
            )
            .map(|_| ())
            .map_err(CallFailure::before)
    }

    fn term_resize(
        &self,
        epoch: &str,
        terminal: &str,
        rows: u16,
        cols: u16,
    ) -> Result<(), OwnerError> {
        self.socket()?
            .call(
                Some(epoch),
                "term_resize",
                json!({"terminal_id": terminal, "size": {"rows": rows, "cols": cols}}),
            )
            .map(|_| ())
            .map_err(CallFailure::before)
    }

    fn shell_cwd(&self, epoch: &str, terminal: &str) -> Result<ShellCwd, OwnerError> {
        let view = self.socket()?.host_view(Some(epoch))?;
        let row = view
            .terminals
            .iter()
            .find(|t| t.terminal_id == terminal && t.running)
            .ok_or(OwnerError::Refused)?;
        read_cwd(row.pid.ok_or(OwnerError::Unsupported)?)
    }
}
