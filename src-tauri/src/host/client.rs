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

/// The request never left whole: the host cannot have acted on it.
pub const HOST_UNAVAILABLE: &str = "host_unavailable";
/// The whole request was written and no answer came back: the host may have acted on it, once,
/// or not at all. Nothing here can tell which, and protocol 1 has no receipt to ask for.
pub const HOST_OUTCOME_UNKNOWN: &str = "host_outcome_unknown";

/// The operations that may be asked again after an answer was lost: each one only reads what
/// the host holds and changes nothing in it. The list is closed and explicit. An operation that
/// is not named here, including one this build has never heard of, is treated as one that
/// changes something, and is never sent twice.
const RETRY_SAFE_READS: [&str; 14] = [
    "hello",
    "term_status",
    "term_read",
    "term_list",
    "term_screen",
    "term_parser",
    "host_persistence",
    "native_launcher",
    "native_launcher_extended",
    "profiles",
    "profiles_extended",
    "delegate_get",
    "delegate_list",
    "delegate_capabilities",
];

/// Whether a lost answer to `op` may be followed by asking again.
pub fn retry_safe(op: &str) -> bool {
    RETRY_SAFE_READS.contains(&op)
}

pub struct Remote {
    path: PathBuf,
}

impl Remote {
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// One request, one answer. A failure before the whole frame was written is
    /// `host_unavailable`: the host reads a complete length-prefixed frame before it acts, so a
    /// partial one does nothing. A failure after that is `host_outcome_unknown`.
    pub fn call(&self, op: &str, params: Value) -> Result<Value, HostError> {
        let unsent = |_: std::io::Error| HostError::new(HOST_UNAVAILABLE);
        let mut stream = UnixStream::connect(&self.path).map_err(unsent)?;
        stream
            .set_read_timeout(Some(CALL_TIMEOUT))
            .map_err(unsent)?;
        stream
            .set_write_timeout(Some(CALL_TIMEOUT))
            .map_err(unsent)?;
        write_frame(
            &mut stream,
            &Request {
                op: op.to_owned(),
                params,
            },
        )
        .map_err(unsent)?;
        match read_frame::<Answer>(&mut stream).map_err(|_| HostError::new(HOST_OUTCOME_UNKNOWN))? {
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
    /// A test's own socket: the link looks for a host there and nowhere else, and never starts
    /// a process. Absent in the product, where the user's runtime directory is used.
    #[cfg(test)]
    fixed: Option<PathBuf>,
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
            #[cfg(test)]
            fixed: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn remote_for_test(path: PathBuf) -> Self {
        Self {
            mode: Arc::new(Mutex::new(Some(Arc::new(Mode::Remote(Remote::at(
                path.clone(),
            )))))),
            fixed: Some(path),
        }
    }

    fn connect(&self) -> Arc<Mode> {
        #[cfg(test)]
        if let Some(path) = &self.fixed {
            let remote = Remote::at(path.clone());
            return Arc::new(if hello(&remote).is_some() {
                Mode::Remote(remote)
            } else {
                Mode::Local(HostCore::new(None))
            });
        }
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

    /// One operation through the host in use.
    ///
    /// * The request never left (`host_unavailable`): the host went away before it could act.
    ///   Another is found or started and the request is sent there, once. What lived in the
    ///   old one is gone, and the answer to a stale id says so.
    /// * The request left and no answer came (`host_outcome_unknown`): an operation on the
    ///   closed list of reads is asked again, once. Anything else is answered as it is, with
    ///   that word: it is not sent again, the host in use is not replaced for it, and it is
    ///   not run in a host of the window's own, because the first one may have run it.
    pub fn call(&self, op: &str, params: Value) -> Result<Value, HostError> {
        match &*self.mode() {
            Mode::Local(core) => core.call(op, params),
            Mode::Remote(remote) => match remote.call(op, params.clone()) {
                Err(error)
                    if error.code == HOST_UNAVAILABLE
                        || (error.code == HOST_OUTCOME_UNKNOWN && retry_safe(op)) =>
                {
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
    pub fn call_attached(&self, op: &str, params: Value) -> Result<Value, HostError> {
        let mode = self
            .mode
            .try_lock()
            .map_err(|_| HostError::new(HOST_UNAVAILABLE))?
            .clone()
            .ok_or_else(|| HostError::new(HOST_UNAVAILABLE))?;
        match &*mode {
            Mode::Local(core) => core.call(op, params),
            Mode::Remote(remote) => remote.call(op, params),
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

#[cfg(test)]
mod uncertainty_tests {
    //! ENG-02. A synthetic host this test owns: it counts every complete request it reads and
    //! every effect it "performs", and can be told to drop the answer to an operation. No
    //! installed host, service or process outside this test is contacted or signalled.
    use super::*;
    use std::{
        collections::HashSet,
        io::Read,
        os::unix::net::UnixListener,
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };

    struct Synthetic {
        socket: PathBuf,
        epoch: u32,
        seen: Arc<Mutex<Vec<String>>>,
        effects: Arc<Mutex<Vec<String>>>,
        drop_reply: Arc<Mutex<HashSet<String>>>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    fn fresh_socket() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wyss-eng02-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("host.sock")
    }

    impl Synthetic {
        /// `epoch` stands for the host process: a replacement answers with another one.
        fn start(socket: PathBuf, epoch: u32) -> Self {
            let _ = std::fs::remove_file(&socket);
            let listener = UnixListener::bind(&socket).unwrap();
            listener.set_nonblocking(true).unwrap();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let effects = Arc::new(Mutex::new(Vec::new()));
            let drop_reply = Arc::new(Mutex::new(HashSet::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (s, e, d, st) = (
                seen.clone(),
                effects.clone(),
                drop_reply.clone(),
                stop.clone(),
            );
            let thread = thread::spawn(move || {
                while !st.load(Ordering::Acquire) {
                    let mut stream = match listener.accept() {
                        Ok((stream, _)) => stream,
                        Err(_) => {
                            thread::sleep(Duration::from_millis(2));
                            continue;
                        }
                    };
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let Ok(request) = read_frame::<Request>(&mut stream) else {
                        continue; // an incomplete frame is not a request: nothing is done
                    };
                    s.lock().unwrap().push(request.op.clone());
                    let answer = match request.op.as_str() {
                        "hello" => Answer::Ok(json!({"protocol": PROTOCOL, "pid": epoch})),
                        "term_list" => Answer::Ok(json!([])),
                        "term_create" | "term_write" | "native_start" | "from_the_future" => {
                            e.lock().unwrap().push(request.op.clone());
                            Answer::Ok(json!({"done_by": epoch}))
                        }
                        _ => Answer::Err(HostError::new("unknown_op")),
                    };
                    // The effect above happened either way; only the answer is lost.
                    if d.lock().unwrap().remove(&request.op) {
                        continue;
                    }
                    let _ = write_frame(&mut stream, &answer);
                }
            });
            Self {
                socket,
                epoch,
                seen,
                effects,
                drop_reply,
                stop,
                thread: Some(thread),
            }
        }
        fn lose_next_answer_to(&self, op: &str) {
            self.drop_reply.lock().unwrap().insert(op.to_owned());
        }
        fn sent(&self, op: &str) -> usize {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|o| *o == op)
                .count()
        }
        fn effects(&self) -> Vec<String> {
            self.effects.lock().unwrap().clone()
        }
        fn link(&self) -> HostLink {
            HostLink::remote_for_test(self.socket.clone())
        }
        fn end(mut self) -> PathBuf {
            self.halt();
            self.socket.clone()
        }
        fn halt(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            let _ = std::fs::remove_file(&self.socket);
        }
    }
    impl Drop for Synthetic {
        fn drop(&mut self) {
            self.halt();
            let _ = std::fs::remove_dir_all(self.socket.parent().unwrap());
        }
    }

    #[test]
    fn a_mutation_whose_answer_is_lost_is_sent_once_and_reported_as_unknown() {
        for op in ["term_create", "term_write", "native_start"] {
            let host = Synthetic::start(fresh_socket(), 1);
            let link = host.link();
            host.lose_next_answer_to(op);
            let error = link.call(op, json!({})).unwrap_err();
            assert_eq!(error.code, HOST_OUTCOME_UNKNOWN, "{op}");
            assert_eq!(host.sent(op), 1, "{op} was sent again");
            assert_eq!(
                host.effects(),
                vec![op.to_owned()],
                "{op} ran more than once"
            );
            // The host that may have done it is still the one in use: it was not replaced.
            assert!(link.is_remote());
            assert_eq!(link.call("hello", Value::Null).unwrap()["pid"], json!(1));
            assert_eq!(host.epoch, 1);
        }
    }

    #[test]
    fn an_operation_this_build_does_not_know_is_treated_as_a_mutation() {
        assert!(!retry_safe("from_the_future"));
        assert!(!retry_safe("term_resize") && !retry_safe("term_close"));
        assert!(!retry_safe("native_stop") && !retry_safe("profile_save"));
        let host = Synthetic::start(fresh_socket(), 1);
        let link = host.link();
        host.lose_next_answer_to("from_the_future");
        assert_eq!(
            link.call("from_the_future", json!({})).unwrap_err().code,
            HOST_OUTCOME_UNKNOWN
        );
        assert_eq!(host.sent("from_the_future"), 1);
        assert_eq!(host.effects().len(), 1);
    }

    #[test]
    fn a_read_on_the_closed_list_is_asked_again_once_after_a_lost_answer() {
        let host = Synthetic::start(fresh_socket(), 1);
        let link = host.link();
        host.lose_next_answer_to("term_list");
        assert_eq!(link.call("term_list", Value::Null).unwrap(), json!([]));
        assert_eq!(host.sent("term_list"), 2);
        assert!(host.effects().is_empty());
    }

    #[test]
    fn a_host_replaced_after_a_lost_answer_never_receives_the_mutation() {
        let first = Synthetic::start(fresh_socket(), 1);
        let link = first.link();
        first.lose_next_answer_to("term_create");
        assert_eq!(
            link.call("term_create", json!({})).unwrap_err().code,
            HOST_OUTCOME_UNKNOWN
        );
        assert_eq!(first.effects(), vec!["term_create".to_owned()]);
        let dir = first.socket.parent().unwrap().to_owned();
        let socket = first.end();
        std::fs::create_dir_all(&dir).unwrap();
        // Another host (another epoch) answers on the same socket.
        let second = Synthetic::start(socket, 2);
        // Later work goes to the host that is there now; the uncertain create is not replayed.
        assert_eq!(link.call("hello", Value::Null).unwrap()["pid"], json!(2));
        assert_eq!(second.sent("term_create"), 0);
        assert!(second.effects().is_empty());
    }

    #[test]
    fn a_request_that_never_left_is_known_unsent_and_may_go_to_the_host_found_next() {
        // Nobody listens: the frame is never written.
        let socket = fresh_socket();
        assert_eq!(
            Remote::at(socket.clone())
                .call("term_create", json!({}))
                .unwrap_err()
                .code,
            HOST_UNAVAILABLE
        );
        // A host appears before the link looks again: the unsent request reaches it, once.
        let link = HostLink::remote_for_test(socket.clone());
        let host = Synthetic::start(socket, 3);
        assert_eq!(
            link.call("term_create", json!({})).unwrap()["done_by"],
            json!(3)
        );
        assert_eq!(host.sent("term_create"), 1);
        assert_eq!(host.effects().len(), 1);
    }

    #[test]
    fn with_no_host_left_an_unsent_request_is_answered_by_the_windows_own_host() {
        let socket = fresh_socket();
        let link = HostLink::remote_for_test(socket.clone());
        // The window's own host has no such terminal and says so; it is a definite answer.
        let error = link
            .call(
                "term_write",
                json!({"terminal_id": "0".repeat(32), "data": [97]}),
            )
            .unwrap_err();
        assert_ne!(error.code, HOST_OUTCOME_UNKNOWN);
        assert_ne!(error.code, HOST_UNAVAILABLE);
        assert!(!link.is_remote());
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    #[test]
    fn a_frame_cut_short_while_being_written_is_known_unsent_and_does_nothing() {
        // The peer takes a few bytes of a frame far larger than the socket buffer and hangs up.
        let socket = fresh_socket();
        let listener = UnixListener::bind(&socket).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut few = [0u8; 16];
            let _ = stream.read_exact(&mut few);
            // Dropping the stream closes it with most of the frame unread.
        });
        let big = "x".repeat(super::super::protocol::MAX_FRAME - 4096);
        let error = Remote::at(socket.clone())
            .call("term_write", json!({ "data": big }))
            .unwrap_err();
        peer.join().unwrap();
        assert_eq!(error.code, HOST_UNAVAILABLE);
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    #[test]
    fn a_whole_frame_followed_by_a_hang_up_is_unknown_not_unsent() {
        let socket = fresh_socket();
        let listener = UnixListener::bind(&socket).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request: Request = read_frame(&mut stream).unwrap();
            request.op
        });
        let error = Remote::at(socket.clone())
            .call("term_write", json!({"data": [97]}))
            .unwrap_err();
        assert_eq!(peer.join().unwrap(), "term_write");
        assert_eq!(error.code, HOST_OUTCOME_UNKNOWN);
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }
}
