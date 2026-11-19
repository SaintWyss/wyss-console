//! The host process: a Unix socket, one request per connection, served by the core.
//! Private to the user: the directory is 0700, the socket 0600, and the peer's uid must be
//! this process's. It exits when it has held nothing for a while (a host with no terminal and
//! no running agent has no reason to stay) or when told to.
use super::{
    core::HostCore,
    protocol::{Answer, HostError, Request, read_frame, write_frame},
};
use nix::poll::{PollFd, PollFlags, poll};
use std::{
    io,
    os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Most connections served at once; past it the oldest one still silent is closed.
pub const MAX_CONNECTIONS: usize = 64;
/// Give the evicted reader a short chance to release its place for the incoming connection.
const EVICTION_WAIT: Duration = Duration::from_millis(50);
/// A whole request frame has this long to arrive, however slowly its bytes drip in.
pub const FRAME_DEADLINE: Duration = Duration::from_secs(5);

/// A host with nothing to hold for this long exits.
pub const IDLE_EXIT: Duration = Duration::from_secs(10 * 60);

/// `$XDG_RUNTIME_DIR/wyss-console/host.sock`.
pub fn socket_path() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())?;
    Some(runtime.join("wyss-console").join("host.sock"))
}

/// The peer process runs as this user.
pub fn peer_is_me(stream: &UnixStream) -> bool {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    getsockopt(stream, PeerCredentials)
        .is_ok_and(|credentials| credentials.uid() == nix::unistd::geteuid().as_raw())
}

/// Opens the directory without following its final component, keeping its identity pinned.
fn open_dir_nofollow(dir: &Path) -> io::Result<OwnedFd> {
    use nix::fcntl::{OFlag, open};
    let fd = open(
        dir,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(io::Error::from)?;
    // SAFETY: `open` just returned this descriptor and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn dir_owner_mode(fd: &OwnedFd) -> io::Result<(u32, u32)> {
    let st = nix::sys::stat::fstat(fd.as_raw_fd()).map_err(io::Error::from)?;
    Ok((st.st_uid, st.st_mode & 0o7777))
}

fn anchored_socket_path(dir: &OwnedFd, name: &std::ffi::OsStr) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd())).join(name)
}

/// The directory holding the socket, when it exists, is ours and closed to group and others.
/// A symlink in its place is refused.
pub fn check_socket_dir(dir: &Path) -> io::Result<()> {
    match open_dir_nofollow(dir).and_then(|fd| dir_owner_mode(&fd)) {
        Ok((uid, mode)) => {
            if uid != nix::unistd::geteuid().as_raw() || mode & 0o077 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("unsafe socket directory {}", dir.display()),
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("unsafe socket directory {}: {error}", dir.display()),
        )),
    }
}

/// `XDG_RUNTIME_DIR` is a directory of ours with mode 0700, and not a symlink.
pub fn check_runtime_dir() -> io::Result<()> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::PermissionDenied, what.to_owned());
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| bad("XDG_RUNTIME_DIR is not absolute"))?;
    check_private_dir(&runtime)
}

fn check_private_dir(runtime: &Path) -> io::Result<()> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::PermissionDenied, what.to_owned());
    let (uid, mode) = open_dir_nofollow(runtime).and_then(|fd| dir_owner_mode(&fd))
        .map_err(|_| bad("XDG_RUNTIME_DIR is not a private directory of this user"))?;
    if uid != nix::unistd::geteuid().as_raw() || mode & 0o777 != 0o700 {
        return Err(bad("XDG_RUNTIME_DIR is not a private directory of this user"));
    }
    Ok(())
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
    let fd = open_dir_nofollow(dir).map_err(|error| match error.raw_os_error() {
        Some(nix::libc::ELOOP | nix::libc::ENOTDIR) => BindError::Unsafe,
        _ => BindError::Io(error),
    })?;
    let (uid, mode) = dir_owner_mode(&fd).map_err(BindError::Io)?;
    if uid != nix::unistd::geteuid().as_raw() {
        return Err(BindError::Unsafe);
    }
    if mode & 0o077 != 0 {
        nix::sys::stat::fchmod(fd.as_raw_fd(), nix::sys::stat::Mode::from_bits_truncate(0o700))
            .map_err(|error| BindError::Io(error.into()))?;
    }
    let anchored = anchored_socket_path(&fd, path.file_name().ok_or(BindError::Unsafe)?);
    bind_socket(&anchored)
}

fn bind_socket(path: &Path) -> Result<UnixListener, BindError> {
    if let Ok(existing) = std::fs::symlink_metadata(path) {
        if !existing.file_type().is_socket() {
            return Err(BindError::Unsafe);
        }
        match UnixStream::connect(path) {
            Ok(_) => return Err(BindError::AlreadyRunning),
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                std::fs::remove_file(path).map_err(BindError::Io)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(BindError::Io(error)),
        }
    }
    // The socket is born 0600, with no window between bind and chmod.
    let old = nix::sys::stat::umask(nix::sys::stat::Mode::from_bits_truncate(0o077));
    let bound = UnixListener::bind(path);
    nix::sys::stat::umask(old);
    let listener = bound.map_err(BindError::Io)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(BindError::Io)?;
    Ok(listener)
}

/// Reads with one deadline for everything, not one timeout per read.
struct Deadline<'a> {
    stream: &'a UnixStream,
    end: Instant,
}

impl io::Read for Deadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        io::Read::read(&mut &*self.stream, buf)
    }
}

#[derive(Default)]
struct Conns {
    active: AtomicUsize,
    /// Long polls (`term_read` that waits) being served right now.
    long: AtomicUsize,
    next: AtomicU64,
    /// Connections still waiting for their request, oldest first.
    reading: Mutex<Vec<(u64, UnixStream)>>,
    freed: Condvar,
}

/// A connection's place in the cap, released when it drops: also when the worker thread could
/// not be created (the closure that owns it is dropped) or the request never arrived.
struct Slot {
    conns: Arc<Conns>,
    id: u64,
}

impl Conns {
    fn admit(self: &Arc<Self>, stream: &UnixStream) -> Option<Slot> {
        let copy = stream.try_clone().ok()?;
        let mut reading = self.reading.lock().ok()?;
        if self.active.load(Ordering::Acquire) >= MAX_CONNECTIONS {
            if reading.is_empty() {
                return None;
            }
            let (_, old) = reading.remove(0);
            let _ = old.shutdown(std::net::Shutdown::Both);
            (reading, _) = self
                .freed
                .wait_timeout_while(reading, EVICTION_WAIT, |_| {
                    self.active.load(Ordering::Acquire) >= MAX_CONNECTIONS
                })
                .ok()?;
            if self.active.load(Ordering::Acquire) >= MAX_CONNECTIONS {
                return None;
            }
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        reading.push((id, copy));
        self.active.fetch_add(1, Ordering::AcqRel);
        Some(Slot {
            conns: self.clone(),
            id,
        })
    }
}

impl Slot {
    fn finish_reading(&self) -> bool {
        let Ok(mut reading) = self.conns.reading.lock() else {
            return false;
        };
        let Some(index) = reading.iter().position(|(id, _)| *id == self.id) else {
            return false;
        };
        reading.remove(index);
        true
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut reading = self.conns.reading.lock().unwrap_or_else(|e| e.into_inner());
        reading.retain(|(id, _)| *id != self.id);
        self.conns.active.fetch_sub(1, Ordering::AcqRel);
        self.conns.freed.notify_one();
    }
}

struct LongPoll<'a>(&'a Conns);
impl Drop for LongPoll<'_> {
    fn drop(&mut self) {
        self.0.long.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Reserve `MAX_CONNECTIONS - MAX_LONG_POLLS` places for new connections and short calls.
pub const MAX_LONG_POLLS: usize = MAX_CONNECTIONS - 16;

/// Takes a place for a waiting `term_read`. A saturated caller must back off.
fn admit_long_poll<'a>(conns: &'a Conns, request: &Request) -> Result<Option<LongPoll<'a>>, HostError> {
    if request.op != "term_read"
        || request.params.get("wait_ms").and_then(serde_json::Value::as_u64).unwrap_or(0) == 0
    {
        return Ok(None);
    }
    conns
        .long
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
            (active < MAX_LONG_POLLS).then_some(active + 1)
        })
        .map_err(|_| HostError::new("busy"))?;
    Ok(Some(LongPoll(conns)))
}

fn serve(mut stream: UnixStream, core: &Arc<HostCore>, stop: &AtomicBool, slot: &Slot) {
    let conns = &slot.conns;
    if !peer_is_me(&stream) {
        return;
    }
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let request = {
        let mut reader = Deadline { stream: &stream, end: Instant::now() + FRAME_DEADLINE };
        read_frame::<Request>(&mut reader)
    };
    if !slot.finish_reading() {
        return;
    }
    let Ok(request) = request else {
        let _ = write_frame(&mut stream, &Answer::Err(HostError::new("request_invalid")));
        return;
    };
    let _long = match admit_long_poll(conns, &request) {
        Ok(slot) => slot,
        Err(error) => {
            let _ = write_frame(&mut stream, &Answer::Err(error));
            return;
        }
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
    let conns = Arc::new(Conns::default());
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let Some(slot) = conns.admit(&stream) else {
                    continue;
                };
                let (core, stop) = (core.clone(), stop.clone());
                // If the thread cannot start, the closure (and so the slot, the stream and its
                // registered copy) is dropped here and the place is free again.
                let _ = thread::Builder::new()
                    .name("wyss-host-conn".into())
                    .spawn(move || serve(stream, &core, &stop, &slot));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if core.idle_for().is_some_and(|idle| idle >= idle_exit) {
                    break;
                }
                // Wait for the listener to become readable. The timeout lets the loop check
                // for a stop request or idle exit at least every 250 ms.
                let mut ready = [PollFd::new(listener.as_fd(), PollFlags::POLLIN)];
                let _ = poll(&mut ready, 250_u16);
            }
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
    core.shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dropping_a_slot_frees_its_place_and_its_registered_stream() {
        let conns = Arc::new(Conns::default());
        let (a, b) = UnixStream::pair().unwrap();
        let slot = conns.admit(&a).unwrap();
        drop(a);
        // The worker thread never started: the closure owning the slot is just dropped.
        drop(slot);
        assert_eq!(conns.active.load(Ordering::Acquire), 0);
        assert!(conns.reading.lock().unwrap().is_empty());
        // The registered copy is gone, so the peer sees the end of the stream.
        let mut buf = [0u8; 1];
        assert_eq!(io::Read::read(&mut &b, &mut buf).unwrap(), 0);
    }

    #[test]
    fn host_long_poll_reports_busy_at_the_limit() {
        let conns = Conns::default();
        let request = Request {
            op: "term_read".into(),
            params: json!({"terminal_id": "t", "from": 0, "wait_ms": 5000}),
        };
        let held: Vec<_> = (0..MAX_LONG_POLLS)
            .map(|_| admit_long_poll(&conns, &request).unwrap().unwrap())
            .collect();
        assert_eq!(admit_long_poll(&conns, &request).err().unwrap().code, "busy");
        assert_eq!(conns.long.load(Ordering::Acquire), MAX_LONG_POLLS);
        let short = Request { op: "hello".into(), params: json!(null) };
        assert!(admit_long_poll(&conns, &short).unwrap().is_none());
        drop(held);
        assert_eq!(conns.long.load(Ordering::Acquire), 0);
        assert!(admit_long_poll(&conns, &request).unwrap().is_some());
    }

    #[test]
    fn host_socket_anchor_survives_a_directory_swap() {
        let root = std::env::temp_dir().join(format!("wyss-socket-parent-{}", uuid::Uuid::new_v4()));
        let dir = root.join("socket-dir");
        let moved = root.join("saved");
        std::fs::create_dir_all(&dir).unwrap();
        let fd = open_dir_nofollow(&dir).unwrap();
        std::fs::rename(&dir, &moved).unwrap();
        std::fs::create_dir(&dir).unwrap();
        let anchor = anchored_socket_path(&fd, std::ffi::OsStr::new("host.sock"));
        std::fs::write(anchor, b"fixture").unwrap();
        assert!(!dir.join("host.sock").exists());
        assert_eq!(std::fs::read(moved.join("host.sock")).unwrap(), b"fixture");
    }

    #[test]
    fn host_eviction_keeps_the_old_slot_until_worker_drop() {
        let conns = Arc::new(Conns::default());
        let (old, peer) = UnixStream::pair().unwrap();
        let (incoming, _incoming_peer) = UnixStream::pair().unwrap();
        let old_slot = conns.admit(&old).unwrap();
        let slots: Vec<_> = (1..MAX_CONNECTIONS)
            .map(|_| conns.admit(&incoming).unwrap())
            .collect();

        assert!(
            conns.admit(&incoming).is_none(),
            "the evicted worker still holds its slot"
        );
        assert_eq!(conns.active.load(Ordering::Acquire), MAX_CONNECTIONS);
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(io::Read::read(&mut &peer, &mut buf).unwrap(), 0);
        assert!(
            !old_slot.finish_reading(),
            "the evicted worker cannot act on a request"
        );

        drop(old_slot);
        assert_eq!(conns.active.load(Ordering::Acquire), MAX_CONNECTIONS - 1);
        let new = conns.admit(&incoming).unwrap();
        assert_eq!(conns.active.load(Ordering::Acquire), MAX_CONNECTIONS);
        drop((new, slots));
        assert_eq!(conns.active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn host_sends_a_busy_error_frame() {
        let conns = Arc::new(Conns::default());
        conns.long.store(MAX_LONG_POLLS, Ordering::Release);
        let (server, mut client) = UnixStream::pair().unwrap();
        let slot = conns.admit(&server).unwrap();
        write_frame(&mut client, &Request {
            op: "term_read".into(),
            params: json!({"terminal_id": "absent", "wait_ms": 5000}),
        }).unwrap();
        serve(server, &HostCore::new(None), &AtomicBool::new(false), &slot);
        assert!(matches!(read_frame::<Answer>(&mut client).unwrap(), Answer::Err(error) if error.code == "busy"));
    }

    #[test]
    fn bound_socket_remains_reachable_by_the_public_path() {
        let dir = std::env::temp_dir().join(format!("bind-{}", uuid::Uuid::new_v4().simple()));
        let socket = dir.join("host.sock");
        let _first = bind(&socket).unwrap_or_else(|_| panic!("could not bind the fixture socket"));
        assert!(UnixStream::connect(&socket).is_ok());
        assert!(matches!(bind(&socket), Err(BindError::AlreadyRunning)));
        assert_eq!(std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn a_symlinked_directory_is_refused() {
        let base = std::env::var_os("WYSS_TEST_SCRATCH")
            .map_or_else(std::env::temp_dir, PathBuf::from)
            .join(format!("wyss-symlink-{}", std::process::id()));
        let real = base.join("real");
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&real).unwrap();
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let ok = (check_socket_dir(&real), check_private_dir(&real));
        let through_link = (check_socket_dir(&link), check_private_dir(&link));
        let _ = std::fs::remove_dir_all(&base);
        ok.0.unwrap();
        ok.1.unwrap();
        assert!(through_link.0.is_err());
        assert!(through_link.1.is_err());
    }
}
