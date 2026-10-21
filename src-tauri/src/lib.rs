pub mod adapters;
mod application;
#[cfg(target_os = "linux")]
pub mod authority;
pub mod contracts;
#[cfg(target_os = "linux")]
pub mod control;
mod desktop;
#[cfg(target_os = "linux")]
pub mod events;
#[cfg(target_os = "linux")]
pub mod lifecycle;
#[cfg(target_os = "linux")]
pub mod sessions;
#[cfg(target_os = "linux")]
pub mod settings;
pub mod terminal;
#[cfg(target_os = "linux")]
pub mod turns;

/// Starts the desktop. `authority_digest` is the SHA-256 of the authority this build was
/// packaged with, in hex, or `None` for a build that was not packaged: only a packaged
/// build installs and starts an authority of its own.
pub fn run(authority_digest: Option<&'static str>) {
    desktop::run(authority_digest, None);
}

pub use desktop::Extension;

/// `run` with commands the binary brings (see `Extension`).
pub fn run_with(authority_digest: Option<&'static str>, extension: Extension) {
    desktop::run(authority_digest, Some(extension));
}

/// F8 implementation behind an inactive local application selection; installed startup stays v1.
pub mod f8;
