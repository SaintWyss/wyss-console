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
    time::{Duration, Instant},
};

/// What the caller requires of descendant containment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainmentPolicy {
    /// Only the direct child is owned; descendants are reported but never signalled.
    DirectChildOnly,
    /// Every descendant is contained. No verified mechanism exists: always refused.
    DescendantsContained,
}

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
    child: Child,
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
            return Err(E::Unsupported);
        }
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
        let pid = i32::try_from(child.id()).map_err(|_| E::InternalError)?;
        // The child is unreaped, so its PID cannot have been reused yet.
        // SAFETY: plain syscall with integer arguments; the returned descriptor is owned below.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        let opened = if fd >= 0 {
            // SAFETY: `fd` is a fresh descriptor returned by the kernel and owned by nobody else.
            Some(unsafe { OwnedFd::from_raw_fd(fd as i32) })
        } else {
            None
        };
        // The pidfd refers to our child only if the child was still ours (unreaped) when it was
        // opened; a child reaped by someone else could have let the PID number be reused.
        let still_ours = matches!(child.try_wait(), Ok(None));
        let start = proc_start_ticks(pid);
        let (Some(pidfd), Ok(start_ticks), true) = (opened, start, still_ours) else {
            // Without an identity-stable handle the child cannot be safely owned: it was our
            // own unreaped child, so killing and reaping it through the handle we hold is exact.
            let _ = child.kill();
            let _ = child.wait();
            return Err(E::InternalError);
        };
        let stdin = child.stdin.take();
        Ok((
            Self {
                child,
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

    pub fn stdin(&mut self) -> Option<&mut ChildStdin> {
        self.stdin.as_mut()
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
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
        let status = self.child.try_wait().map_err(|_| E::InternalError)?;
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

impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.stdin.take();
        if self.exit.is_none() {
            // Safety net for an error/panic path: this handle owns the unreaped child.
            let _ = self.signal(libc::SIGKILL);
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod tests;
