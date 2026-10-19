#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// The configuration engine is part of the desktop only: it is not in the library the
// authority binary links, so that binary cannot contain it (C-02, TS-SEC-03).
#[cfg(target_os = "linux")]
mod agentconfig;

fn main() {
    // Set by the packaging command, which builds the authority first: a build made any
    // other way has no authority of its own to install.
    wyss_console::run(option_env!("WYSS_AUTHORITY_SHA256"));
}
