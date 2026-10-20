//! Owned child processes. Inactive: no provider, socket or installed-state entry point.
//!
//! The only way this module ever signals a process is through a pidfd opened for a child this
//! very value spawned and still holds unreaped. There is no name, path, PID-number or process
//! group selection anywhere: no `pgrep`, no `pkill`, no `kill(pid)`, no `killpg`. A pidfd keeps
//! referring to the same process even if its PID number were reused, so a stale handle yields
//! `ESRCH`, never a signal to an unrelated process that happens to share a path or number.
//!
//! `PR_SET_PDEATHSIG` is tied to the *thread* that forks, not the process: `spawn` must be called
//! from a thread which lives as long as the authority (never a pool/blocking thread that can exit),
//! otherwise the provider dies with that thread. This is a requirement on every caller.
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
    pidfd: OwnedFd,
    pid: i32,
    start_ticks: u64,
    stage: Stage,
    stage_since: Instant,
    grace: Grace,
    exit: Option<ExitStatus>,
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
        let self_pidfd = unsafe {
            libc::syscall(libc::SYS_pidfd_open, nix::unistd::getpid().as_raw(), 0)
        };
        if self_pidfd < 0 {
            return Err(E::Unsupported);
        }
        let self_pidfd = unsafe { OwnedFd::from_raw_fd(self_pidfd as i32) };
        let expected_parent = nix::unistd::getpid();
        // SAFETY: the closure performs only async-signal-safe syscalls after fork and builds
        // an OS error on failure. SIGKILL cannot be masked, and comparing the parent closes the
        // window in which the authority died before the prctl took effect.
        unsafe {
            command.pre_exec(move || {
                nix::sys::prctl::set_pdeathsig(nix::sys::signal::Signal::SIGKILL)
                    .map_err(std::io::Error::from)?;
                if nix::unistd::getppid() != expected_parent {
                    return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
                }
                nix::unistd::setsid().map_err(std::io::Error::from)?;
                // No inherited descriptor (pidfd, staging, database) survives exec: everything
                // above stdio is close-on-exec. Best effort on kernels without close_range.
                let _ = libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32);
                nix::sys::signal::SigSet::empty()
                    .thread_set_mask()
                    .map_err(std::io::Error::from)?;
                Ok(())
            });
        }
        let mut child = command.spawn().map_err(|_| E::ProviderRejected)?;
        // Free the preflight descriptor before opening the owned child's pidfd
        // so we do not use an extra slot during the critical pinning operation.
        drop(self_pidfd);
        let pid = match i32::try_from(child.id()) {
            Ok(pid) => pid,
            Err(_) => {
                // No safe signal target has been established. Retain the owned
                // handle for nonblocking reaping rather than guessing a PID.
                enqueue_reap(child, permit);
                return Err(E::InternalError);
            }
        };
        // The child is unreaped, so its PID cannot have been reused yet.
        // SAFETY: plain syscall with integer arguments; the returned descriptor is owned below.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        let opened = if fd >= 0 {
            // SAFETY: `fd` is a fresh descriptor returned by the kernel and owned by nobody else.
            Some(unsafe { OwnedFd::from_raw_fd(fd as i32) })
        } else {
            None
        };
        // The pidfd refers to our child only if it is still our unreaped direct child.
        // An external SIGCHLD consumer must not make a recycled PID a signal target.
        let still_ours = matches!(child.try_wait(), Ok(None));
        let start = proc_start_ticks(pid);
        if !still_ours || start.is_err() || opened.is_none() {
            // Signal solely through a verified pinned descriptor. If none could
            // be acquired, refuse launch and retain the child without signalling
            // by its numeric PID. The reaper keeps its bounded capacity slot.
            if still_ours {
                if let Some(ref pinned) = opened {
                    // SAFETY: pinned belongs to the still-unreaped child we spawned.
                    let _ = unsafe {
                        libc::syscall(
                            libc::SYS_pidfd_send_signal,
                            pinned.as_raw_fd(),
                            libc::SIGKILL,
                            std::ptr::null::<libc::siginfo_t>(),
                            0,
                        )
                    };
                }
            }
            enqueue_reap(child, permit);
            return Err(E::InternalError);
        }
        let pidfd = opened.ok_or(E::InternalError)?;
        let start_ticks = start.map_err(|_| E::InternalError)?;
        let stdin = child.stdin.take();
        Ok((
            Self {
                child: Some(child),
                reap_permit: Some(permit),
                pidfd,
                pid,
                start_ticks,
                stage: Stage::Running,
                stage_since: Instant::now(),
                grace,
                exit: None,
                stdin,
            },
            ContainmentReport {
                direct_child_pdeathsig: true,
                own_session: true,
                descendants_contained: false,
            },
        ))
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
        let status = self
            .child
            .as_mut()
            .ok_or(E::InternalError)?
            .try_wait()
            .map_err(|_| E::InternalError)?;
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
            return Ok(Progress::Reaped(exit_code(status)));
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
    let reaper = REAPER.get_or_init(|| {
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
                    scanning.retain_mut(|job| !matches!(job.child.try_wait(), Ok(Some(_))));
                    if !scanning.is_empty() {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            });
        started.ok().map(|_| reaper)
    }).as_ref().ok_or(E::InternalError)?;
    reserve_slot(reaper)
}

fn reserve_slot(reaper: &Arc<Reaper>) -> ControlResult<ReapPermit> {
    let mut count = reaper.slots.load(Ordering::Acquire);
    loop {
        if count >= REAPER_LIMIT {
            return Err(E::CapacityExceeded);
        }
        match reaper.slots.compare_exchange_weak(
            count, count + 1, Ordering::AcqRel, Ordering::Acquire,
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
    pending.push(ReapJob { child, _permit: permit });
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
        if let (Some(mut child), Some(permit)) = (self.child.take(), self.reap_permit.take()) {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                enqueue_reap(child, permit);
            }
        }
    }
}

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod tests;
