//! The window's service against a real prepared application: every call crosses the
//! protocol-2 transport, the authority, the engine and an owned fixture process.
use super::{
    harness::{Harness, NativeHarness, eventually},
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
    f8::gui::ApprovalDetail,
    sessions::AgentMode,
};

#[test]
fn a_mode_the_authority_does_not_serve_is_not_an_invalid_request() {
    use crate::contracts::control::ControlError as E;
    assert_eq!(F8Error::of(E::Unsupported).code, F8Failure::Unsupported);
    assert_eq!(F8Error::of(E::InvalidRequest).code, F8Failure::Invalid);
    assert!(!F8Error::of(E::Unsupported).retry);
}

#[test]
fn a_second_runtime_at_the_capacity_of_one_is_told_so_and_is_not_offered_a_retry() {
    let h = Harness::start(true, 1);
    let first = h.gui.prepare(uuid()).unwrap().id;
    let second = h.gui.prepare(uuid()).unwrap().id;
    let workspace = || Some(h.workspace.to_str().unwrap().to_owned());
    let rows = h.gui.sessions(None).unwrap();
    let row = |id: &ManagedSessionId| rows.sessions.iter().find(|r| &r.id == id).unwrap().clone();
    let (a, b) = (row(&first), row(&second));
    h.gui
        .runtime_start(
            uuid(),
            first.clone(),
            a.runtime.revision,
            a.runtime.generation,
            AgentMode::LocalTools,
            workspace(),
        )
        .unwrap();
    let up = ready(&h.gui, &first);

    // The authority's limit holds: the second session is refused, and the refusal says why.
    let refused = h
        .gui
        .runtime_start(
            uuid(),
            second.clone(),
            b.runtime.revision,
            b.runtime.generation,
            AgentMode::LocalTools,
            workspace(),
        )
        .unwrap_err();
    assert_eq!(refused.code, F8Failure::Capacity);
    assert!(!refused.retry);
    assert_ne!(refused.code, F8Failure::Busy);

    // Nothing was admitted past the limit, and it is the first runtime that frees it.
    let second_now = h.gui.session(second.clone()).unwrap();
    assert_eq!(second_now.session.runtime.actual, ActualState::Stopped);
    h.gui
        .runtime_stop(
            uuid(),
            up.session.runtime.fence.clone(),
            up.session.runtime.revision,
        )
        .unwrap();
    eventually(|| {
        (h.gui.session(first.clone()).ok()?.session.runtime.actual == ActualState::Stopped)
            .then_some(())
    });
    let again = h.gui.session(second.clone()).unwrap();
    eventually(|| {
        match h.gui.runtime_start(
            uuid(),
            second.clone(),
            again.session.runtime.revision,
            again.session.runtime.generation,
            AgentMode::LocalTools,
            workspace(),
        ) {
            Ok(_) => Some(()),
            Err(e) if matches!(e.code, F8Failure::Busy | F8Failure::Capacity) => None,
            Err(e) => panic!("{e:?}"),
        }
    });
    ready(&h.gui, &second);
}

#[test]
fn a_refusal_for_capacity_that_no_other_runtime_explains_stays_a_busy_one() {
    use crate::contracts::control::ControlError as E;
    // The mapping alone: only the service, which can see another live runtime, says capacity.
    assert_eq!(F8Error::of(E::CapacityExceeded).code, F8Failure::Busy);
    assert!(F8Error::of(E::CapacityExceeded).retry);
    assert_eq!(
        F8Error::of(E::RuntimeOperationInProgress).code,
        F8Failure::Busy
    );
}

const ALL: [ExecutionScope; 5] = [
    ExecutionScope::SessionCreate,
    ExecutionScope::SessionResume,
    ExecutionScope::TurnStart,
    ExecutionScope::CommandExecute,
    ExecutionScope::FileChange,
];

fn ready(gui: &F8Gui, id: &ManagedSessionId) -> SessionDetail {
    eventually(|| {
        let detail = gui.session(id.clone()).ok()?;
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
    let detail = ready(&h.gui, &session);
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

/// The shape of a value: its keys and the kinds of its leaves, not the leaves.
fn shape(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value as V;
    match value {
        V::Null => V::Null,
        V::Bool(_) => "bool".into(),
        V::Number(_) => "number".into(),
        V::String(_) => "string".into(),
        V::Array(items) => V::Array(items.first().map(shape).into_iter().collect()),
        V::Object(map) => V::Object(map.iter().map(|(k, v)| (k.clone(), shape(v))).collect()),
    }
}
/// Two shapes agree when every key is on both sides and every leaf has the same kind. A null
/// stands for "not present in this reading" and agrees with anything; an empty list too.
fn agree(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> Result<(), String> {
    use serde_json::Value as V;
    match (a, b) {
        (V::Null, _) | (_, V::Null) => Ok(()),
        (V::Array(x), V::Array(y)) => match (x.first(), y.first()) {
            (Some(x), Some(y)) => agree(x, y, &format!("{path}[]")),
            _ => Ok(()),
        },
        (V::Object(x), V::Object(y)) => {
            let keys = |m: &serde_json::Map<String, V>| m.keys().cloned().collect::<Vec<_>>();
            if keys(x) != keys(y) {
                return Err(format!("{path}: keys {:?} vs {:?}", keys(x), keys(y)));
            }
            x.iter()
                .try_for_each(|(k, v)| agree(v, &y[k], &format!("{path}.{k}")))
        }
        (x, y) if std::mem::discriminant(x) == std::mem::discriminant(y) => Ok(()),
        (x, y) => Err(format!("{path}: {x} vs {y}")),
    }
}

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/fixtures/f8-gui-v1.json"
);

#[test]
fn every_payload_the_window_parses_keeps_the_shape_of_the_committed_fixture() {
    use serde_json::{Value, json, to_value};
    let h = NativeHarness::start("approval_command");
    let mut got = serde_json::Map::new();
    let mut put = |name: &str, value: Value| {
        got.insert(name.to_owned(), value);
    };
    put(
        "state_inactive",
        to_value(F8Gui::inactive().state()).unwrap(),
    );
    put("state", to_value(h.gui.state()).unwrap());
    let prepared = h.gui.prepare(uuid()).unwrap();
    put("session_receipt", to_value(&prepared).unwrap());
    let session = prepared.id;
    put("sessions", to_value(h.gui.sessions(None).unwrap()).unwrap());
    let stopped = h.gui.session(session.clone()).unwrap();
    put("session_stopped", to_value(&stopped).unwrap());
    let started = h
        .gui
        .runtime_start(
            uuid(),
            session.clone(),
            stopped.session.runtime.revision,
            stopped.session.runtime.generation,
            AgentMode::LocalTools,
            Some(h.workspace.clone()),
        )
        .unwrap();
    put("runtime_receipt", to_value(started).unwrap());
    let detail = ready(&h.gui, &session);
    put("session_ready", to_value(&detail).unwrap());
    let fence = detail.session.runtime.fence.clone();
    put(
        "models",
        to_value(h.gui.models(fence.clone()).unwrap()).unwrap(),
    );
    let lease = h
        .gui
        .lease_grant(
            uuid(),
            fence.clone(),
            detail.session.revision,
            ALL.to_vec(),
            300,
        )
        .unwrap();
    put("lease_receipt", to_value(&lease).unwrap());
    let intent = uuid();
    let bound = eventually(|| {
        match h.gui.binding_create(
            intent.clone(),
            fence.clone(),
            detail.session.revision,
            h.workspace.clone(),
        ) {
            Ok(receipt) => Some(receipt),
            Err(e) if e.code == F8Failure::Busy => None,
            Err(e) => panic!("{e:?}"),
        }
    });
    put("binding_receipt", to_value(bound).unwrap());
    let attached = eventually(|| {
        let d = h.gui.session(session.clone()).ok()?;
        (d.binding.state == BindingState::BoundAttached).then_some(d)
    });
    put("session_attached", to_value(&attached).unwrap());
    // The runtime's own preflight may still be running: the same intent is asked again.
    let intent = uuid();
    let turn = eventually(|| {
        match h.gui.turn_start(
            intent.clone(),
            fence.clone(),
            attached.session.revision,
            "approve".into(),
        ) {
            Ok(receipt) => Some(receipt),
            Err(e) if e.code == F8Failure::Busy => None,
            Err(e) => panic!("{e:?}"),
        }
    });
    put("turn_receipt", to_value(&turn).unwrap());
    let reading = eventually(|| {
        let r = h.gui.turn(turn.id.clone(), None, true).ok()?;
        r.approval.is_some().then_some(r)
    });
    put("turn_reading", to_value(&reading).unwrap());
    let notice = reading.approval.unwrap();
    let approval = h.gui.approval(notice.id.clone(), None, None).unwrap();
    put("approval", to_value(&approval).unwrap());
    let decision = uuid();
    let decided = eventually(|| {
        match h.gui.approval_respond(
            decision.clone(),
            fence.clone(),
            notice.id.clone(),
            approval.approval.revision,
            ApprovalDecision::Accept,
        ) {
            Ok(receipt) => Some(receipt),
            Err(e) if e.code == F8Failure::Busy => None,
            Err(e) => panic!("{e:?}"),
        }
    });
    put("decision_receipt", to_value(decided).unwrap());
    let audit = h.gui.audit_latest().unwrap();
    put("audit", to_value(&audit).unwrap());
    let stream = audit.page.stream.clone();
    let head = audit.page.head;
    h.gui.prepare(uuid()).unwrap();
    put(
        "watch",
        to_value(h.gui.audit_wait(stream, head, 1_000).unwrap()).unwrap(),
    );
    put(
        "failure",
        to_value(
            h.gui
                .prepare(uuid())
                .and(h.gui.close(
                    uuid(),
                    FenceView {
                        session: session.clone(),
                        generation: 99,
                        instance: None,
                    },
                    1,
                ))
                .unwrap_err(),
        )
        .unwrap(),
    );
    let got = Value::Object(got);
    if std::env::var_os("WYSS_F8_WRITE_FIXTURES").is_some() {
        std::fs::write(FIXTURE, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
    }
    let committed: Value = serde_json::from_str(
        &std::fs::read_to_string(FIXTURE).expect("tests/fixtures/f8-gui-v1.json is committed"),
    )
    .unwrap();
    for (name, value) in got.as_object().unwrap() {
        let reference = committed
            .get(name)
            .unwrap_or_else(|| panic!("fixture {name}"));
        agree(&shape(value), &shape(reference), name)
            .unwrap_or_else(|why| panic!("the window's payload {name} drifted: {why}"));
    }
    assert_eq!(
        json!(committed.as_object().unwrap().len()),
        json!(got.as_object().unwrap().len())
    );
}

/// Retries an intent while the authority is still working on the same one, as the window does.
#[track_caller]
pub(crate) fn settled<T>(mut ask: impl FnMut() -> Result<T, F8Error>) -> T {
    eventually(|| match ask() {
        Ok(value) => Some(value),
        Err(e) if e.code == F8Failure::Busy => None,
        Err(e) => panic!("{e:?}"),
    })
}

/// A session of the native fixture with a ready runtime, an active lease and an attached
/// binding: the state in which the window can start a turn.
pub(crate) fn native_session(h: &NativeHarness) -> (ManagedSessionId, FenceView, SessionDetail) {
    let session = h.gui.prepare(uuid()).unwrap().id;
    let row = h.gui.session(session.clone()).unwrap().session;
    h.gui
        .runtime_start(
            uuid(),
            session.clone(),
            row.runtime.revision,
            row.runtime.generation,
            AgentMode::LocalTools,
            Some(h.workspace.clone()),
        )
        .unwrap();
    let ready = ready(&h.gui, &session);
    let fence = ready.session.runtime.fence.clone();
    h.gui
        .lease_grant(
            uuid(),
            fence.clone(),
            ready.session.revision,
            ALL.to_vec(),
            300,
        )
        .unwrap();
    let intent = uuid();
    settled(|| {
        h.gui.binding_create(
            intent.clone(),
            fence.clone(),
            ready.session.revision,
            h.workspace.clone(),
        )
    });
    let attached = eventually(|| {
        let d = h.gui.session(session.clone()).ok()?;
        (d.binding.state == BindingState::BoundAttached).then_some(d)
    });
    (session, fence, attached)
}

/// A turn that has proposed an action and waits for the window to decide it.
pub(crate) fn native_turn(
    h: &NativeHarness,
    fence: &FenceView,
    detail: &SessionDetail,
) -> (crate::contracts::identities::ManagedTurnId, ApprovalDetail) {
    let intent = uuid();
    let turn = settled(|| {
        h.gui.turn_start(
            intent.clone(),
            fence.clone(),
            detail.session.revision,
            "work".into(),
        )
    })
    .id;
    let notice = eventually(|| h.gui.turn(turn.clone(), None, true).ok()?.approval);
    (turn, h.gui.approval(notice.id, None, None).unwrap())
}

#[test]
fn a_native_runtime_reports_what_it_was_asked_to_be_and_why_it_ended_when_it_did_not_mean_to() {
    use crate::contracts::control::{RuntimeDiagnostic, RuntimeSafetyProfile};
    let h = NativeHarness::start("approval_command_crash_after");
    let (session, fence, attached) = native_session(&h);
    let runtime = &attached.session.runtime;
    assert_eq!(
        runtime.profile,
        Some(RuntimeSafetyProfile::CodexLocalToolsV1)
    );
    assert!(
        runtime
            .workspace
            .as_deref()
            .is_some_and(|w| w.ends_with("/repo/work"))
    );
    assert!(runtime.model_count > 0);
    let (_, approval) = native_turn(&h, &fence, &attached);
    // Accepting makes the fixture runtime exit without a word: the authority did not mean it.
    let decision = uuid();
    settled(|| {
        h.gui.approval_respond(
            decision.clone(),
            fence.clone(),
            approval.approval.id.clone(),
            approval.approval.revision,
            ApprovalDecision::Accept,
        )
    });
    let failed = eventually(|| {
        let d = h.gui.session(session.clone()).ok()?;
        (d.session.runtime.actual == ActualState::Failed).then_some(d)
    });
    assert_eq!(
        failed.session.runtime.diagnostic,
        Some(RuntimeDiagnostic::ChildExited)
    );
    assert_eq!(failed.session.runtime.fence.instance, None);
    assert_eq!(failed.session.runtime.profile, None);
    // The fence the window held is stale now: nothing is decided against a launch that is gone.
    let stale = h
        .gui
        .turn_start(uuid(), fence, failed.session.revision, "again".into());
    assert!(matches!(
        stale.unwrap_err().code,
        F8Failure::Stale | F8Failure::PermissionDenied
    ));
}

#[test]
fn the_window_shows_and_decides_native_command_and_file_approvals_accept_decline_and_cancel() {
    use crate::contracts::control::{FileChangeKind, TurnState};
    for scenario in ["approval_command", "approval_file"] {
        for decision in [
            ApprovalDecision::Accept,
            ApprovalDecision::Decline,
            ApprovalDecision::Cancel,
        ] {
            let h = NativeHarness::start(scenario);
            let (_, fence, attached) = native_session(&h);
            let (turn, approval) = native_turn(&h, &fence, &attached);
            // What is shown to the person who decides is the whole action, as the authority
            // presents it: a command with its directory, or a file with a page of its diff.
            if scenario == "approval_command" {
                let command = approval.command.as_ref().unwrap();
                assert_eq!(command.command, "printf WYSS_FIXTURE_COMMAND");
                assert_eq!(command.cwd, h.workspace);
            } else {
                assert_eq!(approval.files, Some(1));
                let file = approval.file.as_ref().unwrap();
                assert!(file.path.ends_with("wyss-fixture-probe.txt"));
                assert_eq!(file.kind, FileChangeKind::Add);
                assert_eq!(file.chunk, "+WYSS_FIXTURE_PATCH\n");
                assert_eq!(file.next, None);
            }
            assert!(approval.approval.decidable);
            let intent = uuid();
            let ask = || {
                h.gui.approval_respond(
                    intent.clone(),
                    fence.clone(),
                    approval.approval.id.clone(),
                    approval.approval.revision,
                    decision,
                )
            };
            let receipt = settled(ask);
            let state = match decision {
                ApprovalDecision::Accept => ApprovalState::Accepted,
                ApprovalDecision::Decline => ApprovalState::Declined,
                ApprovalDecision::Cancel => ApprovalState::Cancelled,
            };
            assert_eq!(receipt.state, state, "{scenario} {decision:?}");
            // Asking again with the same identity is the same decision, answered the same.
            assert_eq!(settled(ask).state, state);
            let ended = eventually(|| {
                let r = h.gui.turn(turn.clone(), None, false).ok()?;
                r.turn.turn.state.terminal().then_some(r)
            });
            let expected = if decision == ApprovalDecision::Cancel {
                TurnState::Interrupted
            } else {
                TurnState::Completed
            };
            assert_eq!(ended.turn.turn.state, expected, "{scenario} {decision:?}");
            // The effect happened exactly when, and only when, it was accepted.
            let record = |name: &str| {
                std::fs::read_to_string(h.root().join(name))
                    .unwrap_or_default()
                    .lines()
                    .count()
            };
            assert_eq!(record("decisions"), 1, "{scenario} {decision:?}");
            let effects = record(if scenario == "approval_file" {
                "mutations"
            } else {
                "executions"
            });
            assert_eq!(effects, usize::from(decision == ApprovalDecision::Accept));
            let after = h
                .gui
                .approval(approval.approval.id.clone(), None, None)
                .unwrap();
            assert_eq!(after.approval.state, state);
            assert!(!after.approval.decidable);
        }
    }
}
