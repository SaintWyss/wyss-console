//! Deterministic tests of provider approval mediation. The runtime is a channel: every
//! command the core dispatches is read here, so "provider Accept frames" is a count.
use super::{
    conversations::Conversations,
    runtime::{MaterializationStage, ProviderDecision, Runtime, RuntimeCommand},
    store::{Clock, Store, uuid},
    turns::{Turns, Verdict},
};
use crate::{
    adapters::codex::{
        CodexItemId, CodexRequestId, CodexThreadId, CodexTurnId, Direction, TurnOutcome,
        WireMessage,
        safety::{
            self, ActionKind, ApprovalRequest, CommandProposal, FileProposal, ItemEnd, NoticeKind,
            Offered, ProposedChange, TurnNotice,
        },
    },
    contracts::{
        control::*,
        identities::{AgentRuntimeInstanceId, ManagedSessionId},
    },
};
use ApprovalDecision::{Accept, Cancel, Decline};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, symlink},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, sync_channel},
    },
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

struct TestClock(Mutex<OffsetDateTime>);
impl Clock for TestClock {
    fn now(&self) -> OffsetDateTime {
        *self.0.lock().unwrap()
    }
}
fn all_scopes() -> Vec<ExecutionScope> {
    vec![
        ExecutionScope::SessionCreate,
        ExecutionScope::SessionResume,
        ExecutionScope::TurnStart,
        ExecutionScope::CommandExecute,
        ExecutionScope::FileChange,
    ]
}
/// What real Codex 0.159.0 offers for a command: no `decline`.
const COMMAND_OFFER: Offered = Offered {
    accept: true,
    decline: false,
    cancel: true,
};
const FILE_OFFER: Offered = Offered {
    accept: true,
    decline: true,
    cancel: true,
};
fn item(id: &str) -> CodexItemId {
    CodexItemId::parse(id).unwrap()
}
fn request(id: &str) -> CodexRequestId {
    CodexRequestId::String(id.to_owned())
}
fn change(path: &str, kind: FileChangeKind, to: Option<&str>, diff: &str) -> ProposedChange {
    ProposedChange {
        path: path.to_owned(),
        kind,
        move_path: to.map(str::to_owned),
        diff: diff.to_owned(),
    }
}
struct Rig {
    root: PathBuf,
    workspace: String,
    store: Store,
    clock: Arc<TestClock>,
    runtime: Runtime,
    commands: Receiver<RuntimeCommand>,
    _wake: std::os::unix::net::UnixStream,
    conversations: Conversations,
    turns: Turns,
    session: ManagedSessionRecord,
    lease: ExecutionLease,
    instance: AgentRuntimeInstanceId,
    turn: ManagedTurnRecord,
    thread: CodexThreadId,
    provider_turn: CodexTurnId,
}
impl Rig {
    fn new() -> Self {
        Self::with(all_scopes(), 3600, true)
    }
    fn with(scopes: Vec<ExecutionScope>, ttl: u32, local: bool) -> Self {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("wyss-mediate-{}", uuid().as_str()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::create_dir(root.join("ws")).unwrap();
        let workspace = root.join("ws").to_str().unwrap().to_owned();
        let mut store = Store::open(&root.join("db"), uuid()).unwrap();
        let clock = Arc::new(TestClock(Mutex::new(
            OffsetDateTime::parse("2026-10-08T00:00:00Z", &Rfc3339).unwrap(),
        )));
        store.set_clock(clock.clone());
        let ResultValue::Session(session) = store
            .apply(&Operation::Prepare(Prepare {
                intent_id: uuid(),
                adapter_id: RuntimeTarget::codex().adapter_id,
                runtime_kind: RuntimeTarget::codex().runtime_kind,
            }))
            .unwrap()
        else {
            panic!()
        };
        let ResultValue::Lease(lease) = store
            .apply(&Operation::LeaseGrant(LeaseGrant {
                intent_id: uuid(),
                session_id: session.managed_session_id.clone(),
                expected_session_revision: 1,
                scopes,
                ttl_seconds: ttl,
            }))
            .unwrap()
        else {
            panic!()
        };
        let instance = AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
        let reservation = store
            .reserve_binding(&Operation::BindingCreate(BindingCreate {
                intent_id: uuid(),
                managed_session_id: session.managed_session_id.clone(),
                expected_session_revision: 1,
                workspace_root: workspace.clone(),
            }))
            .unwrap();
        store.dispatch_binding(&reservation).unwrap();
        let thread = CodexThreadId::parse("opaque-durable-thread").unwrap();
        for stage in [
            MaterializationStage::Started,
            MaterializationStage::Archived,
            MaterializationStage::Unarchived,
        ] {
            store
                .checkpoint_binding(&reservation, &thread, stage)
                .unwrap();
        }
        store
            .commit_binding(&reservation, &thread, &instance)
            .unwrap();
        let (runtime, commands, wake) =
            Runtime::ready_for_test(&mut store, &instance, local.then_some(workspace.as_str()));
        let mut conversations = Conversations::default();
        conversations.attach_for_test(&session.managed_session_id, &instance);
        let mut rig = Self {
            root,
            workspace,
            store,
            clock,
            runtime,
            commands,
            _wake: wake,
            conversations,
            turns: Turns::default(),
            session,
            lease,
            instance,
            turn: unreachable_turn(),
            thread,
            provider_turn: CodexTurnId::parse("provider-turn-1").unwrap(),
        };
        rig.start_turn();
        rig
    }
    fn start_turn(&mut self) {
        let ResultValue::Turn(turn) = self
            .turns
            .start(
                &mut self.store,
                &mut self.runtime,
                &self.conversations,
                &TurnStart {
                    intent_id: uuid(),
                    managed_session_id: self.session.managed_session_id.clone(),
                    expected_session_revision: 1,
                    prompt: "do the thing".into(),
                },
            )
            .unwrap()
        else {
            panic!()
        };
        assert!(matches!(
            self.commands.try_recv(),
            Ok(RuntimeCommand::Start(_))
        ));
        assert_eq!(
            self.turns
                .accepted(
                    &mut self.store,
                    &mut self.runtime,
                    &self.instance,
                    &turn.managed_turn_id,
                    Ok(self.provider_turn.clone()),
                )
                .unwrap(),
            Verdict::Continue
        );
        self.turn = turn;
        assert_eq!(self.notice(NoticeKind::Started), Verdict::Continue);
    }
    fn notice(&mut self, kind: NoticeKind) -> Verdict {
        self.turns
            .notice(
                &mut self.store,
                &mut self.runtime,
                &self.instance.clone(),
                TurnNotice {
                    thread: self.thread.clone(),
                    turn: self.provider_turn.clone(),
                    kind,
                },
            )
            .unwrap()
    }
    fn path(&self, relative: &str) -> String {
        format!("{}/{relative}", self.workspace)
    }
    fn propose_command(&mut self, id: &str, command: &str, cwd: &str) -> Verdict {
        self.notice(NoticeKind::CommandProposed(CommandProposal {
            item: item(id),
            command: command.to_owned(),
            cwd: cwd.to_owned(),
            actions: vec!["unknown".into()],
        }))
    }
    fn ask(
        &mut self,
        kind: ActionKind,
        req: &str,
        id: &str,
        command: Option<(&str, &str)>,
        offered: Offered,
    ) -> Verdict {
        self.notice(NoticeKind::ApprovalRequested(ApprovalRequest {
            request: request(req),
            item: item(id),
            kind,
            command: command.map(|c| c.0.to_owned()),
            cwd: command.map(|c| c.1.to_owned()),
            actions: Vec::new(),
            offered,
        }))
    }
    fn propose_file(&mut self, id: &str, changes: Vec<ProposedChange>) -> Verdict {
        self.notice(NoticeKind::FileProposed(FileProposal {
            item: item(id),
            changes,
        }))
    }
    /// The ordinary command flow of the pinned runtime: announced, then asked.
    fn command_approval(&mut self) -> ManagedApprovalRecord {
        let cwd = self.workspace.clone();
        assert_eq!(
            self.propose_command("call-1", "printf WYSS_OK", &cwd),
            Verdict::Continue
        );
        assert_eq!(
            self.ask(
                ActionKind::Command,
                "req-1",
                "call-1",
                Some(("printf WYSS_OK", &cwd)),
                COMMAND_OFFER
            ),
            Verdict::Continue
        );
        self.pending().expect("pending approval")
    }
    fn file_approval(&mut self) -> ManagedApprovalRecord {
        let target = self.path("probe.txt");
        assert_eq!(
            self.propose_file(
                "patch-1",
                vec![change(&target, FileChangeKind::Add, None, "+hello\n")]
            ),
            Verdict::Continue
        );
        assert_eq!(
            self.ask(ActionKind::File, "req-f", "patch-1", None, FILE_OFFER),
            Verdict::Continue
        );
        self.pending().expect("pending approval")
    }
    fn pending(&self) -> Option<ManagedApprovalRecord> {
        self.store.pending_approval().unwrap()
    }
    fn record(&self, a: &ManagedApprovalRecord) -> ManagedApprovalRecord {
        self.store.approval(&a.managed_approval_id).unwrap()
    }
    fn intent(a: &ManagedApprovalRecord, decision: ApprovalDecision) -> ApprovalRespond {
        ApprovalRespond {
            intent_id: uuid(),
            managed_approval_id: a.managed_approval_id.clone(),
            expected_revision: a.revision,
            decision,
        }
    }
    fn respond(&mut self, p: &ApprovalRespond) -> Receiver<ControlResult<ResultValue>> {
        let (reply, result) = sync_channel(1);
        self.turns.respond(
            &mut self.store,
            &mut self.runtime,
            &self.conversations,
            p,
            reply,
        );
        result
    }
    /// The supervisor's answer to the attestation the core asked for.
    fn attest(&mut self, valid: bool) -> Verdict {
        let Ok(RuntimeCommand::Attest { request, workspace }) = self.commands.try_recv() else {
            panic!("no attestation was requested")
        };
        assert_eq!(workspace, self.workspace);
        self.turns
            .attested(
                &mut self.store,
                &mut self.runtime,
                &self.conversations,
                &request,
                valid,
            )
            .unwrap()
    }
    /// A complete decision: for an Accept, the attestation succeeds.
    fn decide(&mut self, p: &ApprovalRespond) -> ControlResult<ApprovalReceipt> {
        let result = self.respond(p);
        let outcome = match result.try_recv() {
            Ok(outcome) => outcome,
            Err(_) => {
                self.attest(true);
                result.try_recv().expect("decision reply")
            }
        };
        outcome.map(|r| match r {
            ResultValue::ApprovalReceipt(receipt) => receipt,
            _ => panic!("not a receipt"),
        })
    }
    /// Every answer handed to the supervisor since the last call, and whether anything
    /// else was dispatched.
    fn answers(&mut self) -> Vec<ProviderDecision> {
        let mut answers = Vec::new();
        while let Ok(command) = self.commands.try_recv() {
            match command {
                RuntimeCommand::Resolve { decision, .. } => answers.push(decision),
                RuntimeCommand::Interrupt { .. } => panic!("unexpected interrupt"),
                _ => panic!("unexpected command"),
            }
        }
        answers
    }
    fn enforce(&mut self) {
        self.turns
            .enforce(&mut self.store, &mut self.runtime)
            .unwrap();
    }
    fn advance(&self, seconds: i64) {
        *self.clock.0.lock().unwrap() += time::Duration::seconds(seconds);
    }
    fn end_turn(&mut self, outcome: TurnOutcome) -> ManagedTurnRecord {
        self.notice(NoticeKind::Terminal(outcome));
        self.store.turn(&self.turn.managed_turn_id).unwrap()
    }
    fn apply(&mut self, op: Operation) -> ResultValue {
        self.store.apply(&op).unwrap()
    }
    fn revoke(&mut self) {
        let lease = self.lease.clone();
        self.apply(Operation::LeaseRevoke(LeaseRevoke {
            intent_id: uuid(),
            lease_id: lease.lease_id,
            expected_revision: lease.revision,
        }));
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
/// Placeholder until the first turn of a rig is admitted.
fn unreachable_turn() -> ManagedTurnRecord {
    serde_json::from_value(json!({
        "managed_turn_id": "00000000-0000-4000-8000-000000000001",
        "managed_session_id": "00000000-0000-4000-8000-000000000002",
        "codex_thread_id": "placeholder",
        "codex_turn_id": null,
        "admitted_lease_id": "00000000-0000-4000-8000-000000000003",
        "admitted_lease_revision": 1,
        "runtime_instance_id": "00000000-0000-4000-8000-000000000004",
        "state": "starting",
        "revision": 1,
        "created_at": "2026-10-08T00:00:00Z",
        "updated_at": "2026-10-08T00:00:00Z",
        "terminal_at": null,
        "terminal_reason": null
    }))
    .unwrap()
}

#[test]
fn command_request_is_admitted_as_one_pending_approval_and_nothing_runs() {
    let mut rig = Rig::new();
    let a = rig.command_approval();
    assert_eq!(a.kind, ApprovalKind::CommandExecution);
    assert_eq!(a.state, ApprovalState::Pending);
    assert_eq!(a.provider_item_id, "call-1");
    assert_eq!(a.managed_turn_id, rig.turn.managed_turn_id);
    assert_eq!(a.runtime_instance_id, rig.instance);
    // The ORIGINAL lease of the turn, by identity and admitted revision.
    assert_eq!(a.admitted_lease_id, rig.lease.lease_id);
    assert_eq!(a.admitted_lease_revision, rig.lease.revision);
    assert_eq!(a.structure.command_bytes as usize, "printf WYSS_OK".len());
    assert_eq!(a.structure.cwd_bytes as usize, rig.workspace.len());
    // Five minutes: the lease outlives the approval.
    assert_eq!(
        timestamp(&a.expires_at).unwrap() - timestamp(&a.created_at).unwrap(),
        time::Duration::seconds(APPROVAL_TTL_SECONDS)
    );
    assert_eq!(rig.turns.approval_deadline(), timestamp(&a.expires_at).ok());
    // Admission alone hands the supervisor nothing: no answer, so no execution.
    assert!(rig.answers().is_empty());
    // A second request while one is pending is not a queue: it is a violation.
    assert_eq!(
        rig.ask(
            ActionKind::Command,
            "req-2",
            "call-2",
            Some(("id", &rig.workspace.clone())),
            COMMAND_OFFER
        ),
        Verdict::Violation
    );
    assert!(rig.answers().is_empty());
    // The same holds while the held request has no approval yet.
    let mut rig = Rig::new();
    rig.ask(ActionKind::File, "req-f", "patch-1", None, FILE_OFFER);
    assert!(rig.pending().is_none());
    assert_eq!(
        rig.ask(
            ActionKind::Command,
            "req-2",
            "call-2",
            Some(("id", &rig.workspace.clone())),
            COMMAND_OFFER
        ),
        Verdict::Violation
    );
    assert!(rig.pending().is_none());
    assert!(rig.answers().is_empty());
}
#[test]
fn approval_deadline_is_the_stricter_of_the_ttl_and_the_original_lease() {
    let mut rig = Rig::with(all_scopes(), 60, true);
    let a = rig.command_approval();
    assert_eq!(
        timestamp(&a.expires_at).unwrap(),
        timestamp(&rig.lease.expires_at).unwrap()
    );
    rig.advance(60);
    rig.enforce();
    let expired = rig.record(&a);
    assert_eq!(expired.state, ApprovalState::Expired);
    assert_eq!(expired.terminal_reason, Some(ApprovalReason::LeaseExpired));
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
    assert_eq!(
        rig.decide(&Rig::intent(&expired, Accept)),
        Err(ControlError::InvalidTransition)
    );
    assert!(rig.answers().is_empty());
    assert_eq!(
        rig.end_turn(TurnOutcome::Interrupted).terminal_reason,
        Some(TurnReason::LeaseExpired)
    );
}
#[test]
fn file_request_is_admitted_only_with_its_complete_change_set() {
    let mut rig = Rig::new();
    let a = rig.file_approval();
    assert_eq!(a.kind, ApprovalKind::FileChange);
    assert_eq!((a.structure.files, a.structure.diff_bytes), (1, 7));
    assert!(rig.answers().is_empty());
    // An identical restatement changes nothing; a different change set after the request
    // is not the action that was fingerprinted.
    let target = rig.path("probe.txt");
    assert_eq!(
        rig.propose_file(
            "patch-1",
            vec![change(&target, FileChangeKind::Add, None, "+hello\n")]
        ),
        Verdict::Continue
    );
    assert_eq!(
        rig.propose_file(
            "patch-1",
            vec![change(&target, FileChangeKind::Add, None, "+other\n")]
        ),
        Verdict::Violation
    );

    // The request arrives first: it is held, bounded, and can never be accepted.
    let mut rig = Rig::new();
    assert_eq!(
        rig.ask(ActionKind::File, "req-f", "patch-1", None, FILE_OFFER),
        Verdict::Continue
    );
    assert!(rig.pending().is_none());
    assert!(rig.turns.approval_deadline().is_some());
    // An empty announcement is still no proposal.
    assert_eq!(rig.propose_file("patch-1", Vec::new()), Verdict::Continue);
    assert!(rig.pending().is_none());
    let target = rig.path("late.txt");
    assert_eq!(
        rig.propose_file(
            "patch-1",
            vec![change(&target, FileChangeKind::Add, None, "+late\n")]
        ),
        Verdict::Continue
    );
    let a = rig
        .pending()
        .expect("opened once the change set is complete");
    assert_eq!(a.provider_item_id, "patch-1");
    assert!(rig.answers().is_empty());

    // No complete proposal before the deadline: resolved reductively.
    let mut rig = Rig::new();
    rig.ask(ActionKind::File, "req-f", "patch-1", None, FILE_OFFER);
    rig.advance(9);
    rig.enforce();
    assert!(rig.answers().is_empty());
    rig.advance(1);
    rig.enforce();
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
    assert!(rig.pending().is_none());
    assert_eq!(
        rig.end_turn(TurnOutcome::Interrupted).terminal_reason,
        Some(TurnReason::ActionRejected)
    );
}
fn frame(value: serde_json::Value) -> WireMessage {
    WireMessage::from_value(value, Direction::ServerToClient).unwrap()
}
fn command_request(extra: serde_json::Value) -> serde_json::Value {
    let mut params = json!({
        "threadId": "thread", "turnId": "turn", "itemId": "call-1", "startedAtMs": 1,
        "command": "printf WYSS_OK", "cwd": "/work/ws", "kind": "command", "environmentId": "local",
        "commandActions": [{"type": "unknown", "command": "printf WYSS_OK"}],
        "proposedExecpolicyAmendment": ["printf", "WYSS_OK"],
        "availableDecisions": ["accept", {"acceptWithExecpolicyAmendment": {"execpolicy_amendment": ["printf"]}}, "cancel"],
    });
    for (k, v) in extra.as_object().unwrap() {
        params[k] = v.clone();
    }
    json!({"id": "approval-1", "method": "item/commandExecution/requestApproval", "params": params})
}
#[test]
fn only_the_two_plain_approval_requests_exist_and_only_under_local_tools() {
    // The real command request: an amendment is offered, and never taken.
    let Some(TurnNotice {
        kind: NoticeKind::ApprovalRequested(r),
        ..
    }) = safety::local_notification(&frame(command_request(json!({})))).unwrap()
    else {
        panic!("not admitted")
    };
    assert!(r.offered == COMMAND_OFFER);
    assert_eq!(r.command.as_deref(), Some("printf WYSS_OK"));
    assert_eq!(r.cwd.as_deref(), Some("/work/ws"));
    assert_eq!(r.actions, ["unknown: printf WYSS_OK"]);
    assert!(r.request == request("approval-1") && r.kind == ActionKind::Command);
    let file = json!({"id": 7, "method": "item/fileChange/requestApproval", "params": {
        "threadId": "thread", "turnId": "turn", "itemId": "patch-1", "startedAtMs": 1, "reason": null, "grantRoot": null}});
    let Some(TurnNotice {
        kind: NoticeKind::ApprovalRequested(r),
        ..
    }) = safety::local_notification(&frame(file.clone())).unwrap()
    else {
        panic!("not admitted")
    };
    assert!(r.offered == FILE_OFFER && r.kind == ActionKind::File && r.command.is_none());
    // Everything that asks for more than the one announced action is never answered.
    let mut refused = vec![
        command_request(json!({"kind": "writeStdin"})),
        command_request(json!({"additionalPermissions": {"network": {"enabled": true}}})),
        command_request(json!({"additionalPermissions": {}})),
        command_request(
            json!({"networkApprovalContext": {"host": "example.com", "protocol": "https"}}),
        ),
        command_request(
            json!({"proposedNetworkPolicyAmendments": [{"action": "allow", "host": "example.com"}]}),
        ),
        command_request(json!({"approvalId": "callback-1"})),
        command_request(json!({"environmentId": "remote"})),
        command_request(json!({"futureField": true})),
        command_request(json!({"availableDecisions": "accept"})),
        command_request(json!({"command": 7})),
    ];
    let mut grant = file.clone();
    grant["params"]["grantRoot"] = json!("/");
    refused.push(grant);
    for (method, params) in [
        (
            "item/permissions/requestApproval",
            json!({"threadId": "thread", "turnId": "turn", "itemId": "i", "startedAtMs": 1, "cwd": "/work", "permissions": {}}),
        ),
        ("item/tool/requestUserInput", json!({})),
        ("item/tool/call", json!({})),
        ("mcpServer/elicitation/request", json!({})),
        ("execCommandApproval", json!({})),
        ("applyPatchApproval", json!({})),
        ("future/action", json!({})),
    ] {
        refused.push(json!({"id": "x", "method": method, "params": params}));
    }
    for value in refused {
        // Refused by the frame codec or by the classifier: either way it is never held.
        let m = WireMessage::from_value(value.clone(), Direction::ServerToClient);
        assert!(
            m.is_err()
                || safety::local_notification(&m.unwrap())
                    .is_err_and(|e| e.code == "inference_isolation_violation"),
            "{value}"
        );
    }
    // The inference profile answers nothing: every server request is a violation there,
    // the two mediated ones included.
    for value in [command_request(json!({})), file] {
        assert!(
            safety::notification(&frame(value))
                .is_err_and(|e| e.code == "inference_isolation_violation")
        );
    }
}
#[test]
fn local_tools_items_are_classified_and_stay_forbidden_for_inference() {
    let ids = json!({"threadId": "thread", "turnId": "turn"});
    let with = |method: &str, extra: serde_json::Value| {
        let mut params = ids.clone();
        for (k, v) in extra.as_object().unwrap() {
            params[k] = v.clone();
        }
        json!({"method": method, "params": params})
    };
    let command = |status: &str, source: &str| {
        json!({"item": {"id": "call-1", "type": "commandExecution", "command": "printf x", "cwd": "/work/ws",
            "commandActions": [], "status": status, "source": source, "processId": "42"}, "startedAtMs": 1, "completedAtMs": 2})
    };
    let patch = |status: &str| {
        json!({"item": {"id": "patch-1", "type": "fileChange", "status": status, "changes": [
            {"path": "/work/ws/a", "kind": {"type": "add"}, "diff": "+a\n"},
            {"path": "/work/ws/b", "kind": {"type": "delete"}, "diff": "-b\n"},
            {"path": "/work/ws/c", "kind": {"type": "update", "move_path": null}, "diff": "@@\n"},
            {"path": "/work/ws/d", "kind": {"type": "update", "move_path": "/work/ws/e"}, "diff": "@@\n"}]},
            "startedAtMs": 1, "completedAtMs": 2})
    };
    let accepted = [
        with("item/started", command("inProgress", "unifiedExecStartup")),
        with("item/started", command("inProgress", "agent")),
        with("item/completed", command("completed", "agent")),
        with("item/completed", command("declined", "agent")),
        with("item/started", patch("inProgress")),
        with("item/completed", patch("failed")),
        with(
            "item/commandExecution/outputDelta",
            json!({"itemId": "call-1", "delta": "out"}),
        ),
        with(
            "item/fileChange/outputDelta",
            json!({"itemId": "patch-1", "delta": "ok"}),
        ),
        with(
            "item/fileChange/patchUpdated",
            json!({"itemId": "patch-1", "changes": []}),
        ),
        with("turn/diff/updated", json!({"diff": "--- a\n+++ b\n"})),
    ];
    for value in &accepted {
        let m = frame(value.clone());
        assert!(
            safety::local_notification(&m).unwrap().is_some(),
            "local: {value}"
        );
        // The same frame on an inference generation is an execution surface.
        assert!(safety::notification(&m).is_err(), "inference: {value}");
    }
    let finished =
        |value: serde_json::Value| match safety::local_notification(&frame(value)).unwrap() {
            Some(TurnNotice {
                kind: NoticeKind::ItemEnded { output, end, .. },
                ..
            }) => (end == ItemEnd::Completed, output),
            _ => panic!(),
        };
    let mut reported = with("item/completed", command("completed", "agent"));
    reported["params"]["item"]["aggregatedOutput"] = json!("WYSS_OK");
    assert_eq!(
        finished(reported.clone()),
        (true, Some("WYSS_OK".to_owned()))
    );
    reported["params"]["item"]["aggregatedOutput"] = json!("");
    assert_eq!(finished(reported), (true, None));
    assert_eq!(
        finished(with("item/completed", command("completed", "agent"))),
        (true, None)
    );
    assert_eq!(
        finished(with("item/completed", patch("failed"))),
        (false, None)
    );
    let Some(TurnNotice {
        kind: NoticeKind::FileProposed(p),
        ..
    }) = safety::local_notification(&frame(with("item/started", patch("inProgress")))).unwrap()
    else {
        panic!()
    };
    let kinds: Vec<_> = p
        .changes
        .iter()
        .map(|c| (c.kind, c.move_path.clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            (FileChangeKind::Add, None),
            (FileChangeKind::Delete, None),
            (FileChangeKind::Update, None),
            (FileChangeKind::Move, Some("/work/ws/e".to_owned())),
        ]
    );
    // An item that starts already executed, input to a terminal, a user shell command and
    // every other tool surface stay violations under local tools too.
    let mut rejected = vec![
        with("item/started", command("completed", "agent")),
        with(
            "item/started",
            command("inProgress", "unifiedExecInteraction"),
        ),
        with("item/started", command("inProgress", "userShell")),
        with("item/completed", command("inProgress", "agent")),
        with("item/started", patch("completed")),
        with(
            "item/commandExecution/terminalInteraction",
            json!({"itemId": "call-1", "processId": "42", "stdin": "x"}),
        ),
        with("hook/started", json!({})),
        with("mcpServer/startupStatus/updated", json!({})),
        with(
            "item/fileChange/patchUpdated",
            json!({"itemId": "patch-1", "changes": [{"path": "/work/ws/a", "kind": {"type": "add", "move_path": "/x"}, "diff": ""}]}),
        ),
    ];
    for kind in [
        "mcpToolCall",
        "webSearch",
        "collabAgentToolCall",
        "dynamicToolCall",
    ] {
        rejected.push(with(
            "item/started",
            json!({"item": {"id": "x", "type": kind, "server": "s", "tool": "t", "arguments": {}, "status": "inProgress"}, "startedAtMs": 1}),
        ));
    }
    for value in rejected {
        let m = WireMessage::from_value(value.clone(), Direction::ServerToClient);
        assert!(
            m.is_err() || safety::local_notification(&m.unwrap()).is_err(),
            "{value}"
        );
    }
}
#[test]
fn inference_generation_has_no_mediated_surface_at_all() {
    let mut rig = Rig::with(all_scopes(), 3600, false);
    let cwd = rig.workspace.clone();
    assert_eq!(
        rig.propose_command("call-1", "id", &cwd),
        Verdict::Violation
    );
    assert_eq!(
        rig.ask(
            ActionKind::Command,
            "req-1",
            "call-1",
            Some(("id", &cwd)),
            COMMAND_OFFER
        ),
        Verdict::Violation
    );
    assert!(rig.pending().is_none());
    assert!(rig.answers().is_empty());
}
#[test]
fn accept_is_attested_committed_once_and_answered_exactly_once() {
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let p = Rig::intent(&a, Accept);
    let reply = rig.respond(&p);
    // Nothing is decided while the profile is being re-proven.
    assert!(reply.try_recv().is_err());
    assert_eq!(rig.record(&a).state, ApprovalState::Pending);
    assert_eq!(rig.attest(true), Verdict::Continue);
    let Ok(Ok(ResultValue::ApprovalReceipt(receipt))) = reply.try_recv() else {
        panic!("no receipt")
    };
    assert_eq!(
        (receipt.decision, receipt.state),
        (Accept, ApprovalState::Accepted)
    );
    assert_eq!(receipt.revision, a.revision + 1);
    assert_eq!(receipt.action_fingerprint, a.action_fingerprint);
    let accepted = rig.record(&a);
    assert_eq!(accepted.effect, ApprovalEffect::Dispatched);
    assert_eq!(rig.answers(), [ProviderDecision::Accept]);
    // The same intent returns the same receipt and never a second answer.
    assert_eq!(rig.decide(&p), Ok(receipt.clone()));
    // The same intent with another payload, and any other intent, change nothing.
    let mut conflict = p.clone();
    conflict.decision = Cancel;
    assert_eq!(rig.decide(&conflict), Err(ControlError::IntentConflict));
    for decision in [Accept, Decline, Cancel] {
        assert_eq!(
            rig.decide(&Rig::intent(&accepted, decision)),
            Err(ControlError::InvalidTransition)
        );
    }
    assert!(rig.answers().is_empty());
    // Only now may the item produce output and complete; the report confirms the effect.
    assert_eq!(
        rig.notice(NoticeKind::RequestResolved(request("req-1"))),
        Verdict::Continue
    );
    assert_eq!(
        rig.notice(NoticeKind::Output {
            item: item("call-1"),
            kind: ActionKind::Command,
            delta: "WYSS_OK".into()
        }),
        Verdict::Continue
    );
    assert_eq!(
        rig.notice(NoticeKind::ItemEnded {
            output: None,
            item: item("call-1"),
            kind: ActionKind::Command,
            end: ItemEnd::Completed
        }),
        Verdict::Continue
    );
    assert_eq!(rig.record(&a).effect, ApprovalEffect::Confirmed);
    // A second approval in the same turn is a new decision, never inherited.
    let cwd = rig.workspace.clone();
    rig.propose_command("call-2", "id", &cwd);
    rig.ask(
        ActionKind::Command,
        "req-2",
        "call-2",
        Some(("id", &cwd)),
        COMMAND_OFFER,
    );
    let second = rig.pending().unwrap();
    assert_ne!(second.managed_approval_id, a.managed_approval_id);
    assert!(rig.answers().is_empty());
    assert_eq!(
        rig.end_turn(TurnOutcome::Completed).state,
        TurnState::Completed
    );
    assert_eq!(rig.record(&second).state, ApprovalState::Invalidated);
    assert_eq!(rig.record(&a).state, ApprovalState::Accepted);
}
#[test]
fn nothing_runs_before_the_authority_accepted_that_exact_item() {
    for early in ["output", "completed", "failed", "diff", "unannounced"] {
        let mut rig = Rig::new();
        rig.command_approval();
        let verdict = match early {
            "output" => rig.notice(NoticeKind::Output {
                item: item("call-1"),
                kind: ActionKind::Command,
                delta: "x".into(),
            }),
            "diff" => rig.notice(NoticeKind::DiffUpdated),
            "unannounced" => rig.notice(NoticeKind::ItemEnded {
                output: None,
                item: item("never-announced"),
                kind: ActionKind::Command,
                end: ItemEnd::Completed,
            }),
            end => rig.notice(NoticeKind::ItemEnded {
                output: None,
                item: item("call-1"),
                kind: ActionKind::Command,
                end: if end == "failed" {
                    ItemEnd::Failed
                } else {
                    ItemEnd::Completed
                },
            }),
        };
        assert_eq!(verdict, Verdict::Violation, "{early}");
        assert!(rig.answers().is_empty(), "{early}");
    }
    // An accepted command does not authorize applied file changes.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    rig.decide(&Rig::intent(&a, Accept)).unwrap();
    assert_eq!(rig.notice(NoticeKind::DiffUpdated), Verdict::Violation);
    // Nor does the item it was accepted for finish as another kind of action.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    rig.decide(&Rig::intent(&a, Accept)).unwrap();
    assert_eq!(
        rig.notice(NoticeKind::ItemEnded {
            output: None,
            item: item("call-1"),
            kind: ActionKind::File,
            end: ItemEnd::Completed
        }),
        Verdict::Violation
    );
    // A command the runtime ran on its own, without ever asking, is not mediated.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    rig.propose_command("call-9", "ls", &cwd);
    assert_eq!(
        rig.notice(NoticeKind::ItemEnded {
            output: None,
            item: item("call-9"),
            kind: ActionKind::Command,
            end: ItemEnd::Completed
        }),
        Verdict::Violation
    );
}
#[test]
fn wrong_turn_runtime_item_request_and_fingerprint_never_reach_an_accept() {
    // Wrong provider turn or runtime generation: the notice is not about this turn.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    let foreign = TurnNotice {
        thread: rig.thread.clone(),
        turn: CodexTurnId::parse("another-turn").unwrap(),
        kind: NoticeKind::ApprovalRequested(ApprovalRequest {
            request: request("req-x"),
            item: item("call-x"),
            kind: ActionKind::Command,
            command: Some("id".into()),
            cwd: Some(cwd.clone()),
            actions: Vec::new(),
            offered: COMMAND_OFFER,
        }),
    };
    assert_eq!(
        rig.turns
            .notice(
                &mut rig.store,
                &mut rig.runtime,
                &rig.instance,
                foreign.clone()
            )
            .unwrap(),
        Verdict::Continue
    );
    let other = AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
    let mut same_turn = foreign.clone();
    same_turn.turn = rig.provider_turn.clone();
    assert_eq!(
        rig.turns
            .notice(&mut rig.store, &mut rig.runtime, &other, same_turn)
            .unwrap(),
        Verdict::Continue
    );
    assert!(rig.pending().is_none());
    // Wrong item: asked as a command, announced as a file change, and the reverse.
    let target = rig.path("a.txt");
    rig.propose_file(
        "patch-1",
        vec![change(&target, FileChangeKind::Add, None, "+a\n")],
    );
    assert_eq!(
        rig.ask(
            ActionKind::Command,
            "req-1",
            "patch-1",
            Some(("id", &cwd)),
            COMMAND_OFFER
        ),
        Verdict::Violation
    );
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    rig.propose_command("call-1", "id", &cwd);
    assert_eq!(
        rig.ask(ActionKind::File, "req-1", "call-1", None, FILE_OFFER),
        Verdict::Violation
    );
    // Wrong fingerprint: the request restates another command or directory than announced.
    for (command, relative) in [("printf OTHER", ""), ("printf WYSS_OK", "sub")] {
        let mut rig = Rig::new();
        fs::create_dir(rig.root.join("ws/sub")).unwrap();
        let cwd = rig.workspace.clone();
        rig.propose_command("call-1", "printf WYSS_OK", &cwd);
        let asked = if relative.is_empty() {
            cwd.clone()
        } else {
            rig.path(relative)
        };
        assert_eq!(
            rig.ask(
                ActionKind::Command,
                "req-1",
                "call-1",
                Some((command, &asked)),
                COMMAND_OFFER
            ),
            Verdict::Violation
        );
        assert!(rig.pending().is_none());
    }
    // A command is announced once.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    rig.propose_command("call-1", "id", &cwd);
    assert_eq!(
        rig.propose_command("call-1", "id", &cwd),
        Verdict::Violation
    );
    // Wrong request: an attestation for another request decides nothing.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let reply = rig.respond(&Rig::intent(&a, Accept));
    assert_eq!(
        rig.turns
            .attested(
                &mut rig.store,
                &mut rig.runtime,
                &rig.conversations,
                &request("not-the-held-request"),
                true
            )
            .unwrap(),
        Verdict::Continue
    );
    assert!(reply.try_recv().is_err());
    assert_eq!(rig.record(&a).state, ApprovalState::Pending);
    // The provider withdrew the request: it can be neither accepted nor refused any more.
    assert!(matches!(
        rig.commands.try_recv(),
        Ok(RuntimeCommand::Attest { .. })
    ));
    assert_eq!(
        rig.notice(NoticeKind::RequestResolved(request("req-1"))),
        Verdict::Continue
    );
    let withdrawn = rig.record(&a);
    assert_eq!(withdrawn.state, ApprovalState::Invalidated);
    assert_eq!(
        rig.turns
            .attested(
                &mut rig.store,
                &mut rig.runtime,
                &rig.conversations,
                &request("req-1"),
                true
            )
            .unwrap(),
        Verdict::Continue
    );
    assert_eq!(
        reply.try_recv().unwrap().unwrap_err(),
        ControlError::InvalidTransition
    );
    assert!(rig.answers().is_empty());
}
#[test]
fn accept_gate_denies_each_missing_fact_with_zero_accept_answers() {
    // Facts the authority holds in memory: denied before anything is asked of the runtime.
    for fact in [
        "profile",
        "profile_missing",
        "runtime",
        "attachment",
        "session",
        "storage",
        "confinement",
    ] {
        let mut rig = Rig::new();
        fs::create_dir(rig.root.join("ws/sub")).unwrap();
        let cwd = rig.path("sub");
        rig.propose_command("call-1", "id", &cwd);
        rig.ask(
            ActionKind::Command,
            "req-1",
            "call-1",
            Some(("id", &cwd)),
            COMMAND_OFFER,
        );
        let a = rig.pending().unwrap();
        match fact {
            "profile" => rig
                .runtime
                .set_profile_for_test(Some(RuntimeSafetyProfile::CodexInferenceV1)),
            "profile_missing" => rig.runtime.set_profile_for_test(None),
            "runtime" => {
                // Another generation is live, and the session is attached to it: only the
                // identity of the generation that asked can deny.
                let other = AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
                rig.runtime.set_instance_for_test(&other);
                let session = rig.session.managed_session_id.clone();
                rig.conversations.attach_for_test(&session, &other);
            }
            "attachment" => rig.conversations.detach_for_test(),
            "session" => {
                rig.conversations.detach_for_test();
                let other = ManagedSessionId::parse(uuid().as_str()).unwrap();
                rig.conversations.attach_for_test(&other, &rig.instance);
            }
            "storage" => rig.store.execution_available = false,
            _ => {
                // The directory the command would run in is now a link out of the workspace.
                fs::remove_dir(rig.root.join("ws/sub")).unwrap();
                symlink(&rig.root, rig.root.join("ws/sub")).unwrap();
            }
        }
        assert_eq!(
            rig.decide(&Rig::intent(&a, Accept)),
            Err(ControlError::GateDenied),
            "{fact}"
        );
        assert!(rig.answers().is_empty(), "{fact}");
        assert_eq!(rig.record(&a).state, ApprovalState::Pending, "{fact}");
    }
    // Stale revision and an unknown approval.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let mut stale = Rig::intent(&a, Accept);
    stale.expected_revision = a.revision + 1;
    assert_eq!(rig.decide(&stale), Err(ControlError::StaleRevision));
    let mut missing = Rig::intent(&a, Accept);
    missing.managed_approval_id =
        crate::contracts::identities::ManagedApprovalId::parse(uuid().as_str()).unwrap();
    assert_eq!(rig.decide(&missing), Err(ControlError::ApprovalNotFound));
    assert!(rig.answers().is_empty());
    // Config or safety drift: the runtime could not re-prove its profile.
    let reply = rig.respond(&Rig::intent(&a, Accept));
    assert_eq!(rig.attest(false), Verdict::Continue);
    assert_eq!(
        reply.try_recv().unwrap().unwrap_err(),
        ControlError::GateDenied
    );
    assert_eq!(rig.record(&a).state, ApprovalState::Pending);
    assert!(rig.answers().is_empty());
    // The generation ends because it could not attest: the approval ends with it.
    rig.turns
        .lost(&mut rig.store, TurnReason::RuntimeFailed)
        .unwrap();
    let lost = rig.record(&a);
    assert_eq!(
        (lost.state, lost.terminal_reason),
        (
            ApprovalState::Invalidated,
            Some(ApprovalReason::RuntimeLost)
        )
    );
    // The provider does not offer `accept` for this request: none is ever synthesized.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    rig.propose_command("call-1", "id", &cwd);
    rig.ask(
        ActionKind::Command,
        "req-1",
        "call-1",
        Some(("id", &cwd)),
        Offered {
            accept: false,
            decline: false,
            cancel: true,
        },
    );
    let a = rig.pending().unwrap();
    // It is shown, and shown as not acceptable.
    let shown = pending_status(&rig).unwrap();
    assert!(shown.command.is_some() && !shown.acceptable);
    assert_eq!(
        rig.decide(&Rig::intent(&a, Accept)),
        Err(ControlError::GateDenied)
    );
    assert!(rig.answers().is_empty());
}
#[test]
fn original_lease_scope_expiry_revocation_and_replacement_gate_accept() {
    // Missing scope on the ORIGINAL lease: the request is visible, accepting is impossible,
    // refusing still works.
    for (kind, scope) in [
        (ActionKind::Command, ExecutionScope::CommandExecute),
        (ActionKind::File, ExecutionScope::FileChange),
    ] {
        let scopes: Vec<_> = all_scopes().into_iter().filter(|s| *s != scope).collect();
        let mut rig = Rig::with(scopes, 3600, true);
        let a = if kind == ActionKind::Command {
            rig.command_approval()
        } else {
            rig.file_approval()
        };
        assert_eq!(
            rig.decide(&Rig::intent(&a, Accept)),
            Err(ControlError::GateDenied)
        );
        assert!(rig.answers().is_empty());
        assert_eq!(rig.record(&a).state, ApprovalState::Pending);
        let receipt = rig.decide(&Rig::intent(&a, Cancel)).unwrap();
        assert_eq!(receipt.state, ApprovalState::Cancelled);
        assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
    }
    // Revoked while pending: terminal at once, refused to the provider, never accepted.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    rig.revoke();
    rig.enforce();
    let revoked = rig.record(&a);
    assert_eq!(
        (revoked.state, revoked.terminal_reason),
        (
            ApprovalState::Invalidated,
            Some(ApprovalReason::LeaseRevoked)
        )
    );
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
    assert_eq!(
        rig.decide(&Rig::intent(&revoked, Accept)),
        Err(ControlError::InvalidTransition)
    );
    assert_eq!(
        rig.end_turn(TurnOutcome::Interrupted).terminal_reason,
        Some(TurnReason::LeaseRevoked)
    );
    // A replacement lease never transfers authority, even with every scope and even
    // before the core noticed the revocation.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let reply = rig.respond(&Rig::intent(&a, Accept));
    rig.revoke();
    let ResultValue::Lease(replacement) = rig.apply(Operation::LeaseGrant(LeaseGrant {
        intent_id: uuid(),
        session_id: rig.session.managed_session_id.clone(),
        expected_session_revision: 1,
        scopes: all_scopes(),
        ttl_seconds: 3600,
    })) else {
        panic!()
    };
    assert_ne!(replacement.lease_id, a.admitted_lease_id);
    assert_eq!(rig.attest(true), Verdict::Continue);
    assert_eq!(
        reply.try_recv().unwrap().unwrap_err(),
        ControlError::GateDenied
    );
    assert!(rig.answers().is_empty());
    // Renewing the SAME lease keeps the approval acceptable, within its own deadline.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let lease = rig.lease.clone();
    rig.apply(Operation::LeaseRenew(LeaseRenew {
        intent_id: uuid(),
        lease_id: lease.lease_id,
        expected_revision: lease.revision,
        ttl_seconds: 3600,
    }));
    rig.enforce();
    assert_eq!(rig.record(&a).expires_at, a.expires_at);
    rig.decide(&Rig::intent(&a, Accept)).unwrap();
    assert_eq!(rig.answers(), [ProviderDecision::Accept]);
}
#[test]
fn approval_ttl_expires_durably_refuses_the_provider_and_stops_the_turn() {
    let mut rig = Rig::new();
    let a = rig.command_approval();
    rig.advance(APPROVAL_TTL_SECONDS - 1);
    rig.enforce();
    assert_eq!(rig.record(&a).state, ApprovalState::Pending);
    assert!(rig.answers().is_empty());
    // An Accept in flight when the deadline passes decides nothing.
    let reply = rig.respond(&Rig::intent(&a, Accept));
    rig.advance(1);
    assert_eq!(rig.attest(true), Verdict::Continue);
    assert_eq!(
        reply.try_recv().unwrap().unwrap_err(),
        ControlError::GateDenied
    );
    rig.enforce();
    let expired = rig.record(&a);
    assert_eq!(
        (expired.state, expired.terminal_reason, expired.decision),
        (
            ApprovalState::Expired,
            Some(ApprovalReason::ApprovalTimeout),
            None
        )
    );
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
    assert!(rig.turns.approval_deadline().is_none());
    for decision in [Accept, Decline, Cancel] {
        assert_eq!(
            rig.decide(&Rig::intent(&expired, decision)),
            Err(ControlError::InvalidTransition)
        );
    }
    assert!(rig.answers().is_empty());
    let turn = rig.end_turn(TurnOutcome::Interrupted);
    assert_eq!(
        (turn.state, turn.terminal_reason),
        (TurnState::Interrupted, Some(TurnReason::ApprovalExpired))
    );
}
#[test]
fn accept_races_have_exactly_one_committed_outcome_and_at_most_one_answer() {
    // Accept against Accept.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let first = Rig::intent(&a, Accept);
    let second = Rig::intent(&a, Accept);
    let first_reply = rig.respond(&first);
    assert_eq!(
        rig.respond(&second).try_recv().unwrap().unwrap_err(),
        ControlError::RuntimeOperationInProgress
    );
    assert_eq!(
        rig.respond(&first).try_recv().unwrap().unwrap_err(),
        ControlError::RuntimeOperationInProgress
    );
    rig.attest(true);
    assert!(first_reply.try_recv().unwrap().is_ok());
    assert_eq!(rig.decide(&second), Err(ControlError::InvalidTransition));
    assert_eq!(rig.answers(), [ProviderDecision::Accept]);
    // A reductive decision wins against an Accept that is still being attested.
    for reductive in [Decline, Cancel] {
        let mut rig = Rig::new();
        let a = rig.command_approval();
        let accept = rig.respond(&Rig::intent(&a, Accept));
        let receipt = rig.decide(&Rig::intent(&a, reductive)).unwrap();
        assert_eq!(receipt.decision, reductive);
        assert!(matches!(
            rig.commands.try_recv(),
            Ok(RuntimeCommand::Attest { .. })
        ));
        assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
        rig.turns
            .attested(
                &mut rig.store,
                &mut rig.runtime,
                &rig.conversations,
                &request("req-1"),
                true,
            )
            .unwrap();
        assert_eq!(
            accept.try_recv().unwrap().unwrap_err(),
            ControlError::InvalidTransition
        );
        assert!(rig.answers().is_empty());
    }
    // Accept against lease revocation, turn cancel and session close.
    for rival in ["revoke", "cancel", "close", "expiry"] {
        let mut rig = Rig::with(
            all_scopes(),
            if rival == "expiry" { 30 } else { 3600 },
            true,
        );
        let a = rig.command_approval();
        let accept = rig.respond(&Rig::intent(&a, Accept));
        match rival {
            "revoke" => rig.revoke(),
            "expiry" => rig.advance(30),
            "close" => {
                let session = rig.session.managed_session_id.clone();
                rig.apply(Operation::Close(Close {
                    intent_id: uuid(),
                    managed_session_id: session,
                    expected_revision: 1,
                }));
            }
            _ => {
                let running = rig.store.turn(&rig.turn.managed_turn_id).unwrap();
                rig.turns
                    .cancel(
                        &mut rig.store,
                        &mut rig.runtime,
                        &TurnCancel {
                            intent_id: uuid(),
                            managed_turn_id: running.managed_turn_id,
                            expected_revision: running.revision,
                        },
                    )
                    .unwrap();
            }
        }
        rig.enforce();
        let terminal = rig.record(&a);
        assert!(terminal.state.terminal(), "{rival}");
        assert_eq!(terminal.decision, None, "{rival}");
        assert_eq!(
            terminal.terminal_reason,
            Some(match rival {
                "revoke" => ApprovalReason::LeaseRevoked,
                "expiry" => ApprovalReason::LeaseExpired,
                "close" => ApprovalReason::SessionClosed,
                _ => ApprovalReason::TurnCancelled,
            }),
            "{rival}"
        );
        assert_eq!(rig.attest(true), Verdict::Continue);
        assert_eq!(
            accept.try_recv().unwrap().unwrap_err(),
            ControlError::InvalidTransition,
            "{rival}"
        );
        // Exactly one answer left the authority, and it was the refusal.
        assert_eq!(rig.answers(), [ProviderDecision::Cancel], "{rival}");
    }
}
#[test]
fn public_decline_and_cancel_map_to_the_weakest_offered_provider_answer() {
    // Command: 0.159.0 offers no `decline`, so both public refusals are `cancel`.
    for decision in [Decline, Cancel] {
        let mut rig = Rig::new();
        let a = rig.command_approval();
        let p = Rig::intent(&a, decision);
        let receipt = rig.decide(&p).unwrap();
        assert_eq!(receipt.decision, decision);
        assert_eq!(rig.record(&a).effect, ApprovalEffect::None);
        assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
        assert_eq!(rig.decide(&p), Ok(receipt));
        assert!(rig.answers().is_empty());
        assert_eq!(
            rig.notice(NoticeKind::ItemEnded {
                output: None,
                item: item("call-1"),
                kind: ActionKind::Command,
                end: ItemEnd::Declined
            }),
            Verdict::Continue
        );
        let turn = rig.end_turn(TurnOutcome::Interrupted);
        assert_eq!(
            (turn.state, turn.terminal_reason),
            (TurnState::Interrupted, Some(TurnReason::UserCancel))
        );
    }
    // Even a command request that offers `decline` is refused with `cancel`: a refused
    // command never leaves its turn running.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    rig.propose_command("call-1", "id", &cwd);
    rig.ask(
        ActionKind::Command,
        "req-1",
        "call-1",
        Some(("id", &cwd)),
        FILE_OFFER,
    );
    let a = rig.pending().unwrap();
    rig.decide(&Rig::intent(&a, Decline)).unwrap();
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
    // File change: `decline` is offered and keeps the turn; `cancel` ends it.
    let mut rig = Rig::new();
    let a = rig.file_approval();
    rig.decide(&Rig::intent(&a, Decline)).unwrap();
    assert_eq!(rig.answers(), [ProviderDecision::Decline]);
    rig.notice(NoticeKind::ItemEnded {
        output: None,
        item: item("patch-1"),
        kind: ActionKind::File,
        end: ItemEnd::Declined,
    });
    rig.enforce();
    assert!(rig.answers().is_empty());
    assert_eq!(
        rig.store.turn(&rig.turn.managed_turn_id).unwrap().state,
        TurnState::Running
    );
    let mut rig = Rig::new();
    let a = rig.file_approval();
    rig.decide(&Rig::intent(&a, Cancel)).unwrap();
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
    // A file request that does not offer `decline` is refused with `cancel`.
    let mut rig = Rig::new();
    let target = rig.path("probe.txt");
    rig.propose_file(
        "patch-1",
        vec![change(&target, FileChangeKind::Add, None, "+x\n")],
    );
    rig.ask(ActionKind::File, "req-f", "patch-1", None, COMMAND_OFFER);
    let a = rig.pending().unwrap();
    rig.decide(&Rig::intent(&a, Decline)).unwrap();
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
    // Nothing reductive is offered: no answer is synthesized, the turn is interrupted.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    rig.propose_command("call-1", "id", &cwd);
    rig.ask(
        ActionKind::Command,
        "req-1",
        "call-1",
        Some(("id", &cwd)),
        Offered {
            accept: true,
            decline: false,
            cancel: false,
        },
    );
    let a = rig.pending().unwrap();
    assert_eq!(
        rig.decide(&Rig::intent(&a, Decline)).unwrap().state,
        ApprovalState::Declined
    );
    assert!(matches!(
        rig.commands.try_recv(),
        Ok(RuntimeCommand::Interrupt { .. })
    ));
    assert!(rig.commands.try_recv().is_err());
}
#[test]
fn refusing_needs_no_execution_authority() {
    // No scope, an inference-looking profile and a lost attachment: still refusable while
    // the exact approval is pending and its request is held.
    let scopes: Vec<_> = all_scopes()
        .into_iter()
        .filter(|s| *s != ExecutionScope::CommandExecute)
        .collect();
    let mut rig = Rig::with(scopes, 3600, true);
    let a = rig.command_approval();
    rig.conversations.detach_for_test();
    assert_eq!(
        rig.decide(&Rig::intent(&a, Accept)),
        Err(ControlError::GateDenied)
    );
    let receipt = rig.decide(&Rig::intent(&a, Decline)).unwrap();
    assert_eq!(receipt.state, ApprovalState::Declined);
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
}
#[test]
fn turn_cancel_session_close_and_shutdown_resolve_the_pending_approval_reductively() {
    for cause in ["cancel", "close", "shutdown"] {
        let mut rig = Rig::new();
        let a = rig.command_approval();
        match cause {
            "cancel" => {
                let running = rig.store.turn(&rig.turn.managed_turn_id).unwrap();
                rig.turns
                    .cancel(
                        &mut rig.store,
                        &mut rig.runtime,
                        &TurnCancel {
                            intent_id: uuid(),
                            managed_turn_id: running.managed_turn_id,
                            expected_revision: running.revision,
                        },
                    )
                    .unwrap();
            }
            "close" => {
                let session = rig.session.managed_session_id.clone();
                rig.apply(Operation::Close(Close {
                    intent_id: uuid(),
                    managed_session_id: session,
                    expected_revision: 1,
                }));
                rig.enforce();
            }
            _ => rig.turns.shutdown(&mut rig.store, &mut rig.runtime),
        }
        let settled = rig.record(&a);
        assert_eq!(settled.state, ApprovalState::Invalidated, "{cause}");
        assert_eq!(
            settled.terminal_reason,
            Some(match cause {
                "cancel" => ApprovalReason::TurnCancelled,
                "close" => ApprovalReason::SessionClosed,
                _ => ApprovalReason::AuthorityShutdown,
            })
        );
        // One reductive answer, no interrupt on top of it, and no Accept ever after.
        assert_eq!(rig.answers(), [ProviderDecision::Cancel], "{cause}");
        rig.enforce();
        assert!(rig.answers().is_empty(), "{cause}");
        assert_eq!(
            rig.decide(&Rig::intent(&settled, Accept)),
            Err(ControlError::InvalidTransition)
        );
        let turn = rig.end_turn(TurnOutcome::Interrupted);
        assert_eq!(turn.state, TurnState::Interrupted, "{cause}");
    }
}
#[test]
fn uncertain_accept_delivery_is_indeterminate_contained_and_never_resent() {
    // The supervisor reports that the Accept frame did not provably reach the provider.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let p = Rig::intent(&a, Accept);
    let receipt = rig.decide(&p).unwrap();
    assert_eq!(rig.answers(), [ProviderDecision::Accept]);
    // A report about any other request is not about this answer.
    assert_eq!(
        rig.turns
            .delivered(
                &mut rig.store,
                &request("another"),
                ProviderDecision::Accept,
                false
            )
            .unwrap(),
        Verdict::Continue
    );
    assert_eq!(
        rig.turns
            .delivered(
                &mut rig.store,
                &request("req-1"),
                ProviderDecision::Accept,
                false
            )
            .unwrap(),
        Verdict::Uncertain
    );
    let unknown = rig.record(&a);
    assert_eq!(
        (unknown.state, unknown.effect, unknown.terminal_reason),
        (
            ApprovalState::Indeterminate,
            ApprovalEffect::Unknown,
            Some(ApprovalReason::EffectUnknown)
        )
    );
    // The core contains: the turn is indeterminate. Nothing is ever sent again.
    rig.turns
        .lost(&mut rig.store, TurnReason::DispatchUnknown)
        .unwrap();
    assert_eq!(
        rig.store.turn(&rig.turn.managed_turn_id).unwrap().state,
        TurnState::Indeterminate
    );
    assert_eq!(rig.decide(&p), Ok(receipt));
    assert_eq!(
        rig.decide(&Rig::intent(&unknown, Accept)),
        Err(ControlError::InvalidTransition)
    );
    assert!(rig.answers().is_empty());

    // The decision committed, but the answer could not even be handed to the supervisor.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let reply = rig.respond(&Rig::intent(&a, Accept));
    let Ok(RuntimeCommand::Attest { request: held, .. }) = rig.commands.try_recv() else {
        panic!()
    };
    let (_closed, dead) = sync_channel::<RuntimeCommand>(1);
    drop(std::mem::replace(&mut rig.commands, dead));
    assert_eq!(
        rig.turns
            .attested(
                &mut rig.store,
                &mut rig.runtime,
                &rig.conversations,
                &held,
                true
            )
            .unwrap(),
        Verdict::Uncertain
    );
    assert!(reply.try_recv().unwrap().is_ok());
    assert_eq!(rig.record(&a).state, ApprovalState::Indeterminate);

    // The supervisor cannot be reached at all: the Accept is refused before any decision.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let (_closed, dead) = sync_channel::<RuntimeCommand>(1);
    drop(std::mem::replace(&mut rig.commands, dead));
    assert_eq!(
        rig.respond(&Rig::intent(&a, Accept))
            .try_recv()
            .unwrap()
            .unwrap_err(),
        ControlError::GateDenied
    );
    assert_eq!(rig.record(&a).state, ApprovalState::Pending);
}
#[test]
fn runtime_loss_and_authority_restart_never_replay_an_answer_or_an_action() {
    // Runtime lost while pending: invalidated.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    rig.turns
        .lost(&mut rig.store, TurnReason::RuntimeFailed)
        .unwrap();
    let lost = rig.record(&a);
    assert_eq!(
        (lost.state, lost.terminal_reason),
        (
            ApprovalState::Invalidated,
            Some(ApprovalReason::RuntimeLost)
        )
    );
    assert!(rig.answers().is_empty());
    // Runtime lost after the Accept was dispatched and before the provider reported.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    rig.decide(&Rig::intent(&a, Accept)).unwrap();
    rig.answers();
    rig.turns
        .lost(&mut rig.store, TurnReason::RuntimeFailed)
        .unwrap();
    let unknown = rig.record(&a);
    assert_eq!(
        (unknown.state, unknown.effect),
        (ApprovalState::Indeterminate, ApprovalEffect::Unknown)
    );
    assert!(rig.answers().is_empty());
    // Authority restart: the transient action is gone with the process.
    for accepted in [false, true] {
        let mut rig = Rig::new();
        let a = rig.command_approval();
        if accepted {
            rig.decide(&Rig::intent(&a, Accept)).unwrap();
        }
        rig.answers();
        let mut next = Store::open(&rig.root.join("db"), uuid()).unwrap();
        next.set_clock(rig.clock.clone());
        next.recover_approvals().unwrap();
        next.recover_turns().unwrap();
        let recovered = next.approval(&a.managed_approval_id).unwrap();
        assert_eq!(
            recovered.state,
            if accepted {
                ApprovalState::Indeterminate
            } else {
                ApprovalState::Invalidated
            }
        );
        assert_eq!(
            recovered.terminal_reason,
            Some(ApprovalReason::AuthorityRestarted)
        );
        assert_eq!(
            next.turn(&rig.turn.managed_turn_id).unwrap().state,
            TurnState::Indeterminate
        );
        // A fresh core holds no request: every decision is refused and nothing is sent.
        let (mut runtime, commands, _wake) =
            Runtime::ready_for_test(&mut next, &rig.instance, Some(rig.workspace.as_str()));
        let mut turns = Turns::default();
        for decision in [Accept, Decline, Cancel] {
            let (reply, result) = sync_channel(1);
            turns.respond(
                &mut next,
                &mut runtime,
                &Conversations::default(),
                &Rig::intent(&recovered, decision),
                reply,
            );
            assert_eq!(
                result.try_recv().unwrap().unwrap_err(),
                ControlError::InvalidTransition
            );
        }
        assert!(commands.try_recv().is_err());
        // Recovery is idempotent: a second start changes nothing.
        let head = next.status().unwrap().last_sequence;
        next.recover_approvals().unwrap();
        assert_eq!(next.status().unwrap().last_sequence, head);
    }
}
#[test]
fn command_bounds_and_workspace_confinement_refuse_before_any_approval_exists() {
    type Case = fn(&Rig) -> (String, String);
    let cases: Vec<(&str, Case)> = vec![
        ("oversized", |r| {
            ("x".repeat(MAX_COMMAND_BYTES + 1), r.workspace.clone())
        }),
        ("empty", |r| (String::new(), r.workspace.clone())),
        ("nul", |r| ("a\0b".into(), r.workspace.clone())),
        ("outside", |r| {
            ("id".into(), r.root.to_str().unwrap().into())
        }),
        ("root", |_| ("id".into(), "/".into())),
        ("dotdot", |r| ("id".into(), r.path("sub/.."))),
        ("escape", |r| ("id".into(), r.path("../ws"))),
        ("relative", |_| ("id".into(), "ws".into())),
        // A sibling whose name merely starts with the workspace path.
        ("prefix", |r| ("id".into(), format!("{}-evil", r.workspace))),
        ("symlink", |r| ("id".into(), r.path("link"))),
        ("through_symlink", |r| ("id".into(), r.path("link/inner"))),
        ("missing", |r| ("id".into(), r.path("absent"))),
        ("file", |r| ("id".into(), r.path("plain.txt"))),
        ("protected", |r| ("id".into(), r.path(".git"))),
        ("long_cwd", |r| {
            ("id".into(), r.path(&"d".repeat(MAX_CWD_BYTES)))
        }),
    ];
    for (name, case) in cases {
        let mut rig = Rig::new();
        fs::create_dir_all(rig.root.join("ws/sub")).unwrap();
        fs::create_dir_all(rig.root.join("ws/.git")).unwrap();
        fs::create_dir_all(rig.root.join("outside/inner")).unwrap();
        fs::create_dir(format!("{}-evil", rig.workspace)).unwrap();
        fs::write(rig.root.join("ws/plain.txt"), "x").unwrap();
        symlink(rig.root.join("outside"), rig.root.join("ws/link")).unwrap();
        let (command, cwd) = case(&rig);
        assert_eq!(
            rig.ask(
                ActionKind::Command,
                "req-1",
                "call-1",
                Some((&command, &cwd)),
                COMMAND_OFFER
            ),
            Verdict::Continue,
            "{name}"
        );
        // No approval capable of Accept ever existed, and the provider was refused.
        assert!(rig.pending().is_none(), "{name}");
        assert_eq!(
            rig.store
                .conn
                .query_row("SELECT count(*) FROM managed_approvals", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0,
            "{name}"
        );
        assert_eq!(rig.answers(), [ProviderDecision::Cancel], "{name}");
        assert_eq!(
            rig.end_turn(TurnOutcome::Interrupted).terminal_reason,
            Some(TurnReason::ActionRejected),
            "{name}"
        );
    }
    // Inside the workspace, including the workspace itself and a nested directory.
    for relative in ["", "sub", "sub/deeper"] {
        let mut rig = Rig::new();
        fs::create_dir_all(rig.root.join("ws/sub/deeper")).unwrap();
        let cwd = if relative.is_empty() {
            rig.workspace.clone()
        } else {
            rig.path(relative)
        };
        // A trailing slash and `.` components are the same directory.
        let spelled = format!("{cwd}/./");
        rig.ask(
            ActionKind::Command,
            "req-1",
            "call-1",
            Some(("id", &spelled)),
            COMMAND_OFFER,
        );
        assert!(rig.pending().is_some(), "{relative}");
        assert!(rig.answers().is_empty());
    }
    // More parsed actions than the bound, or one that is too long, is not an approval.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    rig.notice(NoticeKind::ApprovalRequested(ApprovalRequest {
        request: request("req-1"),
        item: item("call-1"),
        kind: ActionKind::Command,
        command: Some("id".into()),
        cwd: Some(cwd),
        actions: vec!["unknown".into(); MAX_PARSED_ACTIONS + 1],
        offered: COMMAND_OFFER,
    }));
    assert!(rig.pending().is_none());
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
}
#[test]
fn file_bounds_and_path_confinement_refuse_before_any_approval_exists() {
    use FileChangeKind::{Add, Delete, Move, Update};
    type Case = fn(&Rig) -> Vec<ProposedChange>;
    let refused: Vec<(&str, Case)> = vec![
        ("outside", |r| {
            vec![change(&format!("{}/x", r.root.display()), Add, None, "+")]
        }),
        ("dotdot", |r| {
            vec![change(&r.path("sub/../../x"), Add, None, "+")]
        }),
        ("relative", |_| vec![change("x.txt", Add, None, "+")]),
        ("prefix", |r| {
            vec![change(&format!("{}-evil/x", r.workspace), Add, None, "+")]
        }),
        ("symlink_leaf", |r| {
            vec![change(&r.path("link.txt"), Update, None, "@@")]
        }),
        ("symlink_parent", |r| {
            vec![change(&r.path("link/new.txt"), Add, None, "+")]
        }),
        ("hardlink", |r| {
            vec![change(&r.path("hard.txt"), Update, None, "@@")]
        }),
        ("codex_policy", |r| {
            vec![change(&r.path(".codex/config.toml"), Add, None, "+")]
        }),
        ("git_hooks", |r| {
            vec![change(&r.path(".git/hooks/pre-commit"), Add, None, "+")]
        }),
        ("nested_policy", |r| {
            vec![change(&r.path("sub/.codex/rules/a.rules"), Add, None, "+")]
        }),
        ("update_missing", |r| {
            vec![change(&r.path("absent.txt"), Update, None, "@@")]
        }),
        ("delete_missing", |r| {
            vec![change(&r.path("absent.txt"), Delete, None, "-")]
        }),
        ("update_directory", |r| {
            vec![change(&r.path("sub"), Update, None, "@@")]
        }),
        ("add_over_directory", |r| {
            vec![change(&r.path("sub"), Add, None, "+")]
        }),
        ("file_as_parent", |r| {
            vec![change(&r.path("real.txt/x"), Add, None, "+")]
        }),
        ("move_out", |r| {
            vec![change(
                &r.path("real.txt"),
                Move,
                Some(&format!("{}/x", r.root.display())),
                "@@",
            )]
        }),
        ("move_to_link", |r| {
            vec![change(
                &r.path("real.txt"),
                Move,
                Some(&r.path("link/x")),
                "@@",
            )]
        }),
        ("move_to_policy", |r| {
            vec![change(
                &r.path("real.txt"),
                Move,
                Some(&r.path(".codex/x")),
                "@@",
            )]
        }),
        ("move_missing_source", |r| {
            vec![change(
                &r.path("absent.txt"),
                Move,
                Some(&r.path("b.txt")),
                "@@",
            )]
        }),
        ("move_without_destination", |r| {
            vec![change(&r.path("real.txt"), Move, None, "@@")]
        }),
        ("destination_on_add", |r| {
            vec![change(&r.path("a.txt"), Add, Some(&r.path("b.txt")), "+")]
        }),
        ("duplicate", |r| {
            vec![
                change(&r.path("a.txt"), Add, None, "+"),
                change(&r.path("a.txt"), Add, None, "+"),
            ]
        }),
        ("one_escape_among_many", |r| {
            vec![
                change(&r.path("a.txt"), Add, None, "+"),
                change("/etc/passwd", Update, None, "@@"),
            ]
        }),
        ("too_many", |r| {
            (0..=MAX_FILES)
                .map(|i| change(&r.path(&format!("f{i}")), Add, None, "+"))
                .collect()
        }),
        ("diff_too_large", |r| {
            vec![change(
                &r.path("a.txt"),
                Add,
                None,
                &"+".repeat(MAX_FILE_DIFF_BYTES + 1),
            )]
        }),
        ("total_too_large", |r| {
            (0..3)
                .map(|i| {
                    change(
                        &r.path(&format!("f{i}")),
                        Add,
                        None,
                        &"+".repeat(MAX_FILE_DIFF_BYTES),
                    )
                })
                .collect()
        }),
    ];
    let layout = |rig: &Rig| {
        fs::create_dir_all(rig.root.join("ws/sub")).unwrap();
        fs::create_dir_all(rig.root.join("outside")).unwrap();
        fs::create_dir(format!("{}-evil", rig.workspace)).unwrap();
        fs::write(rig.root.join("ws/real.txt"), "x").unwrap();
        fs::write(rig.root.join("outside/secret.txt"), "x").unwrap();
        symlink(
            rig.root.join("outside/secret.txt"),
            rig.root.join("ws/link.txt"),
        )
        .unwrap();
        symlink(rig.root.join("outside"), rig.root.join("ws/link")).unwrap();
        fs::hard_link(
            rig.root.join("outside/secret.txt"),
            rig.root.join("ws/hard.txt"),
        )
        .unwrap();
    };
    for (name, case) in refused {
        let mut rig = Rig::new();
        layout(&rig);
        let changes = case(&rig);
        assert_eq!(
            rig.propose_file("patch-1", changes),
            Verdict::Continue,
            "{name}"
        );
        assert_eq!(
            rig.ask(ActionKind::File, "req-f", "patch-1", None, FILE_OFFER),
            Verdict::Continue,
            "{name}"
        );
        assert!(rig.pending().is_none(), "{name}");
        assert_eq!(rig.answers(), [ProviderDecision::Cancel], "{name}");
        // Nothing the authority did touched the files it refused to approve.
        assert_eq!(
            fs::read_to_string(rig.root.join("outside/secret.txt")).unwrap(),
            "x"
        );
        assert!(!rig.root.join("x").exists());
    }
    let accepted: Vec<(&str, Case)> = vec![
        ("add", |r| vec![change(&r.path("new.txt"), Add, None, "+")]),
        ("add_in_new_directories", |r| {
            vec![change(&r.path("a/b/c/new.txt"), Add, None, "+")]
        }),
        ("add_over_file", |r| {
            vec![change(&r.path("real.txt"), Add, None, "+")]
        }),
        ("update", |r| {
            vec![change(&r.path("real.txt"), Update, None, "@@")]
        }),
        ("delete", |r| {
            vec![change(&r.path("real.txt"), Delete, None, "-")]
        }),
        ("move", |r| {
            vec![change(
                &r.path("real.txt"),
                Move,
                Some(&r.path("sub/moved.txt")),
                "@@",
            )]
        }),
        ("many", |r| {
            (0..MAX_FILES)
                .map(|i| change(&r.path(&format!("f{i}")), Add, None, "+"))
                .collect()
        }),
    ];
    for (name, case) in accepted {
        let mut rig = Rig::new();
        layout(&rig);
        let changes = case(&rig);
        rig.propose_file("patch-1", changes);
        rig.ask(ActionKind::File, "req-f", "patch-1", None, FILE_OFFER);
        let a = rig.pending().unwrap_or_else(|| panic!("{name}"));
        assert!(rig.answers().is_empty(), "{name}");
        // Confinement is proven again at Accept: a destination swapped for a link since
        // the request is no longer acceptable.
        if name == "add" {
            symlink(
                rig.root.join("outside/secret.txt"),
                rig.root.join("ws/new.txt"),
            )
            .unwrap();
            assert_eq!(
                rig.decide(&Rig::intent(&a, Accept)),
                Err(ControlError::GateDenied)
            );
            assert!(rig.answers().is_empty());
        } else {
            rig.decide(&Rig::intent(&a, Accept)).unwrap();
            assert_eq!(rig.answers(), [ProviderDecision::Accept], "{name}");
            assert_eq!(rig.notice(NoticeKind::DiffUpdated), Verdict::Continue);
        }
    }
}
#[test]
fn no_command_output_path_or_diff_reaches_the_database_wal_or_audit() {
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    let command = format!("printf WYSS_SECRET_COMMAND_{}", uuid().as_str());
    let output = format!("WYSS_SECRET_OUTPUT_{}", uuid().as_str());
    rig.propose_command("call-1", &command, &cwd);
    rig.ask(
        ActionKind::Command,
        "req-1",
        "call-1",
        Some((&command, &cwd)),
        COMMAND_OFFER,
    );
    let a = rig.pending().unwrap();
    rig.decide(&Rig::intent(&a, Accept)).unwrap();
    rig.notice(NoticeKind::RequestResolved(request("req-1")));
    rig.notice(NoticeKind::Output {
        item: item("call-1"),
        kind: ActionKind::Command,
        delta: output.clone(),
    });
    rig.notice(NoticeKind::ItemEnded {
        output: None,
        item: item("call-1"),
        kind: ActionKind::Command,
        end: ItemEnd::Completed,
    });
    let diff = format!("+WYSS_SECRET_PATCH_{}\n", uuid().as_str());
    let name = format!("WYSS_SECRET_PATH_{}.txt", uuid().as_str());
    let target = rig.path(&name);
    rig.propose_file(
        "patch-1",
        vec![change(&target, FileChangeKind::Add, None, &diff)],
    );
    rig.ask(ActionKind::File, "req-f", "patch-1", None, FILE_OFFER);
    let b = rig.pending().unwrap();
    rig.decide(&Rig::intent(&b, Decline)).unwrap();
    rig.end_turn(TurnOutcome::Completed);
    let mut durable = Vec::new();
    for file in ["db", "db-wal", "db-shm"] {
        durable.extend(fs::read(rig.root.join(file)).unwrap_or_default());
    }
    for raw in [
        command.as_bytes(),
        output.as_bytes(),
        diff.trim().as_bytes(),
        name.as_bytes(),
        b"WYSS_SECRET",
    ] {
        assert!(
            !durable.windows(raw.len()).any(|w| w == raw),
            "transient action content is durable"
        );
    }
    // The fingerprints are the only trace of the two actions.
    for fingerprint in [&a.action_fingerprint, &b.action_fingerprint] {
        assert!(
            durable
                .windows(fingerprint.len())
                .any(|w| w == fingerprint.as_bytes())
        );
    }
    // The durable journal is consistent and reopens.
    let next = Store::open(&rig.root.join("db"), uuid()).unwrap();
    assert_eq!(
        next.approval(&a.managed_approval_id).unwrap().effect,
        ApprovalEffect::Confirmed
    );
    assert_eq!(
        next.approval(&b.managed_approval_id).unwrap().state,
        ApprovalState::Declined
    );
}
fn stream(rig: &Rig, after: Option<crate::contracts::Sequence>) -> TurnStreamPage {
    let ResultValue::TurnEvents(page) = rig
        .turns
        .events(
            &rig.store,
            &TurnEvents {
                managed_turn_id: rig.turn.managed_turn_id.clone(),
                after_sequence: after,
                limit: 50,
            },
        )
        .unwrap()
    else {
        panic!()
    };
    page
}
fn kinds(rig: &Rig) -> Vec<StreamKind> {
    stream(rig, None).events.iter().map(|e| e.kind).collect()
}
/// What a client actually receives: the result survives the control frame codec.
fn over_the_wire(result: ResultValue) -> ResultValue {
    let bytes = Response {
        request_id: uuid(),
        authority_instance_id: uuid(),
        outcome: Ok(result),
    }
    .encode()
    .expect("one control frame");
    assert!(bytes.len() <= MAX_FRAME);
    Response::parse(&bytes).unwrap().outcome.unwrap()
}
fn pending_status(rig: &Rig) -> Option<ApprovalStatus> {
    let result = rig
        .turns
        .approval_pending(
            &rig.store,
            &ApprovalPending {
                managed_turn_id: rig.turn.managed_turn_id.clone(),
            },
        )
        .unwrap();
    match over_the_wire(result) {
        ResultValue::PendingApproval(s) => s.map(|s| *s),
        _ => panic!(),
    }
}
fn status(
    rig: &Rig,
    a: &ManagedApprovalRecord,
    file_index: Option<u16>,
    offset: Option<u32>,
) -> ControlResult<ApprovalStatus> {
    let result = rig.turns.approval_status(
        &rig.store,
        &ApprovalGet {
            managed_approval_id: a.managed_approval_id.clone(),
            file_index,
            offset,
        },
    )?;
    match over_the_wire(result) {
        ResultValue::Approval(s) => Ok(*s),
        _ => panic!(),
    }
}
#[test]
fn approval_queries_present_the_held_action_and_only_metadata_once_it_is_gone() {
    let mut rig = Rig::new();
    assert!(pending_status(&rig).is_none());
    let a = rig.command_approval();
    let shown = pending_status(&rig).expect("pending approval");
    assert_eq!(shown.approval, a);
    assert!(shown.acceptable);
    let command = shown.command.as_ref().unwrap();
    assert_eq!(command.command, "printf WYSS_OK");
    assert_eq!(command.cwd, rig.workspace);
    assert_eq!(command.parsed_actions, ["unknown"]);
    assert!(shown.file_count.is_none() && shown.file.is_none());
    // The same approval by identity, any number of times, without changing anything.
    for _ in 0..3 {
        assert!(status(&rig, &a, None, None).unwrap() == shown);
    }
    assert_eq!(rig.record(&a), a);
    assert!(rig.answers().is_empty());
    // A command has no file pages; unknown identities are not found.
    assert_eq!(
        status(&rig, &a, Some(0), None).err(),
        Some(ControlError::InvalidRequest)
    );
    let mut unknown = a.clone();
    unknown.managed_approval_id =
        crate::contracts::identities::ManagedApprovalId::parse(uuid().as_str()).unwrap();
    assert_eq!(
        status(&rig, &unknown, None, None).err(),
        Some(ControlError::ApprovalNotFound)
    );
    assert_eq!(
        rig.turns
            .approval_pending(
                &rig.store,
                &ApprovalPending {
                    managed_turn_id: crate::contracts::identities::ManagedTurnId::parse(
                        uuid().as_str()
                    )
                    .unwrap()
                }
            )
            .err(),
        Some(ControlError::TurnNotFound)
    );
    // While an Accept is being attested the approval is still pending and still shown.
    let reply = rig.respond(&Rig::intent(&a, Accept));
    assert!(pending_status(&rig).unwrap().acceptable);
    rig.attest(true);
    assert!(reply.try_recv().unwrap().is_ok());
    // Decided: nothing is pending, and the status is canonical metadata only.
    assert!(pending_status(&rig).is_none());
    let decided = status(&rig, &a, None, None).unwrap();
    assert_eq!(decided.approval.state, ApprovalState::Accepted);
    assert!(!decided.acceptable && decided.command.is_none());
    // A turn that is being stopped still shows what it held, but not as acceptable.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    let reply = rig.respond(&Rig::intent(&a, Accept));
    assert!(reply.try_recv().is_err());
    rig.revoke();
    rig.enforce();
    assert!(pending_status(&rig).is_none());
    let revoked = status(&rig, &a, None, None).unwrap();
    assert_eq!(revoked.approval.state, ApprovalState::Invalidated);
    assert!(!revoked.acceptable && revoked.command.is_none());
    // The pending approval belongs to one turn: an earlier turn is waiting on nothing.
    let mut rig = Rig::new();
    let earlier = rig.turn.managed_turn_id.clone();
    rig.end_turn(TurnOutcome::Completed);
    rig.start_turn();
    let a = rig.command_approval();
    assert_eq!(pending_status(&rig).unwrap().approval, a);
    let ResultValue::PendingApproval(none) = rig
        .turns
        .approval_pending(
            &rig.store,
            &ApprovalPending {
                managed_turn_id: earlier,
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert!(none.is_none());
    // After an authority restart the action is gone with the process that held it.
    let mut rig = Rig::new();
    let a = rig.file_approval();
    let mut next = Store::open(&rig.root.join("db"), uuid()).unwrap();
    next.recover_approvals().unwrap();
    next.recover_turns().unwrap();
    let ResultValue::Approval(after) = Turns::default()
        .approval_status(
            &next,
            &ApprovalGet {
                managed_approval_id: a.managed_approval_id.clone(),
                file_index: Some(0),
                offset: Some(0),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(after.approval.state, ApprovalState::Invalidated);
    assert!(!after.acceptable && after.file_count.is_none() && after.file.is_none());
}
#[test]
fn file_approval_presents_the_complete_change_set_in_exact_pages() {
    let mut rig = Rig::new();
    fs::write(rig.root.join("ws/old.txt"), "x").unwrap();
    // Multibyte content, so that a page boundary would fall inside a character.
    let diff = "+ñ😃línea\n".repeat(2500);
    assert!(diff.len() > 2 * DIFF_PAGE_BYTES);
    let (b, old, moved) = (
        rig.path("b.txt"),
        rig.path("old.txt"),
        rig.path("sub/new.txt"),
    );
    rig.propose_file(
        "patch-1",
        vec![
            change(&old, FileChangeKind::Move, Some(&moved), "@@ move\n"),
            change(&b, FileChangeKind::Add, None, &diff),
        ],
    );
    rig.ask(ActionKind::File, "req-f", "patch-1", None, FILE_OFFER);
    let a = rig.pending().unwrap();
    let shown = pending_status(&rig).unwrap();
    assert_eq!(shown.file_count, Some(2));
    assert!(shown.command.is_none() && shown.acceptable);
    // Files are presented in path order, whatever order the provider listed them in.
    let first = shown.file.unwrap();
    assert_eq!((first.index, first.path.as_str()), (0, b.as_str()));
    assert_eq!(first.kind, FileChangeKind::Add);
    assert_eq!(first.diff_bytes as usize, diff.len());
    // Paging the diff reconstructs exactly what was fingerprinted.
    let mut rebuilt = String::new();
    let mut offset = Some(0);
    let mut pages = 0;
    while let Some(at) = offset {
        let page = status(&rig, &a, Some(0), Some(at)).unwrap().file.unwrap();
        assert!(page.chunk.len() <= DIFF_PAGE_BYTES && !page.chunk.is_empty());
        assert_eq!(page.offset, at);
        rebuilt.push_str(&page.chunk);
        offset = page.next_offset;
        pages += 1;
    }
    assert!(rebuilt == diff && pages >= 3);
    let second = status(&rig, &a, Some(1), None).unwrap().file.unwrap();
    assert_eq!(second.path, old);
    assert_eq!(second.kind, FileChangeKind::Move);
    assert_eq!(second.move_path.as_deref(), Some(moved.as_str()));
    assert_eq!(
        (second.chunk.as_str(), second.next_offset),
        ("@@ move\n", None)
    );
    // Out-of-range pages are refused, never clamped to some other content.
    for (index, at) in [
        (2, 0),
        (0, diff.len() as u32 + 1),
        (0, 2), // inside the two-byte `ñ`
    ] {
        assert_eq!(
            status(&rig, &a, Some(index), Some(at)).err(),
            Some(ControlError::InvalidRequest),
            "{index}/{at}"
        );
    }
    assert_eq!(rig.record(&a), a);
    // The presentation is the decision surface: accepting covers exactly those pages.
    rig.decide(&Rig::intent(&a, Accept)).unwrap();
    assert_eq!(rig.answers(), [ProviderDecision::Accept]);
    assert!(status(&rig, &a, Some(0), Some(0)).unwrap().file.is_none());
}
#[test]
fn command_that_cannot_be_presented_whole_is_never_approvable() {
    // The largest plain command fits one control frame with its directory and actions.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    let largest = "c".repeat(MAX_COMMAND_BYTES);
    rig.notice(NoticeKind::ApprovalRequested(ApprovalRequest {
        request: request("req-1"),
        item: item("call-1"),
        kind: ActionKind::Command,
        command: Some(largest.clone()),
        cwd: Some(cwd.clone()),
        actions: vec!["a".repeat(MAX_PARSED_ACTION_BYTES); MAX_PARSED_ACTIONS],
        offered: COMMAND_OFFER,
    }));
    let shown = pending_status(&rig).expect("presentable whole");
    assert!(shown.command.unwrap().command == largest);
    // A command whose encoding would overflow the frame is not truncated: it is refused.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    let unprintable = "\u{1}".repeat(MAX_COMMAND_BYTES);
    assert_eq!(
        rig.ask(
            ActionKind::Command,
            "req-1",
            "call-1",
            Some((&unprintable, &cwd)),
            COMMAND_OFFER
        ),
        Verdict::Continue
    );
    assert!(rig.pending().is_none());
    assert_eq!(rig.answers(), [ProviderDecision::Cancel]);
}
#[test]
fn stream_reports_the_mediated_lifecycle_in_order_and_never_the_action() {
    use StreamKind::*;
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    let secret = "printf WYSS_STREAM_SECRET_COMMAND";
    rig.propose_command("call-1", secret, &cwd);
    rig.ask(
        ActionKind::Command,
        "req-1",
        "call-1",
        Some((secret, &cwd)),
        COMMAND_OFFER,
    );
    let a = rig.pending().unwrap();
    assert_eq!(
        kinds(&rig),
        [TurnStarted, CommandProposed, ApprovalRequired]
    );
    rig.decide(&Rig::intent(&a, Accept)).unwrap();
    rig.answers();
    assert_eq!(
        rig.turns
            .delivered(
                &mut rig.store,
                &request("req-1"),
                ProviderDecision::Accept,
                true
            )
            .unwrap(),
        Verdict::Continue
    );
    rig.notice(NoticeKind::RequestResolved(request("req-1")));
    rig.notice(NoticeKind::Output {
        item: item("call-1"),
        kind: ActionKind::Command,
        delta: "WYSS_STREAM_OUTPUT".into(),
    });
    // The finished item repeats the output that was already streamed: it is shown once.
    rig.notice(NoticeKind::ItemEnded {
        output: Some("WYSS_STREAM_OUTPUT".into()),
        item: item("call-1"),
        kind: ActionKind::Command,
        end: ItemEnd::Failed,
    });
    // A file change in the same turn, declined.
    let target = rig.path("probe.txt");
    rig.propose_file(
        "patch-1",
        vec![change(
            &target,
            FileChangeKind::Add,
            None,
            "+WYSS_STREAM_SECRET_PATCH\n",
        )],
    );
    rig.propose_file(
        "patch-1",
        vec![change(
            &target,
            FileChangeKind::Add,
            None,
            "+WYSS_STREAM_SECRET_PATCH\n",
        )],
    );
    rig.ask(ActionKind::File, "req-f", "patch-1", None, FILE_OFFER);
    let b = rig.pending().unwrap();
    rig.decide(&Rig::intent(&b, Decline)).unwrap();
    assert_eq!(rig.answers(), [ProviderDecision::Decline]);
    rig.notice(NoticeKind::ItemEnded {
        output: None,
        item: item("patch-1"),
        kind: ActionKind::File,
        end: ItemEnd::Declined,
    });
    // And one that is accepted and applied.
    let other = rig.path("second.txt");
    rig.propose_file(
        "patch-2",
        vec![change(&other, FileChangeKind::Add, None, "+x\n")],
    );
    rig.ask(ActionKind::File, "req-g", "patch-2", None, FILE_OFFER);
    let c = rig.pending().unwrap();
    rig.decide(&Rig::intent(&c, Accept)).unwrap();
    rig.turns
        .delivered(
            &mut rig.store,
            &request("req-g"),
            ProviderDecision::Accept,
            true,
        )
        .unwrap();
    rig.notice(NoticeKind::Output {
        item: item("patch-2"),
        kind: ActionKind::File,
        delta: "Success".into(),
    });
    rig.notice(NoticeKind::ItemEnded {
        output: None,
        item: item("patch-2"),
        kind: ActionKind::File,
        end: ItemEnd::Completed,
    });
    rig.end_turn(TurnOutcome::Completed);
    let page = stream(&rig, None);
    assert_eq!(
        page.events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        [
            TurnStarted,
            CommandProposed,
            ApprovalRequired,
            ApprovalResolved,
            CommandStarted,
            CommandOutputDelta,
            CommandFailed,
            FileChangeProposed,
            ApprovalRequired,
            ApprovalResolved,
            FileChangeProposed,
            ApprovalRequired,
            ApprovalResolved,
            FileChangeStarted,
            FileChangeCompleted,
            TurnCompleted,
        ]
    );
    // One per-turn sequence, positive and strictly increasing.
    let sequences: Vec<u64> = page.events.iter().map(|e| e.sequence.get()).collect();
    assert_eq!(sequences, (1..=16).collect::<Vec<u64>>());
    assert!(page.terminal && !page.gap && page.dropped_events == 0);
    // Approval events name the approval and its state; nothing else carries either.
    let approvals: Vec<_> = page
        .events
        .iter()
        .filter(|e| e.managed_approval_id.is_some())
        .map(|e| {
            (
                e.kind,
                e.managed_approval_id.clone().unwrap(),
                e.text.clone(),
            )
        })
        .collect();
    assert_eq!(
        approvals,
        [
            (ApprovalRequired, a.managed_approval_id.clone(), None),
            (
                ApprovalResolved,
                a.managed_approval_id.clone(),
                Some("accepted".into())
            ),
            (ApprovalRequired, b.managed_approval_id.clone(), None),
            (
                ApprovalResolved,
                b.managed_approval_id.clone(),
                Some("declined".into())
            ),
            (ApprovalRequired, c.managed_approval_id.clone(), None),
            (
                ApprovalResolved,
                c.managed_approval_id.clone(),
                Some("accepted".into())
            ),
        ]
    );
    // The stream carries command output and never the command, the paths or the diff.
    let raw = serde_json::to_string(&page).unwrap();
    assert!(raw.contains("WYSS_STREAM_OUTPUT"));
    for absent in ["WYSS_STREAM_SECRET", "probe.txt", "second.txt", "Success"] {
        assert!(!raw.contains(absent), "{absent}");
    }
    // Reductive authority outcomes are announced with their state as well.
    for (cause, state) in [("expiry", "expired"), ("revoke", "invalidated")] {
        let mut rig = Rig::new();
        rig.command_approval();
        if cause == "expiry" {
            rig.advance(APPROVAL_TTL_SECONDS);
        } else {
            rig.revoke();
        }
        rig.enforce();
        let last = stream(&rig, None).events.pop().unwrap();
        assert_eq!(
            (last.kind, last.text.as_deref()),
            (ApprovalResolved, Some(state))
        );
    }
    // A one-shot command reports its output only when it finishes: shown then, once.
    let mut rig = Rig::new();
    let a = rig.command_approval();
    rig.decide(&Rig::intent(&a, Accept)).unwrap();
    rig.notice(NoticeKind::ItemEnded {
        output: Some("WYSS_FINAL_OUTPUT".into()),
        item: item("call-1"),
        kind: ActionKind::Command,
        end: ItemEnd::Completed,
    });
    let tail: Vec<_> = stream(&rig, None)
        .events
        .into_iter()
        .rev()
        .take(2)
        .map(|e| (e.kind, e.text))
        .collect();
    assert_eq!(
        tail,
        [
            (CommandCompleted, None),
            (CommandOutputDelta, Some("WYSS_FINAL_OUTPUT".to_owned())),
        ]
    );
    // A violation ends the stream with the approval invalidated and the turn failed.
    let mut rig = Rig::new();
    rig.command_approval();
    rig.turns
        .lost(&mut rig.store, TurnReason::InferenceIsolationViolation)
        .unwrap();
    let tail: Vec<_> = kinds(&rig).into_iter().rev().take(2).collect();
    assert_eq!(tail, [TurnFailed, ApprovalResolved]);
}
#[test]
fn command_output_is_chunked_on_characters_bounded_in_memory_and_reports_its_gap() {
    let mut rig = Rig::new();
    let a = rig.command_approval();
    rig.decide(&Rig::intent(&a, Accept)).unwrap();
    let before = stream(&rig, None).latest_sequence.unwrap();
    // One provider delta far above the per-event bound, made of multibyte characters.
    let output = "ñ😃".repeat(3000);
    assert_eq!(
        rig.notice(NoticeKind::Output {
            item: item("call-1"),
            kind: ActionKind::Command,
            delta: output.clone(),
        }),
        Verdict::Continue
    );
    let mut rebuilt = String::new();
    let mut after = Some(before);
    loop {
        let page = stream(&rig, after);
        if page.events.is_empty() {
            break;
        }
        for event in &page.events {
            assert_eq!(event.kind, StreamKind::CommandOutputDelta);
            let text = event.text.as_ref().unwrap();
            assert!(text.len() <= 4096);
            rebuilt.push_str(text);
        }
        after = page.next_sequence;
    }
    assert!(rebuilt == output);
    // A flood: memory stays bounded, the oldest events give way and the gap is explicit.
    let chunk = "o".repeat(4096);
    for _ in 0..200 {
        rig.notice(NoticeKind::Output {
            item: item("call-1"),
            kind: ActionKind::Command,
            delta: chunk.clone(),
        });
    }
    let page = stream(&rig, None);
    assert!(page.gap && page.dropped_events > 0);
    assert_eq!(page.events[0].kind, StreamKind::StreamGap);
    assert!(serde_json::to_vec(&page).unwrap().len() < MAX_FRAME);
    let mut retained = 0;
    let mut after = None;
    loop {
        let page = stream(&rig, after);
        let texts: usize = page
            .events
            .iter()
            .filter_map(|e| e.text.as_ref())
            .map(String::len)
            .sum();
        if page.events.iter().all(|e| e.kind == StreamKind::StreamGap) {
            break;
        }
        retained += texts;
        after = page.next_sequence;
    }
    assert!(
        retained <= 256 * 1024,
        "retained {retained} bytes of output"
    );
    assert!(retained > 128 * 1024);
    // Whatever was dropped, the sequence never goes backwards and never repeats.
    let latest = stream(&rig, None).latest_sequence.unwrap().get();
    assert!(latest > 200);
    // Raw output is never durable.
    let mut durable = Vec::new();
    for file in ["db", "db-wal", "db-shm"] {
        durable.extend(fs::read(rig.root.join(file)).unwrap_or_default());
    }
    assert!(!durable.windows(64).any(|w| w == &chunk.as_bytes()[..64]));
}
#[test]
fn a_stopping_turn_has_no_approval_deadline_left_to_wake_the_core_for() {
    // The provider offers no `cancel`, so an expired approval cannot be answered: the turn
    // is interrupted and its held request stays unanswered until the provider ends it. From
    // then on only the turn's own stop deadline may wake the core. A deadline that already
    // passed would make every wait zero and spin the single writer until the turn ends.
    let no_cancel = Offered {
        accept: true,
        decline: true,
        cancel: false,
    };
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    assert_eq!(
        rig.propose_command("call-1", "printf WYSS_OK", &cwd),
        Verdict::Continue
    );
    assert_eq!(
        rig.ask(
            ActionKind::Command,
            "req-1",
            "call-1",
            Some(("printf WYSS_OK", &cwd)),
            no_cancel
        ),
        Verdict::Continue
    );
    let approval = rig.pending().expect("pending approval");
    let deadline = rig.turns.approval_deadline().expect("approval deadline");
    assert_eq!(deadline, timestamp(&approval.expires_at).unwrap());
    rig.advance(APPROVAL_TTL_SECONDS + 1);
    rig.enforce();
    assert_eq!(rig.record(&approval).state, ApprovalState::Expired);
    assert!(matches!(
        rig.commands.try_recv(),
        Ok(RuntimeCommand::Interrupt { .. })
    ));
    assert!(rig.turns.approval_deadline().is_none());
    assert!(rig.turns.wait().is_some_and(|left| !left.is_zero()));
    // Enforcing again changes nothing and still leaves no passed deadline behind.
    rig.enforce();
    assert!(rig.commands.try_recv().is_err());
    assert!(rig.turns.approval_deadline().is_none());

    // The same for a file request whose change set never arrived.
    let mut rig = Rig::new();
    assert_eq!(
        rig.ask(ActionKind::File, "req-f", "patch-1", None, no_cancel),
        Verdict::Continue
    );
    assert!(rig.pending().is_none());
    assert!(rig.turns.approval_deadline().is_some());
    rig.advance(crate::control::mediation::PROPOSAL_WAIT_SECONDS + 1);
    rig.enforce();
    assert!(matches!(
        rig.commands.try_recv(),
        Ok(RuntimeCommand::Interrupt { .. })
    ));
    assert!(rig.turns.approval_deadline().is_none());
    assert!(rig.turns.wait().is_some_and(|left| !left.is_zero()));
}
#[test]
fn a_cancel_request_is_audited_at_the_time_it_was_made() {
    let mut rig = Rig::new();
    let last_event = |rig: &Rig| -> serde_json::Value {
        let raw: String = rig
            .store
            .conn
            .query_row(
                "SELECT envelope FROM control_events ORDER BY sequence DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        serde_json::from_str(&raw).unwrap()
    };
    let turn = rig.store.turn(&rig.turn.managed_turn_id).unwrap();
    assert_eq!(turn.state, TurnState::Running);
    // The turn was last updated when it started running; the request comes later.
    rig.advance(90);
    let requested_at = rig.store.lease_now();
    let cancel = TurnCancel {
        intent_id: uuid(),
        managed_turn_id: turn.managed_turn_id.clone(),
        expected_revision: turn.revision,
    };
    let ResultValue::Turn(receipt) = rig
        .turns
        .cancel(&mut rig.store, &mut rig.runtime, &cancel)
        .unwrap()
    else {
        panic!()
    };
    // The intent changes no turn state: the receipt is the turn as it was.
    assert_eq!(receipt, turn);
    let event = last_event(&rig);
    assert_eq!(
        event["event_type"],
        "authority.managed_turn.cancel_requested"
    );
    assert_eq!(event["payload"]["revision"], turn.revision);
    let stamped = timestamp(&serde_json::from_value(event["timestamp"].clone()).unwrap()).unwrap();
    assert_eq!(stamped, requested_at);
    assert_eq!(
        stamped - timestamp(&turn.updated_at).unwrap(),
        time::Duration::seconds(90)
    );
    // The same intent returns the same receipt and records nothing again.
    let events = rig.store.head().unwrap();
    rig.advance(30);
    let ResultValue::Turn(again) = rig
        .turns
        .cancel(&mut rig.store, &mut rig.runtime, &cancel)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(again, turn);
    assert_eq!(rig.store.head().unwrap(), events);
    // A clock that stepped back never dates the request before the turn it is about.
    rig.advance(-3600);
    rig.turns
        .cancel(
            &mut rig.store,
            &mut rig.runtime,
            &TurnCancel {
                intent_id: uuid(),
                ..cancel.clone()
            },
        )
        .unwrap();
    let event = last_event(&rig);
    assert_eq!(
        event["event_type"],
        "authority.managed_turn.cancel_requested"
    );
    assert_eq!(event["timestamp"], json!(turn.updated_at));
    // The stream it leaves is one the authority accepts on its next start.
    assert!(Store::open(&rig.root.join("db"), uuid()).is_ok());
}
#[test]
fn an_uncertain_create_that_cannot_be_audited_stays_blocked_uncertain_and_closed() {
    let mut rig = Rig::new();
    let ResultValue::Session(session) = rig.apply(Operation::Prepare(Prepare {
        intent_id: uuid(),
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: RuntimeTarget::codex().runtime_kind,
    })) else {
        panic!()
    };
    rig.apply(Operation::LeaseGrant(LeaseGrant {
        intent_id: uuid(),
        session_id: session.managed_session_id.clone(),
        expected_session_revision: 1,
        scopes: all_scopes(),
        ttl_seconds: 3600,
    }));
    let create = |rig: &Rig| {
        Operation::BindingCreate(BindingCreate {
            intent_id: uuid(),
            managed_session_id: session.managed_session_id.clone(),
            expected_session_revision: 1,
            workspace_root: rig.workspace.clone(),
        })
    };
    let begin = |rig: &mut Rig, op: &Operation| {
        let (reply, result) = sync_channel(1);
        rig.conversations
            .begin(&mut rig.store, &mut rig.runtime, op, reply);
        result
    };
    let status = |rig: &Rig| {
        rig.conversations
            .projection(&rig.store, &rig.runtime, &session.managed_session_id)
            .unwrap()
    };
    let first = create(&rig);
    let pending = begin(&mut rig, &first);
    assert!(matches!(
        rig.commands.try_recv(),
        Ok(RuntimeCommand::Conversation(_))
    ));
    // Dispatched and still awaited: this is the one state that is honestly "creating".
    assert!(pending.try_recv().is_err());
    let open = status(&rig);
    assert_eq!(open.state, BindingState::Creating);
    let operation = open.operation_id.clone().expect("open operation");
    let head = rig.store.head().unwrap();
    // The runtime is lost and the audit log refuses the uncertainty event.
    rig.store
        .conn
        .execute_batch("CREATE TEMP TRIGGER injected BEFORE INSERT ON control_events BEGIN SELECT RAISE(ABORT,'injected'); END")
        .unwrap();
    rig.conversations.detach(&mut rig.store);
    assert_eq!(
        pending.try_recv().unwrap().unwrap_err(),
        ControlError::ExternalEffectUnknown
    );
    // Nothing was recorded, so the durable journal still holds the dispatch marker. The
    // uncertainty is not lost: the session reports it, and execution is closed.
    assert_eq!(rig.store.head().unwrap(), head);
    assert!(!rig.store.execution_available);
    let stuck = status(&rig);
    assert_eq!(stuck.state, BindingState::Indeterminate);
    assert_eq!(stuck.operation_id.as_ref(), Some(&operation));
    assert!(stuck.binding.is_none());
    // No second create, no retry of the first, and no admission of anything at all.
    for op in [create(&rig), first.clone()] {
        let refused = begin(&mut rig, &op);
        assert!(matches!(
            refused.try_recv().unwrap(),
            Err(ControlError::RuntimeOperationInProgress | ControlError::GateDenied)
        ));
    }
    assert!(rig.commands.try_recv().is_err());
    for scope in all_scopes() {
        assert_eq!(
            crate::control::gate::ExecutionGate::evaluate(
                &rig.store,
                &session.managed_session_id,
                scope,
                &rig.runtime.status()
            )
            .reason(),
            GateReason::AuthorityUnavailable
        );
    }
    // It cannot be abandoned as long as it was never declared uncertain.
    let abandon = BindingAbandon {
        intent_id: uuid(),
        managed_session_id: session.managed_session_id.clone(),
        operation_id: operation.clone(),
    };
    assert_eq!(
        rig.store.abandon_binding(&abandon).unwrap_err(),
        ControlError::InvalidTransition
    );
    // The explicit recovery path is the next authority start: it declares the operation
    // uncertain with its audit event, and only then can the client abandon it.
    rig.store
        .conn
        .execute_batch("DROP TRIGGER injected")
        .unwrap();
    let mut next = Store::open(&rig.root.join("db"), uuid()).unwrap();
    next.recover_binding_operations().unwrap();
    assert_eq!(next.head().unwrap(), head + 1);
    let recovered = next.binding_status(&session.managed_session_id).unwrap();
    assert_eq!(recovered.state, BindingState::Indeterminate);
    assert_eq!(recovered.operation_id.as_ref(), Some(&operation));
    assert!(matches!(
        next.reserve_binding(&create(&rig)),
        Err(ControlError::BindingIndeterminate)
    ));
    assert!(matches!(
        next.abandon_binding(&abandon).unwrap(),
        ResultValue::BindingAbandoned(_)
    ));
    assert_eq!(
        next.binding_status(&session.managed_session_id)
            .unwrap()
            .state,
        BindingState::Unbound
    );
}
#[test]
fn admission_is_withdrawn_before_a_revocation_or_close_of_the_turn_commits_and_a_stop_is_refused() {
    use std::sync::atomic::Ordering;
    let mut rig = Rig::new();
    rig.end_turn(TurnOutcome::Completed);
    // A turn that was admitted and whose prompt the supervisor has not written yet. The
    // supervisor reads this flag immediately before the write.
    let ResultValue::Turn(turn) = rig
        .turns
        .start(
            &mut rig.store,
            &mut rig.runtime,
            &rig.conversations,
            &TurnStart {
                intent_id: uuid(),
                managed_session_id: rig.session.managed_session_id.clone(),
                expected_session_revision: 1,
                prompt: "another thing".into(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    let Ok(RuntimeCommand::Start(command)) = rig.commands.try_recv() else {
        panic!("the turn was not handed to the supervisor")
    };
    let admitted = || command.admission.load(Ordering::Acquire);
    assert!(admitted());
    let revoke = |lease: &ExecutionLease| {
        Operation::LeaseRevoke(LeaseRevoke {
            intent_id: uuid(),
            lease_id: lease.lease_id.clone(),
            expected_revision: lease.revision,
        })
    };
    let close = |session: &ManagedSessionRecord| {
        Operation::Close(Close {
            intent_id: uuid(),
            managed_session_id: session.managed_session_id.clone(),
            expected_revision: 1,
        })
    };
    // Nothing that concerns another lease, another session or no authority at all.
    let ResultValue::Session(other) = rig.apply(Operation::Prepare(Prepare {
        intent_id: uuid(),
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: RuntimeTarget::codex().runtime_kind,
    })) else {
        panic!()
    };
    let ResultValue::Lease(other_lease) = rig.apply(Operation::LeaseGrant(LeaseGrant {
        intent_id: uuid(),
        session_id: other.managed_session_id.clone(),
        expected_session_revision: 1,
        scopes: all_scopes(),
        ttl_seconds: 600,
    })) else {
        panic!()
    };
    for unrelated in [
        revoke(&other_lease),
        close(&other),
        Operation::LeaseRenew(LeaseRenew {
            intent_id: uuid(),
            lease_id: rig.lease.lease_id.clone(),
            expected_revision: rig.lease.revision,
            ttl_seconds: 600,
        }),
        Operation::Status(Empty {}),
    ] {
        assert!(!rig.turns.suspend_admission(&unrelated));
        assert!(admitted());
    }
    // A stop of the runtime the turn runs on is not one of them: it is refused before it
    // could take anything, so it withdraws nothing, however often it is asked.
    let stop = Operation::RuntimeStop(RuntimeMutation {
        intent_id: uuid(),
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: RuntimeTarget::codex().runtime_kind,
        expected_revision: None,
        safety_profile: None,
        workspace_root: None,
    });
    let shouting = Operation::LeaseRevoke(LeaseRevoke {
        intent_id: uuid(),
        lease_id: serde_json::from_value(json!(rig.lease.lease_id.as_str().to_ascii_uppercase()))
            .unwrap(),
        expected_revision: rig.lease.revision,
    });
    for _ in 0..2 {
        assert_eq!(
            rig.turns.admit_stop(&rig.store),
            Err(ControlError::ActiveTurnExists)
        );
        assert!(!rig.turns.suspend_admission(&stop));
        assert!(admitted());
    }
    // The turn's own lease and its own session: withdrawn before the commit, and given
    // back only when the mutation turned out to change nothing.
    for own in [revoke(&rig.lease), shouting, close(&rig.session)] {
        assert!(rig.turns.suspend_admission(&own));
        assert!(!admitted());
        // Already withdrawn: a second mutation in flight withdraws nothing more.
        assert!(!rig.turns.suspend_admission(&own));
        rig.turns.restore_admission();
        assert!(admitted());
    }
    // Once the turn is being stopped its admission never comes back.
    let lease = rig.lease.clone();
    assert!(rig.turns.suspend_admission(&revoke(&lease)));
    rig.revoke();
    rig.enforce();
    assert!(!admitted());
    rig.turns.restore_admission();
    assert!(!admitted());
    assert!(!rig.turns.suspend_admission(&revoke(&lease)));
    // The supervisor, reading the flag, never writes the prompt: the turn ends undispatched.
    assert_eq!(
        rig.turns
            .accepted(
                &mut rig.store,
                &mut rig.runtime,
                &rig.instance,
                &turn.managed_turn_id,
                Err(crate::control::runtime::ConversationFailure::NotDispatched),
            )
            .unwrap(),
        Verdict::Continue
    );
    let ended = rig.store.turn(&turn.managed_turn_id).unwrap();
    assert_eq!(
        (ended.state, ended.terminal_reason),
        (TurnState::Interrupted, Some(TurnReason::LeaseRevoked))
    );
    assert!(ended.codex_turn_id.is_none());
    // The turn ended: a stop is admitted again.
    assert_eq!(rig.turns.admit_stop(&rig.store), Ok(()));
}
#[test]
fn a_binding_request_in_flight_gets_back_the_admission_a_mutation_that_changed_nothing_took() {
    use std::sync::atomic::Ordering;
    let mut rig = Rig::new();
    rig.end_turn(TurnOutcome::Completed);
    // Nothing is in flight: there is nothing to withdraw and nothing to give back.
    assert!(!rig.conversations.suspend_admission());
    rig.conversations.restore_admission();
    // Another session asks to be bound. The request is handed to the supervisor, which
    // reads this flag immediately before it writes to the provider.
    let ResultValue::Session(other) = rig.apply(Operation::Prepare(Prepare {
        intent_id: uuid(),
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: RuntimeTarget::codex().runtime_kind,
    })) else {
        panic!()
    };
    rig.apply(Operation::LeaseGrant(LeaseGrant {
        intent_id: uuid(),
        session_id: other.managed_session_id.clone(),
        expected_session_revision: 1,
        scopes: all_scopes(),
        ttl_seconds: 600,
    }));
    let (reply, answer) = sync_channel(1);
    rig.conversations.begin(
        &mut rig.store,
        &mut rig.runtime,
        &Operation::BindingCreate(BindingCreate {
            intent_id: uuid(),
            managed_session_id: other.managed_session_id.clone(),
            expected_session_revision: 1,
            workspace_root: rig.workspace.clone(),
        }),
        reply,
    );
    assert!(answer.try_recv().is_err(), "the request was answered early");
    let Ok(RuntimeCommand::Conversation(command)) = rig.commands.try_recv() else {
        panic!("the request was not handed to the supervisor")
    };
    let admitted = || command.admission.load(Ordering::Acquire);
    assert!(admitted());
    // A mutation that may take its authority away withdraws it before it commits, once.
    assert!(rig.conversations.suspend_admission());
    assert!(!admitted());
    assert!(!rig.conversations.suspend_admission());
    // It changed nothing: the request is admitted as it was.
    rig.conversations.restore_admission();
    assert!(admitted());
    // One that did change something leaves it withdrawn, and a later one that changes
    // nothing has nothing of its own to give back.
    assert!(rig.conversations.suspend_admission());
    assert!(!rig.conversations.suspend_admission());
    assert!(!admitted());
    // A stop that committed ends the request: nothing brings its admission back.
    rig.conversations.detach(&mut rig.store);
    assert!(matches!(
        answer.try_recv(),
        Ok(Err(ControlError::ExternalEffectUnknown))
    ));
    rig.conversations.restore_admission();
    assert!(!admitted());
    assert!(!rig.conversations.suspend_admission());
}
#[test]
fn a_runtime_stop_is_admitted_only_while_no_managed_turn_is_active() {
    let refused = Err(ControlError::ActiveTurnExists);
    // Running, from the moment the provider named the turn.
    let mut rig = Rig::new();
    assert_eq!(
        rig.store.turn(&rig.turn.managed_turn_id).unwrap().state,
        TurnState::Running
    );
    assert_eq!(rig.turns.admit_stop(&rig.store), refused);
    // Waiting on an approval is running.
    let approval = rig.command_approval();
    assert_eq!(approval.state, ApprovalState::Pending);
    assert_eq!(rig.turns.admit_stop(&rig.store), refused);
    // A cancel that was asked for has not ended the turn yet: it is active until the
    // provider, or the deadline, says so.
    let running = rig.store.turn(&rig.turn.managed_turn_id).unwrap();
    rig.turns
        .cancel(
            &mut rig.store,
            &mut rig.runtime,
            &TurnCancel {
                intent_id: uuid(),
                managed_turn_id: running.managed_turn_id.clone(),
                expected_revision: running.revision,
            },
        )
        .unwrap();
    assert_eq!(
        rig.store.turn(&running.managed_turn_id).unwrap().state,
        TurnState::Running
    );
    assert_eq!(rig.turns.admit_stop(&rig.store), refused);
    // The refusals changed nothing: the turn is the one it was, at the same revision.
    assert_eq!(
        rig.store.turn(&running.managed_turn_id).unwrap().revision,
        running.revision
    );
    // Ended, in whatever way: nothing is left to protect.
    let ended = rig.end_turn(TurnOutcome::Interrupted);
    assert_eq!(ended.state, TurnState::Interrupted);
    assert_eq!(rig.turns.admit_stop(&rig.store), Ok(()));

    // Admitted and not yet written to the provider is active as well.
    let ResultValue::Turn(starting) = rig
        .turns
        .start(
            &mut rig.store,
            &mut rig.runtime,
            &rig.conversations,
            &TurnStart {
                intent_id: uuid(),
                managed_session_id: rig.session.managed_session_id.clone(),
                expected_session_revision: 1,
                prompt: "one more".into(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(starting.state, TurnState::Starting);
    assert_eq!(rig.turns.admit_stop(&rig.store), refused);
    // The durable record alone is enough: a core that holds no turn of its own does not
    // admit a stop over one the store still records as not ended.
    assert_eq!(Turns::default().admit_stop(&rig.store), refused);
    for outcome in [TurnOutcome::Completed, TurnOutcome::Failed] {
        let mut rig = Rig::new();
        assert_eq!(rig.turns.admit_stop(&rig.store), refused);
        rig.end_turn(outcome);
        assert_eq!(rig.turns.admit_stop(&rig.store), Ok(()));
        assert_eq!(Turns::default().admit_stop(&rig.store), Ok(()));
    }
}
#[test]
fn a_turn_is_admitted_only_at_the_session_revision_the_client_saw() {
    let mut rig = Rig::new();
    rig.end_turn(TurnOutcome::Completed);
    let head = rig.store.head().unwrap();
    let start = |rig: &mut Rig, revision: u64| {
        rig.turns.start(
            &mut rig.store,
            &mut rig.runtime,
            &rig.conversations,
            &TurnStart {
                intent_id: uuid(),
                managed_session_id: rig.session.managed_session_id.clone(),
                expected_session_revision: revision,
                prompt: "one more thing".into(),
            },
        )
    };
    // The session is prepared at revision 1. A client that expects any other revision
    // has not seen this session: nothing is admitted, recorded or dispatched.
    for stale in [2, 3] {
        assert_eq!(
            start(&mut rig, stale).unwrap_err(),
            ControlError::StaleRevision
        );
        assert_eq!(rig.store.head().unwrap(), head);
        assert!(rig.store.active_turn().unwrap().is_none());
        assert!(rig.commands.try_recv().is_err());
    }
    assert!(matches!(start(&mut rig, 1), Ok(ResultValue::Turn(_))));
    assert!(matches!(
        rig.commands.try_recv(),
        Ok(RuntimeCommand::Start(_))
    ));
}

fn reroute_frame(extra: serde_json::Value) -> serde_json::Value {
    let mut params = json!({
        "threadId": "opaque-durable-thread", "turnId": "provider-turn-1",
        "fromModel": "model-a", "toModel": "model-b", "reason": "highRiskCyberActivity",
    });
    for (k, v) in extra.as_object().unwrap() {
        if v.is_null() {
            params.as_object_mut().unwrap().remove(k);
        } else {
            params[k] = v.clone();
        }
    }
    json!({"method": "model/rerouted", "params": params})
}

#[test]
fn a_reroute_is_tolerated_only_when_it_is_exactly_the_schema_and_in_both_profiles() {
    use crate::contracts::binding::RerouteReason;
    for local in [false, true] {
        let classify = |v: serde_json::Value| {
            let m = frame(v);
            if local {
                safety::local_notification(&m)
            } else {
                safety::notification(&m)
            }
        };
        let Ok(Some(TurnNotice {
            kind: NoticeKind::Rerouted(r),
            thread,
            turn,
        })) = classify(reroute_frame(json!({})))
        else {
            panic!("not accepted (local={local})")
        };
        assert_eq!((r.from.as_str(), r.to.as_str()), ("model-a", "model-b"));
        assert_eq!(r.reason, RerouteReason::HighRiskCyberActivity);
        assert_eq!(
            (thread.as_str(), turn.as_str()),
            ("opaque-durable-thread", "provider-turn-1")
        );
        // An unknown reason is a closed word, never the provider's text, and never a failure.
        let Ok(Some(TurnNotice {
            kind: NoticeKind::Rerouted(r),
            ..
        })) = classify(reroute_frame(json!({"reason": "somethingNew"})))
        else {
            panic!()
        };
        assert_eq!(r.reason, RerouteReason::Unrecognised);
        // Everything else about it is the isolation violation it has always been.
        for bad in [
            json!({"extra": "field"}),
            json!({"reason": null}),
            json!({"fromModel": null}),
            json!({"toModel": null}),
            json!({"threadId": null}),
            json!({"turnId": null}),
            json!({"fromModel": ""}),
            json!({"toModel": "m".repeat(257)}),
            json!({"toModel": "has space"}),
            json!({"fromModel": "line\nbreak"}),
            json!({"toModel": 7}),
            json!({"reason": ["highRiskCyberActivity"]}),
            json!({"reason": "r".repeat(65)}),
            json!({"threadId": 5}),
            json!({"turnId": ""}),
        ] {
            assert!(
                classify(reroute_frame(bad.clone()))
                    .is_err_and(|e| e.code == "inference_isolation_violation"),
                "local={local}: {bad}"
            );
        }
        // And nothing next to it is let through by it: an unknown notification still is not.
        assert!(
            classify(json!({"method": "model/somethingElse", "params": {}}))
                .is_err_and(|e| e.code == "inference_isolation_violation")
        );
    }
}

#[test]
fn a_reroute_of_the_running_turn_is_recorded_in_memory_and_every_other_one_is_a_violation() {
    use crate::contracts::binding::{ObservedModel, ObservedModelSource, RerouteReason};
    let mut rig = Rig::new();
    let reroute = |from: &str, to: &str| {
        NoticeKind::Rerouted(safety::Reroute {
            from: from.into(),
            to: to.into(),
            reason: RerouteReason::HighRiskCyberActivity,
        })
    };
    // The session was attached with what its thread answer reported.
    rig.conversations.attach_with_model_for_test(
        &rig.session.managed_session_id,
        &rig.instance,
        ObservedModel {
            model_id: "model-a".into(),
            provider_id: Some("custom".into()),
            reasoning_effort: Some("high".into()),
            source: ObservedModelSource::ThreadStart,
            rerouted_from: None,
            reroute_reason: None,
            rerouted_turn: None,
        },
    );
    // Another thread, another turn: a violation, and nothing is recorded.
    for (thread, turn) in [
        ("another-thread", "provider-turn-1"),
        ("opaque-durable-thread", "another-turn"),
    ] {
        let verdict = rig
            .turns
            .notice(
                &mut rig.store,
                &mut rig.runtime,
                &rig.instance.clone(),
                TurnNotice {
                    thread: CodexThreadId::parse(thread).unwrap(),
                    turn: CodexTurnId::parse(turn).unwrap(),
                    kind: reroute("model-a", "model-b"),
                },
            )
            .unwrap();
        assert_eq!(verdict, Verdict::Violation, "{thread}/{turn}");
        assert!(rig.turns.take_reroute().is_none());
    }
    // The running turn: accepted, the turn goes on, and the controller hands it over once.
    assert_eq!(rig.notice(reroute("model-a", "model-b")), Verdict::Continue);
    let accepted = rig.turns.take_reroute().expect("accepted");
    assert!(rig.turns.take_reroute().is_none(), "taken once");
    assert_eq!(accepted.session, rig.session.managed_session_id);
    assert_eq!(accepted.turn, rig.turn.managed_turn_id);
    rig.conversations.rerouted(
        &rig.instance,
        &accepted.session,
        accepted.turn.clone(),
        &accepted.reroute,
    );
    let status = rig
        .conversations
        .projection(&rig.store, &rig.runtime, &rig.session.managed_session_id)
        .unwrap();
    let seen = status.observed_model.expect("observed");
    // The destination is what is reported now, marked as such and from where; what the thread
    // answer said about provider and effort is kept, not inferred or dropped.
    assert_eq!(seen.model_id, "model-b");
    assert_eq!(seen.source, ObservedModelSource::TurnRerouted);
    assert_eq!(seen.rerouted_from.as_deref(), Some("model-a"));
    assert_eq!(
        seen.reroute_reason,
        Some(RerouteReason::HighRiskCyberActivity)
    );
    assert_eq!(seen.rerouted_turn.as_ref(), Some(&rig.turn.managed_turn_id));
    assert_eq!(seen.provider_id.as_deref(), Some("custom"));
    assert_eq!(seen.reasoning_effort.as_deref(), Some("high"));
    // The turn is still running, and its stream carries nothing new for it.
    assert_eq!(
        rig.notice(NoticeKind::Terminal(TurnOutcome::Completed)),
        Verdict::Continue
    );
    // With no turn at all a reroute is a violation.
    assert_eq!(
        rig.notice(reroute("model-b", "model-c")),
        Verdict::Violation
    );
    // A reroute for another generation's attachment records nothing.
    let other = AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
    let before = rig
        .conversations
        .projection(&rig.store, &rig.runtime, &rig.session.managed_session_id)
        .unwrap()
        .observed_model;
    rig.conversations.rerouted(
        &other,
        &rig.session.managed_session_id,
        rig.turn.managed_turn_id.clone(),
        &safety::Reroute {
            from: "x".into(),
            to: "y".into(),
            reason: RerouteReason::Unrecognised,
        },
    );
    assert_eq!(
        rig.conversations
            .projection(&rig.store, &rig.runtime, &rig.session.managed_session_id)
            .unwrap()
            .observed_model,
        before
    );
}
