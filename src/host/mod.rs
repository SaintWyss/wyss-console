//! terminals and native sessions held by a host process that outlives the window.
//! Starts via the standalone `wyss-host` executable.
// The window's side of the host is reached only from `main`, which a test build has not.
#![cfg_attr(test, allow(dead_code))]
#[cfg(test)]
mod client;
pub mod core;
pub mod persistence;
pub mod protocol;
pub mod ring;
pub mod server;
#[cfg(test)]
mod tests;

/// The host entry: binds the private socket and serves. Exit codes: 0 stopped or
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
