#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// The configuration engine is part of the desktop only: it is not in the library the
// authority binary links, so that binary cannot contain it (C-02, TS-SEC-03).
#[cfg(target_os = "linux")]
mod agentconfig;
// The window commands of the above: the one place that names both it and the toolkit.
#[cfg(target_os = "linux")]
mod configuration;
#[cfg(target_os = "linux")]
mod credentials;
#[cfg(target_os = "linux")]
mod diagnose;
#[cfg(target_os = "linux")]
mod handoff;
#[cfg(target_os = "linux")]
mod host;
#[cfg(target_os = "linux")]
mod native;
#[cfg(target_os = "linux")]
mod native_dialog;
// The remote gateway's core (F10a foundations, ADR 0049): decisions with no transport. Nothing
// constructs it outside its tests, and it is not in the library the authority links.
#[cfg(target_os = "linux")]
mod remote;

fn main() {
    // The host (F9): the same executable, started apart from any window, owns the terminals.
    #[cfg(target_os = "linux")]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == "--wyss-host")
    {
        std::process::exit(host::run());
    }
    #[cfg(target_os = "linux")]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == "--wyss-host-stop")
    {
        std::process::exit(host::stop());
    }
    #[cfg(target_os = "linux")]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == "--diagnose")
    {
        std::process::exit(diagnose::run());
    }
    // Set by the packaging command, which builds the authority first: a build made any
    // other way has no authority of its own to install.
    let digest = option_env!("WYSS_AUTHORITY_SHA256");
    // Protocol 2 is selected by the command line alone: `--f8-authority <directory>`. Without
    // it the interface is inactive and nothing is sent to any authority but the installed one.
    #[cfg(target_os = "linux")]
    {
        let f8 =
            wyss_console::f8_from_arguments(std::env::args_os().skip(1)).unwrap_or_else(|error| {
                eprintln!("wyss-console: {error}");
                std::process::exit(2);
            });
        wyss_console::run_with_f8(
            digest,
            Some(
                configuration::extension()
                    .merge(host::commands::extension())
                    .merge(native::extension())
                    .merge(credentials::commands::extension()),
            ),
            f8,
        );
    }
    #[cfg(not(target_os = "linux"))]
    wyss_console::run(digest);
}
