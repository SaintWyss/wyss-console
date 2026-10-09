//! Activation: making a prepared session usable, one step per call.
//!
//! A session is usable once it is bound to a workspace and attached to the running
//! runtime. Getting there takes several authority operations, and no transaction spans
//! them: any of them can succeed and the next one fail. So nothing here pretends to be
//! one. A call reads what the session and the runtime are *now*, asks the authority for
//! the one operation that is missing, and says how far that got. The caller sends the
//! same request again until the answer is `Ready`.
//!
//! ```text
//! runtime ready in another mode or for another workspace -> runtime/stop -> RuntimeStopping
//! runtime stopping            -> nothing                        -> RuntimeStopping
//! runtime not up              -> runtime/start (the mode asked) -> RuntimeStarting
//! runtime starting            -> nothing                        -> RuntimeStarting
//! unbound,  runtime ready     -> managedSession/runtime/create  -> Bound
//! creating                    -> nothing                        -> Creating
//! bound, detached             -> managedSession/runtime/resume  -> Ready
//! bound, attached             -> nothing                        -> Ready
//! indeterminate               -> nothing                        -> uncertain (an error)
//! ```
//!
//! **One runtime, one mode.** The authority supervises a single managed runtime, and a
//! generation of it has one profile for its whole life: inference, or local tools for
//! exactly one workspace. A session that needs another one needs the runtime stopped and
//! started again, which detaches every session attached to it. That is asked for only as
//! part of an action that names the mode, and never while a managed turn is running. The
//! profile is chosen here from the product's two modes; no profile or path of the
//! window's own reaches the authority.
//!
//! **A session keeps the mode it was bound in.** The provider keeps a conversation in
//! the home of the profile that created it, so the authority can attach a binding only
//! under that profile: under the other one the provider refuses. Nothing here hides
//! that. A session known to be attached in one mode is not moved to the other, and a
//! refusal by the provider is reported as one.
//!
//! **Identities.** The caller names the whole user action once. Each authority mutation
//! it leads to has an identity of its own, derived from that name, so the same step asked
//! again is the same intent with the same payload, and no two operations share one. The
//! leases around a binding or a resume are not the user's action and have identities of
//! their own (see `service`).
//!
//! **Uncertainty.** A binding request whose provider outcome is unknown is never sent
//! under a new identity, and never reported as a refusal. While the authority holds it as
//! indeterminate this desktop sends no binding request at all for that session; the one
//! way out is `abandon`, which the authority audits and which is the user's to ask for.
use super::{
    service::{Grant, RESUME_LEASE_SECONDS, SessionService, session_revision},
    view::{
        ActivationProgress, ActivationStage, AgentMode, SessionError, SessionFailure,
        SessionReceipt,
    },
};
use crate::{
    contracts::{
        WireUuid,
        control::{
            ActualState, BindingAbandon, BindingCreate, BindingResume, BindingState, ControlError,
            DesiredState, ExecutionScope, RuntimeDiagnostic, RuntimeMutation, RuntimeSafetyProfile,
            RuntimeStatus, RuntimeTarget,
        },
        identities::ManagedSessionId,
    },
    control::client::ControlClient,
};
use sha2::{Digest, Sha256};

/// How long the lease for a binding may live if it cannot be revoked afterwards. The
/// authority checks it when it admits the request and once more as it writes to the
/// runtime, both moments after the grant: this covers them and the queue in between.
const CREATE_LEASE_SECONDS: u32 = 15;

/// Whether a managed turn is running, read from the authority. Asked before a runtime
/// is stopped, and only then.
pub type TurnGuard<'a> = &'a dyn Fn(&mut ControlClient) -> Result<bool, SessionError>;

/// The authority mutations one activation can lead to.
#[derive(Clone, Copy)]
enum Step {
    Stop,
    Start,
    Create,
    Attach,
}

/// The identity of one mutation of an activation: a function of the action's own
/// identity and the step, so it is the same whenever that step is asked for again.
fn derive(operation: &WireUuid, step: Step) -> WireUuid {
    let mut hash = Sha256::new();
    hash.update(b"wyss-console/activation/v1\0");
    hash.update(match step {
        Step::Stop => b"stop\0".as_slice(),
        Step::Start => b"start\0".as_slice(),
        Step::Create => b"create\0".as_slice(),
        Step::Attach => b"attach\0".as_slice(),
    });
    // The authority compares identities whatever their case.
    hash.update(operation.as_str().to_ascii_lowercase());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.finalize()[..16]);
    let id = uuid::Builder::from_random_bytes(bytes).into_uuid();
    WireUuid::try_from(id.to_string()).expect("derived UUID")
}

/// The runtime a step needs: a mode and, for local tools, the one workspace.
struct Want<'a> {
    mode: AgentMode,
    workspace: Option<&'a str>,
}
impl<'a> Want<'a> {
    fn new(mode: AgentMode, workspace: &'a str) -> Self {
        Self {
            mode,
            // Only a local-tools generation is isolated for a workspace.
            workspace: (mode == AgentMode::LocalTools).then_some(workspace),
        }
    }
    fn start(&self, operation: &WireUuid) -> RuntimeMutation {
        let target = RuntimeTarget::codex();
        RuntimeMutation {
            intent_id: derive(operation, Step::Start),
            adapter_id: target.adapter_id,
            runtime_kind: target.runtime_kind,
            expected_revision: None,
            // The profile is named here, from the mode: the default one is the
            // authority's own choice when none is named.
            safety_profile: self
                .workspace
                .map(|_| RuntimeSafetyProfile::CodexLocalToolsV1),
            workspace_root: self.workspace.map(str::to_owned),
        }
    }
}

impl SessionService {
    /// Takes the next step towards a session bound to `workspace` and attached, in
    /// inference. It never asks for a runtime to be stopped: with a runtime in another
    /// mode it reports that a turn may be running rather than find out.
    pub fn activate(
        &self,
        operation: WireUuid,
        id: ManagedSessionId,
        expected_revision: u64,
        workspace: String,
    ) -> Result<ActivationProgress, SessionError> {
        self.activate_in(
            operation,
            id,
            expected_revision,
            workspace,
            AgentMode::Inference,
            &|_| Ok(true),
        )
    }

    /// Takes the next step towards a session bound to `workspace` and attached in `mode`.
    ///
    /// `operation` names the user's action and `expected_revision` is the session
    /// revision the caller saw. `workspace` is what the user chose: whether it may be a
    /// workspace is the authority's to say. `turn_active` is asked before a runtime in
    /// another mode is stopped.
    pub fn activate_in(
        &self,
        operation: WireUuid,
        id: ManagedSessionId,
        expected_revision: u64,
        workspace: String,
        mode: AgentMode,
        turn_active: TurnGuard<'_>,
    ) -> Result<ActivationProgress, SessionError> {
        session_revision(expected_revision)?;
        let create = BindingCreate {
            intent_id: derive(&operation, Step::Create),
            managed_session_id: id.clone(),
            expected_session_revision: expected_revision,
            workspace_root: workspace.clone(),
        };
        self.call(|client| {
            self.settle(client, &id);
            let status = client
                .binding_status(id.clone())
                .map_err(SessionError::of)?;
            let stage = match status.state {
                // The provider may hold a thread for it. Nothing is sent, least of all
                // another binding request.
                BindingState::Indeterminate => {
                    return Err(SessionError::of(ControlError::BindingIndeterminate));
                }
                BindingState::Creating => ActivationStage::Creating,
                BindingState::Unbound => {
                    let want = Want::new(mode, &workspace);
                    match runtime(client, &operation, &want, turn_active)? {
                        Some(waiting) => waiting,
                        None => {
                            self.gated(
                                client,
                                &id,
                                expected_revision,
                                Grant {
                                    scopes: &[ExecutionScope::SessionCreate],
                                    ttl_seconds: CREATE_LEASE_SECONDS,
                                    hold: false,
                                },
                                |client| client.binding_create(create.clone()),
                            )
                            .map_err(SessionError::binding)?;
                            ActivationStage::Bound
                        }
                    }
                }
                BindingState::BoundDetached | BindingState::BoundAttached => {
                    // Whose binding is it? The authority answers this action's own
                    // binding request with its receipt only if this action made it. For
                    // any other binding it refuses, and nothing can be created: the
                    // session is bound.
                    match client.binding_create(create.clone()) {
                        Ok(_) => (),
                        Err(error) if SessionError::of(error).retry => {
                            return Err(SessionError::of(error));
                        }
                        Err(_) => {
                            return Err(SessionError::of(ControlError::BindingAlreadyExists));
                        }
                    }
                    if status.state == BindingState::BoundAttached {
                        attached(
                            status.attachment_profile.map(AgentMode::of_attachment),
                            mode,
                        )?
                    } else {
                        // The workspace the authority bound, as it holds it.
                        let bound = status
                            .binding
                            .as_ref()
                            .map_or(workspace.as_str(), |b| b.workspace_root.as_str());
                        let want = Want::new(mode, bound);
                        match runtime(client, &operation, &want, turn_active)? {
                            Some(waiting) => waiting,
                            None => {
                                self.attach(client, &operation, &id, expected_revision, mode)?
                            }
                        }
                    }
                }
            };
            Ok(ActivationProgress { stage })
        })
    }

    /// Takes the next step towards a bound session attached in `mode`: the runtime in
    /// that mode, for the workspace the authority holds for the session, and the session
    /// attached to it. The caller names no workspace.
    ///
    /// When the runtime runs in another mode, or for another workspace, it is asked to
    /// stop first. That detaches every session attached to it, and it is not asked while
    /// a managed turn is running.
    pub fn enter(
        &self,
        operation: WireUuid,
        id: ManagedSessionId,
        expected_revision: u64,
        mode: AgentMode,
        turn_active: TurnGuard<'_>,
    ) -> Result<ActivationProgress, SessionError> {
        session_revision(expected_revision)?;
        self.call(|client| {
            self.settle(client, &id);
            let status = client
                .binding_status(id.clone())
                .map_err(SessionError::of)?;
            let stage = match (status.state, status.binding.as_ref()) {
                (BindingState::Indeterminate, _) => {
                    return Err(SessionError::of(ControlError::BindingIndeterminate));
                }
                (BindingState::BoundAttached, Some(_)) => attached(
                    status.attachment_profile.map(AgentMode::of_attachment),
                    mode,
                )?,
                (BindingState::BoundDetached, Some(binding)) => {
                    let want = Want::new(mode, &binding.workspace_root);
                    match runtime(client, &operation, &want, turn_active)? {
                        Some(waiting) => waiting,
                        None => self.attach(client, &operation, &id, expected_revision, mode)?,
                    }
                }
                // No workspace is bound: there is none to run the mode for.
                _ => return Err(SessionError::new(SessionFailure::NotBound)),
            };
            Ok(ActivationProgress { stage })
        })
    }

    /// Asks the authority to attach the session's binding to the runtime that is ready,
    /// and reads what the session is afterwards.
    fn attach(
        &self,
        client: &mut ControlClient,
        operation: &WireUuid,
        id: &ManagedSessionId,
        expected_revision: u64,
        mode: AgentMode,
    ) -> Result<ActivationStage, SessionError> {
        let resume = BindingResume {
            intent_id: derive(operation, Step::Attach),
            managed_session_id: id.clone(),
            expected_session_revision: expected_revision,
        };
        self.gated(
            client,
            id,
            expected_revision,
            Grant {
                scopes: &[ExecutionScope::SessionResume],
                ttl_seconds: RESUME_LEASE_SECONDS,
                hold: false,
            },
            |client| client.binding_resume(resume.clone()),
        )?;
        // What the session is now is read, not taken from the receipt: a receipt of an
        // earlier attachment says nothing about this runtime.
        let now = client
            .binding_status(id.clone())
            .map_err(SessionError::of)?;
        match now.state {
            BindingState::BoundAttached => {
                attached(now.attachment_profile.map(AgentMode::of_attachment), mode)
            }
            _ => Err(SessionError::new(SessionFailure::RuntimeUnavailable)),
        }
    }

    /// Asks the authority to give up on a binding attempt whose outcome it could not
    /// confirm. The attempt is the one the authority names; the caller names none. A
    /// thread the provider may have created for it is not removed, and the authority
    /// records that it was abandoned.
    pub fn abandon(
        &self,
        intent: WireUuid,
        id: ManagedSessionId,
    ) -> Result<SessionReceipt, SessionError> {
        self.call(|client| {
            let status = client
                .binding_status(id.clone())
                .map_err(SessionError::of)?;
            let (BindingState::Indeterminate, Some(operation_id)) =
                (status.state, status.operation_id)
            else {
                return Err(SessionError::new(SessionFailure::NotAllowed));
            };
            client
                .binding_abandon(BindingAbandon {
                    intent_id: intent,
                    managed_session_id: id.clone(),
                    operation_id,
                })
                .map(|_| SessionReceipt { id: id.clone() })
                .map_err(SessionError::of)
        })
    }
}

/// An attached session is ready in the mode asked, or it is attached in the other one:
/// its conversation exists only there, and it is not moved.
fn attached(is: Option<AgentMode>, asked: AgentMode) -> Result<ActivationStage, SessionError> {
    match is {
        Some(mode) if mode != asked => Err(SessionError::new(SessionFailure::ModeFixed)),
        _ => Ok(ActivationStage::Ready),
    }
}

/// The managed runtime, as a step needs it: `None` once it is ready in the mode and for
/// the workspace wanted, otherwise the stage to wait in, after asking the authority for
/// the one thing that is missing.
///
/// The desktop starts and stops nothing: both are requests, and the authority launches,
/// isolates, supervises and attests the runtime or refuses. A runtime already on its way
/// is never asked again.
fn runtime(
    client: &mut ControlClient,
    operation: &WireUuid,
    want: &Want<'_>,
    turn_active: TurnGuard<'_>,
) -> Result<Option<ActivationStage>, SessionError> {
    let status = client.runtime_status().map_err(SessionError::of)?;
    let wanted = status.desired_state == DesiredState::Running;
    match status.actual_state {
        ActualState::Ready if wanted => {
            if serves(client, operation, &status, want)? {
                return Ok(None);
            }
            // It serves another mode or another workspace, and it is the only one there
            // is. Stopping it ends whatever runs on it, so nothing may be running.
            if turn_active(client)? {
                return Err(SessionError::new(SessionFailure::TurnActive));
            }
            return stop(client, operation, &status).map(Some);
        }
        // Which generation is coming up is known once it is ready.
        ActualState::Starting | ActualState::Initializing if wanted => {
            return Ok(Some(ActivationStage::RuntimeStarting));
        }
        // On its way down: what to ask for is known once it has settled.
        ActualState::Stopping => return Ok(Some(ActivationStage::RuntimeStopping)),
        _ => (),
    }
    // A generation the authority contained for safety is not restarted from here.
    if status.actual_state == ActualState::Failed
        && matches!(
            status.diagnostic,
            Some(
                RuntimeDiagnostic::InferenceIsolationViolation | RuntimeDiagnostic::SafetyUnproven
            )
        )
    {
        return Err(SessionError::new(SessionFailure::RuntimeUnavailable));
    }
    match client.runtime_start(want.start(operation)) {
        // The receipt of a start this action already asked for: it changed nothing now,
        // and the runtime it started is not up.
        Ok(record) if record.revision <= status.revision => {
            Err(SessionError::new(SessionFailure::RuntimeUnavailable))
        }
        Ok(_) => Ok(Some(ActivationStage::RuntimeStarting)),
        Err(error) => Err(refused(error)),
    }
}

/// Whether the generation that is ready is the one wanted.
fn serves(
    client: &mut ControlClient,
    operation: &WireUuid,
    status: &RuntimeStatus,
    want: &Want<'_>,
) -> Result<bool, SessionError> {
    let local = status.runtime_safety_profile == Some(RuntimeSafetyProfile::CodexLocalToolsV1);
    match want.mode {
        AgentMode::Inference => Ok(!local),
        AgentMode::LocalTools if !local => Ok(false),
        AgentMode::LocalTools if status.workspace_root.as_deref() == want.workspace => Ok(true),
        // Another spelling of the same folder, or another folder: only the authority
        // can tell. It admits a start on a live generation exactly when the start names
        // that generation's own profile and workspace, and launches nothing for it.
        AgentMode::LocalTools => match client.runtime_start(want.start(operation)) {
            Ok(_) => Ok(true),
            Err(ControlError::InvalidTransition) => Ok(false),
            Err(error) => Err(refused(error)),
        },
    }
}

/// Asks the authority to stop the runtime, at no revision of the desktop's choosing.
fn stop(
    client: &mut ControlClient,
    operation: &WireUuid,
    status: &RuntimeStatus,
) -> Result<ActivationStage, SessionError> {
    let target = RuntimeTarget::codex();
    let request = RuntimeMutation {
        intent_id: derive(operation, Step::Stop),
        adapter_id: target.adapter_id,
        runtime_kind: target.runtime_kind,
        expected_revision: None,
        safety_profile: None,
        workspace_root: None,
    };
    match client.runtime_stop(request) {
        // The receipt of the stop this action already asked for: the runtime that is up
        // now came after it and is somebody else's. It is not stopped a second time.
        Ok(record) if record.revision <= status.revision => {
            Err(SessionError::new(SessionFailure::RuntimeUnavailable))
        }
        Ok(_) => Ok(ActivationStage::RuntimeStopping),
        Err(error) => Err(refused(error)),
    }
}

/// A runtime request the authority did not take, in product terms.
fn refused(error: ControlError) -> SessionError {
    let named = SessionError::of(error);
    match (error, named.code) {
        // The folder itself is what the authority did not accept.
        (ControlError::WorkspaceInvalid, _) => named.binding(),
        (
            _,
            SessionFailure::Unavailable
            | SessionFailure::Busy
            | SessionFailure::Incompatible
            | SessionFailure::Conflict,
        ) => named,
        _ => SessionError {
            cause: Some(error),
            ..SessionError::new(SessionFailure::StartRefused)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(text: &str) -> WireUuid {
        WireUuid::try_from(text.to_owned()).unwrap()
    }

    #[test]
    fn every_step_of_an_action_has_its_own_stable_identity() {
        let action = uuid("11111111-1111-4111-8111-111111111111");
        let steps = [Step::Stop, Step::Start, Step::Create, Step::Attach];
        let derived: Vec<WireUuid> = steps.iter().map(|s| derive(&action, *s)).collect();
        // The same step is the same identity, however often it is asked for.
        for (step, id) in steps.iter().zip(&derived) {
            assert_eq!(&derive(&action, *step), id);
        }
        // No two operations share one, and none borrows the action's own.
        for (i, id) in derived.iter().enumerate() {
            assert_ne!(id, &action);
            assert!(derived.iter().skip(i + 1).all(|other| other != id));
            assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
        }
        // The case of the action's identity is not part of it.
        let upper = uuid("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA");
        let lower = uuid("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
        assert_eq!(derive(&upper, Step::Create), derive(&lower, Step::Create));
        // Another action is other identities.
        assert_ne!(derive(&lower, Step::Create), derived[2]);
    }
}
