use super::{
    framing::{read_frame, write_frame},
    paths::Paths,
    runtime::{Observation, Runtime},
    store::{Store, uuid},
    turns::Verdict,
    watch::{EventSignal, Wake},
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
/// One request to the single-writer core. The connection never touches the store itself.
fn ask(core: &SyncSender<CoreMessage>, op: Operation) -> ControlResult<ResultValue> {
    let (reply, rx) = mpsc::sync_channel(1);
    match core.try_send(CoreMessage::Client(Job { op, reply })) {
        Ok(()) => rx
            .recv_timeout(Duration::from_secs(15))
            .unwrap_or(Err(ControlError::StorageUnavailable)),
        Err(mpsc::TrySendError::Full(_)) => Err(ControlError::CapacityExceeded),
        Err(_) => Err(ControlError::StorageUnavailable),
    }
}
/// A durable-event watch. Both reads run in the core and finish before this returns to
/// the socket; the wait in between holds no transaction, no core slot and no store lock,
/// only one of the bounded watcher slots. `None` means there is nobody left to answer.
fn watch(
    core: &SyncSender<CoreMessage>,
    signal: &Arc<EventSignal>,
    stream: &UnixStream,
    p: &AuditWatch,
) -> Option<ControlResult<ResultValue>> {
    let read = || ask(core, Operation::AuditWatch(p.clone()));
    let first = match read() {
        Ok(r) => r,
        Err(e) => return Some(Err(e)),
    };
    if p.wait_ms == 0 || !matches!(&first, ResultValue::Audit { events, .. } if events.is_empty()) {
        return Some(Ok(first));
    }
    let mut waiter = match signal.register() {
        Ok(w) => w,
        Err(e) => return Some(Err(e)),
    };
    let deadline = Instant::now() + Duration::from_millis(p.wait_ms.into());
    let woke = waiter.wait(p.after_sequence.map_or(0, |s| s.get()), stream, deadline);
    drop(waiter);
    match woke {
        // The wakeup is only a hint: the events come from SQLite, by the client's cursor.
        Wake::Advanced => Some(read()),
        Wake::Elapsed | Wake::ClientSpoke => Some(Ok(first)),
        Wake::Closed | Wake::ClientGone => None,
    }
}
fn connection(
    mut stream: UnixStream,
    core: SyncSender<CoreMessage>,
    instance: WireUuid,
    stop: Arc<AtomicBool>,
    signal: Arc<EventSignal>,
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
        } else if let Operation::AuditWatch(p) = &request.operation {
            match watch(&core, &signal, &stream, p) {
                Some(outcome) => outcome,
                None => return,
            }
        } else {
            ask(&core, request.operation)
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
    let database = paths.state.join("control.sqlite3");
    // Canonical state is never created over the traces of an earlier one, and never
    // accepted when it is another stream or older than it has been seen to be.
    let witnessed = super::anchor::admit(&database)?;
    let mut store = Store::open(&database, instance.clone())?;
    store.checkpoint_on_close(false)?;
    let mut anchor = super::anchor::Anchor::confirm(&database, witnessed, &store)?;
    store.checkpoint_on_close(true)?;
    let (listener, _socket) = bind(&paths.runtime.join("control.sock"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let signal = EventSignal::new();
    let core_signal = signal.clone();
    let (sender, receiver) = mpsc::sync_channel::<CoreMessage>(QUEUE);
    let core_sender = sender.clone();
    let core_stop = stop.clone();
    let mut runtime = Runtime::new(&store, &paths.runtime, &paths.state)?;
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
            // Every path of this loop returns here, after its transactions committed or
            // rolled back, and what the authority just did on its own (an expiry, an
            // enforcement) has committed too: nothing it wrote waits for the next message
            // to be announced. Watchers are told the committed head and nothing else; a
            // failed read announces nothing, which costs them latency and never an event.
            if let Ok(head) = store.head() {
                // The witness follows every commit. Without it a later loss of this
                // history could not be told from its absence: execution closes.
                if anchor.advance(head).is_err() {
                    store.execution_available = false;
                }
                core_signal.publish(head);
            }
            if closing && !turns.active() {
                break;
            }
            let expiry = next_expiry.map(|t| expiry_wait(t, store.lease_now()));
            // The pending approval has its own deadline on the same authority clock.
            let approval = turns
                .approval_deadline()
                .map(|t| expiry_wait(t, store.lease_now()));
            let wait = [expiry, approval, turns.wait()].into_iter().flatten().min();
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
                    turns.shutdown(&mut store, &mut runtime);
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
                    let verdict = match &observation {
                        Observation::TurnNotice(n) => {
                            turns.notice(&mut store, &mut runtime, &id, n.clone())
                        }
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
                        Observation::Attested { request, valid } => turns.attested(
                            &mut store,
                            &mut runtime,
                            &conversations,
                            request,
                            *valid,
                        ),
                        Observation::Delivered {
                            request,
                            decision,
                            written,
                            ..
                        } => turns.delivered(&mut store, request, *decision, *written),
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
                                turns
                                    .lost(&mut store, TurnReason::InterruptTimeout)
                                    .map(|()| Verdict::Continue)
                            } else {
                                Ok(Verdict::Continue)
                            }
                        }
                        Observation::IsolationViolation => Ok(Verdict::Violation),
                        Observation::Exited { reason, .. } => turns
                            .lost(
                                &mut store,
                                if *reason == RuntimeDiagnostic::InferenceIsolationViolation {
                                    TurnReason::InferenceIsolationViolation
                                } else {
                                    TurnReason::RuntimeFailed
                                },
                            )
                            .map(|()| Verdict::Continue),
                        _ => Ok(Verdict::Continue),
                    };
                    // Containment: the turn is terminal, the owned runtime is reaped and no
                    // attachment survives. Nothing is answered and nothing is replayed.
                    let turn_result = match verdict {
                        Ok(Verdict::Continue) => Ok(()),
                        Ok(contained) => {
                            let (turn, diagnostic) = if contained == Verdict::Violation {
                                (
                                    TurnReason::InferenceIsolationViolation,
                                    RuntimeDiagnostic::InferenceIsolationViolation,
                                )
                            } else {
                                (
                                    TurnReason::DispatchUnknown,
                                    RuntimeDiagnostic::TransportFailed,
                                )
                            };
                            let result = turns.lost(&mut store, turn);
                            let _ = runtime.contain(&mut store, diagnostic);
                            conversations.detach(&mut store);
                            result
                        }
                        Err(e) => Err(e),
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
                    // An Accept replies only after the runtime re-proved its profile.
                    if let Operation::ApprovalRespond(p) = &job.op
                        && !core_stop.load(Ordering::Acquire)
                    {
                        turns.respond(&mut store, &mut runtime, &conversations, p, job.reply);
                        continue;
                    }
                    // A stop is refused while a managed turn is active, before it takes
                    // anything away: no admission is withdrawn, nothing is detached and
                    // nothing durable changes. This thread is also where a turn is
                    // admitted, so whichever of the two requests it takes first decides.
                    if matches!(job.op, Operation::RuntimeStop(_))
                        && !core_stop.load(Ordering::Acquire)
                        && let Err(refused) = turns.admit_stop(&store)
                    {
                        if matches!(
                            refused,
                            ControlError::StorageUnavailable | ControlError::StorageIncompatible
                        ) {
                            store.execution_available = false;
                        }
                        let _ = job.reply.try_send(Err(refused));
                        continue;
                    }
                    // A binding request in flight loses its admission before any of these
                    // commits, and gets it back if the mutation turned out to change nothing.
                    let withdrawn = matches!(
                        job.op,
                        Operation::LeaseRevoke(_)
                            | Operation::LeaseRenew(_)
                            | Operation::Close(_)
                            | Operation::RuntimeStop(_)
                    ) && conversations.suspend_admission();
                    // A revocation or a close of what the active turn runs under withdraws
                    // its admission before it commits: once the mutation is durable, the
                    // prompt of a turn that was not yet dispatched can no longer leave.
                    let suspended = turns.suspend_admission(&job.op);
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
                            Operation::ApprovalPending(p) => turns.approval_pending(&store, p),
                            Operation::ApprovalStatus(p) => turns.approval_status(&store, p),
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
                            Operation::RuntimeStart(p) => {
                                // Refused before the durable desired state changes.
                                runtime.admit_start(p).and_then(|workspace| {
                                    let result = store.apply(&job.op)?;
                                    if let ResultValue::RuntimeControl(record) = &result {
                                        runtime.request(workspace, record);
                                    }
                                    Ok(result)
                                })
                            }
                            _ => store.apply(&job.op),
                        }
                    };
                    if matches!(
                        result,
                        Err(ControlError::StorageUnavailable | ControlError::StorageIncompatible)
                    ) {
                        store.execution_available = false;
                    }
                    // What a runtime mutation leaves to do, decided against the revision the
                    // core already carried out, before anything below moves it.
                    let effects = runtime_effects(&job.op, &result, runtime.status().revision);
                    // A mutation that changed nothing took nothing away.
                    let unchanged = match &result {
                        Err(_) => true,
                        Ok(ResultValue::RuntimeControl(_)) => effects == RuntimeEffects::None,
                        Ok(_) => false,
                    };
                    if suspended && unchanged {
                        turns.restore_admission();
                    }
                    if withdrawn && unchanged {
                        conversations.restore_admission();
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
                    // Canonical receipt is returned before any external side effect, and
                    // never for a head the witness does not hold: a transition somebody was
                    // told about is one whose later loss can be told. What committed stays
                    // committed; its intent returns this receipt once the witness holds it.
                    let result = match result {
                        Ok(value) if receipt(&job.op) => {
                            match store.head().and_then(|head| anchor.advance(head)) {
                                Ok(()) => Ok(value),
                                Err(_) => {
                                    store.execution_available = false;
                                    Err(ControlError::StorageUnavailable)
                                }
                            }
                        }
                        other => other,
                    };
                    let _ = job.reply.try_send(result);
                    // Only a transition that committed now is carried out. A receipt that
                    // is answered again detaches nothing and reconciles nothing.
                    if effects == RuntimeEffects::DetachAndReconcile {
                        conversations.detach(&mut store);
                    }
                    if effects != RuntimeEffects::None
                        && runtime.reconcile(&mut store, &core_sender, false).is_err()
                    {
                        let _ = runtime.audit_failed(&mut store);
                    }
                }
            }
        }
        // Process shutdown is not a canonical runtime/stop intent.
        conversations.detach(&mut store);
        let stopped = runtime.stop(&mut store, true);
        // What shutdown itself recorded is witnessed too.
        let witnessed = store.head().and_then(|head| anchor.advance(head));
        stopped.and(witnessed)
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
                let signal = signal.clone();
                workers.push(thread::spawn(move || {
                    connection(stream, tx, instance, stop, signal)
                }));
            }
        }
        Ok(())
    })();
    stop.store(true, Ordering::Release);
    // No watcher delays shutdown for the rest of its wait.
    signal.close();
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

/// Whether a successful answer is the receipt of something this request may have
/// committed, as opposed to a reading of what is there. An operation not named here is
/// taken for one that commits.
/// What the core still has to carry out for a runtime mutation the store answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeEffects {
    /// Nothing: it failed, it is not a runtime mutation, or the store answered an intent
    /// that had already committed with the receipt it has kept for it.
    None,
    /// A start that committed now.
    Reconcile,
    /// A stop that committed now: every attachment ends, then the child is reaped.
    DetachAndReconcile,
}
/// Tells a runtime transition that committed now from a receipt answered again. The store
/// gives every committed runtime mutation the next revision, and the core carries a
/// revision out before it takes another request: a receipt at or below `carried` is one
/// whose effects already happened, and it has none left.
fn runtime_effects(
    op: &Operation,
    result: &ControlResult<ResultValue>,
    carried: u64,
) -> RuntimeEffects {
    let Ok(ResultValue::RuntimeControl(record)) = result else {
        return RuntimeEffects::None;
    };
    if record.revision <= carried {
        return RuntimeEffects::None;
    }
    match op {
        Operation::RuntimeStop(_) => RuntimeEffects::DetachAndReconcile,
        Operation::RuntimeStart(_) => RuntimeEffects::Reconcile,
        _ => RuntimeEffects::None,
    }
}
fn receipt(op: &Operation) -> bool {
    !matches!(
        op,
        Operation::Hello(_)
            | Operation::Status(_)
            | Operation::Get(_)
            | Operation::List(_)
            | Operation::AuditRead(_)
            | Operation::AuditWatch(_)
            | Operation::LeaseGet(_)
            | Operation::LeaseActive(_)
            | Operation::GateStatus(_)
            | Operation::TurnStatus(_)
            | Operation::TurnEvents(_)
            | Operation::ApprovalPending(_)
            | Operation::ApprovalStatus(_)
            | Operation::BindingStatus(_)
            | Operation::RuntimeStatus(_)
            | Operation::RuntimeModels(_)
    )
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
mod runtime_effects_tests {
    use super::*;
    fn mutation() -> RuntimeMutation {
        RuntimeMutation {
            intent_id: serde_json::from_value(serde_json::json!(
                "11111111-1111-4111-8111-111111111111"
            ))
            .unwrap(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
            expected_revision: None,
            safety_profile: None,
            workspace_root: None,
        }
    }
    fn receipt(revision: u64, desired: &str) -> ControlResult<ResultValue> {
        Ok(ResultValue::RuntimeControl(
            serde_json::from_value(serde_json::json!({
                "adapter_id": "codex", "runtime_kind": "codex", "desired_state": desired,
                "revision": revision, "created_at": "2026-10-09T00:00:00Z",
                "updated_at": "2026-10-09T00:00:00Z",
            }))
            .unwrap(),
        ))
    }
    #[test]
    fn only_a_runtime_transition_that_committed_now_has_effects_left_to_carry_out() {
        let stop = Operation::RuntimeStop(mutation());
        let start = Operation::RuntimeStart(mutation());
        // The store gave the mutation the revision after the one the core carried out.
        assert_eq!(
            runtime_effects(&stop, &receipt(4, "stopped"), 3),
            RuntimeEffects::DetachAndReconcile
        );
        assert_eq!(
            runtime_effects(&start, &receipt(4, "running"), 3),
            RuntimeEffects::Reconcile
        );
        // The very first one, with nothing carried out before it.
        assert_eq!(
            runtime_effects(&start, &receipt(1, "running"), 0),
            RuntimeEffects::Reconcile
        );
        // A receipt answered again has none: neither the latest transition repeated, nor
        // one from before later transitions, whatever the runtime is doing by now.
        for (op, desired) in [(&stop, "stopped"), (&start, "running")] {
            for (revision, carried) in [(4, 4), (2, 4), (1, 9), (3, u64::MAX)] {
                assert_eq!(
                    runtime_effects(op, &receipt(revision, desired), carried),
                    RuntimeEffects::None,
                    "{desired} {revision}/{carried}"
                );
            }
        }
        // A mutation the store refused changed nothing.
        for refused in [
            ControlError::StaleRevision,
            ControlError::IntentConflict,
            ControlError::StorageUnavailable,
        ] {
            assert_eq!(
                runtime_effects(&stop, &Err(refused), 3),
                RuntimeEffects::None
            );
            assert_eq!(
                runtime_effects(&start, &Err(refused), 3),
                RuntimeEffects::None
            );
        }
        // Nothing else the core answers is a runtime transition.
        assert_eq!(
            runtime_effects(&Operation::Status(Empty {}), &receipt(4, "stopped"), 3),
            RuntimeEffects::None
        );
        assert_eq!(
            runtime_effects(&stop, &Ok(ResultValue::ActiveLease(None)), 3),
            RuntimeEffects::None
        );
    }
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
