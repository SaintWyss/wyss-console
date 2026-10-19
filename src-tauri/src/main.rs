#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// The configuration engine is part of the desktop only: it is not in the library the
// authority binary links, so that binary cannot contain it (C-02, TS-SEC-03).
#[cfg(target_os = "linux")]
mod agentconfig;
// The window commands of the above: the one place that names both it and the toolkit.
#[cfg(target_os = "linux")]
mod configuration;
#[cfg(target_os = "linux")]
mod native;
#[cfg(target_os = "linux")]
mod native_dialog;

fn main() {
    // Set by the packaging command, which builds the authority first: a build made any
    // other way has no authority of its own to install.
    let digest = option_env!("WYSS_AUTHORITY_SHA256");
    #[cfg(target_os = "linux")]
    wyss_console::run_with(
        digest,
        configuration::extension().merge(native::extension()),
    );
    #[cfg(not(target_os = "linux"))]
    wyss_console::run(digest);
}
