//! The host's side of one parser helper: starts it, feeds it the terminal's output from the
//! host's own byte ring, asks it for snapshots, and keeps it alive.
//!
//! The PTY and its ring belong to the host; this link only *reads* the ring and talks to a helper.
//! Whatever goes wrong here (a helper that dies, hangs, lies or is killed for memory) is contained:
//! the link kills its own child, starts another and replays what the ring still holds, and says
//! truthfully how much of the history it could not. It never writes to the PTY and never replays a
//! command: replay goes to the parser only.

use super::ipc::{
    self, IpcError, MAX_FEED, MAX_REQUEST, MAX_RESPONSE, PROTOCOL, Request, Response, SnapshotBody,
    layer,
};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Where the bytes come from: the host's ring for one terminal.
pub trait ByteSource: Send + Sync + 'static {
    /// Offset of the oldest byte still kept.
    fn start(&self) -> u64;
    /// Offset after the newest byte.
    fn end(&self) -> u64;
    /// Up to `max` bytes from `from` (which is not before `start`).
    fn read(&self, from: u64, max: usize) -> Vec<u8>;
}

pub trait ChildHandle: Send {
    fn pid(&self) -> Option<u32>;
    /// Ends the helper through the handle this link got when it started it.
    fn kill(&mut self);
    fn reap(&mut self);
}

pub struct Spawned {
    pub input: Box<dyn Write + Send>,
    pub output: Box<dyn Read + Send>,
    pub handle: Box<dyn ChildHandle>,
}

pub trait Spawner: Send + Sync + 'static {
    fn spawn(&self) -> std::io::Result<Spawned>;
}

/// Starts the real helper executable: no environment, `/` as its directory, nothing inherited but
/// the two pipes, and the kernel ends it if this process dies.
pub struct ProcessSpawner {
    pub path: std::path::PathBuf,
    pub require: u8,
}

struct ProcessChild(std::process::Child);
impl ChildHandle for ProcessChild {
    fn pid(&self) -> Option<u32> {
        Some(self.0.id())
    }
    fn kill(&mut self) {
        let _ = self.0.kill();
    }
    fn reap(&mut self) {
        let _ = self.0.wait();
    }
}

impl Spawner for ProcessSpawner {
    fn spawn(&self) -> std::io::Result<Spawned> {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new(&self.path);
        command
            .arg("--require")
            .arg(self.require.to_string())
            .env_clear()
            .current_dir("/")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        // SAFETY: only async-signal-safe calls (prctl, setsid) between fork and exec.
        unsafe {
            command.pre_exec(|| {
                nix::libc::prctl(nix::libc::PR_SET_PDEATHSIG, nix::libc::SIGKILL, 0, 0, 0);
                nix::libc::setsid();
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        let input = child.stdin.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
        let output = child.stdout.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
        Ok(Spawned {
            input: Box::new(input),
            output: Box::new(output),
            handle: Box::new(ProcessChild(child)),
        })
    }
}

#[derive(Clone, Debug)]
pub struct LinkConfig {
    pub scrollback_lines: u32,
    pub width_table: u8,
    /// Layers the helper must have applied; a helper without them is not used.
    pub require: u8,
    /// At most `max_restarts` starts of a helper within `restart_window`.
    pub max_restarts: u32,
    pub restart_window: Duration,
    /// How long one exchange with the helper may take before it is killed.
    pub exchange: Duration,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            scrollback_lines: 2000,
            width_table: 0,
            require: layer::ALL,
            max_restarts: 3,
            restart_window: Duration::from_secs(60),
            exchange: Duration::from_secs(3),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Starting,
    Healthy,
    /// A helper was lost and the next one is being brought up.
    Restarting,
    /// No screen can be offered; the closed word says why.
    Unavailable(&'static str),
}

/// What a snapshot can claim about the screen it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fidelity {
    /// The parser has consumed every byte the terminal ever printed.
    Exact,
    /// The parser was rebuilt from the bytes the host still keeps: `missing` bytes at the start of
    /// the history (and whatever state they set) are not in the screen.
    Tail {
        missing: u64,
    },
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub health: Health,
    pub fidelity: Fidelity,
    pub restarts: u32,
    pub last_failure: Option<&'static str>,
    /// `(layers applied, landlock abi)` of the helper in service.
    pub sandbox: Option<(u8, u8)>,
    pub helper_pid: Option<u32>,
    /// Offset after the last byte the screen consumed.
    pub fed: u64,
}

pub struct Snap {
    pub body: SnapshotBody,
    pub status: Status,
    /// The offset in the terminal's output where this screen is: bytes from here on are the
    /// ones a window that draws it must still read.
    pub at: u64,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LinkError {
    Unavailable(&'static str),
    Timeout,
}

enum Command {
    Wake,
    Snapshot {
        scrollback: u32,
        reply: SyncSender<Result<Snap, LinkError>>,
    },
    Stop,
}

pub struct ScreenLink {
    commands: SyncSender<Command>,
    shared: Arc<Mutex<Status>>,
    /// The PTY's size as the host last set it. A command could be lost to a full queue; a value
    /// cannot: the worker compares it with what its helper has at every step.
    size: Arc<Mutex<(u16, u16)>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ScreenLink {
    pub fn start(
        spawner: Arc<dyn Spawner>,
        source: Arc<dyn ByteSource>,
        size: (u16, u16),
        config: LinkConfig,
    ) -> ScreenLink {
        let shared = Arc::new(Mutex::new(Status {
            health: Health::Starting,
            fidelity: Fidelity::Unavailable,
            restarts: 0,
            last_failure: None,
            sandbox: None,
            helper_pid: None,
            fed: 0,
        }));
        let (tx, rx) = mpsc::sync_channel(64);
        let want = Arc::new(Mutex::new(size));
        let worker = Worker {
            spawner,
            source,
            config,
            want: want.clone(),
            have: size,
            shared: shared.clone(),
            commands: rx,
            session: None,
            next: 0,
            starts: VecDeque::new(),
        };
        let thread = thread::Builder::new()
            .name("wyss-screen-link".into())
            .spawn(move || worker.run())
            .ok();
        ScreenLink {
            commands: tx,
            shared,
            size: want,
            thread: Mutex::new(thread),
        }
    }

    /// New output is in the ring. Never blocks; a link that is behind already knows.
    pub fn wake(&self) {
        let _ = self.commands.try_send(Command::Wake);
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        *lock(&self.size) = (rows, cols);
        self.wake();
    }

    pub fn status(&self) -> Status {
        lock(&self.shared).clone()
    }

    pub fn snapshot(&self, scrollback: u32, wait: Duration) -> Result<Snap, LinkError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.commands
            .try_send(Command::Snapshot { scrollback, reply })
            .map_err(|_| LinkError::Unavailable("busy"))?;
        match answer.recv_timeout(wait) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(LinkError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(LinkError::Unavailable("stopped")),
        }
    }
}

impl Drop for ScreenLink {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(t) = lock(&self.thread).take() {
            let _ = t.join();
        }
    }
}

// ---- the worker thread -----------------------------------------------------------------------

enum Fail {
    /// The helper is gone, hung, refused, lied or ended: it is replaced.
    Helper(&'static str),
}

struct Session {
    /// Frames to the helper go through a thread of their own, so that a helper that stopped
    /// reading cannot stop this one from enforcing its deadline.
    out: Sender<Vec<u8>>,
    responses: Receiver<Result<Response, IpcError>>,
    handle: Box<dyn ChildHandle>,
    threads: Vec<JoinHandle<()>>,
}

impl Session {
    fn end(mut self) {
        self.handle.kill();
        self.handle.reap();
        drop(self.out);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

struct Worker {
    spawner: Arc<dyn Spawner>,
    source: Arc<dyn ByteSource>,
    config: LinkConfig,
    /// The size wanted, and the size the helper in service has.
    want: Arc<Mutex<(u16, u16)>>,
    have: (u16, u16),
    shared: Arc<Mutex<Status>>,
    commands: Receiver<Command>,
    session: Option<Session>,
    /// Offset of the next byte to give the helper.
    next: u64,
    starts: VecDeque<Instant>,
}

impl Worker {
    fn set(&self, f: impl FnOnce(&mut Status)) {
        f(&mut lock(&self.shared));
    }

    fn run(mut self) {
        // Start eagerly: a helper is ready before the first snapshot is asked for.
        let _ = self.ensure();
        loop {
            match self.commands.recv_timeout(Duration::from_millis(500)) {
                Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Command::Wake) | Err(RecvTimeoutError::Timeout) => {
                    let _ = self.pump();
                }
                Ok(Command::Snapshot { scrollback, reply }) => {
                    let result = self.snapshot(scrollback);
                    let _ = reply.send(result);
                }
            }
        }
        if let Some(s) = self.session.take() {
            let _ = s.out.send(Request::Shutdown.encode());
            s.end();
        }
        self.set(|s| {
            s.health = Health::Unavailable("stopped");
            s.helper_pid = None;
        });
    }

    fn unavailable(&self) -> Option<&'static str> {
        match lock(&self.shared).health {
            Health::Unavailable(why) => Some(why),
            _ => None,
        }
    }

    /// A helper in service, started (and the history replayed into it) when there is none.
    fn ensure(&mut self) -> Result<(), Fail> {
        if self.session.is_some() {
            return Ok(());
        }
        if let Some(why) = self.unavailable() {
            return Err(Fail::Helper(why));
        }
        // A restart budget: a helper that keeps dying is not restarted for ever.
        let now = Instant::now();
        while self
            .starts
            .front()
            .is_some_and(|t| now.duration_since(*t) > self.config.restart_window)
        {
            self.starts.pop_front();
        }
        if self.starts.len() as u32 > self.config.max_restarts {
            self.set(|s| s.health = Health::Unavailable("restart_budget"));
            return Err(Fail::Helper("restart_budget"));
        }
        let restarts = self.starts.len() as u32;
        self.starts.push_back(now);
        if restarts > 0 {
            thread::sleep(Duration::from_millis(50 * u64::from(restarts)));
        }
        self.set(|s| {
            s.health = if restarts == 0 {
                Health::Starting
            } else {
                Health::Restarting
            };
            s.restarts = restarts;
        });
        let spawned = match self.spawner.spawn() {
            Ok(s) => s,
            Err(_) => {
                self.set(|s| {
                    s.health = Health::Unavailable("spawn_failed");
                    s.last_failure = Some("spawn_failed");
                });
                return Err(Fail::Helper("spawn_failed"));
            }
        };
        let pid = spawned.handle.pid();
        let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
        let (resp_tx, resp_rx) = mpsc::sync_channel(8);
        let mut input = spawned.input;
        let mut output = spawned.output;
        let writer = thread::spawn(move || {
            while let Ok(frame) = out_rx.recv() {
                if ipc::write_frame(&mut input, &frame, MAX_REQUEST).is_err() {
                    break;
                }
            }
        });
        let reader = thread::spawn(move || {
            loop {
                match ipc::read_frame(&mut output, MAX_RESPONSE) {
                    Ok(Some(frame)) => {
                        if resp_tx.send(Response::decode(&frame)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = resp_tx.send(Err(IpcError::Truncated));
                        break;
                    }
                    Err(e) => {
                        let _ = resp_tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        self.session = Some(Session {
            out: out_tx,
            responses: resp_rx,
            handle: spawned.handle,
            threads: vec![writer, reader],
        });
        self.set(|s| s.helper_pid = pid);
        self.have = *lock(&self.want);
        let hello = Request::Hello {
            version: PROTOCOL,
            rows: self.have.0,
            cols: self.have.1,
            scrollback_lines: self.config.scrollback_lines,
            width_table: self.config.width_table,
            require: self.config.require,
        };
        match self.exchange(&hello)? {
            Response::Ready {
                applied,
                landlock_abi,
                ..
            } => {
                if applied & self.config.require != self.config.require {
                    return Err(self.lose("sandbox_unavailable"));
                }
                self.set(|s| s.sandbox = Some((applied, landlock_abi)));
            }
            _ => return Err(self.lose("bad_hello")),
        }
        // The screen starts where the ring starts: from the first byte when nothing was ever lost.
        let start = self.source.start();
        self.next = start;
        self.set(|s| {
            s.fidelity = if start == 0 {
                Fidelity::Exact
            } else {
                Fidelity::Tail { missing: start }
            };
            s.fed = start;
        });
        self.set(|s| s.health = Health::Healthy);
        Ok(())
    }

    /// Drops the helper in service for `why`, to be replaced at the next use.
    fn lose(&mut self, why: &'static str) -> Fail {
        if let Some(s) = self.session.take() {
            s.end();
        }
        self.set(|s| {
            s.last_failure = Some(why);
            s.helper_pid = None;
            s.sandbox = None;
            if !matches!(s.health, Health::Unavailable(_)) {
                s.health = Health::Restarting;
            }
            s.fidelity = Fidelity::Unavailable;
        });
        Fail::Helper(why)
    }

    /// One request, one answer, within the deadline; otherwise the helper is lost.
    fn exchange(&mut self, request: &Request) -> Result<Response, Fail> {
        let deadline = self.config.exchange;
        let Some(session) = self.session.as_ref() else {
            return Err(Fail::Helper("no_helper"));
        };
        if session.out.send(request.encode()).is_err() {
            return Err(self.lose("write_failed"));
        }
        match session.responses.recv_timeout(deadline) {
            Ok(Ok(Response::Refused { .. })) => Err(self.lose("refused")),
            Ok(Ok(r)) => Ok(r),
            Ok(Err(_)) => Err(self.lose("bad_frame_or_exit")),
            Err(RecvTimeoutError::Timeout) => Err(self.lose("timeout")),
            Err(RecvTimeoutError::Disconnected) => Err(self.lose("disconnected")),
        }
    }

    /// Feeds every byte the ring has that the helper has not seen.
    fn pump(&mut self) -> Result<(), Fail> {
        loop {
            self.ensure()?;
            let end = self.source.end();
            if self.next >= end {
                return self.apply_size();
            }
            let start = self.source.start();
            if self.next < start {
                // The ring moved on past bytes the helper never got: rebuild from what is left.
                return Err(self.lose("fell_behind"));
            }
            let bytes = self.source.read(self.next, MAX_FEED);
            if bytes.is_empty() {
                return self.apply_size();
            }
            let len = bytes.len() as u64;
            let offset = self.next;
            match self.exchange(&Request::Feed { offset, bytes })? {
                Response::Fed { end, .. } if end == offset + len => {
                    self.next = end;
                    self.set(|s| s.fed = end);
                }
                _ => return Err(self.lose("bad_ack")),
            }
        }
    }

    /// The helper's size follows the PTY's, after the bytes that came before the change.
    fn apply_size(&mut self) -> Result<(), Fail> {
        let (rows, cols) = *lock(&self.want);
        if (rows, cols) == self.have {
            return Ok(());
        }
        self.exchange(&Request::Resize { rows, cols })?;
        self.have = (rows, cols);
        Ok(())
    }

    fn snapshot(&mut self, scrollback: u32) -> Result<Snap, LinkError> {
        // A snapshot may need a helper replaced first; one retry only.
        for attempt in 0..2 {
            let result = (|| {
                self.pump()?;
                match self.exchange(&Request::Snapshot {
                    scrollback_lines: scrollback,
                })? {
                    Response::Snapshot(body) => Ok(*body),
                    _ => Err(self.lose("bad_ack")),
                }
            })();
            match result {
                Ok(body) => {
                    return Ok(Snap {
                        body,
                        status: lock(&self.shared).clone(),
                        at: self.next,
                    });
                }
                Err(Fail::Helper(why)) => {
                    if self.unavailable().is_some() || attempt == 1 {
                        return Err(LinkError::Unavailable(why));
                    }
                }
            }
        }
        Err(LinkError::Unavailable("failed"))
    }
}

#[cfg(test)]
mod tests;
