//! A real, process-backed `Worker`: bounded, non-blocking newline-framed pipes around an owned
//! child. Inactive: nothing here launches a provider or touches credentials; the tests use plain
//! shell fixtures.
//!
//! Every path is bounded and non-blocking. Outbound frames queue in a fixed buffer and a full
//! buffer is an immediate refusal (back pressure), never a wait; inbound reading stops at a frame
//! or byte allowance per call and a line over the provider frame bound fails the worker; stderr is
//! drained into nothing (content is never kept) so a chatty child cannot wedge on a full pipe.
//! Lifecycle (EOF, TERM, KILL, reap) is the owned handle's, never a PID or name.
use super::{
    engine::{StopState, Worker},
    ownership::{ContainmentPolicy, Grace, OwnedChild, Progress},
};
use crate::contracts::control::{ControlError as E, ControlResult};
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use std::{
    collections::VecDeque,
    io::{ErrorKind, Read, Write},
    os::fd::AsRawFd,
    process::{ChildStderr, ChildStdout, Command, Stdio},
    time::Instant,
};

/// Contract ceilings (docs/contracts/f8/resources.json).
const OUTBOUND_FRAME_MAX: usize = 65_536;
const INBOUND_FRAME_MAX: usize = 262_144;
const OUTBOUND_BUFFER_MAX: usize = 262_144;
/// Bytes read from one pipe in one call: bounds the work per `inbound`.
const READ_BUDGET: usize = 128 * 1024;
const CHUNK: usize = 16 * 1024;

pub struct ProcessWorker {
    child: OwnedChild,
    stdout: ChildStdout,
    stderr: ChildStderr,
    out: VecDeque<u8>,
    inbuf: Vec<u8>,
    failed: Option<E>,
    eof: bool,
}

fn nonblocking(fd: i32) -> ControlResult<()> {
    let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(|_| E::TransportError)?;
    fcntl(
        fd,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map(|_| ())
    .map_err(|_| E::TransportError)
}

impl ProcessWorker {
    /// Spawns the command as an owned child with piped stdio. The caller picks the program and
    /// arguments; this type adds no environment, shell or credential.
    pub fn spawn(command: &mut Command, grace: Grace) -> ControlResult<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut child, _) = OwnedChild::spawn(command, ContainmentPolicy::DirectChildOnly, grace)?;
        let stdout = child.take_stdout().ok_or(E::TransportError)?;
        let stderr = child.take_stderr().ok_or(E::TransportError)?;
        let stdin_fd = child.stdin().ok_or(E::TransportError)?.as_raw_fd();
        nonblocking(stdin_fd)?;
        nonblocking(stdout.as_raw_fd())?;
        nonblocking(stderr.as_raw_fd())?;
        Ok(Self {
            child,
            stdout,
            stderr,
            out: VecDeque::new(),
            inbuf: Vec::new(),
            failed: None,
            eof: false,
        })
    }
    pub fn failure(&self) -> Option<E> {
        self.failed
    }
    pub fn buffered_outbound(&self) -> usize {
        self.out.len()
    }
    /// Stdout reached EOF: the provider closed its side.
    pub fn stdout_closed(&self) -> bool {
        self.eof
    }

    fn drain_stderr(&mut self) {
        let mut scratch = [0u8; 4096];
        for _ in 0..8 {
            match self.stderr.read(&mut scratch) {
                Ok(0) | Err(_) => break,
                Ok(_) => (),
            }
        }
    }
}

impl Worker for ProcessWorker {
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        if frame.len() > OUTBOUND_FRAME_MAX || frame.contains(&b'\n') {
            return Err(E::FrameTooLarge);
        }
        if self.out.len() + frame.len() + 1 > OUTBOUND_BUFFER_MAX {
            // The child is not reading: refuse at once, never wait.
            return Err(E::CapacityExceeded);
        }
        self.out.extend(frame);
        self.out.push_back(b'\n');
        self.flush()
    }
    fn flush(&mut self) -> ControlResult<()> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        while !self.out.is_empty() {
            let Some(stdin) = self.child.stdin() else {
                self.failed = Some(E::TransportError);
                return Err(E::TransportError);
            };
            let (head, _) = self.out.as_slices();
            match stdin.write(head) {
                Ok(0) => break,
                Ok(n) => {
                    self.out.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => (),
                Err(_) => {
                    self.failed = Some(E::TransportError);
                    return Err(E::TransportError);
                }
            }
        }
        Ok(())
    }
    fn inbound(&mut self, max_frames: usize) -> ControlResult<Vec<Vec<u8>>> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        self.drain_stderr();
        let mut frames = Vec::new();
        let mut budget = READ_BUDGET;
        loop {
            // Complete frames already buffered are delivered before reading more.
            while frames.len() < max_frames {
                let Some(end) = self.inbuf.iter().position(|b| *b == b'\n') else {
                    break;
                };
                let line: Vec<u8> = self.inbuf.drain(..=end).collect();
                if line.len() - 1 > INBOUND_FRAME_MAX {
                    self.failed = Some(E::FrameTooLarge);
                    return Err(E::FrameTooLarge);
                }
                if line.len() > 1 {
                    frames.push(line[..line.len() - 1].to_vec());
                }
            }
            if frames.len() >= max_frames || budget == 0 || self.eof {
                break;
            }
            // A partial line longer than the bound can never become a valid frame.
            if self.inbuf.len() > INBOUND_FRAME_MAX {
                self.failed = Some(E::FrameTooLarge);
                return Err(E::FrameTooLarge);
            }
            let mut chunk = [0u8; CHUNK];
            let want = CHUNK.min(budget);
            match self.stdout.read(&mut chunk[..want]) {
                Ok(0) => self.eof = true,
                Ok(n) => {
                    budget -= n;
                    self.inbuf.extend_from_slice(&chunk[..n]);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => (),
                Err(_) => {
                    self.failed = Some(E::TransportError);
                    return Err(E::TransportError);
                }
            }
        }
        Ok(frames)
    }
    fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
    fn begin_stop(&mut self, now: Instant) {
        self.child.begin_stop(now);
    }
    fn poll_stop(&mut self, now: Instant) -> StopState {
        match self.child.poll_stop(now) {
            Ok(Progress::Reaped(_)) => StopState::Reaped,
            Ok(Progress::Stuck) => StopState::Stuck,
            Ok(Progress::Pending(_)) => StopState::Pending,
            // Not provably gone: keep the slot and the handle.
            Err(_) => StopState::Stuck,
        }
    }
    fn descendants(&self) -> usize {
        self.child.descendant_count()
    }
    fn pid_for_diagnostics(&self) -> Option<i32> {
        Some(self.child.pid_for_diagnostics())
    }
}

#[cfg(test)]
#[path = "process_worker_tests.rs"]
mod tests;
