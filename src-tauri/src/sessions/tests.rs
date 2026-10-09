//! The service against a scripted peer that speaks the real wire protocol, for what the
//! real authority cannot be made to do on demand: lose an answer after taking a request,
//! list without end, or answer out of contract.
use super::*;
use crate::contracts::{WireUuid, control::*, identities::ManagedSessionId};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) fn uuid() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}
pub(crate) fn session_id() -> ManagedSessionId {
    ManagedSessionId::parse(uuid().as_str()).unwrap()
}
pub(crate) fn record(id: &ManagedSessionId, sequence: u64) -> ManagedSessionRecord {
    serde_json::from_value(json!({
        "managed_session_id": id, "adapter_id": "codex", "runtime_kind": "codex",
        "control_state": "prepared", "revision": 1,
        "created_at": "2026-10-08T00:00:00Z", "updated_at": "2026-10-08T00:00:00Z",
        "created_sequence": sequence.to_string(),
    }))
    .unwrap()
}
pub(crate) fn binding(id: &ManagedSessionId) -> SessionBinding {
    serde_json::from_value(json!({
        "managed_session_id": id, "adapter_id": "codex", "runtime_kind": "codex",
        "runtime_session_id": "0199a213-81c0-7800-8aa1-bbab2a035a53",
        "workspace_root": "/work/space", "binding_revision": 1,
        "created_at": "2026-10-08T00:00:00Z", "updated_at": "2026-10-08T00:00:00Z",
    }))
    .unwrap()
}
fn unbound(id: &ManagedSessionId) -> ResultValue {
    ResultValue::BindingStatus(
        serde_json::from_value(json!({
            "managed_session_id": id, "state": "unbound", "binding": null,
            "runtime_instance_id": null, "operation_id": null,
        }))
        .unwrap(),
    )
}
pub(crate) fn lease(session: &ManagedSessionId) -> ExecutionLease {
    serde_json::from_value(json!({
        "lease_id": uuid(), "session_id": session, "state": "active",
        "scopes": ["session_resume"], "revision": 1,
        "issued_at": "2026-10-08T00:00:00Z", "expires_at": "2026-10-08T00:00:30Z",
        "updated_at": "2026-10-08T00:00:00Z", "revocation_reason": null,
    }))
    .unwrap()
}
fn gate(session: &ManagedSessionId, reason: GateReason) -> ResultValue {
    ResultValue::GateStatus(GateStatus {
        session_id: session.clone(),
        action: ExecutionScope::SessionResume,
        reason,
    })
}

/// What the peer does with one request after the handshake.
pub(crate) enum Reply {
    Answer(Box<ControlResult<ResultValue>>),
    /// Takes the request and closes the connection without answering.
    HangUp,
    /// Takes the request and never answers.
    Silent,
}
pub(crate) fn answer(outcome: ControlResult<ResultValue>) -> Reply {
    Reply::Answer(Box::new(outcome))
}
/// The one managed runtime as the authority would report it: stopped, or ready in a
/// profile and, for local tools, for a workspace.
pub(crate) fn runtime_status(profile: Option<&str>, workspace: Option<&str>) -> ResultValue {
    let ready = profile.is_some();
    let mut status = json!({
        "adapter_id": "codex", "runtime_kind": "codex",
        "desired_state": if ready { "running" } else { "stopped" }, "revision": 1,
        "actual_state": if ready { "ready" } else { "stopped" },
        "runtime_instance_id": ready.then(uuid),
        "runtime_version": ready.then_some("0.159.0"),
        "compatibility": if ready { "compatible" } else { "unknown" },
        "model_count": u8::from(ready), "diagnostic": null,
        "outbound_methods": {"initialize": 1, "initialized": 1, "model_list": 1},
        "runtime_safety_profile": profile,
    });
    if let Some(workspace) = workspace {
        status["workspace_root"] = json!(workspace);
    }
    ResultValue::RuntimeStatus(serde_json::from_value(status).unwrap())
}
pub(crate) type Script = Box<dyn FnMut(&Operation) -> Reply + Send>;

/// A private runtime directory with one scripted authority on its socket.
pub(crate) struct Peer {
    pub(crate) root: PathBuf,
    /// Every request after a handshake, in arrival order.
    log: Arc<Mutex<Vec<Operation>>>,
    pub(crate) hellos: Arc<Mutex<u64>>,
    /// Connections accepted, whether or not anything was said on them.
    pub(crate) accepted: Arc<Mutex<u64>>,
    /// The durable head the next handshake reports.
    pub(crate) head: Arc<Mutex<u64>>,
    stop: Arc<AtomicBool>,
    accept: Option<thread::JoinHandle<()>>,
}
impl Peer {
    pub(crate) fn start(hello: ControlResult<()>, mut script: Script) -> Self {
        let root = std::env::temp_dir().join(format!("wyss-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join("wyss-console"))
            .unwrap();
        let socket = root.join("wyss-console/control.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (log, hellos, accepted, stop) = (
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(0)),
            Arc::new(Mutex::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        let head = Arc::new(Mutex::new(41));
        let accept = thread::spawn({
            let (log, hellos, stop) = (log.clone(), hellos.clone(), stop.clone());
            let (accepted, head) = (accepted.clone(), head.clone());
            move || {
                let instance = uuid();
                let stream_id = uuid();
                while !stop.load(Ordering::Acquire) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    *accepted.lock().unwrap() += 1;
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_millis(10)))
                        .unwrap();
                    // One connection at a time: the service never opens two.
                    while let Some(bytes) = receive(&mut stream, &stop) {
                        let request = Request::parse(&bytes).unwrap();
                        let outcome = if matches!(request.operation, Operation::Hello(_)) {
                            *hellos.lock().unwrap() += 1;
                            hello.map(|()| {
                                ResultValue::Hello(AuthorityStatus {
                                    protocol_version: 1,
                                    authority_instance_id: instance.clone(),
                                    service_version: "9.9.9-test".into(),
                                    storage_schema_version: 7,
                                    stream_id: stream_id.clone(),
                                    last_sequence: head.lock().unwrap().to_string().parse().ok(),
                                    prepared_count: 0,
                                    closed_count: 0,
                                })
                            })
                        } else {
                            log.lock().unwrap().push(request.operation.clone());
                            match script(&request.operation) {
                                Reply::Answer(outcome) => *outcome,
                                Reply::HangUp => break,
                                Reply::Silent => {
                                    while receive(&mut stream, &stop).is_some() {}
                                    break;
                                }
                            }
                        };
                        let response = Response {
                            request_id: request.request_id,
                            authority_instance_id: instance.clone(),
                            outcome,
                        };
                        let bytes = response.encode().unwrap();
                        if stream
                            .write_all(&(bytes.len() as u32).to_be_bytes())
                            .and_then(|()| stream.write_all(&bytes))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        Self {
            root,
            log,
            hellos,
            accepted,
            head,
            stop,
            accept: Some(accept),
        }
    }
    pub(crate) fn canonical(script: impl FnMut(&Operation) -> Reply + Send + 'static) -> Self {
        Self::start(Ok(()), Box::new(script))
    }
    pub(crate) fn service(&self) -> SessionService {
        SessionService::new(Some(self.root.clone()))
    }
    pub(crate) fn requests(&self) -> Vec<Operation> {
        self.log.lock().unwrap().clone()
    }
    pub(crate) fn methods(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|op| {
                serde_json::to_value(op).unwrap()["method"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(accept) = self.accept.take() {
            accept.join().unwrap();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
/// One frame, or `None` once the client is gone or the peer is stopping.
fn receive(stream: &mut UnixStream, stop: &AtomicBool) -> Option<Vec<u8>> {
    let mut read = |out: &mut [u8]| {
        let mut offset = 0;
        while offset < out.len() {
            if stop.load(Ordering::Acquire) {
                return false;
            }
            match stream.read(&mut out[offset..]) {
                Ok(0) => return false,
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return false,
            }
        }
        true
    };
    let mut header = [0; 4];
    if !read(&mut header) {
        return None;
    }
    let mut body = vec![0; u32::from_be_bytes(header) as usize];
    read(&mut body).then_some(body)
}

#[test]
fn a_lost_answer_is_reported_as_unknown_and_the_same_intent_settles_it() {
    // The authority commits and goes away before its answer leaves: the first exchange
    // ends without one. The session it committed is the one the second exchange returns.
    let session = session_id();
    let committed = record(&session, 42);
    let mut asked = 0;
    let peer = Peer::canonical(move |op| {
        assert!(matches!(op, Operation::Prepare(_)));
        asked += 1;
        if asked == 1 {
            Reply::HangUp
        } else {
            answer(Ok(ResultValue::Session(committed.clone())))
        }
    });
    let service = peer.service();
    let intent = uuid();
    let lost = service.prepare(intent.clone()).unwrap_err();
    assert_eq!((lost.code, lost.retry), (SessionFailure::Unavailable, true));
    // Nothing was resent on the service's own initiative.
    assert_eq!(peer.requests().len(), 1);
    assert_eq!(service.prepare(intent.clone()).unwrap().id, session);
    let sent: Vec<WireUuid> = peer
        .requests()
        .into_iter()
        .map(|op| match op {
            Operation::Prepare(p) => {
                // Always a Codex session: the caller has no say in it.
                assert_eq!(p.adapter_id.as_str(), "codex");
                p.intent_id
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(sent, [intent.clone(), intent]);
    assert_eq!(service.stats().connections, 2);
}

#[test]
fn resume_asks_for_a_lease_only_when_the_authority_names_it_as_missing() {
    let session = session_id();
    let granted = lease(&session);
    let script = |denials: usize, reason: GateReason, session: ManagedSessionId| {
        let granted = granted.clone();
        let mut resumes = 0;
        move |op: &Operation| {
            answer(match op {
                Operation::BindingResume(_) => {
                    resumes += 1;
                    if resumes <= denials {
                        Err(ControlError::GateDenied)
                    } else {
                        Ok(ResultValue::Binding(binding(&session)))
                    }
                }
                Operation::GateStatus(_) => Ok(gate(&session, reason)),
                Operation::LeaseGrant(_) => Ok(ResultValue::Lease(granted.clone())),
                Operation::LeaseRevoke(_) => {
                    let mut revoked = granted.clone();
                    revoked.state = LeaseState::Revoked;
                    revoked.revision = 2;
                    revoked.revocation_reason = Some(RevocationReason::UserRequest);
                    Ok(ResultValue::Lease(revoked))
                }
                other => panic!("{other:?}"),
            })
        }
    };

    // A lease is already in place, or the intent is already settled: one request.
    let peer = Peer::canonical(script(0, GateReason::Allowed, session.clone()));
    let service = peer.service();
    assert_eq!(
        service.resume(uuid(), session.clone(), 1).unwrap().id,
        session
    );
    assert_eq!(peer.methods(), ["managedSession/runtime/resume"]);
    assert_eq!(service.stats().leases, 0);

    // The gate names the lease: one grant for exactly this scope, the same intent again,
    // and the grant is given back.
    for reason in [
        GateReason::LeaseMissing,
        GateReason::LeaseNotActive,
        GateReason::LeaseExpired,
    ] {
        let peer = Peer::canonical(script(1, reason, session.clone()));
        let service = peer.service();
        let intent = uuid();
        service.resume(intent.clone(), session.clone(), 1).unwrap();
        assert_eq!(
            peer.methods(),
            [
                "managedSession/runtime/resume",
                "executionGate/status",
                "executionLease/grant",
                "managedSession/runtime/resume",
                "executionLease/revoke",
            ]
        );
        let requests = peer.requests();
        let (Operation::BindingResume(first), Operation::BindingResume(second)) =
            (&requests[0], &requests[3])
        else {
            unreachable!()
        };
        assert_eq!(first, second);
        assert_eq!(
            (
                &first.intent_id,
                &first.managed_session_id,
                first.expected_session_revision
            ),
            (&intent, &session, 1)
        );
        let Operation::LeaseGrant(grant) = &requests[2] else {
            unreachable!()
        };
        assert_eq!(grant.scopes, [ExecutionScope::SessionResume]);
        assert_eq!(
            (grant.ttl_seconds, grant.expected_session_revision),
            (30, 1)
        );
        assert_eq!(grant.session_id, session);
        let Operation::LeaseRevoke(revoke) = &requests[4] else {
            unreachable!()
        };
        assert_eq!(
            (&revoke.lease_id, revoke.expected_revision),
            (&granted.lease_id, 1)
        );
        // Neither the grant nor the revoke borrows the identity of the user's action.
        assert_ne!(grant.intent_id, intent);
        assert_ne!(revoke.intent_id, intent);
        assert_ne!(grant.intent_id, revoke.intent_id);
        assert_eq!(service.stats().leases, 1);
    }

    // Any other reason is the answer: no lease is asked for, none is touched.
    for (reason, code) in [
        (
            GateReason::ScopeNotGranted,
            SessionFailure::PermissionDenied,
        ),
        (
            GateReason::RuntimeNotReady,
            SessionFailure::RuntimeUnavailable,
        ),
        (
            GateReason::RuntimeNotRunning,
            SessionFailure::RuntimeUnavailable,
        ),
        (
            GateReason::RuntimeMismatch,
            SessionFailure::RuntimeUnavailable,
        ),
        (
            GateReason::RuntimeIdentityMissing,
            SessionFailure::RuntimeUnavailable,
        ),
        (
            GateReason::RuntimeIncompatible,
            SessionFailure::RuntimeUnavailable,
        ),
        (GateReason::SessionNotPrepared, SessionFailure::NotAllowed),
        (GateReason::SessionNotFound, SessionFailure::NotFound),
        (
            GateReason::AuthorityUnavailable,
            SessionFailure::Unavailable,
        ),
        (GateReason::Allowed, SessionFailure::PermissionDenied),
    ] {
        let peer = Peer::canonical(script(usize::MAX, reason, session.clone()));
        let service = peer.service();
        let error = service.resume(uuid(), session.clone(), 1).unwrap_err();
        assert_eq!(error.code, code, "{reason:?}");
        assert_eq!(error.cause, Some(ControlError::GateDenied));
        assert_eq!(
            peer.methods(),
            ["managedSession/runtime/resume", "executionGate/status"]
        );
        assert_eq!(service.stats().leases, 0);
    }

    // Granted and still denied: the reason is read while the lease stands, and the lease
    // is given back all the same.
    let mut gates = 0;
    let (inner, target) = (granted.clone(), session.clone());
    let peer = Peer::canonical(move |op| {
        answer(match op {
            Operation::BindingResume(_) => Err(ControlError::GateDenied),
            Operation::GateStatus(_) => {
                gates += 1;
                Ok(gate(
                    &target,
                    if gates == 1 {
                        GateReason::LeaseMissing
                    } else {
                        GateReason::RuntimeNotReady
                    },
                ))
            }
            Operation::LeaseGrant(_) | Operation::LeaseRevoke(_) => {
                Ok(ResultValue::Lease(inner.clone()))
            }
            other => panic!("{other:?}"),
        })
    });
    let error = peer
        .service()
        .resume(uuid(), session.clone(), 1)
        .unwrap_err();
    assert_eq!(error.code, SessionFailure::RuntimeUnavailable);
    assert_eq!(
        peer.methods(),
        [
            "managedSession/runtime/resume",
            "executionGate/status",
            "executionLease/grant",
            "managedSession/runtime/resume",
            "executionGate/status",
            "executionLease/revoke",
        ]
    );

    // The authority refuses the grant: nothing follows it.
    let target = session.clone();
    let peer = Peer::canonical(move |op| {
        answer(match op {
            Operation::BindingResume(_) => Err(ControlError::GateDenied),
            Operation::GateStatus(_) => Ok(gate(&target, GateReason::LeaseMissing)),
            Operation::LeaseGrant(_) => Err(ControlError::LeaseAlreadyActive),
            other => panic!("{other:?}"),
        })
    });
    let service = peer.service();
    let error = service.resume(uuid(), session, 1).unwrap_err();
    assert_eq!(
        (error.code, error.cause),
        (
            SessionFailure::PermissionDenied,
            Some(ControlError::LeaseAlreadyActive)
        )
    );
    assert_eq!(peer.methods().len(), 3);
    assert_eq!(service.stats().leases, 0);
}

#[test]
fn a_list_that_never_ends_is_walked_to_its_bound_and_no_further() {
    // Every page is full and promises more: two thousand sessions and then a stop.
    let mut next = 0u64;
    let peer = Peer::canonical(move |op| {
        answer(Ok(match op {
            Operation::List(p) => {
                assert_eq!(p.limit, MAX_PAGE);
                assert_eq!(p.after.map_or(0, |s| s.get()), next);
                let sessions: Vec<_> = (1..=u64::from(MAX_PAGE))
                    .map(|i| record(&session_id(), next + i))
                    .collect();
                next += u64::from(MAX_PAGE);
                ResultValue::Sessions {
                    next: sessions.last().map(|s| s.created_sequence),
                    sessions,
                }
            }
            Operation::BindingStatus(p) => unbound(&p.managed_session_id),
            Operation::RuntimeStatus(_) => runtime_status(None, None),
            other => panic!("{other:?}"),
        }))
    });
    let page = peer.service().page(None).unwrap();
    assert!(page.truncated);
    assert_eq!(page.sessions.len(), PAGE_SIZE);
    assert_eq!(page.head.unwrap().get(), 41);
    // The last ones reached, newest first, and the cursor of the page before them.
    let positions: Vec<u64> = page.sessions.iter().map(|s| s.position.get()).collect();
    let expected: Vec<u64> = (2000 - PAGE_SIZE as u64 + 1..=2000).rev().collect();
    assert_eq!(positions, expected);
    assert_eq!(page.older.unwrap().get(), 2000 - PAGE_SIZE as u64 + 1);
    let methods = peer.methods();
    assert_eq!(
        methods
            .iter()
            .filter(|m| *m == "managedSession/list")
            .count(),
        40
    );
    // One read of the runtime, one binding read per session shown, and none for the
    // ones walked past.
    assert_eq!(methods.len(), 1 + 40 + PAGE_SIZE);
}

#[test]
fn answers_out_of_contract_fail_the_read_instead_of_being_shown() {
    let stuck = record(&session_id(), 7);
    for (case, script) in [
        (
            "a page that does not move past its cursor",
            Box::new({
                let stuck = stuck.clone();
                move |op: &Operation| {
                    answer(Ok(match op {
                        Operation::List(_) => ResultValue::Sessions {
                            sessions: (0..MAX_PAGE).map(|_| stuck.clone()).collect(),
                            next: Some(stuck.created_sequence),
                        },
                        Operation::RuntimeStatus(_) => runtime_status(None, None),
                        other => panic!("{other:?}"),
                    }))
                }
            }) as Script,
        ),
        (
            "a next cursor that is not where the page ended",
            Box::new(|op: &Operation| {
                answer(Ok(match op {
                    Operation::List(_) => ResultValue::Sessions {
                        sessions: (1..=u64::from(MAX_PAGE))
                            .map(|i| record(&session_id(), i))
                            .collect(),
                        next: "3".parse().ok(),
                    },
                    Operation::RuntimeStatus(_) => runtime_status(None, None),
                    other => panic!("{other:?}"),
                }))
            }),
        ),
        (
            "an answer of another kind",
            Box::new(|op: &Operation| {
                answer(Ok(match op {
                    Operation::List(_) => ResultValue::ActiveLease(None),
                    Operation::RuntimeStatus(_) => runtime_status(None, None),
                    other => panic!("{other:?}"),
                }))
            }),
        ),
        (
            "a binding that belongs to another session",
            Box::new({
                let stuck = stuck.clone();
                move |op: &Operation| {
                    answer(Ok(match op {
                        Operation::List(_) => ResultValue::Sessions {
                            sessions: vec![stuck.clone()],
                            next: None,
                        },
                        Operation::BindingStatus(p) => {
                            let mut status = match unbound(&p.managed_session_id) {
                                ResultValue::BindingStatus(s) => s,
                                _ => unreachable!(),
                            };
                            status.state = BindingState::BoundDetached;
                            status.binding = Some(binding(&session_id()));
                            ResultValue::BindingStatus(status)
                        }
                        Operation::RuntimeStatus(_) => runtime_status(None, None),
                        other => panic!("{other:?}"),
                    }))
                }
            }),
        ),
    ] {
        let peer = Peer::start(Ok(()), script);
        let error = peer.service().page(None).unwrap_err();
        assert_eq!(
            (error.code, error.retry),
            (SessionFailure::Internal, false),
            "{case}"
        );
    }
}

#[test]
fn a_peer_that_cannot_be_spoken_to_is_named_before_anything_is_asked() {
    for (hello, code) in [
        (
            ControlError::ProtocolVersionUnsupported,
            SessionFailure::Incompatible,
        ),
        (
            ControlError::StorageIncompatible,
            SessionFailure::Incompatible,
        ),
        (
            ControlError::StorageUnavailable,
            SessionFailure::Unavailable,
        ),
        (ControlError::InvalidRequest, SessionFailure::Unavailable),
        (ControlError::CapacityExceeded, SessionFailure::Unavailable),
    ] {
        let peer = Peer::start(Err(hello), Box::new(|op| panic!("{op:?}")));
        let service = peer.service();
        let session = session_id();
        let refused = [
            service.page(None).unwrap_err(),
            service.prepare(uuid()).unwrap_err(),
            service.close(uuid(), session.clone(), 1).unwrap_err(),
            service.resume(uuid(), session, 1).unwrap_err(),
        ];
        for error in refused {
            assert_eq!((error.code, error.cause), (code, Some(hello)));
        }
        // Four handshakes were refused and no request followed any of them.
        assert_eq!(*peer.hellos.lock().unwrap(), 4);
        assert!(peer.requests().is_empty());
        assert_eq!(service.stats().connections, 0);
    }
}

#[test]
fn shutdown_cancels_a_call_nobody_answers_and_refuses_the_next() {
    let peer = Peer::canonical(|_| Reply::Silent);
    let service = peer.service();
    let call = thread::spawn({
        let service = service.clone();
        move || service.prepare(uuid())
    });
    // The request is in the peer's hands and its answer will never come.
    let deadline = Instant::now() + Duration::from_secs(3);
    while peer.requests().is_empty() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    let started = Instant::now();
    service.shutdown();
    let error = call.join().unwrap().unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        (error.code, error.retry),
        (SessionFailure::Unavailable, true)
    );
    // Stopped: the authority's socket is not even connected to again.
    for _ in 0..3 {
        assert_eq!(
            service.close(uuid(), session_id(), 1).unwrap_err().code,
            SessionFailure::Unavailable
        );
        assert_eq!(
            service.page(None).unwrap_err().code,
            SessionFailure::Unavailable
        );
    }
    thread::sleep(Duration::from_millis(100));
    assert_eq!(*peer.accepted.lock().unwrap(), 1);
    assert_eq!(service.stats().connections, 1);
    assert_eq!(peer.requests().len(), 1);
}

#[test]
fn every_authority_error_has_one_product_meaning() {
    use ControlError as E;
    use SessionFailure as F;
    let table = [
        (E::TransportError, F::Unavailable, true),
        (E::Timeout, F::Unavailable, true),
        (E::TruncatedFrame, F::Unavailable, true),
        (E::StorageUnavailable, F::Unavailable, true),
        (E::AuthorityUnavailable, F::Unavailable, true),
        (E::AlreadyRunning, F::Unavailable, true),
        (E::UnsafePath, F::Unavailable, true),
        (E::PeerRejected, F::Unavailable, true),
        (E::ProtocolVersionUnsupported, F::Incompatible, false),
        (E::StorageIncompatible, F::Incompatible, false),
        (E::SessionNotFound, F::NotFound, false),
        (E::StaleRevision, F::Changed, false),
        (E::InvalidTransition, F::NotAllowed, false),
        (E::BindingNotFound, F::NotBound, false),
        (E::WorkspaceInvalid, F::RuntimeUnavailable, false),
        (E::GateDenied, F::PermissionDenied, false),
        (E::LeaseAlreadyActive, F::PermissionDenied, false),
        (E::LeaseNotFound, F::PermissionDenied, false),
        (E::RuntimeOperationInProgress, F::Busy, true),
        (E::CapacityExceeded, F::Busy, true),
        (E::ActiveTurnExists, F::Busy, true),
        (E::ProviderRejected, F::Rejected, false),
        (E::ExternalEffectUnknown, F::Uncertain, false),
        (E::BindingIndeterminate, F::Uncertain, false),
        (E::IntentConflict, F::Conflict, false),
        (E::InvalidRequest, F::Internal, false),
        (E::CorrelationError, F::Internal, false),
        (E::InternalError, F::Internal, false),
        (E::UnsupportedMethod, F::Internal, false),
        (E::Unsupported, F::Internal, false),
        (E::HandshakeRequired, F::Internal, false),
        (E::FrameTooLarge, F::Internal, false),
        (E::BindingAlreadyExists, F::AlreadyBound, false),
        (E::TurnNotFound, F::NotFound, false),
        (E::ApprovalNotFound, F::NotFound, false),
    ];
    for (error, code, retry) in table {
        let mapped = SessionError::of(error);
        assert_eq!(
            (mapped.code, mapped.retry, mapped.cause),
            (code, retry, Some(error))
        );
        // Only the product code and the retry hint are ever serialized.
        assert_eq!(
            serde_json::to_value(mapped).unwrap(),
            json!({"code": code, "retry": retry})
        );
    }
    assert_eq!(table.len(), 35, "every ControlError is in the table");
    // Where a workspace is being bound, a refused workspace is the folder itself.
    let folder = SessionError::of(E::WorkspaceInvalid).binding();
    assert_eq!(
        (folder.code, folder.retry, folder.cause),
        (F::WorkspaceInvalid, false, Some(E::WorkspaceInvalid))
    );
    for (error, code, _) in table {
        if error != E::WorkspaceInvalid {
            assert_eq!(SessionError::of(error).binding().code, code);
        }
        // Only an answer that never came leaves the outcome unknown.
        assert_eq!(SessionError::unknown(error), code == F::Unavailable);
    }
    let codes: Vec<String> = [
        F::Unavailable,
        F::Incompatible,
        F::Invalid,
        F::NotFound,
        F::Changed,
        F::NotAllowed,
        F::NotBound,
        F::RuntimeUnavailable,
        F::StartRefused,
        F::WorkspaceInvalid,
        F::AlreadyBound,
        F::PermissionDenied,
        F::Busy,
        F::Rejected,
        F::Uncertain,
        F::Conflict,
        F::Internal,
    ]
    .iter()
    .map(|c| {
        serde_json::to_value(c)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    })
    .collect();
    assert_eq!(
        codes,
        [
            "unavailable",
            "incompatible",
            "invalid",
            "not_found",
            "changed",
            "not_allowed",
            "not_bound",
            "runtime_unavailable",
            "start_refused",
            "workspace_invalid",
            "already_bound",
            "permission_denied",
            "busy",
            "rejected",
            "uncertain",
            "conflict",
            "internal",
        ]
    );
}

#[test]
fn the_page_projection_serializes_exactly_the_v3_fields() {
    let session = session_id();
    let (kept, target) = (record(&session, 9), session.clone());
    let peer = Peer::canonical(move |op| {
        answer(Ok(match op {
            Operation::List(_) => ResultValue::Sessions {
                sessions: vec![kept.clone()],
                next: None,
            },
            Operation::BindingStatus(_) => ResultValue::BindingStatus(
                serde_json::from_value(json!({
                    "managed_session_id": target, "state": "bound_detached",
                    "binding": binding(&target), "runtime_instance_id": null,
                    "operation_id": uuid(), "attachment_profile": null,
                }))
                .unwrap(),
            ),
            Operation::RuntimeStatus(_) => runtime_status(None, None),
            other => panic!("{other:?}"),
        }))
    });
    let page = serde_json::to_value(peer.service().page(None).unwrap()).unwrap();
    assert_eq!(
        page,
        json!({
            "schema_version": 3, "head": "41", "older": null, "truncated": false,
            // No runtime is up: there is no mode to report for it.
            "runtime": {"mode": null, "workspace": null},
            "sessions": [{
                "id": session, "adapter": "codex", "state": "prepared", "revision": 1,
                "created_at": "2026-10-08T00:00:00Z", "updated_at": "2026-10-08T00:00:00Z",
                "position": "9", "binding": "bound_detached", "workspace": "/work/space",
                // Detached: the authority reports no attachment, so no mode is claimed.
                "mode": "unknown",
                "resumable": true, "closable": true,
                "activatable": false, "recoverable": false,
            }],
        })
    );
    // The window's own fixture is this shape.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/session-page-v3.json")).unwrap();
    let keys = |v: &serde_json::Value| -> Vec<String> {
        let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(keys(&fixture["loaded"]), keys(&page));
    for shown in fixture["loaded"]["sessions"].as_array().unwrap() {
        assert_eq!(keys(shown), keys(&page["sessions"][0]));
    }
}

#[path = "activation_tests.rs"]
mod activation;

#[path = "mode_tests.rs"]
mod modes;
