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
                item: item("never-announced"),
                kind: ActionKind::Command,
                end: ItemEnd::Completed,
            }),
            end => rig.notice(NoticeKind::ItemEnded {
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
    // A command the runtime ran on its own, without ever asking, is not mediated.
    let mut rig = Rig::new();
    let cwd = rig.workspace.clone();
    rig.propose_command("call-9", "ls", &cwd);
    assert_eq!(
        rig.notice(NoticeKind::ItemEnded {
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
