//! Modes against a scripted authority: the one runtime, its profile and its workspace,
//! and what a step asks for when the runtime that is up is not the one a session needs.
//! How the real authority and the real namespace behave is covered by
//! `tests/approvals_process.rs`; this peer models only what a step can observe.
use super::*;
use crate::{contracts::control::RuntimeSafetyProfile, control::client::ControlClient};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

const WORKSPACE: &str = "/work/space";
const OTHER: &str = "/work/other";

#[derive(Clone, Copy)]
enum Fault {
    Refuse(ControlError),
    /// The request takes effect and its answer never leaves.
    AnswerLost,
}

/// The authority as a mode step can observe it: one runtime, one session.
struct Machine {
    session: ManagedSessionId,
    binding: BindingState,
    bound_by: Option<WireUuid>,
    /// The mode the binding was created in. The provider resumes it only there.
    born: Option<AgentMode>,
    desired: DesiredState,
    actual: ActualState,
    diagnostic: Option<RuntimeDiagnostic>,
    revision: u64,
    /// The generation that exists: none, inference (`Some(None)`) or local tools for a
    /// workspace.
    generation: Option<Option<String>>,
    starts: HashMap<String, u64>,
    stops: HashMap<String, u64>,
    lease: Option<ExecutionLease>,
    grants: HashMap<String, ExecutionLease>,
    faults: Vec<(&'static str, usize, Fault)>,
    seen: HashMap<String, usize>,
}
/// The one spelling the authority keeps of a folder.
fn canonical(path: &str) -> String {
    path.replace("/./", "/")
}
impl Machine {
    /// A runtime that is ready in `generation`, and a session with no binding.
    fn ready(session: &ManagedSessionId, generation: Option<&str>) -> Self {
        Self {
            session: session.clone(),
            binding: BindingState::Unbound,
            bound_by: None,
            born: None,
            desired: DesiredState::Running,
            actual: ActualState::Ready,
            diagnostic: None,
            revision: 1,
            generation: Some(generation.map(str::to_owned)),
            starts: HashMap::new(),
            stops: HashMap::new(),
            lease: None,
            grants: HashMap::new(),
            faults: Vec::new(),
            seen: HashMap::new(),
        }
    }
    fn stopped(session: &ManagedSessionId) -> Self {
        Self {
            desired: DesiredState::Stopped,
            actual: ActualState::Stopped,
            revision: 0,
            generation: None,
            ..Self::ready(session, None)
        }
    }
    /// A session whose binding was created in `mode`, detached.
    fn bound(mut self, mode: AgentMode) -> Self {
        (self.binding, self.born) = (BindingState::BoundDetached, Some(mode));
        self
    }
    fn fault(mut self, method: &'static str, nth: usize, fault: Fault) -> Self {
        self.faults.push((method, nth, fault));
        self
    }
    fn mode(&self) -> Option<AgentMode> {
        match (&self.generation, self.actual) {
            (Some(None), ActualState::Ready) => Some(AgentMode::Inference),
            (Some(Some(_)), ActualState::Ready) => Some(AgentMode::LocalTools),
            _ => None,
        }
    }
    /// What the authority's worker does next, by itself.
    fn settle(&mut self) {
        match self.actual {
            ActualState::Stopping => {
                (self.actual, self.generation) = (ActualState::Stopped, None);
                if self.binding == BindingState::BoundAttached {
                    self.binding = BindingState::BoundDetached;
                }
            }
            ActualState::Starting => self.actual = ActualState::Ready,
            _ => (),
        }
    }
    fn reason(&self, action: ExecutionScope) -> GateReason {
        match &self.lease {
            None => GateReason::LeaseMissing,
            Some(lease) if !lease.scopes.contains(&action) => GateReason::ScopeNotGranted,
            Some(_) if self.desired != DesiredState::Running => GateReason::RuntimeNotRunning,
            Some(_) if self.actual != ActualState::Ready => GateReason::RuntimeNotReady,
            Some(_) => GateReason::Allowed,
        }
    }
    fn control(&self, revision: u64, desired: &str) -> ResultValue {
        ResultValue::RuntimeControl(
            serde_json::from_value(json!({
                "adapter_id": "codex", "runtime_kind": "codex", "desired_state": desired,
                "revision": revision, "created_at": "2026-10-08T00:00:00Z",
                "updated_at": "2026-10-08T00:00:00Z",
            }))
            .unwrap(),
        )
    }
    fn apply(&mut self, op: &Operation) -> ControlResult<ResultValue> {
        match op {
            Operation::BindingStatus(_) => {
                let bound = matches!(
                    self.binding,
                    BindingState::BoundDetached | BindingState::BoundAttached
                );
                let attached = self.binding == BindingState::BoundAttached;
                Ok(ResultValue::BindingStatus(
                    serde_json::from_value(json!({
                        "managed_session_id": self.session, "state": self.binding,
                        "binding": bound.then(|| binding(&self.session)),
                        "runtime_instance_id": attached.then(uuid),
                        "operation_id": null,
                        "attachment_profile": attached.then(|| match self.mode() {
                            Some(AgentMode::LocalTools) => "local_tools_v1",
                            _ => "inference_only_v1",
                        }),
                    }))
                    .unwrap(),
                ))
            }
            Operation::RuntimeStatus(_) => {
                let ready = self.actual == ActualState::Ready;
                let (profile, workspace) = match (&self.generation, ready) {
                    (Some(None), true) => (Some("codex_inference_v1"), None),
                    (Some(Some(w)), true) => (Some("codex_local_tools_v1"), Some(w.as_str())),
                    _ => (None, None),
                };
                let mut status = match runtime_status(profile, workspace) {
                    ResultValue::RuntimeStatus(status) => serde_json::to_value(status).unwrap(),
                    _ => unreachable!(),
                };
                status["desired_state"] = json!(self.desired);
                status["actual_state"] = json!(self.actual);
                status["revision"] = json!(self.revision);
                status["diagnostic"] = json!(self.diagnostic);
                Ok(ResultValue::RuntimeStatus(
                    serde_json::from_value(status).unwrap(),
                ))
            }
            Operation::RuntimeStart(p) => {
                // Admission first, as the authority does it: a live generation never
                // changes profile or workspace, whoever asks and however often.
                let workspace = p.workspace_root.as_deref().map(canonical);
                if workspace.as_deref() == Some("/not/a/folder") {
                    return Err(ControlError::WorkspaceInvalid);
                }
                if self
                    .generation
                    .as_ref()
                    .is_some_and(|live| *live != workspace)
                {
                    return Err(ControlError::InvalidTransition);
                }
                if let Some(revision) = self.starts.get(p.intent_id.as_str()) {
                    return Ok(self.control(*revision, "running"));
                }
                self.revision += 1;
                self.desired = DesiredState::Running;
                if self.generation.is_none() {
                    (self.generation, self.actual) = (Some(workspace), ActualState::Starting);
                    self.diagnostic = None;
                }
                self.starts
                    .insert(p.intent_id.as_str().to_owned(), self.revision);
                Ok(self.control(self.revision, "running"))
            }
            Operation::RuntimeStop(p) => {
                if let Some(revision) = self.stops.get(p.intent_id.as_str()) {
                    return Ok(self.control(*revision, "stopped"));
                }
                self.revision += 1;
                self.desired = DesiredState::Stopped;
                if self.generation.is_some() {
                    self.actual = ActualState::Stopping;
                }
                self.stops
                    .insert(p.intent_id.as_str().to_owned(), self.revision);
                Ok(self.control(self.revision, "stopped"))
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
                let mut granted = lease(&self.session);
                granted.scopes = p.scopes.clone();
                self.grants
                    .insert(p.intent_id.as_str().to_owned(), granted.clone());
                self.lease = Some(granted.clone());
                Ok(ResultValue::Lease(granted))
            }
            Operation::LeaseRevoke(_) => {
                let mut held = self.lease.take().ok_or(ControlError::InvalidTransition)?;
                held.state = LeaseState::Revoked;
                held.revision = 2;
                held.revocation_reason = Some(RevocationReason::UserRequest);
                Ok(ResultValue::Lease(held))
            }
            Operation::BindingCreate(p) => {
                if self.bound_by.as_ref() == Some(&p.intent_id) {
                    return Ok(ResultValue::Binding(binding(&self.session)));
                }
                if self.binding != BindingState::Unbound {
                    return Err(ControlError::BindingAlreadyExists);
                }
                // A local-tools generation binds only the folder it was started for.
                if let Some(Some(served)) = &self.generation
                    && canonical(&p.workspace_root) != *served
                {
                    return Err(ControlError::WorkspaceInvalid);
                }
                if self.reason(ExecutionScope::SessionCreate) != GateReason::Allowed {
                    return Err(ControlError::GateDenied);
                }
                self.binding = BindingState::BoundDetached;
                self.bound_by = Some(p.intent_id.clone());
                self.born = self.mode();
                Ok(ResultValue::Binding(binding(&self.session)))
            }
            Operation::BindingResume(_) => {
                if self.reason(ExecutionScope::SessionResume) != GateReason::Allowed {
                    return Err(ControlError::GateDenied);
                }
                // The conversation lives in the home of the profile that created it.
                if self.born != self.mode() {
                    return Err(ControlError::ProviderRejected);
                }
                self.binding = BindingState::BoundAttached;
                Ok(ResultValue::Binding(binding(&self.session)))
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
fn authority(machine: Machine) -> (Peer, Arc<Mutex<Machine>>) {
    let machine = Arc::new(Mutex::new(machine));
    let shared = machine.clone();
    let peer = Peer::canonical(move |op| {
        let mut machine = shared.lock().unwrap();
        let name = method(op);
        let nth = {
            let seen = machine.seen.entry(name.clone()).or_insert(0);
            *seen += 1;
            *seen
        };
        let fault = machine
            .faults
            .iter()
            .find(|(m, n, _)| *m == name && *n == nth)
            .map(|(_, _, fault)| *fault);
        match fault {
            Some(Fault::Refuse(error)) => answer(Err(error)),
            Some(Fault::AnswerLost) => {
                let _ = machine.apply(op);
                Reply::HangUp
            }
            None => answer(machine.apply(op)),
        }
    });
    (peer, machine)
}

const STATUS: &str = "managedSession/runtime/status";
const CREATE: &str = "managedSession/runtime/create";
const RESUME: &str = "managedSession/runtime/resume";
const RUNTIME: &str = "runtime/status";
const START: &str = "runtime/start";
const STOP: &str = "runtime/stop";
const GATE: &str = "executionGate/status";
const GRANT: &str = "executionLease/grant";
const REVOKE: &str = "executionLease/revoke";

/// Whether a turn runs, as the test says, and how often that was asked.
#[derive(Default)]
struct Turn {
    running: AtomicBool,
    asked: AtomicUsize,
}
impl Turn {
    fn guard(&self) -> impl Fn(&mut ControlClient) -> Result<bool, SessionError> + '_ {
        |_| {
            self.asked.fetch_add(1, Ordering::AcqRel);
            Ok(self.running.load(Ordering::Acquire))
        }
    }
    fn asked(&self) -> usize {
        self.asked.load(Ordering::Acquire)
    }
}
fn stage(result: Result<ActivationProgress, SessionError>) -> ActivationStage {
    result.unwrap().stage
}
fn failed(result: Result<ActivationProgress, SessionError>) -> (SessionFailure, bool) {
    let error = result.unwrap_err();
    (error.code, error.retry)
}
fn since(peer: &Peer, from: usize) -> Vec<String> {
    peer.methods().split_off(from)
}
fn starts(peer: &Peer) -> Vec<RuntimeMutation> {
    peer.requests()
        .into_iter()
        .filter_map(|op| match op {
            Operation::RuntimeStart(p) => Some(p),
            _ => None,
        })
        .collect()
}

#[test]
fn inference_is_the_default_profile_and_local_tools_is_named_with_its_one_workspace() {
    let session = session_id();
    let turn = Turn::default();
    for (mode, profile, workspace) in [
        (AgentMode::Inference, None, None),
        (
            AgentMode::LocalTools,
            Some(RuntimeSafetyProfile::CodexLocalToolsV1),
            Some(WORKSPACE.to_owned()),
        ),
    ] {
        let (peer, _) = authority(Machine::stopped(&session));
        let service = peer.service();
        let step = service.activate_in(
            uuid(),
            session.clone(),
            1,
            WORKSPACE.into(),
            mode,
            &turn.guard(),
        );
        assert_eq!(stage(step), ActivationStage::RuntimeStarting);
        assert_eq!(peer.methods(), [STATUS, RUNTIME, START]);
        let asked = &starts(&peer)[0];
        // Inference names no profile: it is the authority's own default. Local tools is
        // named, with the folder the user selected and nothing else.
        assert_eq!(
            (asked.safety_profile, asked.workspace_root.clone()),
            (profile, workspace)
        );
        assert_eq!(asked.expected_revision, None);
        assert_eq!(asked.adapter_id.as_str(), "codex");
    }
    // Nothing had to stop, so whether a turn runs was nobody's question.
    assert_eq!(turn.asked(), 0);
}

#[test]
fn a_runtime_in_another_mode_is_stopped_first_started_again_and_only_then_bound() {
    let session = session_id();
    let (peer, machine) = authority(Machine::ready(&session, None));
    let service = peer.service();
    let turn = Turn::default();
    let operation = uuid();
    let step = || {
        service.activate_in(
            operation.clone(),
            session.clone(),
            1,
            WORKSPACE.into(),
            AgentMode::LocalTools,
            &turn.guard(),
        )
    };

    // The runtime is ready in inference. It is the only one: it is asked to stop.
    assert_eq!(stage(step()), ActivationStage::RuntimeStopping);
    assert_eq!(peer.methods(), [STATUS, RUNTIME, STOP]);
    assert_eq!(turn.asked(), 1);
    let Some(Operation::RuntimeStop(stop)) = peer.requests().pop() else {
        unreachable!()
    };
    // A stop names no profile and no workspace, and no revision of the desktop's choosing.
    assert_eq!(
        (
            stop.safety_profile,
            stop.workspace_root.clone(),
            stop.expected_revision
        ),
        (None, None, None)
    );
    // On its way down: nothing is asked, least of all a second stop.
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::RuntimeStopping);
    assert_eq!(since(&peer, sent), [STATUS, RUNTIME]);
    machine.lock().unwrap().settle();

    // Down: the start, for local tools and this folder.
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::RuntimeStarting);
    assert_eq!(since(&peer, sent), [STATUS, RUNTIME, START]);
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::RuntimeStarting);
    assert_eq!(since(&peer, sent), [STATUS, RUNTIME]);
    machine.lock().unwrap().settle();
    assert_eq!(machine.lock().unwrap().mode(), Some(AgentMode::LocalTools));

    // Up, in the mode asked: bound, then attached, as any activation.
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Bound);
    assert_eq!(
        since(&peer, sent),
        [STATUS, RUNTIME, CREATE, GATE, GRANT, CREATE, REVOKE]
    );
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Ready);
    assert_eq!(
        since(&peer, sent),
        [
            STATUS, CREATE, RUNTIME, RESUME, GATE, GRANT, RESUME, REVOKE, STATUS
        ]
    );
    assert_eq!(machine.lock().unwrap().born, Some(AgentMode::LocalTools));
    // Whether a turn ran was asked once, before the one stop, and never again.
    assert_eq!(turn.asked(), 1);
    assert_eq!(peer.methods().iter().filter(|m| *m == STOP).count(), 1);

    // Every mutation of the action has an identity of its own, and it is the action's:
    // the same step again is the same intent.
    let mut identities: Vec<String> = peer
        .requests()
        .into_iter()
        .filter_map(|op| match op {
            Operation::RuntimeStop(p) | Operation::RuntimeStart(p) => Some(p.intent_id),
            Operation::BindingCreate(p) => Some(p.intent_id),
            Operation::BindingResume(p) => Some(p.intent_id),
            _ => None,
        })
        .map(|id| id.as_str().to_owned())
        .collect();
    identities.sort();
    identities.dedup();
    assert_eq!(identities.len(), 4);
    assert!(!identities.contains(&operation.as_str().to_owned()));
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Ready);
    assert_eq!(since(&peer, sent), [STATUS, CREATE]);
}

#[test]
fn a_runtime_is_never_asked_to_stop_while_a_turn_is_read_as_running() {
    let session = session_id();
    let turn = Turn::default();
    turn.running.store(true, Ordering::Release);
    for (generation, mode, workspace) in [
        // Inference is up, local tools is asked for.
        (None, AgentMode::LocalTools, WORKSPACE),
        // Local tools is up, inference is asked for.
        (Some(WORKSPACE), AgentMode::Inference, WORKSPACE),
        // Local tools is up for one folder, and asked for another.
        (Some(OTHER), AgentMode::LocalTools, WORKSPACE),
    ] {
        let (peer, machine) = authority(Machine::ready(&session, generation));
        let service = peer.service();
        let before = {
            let machine = machine.lock().unwrap();
            (machine.revision, machine.mode())
        };
        for _ in 0..3 {
            assert_eq!(
                failed(service.activate_in(
                    uuid(),
                    session.clone(),
                    1,
                    workspace.into(),
                    mode,
                    &turn.guard()
                )),
                (SessionFailure::TurnActive, false)
            );
        }
        // Not stopped, not started, nothing bound: the runtime is as it was.
        assert!(!peer.methods().iter().any(|m| m == STOP || m == CREATE));
        let machine = machine.lock().unwrap();
        assert_eq!((machine.revision, machine.mode()), before);
        assert_eq!(machine.binding, BindingState::Unbound);
        assert_eq!(machine.desired, DesiredState::Running);
    }
    // The shorthand that names no mode assumes a turn may be running: it never stops one.
    let (peer, _) = authority(Machine::ready(&session, Some(WORKSPACE)));
    assert_eq!(
        failed(
            peer.service()
                .activate(uuid(), session.clone(), 1, WORKSPACE.into())
        ),
        (SessionFailure::TurnActive, false)
    );
    assert!(!peer.methods().iter().any(|m| m == STOP));
    // And a reading that fails is not a reading of "no turn".
    let (peer, _) = authority(Machine::ready(&session, None));
    let unknown = |_: &mut ControlClient| Err(SessionError::new(SessionFailure::Unavailable));
    assert_eq!(
        failed(peer.service().activate_in(
            uuid(),
            session.clone(),
            1,
            WORKSPACE.into(),
            AgentMode::LocalTools,
            &unknown
        )),
        (SessionFailure::Unavailable, true)
    );
    assert!(!peer.methods().iter().any(|m| m == STOP));
}

#[test]
fn a_stop_the_authority_refuses_under_a_turn_is_said_as_a_running_turn_and_changes_nothing() {
    let session = session_id();
    // The desktop reads that no turn runs. By the time its stop arrives one was admitted,
    // and the authority, not the desktop, is what keeps the runtime up.
    let idle = Turn::default();
    for (generation, mode) in [
        (None, AgentMode::LocalTools),
        (Some(WORKSPACE), AgentMode::Inference),
        (Some(OTHER), AgentMode::LocalTools),
    ] {
        let refuse = Fault::Refuse(ControlError::ActiveTurnExists);
        let (peer, machine) = authority(
            Machine::ready(&session, generation)
                .fault(STOP, 1, refuse)
                .fault(STOP, 2, refuse),
        );
        let service = peer.service();
        let before = {
            let machine = machine.lock().unwrap();
            (machine.revision, machine.mode())
        };
        let operation = uuid();
        for asked in 1..=2 {
            // Not a refusal to start Codex, not "busy": the turn that is running.
            assert_eq!(
                failed(service.activate_in(
                    operation.clone(),
                    session.clone(),
                    1,
                    WORKSPACE.into(),
                    mode,
                    &idle.guard()
                )),
                (SessionFailure::TurnActive, false)
            );
            // The stop was asked, once per attempt, and nothing followed it: no start
            // of another generation, nothing bound.
            let methods = peer.methods();
            assert_eq!(methods.iter().filter(|m| *m == STOP).count(), asked);
            assert_eq!(methods.last().map(String::as_str), Some(STOP));
            assert!(!methods.iter().any(|m| m == CREATE));
        }
        {
            let machine = machine.lock().unwrap();
            assert_eq!((machine.revision, machine.mode()), before);
            assert_eq!(machine.binding, BindingState::Unbound);
            assert_eq!(machine.desired, DesiredState::Running);
        }
        // The turn ended: the same action goes on from where it was refused.
        assert_eq!(
            stage(service.activate_in(
                operation,
                session.clone(),
                1,
                WORKSPACE.into(),
                mode,
                &idle.guard()
            )),
            ActivationStage::RuntimeStopping
        );
    }
}

#[test]
fn a_runtime_that_already_serves_the_mode_is_left_alone() {
    let session = session_id();
    let turn = Turn::default();
    turn.running.store(true, Ordering::Release);
    for (generation, mode) in [
        (None, AgentMode::Inference),
        (Some(WORKSPACE), AgentMode::LocalTools),
    ] {
        let (peer, _) = authority(Machine::ready(&session, generation));
        let step = peer.service().activate_in(
            uuid(),
            session.clone(),
            1,
            WORKSPACE.into(),
            mode,
            &turn.guard(),
        );
        assert_eq!(stage(step), ActivationStage::Bound);
        assert!(!peer.methods().iter().any(|m| m == STOP || m == START));
    }
    // Nothing had to stop: a running turn is no obstacle, and nobody asked about one.
    assert_eq!(turn.asked(), 0);
}

#[test]
fn whether_two_spellings_are_one_folder_is_the_authority_s_to_say() {
    let session = session_id();
    let turn = Turn::default();
    // The generation serves `/work/space`. The folder the user selected is spelled
    // otherwise: only the authority knows whether it is the same one.
    let (peer, machine) = authority(Machine::ready(&session, Some(WORKSPACE)));
    let step = peer.service().activate_in(
        uuid(),
        session.clone(),
        1,
        "/work/./space".into(),
        AgentMode::LocalTools,
        &turn.guard(),
    );
    // It was: the authority admitted a start for it on the live generation, launched
    // nothing, and the binding goes ahead. Nothing was stopped.
    assert_eq!(stage(step), ActivationStage::Bound);
    assert_eq!(
        peer.methods(),
        [STATUS, RUNTIME, START, CREATE, GATE, GRANT, CREATE, REVOKE]
    );
    assert_eq!(
        starts(&peer)[0].workspace_root.as_deref(),
        Some("/work/./space")
    );
    assert_eq!(machine.lock().unwrap().actual, ActualState::Ready);
    assert_eq!(turn.asked(), 0);

    // Another folder: the authority refuses that start, and so the runtime has to stop.
    let (peer, _) = authority(Machine::ready(&session, Some(OTHER)));
    let step = peer.service().activate_in(
        uuid(),
        session.clone(),
        1,
        WORKSPACE.into(),
        AgentMode::LocalTools,
        &turn.guard(),
    );
    assert_eq!(stage(step), ActivationStage::RuntimeStopping);
    assert_eq!(peer.methods(), [STATUS, RUNTIME, START, STOP]);
    assert_eq!(turn.asked(), 1);
}

#[test]
fn a_bound_session_enters_a_mode_for_the_workspace_the_authority_holds() {
    let session = session_id();
    let turn = Turn::default();
    // Bound in local tools, detached, and the runtime is up in inference: as after an
    // authority restart.
    let (peer, machine) = authority(Machine::ready(&session, None).bound(AgentMode::LocalTools));
    let service = peer.service();
    let operation = uuid();
    let step = || {
        service.enter(
            operation.clone(),
            session.clone(),
            1,
            AgentMode::LocalTools,
            &turn.guard(),
        )
    };
    assert_eq!(stage(step()), ActivationStage::RuntimeStopping);
    machine.lock().unwrap().settle();
    assert_eq!(stage(step()), ActivationStage::RuntimeStarting);
    // The caller named no folder: the one asked for is the binding's own.
    assert_eq!(
        starts(&peer)[0].workspace_root.as_deref(),
        Some(binding(&session).workspace_root.as_str())
    );
    assert_eq!(
        starts(&peer)[0].safety_profile,
        Some(RuntimeSafetyProfile::CodexLocalToolsV1)
    );
    machine.lock().unwrap().settle();
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Ready);
    // No binding request: the session has one.
    assert_eq!(
        since(&peer, sent),
        [STATUS, RUNTIME, RESUME, GATE, GRANT, RESUME, REVOKE, STATUS]
    );
    assert!(!peer.methods().iter().any(|m| m == CREATE));
    assert_eq!(machine.lock().unwrap().binding, BindingState::BoundAttached);
    // Attached in that mode: asked again, there is nothing to do and nothing is sent
    // beyond reading that.
    let sent = peer.requests().len();
    assert_eq!(stage(step()), ActivationStage::Ready);
    assert_eq!(since(&peer, sent), [STATUS]);
    assert_eq!(turn.asked(), 1);
}

#[test]
fn a_session_attached_in_one_mode_is_not_moved_to_the_other() {
    let session = session_id();
    let turn = Turn::default();
    for (generation, born, asked) in [
        (None, AgentMode::Inference, AgentMode::LocalTools),
        (Some(WORKSPACE), AgentMode::LocalTools, AgentMode::Inference),
    ] {
        let mut machine = Machine::ready(&session, generation).bound(born);
        machine.binding = BindingState::BoundAttached;
        let (peer, _) = authority(machine);
        assert_eq!(
            failed(
                peer.service()
                    .enter(uuid(), session.clone(), 1, asked, &turn.guard())
            ),
            (SessionFailure::ModeFixed, false)
        );
        // Known from the attachment the authority reports: the runtime is not even read,
        // let alone stopped for a request the provider could only refuse.
        assert_eq!(peer.methods(), [STATUS]);
    }
    assert_eq!(turn.asked(), 0);
    // A session with no workspace has none to enter a mode for.
    let (peer, _) = authority(Machine::ready(&session, None));
    assert_eq!(
        failed(peer.service().enter(
            uuid(),
            session.clone(),
            1,
            AgentMode::LocalTools,
            &turn.guard()
        )),
        (SessionFailure::NotBound, false)
    );
    assert_eq!(peer.methods(), [STATUS]);
    // And one whose binding is uncertain is not touched.
    let mut machine = Machine::ready(&session, None);
    machine.binding = BindingState::Indeterminate;
    let (peer, _) = authority(machine);
    assert_eq!(
        failed(peer.service().enter(
            uuid(),
            session.clone(),
            1,
            AgentMode::LocalTools,
            &turn.guard()
        )),
        (SessionFailure::Uncertain, false)
    );
    for revision in [0, 3] {
        assert_eq!(
            failed(peer.service().enter(
                uuid(),
                session.clone(),
                revision,
                AgentMode::LocalTools,
                &turn.guard()
            )),
            (SessionFailure::Invalid, false)
        );
    }
}

#[test]
fn a_detached_session_of_the_other_mode_is_refused_by_the_provider_and_said_so() {
    let session = session_id();
    let turn = Turn::default();
    // Bound in inference, detached. Nothing the authority holds says which mode it was
    // bound in, so the request is made, and the provider answers it.
    let (peer, machine) = authority(Machine::ready(&session, None).bound(AgentMode::Inference));
    let service = peer.service();
    let operation = uuid();
    let step = || {
        service.enter(
            operation.clone(),
            session.clone(),
            1,
            AgentMode::LocalTools,
            &turn.guard(),
        )
    };
    assert_eq!(stage(step()), ActivationStage::RuntimeStopping);
    machine.lock().unwrap().settle();
    assert_eq!(stage(step()), ActivationStage::RuntimeStarting);
    machine.lock().unwrap().settle();
    let sent = peer.requests().len();
    assert_eq!(failed(step()), (SessionFailure::Rejected, false));
    // The lease asked for the attempt is given back.
    assert_eq!(
        since(&peer, sent),
        [STATUS, RUNTIME, RESUME, GATE, GRANT, RESUME, REVOKE]
    );
    // What is true now is what is left: local tools is up, the session is detached, and
    // nothing was rolled back or claimed to be.
    let machine = machine.lock().unwrap();
    assert_eq!(machine.mode(), Some(AgentMode::LocalTools));
    assert_eq!(machine.binding, BindingState::BoundDetached);
    assert!(machine.lease.is_none());
}

#[test]
fn a_stop_or_a_start_the_authority_does_not_take_is_reported_and_nothing_loops() {
    let session = session_id();
    let turn = Turn::default();
    let local = |peer: &Peer, operation: &WireUuid, workspace: &str| {
        peer.service().activate_in(
            operation.clone(),
            session.clone(),
            1,
            workspace.into(),
            AgentMode::LocalTools,
            &turn.guard(),
        )
    };
    // A stop whose answer is lost took effect. The same step finds the runtime going
    // down and asks for nothing.
    let (peer, machine) =
        authority(Machine::ready(&session, None).fault(STOP, 1, Fault::AnswerLost));
    let operation = uuid();
    assert_eq!(
        failed(local(&peer, &operation, WORKSPACE)),
        (SessionFailure::Unavailable, true)
    );
    assert_eq!(
        stage(local(&peer, &operation, WORKSPACE)),
        ActivationStage::RuntimeStopping
    );
    assert_eq!(peer.methods().iter().filter(|m| *m == STOP).count(), 1);
    // Somebody else brings the old generation back. This action's stop is already
    // spent: the authority answers it with its receipt, and it is not asked again.
    {
        let mut machine = machine.lock().unwrap();
        machine.settle();
        (machine.generation, machine.actual, machine.desired) =
            (Some(None), ActualState::Ready, DesiredState::Running);
        machine.revision += 1;
    }
    assert_eq!(
        failed(local(&peer, &operation, WORKSPACE)),
        (SessionFailure::RuntimeUnavailable, false)
    );
    assert_eq!(machine.lock().unwrap().mode(), Some(AgentMode::Inference));

    // A stop the authority refuses.
    let refusal = ControlError::StorageUnavailable;
    let (peer, _) =
        authority(Machine::ready(&session, None).fault(STOP, 1, Fault::Refuse(refusal)));
    assert_eq!(
        failed(local(&peer, &uuid(), WORKSPACE)),
        (SessionFailure::Unavailable, true)
    );

    // A folder the authority does not accept as a workspace: said as that, and nothing
    // is started in its place.
    let (peer, machine) = authority(Machine::stopped(&session));
    assert_eq!(
        failed(local(&peer, &uuid(), "/not/a/folder")),
        (SessionFailure::WorkspaceInvalid, false)
    );
    assert_eq!(machine.lock().unwrap().actual, ActualState::Stopped);

    // A generation the authority contained because it could not prove the isolation is
    // not started again from here: it fails closed and stays closed.
    let mut contained = Machine::stopped(&session);
    (contained.desired, contained.actual, contained.diagnostic) = (
        DesiredState::Running,
        ActualState::Failed,
        Some(RuntimeDiagnostic::SafetyUnproven),
    );
    let (peer, _) = authority(contained);
    assert_eq!(
        failed(local(&peer, &uuid(), WORKSPACE)),
        (SessionFailure::RuntimeUnavailable, false)
    );
    assert_eq!(peer.methods(), [STATUS, RUNTIME]);

    // A start this action already asked for, with nothing up afterwards: not asked for
    // a third time.
    let (peer, machine) = authority(Machine::stopped(&session));
    let operation = uuid();
    assert_eq!(
        stage(local(&peer, &operation, WORKSPACE)),
        ActivationStage::RuntimeStarting
    );
    {
        let mut machine = machine.lock().unwrap();
        (machine.actual, machine.generation) = (ActualState::Failed, None);
        machine.diagnostic = Some(RuntimeDiagnostic::ChildExited);
    }
    assert_eq!(
        failed(local(&peer, &operation, WORKSPACE)),
        (SessionFailure::RuntimeUnavailable, false)
    );
    assert_eq!(starts(&peer).len(), 2);
    assert_eq!(turn.asked(), 3);
}
