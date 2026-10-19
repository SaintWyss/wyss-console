//! The `--host` process: a Unix socket, one request per connection, served by the core.
//! Private to the user: the directory is 0700, the socket 0600, and the peer's uid must be
//! this process's. It exits when it has held nothing for a while (a host with no terminal and
//! no running agent has no reason to stay) or when told to.
use super::{
    core::HostCore,
    protocol::{Answer, HostError, Request, read_frame, write_frame},
};
use std::{
    io,
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

/// A host with nothing to hold for this long exits.
pub const IDLE_EXIT: Duration = Duration::from_secs(10 * 60);

/// `$XDG_RUNTIME_DIR/wyss-console/host.sock`.
pub fn socket_path() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())?;
    Some(runtime.join("wyss-console").join("host.sock"))
}

fn peer_is_me(stream: &UnixStream) -> bool {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    getsockopt(stream, PeerCredentials)
        .is_ok_and(|credentials| credentials.uid() == nix::unistd::geteuid().as_raw())
}

pub enum BindError {
    AlreadyRunning,
    Unsafe,
    Io(io::Error),
}

/// Makes the directory private and binds the socket; a socket nobody answers on is removed.
pub fn bind(path: &Path) -> Result<UnixListener, BindError> {
    let dir = path.parent().ok_or(BindError::Unsafe)?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(BindError::Io)?;
    let meta = std::fs::symlink_metadata(dir).map_err(BindError::Io)?;
    use std::os::unix::fs::MetadataExt;
    if !meta.is_dir() || meta.uid() != nix::unistd::geteuid().as_raw() {
        return Err(BindError::Unsafe);
    }
    if meta.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(BindError::Io)?;
    }
    if let Ok(existing) = std::fs::symlink_metadata(path) {
        if !existing.file_type().is_socket() {
            return Err(BindError::Unsafe);
        }
        if UnixStream::connect(path).is_ok() {
            return Err(BindError::AlreadyRunning);
        }
        let _ = std::fs::remove_file(path);
    }
    let listener = UnixListener::bind(path).map_err(BindError::Io)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(BindError::Io)?;
    Ok(listener)
}

fn serve(mut stream: UnixStream, core: &Arc<HostCore>, stop: &AtomicBool) {
    if !peer_is_me(&stream) {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok(request) = read_frame::<Request>(&mut stream) else {
        let _ = write_frame(&mut stream, &Answer::Err(HostError::new("request_invalid")));
        return;
    };
    let answer = if request.op == "shutdown" {
        stop.store(true, Ordering::Release);
        Answer::Ok(serde_json::Value::Null)
    } else {
        match core.call(&request.op, request.params) {
            Ok(value) => Answer::Ok(value),
            Err(error) => Answer::Err(error),
        }
    };
    let _ = write_frame(&mut stream, &answer);
}

/// Serves until told to stop or idle for `idle_exit`. Returns when the host should exit.
pub fn run(listener: UnixListener, core: Arc<HostCore>, idle_exit: Duration) {
    let stop = Arc::new(AtomicBool::new(false));
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let (core, stop) = (core.clone(), stop.clone());
                let _ = thread::Builder::new()
                    .name("wyss-host-conn".into())
                    .spawn(move || serve(stream, &core, &stop));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if core.idle_for().is_some_and(|idle| idle >= idle_exit) {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
    core.shutdown();
}
