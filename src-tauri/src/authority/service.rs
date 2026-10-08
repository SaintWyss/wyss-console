//! The one owner of the desktop's connection to the authority.
//!
//! Events are never the source of what is shown. The view is always a canonical reading
//! (`authority/status`, then `runtime/status`), and the durable stream only says when to
//! read again and proves that nothing was skipped. The order is what closes the race
//! between a snapshot and the events around it:
//!
//! 1. the head is read before what it describes, so a snapshot is never older than its head;
//! 2. the cursor only moves by events that were received, one sequence at a time;
//! 3. a snapshot is published only when its head is at or past the cursor.
//!
//! An event committed before the head was read is already in the snapshot. One committed
//! after it is past the cursor, so `audit/watch` returns it and the snapshot is read again.
//! There is no third case.
use super::{
    link::{Backoff, Connection, Fault, Signal},
    view::{AuthorityView, Snapshot},
};
use crate::{
    contracts::{
        Sequence, WireUuid,
        control::{
            AuditRead, AuditWatch, AuthorityStatus, ControlError, ControlResult, MAX_PAGE,
            ResultValue,
        },
        event_envelope::EventEnvelopeV1,
    },
    control::client::ControlClient,
};
use std::{
    convert::Infallible,
    env, fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Longest one watch waits for a commit. A watch that ends empty still re-reads the
/// snapshot, so this is also how stale a change that wrote no event can get.
const WATCH_MS: u32 = 20_000;
/// The same while the runtime is between two resting states.
const SETTLING_WATCH_MS: u32 = 1_000;
/// Pause between reads while every watcher slot of the authority is taken.
const CROWDED_PAUSE: Duration = Duration::from_secs(1);
/// Missed events replayed after a reconnection. A longer gap is not replayed: the cursor
/// moves to the head, which the snapshot already describes.
const CATCH_UP_LIMIT: u64 = 2_000;
/// How long shutdown waits for the worker, which answers within one poll interval.
const JOIN_LIMIT: Duration = Duration::from_secs(3);

/// Counters of what the link did. For tests and diagnostics: none of it is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkStats {
    /// Handshakes an authority completed.
    pub connections: u64,
    /// Attempts and connections that ended.
    pub failures: u64,
    /// Durable events received, each exactly once and in order.
    pub events: u64,
    /// Canonical snapshots read.
    pub snapshots: u64,
    /// Times a known cursor was dropped for the head instead of being replayed.
    pub resyncs: u64,
}
#[derive(Default)]
struct Counters {
    connections: AtomicU64,
    failures: AtomicU64,
    events: AtomicU64,
    snapshots: AtomicU64,
    resyncs: AtomicU64,
}

struct Shared {
    view: Mutex<AuthorityView>,
    changed: Condvar,
    /// Ends the service and cancels the exchange in flight.
    stop: Arc<AtomicBool>,
    counters: Counters,
}
impl Shared {
    fn lock(&self) -> MutexGuard<'_, AuthorityView> {
        // Every update replaces the view in one step, so it is coherent after a panic.
        self.view.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
    /// Moves the link and publishes what changed. After `Stop` nothing changes any more.
    fn publish(&self, signal: Signal, update: impl FnOnce(&mut AuthorityView)) {
        let mut view = self.lock();
        let Some(next) = view.connection.on(signal) else {
            debug_assert_eq!(view.connection, Connection::Stopped, "{signal:?}");
            return;
        };
        let mut draft = view.clone();
        draft.connection = next;
        update(&mut draft);
        if draft != *view {
            draft.revision = view.revision + 1;
            *view = draft;
            self.changed.notify_all();
        }
    }
    /// Sleeps unless the service stops first. True when it is stopping.
    fn pause(&self, duration: Duration) -> bool {
        let deadline = Instant::now() + duration;
        let mut view = self.lock();
        loop {
            // The flag is read under the lock `shutdown` notifies under: no lost wakeup.
            if self.stopping() {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            view = self
                .changed
                .wait_timeout(view, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

struct Inner {
    shared: Arc<Shared>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Inner {
    fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.publish(Signal::Stop, forget);
        let handle = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            let deadline = Instant::now() + JOIN_LIMIT;
            while !handle.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Application-lifetime, read-only link to `wyss-control`. Cloning shares the one link.
#[derive(Clone)]
pub struct AuthorityService {
    inner: Arc<Inner>,
}
impl AuthorityService {
    /// The authority of this user's session: `$XDG_RUNTIME_DIR`.
    pub fn from_environment() -> Self {
        Self::start(env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
    }
    /// Follows the authority of one runtime directory. It never starts one.
    pub fn start(runtime_root: Option<PathBuf>) -> Self {
        let shared = Arc::new(Shared {
            view: Mutex::new(AuthorityView::initial()),
            changed: Condvar::new(),
            stop: Arc::new(AtomicBool::new(false)),
            counters: Counters::default(),
        });
        let worker = thread::Builder::new()
            .name("wyss-authority-link".into())
            .spawn({
                let shared = shared.clone();
                move || run(&shared, runtime_root.as_deref())
            });
        let inner = Inner {
            shared,
            worker: Mutex::new(worker.ok()),
        };
        // Without its worker the link cannot do anything: it says so instead of pretending.
        if inner
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
        {
            inner.shutdown();
        }
        Self {
            inner: Arc::new(inner),
        }
    }
    pub fn view(&self) -> AuthorityView {
        self.inner.shared.lock().clone()
    }
    /// The view once its revision is not `after`, or after `timeout`, whichever is first.
    /// A stopped service answers at once.
    pub fn wait_changed(&self, after: Option<u64>, timeout: Duration) -> AuthorityView {
        let shared = &self.inner.shared;
        let deadline = Instant::now() + timeout;
        let mut view = shared.lock();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if Some(view.revision) != after
                || view.connection == Connection::Stopped
                || left.is_zero()
            {
                return view.clone();
            }
            view = shared
                .changed
                .wait_timeout(view, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
    pub fn stats(&self) -> LinkStats {
        let c = &self.inner.shared.counters;
        LinkStats {
            connections: c.connections.load(Ordering::Acquire),
            failures: c.failures.load(Ordering::Acquire),
            events: c.events.load(Ordering::Acquire),
            snapshots: c.snapshots.load(Ordering::Acquire),
            resyncs: c.resyncs.load(Ordering::Acquire),
        }
    }
    /// Cancels the exchange in flight, closes the connection and ends the worker.
    /// Idempotent. Nothing is sent to the authority.
    pub fn shutdown(&self) {
        self.inner.shutdown();
    }
}

fn forget(view: &mut AuthorityView) {
    view.reason = None;
    view.authority = None;
    view.agent = None;
}

/// What outlives one connection: who answered last and how far its stream was read.
#[derive(Default)]
struct Memory {
    instance: Option<WireUuid>,
    stream: Option<WireUuid>,
    /// Last sequence received on `stream`. Zero is before its first event.
    cursor: Option<u64>,
    restarts: u64,
}
impl Memory {
    fn greet(&mut self, hello: &AuthorityStatus) {
        if self.instance.as_ref() != Some(&hello.authority_instance_id) {
            if self.instance.is_some() {
                self.restarts += 1;
            }
            self.instance = Some(hello.authority_instance_id.clone());
        }
        // A cursor means nothing on another stream.
        if self.stream.as_ref() != Some(&hello.stream_id) {
            self.stream = Some(hello.stream_id.clone());
            self.cursor = None;
        }
    }
    /// Places the cursor for a stream whose head is `head`. Returns the position to
    /// replay from, or `None` when the cursor went straight to the head.
    fn place(&mut self, head: u64, counters: &Counters) -> Option<u64> {
        match self.cursor {
            Some(cursor) if cursor <= head && head - cursor <= CATCH_UP_LIMIT => Some(cursor),
            known => {
                if known.is_some() {
                    counters.resyncs.fetch_add(1, Ordering::AcqRel);
                }
                self.cursor = Some(head);
                None
            }
        }
    }
    /// Receives one page. A sequence that is not the next one is a broken stream.
    fn receive(&mut self, events: &[EventEnvelopeV1], counters: &Counters) -> ControlResult<()> {
        let mut cursor = self.cursor.ok_or(ControlError::InternalError)?;
        for event in events {
            if cursor.checked_add(1) != Some(event.sequence().get()) {
                return Err(ControlError::CorrelationError);
            }
            cursor += 1;
            counters.events.fetch_add(1, Ordering::AcqRel);
        }
        self.cursor = Some(cursor);
        Ok(())
    }
}

fn after(cursor: u64) -> Option<Sequence> {
    cursor.to_string().parse().ok()
}

fn page(result: ResultValue) -> ControlResult<Vec<EventEnvelopeV1>> {
    match result {
        ResultValue::Audit { events, .. } => Ok(events),
        _ => Err(ControlError::InvalidRequest),
    }
}

/// The head first, then what it describes, both from the stream that was greeted.
fn snapshot(
    client: &mut ControlClient,
    stream: &WireUuid,
    counters: &Counters,
) -> ControlResult<Snapshot> {
    let status = client.status()?;
    if &status.stream_id != stream {
        return Err(ControlError::CorrelationError);
    }
    let runtime = client.runtime_status()?;
    counters.snapshots.fetch_add(1, Ordering::AcqRel);
    Ok(Snapshot { status, runtime })
}

/// One connection, from its handshake to the error that ends it.
fn follow(
    shared: &Shared,
    client: &mut ControlClient,
    memory: &mut Memory,
    backoff: &mut Backoff,
) -> ControlResult<Infallible> {
    let counters = &shared.counters;
    memory.greet(client.hello_status());
    let stream = client.hello_status().stream_id.clone();
    let read = |cursor: u64| AuditRead {
        stream_id: stream.clone(),
        after_sequence: after(cursor),
        limit: MAX_PAGE,
    };
    let mut current = snapshot(client, &stream, counters)?;
    if let Some(mut cursor) = memory.place(current.head(), counters) {
        while cursor < current.head() {
            let events = page(client.audit(read(cursor))?)?;
            // The head promised history after the cursor.
            if events.is_empty() {
                return Err(ControlError::CorrelationError);
            }
            memory.receive(&events, counters)?;
            cursor = memory.cursor.ok_or(ControlError::InternalError)?;
        }
    }
    loop {
        let cursor = memory.cursor.ok_or(ControlError::InternalError)?;
        // Replay or a watch moved the cursor past what was read: read again.
        if current.head() < cursor {
            current = snapshot(client, &stream, counters)?;
            // A head behind events it already served is not a stream.
            if current.head() < cursor {
                return Err(ControlError::CorrelationError);
            }
        }
        shared.publish(Signal::Synchronized, |view| {
            view.reason = None;
            view.authority = Some(current.authority(memory.restarts));
            view.agent = Some(current.agent());
        });
        backoff.reset();
        let events = match client.audit_watch(AuditWatch {
            stream_id: stream.clone(),
            after_sequence: after(cursor),
            limit: MAX_PAGE,
            wait_ms: if current.settling() {
                SETTLING_WATCH_MS
            } else {
                WATCH_MS
            },
        }) {
            Ok(result) => page(result)?,
            // Every watcher slot is taken: read without waiting, at a bounded pace.
            Err(ControlError::CapacityExceeded) => {
                let events = page(client.audit(read(cursor))?)?;
                if events.is_empty() && shared.pause(CROWDED_PAUSE) {
                    return Err(ControlError::TransportError);
                }
                events
            }
            Err(error) => return Err(error),
        };
        memory.receive(&events, counters)?;
        // Read again whether or not anything arrived: an empty watch is the bounded
        // re-read for changes that write no event.
        current = snapshot(client, &stream, counters)?;
    }
}

/// One attempt: find the socket, greet, follow. Returns why it ended.
fn attempt(
    shared: &Shared,
    root: Option<&Path>,
    memory: &mut Memory,
    backoff: &mut Backoff,
) -> Fault {
    let Some(root) = root else {
        return Fault::UnsafePath;
    };
    // An authority that never ran left nothing to validate: it is absent, not unsafe.
    if fs::symlink_metadata(ControlClient::socket_path(root))
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Fault::Absent;
    }
    let mut client = match ControlClient::connect_cancellable(root, shared.stop.clone()) {
        Ok(client) => client,
        Err(error) => return Fault::connecting(error),
    };
    shared.counters.connections.fetch_add(1, Ordering::AcqRel);
    shared.publish(Signal::Greeted, |view| view.reason = None);
    match follow(shared, &mut client, memory, backoff) {
        Err(error) => Fault::of(error),
        Ok(never) => match never {},
    }
}

fn run(shared: &Shared, root: Option<&Path>) {
    let mut memory = Memory::default();
    let mut backoff = Backoff::default();
    loop {
        let fault = attempt(shared, root, &mut memory, &mut backoff);
        if shared.stopping() {
            return;
        }
        shared.counters.failures.fetch_add(1, Ordering::AcqRel);
        shared.publish(Signal::Failed, |view| {
            forget(view);
            view.reason = Some(fault);
        });
        if shared.pause(backoff.next()) {
            return;
        }
        shared.publish(Signal::RetryDue, |_| ());
    }
}
