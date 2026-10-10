//! Making the authority available, without becoming it.
//!
//! `wyss-control` stays what it was: a separate process that owns canonical state and
//! decides. What this module adds is that a user no longer has to start it. A packaged
//! desktop carries the authority it was built with, installs it under the user's own
//! XDG data directory, writes one `systemd --user` unit for it and asks the user's
//! service manager to start it when none answers.
//!
//! Who owns what:
//!
//! - **the service manager** owns the process: it starts it, restarts it after a crash
//!   within a limit, and stops it when the session ends. Nothing here restarts or stops
//!   an authority, and closing the desktop does neither;
//! - **the authority** owns its store, its socket and its locks, including what to do
//!   with a socket nobody listens on;
//! - **`AuthorityService`** (ADR 0019) is still the one observer of the connection. Ready
//!   means it is connected, never that a process or a unit exists;
//! - **this module** owns two files, the managed binary and the unit, and one request:
//!   start. It asks for that by itself at most once in a desktop's lifetime; after that
//!   only the user's Retry asks again.
//!
//! An authority that already answers is used as it is, whoever started it. One that runs
//! is never replaced: a newer packaged authority is installed the next time none runs.
mod bootstrap;
pub mod install;
mod layout;
mod manager;
mod package;
#[cfg(test)]
mod tests;
mod view;

pub use bootstrap::{AuthorityBootstrap, AuthorityLink, Limits, LinkState};
pub use layout::{Layout, UNIT_NAME};
pub use manager::{
    ActiveState, LoadState, ManagerError, ServiceManager, Systemd, UnitResult, UnitStatus,
};
pub use package::{Digest, Package, Payload, Provision};
pub use view::{
    BuildKind, Diagnostics, Failure, InstallationState, LifecycleView, Origin, Phase, ServiceState,
};

/// Why a step of the lifecycle could not be carried out. Internal: the window is shown a
/// [`Failure`], which says less.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleError {
    /// A path that has to be this user's alone is not: a link, another owner, a
    /// directory others can write to.
    UnsafePath,
    /// A file could not be read or written.
    Io,
    /// The packaged authority is missing or is not the one this build was made with.
    PackageInvalid,
    /// A file this product did not write is where its own would go.
    Foreign,
    /// Another installer holds the installation and did not finish in time.
    Busy,
}
pub type LifecycleResult<T> = Result<T, LifecycleError>;
