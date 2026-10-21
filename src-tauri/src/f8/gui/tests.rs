//! The window's service against a real prepared application: every call crosses the
//! protocol-2 transport, the authority, the engine and an owned fixture process.
use super::{
    harness::{Harness, eventually},
    *,
};
use crate::{
    contracts::{
        control::{
            ActualState, ApprovalDecision, ApprovalState, BindingState, ExecutionScope, TurnState,
        },
        identities::ManagedSessionId,
    },
    control::store::uuid,
    sessions::AgentMode,
};

const ALL: [ExecutionScope; 5] = [
    ExecutionScope::SessionCreate,
    ExecutionScope::SessionResume,
    ExecutionScope::TurnStart,
    ExecutionScope::CommandExecute,
    ExecutionScope::FileChange,
];

fn ready(h: &Harness, id: &ManagedSessionId) -> SessionDetail {
    eventually(|| {
        let detail = h.gui.session(id.clone()).ok()?;
        (detail.session.runtime.actual == ActualState::Ready
            && detail.session.runtime.instance.is_some())
        .then_some(detail)
    })
}

#[test]
fn an_inactive_service_answers_inactive_and_sends_nothing() {
    let gui = F8Gui::inactive();
    assert!(!gui.is_active());
    let state = gui.state();
    assert!(!state.active && state.authority.is_none());
    assert_eq!(gui.sessions(None).unwrap_err().code, F8Failure::Inactive);
    assert_eq!(gui.prepare(uuid()).unwrap_err().code, F8Failure::Inactive);
    assert_eq!(gui.audit_latest().unwrap_err().code, F8Failure::Inactive);
    assert_eq!(gui.stats().connections, 0);
}

#[test]
fn the_window_drives_a_session_from_prepare_to_a_decided_approval_and_a_stopped_runtime() {
    let h = Harness::start(true, 1);
    let state = h.gui.state();
    let authority = state.authority.expect("a greeted authority");
    assert_eq!(
        (authority.protocol_version, authority.storage_schema_version),
        (2, 8)
    );

    let session = h.gui.prepare(uuid()).unwrap().id;
    let listed = h.gui.sessions(None).unwrap();
    let row = &listed.sessions[0];
    assert_eq!(row.id, session);
    assert_eq!(
        (row.runtime.actual, row.runtime.generation),
        (ActualState::Stopped, 0)
    );

    let started = h
        .gui
        .runtime_start(
            uuid(),
            session.clone(),
            row.runtime.revision,
            row.runtime.generation,
            AgentMode::LocalTools,
            Some(h.workspace.to_str().unwrap().to_owned()),
        )
        .unwrap();
    assert_eq!(started.generation, 1);
    let detail = ready(&h, &session);
    let fence = detail.session.runtime.fence.clone();
    assert_eq!(fence.generation, 1);

    // A lease for the five scopes, then the binding the authority confirms asynchronously.
    let lease = h
        .gui
        .lease_grant(
            uuid(),
            fence.clone(),
            detail.session.revision,
            ALL.to_vec(),
            300,
        )
        .unwrap()
        .lease;
    let intent = uuid();
    eventually(|| {
        match h.gui.binding_create(
            intent.clone(),
            fence.clone(),
            detail.session.revision,
            h.workspace.to_str().unwrap().to_owned(),
        ) {
            Ok(_) => Some(()),
            Err(e) if e.code == F8Failure::Busy => None,
            Err(e) => panic!("{e:?}"),
        }
    });
    let bound = eventually(|| {
        let d = h.gui.session(session.clone()).ok()?;
        (d.binding.state == BindingState::BoundAttached).then_some(d)
    });
    assert_eq!(bound.lease.as_ref().map(|l| &l.id), Some(&lease.id));

    // A turn that proposes a command: the window sees the approval and decides it.
    let turn = h
        .gui
        .turn_start(
            uuid(),
            fence.clone(),
            bound.session.revision,
            "approve".into(),
        )
        .unwrap()
        .id;
    let reading = eventually(|| {
        let r = h.gui.turn(turn.clone(), None, true).ok()?;
        r.approval.is_some().then_some(r)
    });
    assert!(
        reading
            .events
            .iter()
            .any(|e| e.text.as_deref() == Some("fixture stream"))
    );
    let notice = reading.approval.unwrap();
    let detail = h.gui.approval(notice.id.clone(), None, None).unwrap();
    assert!(detail.approval.decidable);
    assert_eq!(
        detail.command.as_ref().map(|c| c.command.as_str()),
        Some("printf fixture")
    );
    let decided = h
        .gui
        .approval_respond(
            uuid(),
            fence.clone(),
            notice.id.clone(),
            detail.approval.revision,
            ApprovalDecision::Accept,
        )
        .unwrap();
    assert_eq!(decided.state, ApprovalState::Accepted);

    // The turn is cancelled and ends interrupted; the runtime is stopped and reaped.
    let current = h.gui.turn(turn.clone(), None, false).unwrap();
    h.gui
        .turn_cancel(
            uuid(),
            fence.clone(),
            turn.clone(),
            current.turn.turn.revision,
        )
        .unwrap();
    let ended = eventually(|| {
        let r = h.gui.turn(turn.clone(), None, false).ok()?;
        r.turn.turn.state.terminal().then_some(r)
    });
    assert_eq!(ended.turn.turn.state, TurnState::Interrupted);
    let after = h.gui.session(session.clone()).unwrap();
    h.gui
        .runtime_stop(
            uuid(),
            after.session.runtime.fence.clone(),
            after.session.runtime.revision,
        )
        .unwrap();
    eventually(|| {
        let d = h.gui.session(session.clone()).ok()?;
        (d.session.runtime.actual == ActualState::Stopped).then_some(())
    });
    let audit = h.gui.audit_latest().unwrap();
    assert!(!audit.page.events.is_empty());
}
