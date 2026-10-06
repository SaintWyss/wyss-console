//! Linux-first, nonblocking owned pipes. No reader/writer threads or output queues.
use super::{ProbeError, ProbeResult};
use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl},
    poll::{PollFd, PollFlags, poll},
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    os::fd::{AsFd, AsRawFd},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub(super) const STDERR_LIMIT: usize = 32 * 1024;
const SCRATCH: usize = 4096;

#[derive(Debug, Default, Serialize)]
pub struct ShutdownReport {
    pub reaped: bool,
    pub stdin_closed: bool,
    pub terminated: bool,
    pub killed: bool,
    pub exit_code: Option<i32>,
}

pub(super) struct OwnedProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    tail: VecDeque<u8>,
    pub stderr_bytes: u64,
    status: Option<ExitStatus>,
}

fn io_error(_: impl std::fmt::Debug) -> ProbeError {
    ProbeError::Transport
}
fn nonblocking(fd: i32) -> ProbeResult<()> {
    let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(io_error)?;
    fcntl(
        fd,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(io_error)?;
    Ok(())
}
impl OwnedProcess {
    pub fn spawn(command: &mut Command) -> ProbeResult<Self> {
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| ProbeError::Spawn)?;
        // Establish ownership before fallible pipe setup, including its Drop guard.
        let mut owner = Self {
            child,
            stdin: None,
            stdout: None,
            stderr: None,
            tail: VecDeque::with_capacity(STDERR_LIMIT),
            stderr_bytes: 0,
            status: None,
        };
        owner.stdin = owner.child.stdin.take();
        owner.stdout = owner.child.stdout.take();
        owner.stderr = owner.child.stderr.take();
        for fd in [
            owner.stdin.as_ref().unwrap().as_raw_fd(),
            owner.stdout.as_ref().unwrap().as_raw_fd(),
            owner.stderr.as_ref().unwrap().as_raw_fd(),
        ] {
            nonblocking(fd)?;
        }
        Ok(owner)
    }
    pub fn close_input(&mut self) {
        self.stdin.take();
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    pub fn write(&mut self, bytes: &[u8], deadline: Instant) -> ProbeResult<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            if Instant::now() >= deadline {
                return Err(ProbeError::Timeout);
            }
            self.drain_stderr()?;
            match self
                .stdin
                .as_mut()
                .ok_or(ProbeError::Transport)?
                .write(&bytes[offset..])
            {
                Ok(0) => return Err(ProbeError::Transport),
                Ok(n) => offset += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    self.wait(deadline, true)?
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(ProbeError::Transport),
            }
        }
        Ok(())
    }
    pub fn read(&mut self, buffer: &mut [u8]) -> ProbeResult<Option<usize>> {
        self.drain_stderr()?;
        let Some(stdout) = self.stdout.as_mut() else {
            return Ok(Some(0));
        };
        match stdout.read(buffer) {
            Ok(0) => {
                self.stdout.take();
                Ok(Some(0))
            }
            Ok(n) => Ok(Some(n)),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err(ProbeError::Transport),
        }
    }
    #[cfg(test)]
    pub(super) fn tail_len(&self) -> usize {
        self.tail.len()
    }
    pub(super) fn retain_stderr(&mut self, bytes: &[u8]) {
        self.stderr_bytes = self.stderr_bytes.saturating_add(bytes.len() as u64);
        for &byte in bytes {
            if self.tail.len() == STDERR_LIMIT {
                self.tail.pop_front();
            }
            self.tail.push_back(byte);
        }
    }
    fn drain_stderr(&mut self) -> ProbeResult<()> {
        // Fixed work per pump prevents a stderr flood from starving deadlines/stdout.
        for _ in 0..16 {
            let Some(stderr) = self.stderr.as_mut() else {
                break;
            };
            let mut scratch = [0; SCRATCH];
            match stderr.read(&mut scratch) {
                Ok(0) => {
                    self.stderr.take();
                    break;
                }
                Ok(n) => self.retain_stderr(&scratch[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(ProbeError::Transport),
            }
        }
        Ok(())
    }
    pub fn wait(&self, deadline: Instant, writing: bool) -> ProbeResult<()> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ProbeError::Timeout);
        }
        let mut fds = Vec::with_capacity(3);
        if !writing && let Some(stdout) = &self.stdout {
            fds.push(PollFd::new(stdout.as_fd(), PollFlags::POLLIN));
        }
        if let Some(stderr) = &self.stderr {
            fds.push(PollFd::new(stderr.as_fd(), PollFlags::POLLIN));
        }
        if writing && let Some(stdin) = &self.stdin {
            fds.push(PollFd::new(stdin.as_fd(), PollFlags::POLLOUT));
        }
        let millis = remaining.as_millis().clamp(1, 50) as u16;
        match poll(&mut fds, millis) {
            Ok(_) | Err(Errno::EINTR) => Ok(()),
            Err(_) => Err(ProbeError::Transport),
        }
    }
    fn exited(&mut self) -> ProbeResult<bool> {
        if self.status.is_none() {
            self.status = self.child.try_wait().map_err(io_error)?;
        }
        Ok(self.status.is_some())
    }
    fn await_exit(&mut self, duration: Duration) -> ProbeResult<bool> {
        let deadline = Instant::now() + duration;
        while !self.exited()? {
            // Discard shutdown stdout, still consuming stderr without retaining protocol content.
            let mut scratch = [0; SCRATCH];
            self.read(&mut scratch)?;
            if Instant::now() >= deadline {
                return Ok(false);
            }
            self.wait(deadline, false)?;
        }
        self.drain_stderr()?;
        Ok(true)
    }
    pub fn shutdown(&mut self) -> ProbeResult<ShutdownReport> {
        self.stdin.take();
        let mut result = ShutdownReport {
            stdin_closed: true,
            ..Default::default()
        };
        if !self.await_exit(Duration::from_secs(1))? {
            // The PID comes only from this unreaped Child; no external PID is accepted.
            kill(Pid::from_raw(self.child.id() as i32), Signal::SIGTERM)
                .or_else(|e| if e == Errno::ESRCH { Ok(()) } else { Err(e) })
                .map_err(io_error)?;
            result.terminated = true;
            if !self.await_exit(Duration::from_millis(500))? {
                self.child.kill().map_err(io_error)?;
                result.killed = true;
                if !self.await_exit(Duration::from_secs(2))? {
                    return Err(ProbeError::Shutdown);
                }
            }
        }
        result.reaped = self.status.is_some();
        result.exit_code = self.status.and_then(|s| s.code());
        self.stdout.take();
        self.stderr.take();
        self.tail.clear();
        Ok(result)
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.stdin.take();
        // Error/panic safety net. SIGKILL + wait ensures this owned child is not detached.
        if self.status.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
