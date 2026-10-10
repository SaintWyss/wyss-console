//! What the window is told about the lifecycle: closed words, and nothing of the system
//! underneath. No path, process, unit name, command or error text crosses.
use serde::Serialize;

pub(super) const VIEW_SCHEMA_VERSION: u8 = 1;

/// Where making the authority available stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Finding out whether an authority answers and what is installed.
    Checking,
    /// Writing the managed authority or its service definition.
    Installing,
    /// The service manager was asked to start it.
    Starting,
    /// It is started, or being restarted, and has not answered yet. Bounded.
    Connecting,
    /// An authority answered and the desktop is connected to it.
    Ready,
    /// An authority answers and this build cannot work with it.
    Incompatible,
    /// The attempt ended. Nothing more is tried until something changes or Retry.
    Failed,
}

/// Why an attempt ended, in the product's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    /// This build carries no authority (a development build) and none is running.
    NotPackaged,
    /// The authority packaged with this build is missing or is not the one it was built with.
    PackageInvalid,
    /// A place the installation has to own is not safely this user's.
    InstallationUnsafe,
    /// A service definition this product did not write is where its own would go.
    InstallationMismatch,
    /// The installation could not be written.
    InstallFailed,
    /// No user service manager answers.
    ManagerUnavailable,
    /// The service manager did not start it.
    StartRefused,
    /// It was started and ended before answering.
    StartupFailed,
    /// It ended as often as its restart limit allows.
    ExitedRepeatedly,
    /// It was running and is not any more.
    Stopped,
    /// It is running and nothing answered in time.
    SocketUnavailable,
    /// The place of the control socket is not safely this user's.
    SocketUnsafe,
}

/// Who started the authority that answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// This installation's service.
    Managed,
    /// Something else: it is used, and never replaced, restarted or stopped.
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildKind {
    Packaged,
    Development,
    Invalid,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationState {
    Unknown,
    Absent,
    Current,
    /// Installed by another build. Replaced the next time no authority runs.
    Outdated,
    /// A service definition that is not this product's.
    Mismatch,
    Unsafe,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Unknown,
    Unavailable,
    NotInstalled,
    Inactive,
    Starting,
    Running,
    Failed,
}

/// What a person troubleshooting needs, and nothing that identifies a process or a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Diagnostics {
    pub build: BuildKind,
    pub installation: InstallationState,
    pub service: ServiceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LifecycleView {
    pub schema_version: u8,
    /// Grows by one with every change of this view. It is the caller's wait cursor.
    pub revision: u64,
    pub phase: Phase,
    /// Present exactly while `phase` is `failed`.
    pub failure: Option<Failure>,
    /// Whether Retry would do anything now.
    pub retry: bool,
    /// Present while an authority answers (`ready`, `incompatible`) and it is known.
    pub origin: Option<Origin>,
    /// A newer authority than the running managed one is packaged with this build. It
    /// is installed the next time no authority runs, and never under a running one.
    pub update_pending: bool,
    pub diagnostics: Diagnostics,
}
impl LifecycleView {
    pub(super) fn initial(build: BuildKind) -> Self {
        Self {
            schema_version: VIEW_SCHEMA_VERSION,
            revision: 0,
            phase: Phase::Checking,
            failure: None,
            retry: false,
            origin: None,
            update_pending: false,
            diagnostics: Diagnostics {
                build,
                installation: InstallationState::Unknown,
                service: ServiceState::Unknown,
            },
        }
    }
}
