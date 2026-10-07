use super::{
    framing::{read_frame, write_frame},
    paths::Paths,
    store::{Store, uuid},
};
use crate::contracts::{WireUuid, control::*};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, poll},
    sys::{
        signal::{SigSet, Signal},
        signalfd::{SfdFlags, SignalFd},
        socket::{
            AddressFamily, SockFlag, SockType, UnixAddr, connect, getsockopt, socket, sockopt,
        },
    },
    unistd::geteuid,
};
use std::{
    fs,
    os::{
        fd::{AsFd, AsRawFd},
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

pub const MAX_CLIENTS: usize = 8;
const QUEUE: usize = 8;
const HELLO_TIMEOUT: Duration = Duration::from_secs(3);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);
struct Job {
    op: Operation,
    reply: SyncSender<ControlResult<ResultValue>>,
}
pub(crate) struct SocketGuard {
    path: PathBuf,
    dev: u64,
    ino: u64,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Ok(m) = fs::symlink_metadata(&self.path)
            && m.file_type().is_socket()
            && m.uid() == geteuid().as_raw()
            && m.dev() == self.dev
            && m.ino() == self.ino
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
pub(crate) fn bind(path: &Path) -> ControlResult<(UnixListener, SocketGuard)> {
    UnixAddr::new(path).map_err(|_| ControlError::UnsafePath)?;
    match fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.file_type().is_socket() || m.uid() != geteuid().as_raw() {
                return Err(ControlError::UnsafePath);
            }
            // Nonblocking connect: an active or indeterminate listener is never unlinked.
            let fd = socket(
                AddressFamily::Unix,
                SockType::Stream,
                SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
                None,
            )
            .map_err(|_| ControlError::TransportError)?;
            let addr = UnixAddr::new(path).map_err(|_| ControlError::UnsafePath)?;
            match connect(fd.as_raw_fd(), &addr) {
                Err(Errno::ECONNREFUSED) => (),
                _ => return Err(ControlError::AlreadyRunning),
            }
            let current = fs::symlink_metadata(path).map_err(|_| ControlError::UnsafePath)?;
            if current.dev() != m.dev()
                || current.ino() != m.ino()
                || !current.file_type().is_socket()
            {
                return Err(ControlError::UnsafePath);
            }
            fs::remove_file(path).map_err(|_| ControlError::UnsafePath)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err(ControlError::UnsafePath),
    }
    let listener = UnixListener::bind(path).map_err(|_| ControlError::AlreadyRunning)?;
    let m = fs::symlink_metadata(path).map_err(|_| ControlError::UnsafePath)?;
    let guard = SocketGuard {
        path: path.to_owned(),
        dev: m.dev(),
        ino: m.ino(),
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| ControlError::UnsafePath)?;
    listener
        .set_nonblocking(true)
        .map_err(|_| ControlError::TransportError)?;
    Ok((listener, guard))
}
pub(crate) fn check_uid(uid: u32) -> ControlResult<()> {
    if uid == geteuid().as_raw() {
        Ok(())
    } else {
        Err(ControlError::PeerRejected)
    }
}
pub(crate) fn peer(stream: &UnixStream) -> ControlResult<()> {
    let creds =
        getsockopt(stream, sockopt::PeerCredentials).map_err(|_| ControlError::PeerRejected)?;
    check_uid(creds.uid())
}
fn connection(
    mut stream: UnixStream,
    core: SyncSender<Job>,
    instance: WireUuid,
    stop: Arc<AtomicBool>,
) {
    if peer(&stream).is_err() || stream.set_nonblocking(true).is_err() {
        return;
    }
    let mut hello = false;
    loop {
        let timeout = if hello { READ_TIMEOUT } else { HELLO_TIMEOUT };
        let bytes = match read_frame(&mut stream, Instant::now() + timeout, &stop) {
            Ok(Some(b)) => b,
            _ => return,
        };
        // If an invalid envelope still has an unambiguous UUID, return a safe error.
        let request = match Request::parse(&bytes) {
            Ok(r) => r,
            Err(e) => {
                if let Ok(v) = unique_json(&bytes)
                    && let Some(id) = v.get("request_id")
                    && let Ok(id) = serde_json::from_value::<WireUuid>(id.clone())
                {
                    let response = Response {
                        request_id: id,
                        authority_instance_id: instance.clone(),
                        outcome: Err(e),
                    };
                    if let Ok(bytes) = response.encode() {
                        let _ =
                            write_frame(&mut stream, &bytes, Instant::now() + WRITE_TIMEOUT, &stop);
                    }
                }
                return;
            }
        };
        let is_hello = matches!(request.operation, Operation::Hello(_));
        let outcome = if !hello && !is_hello {
            Err(ControlError::HandshakeRequired)
        } else if hello && is_hello {
            Err(ControlError::InvalidRequest)
        } else {
            let (reply, rx) = mpsc::sync_channel(1);
            match core.try_send(Job {
                op: request.operation,
                reply,
            }) {
                Ok(()) => rx
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap_or(Err(ControlError::StorageUnavailable)),
                Err(mpsc::TrySendError::Full(_)) => Err(ControlError::CapacityExceeded),
                Err(_) => Err(ControlError::StorageUnavailable),
            }
        };
        let success = outcome.is_ok();
        let response = Response {
            request_id: request.request_id,
            authority_instance_id: instance.clone(),
            outcome,
        };
        let bytes = match response.encode() {
            Ok(b) => b,
            Err(_) => return,
        };
        if write_frame(&mut stream, &bytes, Instant::now() + WRITE_TIMEOUT, &stop).is_err() {
            return;
        }
        if is_hello && success {
            hello = true;
        }
        if !hello {
            return;
        }
    }
}
/// Called only by the independent Linux binary, before starting any worker thread.
pub fn run_from_environment() -> ControlResult<()> {
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGTERM);
    mask.add(Signal::SIGINT);
    mask.thread_block()
        .map_err(|_| ControlError::InternalError)?;
    let mut signals = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC)
        .map_err(|_| ControlError::InternalError)?;
    let paths = Paths::from_environment()?;
    let _locks = paths.lock()?;
    let instance = uuid();
    let mut store = Store::open(&paths.state.join("control.sqlite3"), instance.clone())?;
    let (listener, _socket) = bind(&paths.runtime.join("control.sock"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::sync_channel::<Job>(QUEUE);
    let core_stop = stop.clone();
    let core = thread::spawn(move || {
        while let Ok(job) = receiver.recv() {
            let result = if core_stop.load(Ordering::Acquire) {
                Err(ControlError::StorageUnavailable)
            } else {
                store.apply(&job.op)
            };
            let _ = job.reply.try_send(result);
        }
    });
    let mut workers: Vec<thread::JoinHandle<()>> = Vec::new();
    let result = (|| -> ControlResult<()> {
        loop {
            let mut fds = [
                PollFd::new(listener.as_fd(), PollFlags::POLLIN),
                PollFd::new(signals.as_fd(), PollFlags::POLLIN),
            ];
            match poll(&mut fds, None::<u16>) {
                Ok(_) => (),
                Err(Errno::EINTR) => continue,
                Err(_) => return Err(ControlError::TransportError),
            }
            if signals
                .read_signal()
                .map_err(|_| ControlError::InternalError)?
                .is_some()
            {
                break;
            }
            // Bound work per wake so a connection flood cannot starve signal handling.
            for _ in 0..MAX_CLIENTS {
                let (stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => return Err(ControlError::TransportError),
                };
                let mut live = Vec::new();
                for h in workers.drain(..) {
                    if h.is_finished() {
                        let _ = h.join();
                    } else {
                        live.push(h);
                    }
                }
                workers = live;
                if workers.len() == MAX_CLIENTS {
                    drop(stream);
                    continue;
                }
                let tx = sender.clone();
                let instance = instance.clone();
                let stop = stop.clone();
                workers.push(thread::spawn(move || {
                    connection(stream, tx, instance, stop)
                }));
            }
        }
        Ok(())
    })();
    stop.store(true, Ordering::Release);
    drop(listener);
    for h in workers {
        let _ = h.join();
    }
    drop(sender);
    core.join().map_err(|_| ControlError::InternalError)?;
    result
}
