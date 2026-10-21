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
    engine::{Containment, StopState, Worker},
    ownership::{ContainmentPolicy, Grace, OwnedChild, PendingOwnedSpawn, Progress},
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
    /// Spawned under the PID-namespace containment policy.
    namespace: bool,
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
    /// Legacy synchronous fixture API. Application spawners use DeferredProcessWorker
    /// so outstanding spawn cleanup cannot escape the engine's runtime capacity.
    /// The caller picks the program and arguments; no environment or credential is added.
    pub fn spawn(command: &mut Command, grace: Grace) -> ControlResult<Self> {
        Self::spawn_with(command, grace, ContainmentPolicy::DirectChildOnly)
    }
    /// As `spawn`, with an explicit containment requirement (`Unsupported` if it cannot be met).
    pub fn spawn_with(
        command: &mut Command,
        grace: Grace,
        policy: ContainmentPolicy,
    ) -> ControlResult<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (child, _) = OwnedChild::spawn(command, policy, grace)?;
        Self::from_owned(child, policy).map_err(|(error, _child)| error)
    }

    fn from_owned(
        mut child: OwnedChild,
        policy: ContainmentPolicy,
    ) -> Result<Self, (E, Box<OwnedChild>)> {
        let pipes = (|| {
            let stdout = child.take_stdout().ok_or(E::TransportError)?;
            let stderr = child.take_stderr().ok_or(E::TransportError)?;
            let stdin_fd = child.stdin().ok_or(E::TransportError)?.as_raw_fd();
            nonblocking(stdin_fd)?;
            nonblocking(stdout.as_raw_fd())?;
            nonblocking(stderr.as_raw_fd())?;
            Ok((stdout, stderr))
        })();
        let (stdout, stderr) = match pipes {
            Ok(pipes) => pipes,
            Err(error) => return Err((error, Box::new(child))),
        };
        Ok(Self {
            child,
            stdout,
            stderr,
            out: VecDeque::new(),
            inbuf: Vec::new(),
            failed: None,
            eof: false,
            namespace: policy == ContainmentPolicy::DescendantsContained,
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
    fn send_critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        // Linux guarantees atomic pipe writes up to PIPE_BUF (4096 bytes).
        // An approval must never sit in our own deferred output queue: an
        // intervening revocation could otherwise leave a stale executable frame.
        if frame.len() > 4095 || frame.contains(&b'\n') {
            return Err(E::FrameTooLarge);
        }
        if !self.out.is_empty() {
            return Err(E::CapacityExceeded);
        }
        let mut payload = Vec::with_capacity(frame.len() + 1);
        payload.extend_from_slice(frame);
        payload.push(b'\n');
        let Some(stdin) = self.child.stdin() else {
            return Err(E::TransportError);
        };
        match stdin.write(&payload) {
            Ok(n) if n == payload.len() => Ok(()),
            Ok(0) => Err(E::CapacityExceeded),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                Err(E::CapacityExceeded)
            }
            // A partial critical write cannot be retried safely. The parser may
            // have received a prefix; fence the worker and require owned cleanup.
            Ok(_) | Err(_) => {
                self.failed = Some(E::TransportError);
                Err(E::TransportError)
            }
        }
    }
    fn flush(&mut self) -> ControlResult<()> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        for _ in 0..32 {
            if self.out.is_empty() {
                break;
            }
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
        for _ in 0..32 {
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
                Ok(0) => {
                    self.eof = true;
                    if !self.inbuf.is_empty() {
                        self.failed = Some(E::TransportError);
                        return Err(E::TransportError);
                    }
                }
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
    fn containment(&mut self) -> Containment {
        if self.namespace {
            if self.child.namespace_contained() {
                Containment::PidNamespace
            } else {
                Containment::DirectChildOnly
            }
        } else {
            Containment::DirectChildOnly
        }
    }
    fn descendants(&self) -> usize {
        self.child.descendant_count()
    }
    fn pid_for_diagnostics(&self) -> Option<i32> {
        Some(self.child.pid_for_diagnostics())
    }
}

/// Application process worker: reserving ownership returns immediately, while
/// spawn, gate pinning and refused-child cleanup advance cooperatively. A failed
/// initialization retains this worker (and the engine slot) until reap proof.
pub struct DeferredProcessWorker {
    pending: Option<PendingOwnedSpawn>,
    worker: Option<ProcessWorker>,
    cleanup: Option<OwnedChild>,
    queued: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    failed: Option<E>,
    stopped_at: Option<Instant>,
    complete: bool,
    policy: ContainmentPolicy,
    grace: Grace,
}
impl DeferredProcessWorker {
    pub fn spawn(
        mut command: Command,
        grace: Grace,
        policy: ContainmentPolicy,
    ) -> ControlResult<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let pending = PendingOwnedSpawn::spawn(command, policy, grace)?;
        Ok(Self::pending(pending, grace, policy))
    }
    fn pending(pending: PendingOwnedSpawn, grace: Grace, policy: ContainmentPolicy) -> Self {
        Self {
            pending: Some(pending),
            worker: None,
            cleanup: None,
            queued: VecDeque::new(),
            queued_bytes: 0,
            failed: None,
            stopped_at: None,
            complete: false,
            policy,
            grace,
        }
    }
    fn advance(&mut self, now: Instant) {
        let outcome = self.pending.as_mut().and_then(|pending| {
            let outcome = pending.poll(now);
            self.failed = self.failed.or(pending.failure());
            outcome
        });
        if let Some(outcome) = outcome {
            self.pending.take();
            match outcome {
                Ok((child, _)) => match ProcessWorker::from_owned(child, self.policy) {
                    Ok(mut worker) => {
                        if self.stopped_at.is_some() {
                            worker.begin_stop(now);
                        }
                        self.worker = Some(worker);
                    }
                    Err((error, mut child)) => {
                        self.failed = Some(error);
                        child.begin_stop(now);
                        self.cleanup = Some(*child);
                    }
                },
                Err(error) => {
                    self.failed = Some(error);
                    self.complete = true;
                }
            }
        }
        if self.failed.is_some() || self.stopped_at.is_some() {
            self.queued.clear();
            self.queued_bytes = 0;
        }
    }
}
impl Worker for DeferredProcessWorker {
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.advance(Instant::now());
        if let Some(error) = self.failed {
            return Err(error);
        }
        if self.stopped_at.is_some() {
            return Err(E::TransportError);
        }
        if frame.len() > OUTBOUND_FRAME_MAX || frame.contains(&b'\n') {
            return Err(E::FrameTooLarge);
        }
        if self.queued_bytes + frame.len() + 1 > OUTBOUND_BUFFER_MAX {
            return Err(E::CapacityExceeded);
        }
        self.queued.push_back(frame.to_vec());
        self.queued_bytes += frame.len() + 1;
        self.flush()
    }
    fn send_critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.advance(Instant::now());
        if let Some(error) = self.failed {
            return Err(error);
        }
        if self.stopped_at.is_some() {
            return Err(E::TransportError);
        }
        if !self.queued.is_empty() {
            return Err(E::CapacityExceeded);
        }
        self.worker
            .as_mut()
            .ok_or(E::CapacityExceeded)?
            .send_critical(frame)
    }
    fn flush(&mut self) -> ControlResult<()> {
        self.advance(Instant::now());
        if let Some(error) = self.failed {
            return Err(error);
        }
        if self.stopped_at.is_some() {
            return Err(E::TransportError);
        }
        let Some(worker) = &mut self.worker else {
            return Ok(());
        };
        // Bound the number of small initialization frames moved per quantum.
        for _ in 0..16 {
            let Some(frame) = self.queued.front() else {
                break;
            };
            match worker.send(frame) {
                Ok(()) => {
                    self.queued_bytes -= frame.len() + 1;
                    self.queued.pop_front();
                }
                Err(E::CapacityExceeded) => break,
                Err(error) => {
                    self.failed = Some(error);
                    return Err(error);
                }
            }
        }
        worker.flush()
    }
    fn inbound(&mut self, max_frames: usize) -> ControlResult<Vec<Vec<u8>>> {
        self.advance(Instant::now());
        if let Some(error) = self.failed {
            return Err(error);
        }
        match &mut self.worker {
            Some(worker) => worker.inbound(max_frames),
            None => Ok(Vec::new()),
        }
    }
    fn has_exited(&mut self) -> bool {
        self.advance(Instant::now());
        self.complete || self.worker.as_mut().is_some_and(Worker::has_exited)
    }
    fn begin_stop(&mut self, now: Instant) {
        self.stopped_at.get_or_insert(now);
        if let Some(pending) = &mut self.pending {
            pending.cancel();
        }
        self.advance(now);
        if let Some(worker) = &mut self.worker {
            worker.begin_stop(now);
        }
        if let Some(child) = &mut self.cleanup {
            child.begin_stop(now);
        }
    }
    fn poll_stop(&mut self, now: Instant) -> StopState {
        self.advance(now);
        if self.complete {
            return StopState::Reaped;
        }
        if let Some(worker) = &mut self.worker {
            return worker.poll_stop(now);
        }
        if let Some(child) = &mut self.cleanup {
            return match child.poll_stop(now) {
                Ok(Progress::Reaped(_)) => {
                    self.complete = true;
                    StopState::Reaped
                }
                Ok(Progress::Pending(_)) => StopState::Pending,
                Ok(Progress::Stuck) | Err(_) => StopState::Stuck,
            };
        }
        if self.stopped_at.is_some_and(|at| {
            now.saturating_duration_since(at) >= self.grace.eof + self.grace.term + self.grace.kill
        }) {
            StopState::Stuck
        } else {
            StopState::Pending
        }
    }
    fn containment(&mut self) -> Containment {
        self.advance(Instant::now());
        self.worker
            .as_mut()
            .map_or(Containment::DirectChildOnly, Worker::containment)
    }
    fn descendants(&self) -> usize {
        self.worker.as_ref().map_or(0, Worker::descendants)
    }
    fn pid_for_diagnostics(&self) -> Option<i32> {
        self.worker.as_ref().and_then(Worker::pid_for_diagnostics)
    }
}

#[cfg(test)]
#[path = "process_worker_tests.rs"]
mod tests;
