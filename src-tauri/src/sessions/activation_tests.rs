//! Activation against a scripted authority: the exact requests each step sends, their
//! identities, and what a lost answer, a refusal or an unknown outcome leaves behind.
//! How the real authority behaves is covered by `tests/sessions_process.rs`; this peer
//! models only what a step can observe.
use super::*;
use std::collections::HashMap;

const WORKSPACE: &str = "/work/space";

/// What the peer does instead of answering one request.
#[derive(Clone, Copy)]
enum Fault {
    Refuse(ControlError),
    /// The request is dropped: nothing changes and no answer leaves.
    Lost,
    /// The request takes effect and its answer never leaves.
    AnswerLost,
}

/// The authority as one activation can observe it.
struct World {
    session: ManagedSessionId,
    closed: bool,
    binding: BindingState,
    /// The open operation the authority reports while a binding is not settled.
    operation: Option<WireUuid>,
    /// The intent whose binding request the authority holds a receipt for.
    bound_by: Option<WireUuid>,
    desired: DesiredState,
    actual: ActualState,
    diagnostic: Option<RuntimeDiagnostic>,
    revision: u64,
    starts: HashMap<String, u64>,
    lease: Option<ExecutionLease>,
    grants: HashMap<String, ExecutionLease>,
    /// Whether a resume attaches, or only answers with a receipt.
    attaches: bool,
    faults: Vec<(&'static str, usize, Fault)>,
    seen: HashMap<String, usize>,
}
impl World {
    fn new(session: &ManagedSessionId) -> Self {
        Self {
            session: session.clone(),
            closed: false,
            binding: BindingState::Unbound,
            operation: None,
            bound_by: None,
            desired: DesiredState::Running,
            actual: ActualState::Ready,
            diagnostic: None,
            revision: 1,
            starts: HashMap::new(),
            lease: None,
            grants: HashMap::new(),
            attaches: true,
            faults: Vec::new(),
            seen: HashMap::new(),
        }
    }
    fn stopped(mut self) -> Self {
        (self.desired, self.actual, self.revision) =
            (DesiredState::Stopped, ActualState::Stopped, 0);
        self
    }
    fn fault(mut self, method: &'static str, nth: usize, fault: Fault) -> Self {
        self.faults.push((method, nth, fault));
        self
    }
    fn reason(&self, action: ExecutionScope) -> GateReason {
        match &self.lease {
            _ if self.closed => GateReason::SessionNotPrepared,
            None => GateReason::LeaseMissing,
            Some(lease) if !lease.scopes.contains(&action) => GateReason::ScopeNotGranted,
            Some(_) if self.desired != DesiredState::Running => GateReason::RuntimeNotRunning,
            Some(_) if self.actual != ActualState::Ready => GateReason::RuntimeNotReady,
            Some(_) => GateReason::Allowed,
        }
    }
    fn status(&self) -> ResultValue {
        let bound = matches!(
            self.binding,
            BindingState::BoundDetached | BindingState::BoundAttached
        );
        ResultValue::BindingStatus(
            serde_json::from_value(json!({
                "managed_session_id": self.session, "state": self.binding,
                "binding": bound.then(|| binding(&self.session)),
                "runtime_instance_id":
                    (self.binding == BindingState::BoundAttached).then(uuid),
                "operation_id": self.operation,
            }))
            .unwrap(),
        )
    }
    fn runtime(&self) -> ResultValue {
        let ready = self.actual == ActualState::Ready;
        ResultValue::RuntimeStatus(
            serde_json::from_value(json!({
                "adapter_id": "codex", "runtime_kind": "codex",
                "desired_state": self.desired, "revision": self.revision,
                "actual_state": self.actual,
                "runtime_instance_id": ready.then(uuid),
                "runtime_version": ready.then_some("0.159.0"),
                "compatibility": if ready { "compatible" } else { "unknown" },
                "model_count": u8::from(ready), "diagnostic": self.diagnostic,
                "outbound_methods": {"initialize": 1, "initialized": 1, "model_list": 1},
                "runtime_safety_profile": ready.then_some("codex_inference_v1"),
            }))
            .unwrap(),
        )
    }
    fn control(&self, revision: u64) -> ResultValue {
        ResultValue::RuntimeControl(
            serde_json::from_value(json!({
                "adapter_id": "codex", "runtime_kind": "codex", "desired_state": "running",
                "revision": revision, "created_at": "2026-10-08T00:00:00Z",
                "updated_at": "2026-10-08T00:00:00Z",
            }))
            .unwrap(),
        )
    }
    fn apply(&mut self, op: &Operation) -> ControlResult<ResultValue> {
        match op {
            Operation::BindingStatus(_) => Ok(self.status()),
            Operation::RuntimeStatus(_) => Ok(self.runtime()),
            Operation::RuntimeStart(p) => {
                let known = self.starts.get(p.intent_id.as_str()).copied();
                let revision = known.unwrap_or_else(|| {
                    self.revision += 1;
                    (self.desired, self.actual) = (DesiredState::Running, ActualState::Starting);
                    self.diagnostic = None;
                    self.revision
                });
                self.starts
                    .insert(p.intent_id.as_str().to_owned(), revision);
                Ok(self.control(revision))
            }
            Operation::GateStatus(q) => Ok(ResultValue::GateStatus(GateStatus {
                session_id: q.session_id.clone(),
                action: q.action,
                reason: self.reason(q.action),
            })),
            Operation::LeaseActive(_) => Ok(ResultValue::ActiveLease(self.lease.clone())),
            Operation::LeaseGrant(p) => {
                if let Some(receipt) = self.grants.get(p.intent_id.as_str()) {
                    return Ok(ResultValue::Lease(receipt.clone()));
                }
                if self.lease.is_some() {
                    return Err(ControlError::LeaseAlreadyActive);
                }
                let mut granted = lease(&self.session);
                granted.scopes = p.scopes.clone();
                self.grants
                    .insert(p.intent_id.as_str().to_owned(), granted.clone());
                self.lease = Some(granted.clone());
                Ok(ResultValue::Lease(granted))
            }
            Operation::LeaseRevoke(p) => match self.lease.take() {
                Some(mut held) if held.lease_id == p.lease_id => {
                    held.state = LeaseState::Revoked;
                    held.revision = 2;
                    held.revocation_reason = Some(RevocationReason::UserRequest);
                    Ok(ResultValue::Lease(held))
                }
                other => {
                    self.lease = other;
                    Err(ControlError::InvalidTransition)
                }
            },
            Operation::BindingCreate(p) => {
                if self.bound_by.as_ref() == Some(&p.intent_id) {
                    return Ok(ResultValue::Binding(binding(&self.session)));
                }
                if p.workspace_root != WORKSPACE {
                    return Err(ControlError::WorkspaceInvalid);
                }
                if self.reason(ExecutionScope::SessionCreate) != GateReason::Allowed {
                    return Err(ControlError::GateDenied);
                }
                match self.binding {
                    BindingState::Unbound => {
                        self.binding = BindingState::BoundDetached;
                        self.bound_by = Some(p.intent_id.clone());
                        Ok(ResultValue::Binding(binding(&self.session)))
                    }
                    BindingState::Creating | BindingState::Indeterminate => {
                        Err(ControlError::BindingIndeterminate)
                    }
                    _ => Err(ControlError::BindingAlreadyExists),
                }
            }
            Operation::BindingResume(_) => {
                if self.reason(ExecutionScope::SessionResume) != GateReason::Allowed {
                    return Err(ControlError::GateDenied);
                }
                if self.attaches {
                    self.binding = BindingState::BoundAttached;
                }
                Ok(ResultValue::Binding(binding(&self.session)))
            }
            Operation::BindingAbandon(p) => {
                if self.binding != BindingState::Indeterminate
                    || self.operation.as_ref() != Some(&p.operation_id)
                {
                    return Err(ControlError::InvalidTransition);
                }
                (self.binding, self.operation) = (BindingState::Unbound, None);
                Ok(ResultValue::BindingAbandoned(AbandonedOperation {
                    operation_id: p.operation_id.clone(),
                }))
            }
            other => panic!("{other:?}"),
        }
    }
}

fn method(op: &Operation) -> String {
    serde_json::to_value(op).unwrap()["method"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A peer that is `world`, and the world itself for the test to look at and change.
fn authority(world: World) -> (Peer, Arc<Mutex<World>>) {
    let world = Arc::new(Mutex::new(world));
    let shared = world.clone();
    let peer = Peer::canonical(move |op| {
        let mut world = shared.lock().unwrap();
        let name = method(op);
        let nth = {
            let seen = world.seen.entry(name.clone()).or_insert(0);
            *seen += 1;
            *seen
        };
        let fault = world
            .faults
            .iter()
            .find(|(m, n, _)| *m == name && *n == nth)
            .map(|(_, _, fault)| *fault);
        match fault {
            Some(Fault::Refuse(error)) => answer(Err(error)),
            Some(Fault::Lost) => Reply::HangUp,
            Some(Fault::AnswerLost) => {
                let _ = world.apply(op);
                Reply::HangUp
            }
            None => answer(world.apply(op)),
        }
    });
    (peer, world)
}

const STATUS: &str = "managedSession/runtime/status";
const CREATE: &str = "managedSession/runtime/create";
const RESUME: &str = "managedSession/runtime/resume";
const ABANDON: &str = "managedSession/runtime/abandon";
const RUNTIME: &str = "runtime/status";
const START: &str = "runtime/start";
const GATE: &str = "executionGate/status";
const GRANT: &str = "executionLease/grant";
const REVOKE: &str = "executionLease/revoke";
const ACTIVE: &str = "executionLease/active";

fn stage(result: Result<ActivationProgress, SessionError>) -> ActivationStage {
    result.unwrap().stage
}
fn failed(result: Result<ActivationProgress, SessionError>) -> (SessionFailure, bool) {
    let error = result.unwrap_err();
    (error.code, error.retry)
}
/// The requests of the last call: everything after `from`.
fn since(peer: &Peer, from: usize) -> Vec<String> {
    peer.methods().split_off(from)
}

#[test]
fn an_unbound_session_is_bound_then_attached_one_authority_step_at_a_time() {
    let session = session_id();
    let (peer, world) = authority(World::new(&session));
    let service = peer.service();
    let operation = uuid();
    let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());

    // The runtime is ready: the binding is asked for, under a lease for that alone.
    assert_eq!(stage(step()), ActivationStage::Bound);
    assert_eq!(
        peer.methods(),
        [STATUS, RUNTIME, CREATE, GATE, GRANT, CREATE, REVOKE]
    );
    assert_eq!(
        world.lock().unwrap().binding,
        BindingState::BoundDetached,
        "a binding is created detached: nothing was attached by this step"
    );
    let requests = peer.requests();
    let (Operation::BindingCreate(asked), Operation::BindingCreate(again)) =
        (&requests[2], &requests[5])
    else {
        unreachable!()
    };
    assert_eq!(asked, again);
    assert_eq!(
        (
            &asked.managed_session_id,
            asked.expected_session_revision,
            asked.workspace_root.as_str()
        ),
        (&session, 1, WORKSPACE)
    );
    let Operation::LeaseGrant(grant) = &requests[4] else {
        unreachable!()
    };
    // Exactly the scope a binding needs, for seconds, for this session at this revision.
    assert_eq!(grant.scopes, [ExecutionScope::SessionCreate]);
    assert_eq!(
        (grant.ttl_seconds, grant.expected_session_revision),
        (15, 1)
    );
    assert_eq!(grant.session_id, session);
    let Operation::LeaseRevoke(revoke) = &requests[6] else {
        unreachable!()
    };
    assert_eq!(revoke.expected_revision, 1);
    assert!(world.lock().unwrap().lease.is_none());
    // Four mutations, four identities, and none of them is the action's own.
    let identities = [&asked.intent_id, &grant.intent_id, &revoke.intent_id];
    for (i, id) in identities.iter().enumerate() {
        assert_ne!(*id, &operation);
        assert!(identities.iter().skip(i + 1).all(|other| other != id));
    }
    let create_intent = asked.intent_id.clone();

    // Bound: the authority confirms the binding is this action's, then it is attached
    // under a lease for resuming alone, and what the session is now is read.
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Ready);
    assert_eq!(
        since(&peer, sent),
        [
            STATUS, CREATE, RUNTIME, RESUME, GATE, GRANT, RESUME, REVOKE, STATUS
        ]
    );
    let requests = peer.requests().split_off(sent);
    let Operation::BindingCreate(proof) = &requests[1] else {
        unreachable!()
    };
    assert_eq!(proof.intent_id, create_intent);
    let Operation::BindingResume(resume) = &requests[3] else {
        unreachable!()
    };
    assert_ne!(resume.intent_id, create_intent);
    assert_ne!(resume.intent_id, operation);
    assert_eq!(
        (&resume.managed_session_id, resume.expected_session_revision),
        (&session, 1)
    );
    let Operation::LeaseGrant(grant) = &requests[5] else {
        unreachable!()
    };
    assert_eq!(grant.scopes, [ExecutionScope::SessionResume]);
    assert_eq!(grant.ttl_seconds, 30);
    assert!(world.lock().unwrap().lease.is_none());
    assert_eq!(service.stats().leases, 2);

    // Ready: asking again reads, proves and changes nothing.
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Ready);
    assert_eq!(since(&peer, sent), [STATUS, CREATE]);
    assert_eq!(service.stats().leases, 2);
    // One connection a step, each closed when the step returned.
    assert_eq!(service.stats().connections, 3);
}

#[test]
fn the_runtime_is_asked_to_start_once_and_never_while_it_is_starting() {
    let session = session_id();
    let (peer, world) = authority(World::new(&session).stopped());
    let service = peer.service();
    let operation = uuid();
    let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());

    assert_eq!(stage(step()), ActivationStage::RuntimeStarting);
    assert_eq!(peer.methods(), [STATUS, RUNTIME, START]);
    let Operation::RuntimeStart(start) = &peer.requests()[2] else {
        unreachable!()
    };
    // The default profile of the one runtime the authority runs, and nothing else: no
    // workspace is named, no revision is assumed.
    assert_eq!(start.target(), RuntimeTarget::codex());
    assert_eq!(
        (
            start.expected_revision,
            start.safety_profile,
            &start.workspace_root
        ),
        (None, None, &None)
    );
    assert_ne!(start.intent_id, operation);
    let start_intent = start.intent_id.clone();

    // Starting, then initializing: the step waits and asks for nothing.
    for actual in [ActualState::Starting, ActualState::Initializing] {
        world.lock().unwrap().actual = actual;
        let sent = peer.requests().len();
        assert_eq!(stage(step()), ActivationStage::RuntimeStarting);
        assert_eq!(since(&peer, sent), [STATUS, RUNTIME]);
    }
    // Ready: the same action goes on to the binding.
    world.lock().unwrap().actual = ActualState::Ready;
    assert_eq!(stage(step()), ActivationStage::Bound);
    assert_eq!(world.lock().unwrap().revision, 1, "one start was asked for");
    // The binding is another operation: it does not borrow the identity of the start.
    for op in peer.requests() {
        if let Operation::BindingCreate(asked) = op {
            assert_ne!(asked.intent_id, start_intent);
        }
    }

    // The start this action asked for did not bring a runtime up: it is not asked again.
    let other = session_id();
    let (peer, world) = authority(World::new(&other).stopped());
    let service = peer.service();
    let step = || service.activate(operation.clone(), other.clone(), 1, WORKSPACE.into());
    assert_eq!(stage(step()), ActivationStage::RuntimeStarting);
    world.lock().unwrap().actual = ActualState::Failed;
    world.lock().unwrap().diagnostic = Some(RuntimeDiagnostic::SpawnFailed);
    let sent = peer.requests().len();
    assert_eq!(failed(step()), (SessionFailure::RuntimeUnavailable, false));
    // The same intent, which the authority answers with the receipt it already gave.
    assert_eq!(since(&peer, sent), [STATUS, RUNTIME, START]);
    let Operation::RuntimeStart(again) = &peer.requests()[sent + 2] else {
        unreachable!()
    };
    assert_eq!(again.intent_id, start_intent);
    assert_eq!(world.lock().unwrap().revision, 1);
    // A new action is a new request to start.
    assert_eq!(
        stage(service.activate(uuid(), other.clone(), 1, WORKSPACE.into())),
        ActivationStage::RuntimeStarting
    );
    assert_eq!(world.lock().unwrap().revision, 2);
}

#[test]
fn a_refused_start_and_a_contained_runtime_are_reported_and_nothing_is_bound() {
    let session = session_id();
    for (refusal, code, retry) in [
        (
            ControlError::InvalidTransition,
            SessionFailure::StartRefused,
            false,
        ),
        (
            ControlError::Unsupported,
            SessionFailure::StartRefused,
            false,
        ),
        (
            ControlError::StaleRevision,
            SessionFailure::StartRefused,
            false,
        ),
        (ControlError::CapacityExceeded, SessionFailure::Busy, true),
        (
            ControlError::StorageUnavailable,
            SessionFailure::Unavailable,
            true,
        ),
        (
            ControlError::IntentConflict,
            SessionFailure::Conflict,
            false,
        ),
    ] {
        let (peer, _) = authority(World::new(&session).stopped().fault(
            START,
            1,
            Fault::Refuse(refusal),
        ));
        let result = peer
            .service()
            .activate(uuid(), session.clone(), 1, WORKSPACE.into());
        let error = result.unwrap_err();
        assert_eq!((error.code, error.retry), (code, retry), "{refusal:?}");
        assert_eq!(error.cause, Some(refusal));
        assert_eq!(peer.methods(), [STATUS, RUNTIME, START]);
    }
    // A generation the authority contained for safety: no start is asked for at all.
    for diagnostic in [
        RuntimeDiagnostic::InferenceIsolationViolation,
        RuntimeDiagnostic::SafetyUnproven,
    ] {
        let mut world = World::new(&session);
        (world.actual, world.diagnostic) = (ActualState::Failed, Some(diagnostic));
        let (peer, _) = authority(world);
        let result = peer
            .service()
            .activate(uuid(), session.clone(), 1, WORKSPACE.into());
        assert_eq!(failed(result), (SessionFailure::RuntimeUnavailable, false));
        assert_eq!(peer.methods(), [STATUS, RUNTIME]);
    }
    // An answer to the start that never came: unknown, and the same intent settles it.
    let (peer, world) = authority(World::new(&session).stopped().fault(
        START,
        1,
        Fault::AnswerLost,
    ));
    let service = peer.service();
    let operation = uuid();
    let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());
    assert_eq!(failed(step()), (SessionFailure::Unavailable, true));
    assert_eq!(stage(step()), ActivationStage::RuntimeStarting);
    // The start had been taken: the runtime is on its way and no second one was asked.
    assert_eq!(peer.methods().iter().filter(|m| *m == START).count(), 1);
    assert_eq!(world.lock().unwrap().revision, 1);
}

#[test]
fn a_folder_the_authority_refuses_costs_no_lease_and_binds_nothing() {
    let session = session_id();
    let (peer, world) = authority(World::new(&session));
    let service = peer.service();
    let result = service.activate(uuid(), session.clone(), 1, "/not/a/workspace".into());
    let error = result.unwrap_err();
    assert_eq!(
        (error.code, error.retry, error.cause),
        (
            SessionFailure::WorkspaceInvalid,
            false,
            Some(ControlError::WorkspaceInvalid)
        )
    );
    // The path went to the authority exactly as it was chosen, and the authority refused
    // it before any gate: nothing was granted and nothing is bound.
    assert_eq!(peer.methods(), [STATUS, RUNTIME, CREATE]);
    let Operation::BindingCreate(asked) = &peer.requests()[2] else {
        unreachable!()
    };
    assert_eq!(asked.workspace_root, "/not/a/workspace");
    assert_eq!(service.stats().leases, 0);
    assert_eq!(world.lock().unwrap().binding, BindingState::Unbound);
}

#[test]
fn a_lease_somebody_else_holds_is_used_or_obeyed_and_never_touched() {
    let session = session_id();
    let theirs = |scopes: Vec<ExecutionScope>| {
        let mut held = lease(&session);
        held.scopes = scopes;
        held
    };
    // One that grants the binding: it is bound under it, and the lease stays as it was.
    let mut world = World::new(&session);
    let held = theirs(vec![
        ExecutionScope::SessionCreate,
        ExecutionScope::SessionResume,
    ]);
    world.lease = Some(held.clone());
    let (peer, world) = authority(world);
    let service = peer.service();
    let operation = uuid();
    let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());
    assert_eq!(stage(step()), ActivationStage::Bound);
    assert_eq!(peer.methods(), [STATUS, RUNTIME, CREATE]);
    assert_eq!(stage(step()), ActivationStage::Ready);
    assert_eq!(world.lock().unwrap().lease, Some(held));
    assert_eq!(service.stats().leases, 0);
    assert!(!peer.methods().iter().any(|m| m == GRANT || m == REVOKE));

    // One that does not: the binding is denied. It is neither widened nor replaced.
    let mut world = World::new(&session);
    let narrow = theirs(vec![ExecutionScope::TurnStart]);
    world.lease = Some(narrow.clone());
    let (peer, world) = authority(world);
    let service = peer.service();
    assert_eq!(
        failed(service.activate(uuid(), session.clone(), 1, WORKSPACE.into())),
        (SessionFailure::PermissionDenied, false)
    );
    assert_eq!(peer.methods(), [STATUS, RUNTIME, CREATE, GATE]);
    let world = world.lock().unwrap();
    assert_eq!(
        (world.lease.clone(), world.binding),
        (Some(narrow), BindingState::Unbound)
    );
    assert_eq!(service.stats().leases, 0);
}

#[test]
fn a_revoke_whose_answer_is_lost_is_asked_again_with_the_same_identity() {
    let session = session_id();
    for fault in [Fault::Lost, Fault::AnswerLost] {
        let (peer, world) = authority(World::new(&session).fault(REVOKE, 1, fault));
        let service = peer.service();
        let operation = uuid();
        let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());
        // The binding was made and answered: that is what the step reports.
        assert_eq!(stage(step()), ActivationStage::Bound);
        let lost = matches!(fault, Fault::Lost);
        assert_eq!(world.lock().unwrap().lease.is_some(), lost);
        let first = peer.requests();
        let Some(Operation::LeaseRevoke(asked)) = first.last() else {
            unreachable!()
        };

        // The next step for the session settles it before anything else: the same
        // revoke, for the lease this desktop was granted.
        let sent = first.len();
        assert_eq!(stage(step()), ActivationStage::Ready);
        let Operation::LeaseRevoke(again) = &peer.requests()[sent] else {
            unreachable!()
        };
        assert_eq!(again, asked);
        assert!(world.lock().unwrap().lease.is_none());
        // Settled: the step after that has nothing left to give back.
        let sent = peer.requests().len();
        assert_eq!(stage(step()), ActivationStage::Ready);
        assert_eq!(since(&peer, sent), [STATUS, CREATE]);
    }
}

#[test]
fn a_grant_whose_answer_is_lost_is_found_by_its_own_identity_and_given_back() {
    let session = session_id();
    // Taken, answer lost: a lease of this desktop stands and its identity is unknown.
    let (peer, world) = authority(World::new(&session).fault(GRANT, 1, Fault::AnswerLost));
    let service = peer.service();
    let operation = uuid();
    let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());
    assert_eq!(failed(step()), (SessionFailure::Unavailable, true));
    let granted = world.lock().unwrap().lease.clone().expect("granted");
    let Some(Operation::LeaseGrant(asked)) = peer.requests().last().cloned() else {
        unreachable!()
    };
    let sent = peer.requests().len();
    // The retry asks whether a lease stands, names its own grant again to learn which
    // lease it was given, and gives exactly that one back before going on.
    assert_eq!(stage(step()), ActivationStage::Bound);
    let after = since(&peer, sent);
    assert_eq!(after[..3], [ACTIVE, GRANT, REVOKE]);
    let requests = peer.requests().split_off(sent);
    let Operation::LeaseGrant(again) = &requests[1] else {
        unreachable!()
    };
    assert_eq!(again, &asked);
    let Operation::LeaseRevoke(revoke) = &requests[2] else {
        unreachable!()
    };
    assert_eq!(revoke.lease_id, granted.lease_id);
    // The binding itself then took a lease of its own, also given back.
    assert_eq!(
        after[3..],
        [STATUS, RUNTIME, CREATE, GATE, GRANT, CREATE, REVOKE]
    );
    assert!(world.lock().unwrap().lease.is_none());

    // Never taken: no lease stands, so the grant is not sent again (that would grant it).
    let (peer, world) = authority(World::new(&session).fault(GRANT, 1, Fault::Lost));
    let service = peer.service();
    let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());
    assert_eq!(failed(step()), (SessionFailure::Unavailable, true));
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Bound);
    assert_eq!(
        since(&peer, sent),
        [ACTIVE, STATUS, RUNTIME, CREATE, GATE, GRANT, CREATE, REVOKE]
    );
    assert!(world.lock().unwrap().lease.is_none());

    // Never taken, and somebody else's lease stands meanwhile: sending the grant again
    // is refused by the authority, and their lease is not revoked.
    let (peer, world) = authority(World::new(&session).fault(GRANT, 1, Fault::Lost));
    let service = peer.service();
    let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());
    assert_eq!(failed(step()), (SessionFailure::Unavailable, true));
    let mut theirs = lease(&session);
    theirs.scopes = vec![ExecutionScope::TurnStart];
    world.lock().unwrap().lease = Some(theirs.clone());
    let sent = peer.requests().len();
    assert_eq!(failed(step()), (SessionFailure::PermissionDenied, false));
    assert_eq!(
        since(&peer, sent),
        [ACTIVE, GRANT, STATUS, RUNTIME, CREATE, GATE]
    );
    assert_eq!(world.lock().unwrap().lease, Some(theirs));
}

#[test]
fn an_uncertain_binding_is_never_asked_for_again_and_never_called_a_refusal() {
    let session = session_id();
    // The authority could not tell what the provider did.
    for unknown in [
        ControlError::ExternalEffectUnknown,
        ControlError::BindingIndeterminate,
    ] {
        let (peer, world) =
            authority(World::new(&session).fault(CREATE, 2, Fault::Refuse(unknown)));
        let service = peer.service();
        let operation = uuid();
        let error = service
            .activate(operation.clone(), session.clone(), 1, WORKSPACE.into())
            .unwrap_err();
        assert_eq!(
            (error.code, error.retry, error.cause),
            (SessionFailure::Uncertain, false, Some(unknown))
        );
        // The lease asked for it was still given back.
        assert_eq!(peer.methods().last().map(String::as_str), Some(REVOKE));
        assert!(world.lock().unwrap().lease.is_none());

        // The authority now holds the attempt as indeterminate. Whatever identity the
        // caller brings, the same action or a new one, no binding request is sent.
        world.lock().unwrap().binding = BindingState::Indeterminate;
        world.lock().unwrap().operation = Some(uuid());
        for action in [operation.clone(), uuid()] {
            let sent = peer.requests().len();
            let error = service
                .activate(action, session.clone(), 1, WORKSPACE.into())
                .unwrap_err();
            assert_eq!(
                (error.code, error.retry),
                (SessionFailure::Uncertain, false)
            );
            assert_eq!(since(&peer, sent), [STATUS]);
        }
        assert_eq!(peer.methods().iter().filter(|m| *m == CREATE).count(), 2);
    }
}

#[test]
fn a_binding_whose_answer_is_lost_is_settled_by_the_same_intent_and_never_a_new_one() {
    let session = session_id();
    let (peer, world) = authority(World::new(&session).fault(CREATE, 2, Fault::AnswerLost));
    let service = peer.service();
    let operation = uuid();
    let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());
    // The authority bound the workspace and its answer never left.
    assert_eq!(failed(step()), (SessionFailure::Unavailable, true));
    assert_eq!(world.lock().unwrap().binding, BindingState::BoundDetached);
    let Operation::BindingCreate(first) = peer.requests()[2].clone() else {
        unreachable!()
    };

    // While the authority is still waiting on the runtime, the retry only waits.
    world.lock().unwrap().binding = BindingState::Creating;
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Creating);
    // (The lease of the lost exchange is given back first.)
    assert_eq!(since(&peer, sent), [REVOKE, STATUS]);

    // Once it is bound, the retry proves the binding with the identity it first used
    // and goes on to attach: no second binding was ever asked for.
    world.lock().unwrap().binding = BindingState::BoundDetached;
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Ready);
    let Operation::BindingCreate(proof) = &peer.requests()[sent + 1] else {
        unreachable!()
    };
    assert_eq!(proof, &first);
    let creates: Vec<WireUuid> = peer
        .requests()
        .into_iter()
        .filter_map(|op| match op {
            Operation::BindingCreate(p) => Some(p.intent_id),
            _ => None,
        })
        .collect();
    assert!(creates.iter().all(|intent| intent == &first.intent_id));
}

#[test]
fn a_binding_this_action_did_not_make_is_not_attached_in_its_name() {
    let session = session_id();
    for state in [BindingState::BoundDetached, BindingState::BoundAttached] {
        let mut world = World::new(&session);
        (world.binding, world.bound_by) = (state, Some(uuid()));
        let (peer, world) = authority(world);
        let service = peer.service();
        let error = service
            .activate(uuid(), session.clone(), 1, WORKSPACE.into())
            .unwrap_err();
        assert_eq!(
            (error.code, error.retry),
            (SessionFailure::AlreadyBound, false)
        );
        // The authority was asked whose binding it is. No lease, no resume, no start.
        assert_eq!(peer.methods(), [STATUS, CREATE]);
        assert_eq!(world.lock().unwrap().binding, state);
        assert_eq!(service.stats().leases, 0);
    }
    // The question itself going unanswered is not an answer.
    let mut world = World::new(&session);
    (world.binding, world.bound_by) = (BindingState::BoundDetached, Some(uuid()));
    let (peer, _) = authority(world.fault(CREATE, 1, Fault::Lost));
    assert_eq!(
        failed(
            peer.service()
                .activate(uuid(), session.clone(), 1, WORKSPACE.into())
        ),
        (SessionFailure::Unavailable, true)
    );
}

#[test]
fn a_receipt_of_an_attachment_is_not_taken_for_a_session_that_is_attached() {
    let session = session_id();
    let mut world = World::new(&session);
    world.attaches = false;
    let (peer, world) = authority(world);
    let service = peer.service();
    let operation = uuid();
    let step = || service.activate(operation.clone(), session.clone(), 1, WORKSPACE.into());
    assert_eq!(stage(step()), ActivationStage::Bound);
    // The authority answers the resume and the session is still detached: it is not
    // reported ready, and the lease is given back.
    assert_eq!(failed(step()), (SessionFailure::RuntimeUnavailable, false));
    assert_eq!(peer.methods().last().map(String::as_str), Some(STATUS));
    let world = world.lock().unwrap();
    assert_eq!(
        (world.binding, world.lease.clone()),
        (BindingState::BoundDetached, None)
    );
}

#[test]
fn a_closed_session_a_stale_revision_and_an_impossible_one_are_refused() {
    let session = session_id();
    let mut world = World::new(&session);
    world.closed = true;
    let (peer, _) = authority(world);
    let service = peer.service();
    assert_eq!(
        failed(service.activate(uuid(), session.clone(), 2, WORKSPACE.into())),
        (SessionFailure::NotAllowed, false)
    );
    // The gate said why, and no lease was asked for a session that is not prepared.
    assert_eq!(peer.methods(), [STATUS, RUNTIME, CREATE, GATE]);
    assert_eq!(service.stats().leases, 0);

    // A revision the session is not at: the authority refuses the lease, nothing follows.
    let (peer, world) =
        authority(World::new(&session).fault(GRANT, 1, Fault::Refuse(ControlError::StaleRevision)));
    assert_eq!(
        failed(
            peer.service()
                .activate(uuid(), session.clone(), 1, WORKSPACE.into())
        ),
        (SessionFailure::Changed, false)
    );
    assert_eq!(peer.methods(), [STATUS, RUNTIME, CREATE, GATE, GRANT]);
    assert_eq!(world.lock().unwrap().binding, BindingState::Unbound);

    // A revision no session can have is refused here, before a connection is made.
    let (peer, _) = authority(World::new(&session));
    let service = peer.service();
    for revision in [0, 3, u64::MAX] {
        assert_eq!(
            failed(service.activate(uuid(), session.clone(), revision, WORKSPACE.into())),
            (SessionFailure::Invalid, false)
        );
    }
    assert_eq!(service.stats().connections, 0);
}

#[test]
fn a_refused_binding_ends_the_action_and_gives_the_lease_back() {
    let session = session_id();
    for (refusal, code, retry) in [
        (
            ControlError::ProviderRejected,
            SessionFailure::Rejected,
            false,
        ),
        (
            ControlError::RuntimeOperationInProgress,
            SessionFailure::Busy,
            true,
        ),
        (ControlError::CapacityExceeded, SessionFailure::Busy, true),
        (
            ControlError::BindingAlreadyExists,
            SessionFailure::AlreadyBound,
            false,
        ),
        (
            ControlError::IntentConflict,
            SessionFailure::Conflict,
            false,
        ),
        (
            ControlError::WorkspaceInvalid,
            SessionFailure::WorkspaceInvalid,
            false,
        ),
    ] {
        let (peer, world) =
            authority(World::new(&session).fault(CREATE, 2, Fault::Refuse(refusal)));
        let service = peer.service();
        let error = service
            .activate(uuid(), session.clone(), 1, WORKSPACE.into())
            .unwrap_err();
        assert_eq!((error.code, error.retry), (code, retry), "{refusal:?}");
        assert_eq!(
            peer.methods(),
            [STATUS, RUNTIME, CREATE, GATE, GRANT, CREATE, REVOKE]
        );
        let world = world.lock().unwrap();
        assert_eq!(
            (world.binding, world.lease.clone()),
            (BindingState::Unbound, None)
        );
    }
}

#[test]
fn shutdown_cancels_a_binding_nobody_answers_and_sends_nothing_after_it() {
    let session = session_id();
    let peer = Peer::canonical({
        let session = session.clone();
        move |op| match op {
            Operation::BindingStatus(_) => answer(Ok(World::new(&session).status())),
            Operation::RuntimeStatus(_) => answer(Ok(World::new(&session).runtime())),
            Operation::BindingCreate(_) => Reply::Silent,
            other => panic!("{other:?}"),
        }
    });
    let service = peer.service();
    let call = thread::spawn({
        let (service, session) = (service.clone(), session.clone());
        move || service.activate(uuid(), session, 1, WORKSPACE.into())
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while peer.requests().len() < 3 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    let started = Instant::now();
    service.shutdown();
    assert_eq!(
        failed(call.join().unwrap()),
        (SessionFailure::Unavailable, true)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    // No compensation is attempted and nothing more is asked: what the authority and
    // the provider hold is what the next launch reads.
    assert_eq!(peer.methods(), [STATUS, RUNTIME, CREATE]);
    assert_eq!(
        failed(service.activate(uuid(), session, 1, WORKSPACE.into())),
        (SessionFailure::Unavailable, true)
    );
    thread::sleep(Duration::from_millis(100));
    assert_eq!(*peer.accepted.lock().unwrap(), 1);
}

#[test]
fn only_the_attempt_the_authority_holds_as_uncertain_can_be_abandoned() {
    let session = session_id();
    let attempt = uuid();
    let mut world = World::new(&session);
    (world.binding, world.operation) = (BindingState::Indeterminate, Some(attempt.clone()));
    let (peer, world) = authority(world);
    let service = peer.service();
    let intent = uuid();
    assert_eq!(
        service.abandon(intent.clone(), session.clone()).unwrap().id,
        session
    );
    assert_eq!(peer.methods(), [STATUS, ABANDON]);
    let Operation::BindingAbandon(asked) = &peer.requests()[1] else {
        unreachable!()
    };
    // The attempt is the one the authority named; the caller named none.
    assert_eq!(
        (
            &asked.intent_id,
            &asked.managed_session_id,
            &asked.operation_id
        ),
        (&intent, &session, &attempt)
    );
    assert_eq!(world.lock().unwrap().binding, BindingState::Unbound);

    // Nothing uncertain to give up on: nothing is sent.
    for state in [
        BindingState::Unbound,
        BindingState::Creating,
        BindingState::BoundDetached,
        BindingState::BoundAttached,
    ] {
        world.lock().unwrap().binding = state;
        let sent = peer.requests().len();
        let error = service.abandon(uuid(), session.clone()).unwrap_err();
        assert_eq!(
            (error.code, error.retry),
            (SessionFailure::NotAllowed, false)
        );
        assert_eq!(since(&peer, sent), [STATUS]);
    }
}
