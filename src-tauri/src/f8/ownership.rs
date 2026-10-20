//! Owned child processes. Inactive: no provider, socket or installed-state entry point.
//!
//! The only way this module ever signals a process is through a pidfd opened for a child this
//! very value spawned and still holds unreaped. There is no name, path, PID-number or process
//! group selection anywhere: no `pgrep`, no `pkill`, no `kill(pid)`, no `killpg`. A pidfd keeps
//! referring to the same process even if its PID number were reused, so a stale handle yields
//! `ESRCH`, never a signal to an unrelated process that happens to share a path or number.
//!
//! `PR_SET_PDEATHSIG` is tied to the thread that forks. One lifetime coordinator owns
//! every spawn, so leaving the caller's thread cannot terminate a running worker.
//!
//! Containment is deliberately honest: `PR_SET_PDEATHSIG` covers the direct child only. A
//! descendant which outlives or escapes its parent is *reported* (read-only count) but is never
//! discovered into a signal target; `ContainmentPolicy::DescendantsContained` is refused because
//! no verified mechanism exists here (fail closed instead of widening cleanup).
use crate::contracts::control::{ControlError as E, ControlResult};
use nix::libc;
use std::{
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::{CommandExt, ExitStatusExt},
    },
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// What the caller requires of descendant containment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainmentPolicy {
    /// Only the direct child is owned; descendants are reported but never signalled.
    DirectChildOnly,
    /// Every descendant is contained by a PID namespace whose init dies with the owned child.
    /// The only accepted mechanism is the root-owned `bwrap` launcher invoked with both
    /// `--unshare-pid` and `--die-with-parent`; any other command is refused (`Unsupported`),
    /// and after the spawn the namespace is observed (read-only) before it is reported contained.
    DescendantsContained,
}

/// The one launcher whose PID-namespace semantics are relied on.
pub const NAMESPACE_LAUNCHER: &str = "/usr/bin/bwrap";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainmentReport {
    /// `PR_SET_PDEATHSIG(SIGKILL)` was installed, with the parent re-checked, before exec.
    pub direct_child_pdeathsig: bool,
    /// The child leads its own session, so terminal job-control signals never reach it.
    pub own_session: bool,
    /// Descendants are not contained by any mechanism implemented here. NOT_PROVEN.
    pub descendants_contained: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Nothing requested yet.
    Running,
    /// Stdin closed (EOF); waiting for a voluntary exit.
    EofSent,
    /// SIGTERM sent through the pidfd.
    TermSent,
    /// SIGKILL sent through the pidfd.
    KillSent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Pending(Stage),
    /// The child was reaped. Only this proves the process is gone.
    Reaped(i32),
    /// Even after SIGKILL the child cannot be reaped (uninterruptible state). The handle is
    /// kept: the process is never abandoned, and the slot stays consumed.
    Stuck,
}

#[derive(Debug, Clone, Copy)]
pub struct Grace {
    pub eof: Duration,
    pub term: Duration,
    pub kill: Duration,
}
impl Default for Grace {
    /// `eof_grace_ms` and `term_grace_ms` from the reviewed resource contract.
    fn default() -> Self {
        Self {
            eof: Duration::from_millis(1000),
            term: Duration::from_millis(2000),
            kill: Duration::from_millis(2000),
        }
    }
}

pub struct OwnedChild {
    // Option permits handing an unreaped child to the asynchronous reaper from Drop.
    child: Option<Child>,
    reap_permit: Option<ReapPermit>,
    pidfd: Arc<OwnedFd>,
    pid: i32,
    start_ticks: u64,
    stage: Stage,
    stage_since: Instant,
    grace: Grace,
    exit: Option<ExitStatus>,
    reaped_elsewhere: bool,
    stdin: Option<ChildStdin>,
}

fn proc_start_ticks(pid: i32) -> ControlResult<u64> {
    let stat =
        std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|_| E::StorageUnavailable)?;
    // `comm` may contain spaces and parentheses: fields resume after the last ')'.
    let rest = stat.rsplit_once(')').ok_or(E::InvalidRequest)?.1;
    rest.split_whitespace()
        .nth(19)
        .and_then(|f| f.parse().ok())
        .ok_or(E::InvalidRequest)
}

impl OwnedChild {
    /// Spawns and takes ownership. The caller configures stdio and arguments beforehand.
    pub fn spawn(
        command: &mut Command,
        policy: ContainmentPolicy,
        grace: Grace,
    ) -> ControlResult<(Self, ContainmentReport)> {
        if policy == ContainmentPolicy::DescendantsContained {
            verify_namespace_launch(command)?;
        }
        // Fail before spawning if the bounded reaper cannot retain the child on
        // exceptional exit, identity verification failure, or blocked reap.
        let permit = reap_permit()?;
        // Fail before any spawn on a kernel that cannot supply pidfds. This
        // preflight is not proof that a later allocation cannot fail under
        // resource pressure; the reaper retains ownership on that path.
        let self_pidfd =
            unsafe { libc::syscall(libc::SYS_pidfd_open, nix::unistd::getpid().as_raw(), 0) };
        if self_pidfd < 0 {
            return Err(E::Unsupported);
        }
        let self_pidfd = unsafe { OwnedFd::from_raw_fd(self_pidfd as i32) };
        drop(self_pidfd);
        // Command::spawn waits for exec. Run it on one lifetime thread so the
        // caller can pin the blocked pre-exec child before permitting exec.
        gated_spawn(command, permit, grace, false)
    }

    /// Read-only observation that the owned launcher really put its payload into another PID
    /// namespace: some child of the owned process has a different `pid` namespace than this
    /// process. `false` until the payload exists, and never a permission to signal anything.
    pub fn namespace_contained(&self) -> bool {
        let own = std::fs::read_link("/proc/self/ns/pid").ok();
        if self.exit.is_some() || own.is_none() {
            return false;
        }
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{}/task", self.pid)) else {
            return false;
        };
        for task in tasks.flatten() {
            let Ok(children) = std::fs::read_to_string(task.path().join("children")) else {
                continue;
            };
            for child in children.split_whitespace() {
                if std::fs::read_link(format!("/proc/{child}/ns/pid"))
                    .is_ok_and(|ns| Some(ns) != own)
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn stdin(&mut self) -> Option<&mut ChildStdin> {
        self.stdin.as_mut()
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.as_mut().and_then(|child| child.stdout.take())
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.as_mut().and_then(|child| child.stderr.take())
    }
    /// Diagnostic only. It is never an input to any signalling decision.
    pub fn pid_for_diagnostics(&self) -> i32 {
        self.pid
    }

    /// The process this handle owns is still the one which was spawned: same start time, not
    /// yet exited. A mismatch means the handle must not be used to signal.
    pub fn verify(&self) -> ControlResult<()> {
        if self.exit.is_some() {
            return Err(E::StaleRevision);
        }
        match proc_start_ticks(self.pid) {
            Ok(t) if t == self.start_ticks => Ok(()),
            _ => Err(E::StaleRevision),
        }
    }

    fn signal(&self, signal: i32) -> ControlResult<()> {
        if self.exit.is_some() {
            return Err(E::StaleRevision);
        }
        // SAFETY: integer arguments only; the pidfd is owned by this value and refers to the
        // child it was opened for, so a recycled PID number cannot be signalled through it.
        let r = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.pidfd.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if r == 0 {
            return Ok(());
        }
        match std::io::Error::last_os_error().raw_os_error() {
            // Already gone: the next try_wait observes and reaps it.
            Some(libc::ESRCH) => Ok(()),
            _ => Err(E::InternalError),
        }
    }

    /// Non-blocking reap check.
    pub fn try_wait(&mut self) -> ControlResult<Option<ExitStatus>> {
        if let Some(s) = self.exit {
            return Ok(Some(s));
        }
        let status = match self.child.as_mut().ok_or(E::InternalError)?.try_wait() {
            Ok(status) => status,
            Err(e) if e.raw_os_error() == Some(libc::ECHILD) => {
                // ECHILD alone may also reflect SIGCHLD disposition. Require the
                // stable pidfd to report actual termination before freeing capacity.
                let mut fd = libc::pollfd {
                    fd: self.pidfd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                if unsafe { libc::poll(&mut fd, 1, 0) } != 1 || fd.revents & libc::POLLIN == 0 {
                    return Err(E::InternalError);
                }
                self.reaped_elsewhere = true;
                // The kernel status was consumed elsewhere; 255 is only an
                // internal terminal sentinel. poll_stop reports unknown (-1).
                Some(ExitStatus::from_raw(255 << 8))
            }
            Err(_) => return Err(E::InternalError),
        };
        self.exit = status;
        Ok(status)
    }
    pub fn is_reaped(&self) -> bool {
        self.exit.is_some()
    }

    /// Begin the bounded escalation. Idempotent; never blocks.
    pub fn begin_stop(&mut self, now: Instant) {
        if self.stage == Stage::Running && self.exit.is_none() {
            self.stdin.take();
            self.stage = Stage::EofSent;
            self.stage_since = now;
        }
    }
    /// Advance the escalation by one non-blocking step: EOF, then SIGTERM, then SIGKILL, each
    /// after its grace, always through the owned pidfd. The caller (never the authority's sole
    /// writer) calls this repeatedly until it reports `Reaped`.
    pub fn poll_stop(&mut self, now: Instant) -> ControlResult<Progress> {
        if let Some(status) = self.try_wait()? {
            return Ok(Progress::Reaped(if self.reaped_elsewhere {
                -1
            } else {
                exit_code(status)
            }));
        }
        let waited = now.saturating_duration_since(self.stage_since);
        match self.stage {
            Stage::Running => (),
            Stage::EofSent if waited >= self.grace.eof => {
                self.signal(libc::SIGTERM)?;
                self.stage = Stage::TermSent;
                self.stage_since = now;
            }
            Stage::TermSent if waited >= self.grace.term => {
                self.signal(libc::SIGKILL)?;
                self.stage = Stage::KillSent;
                self.stage_since = now;
            }
            Stage::KillSent if waited >= self.grace.kill => return Ok(Progress::Stuck),
            _ => (),
        }
        Ok(Progress::Pending(self.stage))
    }

    /// Read-only count of live descendants of the owned child (bounded walk). Reported for
    /// containment accounting; the PIDs are discarded and never become signal targets.
    pub fn descendant_count(&self) -> usize {
        fn walk(pid: i32, depth: usize, budget: &mut usize) -> usize {
            if depth == 0 || *budget == 0 {
                return 0;
            }
            let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
                return 0;
            };
            let mut found = 0;
            for task in tasks.flatten() {
                let Ok(children) = std::fs::read_to_string(task.path().join("children")) else {
                    continue;
                };
                for child in children.split_whitespace().filter_map(|c| c.parse().ok()) {
                    if *budget == 0 {
                        return found;
                    }
                    *budget -= 1;
                    found += 1 + walk(child, depth - 1, budget);
                }
            }
            found
        }
        if self.exit.is_some() {
            return 0;
        }
        walk(self.pid, 4, &mut 256)
    }
}

struct Pin {
    fd: Arc<OwnedFd>,
    pid: i32,
    start: u64,
}
type SpawnJob = Box<dyn FnOnce() + Send>;

fn spawn_coordinator() -> ControlResult<&'static mpsc::SyncSender<SpawnJob>> {
    static SPAWNER: OnceLock<Option<mpsc::SyncSender<SpawnJob>>> = OnceLock::new();
    SPAWNER
        .get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel::<SpawnJob>(REAPER_LIMIT);
            std::thread::Builder::new()
                .name("wyss-f8-spawn-owner".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        job();
                    }
                })
                .ok()
                .map(|_| tx)
        })
        .as_ref()
        .ok_or(E::InternalError)
}

/// The payload cannot execute until its exact owned pidfd has been acquired.
/// Timeout drops the gate; the coordinator retains all child/reaper ownership.
fn gated_spawn(
    command: &mut Command,
    permit: ReapPermit,
    grace: Grace,
    fail_pin: bool,
) -> ControlResult<(OwnedChild, ContainmentReport)> {
    gated_spawn_timed(
        command,
        permit,
        grace,
        fail_pin,
        Duration::from_secs(3),
        Duration::ZERO,
    )
}

fn gated_spawn_timed(
    command: &mut Command,
    permit: ReapPermit,
    grace: Grace,
    fail_pin: bool,
    timeout: Duration,
    reply_delay: Duration,
) -> ControlResult<(OwnedChild, ContainmentReport)> {
    use std::os::unix::net::UnixStream;
    let coordinator = spawn_coordinator()?;
    let (parent, gate) = UnixStream::pair().map_err(|_| E::InternalError)?;
    let pin = Arc::new(Mutex::new(None::<Pin>));
    let owned_pin = Arc::clone(&pin);
    let (reply, result) = mpsc::sync_channel(1);
    let placeholder = Command::new(command.get_program());
    let mut moved = std::mem::replace(command, placeholder);
    // SAFETY: only raw, async-signal-safe read/write/getpid calls execute after fork.
    // The coordinator retains the socket until spawn returns. On command reuse
    // the completed closure skips its expired descriptor using the atomic flag.
    let parent_fd = parent.as_raw_fd();
    let gate_fd = gate.as_raw_fd();
    let expected_parent = nix::unistd::getpid();
    let used = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let gate_used = Arc::clone(&used);
    unsafe {
        moved.pre_exec(move || {
            if gate_used.load(Ordering::Acquire) {
                return Ok(());
            }
            // The fork inherits both endpoints. Close the parent endpoint in
            // the child so caller timeout/refusal produces EOF on the gate.
            libc::close(parent_fd);
            nix::sys::prctl::set_pdeathsig(nix::sys::signal::Signal::SIGKILL)
                .map_err(std::io::Error::from)?;
            if nix::unistd::getppid() != expected_parent {
                return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
            }
            nix::unistd::setsid().map_err(std::io::Error::from)?;
            let _ = libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32);
            nix::sys::signal::SigSet::empty()
                .thread_set_mask()
                .map_err(std::io::Error::from)?;
            let pid = libc::getpid().to_ne_bytes();
            if libc::write(gate_fd, pid.as_ptr().cast(), pid.len()) != pid.len() as isize {
                return Err(std::io::Error::from_raw_os_error(libc::EIO));
            }
            let mut ack = 0u8;
            loop {
                let n = libc::read(gate_fd, (&mut ack as *mut u8).cast(), 1);
                if n == 1 && ack == 1 {
                    return Ok(());
                }
                if n < 0 && *libc::__errno_location() == libc::EINTR {
                    continue;
                }
                return Err(std::io::Error::from_raw_os_error(libc::ECANCELED));
            }
        });
    }
    let job: SpawnJob = Box::new(move || {
        let spawned = moved.spawn();
        if !reply_delay.is_zero() {
            std::thread::sleep(reply_delay);
        }
        used.store(true, Ordering::Release);
        drop(gate);
        let outcome = spawned
            .map_err(|_| E::ProviderRejected)
            .and_then(|mut child| {
                let pinned = owned_pin.lock().unwrap_or_else(|e| e.into_inner()).take();
                let Some(pinned) = pinned else {
                    // A successful exec requires the parent to have stored the pin.
                    // Preserve ownership even if an internal invariant is violated.
                    enqueue_reap(child, permit);
                    return Err(E::InternalError);
                };
                let stdin = child.stdin.take();
                Ok((
                    OwnedChild {
                        child: Some(child),
                        reap_permit: Some(permit),
                        pidfd: pinned.fd,
                        pid: pinned.pid,
                        start_ticks: pinned.start,
                        stage: Stage::Running,
                        stage_since: Instant::now(),
                        grace,
                        exit: None,
                        reaped_elsewhere: false,
                        stdin,
                    },
                    ContainmentReport {
                        direct_child_pdeathsig: true,
                        own_session: true,
                        descendants_contained: false,
                    },
                ))
            });
        // If the caller timed out or unwound, dropping an OwnedChild here still
        // kills only its pinned process and transfers reap to the bounded reaper.
        let _ = reply.send((moved, outcome));
    });
    coordinator.try_send(job).map_err(|_| E::CapacityExceeded)?;
    let deadline = Instant::now() + timeout;
    let mut refusal_pin: Option<Arc<OwnedFd>> = None;
    let mut pid_bytes = [0u8; std::mem::size_of::<i32>()];
    let mut received = 0;
    let mut refused = false;
    loop {
        match result.try_recv() {
            Ok((original, outcome)) => {
                *command = original;
                return outcome;
            }
            Err(mpsc::TryRecvError::Disconnected) => return Err(E::InternalError),
            Err(mpsc::TryRecvError::Empty) => (),
        }
        if Instant::now() >= deadline {
            if let Some(fd) = refusal_pin {
                // Handoff may have reached exec while the coordinator is delayed.
                // Revoke only this pinned child before reporting a refused launch.
                let _ = unsafe {
                    libc::syscall(
                        libc::SYS_pidfd_send_signal,
                        fd.as_raw_fd(),
                        libc::SIGKILL,
                        std::ptr::null::<libc::siginfo_t>(),
                        0,
                    )
                };
            }
            return Err(E::ProviderRejected);
        }
        if !refused && received < pid_bytes.len() {
            let n = unsafe {
                libc::recv(
                    parent.as_raw_fd(),
                    pid_bytes[received..].as_mut_ptr().cast(),
                    pid_bytes.len() - received,
                    libc::MSG_DONTWAIT,
                )
            };
            if n > 0 {
                received += n as usize;
            }
            if received == pid_bytes.len() {
                let pid = i32::from_ne_bytes(pid_bytes);
                // The child is blocked before exec and cannot be reaped by this
                // coordinator yet; pidfd acquisition precedes payload execution.
                let fd = if fail_pin {
                    -1
                } else {
                    unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) }
                };
                if fd >= 0 {
                    let fd = Arc::new(unsafe { OwnedFd::from_raw_fd(fd as i32) });
                    if let Ok(start) = proc_start_ticks(pid) {
                        refusal_pin = Some(Arc::clone(&fd));
                        *pin.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(Pin { fd, pid, start });
                        let ack = [1u8];
                        let _ = unsafe {
                            libc::send(
                                parent.as_raw_fd(),
                                ack.as_ptr().cast(),
                                1,
                                libc::MSG_NOSIGNAL,
                            )
                        };
                    }
                }
                // Closing the write side refuses a failed pin without signalling
                // any numeric PID, and makes pre_exec return before exec.
                let _ = parent.shutdown(std::net::Shutdown::Write);
                refused = true;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn exit_code(s: ExitStatus) -> i32 {
    s.code()
        .or_else(|| s.signal().map(|n| 128 + n))
        .unwrap_or(-1)
}

/// The command must be the root-owned launcher with both flags that give it its guarantee,
/// given as options before an explicit `--` that starts the payload. The same words appearing
/// in the payload's own arguments count for nothing.
fn verify_namespace_launch(command: &Command) -> ControlResult<()> {
    use std::os::unix::fs::MetadataExt;
    let args: Vec<_> = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let meta = std::fs::symlink_metadata(NAMESPACE_LAUNCHER).map_err(|_| E::Unsupported)?;
    let Some(payload) = args.iter().position(|a| a == "--") else {
        return Err(E::Unsupported);
    };
    let options = &args[..payload];
    if command.get_program() != NAMESPACE_LAUNCHER
        || !meta.is_file()
        || meta.uid() != 0
        || meta.mode() & 0o022 != 0
        || !options.iter().any(|a| a == "--unshare-pid")
        || !options.iter().any(|a| a == "--die-with-parent")
        || options.iter().any(|a| a == "--share-pid")
    {
        return Err(E::Unsupported);
    }
    Ok(())
}

/// A process must reserve one of the fixed lifetime reaping slots BEFORE it can spawn.
/// Reap ownership therefore never depends on creating a new thread under memory pressure.
const REAPER_LIMIT: usize = 4096;

struct Reaper {
    slots: AtomicUsize,
    queue: Mutex<Vec<ReapJob>>,
    wake: Condvar,
}

struct ReapPermit(Arc<Reaper>);
impl Drop for ReapPermit {
    fn drop(&mut self) {
        self.0.slots.fetch_sub(1, Ordering::AcqRel);
    }
}

struct ReapJob {
    child: Child,
    _permit: ReapPermit,
}

fn reap_permit() -> ControlResult<ReapPermit> {
    static REAPER: OnceLock<Option<Arc<Reaper>>> = OnceLock::new();
    let reaper = REAPER
        .get_or_init(|| {
            let reaper = Arc::new(Reaper {
                slots: AtomicUsize::new(0),
                queue: Mutex::new(Vec::with_capacity(REAPER_LIMIT)),
                wake: Condvar::new(),
            });
            let worker = Arc::clone(&reaper);
            let started = std::thread::Builder::new()
                .name("wyss-f8-owned-reaper".into())
                .spawn(move || {
                    let mut scanning = Vec::<ReapJob>::with_capacity(REAPER_LIMIT);
                    loop {
                        // No OS wait, disk I/O or child polling under the writer-facing lock.
                        let mut queued = worker.queue.lock().unwrap_or_else(|e| e.into_inner());
                        while queued.is_empty() && scanning.is_empty() {
                            queued = worker.wake.wait(queued).unwrap_or_else(|e| e.into_inner());
                        }
                        scanning.extend(queued.drain(..));
                        drop(queued);
                        scanning.retain_mut(|job| match job.child.try_wait() {
                            Ok(Some(_)) => false,
                            Ok(None) => true,
                            // An external SIGCHLD collector has already reaped this
                            // child. Never signal a stale numeric PID; release the slot.
                            Err(e) if e.raw_os_error() == Some(libc::ECHILD) => false,
                            // Other failures are not proof of process exit. Preserve
                            // the owned handle, retry and refuse new launches at capacity.
                            Err(_) => true,
                        });
                        if !scanning.is_empty() {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                });
            started.ok().map(|_| reaper)
        })
        .as_ref()
        .ok_or(E::InternalError)?;
    reserve_slot(reaper)
}

fn reserve_slot(reaper: &Arc<Reaper>) -> ControlResult<ReapPermit> {
    let mut count = reaper.slots.load(Ordering::Acquire);
    loop {
        if count >= REAPER_LIMIT {
            return Err(E::CapacityExceeded);
        }
        match reaper.slots.compare_exchange_weak(
            count,
            count + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Ok(ReapPermit(Arc::clone(reaper))),
            Err(next) => count = next,
        }
    }
}

/// The global reaper was started and a slot reserved before spawn. Both the
/// producer vector and scanner are bounded by the same lifetime permit limit.
/// Lock hold time includes only a push, never a blocking wait.
fn enqueue_reap(child: Child, permit: ReapPermit) {
    let reaper = Arc::clone(&permit.0);
    let mut pending = reaper.queue.lock().unwrap_or_else(|e| e.into_inner());
    pending.push(ReapJob {
        child,
        _permit: permit,
    });
    drop(pending);
    reaper.wake.notify_one();
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.stdin.take();
        if self.exit.is_some() {
            return;
        }
        // Signal exactly the pinned child; no process-name or PID lookup.
        let _ = self.signal(libc::SIGKILL);
        if let (Some(mut child), Some(permit)) = (self.child.take(), self.reap_permit.take())
            && !matches!(child.try_wait(), Ok(Some(_)))
        {
            enqueue_reap(child, permit);
        }
    }
}

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod tests;
