#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Set by the packaging command, which builds the authority first: a build made any
    // other way has no authority of its own to install.
    wyss_console::run(option_env!("WYSS_AUTHORITY_SHA256"));
}
