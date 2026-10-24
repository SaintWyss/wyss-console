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
    /// Up to `max` bytes from `from`, in one atomic look at the ring: the offset of the first byte
    /// actually returned (later than `from` when the ring moved on in the meantime) and the bytes.
    fn read(&self, from: u64, max: usize) -> (u64, Vec<u8>);
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
    /// The helper, verified and held open: the process is started from this very file.
    pub exe: Arc<super::locate::Verified>,
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
        let mut command = std::process::Command::new(self.exe.exec_path());
        command
            .arg0("wyss-screen")
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
    /// A few characters to tell this link's thread apart in a process listing.
    pub label: String,
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
            label: String::new(),
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
    /// The parser has everything since the start except `skipped` bytes it was too slow to be given
    /// before the ring moved on: the screen may differ from the program's until it draws again.
    Gap {
        skipped: u64,
    },
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
    /// Bytes skipped since this helper started (see `Fidelity::Gap`).
    pub skipped: u64,
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
    /// The helper with this number ended on its own (its output closed).
    Gone(u64),
    Snapshot {
        scrollback: u32,
        reply: SyncSender<Result<Snap, LinkError>>,
    },
    Stop,
}

/// The PTY's size over the terminal's output: each change is tied to the offset in the stream where
/// it happened, so a screen is resized *between* the bytes written before the change and those
/// after it, never at some later moment. A value, not a command: it cannot be lost to a full queue.
struct SizeLog {
    base: (u16, u16),
    changes: VecDeque<(u64, u16, u16)>,
}
impl SizeLog {
    fn new(size: (u16, u16)) -> Self {
        Self {
            base: size,
            changes: VecDeque::new(),
        }
    }
    fn latest(&self) -> (u16, u16) {
        self.changes.back().map_or(self.base, |c| (c.1, c.2))
    }
    fn record(&mut self, at: u64, rows: u16, cols: u16) {
        if self.latest() == (rows, cols) {
            return;
        }
        self.changes.push_back((at, rows, cols));
        while self.changes.len() > 1024 {
            if let Some((_, r, c)) = self.changes.pop_front() {
                self.base = (r, c);
            }
        }
    }
    /// The size in effect for the byte at `offset`.
    fn size_at(&self, offset: u64) -> (u16, u16) {
        let mut size = self.base;
        for &(at, rows, cols) in &self.changes {
            if at > offset {
                break;
            }
            size = (rows, cols);
        }
        size
    }
    fn next_change_after(&self, offset: u64) -> Option<u64> {
        self.changes.iter().map(|c| c.0).find(|&at| at > offset)
    }
}

pub struct ScreenLink {
    commands: SyncSender<Command>,
    shared: Arc<Mutex<Status>>,
    size: Arc<Mutex<SizeLog>>,
    /// A wake is already waiting in the queue (or about to be): further ones add nothing, and must
    /// not fill the queue a snapshot request needs.
    woken: Arc<std::sync::atomic::AtomicBool>,
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
            skipped: 0,
        }));
        let (tx, rx) = mpsc::sync_channel(64);
        let want = Arc::new(Mutex::new(SizeLog::new(size)));
        let woken = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker = Worker {
            spawner,
            source,
            config,
            sizes: want.clone(),
            woken: woken.clone(),
            have: size,
            shared: shared.clone(),
            commands: rx,
            serial: 0,
            gave_up_at: None,
            commands_tx: tx.clone(),
            session: None,
            next: 0,
            begun: false,
            starts: VecDeque::new(),
        };
        let thread = thread::Builder::new()
            .name(format!(
                "wyss-sl-{}",
                worker.config.label.chars().take(7).collect::<String>()
            ))
            .spawn(move || worker.run())
            .ok();
        ScreenLink {
            commands: tx,
            shared,
            size: want,
            woken,
            thread: Mutex::new(thread),
        }
    }

    /// New output is in the ring. Never blocks; a link that is behind already knows.
    pub fn wake(&self) {
        use std::sync::atomic::Ordering::SeqCst;
        if !self.woken.swap(true, SeqCst) && self.commands.try_send(Command::Wake).is_err() {
            // The queue is full of other work, which pumps whatever has arrived anyway.
        }
    }

    /// The PTY now has this size; the output the host held up to `at` was written before the change.
    pub fn resize(&self, rows: u16, cols: u16, at: u64) {
        lock(&self.size).record(at, rows, cols);
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
    id: u64,
    /// Frames to the helper go through a thread of their own, so that a helper that stopped
    /// reading cannot stop this one from enforcing its deadline.
    out: Sender<Vec<u8>>,
    responses: Receiver<Result<Response, IpcError>>,
    handle: Box<dyn ChildHandle>,
    threads: Vec<JoinHandle<()>>,
}

impl Session {
    /// Ends the helper through the handle held for it, and joins the threads that talk to it. The
    /// receiver goes first: a reader that is blocked handing over a frame nobody asked for (a
    /// helper that floods) is released by it, so this cannot wait for ever.
    fn end(self) {
        let Session {
            out,
            responses,
            mut handle,
            threads,
            ..
        } = self;
        drop(responses);
        handle.kill();
        handle.reap();
        drop(out);
        for t in threads {
            let _ = t.join();
        }
    }
}

struct Worker {
    spawner: Arc<dyn Spawner>,
    source: Arc<dyn ByteSource>,
    config: LinkConfig,
    /// The PTY's sizes over the stream, and the size the helper in service has.
    sizes: Arc<Mutex<SizeLog>>,
    woken: Arc<std::sync::atomic::AtomicBool>,
    have: (u16, u16),
    shared: Arc<Mutex<Status>>,
    commands: Receiver<Command>,
    session: Option<Session>,
    /// Number of the next helper to start, and the way the reader threads tell this worker.
    serial: u64,
    commands_tx: SyncSender<Command>,
    /// When the link last gave up; after the restart window it tries again.
    gave_up_at: Option<Instant>,
    /// Offset of the next byte to give the helper.
    next: u64,
    /// Whether the helper in service has been given any of the stream yet: its first `Feed` fixes
    /// where its screen starts, and a `Skip` before that has nothing to skip.
    begun: bool,
    starts: VecDeque<Instant>,
}

impl Worker {
    fn set(&self, f: impl FnOnce(&mut Status)) {
        f(&mut lock(&self.shared));
    }

    fn run(mut self) {
        // Start eagerly: a helper is ready before the first snapshot is asked for.
        let _ = self.ensure();
        // Nothing here polls: an idle terminal's link sleeps until output, a snapshot request, or
        // its helper ending wakes it.
        loop {
            let command = self.commands.recv();
            // Whatever arrives from now on is a new wake; this command pumps all there is.
            self.woken.store(false, std::sync::atomic::Ordering::SeqCst);
            match command {
                Ok(Command::Stop) | Err(_) => break,
                Ok(Command::Wake) => self.catch_up(),
                Ok(Command::Gone(id)) => {
                    if self.session.as_ref().is_some_and(|s| s.id == id) {
                        let _ = self.lose("exited");
                        // Replaced at once (within the restart budget): the screen is rebuilt from
                        // the ring while it is all still there, not when someone asks for it.
                        self.catch_up();
                    }
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

    /// Feeds what is waiting, a quantum at a time: after each, anything queued (a snapshot, a stop)
    /// is served before the next, so continuous output cannot hold up the rest.
    fn catch_up(&mut self) {
        if matches!(self.pump(PUMP_QUANTUM), Ok(true)) {
            use std::sync::atomic::Ordering::SeqCst;
            if !self.woken.swap(true, SeqCst) && self.commands_tx.try_send(Command::Wake).is_err() {
                // Other work is queued; it pumps too.
            }
        }
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
            // Giving up is not for ever: after the restart window it is tried again.
            let waited = self
                .gave_up_at
                .is_some_and(|t| t.elapsed() > self.config.restart_window);
            if !waited {
                return Err(Fail::Helper(why));
            }
            self.gave_up_at = None;
            self.starts.clear();
            self.set(|s| s.health = Health::Starting);
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
            self.gave_up_at = Some(now);
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
                self.gave_up_at = Some(now);
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
        self.serial += 1;
        let id = self.serial;
        let gone = self.commands_tx.clone();
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
            // Whoever is waiting for an answer has it (an error); a link that is asleep is told.
            let _ = gone.try_send(Command::Gone(id));
        });
        self.session = Some(Session {
            id,
            out: out_tx,
            responses: resp_rx,
            handle: spawned.handle,
            threads: vec![writer, reader],
        });
        self.set(|s| s.helper_pid = pid);
        // The screen starts where the ring starts, at the size the PTY had there.
        let start = self.source.start();
        self.have = lock(&self.sizes).size_at(start);
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
        // From the first byte when nothing was ever lost.
        self.next = start;
        self.begun = false;
        self.set(|s| {
            s.fidelity = if start == 0 {
                Fidelity::Exact
            } else {
                Fidelity::Tail { missing: start }
            };
            s.fed = start;
            s.skipped = 0;
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

    /// Feeds what the ring holds that the helper has not seen, up to a quantum of bytes. `true`:
    /// there is more.
    fn pump(&mut self, quantum: usize) -> Result<bool, Fail> {
        let target = self.source.end();
        self.pump_to(target, quantum)
    }

    /// Feeds the stream up to `target` (never chasing what arrives after it), or `quantum` bytes.
    fn pump_to(&mut self, target: u64, quantum: usize) -> Result<bool, Fail> {
        let mut fed = 0usize;
        loop {
            self.ensure()?;
            if self.next >= target {
                return Ok(false);
            }
            if fed >= quantum {
                return Ok(true);
            }
            // The size in effect for the next byte, and no frame that crosses a change of it.
            self.apply_size_at(self.next)?;
            let until_change = lock(&self.sizes)
                .next_change_after(self.next)
                .map_or(u64::MAX, |at| at - self.next);
            let max = MAX_FEED
                .min(usize::try_from(until_change).unwrap_or(usize::MAX))
                .min(usize::try_from(target - self.next).unwrap_or(usize::MAX));
            let (at, bytes) = self.source.read(self.next, max);
            if bytes.is_empty() {
                // Nothing there: either the ring is empty up to here, or it moved on past us.
                let start = self.source.start();
                if start > self.next {
                    self.skip_to(start)?;
                    continue;
                }
                return Ok(false);
            }
            if at > self.next {
                // The ring moved on between asking and reading: the bytes in between are gone, and
                // what came back is not what was asked for, so it goes in at its own offset.
                self.skip_to(at)?;
            } else if at < self.next {
                return Err(self.lose("bad_source"));
            }
            let len = bytes.len() as u64;
            let offset = self.next;
            match self.exchange(&Request::Feed { offset, bytes })? {
                Response::Fed { end, .. } if end == offset + len => {
                    self.next = end;
                    self.begun = true;
                    fed += usize::try_from(len).unwrap_or(usize::MAX);
                    self.set(|s| s.fed = end);
                }
                _ => return Err(self.lose("bad_ack")),
            }
        }
    }

    /// The ring moved on past bytes this helper never got (a flood faster than it parses).
    /// Replaying from the ring would only fall behind again: the helper is told to go on from `to`,
    /// and the loss is stated.
    fn skip_to(&mut self, to: u64) -> Result<(), Fail> {
        if !self.begun {
            // Nothing of the stream reached this helper yet: its screen simply starts later, as if
            // it had been started there, and what came before is stated as missing.
            self.next = to;
            self.set(|s| {
                s.fed = to;
                s.fidelity = Fidelity::Tail { missing: to };
            });
            return Ok(());
        }
        match self.exchange(&Request::Skip { to })? {
            Response::Fed { end, .. } if end == to => {
                let lost = to - self.next;
                self.next = to;
                self.set(|s| {
                    s.fed = to;
                    s.skipped += lost;
                    if matches!(s.fidelity, Fidelity::Exact | Fidelity::Gap { .. }) {
                        s.fidelity = Fidelity::Gap { skipped: s.skipped };
                    }
                });
                Ok(())
            }
            _ => Err(self.lose("bad_ack")),
        }
    }

    /// The helper has the size the PTY had for the byte at `offset`.
    fn apply_size_at(&mut self, offset: u64) -> Result<(), Fail> {
        let (rows, cols) = lock(&self.sizes).size_at(offset);
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
                // Everything the ring holds now, and not what arrives while this is made.
                let target = self.source.end();
                self.pump_to(target, usize::MAX)?;
                self.apply_size_at(self.next)?;
                match self.exchange(&Request::Snapshot {
                    scrollback_lines: scrollback,
                })? {
                    Response::Snapshot(body) => Ok(Some(*body)),
                    Response::TooLarge => Ok(None),
                    _ => Err(self.lose("bad_ack")),
                }
            })();
            match result {
                // The screen cannot be drawn in the bound: not a fault of the helper.
                Ok(None) => return Err(LinkError::Unavailable("too_large")),
                Ok(Some(body)) => {
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

/// Bytes fed in one go before queued commands are looked at.
const PUMP_QUANTUM: usize = 1024 * 1024;

#[cfg(test)]
mod tests;
