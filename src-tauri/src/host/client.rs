//! The window's way to a host: a socket client, or, when no host could be reached or started,
//! a host of its own inside the window's process (what the window always had: terminals that
//! end with it). Which one is in use is a fact the window can ask for and shows.
use super::{
    core::HostCore,
    protocol::{Answer, HostError, PROTOCOL, Request, read_frame, write_frame},
    server::socket_path,
};
use serde_json::{Value, json};
use std::{
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Remote {
    path: PathBuf,
}

impl Remote {
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn call(&self, op: &str, params: Value) -> Result<Value, HostError> {
        let transport = |_: std::io::Error| HostError::new("host_unavailable");
        let mut stream = UnixStream::connect(&self.path).map_err(transport)?;
        stream
            .set_read_timeout(Some(CALL_TIMEOUT))
            .map_err(transport)?;
        stream
            .set_write_timeout(Some(CALL_TIMEOUT))
            .map_err(transport)?;
        write_frame(
            &mut stream,
            &Request {
                op: op.to_owned(),
                params,
            },
        )
        .map_err(transport)?;
        match read_frame::<Answer>(&mut stream).map_err(transport)? {
            Answer::Ok(value) => Ok(value),
            Answer::Err(error) => Err(error),
        }
    }
}

enum Mode {
    Remote(Remote),
    /// No host outside the window could be used: terminals live and end with the window.
    Local(Arc<HostCore>),
}

/// The one handle the window's commands use. Cloning it shares the connection.
#[derive(Clone)]
pub struct HostLink {
    mode: Arc<Mutex<Option<Arc<Mode>>>>,
}

impl Default for HostLink {
    fn default() -> Self {
        Self::new()
    }
}

/// The running host's `hello`, if one answers with a protocol we speak.
fn hello(remote: &Remote) -> Option<Value> {
    remote
        .call("hello", Value::Null)
        .ok()
        .filter(|h| h["protocol"] == json!(PROTOCOL))
}

fn spawn_host() -> bool {
    use std::os::unix::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    // `WYSS_HOST_EXE` lets a test name the binary; nothing else changes what runs.
    let exe = std::env::var_os("WYSS_HOST_EXE").map_or(exe, PathBuf::from);
    Command::new(exe)
        .arg("--wyss-host")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir("/")
        .process_group(0)
        .spawn()
        .is_ok()
}

impl HostLink {
    pub fn new() -> Self {
        Self {
            mode: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub(crate) fn remote_for_test(path: PathBuf) -> Self {
        Self {
            mode: Arc::new(Mutex::new(Some(Arc::new(Mode::Remote(Remote::at(path)))))),
        }
    }

    fn connect(&self) -> Arc<Mode> {
        if let Some(path) = socket_path() {
            let remote = Remote { path };
            if hello(&remote).is_none() && spawn_host() {
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline && hello(&remote).is_none() {
                    std::thread::sleep(Duration::from_millis(40));
                }
            }
            if hello(&remote).is_some() {
                return Arc::new(Mode::Remote(remote));
            }
        }
        Arc::new(Mode::Local(HostCore::new(
            crate::native::open_from_environment(),
        )))
    }

    fn mode(&self) -> Arc<Mode> {
        self.mode
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(|| self.connect())
            .clone()
    }

    /// Whether the host in use says it has `feature`. A host from before features existed
    /// says nothing, and so has none.
    pub fn has_feature(&self, feature: &str) -> bool {
        self.call("hello", Value::Null).is_ok_and(|hello| {
            hello["features"]
                .as_array()
                .is_some_and(|names| names.iter().any(|n| n == feature))
        })
    }

    /// Whether the terminals live outside this window's process.
    pub fn is_remote(&self) -> bool {
        matches!(*self.mode(), Mode::Remote(_))
    }

    pub fn call(&self, op: &str, params: Value) -> Result<Value, HostError> {
        match &*self.mode() {
            Mode::Local(core) => core.call(op, params),
            Mode::Remote(remote) => match remote.call(op, params.clone()) {
                Err(error) if error.code == "host_unavailable" => {
                    // The host went away: start another and try once more. What lived in the
                    // old one is gone, and the answer to a stale id says so.
                    *self.mode.lock().unwrap_or_else(PoisonError::into_inner) = None;
                    match &*self.mode() {
                        Mode::Remote(again) => again.call(op, params),
                        Mode::Local(core) => core.call(op, params),
                    }
                }
                other => other,
            },
        }
    }

    /// A read of an already attached host. Never initializes, spawns, reconnects or retries.
    /// Readiness must not start a process, even when the previously connected host disappears.
    pub fn native_readiness(&self) -> Result<Value, HostError> {
        let mode = self
            .mode
            .try_lock()
            .map_err(|_| HostError::new("host_unavailable"))?
            .clone()
            .ok_or_else(|| HostError::new("host_unavailable"))?;
        match &*mode {
            Mode::Local(core) => core.call("native_readiness", Value::Null),
            Mode::Remote(remote) => remote.call("native_readiness", Value::Null),
        }
    }

    /// Closes what a window-local host holds. A remote host is left running.
    pub fn close_local(&self) {
        let mode = self
            .mode
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(mode) = mode
            && let Mode::Local(core) = &*mode
        {
            core.shutdown();
        }
    }

    /// What a running host says about itself, without starting one.
    pub fn hello_only() -> Result<Value, HostError> {
        let path = socket_path().ok_or_else(|| HostError::new("host_unavailable"))?;
        Remote { path }.call("hello", Value::Null)
    }

    /// Asks a running host to stop (it closes its terminals).
    pub fn stop_remote() -> Result<(), HostError> {
        let path = socket_path().ok_or_else(|| HostError::new("host_unavailable"))?;
        Remote { path }.call("shutdown", Value::Null).map(|_| ())
    }
}
