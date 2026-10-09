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
//! unbound,  runtime not up   -> runtime/start                  -> RuntimeStarting
//! unbound,  runtime starting -> nothing                        -> RuntimeStarting
//! unbound,  runtime ready    -> managedSession/runtime/create  -> Bound
//! creating                   -> nothing                        -> Creating
//! bound, detached            -> managedSession/runtime/resume  -> Ready
//! bound, attached            -> nothing                        -> Ready
//! indeterminate              -> nothing                        -> uncertain (an error)
//! ```
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
    view::{ActivationProgress, ActivationStage, SessionError, SessionFailure, SessionReceipt},
};
use crate::{
    contracts::{
        WireUuid,
        control::{
            ActualState, BindingAbandon, BindingCreate, BindingResume, BindingState, ControlError,
            DesiredState, ExecutionScope, RuntimeDiagnostic, RuntimeMutation, RuntimeTarget,
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

/// The authority mutations one activation can lead to.
#[derive(Clone, Copy)]
enum Step {
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

impl SessionService {
    /// Takes the next step towards a session bound to `workspace` and attached.
    ///
    /// `operation` names the user's action and `expected_revision` is the session
    /// revision the caller saw. `workspace` is what the user chose: whether it may be a
    /// workspace is the authority's to say.
    pub fn activate(
        &self,
        operation: WireUuid,
        id: ManagedSessionId,
        expected_revision: u64,
        workspace: String,
    ) -> Result<ActivationProgress, SessionError> {
        session_revision(expected_revision)?;
        let create = BindingCreate {
            intent_id: derive(&operation, Step::Create),
            managed_session_id: id.clone(),
            expected_session_revision: expected_revision,
            workspace_root: workspace,
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
                BindingState::Unbound => match runtime(client, &operation)? {
                    Some(waiting) => waiting,
                    None => {
                        self.gated(
                            client,
                            &id,
                            expected_revision,
                            Grant {
                                scope: ExecutionScope::SessionCreate,
                                ttl_seconds: CREATE_LEASE_SECONDS,
                                hold: false,
                            },
                            |client| client.binding_create(create.clone()),
                        )
                        .map_err(SessionError::binding)?;
                        ActivationStage::Bound
                    }
                },
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
                        ActivationStage::Ready
                    } else if let Some(waiting) = runtime(client, &operation)? {
                        waiting
                    } else {
                        let resume = BindingResume {
                            intent_id: derive(&operation, Step::Attach),
                            managed_session_id: id.clone(),
                            expected_session_revision: expected_revision,
                        };
                        self.gated(
                            client,
                            &id,
                            expected_revision,
                            Grant {
                                scope: ExecutionScope::SessionResume,
                                ttl_seconds: RESUME_LEASE_SECONDS,
                                hold: false,
                            },
                            |client| client.binding_resume(resume.clone()),
                        )?;
                        // What the session is now is read, not taken from the receipt: a
                        // receipt of an earlier attachment says nothing about this runtime.
                        match client
                            .binding_status(id.clone())
                            .map_err(SessionError::of)?
                            .state
                        {
                            BindingState::BoundAttached => ActivationStage::Ready,
                            _ => {
                                return Err(SessionError::new(SessionFailure::RuntimeUnavailable));
                            }
                        }
                    }
                }
            };
            Ok(ActivationProgress { stage })
        })
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

/// The managed runtime, as an activation needs it: `None` once it is ready, otherwise
/// the stage to wait in, after asking the authority to start it if nothing is on its way.
///
/// The desktop starts nothing: `runtime/start` is a request, and the authority launches,
/// supervises and attests the runtime or refuses. A runtime already starting is never
/// asked to start again.
fn runtime(
    client: &mut ControlClient,
    operation: &WireUuid,
) -> Result<Option<ActivationStage>, SessionError> {
    let status = client.runtime_status().map_err(SessionError::of)?;
    let wanted = status.desired_state == DesiredState::Running;
    match status.actual_state {
        ActualState::Ready if wanted => return Ok(None),
        ActualState::Starting | ActualState::Initializing if wanted => {
            return Ok(Some(ActivationStage::RuntimeStarting));
        }
        // On its way down: what to ask for is known once it has settled.
        ActualState::Stopping => return Ok(Some(ActivationStage::RuntimeStarting)),
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
    let target = RuntimeTarget::codex();
    // The default profile: no workspace is named and none is isolated for.
    let start = RuntimeMutation {
        intent_id: derive(operation, Step::Start),
        adapter_id: target.adapter_id,
        runtime_kind: target.runtime_kind,
        expected_revision: None,
        safety_profile: None,
        workspace_root: None,
    };
    match client.runtime_start(start) {
        // The receipt of a start this action already asked for: it changed nothing now,
        // and the runtime it started is not up.
        Ok(record) if record.revision <= status.revision => {
            Err(SessionError::new(SessionFailure::RuntimeUnavailable))
        }
        Ok(_) => Ok(Some(ActivationStage::RuntimeStarting)),
        Err(error) => {
            let named = SessionError::of(error);
            Err(match named.code {
                SessionFailure::Unavailable
                | SessionFailure::Busy
                | SessionFailure::Incompatible
                | SessionFailure::Conflict => named,
                _ => SessionError {
                    cause: Some(error),
                    ..SessionError::new(SessionFailure::StartRefused)
                },
            })
        }
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
        let steps = [Step::Start, Step::Create, Step::Attach];
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
        assert_ne!(derive(&lower, Step::Create), derived[1]);
    }
}
