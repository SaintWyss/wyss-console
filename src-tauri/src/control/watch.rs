//! Live wakeups for durable-event watchers.
//!
//! A wakeup is a hint that the durable head moved. It carries no event: a woken watcher
//! reads SQLite again by its own cursor, so a lost or spurious wakeup can delay an answer
//! but can never lose, invent or reorder history. Nothing here is durable and no cursor is
//! kept for any client.
use crate::contracts::control::{ControlError, ControlResult};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, poll},
    sys::socket::{MsgFlags, recv},
};
use std::{
    io::{Read, Write},
    os::{
        fd::{AsFd, AsRawFd},
        unix::net::UnixStream,
    },
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};

/// Connections that may wait at once. It is half of the connection slots, so the other
/// half always remains for approval responses, cancels, revocations, closes and runtime
/// operations however long watchers wait.
pub(super) const MAX_WATCHERS: usize = super::server::MAX_CLIENTS / 2;
const _: () = assert!(MAX_WATCHERS > 0 && MAX_WATCHERS < super::server::MAX_CLIENTS);

struct State {
    /// The highest committed sequence the core has announced. Never ahead of SQLite.
    head: u64,
    closed: bool,
    next: u64,
    /// The write end of each waiting connection's wake channel.
    waiters: Vec<(u64, UnixStream)>,
}
pub(super) struct EventSignal {
    state: Mutex<State>,
}
/// Why a watcher stopped waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Wake {
    /// The durable head is past the cursor: read again.
    Advanced,
    /// The requested wait elapsed with nothing new.
    Elapsed,
    /// The authority is shutting down.
    Closed,
    /// The client hung up. Nothing was consumed and nothing changes.
    ClientGone,
    /// The client sent its next request instead of waiting.
    ClientSpoke,
}
impl EventSignal {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                head: 0,
                closed: false,
                next: 0,
                waiters: Vec::new(),
            }),
        })
    }
    fn lock(&self) -> MutexGuard<'_, State> {
        // The state stays coherent across a panicking holder: every update is one step.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    /// Announces a committed head. Called by the core only after the commit returned.
    pub(super) fn publish(&self, head: u64) {
        let mut state = self.lock();
        if head > state.head {
            state.head = head;
            wake(&state.waiters);
        }
    }
    /// Shutdown: every waiter returns at once and no new one is admitted.
    pub(super) fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        wake(&state.waiters);
    }
    /// Takes one watcher slot or fails at once. The slot is released when the waiter drops.
    pub(super) fn register(self: &Arc<Self>) -> ControlResult<Waiter> {
        let (reader, writer) = UnixStream::pair().map_err(|_| ControlError::InternalError)?;
        reader
            .set_nonblocking(true)
            .and_then(|()| writer.set_nonblocking(true))
            .map_err(|_| ControlError::InternalError)?;
        let mut state = self.lock();
        if state.closed {
            return Err(ControlError::TransportError);
        }
        if state.waiters.len() >= MAX_WATCHERS {
            return Err(ControlError::CapacityExceeded);
        }
        let id = state.next;
        state.next = state.next.wrapping_add(1);
        state.waiters.push((id, writer));
        Ok(Waiter {
            signal: self.clone(),
            id,
            reader,
        })
    }
    #[cfg(test)]
    pub(super) fn waiting(&self) -> usize {
        self.lock().waiters.len()
    }
}
/// One byte per waiter. A full channel is already readable, so a failed write loses nothing.
fn wake(waiters: &[(u64, UnixStream)]) {
    for (_, writer) in waiters {
        let _ = (&*writer).write(&[1]);
    }
}
pub(super) struct Waiter {
    signal: Arc<EventSignal>,
    id: u64,
    reader: UnixStream,
}
impl Waiter {
    /// Sleeps until the announced head passes `cursor`, the deadline, shutdown or the
    /// client's end of `client` changes.
    ///
    /// No wakeup is lost to ordering. This waiter was registered under the lock before the
    /// first check below, and `publish` moves the head and writes the wake byte under that
    /// same lock. A commit announced before registration is seen by the check; one
    /// announced after it leaves a byte that makes the sleep return, and the byte is only
    /// drained before the next check, which then sees the head that wrote it.
    pub(super) fn wait(&mut self, cursor: u64, client: &UnixStream, deadline: Instant) -> Wake {
        loop {
            {
                let state = self.signal.lock();
                if state.closed {
                    return Wake::Closed;
                }
                if state.head > cursor {
                    return Wake::Advanced;
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Wake::Elapsed;
            }
            let (woken, client_ready) = {
                let mut fds = [
                    PollFd::new(self.reader.as_fd(), PollFlags::POLLIN),
                    PollFd::new(client.as_fd(), PollFlags::POLLIN),
                ];
                match poll(&mut fds, left.as_millis().clamp(1, 60_000) as u16) {
                    Ok(_) | Err(Errno::EINTR) => (),
                    Err(_) => return Wake::ClientGone,
                }
                let ready = |fd: &PollFd<'_>| fd.revents().is_some_and(|r| !r.is_empty());
                (ready(&fds[0]), ready(&fds[1]))
            };
            if woken {
                let mut drained = [0u8; 64];
                while matches!((&self.reader).read(&mut drained), Ok(n) if n > 0) {}
            }
            if client_ready {
                let mut byte = [0u8; 1];
                match recv(
                    client.as_raw_fd(),
                    &mut byte,
                    MsgFlags::MSG_PEEK | MsgFlags::MSG_DONTWAIT,
                ) {
                    Ok(0) => return Wake::ClientGone,
                    Ok(_) => return Wake::ClientSpoke,
                    Err(Errno::EAGAIN | Errno::EINTR) => (),
                    Err(_) => return Wake::ClientGone,
                }
            }
        }
    }
}
impl Drop for Waiter {
    fn drop(&mut self) {
        self.signal.lock().waiters.retain(|(id, _)| *id != self.id);
    }
}
#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
