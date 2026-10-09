//! Approvals against the scripted authority of `tests.rs`: what the desktop reads of one,
//! what crosses to the window, and what a decision sends. How the real authority, the
//! real namespace and a provider behave is covered by `tests/approvals_process.rs`.
use super::*;
use crate::sessions::SessionError;

const COMMAND: &str = "/usr/bin/bash -lc 'git status'";
const CWD: &str = "/work/space";
const DIFF: &str = "+one line\n";
const FINGERPRINT: &str = "ab";
const ITEM: &str = "PROVIDER_ITEM_PRIVATE";
const RESPOND: &str = "managedTurn/approval/respond";
const APPROVAL: &str = "managedTurn/approval/status";

impl World {
    /// The authority opened an approval for `turn`: the durable event that names it and
    /// what a read of it answers. `files` is how many files a change set has; none makes
    /// it a command.
    fn ask(&mut self, turn: &ManagedTurnId, files: u16) -> ManagedApprovalId {
        let id = ManagedApprovalId::parse(uuid().as_str()).unwrap();
        let command = files == 0;
        let record: ManagedApprovalRecord = serde_json::from_value(json!({
            "managed_approval_id": id, "managed_turn_id": turn,
            "managed_session_id": self.session, "runtime_instance_id": uuid(),
            "kind": if command { "command_execution" } else { "file_change" },
            "provider_item_id": ITEM, "admitted_lease_id": uuid(),
            "admitted_lease_revision": 1, "state": "pending", "revision": 1,
            "action_fingerprint": FINGERPRINT.repeat(32),
            "structure": if command {
                json!({"command_bytes": COMMAND.len(), "cwd_bytes": CWD.len(),
                       "parsed_actions": 1, "files": 0, "diff_bytes": 0})
            } else {
                json!({"command_bytes": 0, "cwd_bytes": 0, "parsed_actions": 0,
                       "files": files, "diff_bytes": DIFF.len() * usize::from(files)})
            },
            "decision": null, "effect": "none",
            "created_at": "2026-10-08T00:00:00Z", "updated_at": "2026-10-08T00:00:00Z",
            "expires_at": "2026-10-08T00:05:00Z", "terminal_at": null,
            "terminal_reason": null, "created_sequence": (self.audit.len() + 1).to_string(),
        }))
        .unwrap();
        record.validate().unwrap();
        let status: ApprovalStatus = serde_json::from_value(json!({
            "approval": record, "acceptable": true,
            "command": command.then(|| json!({
                "command": COMMAND, "cwd": CWD, "parsed_actions": ["PARSED_PRIVATE"],
            })),
            "file_count": (!command).then_some(files),
            "file": (!command).then(|| json!({
                "index": 0, "path": "/work/space/file-0.txt", "kind": "update",
                "move_path": null, "diff_bytes": DIFF.len(), "offset": 0,
                "chunk": DIFF, "next_offset": null,
            })),
        }))
        .unwrap();
        status.validate().unwrap();
        self.named.insert(self.audit.len(), id.clone());
        self.audit.push((
            "authority.managed_approval.requested",
            self.session.clone(),
            turn.clone(),
        ));
        self.approvals.push(status);
        id
    }
    /// The authority ended an approval by itself.
    fn end(&mut self, id: &ManagedApprovalId, state: &str, reason: &str) {
        let status = self
            .approvals
            .iter_mut()
            .find(|a| &a.approval.managed_approval_id == id)
            .unwrap();
        let mut v = serde_json::to_value(&status.approval).unwrap();
        v["state"] = json!(state);
        v["revision"] = json!(status.approval.revision + 1);
        v["updated_at"] = json!("2026-10-08T00:05:00Z");
        v["terminal_at"] = v["updated_at"].clone();
        v["terminal_reason"] = json!(reason);
        status.approval = serde_json::from_value(v).unwrap();
        status.approval.validate().unwrap();
        (status.acceptable, status.command) = (false, None);
        (status.file_count, status.file) = (None, None);
    }
}

/// A session with a turn running in it, and the world to script approvals in.
fn running() -> (
    Peer,
    TurnService,
    Arc<Mutex<World>>,
    ManagedSessionId,
    ManagedTurnId,
) {
    let session = session_id();
    let mut world = World::new(&session);
    world.profile = "local_tools_v1";
    let (peer, _, turns, world) = authority(world);
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    world
        .lock()
        .unwrap()
        .advance(&started.id, TurnState::Running, None);
    (peer, turns, world, session, started.id)
}
fn refused<T>(result: Result<T, SessionError>) -> (SessionFailure, bool) {
    match result {
        Ok(_) => panic!("answered"),
        Err(error) => (error.code, error.retry),
    }
}

#[test]
fn the_lease_of_a_turn_carries_what_the_mode_of_its_session_can_lead_to() {
    for (profile, scopes) in [
        ("inference_only_v1", vec![ExecutionScope::TurnStart]),
        (
            "local_tools_v1",
            vec![
                ExecutionScope::TurnStart,
                ExecutionScope::CommandExecute,
                ExecutionScope::FileChange,
            ],
        ),
    ] {
        let session = session_id();
        let mut world = World::new(&session);
        world.profile = profile;
        let (peer, _, turns, _) = authority(world);
        turns
            .start(uuid(), session.clone(), 1, PROMPT.into())
            .unwrap();
        // The profile is read from the authority's record of the attachment, right
        // before the turn is asked for: nothing the caller sends chooses a scope.
        assert_eq!(peer.methods(), [BINDING, START, GATE, GRANT, START]);
        let Some(Operation::LeaseGrant(grant)) = peer
            .requests()
            .into_iter()
            .find(|op| matches!(op, Operation::LeaseGrant(_)))
        else {
            unreachable!()
        };
        assert_eq!(grant.scopes, scopes, "{profile}");
        assert_eq!(grant.ttl_seconds, TURN_LEASE_SECONDS);
        // Never a session scope with a turn.
        assert!(!grant.scopes.contains(&ExecutionScope::SessionCreate));
        assert!(!grant.scopes.contains(&ExecutionScope::SessionResume));
    }
}

#[test]
fn a_pending_approval_is_found_in_the_durable_stream_and_shown_with_its_action_only() {
    let (peer, turns, world, session, turn) = running();
    // Nothing was asked for yet.
    let empty = turns.approvals().unwrap();
    assert!(empty.pending.is_empty() && empty.recent.is_empty() && !empty.partial);
    assert_eq!(empty.head.unwrap().get(), 41);

    let id = world.lock().unwrap().ask(&turn, 0);
    let sent = peer.requests().len();
    let list = turns.approvals().unwrap();
    // Only what was appended is read from the stream, then the approval itself and the
    // workspace of its session.
    assert_eq!(since(&peer, sent), [AUDIT, APPROVAL, BINDING]);
    assert_eq!(
        serde_json::to_value(&list.pending).unwrap(),
        json!([{
            "approval": {
                "id": id, "session": session, "turn": turn,
                "kind": "command_execution", "state": "pending", "revision": 1,
                "requested_at": "2026-10-08T00:00:00Z",
                "expires_at": "2026-10-08T00:05:00Z",
                "ended_at": null, "reason": null, "decision": null, "decidable": true,
            },
            "workspace": "/work/space",
            // The command exactly as the authority presents it: what would run.
            "command": {"command": COMMAND, "cwd": CWD},
            "files": null, "file": null,
        }])
    );
    // Nothing a decision has no use for: no provider item, lease, runtime instance,
    // fingerprint, effect marker, counts or parsed labels.
    let wire = serde_json::to_string(&list).unwrap();
    let record = serde_json::to_value(&world.lock().unwrap().approvals[0].approval).unwrap();
    for private in [
        ITEM,
        "PARSED_PRIVATE",
        &FINGERPRINT.repeat(32),
        record["admitted_lease_id"].as_str().unwrap(),
        record["runtime_instance_id"].as_str().unwrap(),
        "fingerprint",
        "lease",
        "provider",
        "runtime_instance",
        "effect",
        "structure",
        "parsed",
    ] {
        assert!(!wire.contains(private), "{private} crossed to the window");
    }
    // The types that carry an action print nothing of it.
    assert_eq!(format!("{list:?}"), "ApprovalList(<sensitive>)");
    assert_eq!(
        format!("{:?}", list.pending[0]),
        "ApprovalDetail(<sensitive>)"
    );
    // The reading of the turn carries the handle, and no more of the action than before.
    let view = turns.read(turn.clone(), None, false).unwrap();
    assert_eq!(
        serde_json::to_value(&view.approval).unwrap(),
        json!({"id": id, "kind": "command_execution", "expires_at": "2026-10-08T00:05:00Z"})
    );
    assert!(!serde_json::to_string(&view).unwrap().contains("git status"));

    // The authority ends it: it is shown ended, without an action, and not as pending.
    world
        .lock()
        .unwrap()
        .end(&id, "expired", "approval_timeout");
    let list = turns.approvals().unwrap();
    assert!(list.pending.is_empty());
    assert_eq!(
        serde_json::to_value(&list.recent).unwrap(),
        json!([{
            "id": id, "session": session, "turn": turn,
            "kind": "command_execution", "state": "expired", "revision": 2,
            "requested_at": "2026-10-08T00:00:00Z", "expires_at": "2026-10-08T00:05:00Z",
            "ended_at": "2026-10-08T00:05:00Z", "reason": "approval_timeout",
            "decision": null, "decidable": false,
        }])
    );
    assert!(turns.read(turn, None, false).unwrap().approval.is_none());
}

#[test]
fn a_change_set_is_read_one_file_and_one_page_at_a_time() {
    let (peer, turns, world, _, turn) = running();
    let id = world.lock().unwrap().ask(&turn, 3);
    let detail = turns.approval(id.clone(), None, None).unwrap();
    assert_eq!(
        serde_json::to_value(&detail).unwrap()["file"],
        json!({
            "index": 0, "path": "/work/space/file-0.txt", "kind": "update",
            "move_path": null, "bytes": DIFF.len(), "offset": 0, "chunk": DIFF, "next": null,
        })
    );
    assert_eq!((detail.files, detail.command.is_none()), (Some(3), true));
    // Another file, another page: the selection goes to the authority as it was asked.
    let sent = peer.requests().len();
    let second = turns.approval(id.clone(), Some(2), Some(4096)).unwrap();
    let file = second.file.unwrap();
    assert_eq!((file.index, file.offset), (2, 4096));
    let Operation::ApprovalStatus(asked) = &peer.requests()[sent] else {
        unreachable!()
    };
    assert_eq!(
        (&asked.managed_approval_id, asked.file_index, asked.offset),
        (&id, Some(2), Some(4096))
    );
    // A file the change set does not have, or a file of a command: not a request to send
    // again, and nothing is shown.
    assert_eq!(
        refused(turns.approval(id.clone(), Some(3), Some(0))),
        (SessionFailure::Invalid, false)
    );
    let command = world.lock().unwrap().ask(&turn, 0);
    assert_eq!(
        refused(turns.approval(command, Some(0), Some(0))),
        (SessionFailure::Invalid, false)
    );
    // An approval the authority does not have.
    assert_eq!(
        refused(turns.approval(
            ManagedApprovalId::parse(uuid().as_str()).unwrap(),
            None,
            None
        )),
        (SessionFailure::NotFound, false)
    );
}

#[test]
fn a_decision_is_one_intent_for_one_approval_at_the_revision_that_was_shown() {
    let (peer, turns, world, _, turn) = running();
    let id = world.lock().unwrap().ask(&turn, 0);
    // A revision that cannot be one is refused before anything is asked.
    let sent = peer.requests().len();
    for revision in [0, u64::MAX] {
        assert_eq!(
            refused(turns.decide(uuid(), id.clone(), revision, Decision::Approve)),
            (SessionFailure::Invalid, false)
        );
    }
    assert!(since(&peer, sent).is_empty());
    // Not the revision the authority holds: refused, and nothing was decided.
    assert_eq!(
        refused(turns.decide(uuid(), id.clone(), 2, Decision::Approve)),
        (SessionFailure::Changed, false)
    );
    assert_eq!(
        world.lock().unwrap().approvals[0].approval.state,
        ApprovalState::Pending
    );

    let intent = uuid();
    let sent = peer.requests().len();
    let receipt = turns
        .decide(intent.clone(), id.clone(), 1, Decision::Approve)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&receipt).unwrap(),
        json!({"id": id, "state": "accepted"})
    );
    // One request: the approval, the revision, the user's own identity and the decision.
    assert_eq!(since(&peer, sent), [RESPOND]);
    let Operation::ApprovalRespond(asked) = &peer.requests()[sent] else {
        unreachable!()
    };
    assert_eq!(
        (
            &asked.intent_id,
            &asked.managed_approval_id,
            asked.expected_revision,
            asked.decision
        ),
        (&intent, &id, 1, ApprovalDecision::Accept)
    );
    // The same action again is the authority's receipt of it, whatever became of the
    // approval since.
    assert_eq!(
        turns
            .decide(intent.clone(), id.clone(), 1, Decision::Approve)
            .unwrap(),
        receipt
    );
    // The same identity never carries the other decision.
    assert_eq!(
        refused(turns.decide(intent, id.clone(), 1, Decision::Deny)),
        (SessionFailure::Conflict, false)
    );
    // Decided is decided: another decision, under any identity, is refused.
    for decision in [Decision::Approve, Decision::Deny] {
        assert_eq!(
            refused(turns.decide(uuid(), id.clone(), 2, decision)),
            (SessionFailure::NotAllowed, false)
        );
    }
    assert_eq!(
        world.lock().unwrap().approvals[0].approval.decision,
        Some(ApprovalDecision::Accept)
    );

    // Deny is a refusal of this one action. It is never the turn's cancel, and never
    // anything broader than a decline.
    let other = world.lock().unwrap().ask(&turn, 1);
    let sent = peer.requests().len();
    let receipt = turns
        .decide(uuid(), other.clone(), 1, Decision::Deny)
        .unwrap();
    assert_eq!(receipt.state, ApprovalState::Declined);
    let Operation::ApprovalRespond(asked) = &peer.requests()[sent] else {
        unreachable!()
    };
    assert_eq!(asked.decision, ApprovalDecision::Decline);
    assert_eq!(since(&peer, sent), [RESPOND]);
    assert!(!peer.methods().iter().any(|m| m == CANCEL || m == REVOKE));
}

#[test]
fn a_decision_whose_answer_is_lost_is_settled_by_the_same_intent_and_no_other() {
    let session = session_id();
    let mut world = World::new(&session);
    world.profile = "local_tools_v1";
    // The authority commits the decision and its answer never leaves.
    let (peer, _, turns, world) = authority(world.fault(RESPOND, 1, Fault::AnswerLost));
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    world
        .lock()
        .unwrap()
        .advance(&started.id, TurnState::Running, None);
    let id = world.lock().unwrap().ask(&started.id, 0);

    let intent = uuid();
    // Not known to be taken: reported as that, never as a refusal.
    assert_eq!(
        refused(turns.decide(intent.clone(), id.clone(), 1, Decision::Approve)),
        (SessionFailure::Unavailable, true)
    );
    // What the authority holds is read: it was taken.
    let now = turns.approval(id.clone(), None, None).unwrap();
    assert_eq!(
        (now.approval.state, now.approval.decidable),
        (ApprovalState::Accepted, false)
    );
    // The same intent settles it: the receipt, and no second decision.
    let receipt = turns
        .decide(intent, id.clone(), 1, Decision::Approve)
        .unwrap();
    assert_eq!(receipt.state, ApprovalState::Accepted);
    assert_eq!(world.lock().unwrap().decided.len(), 1);
    // The other decision, sent to make sure, changes nothing.
    assert_eq!(
        refused(turns.decide(uuid(), id, 1, Decision::Deny)),
        (SessionFailure::NotAllowed, false)
    );
    assert_eq!(
        world.lock().unwrap().approvals[0].approval.decision,
        Some(ApprovalDecision::Accept)
    );
    assert_eq!(peer.methods().iter().filter(|m| *m == RESPOND).count(), 3);
}

#[test]
fn an_answer_about_another_approval_is_not_shown_and_the_list_is_bounded() {
    let (_peer, turns, world, _, turn) = running();
    // More approvals than one list reads: the newest eight, newest first.
    let ids: Vec<ManagedApprovalId> = (0..20)
        .map(|_| {
            let mut world = world.lock().unwrap();
            let id = world.ask(&turn, 0);
            world.end(&id, "invalidated", "turn_ended");
            id
        })
        .collect();
    let list = turns.approvals().unwrap();
    assert!(list.pending.is_empty());
    assert_eq!(list.recent.len(), APPROVAL_PAGE);
    assert_eq!(
        list.recent.iter().map(|a| a.id.clone()).collect::<Vec<_>>(),
        ids.iter()
            .rev()
            .take(APPROVAL_PAGE)
            .cloned()
            .collect::<Vec<_>>()
    );

    // A peer that answers a read with another approval, and a decision with a receipt
    // for another one.
    let session = session_id();
    let asked = ManagedApprovalId::parse(uuid().as_str()).unwrap();
    let mut other = World::new(&session);
    let foreign = other.ask(&ManagedTurnId::parse(uuid().as_str()).unwrap(), 0);
    let status = other.approvals[0].clone();
    let peer = Peer::canonical(move |op| match op {
        Operation::ApprovalStatus(_) => answer(Ok(ResultValue::Approval(Box::new(status.clone())))),
        Operation::ApprovalRespond(p) => answer(Ok(ResultValue::ApprovalReceipt(
            serde_json::from_value(json!({
                "managed_approval_id": foreign, "managed_turn_id": uuid(),
                "kind": "command_execution", "decision": p.decision,
                "state": p.decision.state(), "revision": 2,
                "action_fingerprint": FINGERPRINT.repeat(32),
                "decided_at": "2026-10-08T00:00:09Z",
            }))
            .unwrap(),
        ))),
        other => panic!("{other:?}"),
    });
    let turns = TurnService::new(peer.service());
    assert_eq!(
        refused(turns.approval(asked.clone(), None, None)),
        (SessionFailure::Internal, false)
    );
    assert_eq!(
        refused(turns.decide(uuid(), asked, 1, Decision::Approve)),
        (SessionFailure::Internal, false)
    );
}

#[test]
fn whether_a_turn_runs_is_the_record_of_the_turn_admitted_last() {
    let session = session_id();
    let (peer, sessions, turns, world) = authority(World::new(&session));
    let ask = || sessions.call(|client| turns.active(client));
    // Nothing was ever admitted: no turn can be running, and no record is read for it.
    assert!(!ask().unwrap());
    assert_eq!(peer.methods(), [AUDIT]);

    let first = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    assert!(ask().unwrap());
    world
        .lock()
        .unwrap()
        .advance(&first.id, TurnState::Running, None);
    assert!(ask().unwrap());
    // Ended: nothing runs. Only the last admission is ever read.
    world
        .lock()
        .unwrap()
        .advance(&first.id, TurnState::Completed, Some(TurnReason::Completed));
    let sent = peer.requests().len();
    assert!(!ask().unwrap());
    assert_eq!(since(&peer, sent), [AUDIT, STATUS]);
    let second = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let sent = peer.requests().len();
    assert!(ask().unwrap());
    let Some(Operation::TurnStatus(read)) = peer.requests().split_off(sent).pop() else {
        unreachable!()
    };
    assert_eq!(read.managed_turn_id, second.id);
    // A reading that fails is an error, never "no turn".
    let (peer, sessions, turns, _) =
        authority(World::new(&session).fault(AUDIT, 1, Fault::Refuse(ControlError::Timeout)));
    assert_eq!(
        refused(sessions.call(|client| turns.active(client))),
        (SessionFailure::Unavailable, true)
    );
    drop(peer);
}

#[test]
fn the_window_s_approval_fixture_has_exactly_the_shape_of_this_projection() {
    let (_peer, turns, world, _, turn) = running();
    let command = world.lock().unwrap().ask(&turn, 0);
    let list = serde_json::to_value(turns.approvals().unwrap()).unwrap();
    let detail =
        serde_json::to_value(turns.approval(command.clone(), None, None).unwrap()).unwrap();
    let file = world.lock().unwrap().ask(&turn, 2);
    let change = serde_json::to_value(turns.approval(file, None, None).unwrap()).unwrap();
    let receipt =
        serde_json::to_value(turns.decide(uuid(), command, 1, Decision::Approve).unwrap()).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/approval-list-v1.json"
    ))
    .unwrap();
    let keys = |v: &serde_json::Value| -> Vec<String> {
        let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(keys(&fixture["empty"]), keys(&list));
    for shown in [&fixture["command"], &fixture["file"]] {
        assert_eq!(keys(shown), keys(&detail));
        assert_eq!(keys(&shown["approval"]), keys(&detail["approval"]));
    }
    assert_eq!(
        keys(&fixture["command"]["command"]),
        keys(&detail["command"])
    );
    assert_eq!(keys(&fixture["file"]["file"]), keys(&change["file"]));
    for ended in fixture["recent"].as_array().unwrap() {
        assert_eq!(keys(ended), keys(&detail["approval"]));
        // Every name the fixture uses is one the contract has.
        assert!(serde_json::from_value::<ApprovalState>(ended["state"].clone()).is_ok());
        assert!(serde_json::from_value::<ApprovalReason>(ended["reason"].clone()).is_ok());
    }
    assert!(
        serde_json::from_value::<FileChangeKind>(fixture["file"]["file"]["kind"].clone()).is_ok()
    );
    assert_eq!(keys(&receipt), ["id".to_owned(), "state".to_owned()]);
}
