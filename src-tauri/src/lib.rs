pub mod adapters;
mod application;
#[cfg(target_os = "linux")]
pub mod authority;
/// The constrained B1 candidate: present only in fixture builds, so no packaged binary carries a B1
/// launcher (ADR 0058, doc 51: no production launch before an independent review).
#[cfg(all(target_os = "linux", feature = "b1-fixture"))]
pub mod b1;
pub mod contracts;
#[cfg(target_os = "linux")]
pub mod control;
mod desktop;
#[cfg(target_os = "linux")]
pub mod events;
#[cfg(target_os = "linux")]
pub mod lifecycle;
#[cfg(target_os = "linux")]
pub mod m3;
pub mod screen;
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
    desktop::run(
        authority_digest,
        None,
        desktop::inactive_f8(),
        desktop::inactive_m3(),
    );
}

pub use desktop::Extension;

/// `run` with the protocol-2 interface the caller selected (see `f8_from_arguments`): inactive
/// unless the command line of whoever started the desktop named an authority. Nothing a window
/// sends can change it.
#[cfg(target_os = "linux")]
pub fn run_with_f8(
    authority_digest: Option<&'static str>,
    extension: Option<Extension>,
    f8: f8::gui::F8Gui,
) {
    desktop::run(authority_digest, extension, f8, desktop::inactive_m3());
}

/// `run_with_f8` with the M3 service the caller owns: a build that verifies a Claude installation
/// or hosts a fixture runtime. The installed entry points use the inactive one.
#[cfg(target_os = "linux")]
pub fn run_with_m3(
    authority_digest: Option<&'static str>,
    extension: Option<Extension>,
    f8: f8::gui::F8Gui,
    m3: m3::gui::M3Service,
) {
    desktop::run(authority_digest, extension, f8, m3);
}

/// The protocol-2 interface that `--f8-authority <directory>` selects, or the inactive one when
/// the arguments (without the program name) name none. A malformed selection is an error.
#[cfg(target_os = "linux")]
pub use desktop::select::from_arguments as f8_from_arguments;

/// `run` with commands the binary brings (see `Extension`).
pub fn run_with(authority_digest: Option<&'static str>, extension: Extension) {
    desktop::run(
        authority_digest,
        Some(extension),
        desktop::inactive_f8(),
        desktop::inactive_m3(),
    );
}

/// F8 implementation behind an inactive local application selection; installed startup stays v1.
pub mod f8;
