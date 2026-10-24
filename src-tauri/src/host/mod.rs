//! F9: terminals and native sessions held by a host process that outlives the window.
//! The host is this same executable started with `--wyss-host`; the window is its client.
// The window's side of the host is reached only from `main`, which a test build has not.
#![cfg_attr(test, allow(dead_code))]
pub mod client;
pub mod commands;
pub mod core;
pub mod persistence;
pub mod protocol;
pub mod ring;
pub mod server;
#[cfg(test)]
mod tests;

/// The `--wyss-host` entry: binds the private socket and serves. Exit codes: 0 stopped or
/// idle, 2 already running or the path is not safe, 1 anything else.
pub fn run() -> i32 {
    let Some(path) = server::socket_path() else {
        eprintln!("wyss-host: no runtime directory");
        return 1;
    };
    let listener = match server::bind(&path) {
        Ok(listener) => listener,
        Err(server::BindError::AlreadyRunning | server::BindError::Unsafe) => return 2,
        Err(server::BindError::Io(error)) => {
            eprintln!("wyss-host: {error}");
            return 1;
        }
    };
    let core = core::HostCore::new(crate::native::open_from_environment());
    server::run(listener, core, server::IDLE_EXIT);
    let _ = std::fs::remove_file(&path);
    0
}

/// `--wyss-host-stop`: asks a running host to stop (it closes its terminals). 0 when it did,
/// or when none was running.
pub fn stop() -> i32 {
    match client::HostLink::stop_remote() {
        Ok(()) => 0,
        Err(error) if error.code == "host_unavailable" => 0,
        Err(_) => 1,
    }
}
