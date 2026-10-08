use super::{
    framing::{read_frame, write_frame},
    paths::Paths,
    runtime::{Observation, Runtime},
    store::{Store, uuid},
};
use crate::contracts::{WireUuid, control::*};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, poll},
    sys::{
        signal::{SigSet, Signal},
        signalfd::{SfdFlags, SignalFd},
        socket::{
            AddressFamily, SockFlag, SockType, UnixAddr, connect, getsockopt, socket, sockopt,
        },
    },
    unistd::geteuid,
};
use std::{
    fs,
    os::{
        fd::{AsFd, AsRawFd},
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

pub const MAX_CLIENTS: usize = 8;
const QUEUE: usize = 8;
const HELLO_TIMEOUT: Duration = Duration::from_secs(3);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);
pub(super) enum CoreMessage {
    Client(Job),
    Runtime(
        crate::contracts::identities::AgentRuntimeInstanceId,
        Observation,
    ),
    Shutdown,
}
pub(super) struct Job {
    op: Operation,
    reply: SyncSender<ControlResult<ResultValue>>,
}
pub(crate) struct SocketGuard {
    path: PathBuf,
    dev: u64,
    ino: u64,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Ok(m) = fs::symlink_metadata(&self.path)
            && m.file_type().is_socket()
            && m.uid() == geteuid().as_raw()
            && m.dev() == self.dev
            && m.ino() == self.ino
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
pub(crate) fn bind(path: &Path) -> ControlResult<(UnixListener, SocketGuard)> {
    UnixAddr::new(path).map_err(|_| ControlError::UnsafePath)?;
    match fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.file_type().is_socket() || m.uid() != geteuid().as_raw() {
                return Err(ControlError::UnsafePath);
            }
            // Nonblocking connect: an active or indeterminate listener is never unlinked.
            let fd = socket(
                AddressFamily::Unix,
                SockType::Stream,
                SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
                None,
            )
            .map_err(|_| ControlError::TransportError)?;
            let addr = UnixAddr::new(path).map_err(|_| ControlError::UnsafePath)?;
            match connect(fd.as_raw_fd(), &addr) {
                Err(Errno::ECONNREFUSED) => (),
                _ => return Err(ControlError::AlreadyRunning),
            }
            let current = fs::symlink_metadata(path).map_err(|_| ControlError::UnsafePath)?;
            if current.dev() != m.dev()
                || current.ino() != m.ino()
                || !current.file_type().is_socket()
            {
                return Err(ControlError::UnsafePath);
            }
            fs::remove_file(path).map_err(|_| ControlError::UnsafePath)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err(ControlError::UnsafePath),
    }
    let listener = UnixListener::bind(path).map_err(|_| ControlError::AlreadyRunning)?;
    let m = fs::symlink_metadata(path).map_err(|_| ControlError::UnsafePath)?;
    let guard = SocketGuard {
        path: path.to_owned(),
        dev: m.dev(),
        ino: m.ino(),
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| ControlError::UnsafePath)?;
    listener
        .set_nonblocking(true)
        .map_err(|_| ControlError::TransportError)?;
    Ok((listener, guard))
}
pub(crate) fn check_uid(uid: u32) -> ControlResult<()> {
    if uid == geteuid().as_raw() {
        Ok(())
    } else {
        Err(ControlError::PeerRejected)
    }
}
pub(crate) fn peer(stream: &UnixStream) -> ControlResult<()> {
    let creds =
        getsockopt(stream, sockopt::PeerCredentials).map_err(|_| ControlError::PeerRejected)?;
    check_uid(creds.uid())
}
fn connection(
    mut stream: UnixStream,
    core: SyncSender<CoreMessage>,
    instance: WireUuid,
    stop: Arc<AtomicBool>,
) {
    if peer(&stream).is_err() || stream.set_nonblocking(true).is_err() {
        return;
    }
    let mut hello = false;
    loop {
        let timeout = if hello { READ_TIMEOUT } else { HELLO_TIMEOUT };
        let bytes = match read_frame(&mut stream, Instant::now() + timeout, &stop) {
            Ok(Some(b)) => b,
            _ => return,
        };
        // If an invalid envelope still has an unambiguous UUID, return a safe error.
        let request = match Request::parse(&bytes) {
            Ok(r) => r,
            Err(e) => {
                if let Ok(v) = unique_json(&bytes)
                    && let Some(id) = v.get("request_id")
                    && let Ok(id) = serde_json::from_value::<WireUuid>(id.clone())
                {
                    let response = Response {
                        request_id: id,
                        authority_instance_id: instance.clone(),
                        outcome: Err(e),
                    };
                    if let Ok(bytes) = response.encode() {
                        let _ =
                            write_frame(&mut stream, &bytes, Instant::now() + WRITE_TIMEOUT, &stop);
                    }
                }
                return;
            }
        };
        let is_hello = matches!(request.operation, Operation::Hello(_));
        let outcome = if !hello && !is_hello {
            Err(ControlError::HandshakeRequired)
        } else if hello && is_hello {
            Err(ControlError::InvalidRequest)
        } else {
            let (reply, rx) = mpsc::sync_channel(1);
            match core.try_send(CoreMessage::Client(Job {
                op: request.operation,
                reply,
            })) {
                Ok(()) => rx
                    .recv_timeout(Duration::from_secs(15))
                    .unwrap_or(Err(ControlError::StorageUnavailable)),
                Err(mpsc::TrySendError::Full(_)) => Err(ControlError::CapacityExceeded),
                Err(_) => Err(ControlError::StorageUnavailable),
            }
        };
        let success = outcome.is_ok();
        let response = Response {
            request_id: request.request_id,
            authority_instance_id: instance.clone(),
            outcome,
        };
        let bytes = match response.encode() {
            Ok(b) => b,
            Err(_) => return,
        };
        if write_frame(&mut stream, &bytes, Instant::now() + WRITE_TIMEOUT, &stop).is_err() {
            return;
        }
        if is_hello && success {
            hello = true;
        }
        if !hello {
            return;
        }
    }
}
/// Called only by the independent Linux binary, before starting any worker thread.
pub fn run_from_environment() -> ControlResult<()> {
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGTERM);
    mask.add(Signal::SIGINT);
    mask.thread_block()
        .map_err(|_| ControlError::InternalError)?;
    let mut signals = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC)
        .map_err(|_| ControlError::InternalError)?;
    let paths = Paths::from_environment()?;
    let _locks = paths.lock()?;
    let instance = uuid();
    let mut store = Store::open(&paths.state.join("control.sqlite3"), instance.clone())?;
    let (listener, _socket) = bind(&paths.runtime.join("control.sock"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::sync_channel::<CoreMessage>(QUEUE);
    let core_sender = sender.clone();
    let core_stop = stop.clone();
    let mut runtime = Runtime::new(&store, &paths.runtime)?;
    let core = thread::spawn(move || -> ControlResult<()> {
        store.recover_binding_operations()?;
        store.recover_approvals()?;
        store.recover_turns()?;
        let mut turns = super::turns::Turns::default();
        let mut closing = false;
        let mut conversations = super::conversations::Conversations::default();
        let mut next_expiry = refresh_expiry(&mut store);
        runtime.reconcile(&mut store, &core_sender, true)?;
        loop {
            // A bounded wall-clock recheck handles clock changes, without SQLite polling.
            if next_expiry.is_some_and(|t| store.lease_now() >= t) {
                conversations.invalidate_admission();
                next_expiry = refresh_expiry(&mut store);
            }
            match turns.enforce(&mut store, &mut runtime) {
                Ok(true) => conversations.detach(&mut store),
                Err(_) => {
                    store.execution_available = false;
                    let _ = runtime.contain(&mut store, RuntimeDiagnostic::AuditUnavailable);
                    conversations.detach(&mut store);
                }
                Ok(false) => (),
            }
            if closing && !turns.active() {
                break;
            }
            let expiry = next_expiry.map(|t| expiry_wait(t, store.lease_now()));
            let wait = match (expiry, turns.wait()) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, None) => a,
                (None, b) => b,
            };
            let received = match wait {
                Some(t) => receiver.recv_timeout(t),
                None => receiver
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            let message = match received {
                Ok(m) => m,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            };
            match message {
                CoreMessage::Shutdown => {
                    closing = true;
                    turns.shutdown(&mut runtime);
                }
                CoreMessage::Runtime(id, observation) => {
                    if !runtime.owns_generation(&id) {
                        continue;
                    }
                    if let Observation::SafetyConfigChanged(names) = observation {
                        conversations.reject_pre_dispatch(&mut store, &runtime, &id);
                        if turns.lost(&mut store, TurnReason::DispatchUnknown).is_err()
                            || runtime.reclamp(&mut store, &core_sender, names).is_err()
                        {
                            store.execution_available = false;
                        }
                        continue;
                    }
                    let turn_result = match &observation {
                        Observation::TurnNotice(n) => turns.notice(&mut store, &id, n.clone()),
                        Observation::TurnResult {
                            managed_turn_id,
                            result,
                            ..
                        } => turns.accepted(
                            &mut store,
                            &mut runtime,
                            &id,
                            managed_turn_id,
                            result.clone(),
                        ),
                        Observation::InterruptResult {
                            managed_turn_id,
                            success,
                            ..
                        } if !success => {
                            if store
                                .active_turn()
                                .ok()
                                .flatten()
                                .is_some_and(|t| t.managed_turn_id == *managed_turn_id)
                            {
                                turns.lost(&mut store, TurnReason::InterruptTimeout)
                            } else {
                                Ok(())
                            }
                        }
                        Observation::IsolationViolation => {
                            let result =
                                turns.lost(&mut store, TurnReason::InferenceIsolationViolation);
                            let _ = runtime.contain(
                                &mut store,
                                RuntimeDiagnostic::InferenceIsolationViolation,
                            );
                            conversations.detach(&mut store);
                            result
                        }
                        Observation::Exited { reason, .. } => turns.lost(
                            &mut store,
                            if *reason == RuntimeDiagnostic::InferenceIsolationViolation {
                                TurnReason::InferenceIsolationViolation
                            } else {
                                TurnReason::RuntimeFailed
                            },
                        ),
                        _ => Ok(()),
                    };
                    if turn_result.is_err() {
                        store.execution_available = false;
                        let _ = runtime.contain(&mut store, RuntimeDiagnostic::AuditUnavailable);
                        conversations.detach(&mut store);
                    }
                    match observation {
                        Observation::Materialization {
                            ref operation_id,
                            ref thread_id,
                            stage,
                            ref reply,
                        } => {
                            conversations.checkpoint(
                                &mut store,
                                &id,
                                operation_id,
                                thread_id,
                                stage,
                                reply,
                            );
                        }
                        Observation::Conversation {
                            ref operation_id,
                            ref result,
                            ..
                        } => conversations.complete(
                            &mut store,
                            &runtime,
                            &id,
                            operation_id,
                            result.clone(),
                        ),
                        Observation::Exited { .. } => conversations.detach(&mut store),
                        _ => (),
                    }
                    // A failed observation audit must not bypass child cleanup.
                    if runtime.observe(&mut store, id, observation).is_err() {
                        store.execution_available = false;
                    }
                }
                CoreMessage::Client(job) => {
                    if matches!(
                        job.op,
                        Operation::BindingCreate(_) | Operation::BindingResume(_)
                    ) && !core_stop.load(Ordering::Acquire)
                    {
                        if turns.active() {
                            let _ = job
                                .reply
                                .try_send(Err(ControlError::RuntimeOperationInProgress));
                            continue;
                        }
                        conversations.begin(&mut store, &mut runtime, &job.op, job.reply);
                        continue;
                    }
                    if matches!(
                        job.op,
                        Operation::LeaseRevoke(_)
                            | Operation::LeaseRenew(_)
                            | Operation::Close(_)
                            | Operation::RuntimeStop(_)
                    ) {
                        conversations.invalidate_admission();
                    }
                    let result = if core_stop.load(Ordering::Acquire) {
                        Err(ControlError::StorageUnavailable)
                    } else {
                        match &job.op {
                            Operation::TurnStart(p) => {
                                turns.start(&mut store, &mut runtime, &conversations, p)
                            }
                            Operation::TurnStatus(p) => turns.status(&store, p),
                            Operation::TurnEvents(p) => turns.events(&store, p),
                            Operation::TurnCancel(p) => turns.cancel(&mut store, &mut runtime, p),
                            Operation::BindingStatus(p) => {
                                conversations.status(&store, &runtime, &p.managed_session_id)
                            }
                            Operation::BindingAbandon(p) => store.abandon_binding(p),
                            Operation::RuntimeStatus(p) => p
                                .supported()
                                .map(|()| ResultValue::RuntimeStatus(runtime.status())),
                            Operation::RuntimeModels(p) => runtime.models(p),
                            Operation::GateStatus(p) if p.action == ExecutionScope::TurnStart => {
                                let reason =
                                    match conversations.projection(&store, &runtime, &p.session_id)
                                    {
                                        Ok(b) => super::gate::ExecutionGate::turn(
                                            &store,
                                            &p.session_id,
                                            &runtime.status(),
                                            &b,
                                        )
                                        .reason(),
                                        Err(ControlError::SessionNotFound) => {
                                            GateReason::SessionNotFound
                                        }
                                        Err(_) => GateReason::AuthorityUnavailable,
                                    };
                                Ok(ResultValue::GateStatus(GateStatus {
                                    session_id: p.session_id.clone(),
                                    action: p.action,
                                    reason,
                                }))
                            }
                            Operation::GateStatus(p) => Ok(ResultValue::GateStatus(
                                super::gate::ExecutionGate::diagnostic(
                                    &store,
                                    p,
                                    &runtime.status(),
                                ),
                            )),
                            _ => store.apply(&job.op),
                        }
                    };
                    if matches!(
                        result,
                        Err(ControlError::StorageUnavailable | ControlError::StorageIncompatible)
                    ) {
                        store.execution_available = false;
                    }
                    if result.is_ok()
                        && matches!(
                            job.op,
                            Operation::LeaseGrant(_)
                                | Operation::LeaseRenew(_)
                                | Operation::LeaseRevoke(_)
                                | Operation::Close(_)
                        )
                    {
                        next_expiry = refresh_expiry(&mut store);
                    }
                    let reconcile = result.is_ok()
                        && matches!(
                            job.op,
                            Operation::RuntimeStart(_) | Operation::RuntimeStop(_)
                        );
                    // Canonical receipt is returned before any external side effect.
                    let _ = job.reply.try_send(result);
                    if reconcile && matches!(job.op, Operation::RuntimeStop(_)) {
                        conversations.detach(&mut store);
                    }
                    if reconcile && runtime.reconcile(&mut store, &core_sender, false).is_err() {
                        let _ = runtime.audit_failed(&mut store);
                    }
                    if reconcile && matches!(job.op, Operation::RuntimeStop(_)) {
                        let _ = turns.lost(&mut store, TurnReason::RuntimeStopped);
                    }
                }
            }
        }
        // Process shutdown is not a canonical runtime/stop intent.
        conversations.detach(&mut store);
        runtime.stop(&mut store, true)
    });
    let mut workers: Vec<thread::JoinHandle<()>> = Vec::new();
    let result = (|| -> ControlResult<()> {
        loop {
            let mut fds = [
                PollFd::new(listener.as_fd(), PollFlags::POLLIN),
                PollFd::new(signals.as_fd(), PollFlags::POLLIN),
            ];
            match poll(&mut fds, None::<u16>) {
                Ok(_) => (),
                Err(Errno::EINTR) => continue,
                Err(_) => return Err(ControlError::TransportError),
            }
            if signals
                .read_signal()
                .map_err(|_| ControlError::InternalError)?
                .is_some()
            {
                break;
            }
            // Bound work per wake so a connection flood cannot starve signal handling.
            for _ in 0..MAX_CLIENTS {
                let (stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => return Err(ControlError::TransportError),
                };
                let mut live = Vec::new();
                for h in workers.drain(..) {
                    if h.is_finished() {
                        let _ = h.join();
                    } else {
                        live.push(h);
                    }
                }
                workers = live;
                if workers.len() == MAX_CLIENTS {
                    drop(stream);
                    continue;
                }
                let tx = sender.clone();
                let instance = instance.clone();
                let stop = stop.clone();
                workers.push(thread::spawn(move || {
                    connection(stream, tx, instance, stop)
                }));
            }
        }
        Ok(())
    })();
    stop.store(true, Ordering::Release);
    drop(listener);
    // Cancel owned provider IO before waiting for clients blocked on a pending operation.
    let _ = sender.send(CoreMessage::Shutdown);
    for h in workers {
        let _ = h.join();
    }
    drop(sender);
    core.join().map_err(|_| ControlError::InternalError)??;
    result
}

// A failed expiry write latches execution closed. No retry loop or second writer.
pub(super) fn refresh_expiry(store: &mut Store) -> Option<time::OffsetDateTime> {
    match store.expire_due().and_then(|()| store.next_expiry()) {
        Ok(next) => next,
        Err(_) => {
            store.execution_available = false;
            None
        }
    }
}

fn expiry_wait(deadline: time::OffsetDateTime, now: time::OffsetDateTime) -> Duration {
    Duration::try_from(deadline - now)
        .unwrap_or(Duration::ZERO)
        .min(Duration::from_secs(30))
}
#[cfg(test)]
mod expiry_wait_tests {
    use super::*;
    #[test]
    fn deadline_wait_is_bounded_and_overdue_never_sleeps() {
        let now = time::OffsetDateTime::now_utc();
        assert_eq!(expiry_wait(now, now), Duration::ZERO);
        assert_eq!(
            expiry_wait(now - time::Duration::seconds(1), now),
            Duration::ZERO
        );
        assert_eq!(
            expiry_wait(now + time::Duration::seconds(1), now),
            Duration::from_secs(1)
        );
        assert_eq!(
            expiry_wait(now + time::Duration::hours(1), now),
            Duration::from_secs(30)
        );
    }
}
