use super::*;
use crate::{
    adapters::codex::{CodexThreadId, CodexTurnId},
    control::{
        approval_action::{CommandAction, FileAction, FileChange},
        runtime::MaterializationStage,
        schema,
    },
};
use ApprovalDecision::{Accept, Cancel, Decline};
use ApprovalKind::{CommandExecution, FileChange as FileKind};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Barrier, Mutex},
};

struct TestClock(Mutex<OffsetDateTime>);
impl leases::Clock for TestClock {
    fn now(&self) -> OffsetDateTime {
        *self.0.lock().unwrap()
    }
}
impl TestClock {
    fn advance(&self, seconds: i64) {
        *self.0.lock().unwrap() += time::Duration::seconds(seconds);
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
fn digest(seed: &str) -> String {
    format!("{:x}", Sha256::digest(seed.as_bytes()))
}
fn shape(kind: ApprovalKind) -> ApprovalStructure {
    match kind {
        CommandExecution => ApprovalStructure {
            command_bytes: 12,
            cwd_bytes: 5,
            ..Default::default()
        },
        FileKind => ApprovalStructure {
            files: 1,
            diff_bytes: 10,
            ..Default::default()
        },
    }
}
fn respond(a: &ManagedApprovalRecord, decision: ApprovalDecision) -> ApprovalRespond {
    ApprovalRespond {
        intent_id: uuid(),
        managed_approval_id: a.managed_approval_id.clone(),
        expected_revision: a.revision,
        decision,
    }
}
fn grant(
    store: &mut Store,
    session: &ManagedSessionRecord,
    scopes: Vec<ExecutionScope>,
    ttl: u32,
) -> ExecutionLease {
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
    lease
}
fn events(s: &Store) -> Vec<Value> {
    let mut q = s
        .conn
        .prepare("SELECT envelope FROM control_events ORDER BY sequence")
        .unwrap();
    q.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
        .collect()
}
fn kinds(s: &Store, a: &ManagedApprovalRecord) -> Vec<String> {
    events(s)
        .into_iter()
        .filter(|e| e["payload"]["managed_approval_id"] == a.managed_approval_id.as_str())
        .map(|e| {
            e["event_type"]
                .as_str()
                .unwrap()
                .trim_start_matches("authority.managed_approval.")
                .to_owned()
        })
        .collect()
}
fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}
struct Fixture {
    root: PathBuf,
    store: Store,
    clock: Arc<TestClock>,
    session: ManagedSessionRecord,
    lease: ExecutionLease,
    binding: SessionBinding,
    runtime: AgentRuntimeInstanceId,
    turn: ManagedTurnRecord,
    turns: u32,
}
impl Fixture {
    fn new() -> Self {
        Self::with(all_scopes(), 3600)
    }
    fn with(scopes: Vec<ExecutionScope>, ttl: u32) -> Self {
        let root = std::env::temp_dir().join(format!("wyss-approval-{}", uuid().as_str()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let mut store = Store::open(&root.join("db"), uuid()).unwrap();
        let clock = Arc::new(TestClock(Mutex::new(
            OffsetDateTime::parse("2026-10-07T00:00:00Z", &Rfc3339).unwrap(),
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
        let lease = grant(&mut store, &session, scopes, ttl);
        let runtime = AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
        let reservation = store
            .reserve_binding(&Operation::BindingCreate(BindingCreate {
                intent_id: uuid(),
                managed_session_id: session.managed_session_id.clone(),
                expected_session_revision: 1,
                workspace_root: root.to_str().unwrap().into(),
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
        let binding = store
            .commit_binding(&reservation, &thread, &runtime)
            .unwrap();
        let turn = start_turn(&mut store, &session, &binding, &lease, &runtime, 1);
        Self {
            root,
            store,
            clock,
            session,
            lease,
            binding,
            runtime,
            turn,
            turns: 1,
        }
    }
    fn db(&self) -> PathBuf {
        self.root.join("db")
    }
    fn reopen(&mut self) {
        self.store = Store::open(&self.db(), uuid()).unwrap();
        self.store.set_clock(self.clock.clone());
    }
    fn next_turn(&mut self) {
        self.store
            .transition_turn(
                &self.turn.managed_turn_id,
                None,
                TurnState::Completed,
                Some(TurnReason::Completed),
            )
            .unwrap();
        self.turns += 1;
        self.turn = start_turn(
            &mut self.store,
            &self.session,
            &self.binding,
            &self.lease,
            &self.runtime,
            self.turns,
        );
    }
    fn try_open(&mut self, item: &str, kind: ApprovalKind) -> ControlResult<ManagedApprovalRecord> {
        self.store.open_approval(
            &self.turn.managed_turn_id,
            &self.runtime,
            kind,
            item,
            &digest(item),
            shape(kind),
        )
    }
    fn open(&mut self, item: &str, kind: ApprovalKind) -> ManagedApprovalRecord {
        self.try_open(item, kind).unwrap()
    }
    fn decide(
        &mut self,
        a: &ManagedApprovalRecord,
        decision: ApprovalDecision,
    ) -> ControlResult<Decided> {
        self.store
            .decide_approval(&respond(a, decision), &a.action_fingerprint)
    }
    fn fresh(
        &mut self,
        a: &ManagedApprovalRecord,
        decision: ApprovalDecision,
    ) -> (ManagedApprovalRecord, ApprovalReceipt) {
        match self.decide(a, decision).unwrap() {
            Decided::Fresh(a, r) => (*a, r),
            Decided::Replayed(_) => panic!("fresh decision expected"),
        }
    }
    fn current(&self, a: &ManagedApprovalRecord) -> ManagedApprovalRecord {
        self.store.approval(&a.managed_approval_id).unwrap()
    }
    fn revoke(&mut self) {
        self.store
            .apply(&Operation::LeaseRevoke(LeaseRevoke {
                intent_id: uuid(),
                lease_id: self.lease.lease_id.clone(),
                expected_revision: self.lease.revision,
            }))
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn start_turn(
    store: &mut Store,
    session: &ManagedSessionRecord,
    binding: &SessionBinding,
    lease: &ExecutionLease,
    runtime: &AgentRuntimeInstanceId,
    n: u32,
) -> ManagedTurnRecord {
    let start = TurnStart {
        intent_id: uuid(),
        managed_session_id: session.managed_session_id.clone(),
        expected_session_revision: 1,
        prompt: "prompt".into(),
    };
    let turn = store
        .reserve_turn(
            &turns::StartFingerprint::compute(&start).unwrap(),
            binding,
            lease,
            runtime,
        )
        .unwrap();
    store
        .transition_turn(
            &turn.managed_turn_id,
            Some(&CodexTurnId::parse(&format!("provider-turn-{n}")).unwrap()),
            TurnState::Running,
            None,
        )
        .unwrap()
}

#[test]
fn request_is_recorded_pending_with_the_stricter_deadline_and_a_metadata_only_event() {
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    assert_eq!(
        (a.state, a.revision, a.decision, a.effect),
        (ApprovalState::Pending, 1, None, ApprovalEffect::None)
    );
    assert_eq!(a.managed_turn_id, f.turn.managed_turn_id);
    assert_eq!(a.managed_session_id, f.session.managed_session_id);
    assert_eq!(a.runtime_instance_id, f.runtime);
    assert_eq!(a.admitted_lease_id, f.lease.lease_id);
    assert_eq!(a.admitted_lease_revision, f.turn.admitted_lease_revision);
    assert_eq!(
        timestamp(&a.expires_at).unwrap() - timestamp(&a.created_at).unwrap(),
        time::Duration::seconds(APPROVAL_TTL_SECONDS)
    );
    assert_eq!((a.terminal_at.clone(), a.terminal_reason), (None, None));
    assert_eq!(f.store.pending_approval().unwrap(), Some(a.clone()));
    assert_eq!(f.current(&a), a);
    assert_eq!(
        f.store
            .approval_for_item(&f.turn.managed_turn_id, "call_1")
            .unwrap(),
        Some(a.clone())
    );
    assert_eq!(
        f.store
            .approval_for_item(&f.turn.managed_turn_id, "call_2")
            .unwrap(),
        None
    );
    assert_eq!(
        f.store
            .approval(&ManagedApprovalId::parse(uuid().as_str()).unwrap())
            .unwrap_err(),
        ControlError::ApprovalNotFound
    );
    assert_eq!(
        f.store.next_approval_expiry().unwrap(),
        Some(timestamp(&a.expires_at).unwrap())
    );
    let event = events(&f.store).pop().unwrap();
    assert_eq!(event["event_type"], "authority.managed_approval.requested");
    assert_eq!(event["sequence"], a.created_sequence.get().to_string());
    assert!(event["correlation_id"].is_null());
    assert_eq!(event["session_id"], f.session.managed_session_id.as_str());
    let payload = event["payload"].as_object().unwrap();
    let mut keys: Vec<_> = payload.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut expected = PAYLOAD.to_vec();
    expected.sort_unstable();
    assert_eq!(keys, expected);
    assert_eq!(payload["action_fingerprint"], digest("call_1"));
    // The admitted lease's own expiry is the deadline when it is the stricter bound.
    let mut short = Fixture::with(all_scopes(), 100);
    let b = short.open("call_1", FileKind);
    assert_eq!(b.expires_at, short.lease.expires_at);
    assert_eq!(b.kind, FileKind);
}
#[test]
fn one_pending_approval_globally_and_one_approval_per_provider_item() {
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    assert_eq!(
        f.try_open("call_2", FileKind).unwrap_err(),
        ControlError::CapacityExceeded
    );
    // A repeated request for the same item is an escalation retry, never a new approval.
    assert_eq!(
        f.try_open("call_1", CommandExecution).unwrap_err(),
        ControlError::CorrelationError
    );
    f.fresh(&a, Decline);
    assert_eq!(
        f.try_open("call_1", CommandExecution).unwrap_err(),
        ControlError::CorrelationError
    );
    let b = f.open("call_2", FileKind);
    assert_eq!(f.store.pending_approval().unwrap(), Some(b.clone()));
    assert_eq!(
        count(&f.store.conn, "SELECT count(*) FROM managed_approvals"),
        2
    );
    // The same provider item identifier in a later turn is a different action.
    f.fresh(&b, Cancel);
    f.next_turn();
    let c = f.open("call_1", CommandExecution);
    assert_ne!(c.managed_approval_id, a.managed_approval_id);
    assert_ne!(c.managed_turn_id, a.managed_turn_id);
}
#[test]
fn request_needs_a_running_turn_its_runtime_an_active_lease_and_valid_metadata() {
    let mut f = Fixture::new();
    let turn = f.turn.managed_turn_id.clone();
    let other_runtime = AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
    let open = |f: &mut Fixture,
                turn: &ManagedTurnId,
                runtime: &AgentRuntimeInstanceId,
                item: &str,
                fingerprint: &str,
                structure: ApprovalStructure| {
        f.store
            .open_approval(
                turn,
                runtime,
                CommandExecution,
                item,
                fingerprint,
                structure,
            )
            .unwrap_err()
    };
    let good = shape(CommandExecution);
    let runtime = f.runtime.clone();
    let fp = digest("x");
    assert_eq!(
        open(&mut f, &turn, &other_runtime, "call_1", &fp, good),
        ControlError::CorrelationError
    );
    assert_eq!(
        open(
            &mut f,
            &ManagedTurnId::parse(uuid().as_str()).unwrap(),
            &runtime,
            "call_1",
            &fp,
            good
        ),
        ControlError::TurnNotFound
    );
    let long = "i".repeat(MAX_PROVIDER_ITEM_BYTES + 1);
    let upper = fp.to_uppercase();
    for (item, fingerprint, structure) in [
        ("", fp.as_str(), good),
        ("bad item", fp.as_str(), good),
        (long.as_str(), fp.as_str(), good),
        ("call_1", "not-a-fingerprint", good),
        ("call_1", upper.as_str(), good),
        ("call_1", fp.as_str(), shape(FileKind)),
        ("call_1", fp.as_str(), ApprovalStructure::default()),
    ] {
        assert_eq!(
            open(&mut f, &turn, &runtime, item, fingerprint, structure),
            ControlError::InvalidRequest
        );
    }
    assert!(
        f.store
            .open_approval(
                &turn,
                &runtime,
                CommandExecution,
                &"i".repeat(MAX_PROVIDER_ITEM_BYTES),
                &fp,
                good
            )
            .is_ok()
    );
    let mut revoked = Fixture::new();
    revoked.revoke();
    assert_eq!(
        revoked.try_open("call_1", CommandExecution).unwrap_err(),
        ControlError::GateDenied
    );
    let mut expired = Fixture::with(all_scopes(), 60);
    expired.clock.advance(60);
    assert_eq!(
        expired.try_open("call_1", CommandExecution).unwrap_err(),
        ControlError::GateDenied
    );
    let mut ended = Fixture::new();
    ended
        .store
        .transition_turn(
            &ended.turn.managed_turn_id,
            None,
            TurnState::Completed,
            Some(TurnReason::Completed),
        )
        .unwrap();
    assert_eq!(
        ended.try_open("call_1", CommandExecution).unwrap_err(),
        ControlError::InvalidTransition
    );
    for f in [&revoked, &expired, &ended] {
        assert_eq!(
            count(&f.store.conn, "SELECT count(*) FROM managed_approvals"),
            0
        );
        assert!(kinds_all(&f.store).is_empty());
    }
}
fn kinds_all(s: &Store) -> Vec<String> {
    events(s)
        .into_iter()
        .filter_map(|e| {
            e["event_type"]
                .as_str()
                .and_then(|k| k.strip_prefix("authority.managed_approval."))
                .map(str::to_owned)
        })
        .collect()
}
#[test]
fn accept_commits_decision_dispatch_marker_event_and_receipt_in_one_transaction() {
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    // Nothing is written when any part of the transaction cannot be.
    for blocked in ["processed_intents", "control_events"] {
        f.store.conn.execute_batch(&format!("CREATE TEMP TRIGGER blocked BEFORE INSERT ON {blocked} BEGIN SELECT RAISE(ABORT,'fixture'); END")).unwrap();
        let head = f.store.status().unwrap().last_sequence;
        assert!(f.decide(&a, Accept).is_err());
        assert_eq!(f.current(&a), a);
        assert_eq!(f.store.status().unwrap().last_sequence, head);
        assert_eq!(
            count(
                &f.store.conn,
                &format!("SELECT count(*) FROM processed_intents WHERE mutation LIKE '%{METHOD}%'")
            ),
            0
        );
        f.store.conn.execute_batch("DROP TRIGGER blocked").unwrap();
    }
    f.clock.advance(7);
    let request = respond(&a, Accept);
    let Decided::Fresh(accepted, receipt) = f
        .store
        .decide_approval(&request, &a.action_fingerprint)
        .unwrap()
    else {
        panic!()
    };
    let accepted = *accepted;
    assert_eq!(
        (
            accepted.state,
            accepted.revision,
            accepted.decision,
            accepted.effect,
            accepted.terminal_reason
        ),
        (
            ApprovalState::Accepted,
            2,
            Some(Accept),
            ApprovalEffect::Dispatched,
            Some(ApprovalReason::ClientDecision)
        )
    );
    assert_eq!(accepted.terminal_at, Some(accepted.updated_at.clone()));
    assert_eq!(
        timestamp(&accepted.updated_at).unwrap() - timestamp(&a.created_at).unwrap(),
        time::Duration::seconds(7)
    );
    assert_eq!(f.current(&a), accepted);
    assert_eq!(
        receipt,
        ApprovalReceipt {
            managed_approval_id: a.managed_approval_id.clone(),
            managed_turn_id: a.managed_turn_id.clone(),
            kind: CommandExecution,
            decision: Accept,
            state: ApprovalState::Accepted,
            revision: 2,
            action_fingerprint: a.action_fingerprint.clone(),
            decided_at: accepted.updated_at.clone(),
        }
    );
    let event = events(&f.store).pop().unwrap();
    assert_eq!(event["event_type"], "authority.managed_approval.accepted");
    assert_eq!(event["correlation_id"], request.intent_id.as_str());
    assert_eq!(event["payload"]["effect"], "dispatched");
    let (mutation, result): (String, String) = f
        .store
        .conn
        .query_row(
            "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
            [request.intent_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<ApprovalReceipt>(&result).unwrap(),
        receipt
    );
    assert!(mutation.len() <= 1024 && result.len() <= 1024);
    assert_eq!(f.store.pending_approval().unwrap(), None);
    assert_eq!(kinds(&f.store, &a), ["requested", "accepted"]);
}
#[test]
fn decision_intent_is_idempotent_and_any_conflicting_reuse_is_refused() {
    let mut f = Fixture::new();
    let a = f.open("call_1", FileKind);
    // Mixed-case identifiers of the same intent are the same intent.
    let mut request = respond(&a, Decline);
    request.intent_id = WireUuid::try_from(request.intent_id.as_str().to_uppercase()).unwrap();
    request.managed_approval_id =
        ManagedApprovalId::parse(&a.managed_approval_id.as_str().to_uppercase()).unwrap();
    let Decided::Fresh(declined, receipt) = f
        .store
        .decide_approval(&request, &a.action_fingerprint)
        .unwrap()
    else {
        panic!()
    };
    let declined = *declined;
    assert_eq!(
        (declined.state, declined.effect, declined.decision),
        (ApprovalState::Declined, ApprovalEffect::None, Some(Decline))
    );
    let head = f.store.status().unwrap().last_sequence;
    for retry in [request.clone(), {
        let mut lower = request.clone();
        lower.intent_id = WireUuid::try_from(request.intent_id.as_str().to_lowercase()).unwrap();
        lower
    }] {
        let Decided::Replayed(again) = f
            .store
            .decide_approval(&retry, &a.action_fingerprint)
            .unwrap()
        else {
            panic!("a retried intent must never look fresh")
        };
        assert_eq!(again, receipt);
    }
    assert_eq!(f.store.status().unwrap().last_sequence, head);
    assert_eq!(f.current(&a), declined);
    for conflicting in [
        ApprovalRespond {
            decision: Accept,
            ..request.clone()
        },
        ApprovalRespond {
            expected_revision: 2,
            ..request.clone()
        },
        ApprovalRespond {
            managed_approval_id: ManagedApprovalId::parse(uuid().as_str()).unwrap(),
            ..request.clone()
        },
    ] {
        assert_eq!(
            f.store
                .decide_approval(&conflicting, &a.action_fingerprint)
                .unwrap_err(),
            ControlError::IntentConflict
        );
    }
    // An intent already consumed by another operation cannot decide an approval.
    let b = f.open("call_2", CommandExecution);
    let consumed: String = f
        .store
        .conn
        .query_row(
            "SELECT intent_id FROM processed_intents WHERE mutation LIKE '%managedSession/prepare%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut reused = respond(&b, Accept);
    reused.intent_id = WireUuid::try_from(consumed).unwrap();
    assert_eq!(
        f.store
            .decide_approval(&reused, &b.action_fingerprint)
            .unwrap_err(),
        ControlError::IntentConflict
    );
    assert_eq!(f.current(&b), b);
    assert_eq!(
        f.store.status().unwrap().last_sequence.unwrap().get(),
        b.created_sequence.get()
    );
    let zero = ApprovalRespond {
        expected_revision: 0,
        ..respond(&b, Accept)
    };
    assert_eq!(
        f.store
            .decide_approval(&zero, &b.action_fingerprint)
            .unwrap_err(),
        ControlError::InvalidRequest
    );
}
#[test]
fn stale_revision_changed_action_unknown_approval_and_deadline_never_decide() {
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    let stale = ApprovalRespond {
        expected_revision: 2,
        ..respond(&a, Accept)
    };
    assert_eq!(
        f.store
            .decide_approval(&stale, &a.action_fingerprint)
            .unwrap_err(),
        ControlError::StaleRevision
    );
    assert_eq!(
        f.store
            .decide_approval(&respond(&a, Accept), &digest("another action"))
            .unwrap_err(),
        ControlError::CorrelationError
    );
    let unknown = ApprovalRespond {
        managed_approval_id: ManagedApprovalId::parse(uuid().as_str()).unwrap(),
        ..respond(&a, Accept)
    };
    assert_eq!(
        f.store
            .decide_approval(&unknown, &a.action_fingerprint)
            .unwrap_err(),
        ControlError::ApprovalNotFound
    );
    f.clock.advance(APPROVAL_TTL_SECONDS - 1);
    assert_eq!(f.store.expire_approvals().unwrap(), vec![]);
    f.clock.advance(1);
    for decision in [Accept, Decline, Cancel] {
        assert_eq!(
            f.decide(&a, decision).unwrap_err(),
            ControlError::GateDenied
        );
    }
    assert_eq!(f.current(&a), a);
    assert_eq!(kinds(&f.store, &a), ["requested"]);
    assert_eq!(
        count(
            &f.store.conn,
            &format!("SELECT count(*) FROM processed_intents WHERE mutation LIKE '%{METHOD}%'")
        ),
        0
    );
}
#[test]
fn accept_needs_the_original_active_lease_its_scope_a_running_turn_and_an_open_session() {
    // Without the scope of that kind of action Accept is impossible; refusing is not.
    for (kind, scopes) in [
        (
            CommandExecution,
            vec![
                ExecutionScope::SessionCreate,
                ExecutionScope::TurnStart,
                ExecutionScope::FileChange,
            ],
        ),
        (
            FileKind,
            vec![
                ExecutionScope::SessionCreate,
                ExecutionScope::TurnStart,
                ExecutionScope::CommandExecute,
            ],
        ),
    ] {
        let mut f = Fixture::with(scopes, 3600);
        let a = f.open("call_1", kind);
        assert_eq!(f.decide(&a, Accept).unwrap_err(), ControlError::GateDenied);
        assert_eq!(f.current(&a), a);
        assert_eq!(f.fresh(&a, Decline).0.state, ApprovalState::Declined);
    }
    // A revoked lease never authorizes, and neither does a NEW lease with every scope.
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    f.revoke();
    assert_eq!(f.decide(&a, Accept).unwrap_err(), ControlError::GateDenied);
    let replacement = grant(&mut f.store, &f.session.clone(), all_scopes(), 3600);
    assert_ne!(replacement.lease_id, f.lease.lease_id);
    assert_eq!(f.decide(&a, Accept).unwrap_err(), ControlError::GateDenied);
    assert_eq!(f.fresh(&a, Cancel).0.state, ApprovalState::Cancelled);
    // Renewing the admitted lease keeps it the same lease: it still authorizes.
    let mut f = Fixture::new();
    let a = f.open("call_1", FileKind);
    f.store
        .apply(&Operation::LeaseRenew(LeaseRenew {
            intent_id: uuid(),
            lease_id: f.lease.lease_id.clone(),
            expected_revision: f.lease.revision,
            ttl_seconds: 1800,
        }))
        .unwrap();
    assert_eq!(f.fresh(&a, Accept).0.state, ApprovalState::Accepted);
    // A turn that is no longer running.
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    f.store
        .transition_turn(
            &f.turn.managed_turn_id,
            None,
            TurnState::Interrupted,
            Some(TurnReason::UserCancel),
        )
        .unwrap();
    assert_eq!(f.decide(&a, Accept).unwrap_err(), ControlError::GateDenied);
    // A closed session (closing also revokes its lease).
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    f.store
        .apply(&Operation::Close(Close {
            intent_id: uuid(),
            managed_session_id: f.session.managed_session_id.clone(),
            expected_revision: 1,
        }))
        .unwrap();
    assert_eq!(f.decide(&a, Accept).unwrap_err(), ControlError::GateDenied);
    assert_eq!(f.current(&a), a);
    assert_eq!(
        count(
            &f.store.conn,
            "SELECT count(*) FROM managed_approvals WHERE effect<>'none'"
        ),
        0
    );
}
#[test]
fn terminal_approvals_are_final_through_the_api_and_through_the_schema() {
    let mut f = Fixture::new();
    let mut finals = Vec::new();
    let a = f.open("call_1", CommandExecution);
    finals.push(f.fresh(&a, Accept).0);
    let a = f.open("call_2", CommandExecution);
    finals.push(f.fresh(&a, Decline).0);
    let a = f.open("call_3", FileKind);
    finals.push(f.fresh(&a, Cancel).0);
    f.open("call_4", FileKind);
    finals.extend(
        f.store
            .settle_approvals(
                Some(&f.turn.managed_turn_id.clone()),
                ApprovalReason::ApprovalTimeout,
            )
            .unwrap(),
    );
    f.open("call_5", FileKind);
    finals.extend(
        f.store
            .settle_approvals(
                Some(&f.turn.managed_turn_id.clone()),
                ApprovalReason::TurnCancelled,
            )
            .unwrap(),
    );
    let states: Vec<_> = finals.iter().map(|a| a.state).collect();
    assert_eq!(
        states,
        [
            ApprovalState::Accepted,
            ApprovalState::Declined,
            ApprovalState::Cancelled,
            ApprovalState::Expired,
            // The accepted approval has no confirmed outcome: ending its turn makes it unknown.
            ApprovalState::Indeterminate,
            ApprovalState::Invalidated,
        ]
    );
    let finals: Vec<_> = finals.iter().skip(1).map(|a| f.current(a)).collect();
    let head = f.store.status().unwrap().last_sequence;
    for a in &finals {
        assert!(a.state.terminal() && a.terminal_at.is_some() && a.terminal_reason.is_some());
        for decision in [Accept, Decline, Cancel] {
            assert_eq!(
                f.decide(a, decision).unwrap_err(),
                ControlError::InvalidTransition
            );
        }
        if a.state != ApprovalState::Accepted {
            assert_eq!(
                f.store
                    .confirm_approval(
                        &a.managed_approval_id,
                        &a.managed_turn_id,
                        &a.runtime_instance_id,
                        &a.provider_item_id
                    )
                    .unwrap_err(),
                ControlError::InvalidTransition
            );
        }
        for sql in [
            "UPDATE managed_approvals SET state='pending',decision=NULL,effect='none',terminal_at=NULL,terminal_reason=NULL,revision=revision+1 WHERE managed_approval_id=?1",
            "UPDATE managed_approvals SET state='accepted',decision='accept',effect='dispatched',revision=revision+1 WHERE managed_approval_id=?1",
            "UPDATE managed_approvals SET revision=revision+1 WHERE managed_approval_id=?1",
            "UPDATE managed_approvals SET updated_at=updated_at WHERE managed_approval_id=?1",
            "DELETE FROM managed_approvals WHERE managed_approval_id=?1",
        ] {
            assert!(
                f.store
                    .conn
                    .execute(sql, [a.managed_approval_id.as_str()])
                    .is_err(),
                "{sql}"
            );
        }
        assert_eq!(&f.current(a), a);
    }
    for reason in [ApprovalReason::RuntimeLost, ApprovalReason::LeaseExpired] {
        assert_eq!(f.store.settle_approvals(None, reason).unwrap(), vec![]);
    }
    assert_eq!(f.store.status().unwrap().last_sequence, head);
}
#[test]
fn schema_refuses_rebinding_revision_skips_duplicates_and_a_second_pending_row() {
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    let other = uuid();
    for column in [
        "managed_approval_id",
        "managed_turn_id",
        "managed_session_id",
        "runtime_instance_id",
        "admitted_lease_id",
    ] {
        // Enforcement off: the trigger alone, not a foreign key, must refuse the rebind.
        f.store
            .conn
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        let sql = format!(
            "UPDATE managed_approvals SET revision=revision+1,state='expired',terminal_at=updated_at,terminal_reason='approval_timeout',{column}=?1"
        );
        let error = f.store.conn.execute(&sql, [other.as_str()]).unwrap_err();
        assert!(
            error.to_string().contains("approval_transition"),
            "{column}: {error}"
        );
    }
    f.store
        .conn
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    for change in [
        "kind='file_change'",
        "provider_item_id='call_9'",
        "admitted_lease_revision=admitted_lease_revision+1",
        "action_fingerprint='0000000000000000000000000000000000000000000000000000000000000000'",
        "structure='{}'",
        "created_at='2020-01-01T00:00:00Z'",
        "expires_at='2099-01-01T00:00:00Z'",
        "created_sequence='00000000000000000099'",
        "revision=revision+2",
        "revision=revision",
    ] {
        let sql = format!(
            "UPDATE managed_approvals SET revision=revision+1,state='expired',terminal_at=updated_at,terminal_reason='approval_timeout',{change}"
        );
        assert!(f.store.conn.execute(&sql, []).is_err(), "{change}");
    }
    // Decision, effect and state must agree with each other in every row.
    for change in [
        "state='accepted',decision='decline',effect='dispatched'",
        "state='accepted',decision='accept',effect='none'",
        "state='declined',decision='decline',effect='dispatched'",
        "state='expired',decision='accept',effect='none'",
        "state='indeterminate',decision='accept',effect='unknown'",
        "state='invented',decision=NULL,effect='none'",
    ] {
        let sql = format!(
            "UPDATE managed_approvals SET revision=revision+1,terminal_at=updated_at,terminal_reason='client_decision',{change}"
        );
        assert!(f.store.conn.execute(&sql, []).is_err(), "{change}");
    }
    assert_eq!(f.current(&a), a);
    let copy = "INSERT INTO managed_approvals SELECT ?1,managed_turn_id,managed_session_id,runtime_instance_id,kind,?2,admitted_lease_id,admitted_lease_revision,state,revision,action_fingerprint,structure,decision,effect,created_at,updated_at,expires_at,terminal_at,terminal_reason,?3 FROM managed_approvals";
    let fails = |id: &str, item: &str, sequence: &str| {
        f.store
            .conn
            .execute(copy, [id, item, sequence])
            .unwrap_err()
            .to_string()
    };
    let fresh = uuid();
    let far = "00000000000000009999";
    assert!(fails(fresh.as_str(), "call_2", far).contains("managed_approvals_one_pending"));
    f.store
        .settle_approvals(None, ApprovalReason::RuntimeLost)
        .unwrap();
    let copy = copy.replace("state,revision", "'pending',1").replace(
        "decision,effect,created_at,updated_at,expires_at,terminal_at,terminal_reason",
        "NULL,'none',created_at,created_at,expires_at,NULL,NULL",
    );
    let fails = |id: &str, item: &str, sequence: &str| {
        f.store
            .conn
            .execute(&copy, [id, item, sequence])
            .unwrap_err()
            .to_string()
    };
    assert!(fails(a.managed_approval_id.as_str(), "call_2", far).contains("managed_approval_id"));
    assert!(fails(fresh.as_str(), "call_1", far).contains("provider_item_id"));
    assert!(
        fails(fresh.as_str(), "call_2", &key(a.created_sequence.get()))
            .contains("created_sequence")
    );
    assert!(fails(f.turn.managed_turn_id.as_str(), "call_2", far).contains("CHECK"));
    assert_eq!(
        count(&f.store.conn, "SELECT count(*) FROM managed_approvals"),
        1
    );
}
#[test]
fn confirmation_needs_the_exact_turn_runtime_and_item_of_an_accepted_approval() {
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    let (accepted, _) = f.fresh(&a, Accept);
    let id = a.managed_approval_id.clone();
    let turn = a.managed_turn_id.clone();
    let runtime = a.runtime_instance_id.clone();
    let other_turn = ManagedTurnId::parse(uuid().as_str()).unwrap();
    let other_runtime = AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
    for (t, r, item) in [
        (&other_turn, &runtime, "call_1"),
        (&turn, &other_runtime, "call_1"),
        (&turn, &runtime, "call_2"),
    ] {
        assert_eq!(
            f.store.confirm_approval(&id, t, r, item).unwrap_err(),
            ControlError::CorrelationError
        );
    }
    assert_eq!(f.current(&a), accepted);
    f.clock.advance(3);
    let confirmed = f
        .store
        .confirm_approval(&id, &turn, &runtime, "call_1")
        .unwrap();
    assert_eq!(
        (confirmed.state, confirmed.effect, confirmed.revision),
        (ApprovalState::Accepted, ApprovalEffect::Confirmed, 3)
    );
    // The decision instant and its reason are not rewritten by the outcome.
    assert_eq!(confirmed.terminal_at, accepted.terminal_at);
    assert_eq!(
        confirmed.terminal_reason,
        Some(ApprovalReason::ClientDecision)
    );
    let head = f.store.status().unwrap().last_sequence;
    assert_eq!(
        f.store
            .confirm_approval(&id, &turn, &runtime, "call_1")
            .unwrap(),
        confirmed
    );
    for reason in [
        ApprovalReason::RuntimeLost,
        ApprovalReason::AuthorityRestarted,
    ] {
        assert_eq!(f.store.settle_approvals(None, reason).unwrap(), vec![]);
    }
    assert_eq!(f.store.status().unwrap().last_sequence, head);
    assert_eq!(kinds(&f.store, &a), ["requested", "accepted", "confirmed"]);
    let b = f.open("call_2", CommandExecution);
    assert_eq!(
        f.store
            .confirm_approval(&b.managed_approval_id, &turn, &runtime, "call_2")
            .unwrap_err(),
        ControlError::InvalidTransition
    );
}
#[test]
fn settlement_expires_or_invalidates_undecided_and_marks_unconfirmed_accepts_unknown() {
    // TTL is the binding deadline.
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    assert_eq!(f.store.expire_approvals().unwrap(), vec![]);
    f.clock.advance(APPROVAL_TTL_SECONDS);
    let expired = f.store.expire_approvals().unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(
        (
            expired[0].state,
            expired[0].terminal_reason,
            expired[0].decision,
            expired[0].effect
        ),
        (
            ApprovalState::Expired,
            Some(ApprovalReason::ApprovalTimeout),
            None,
            ApprovalEffect::None
        )
    );
    assert_eq!(f.store.expire_approvals().unwrap(), vec![]);
    assert_eq!(f.store.next_approval_expiry().unwrap(), None);
    assert_eq!(kinds(&f.store, &a), ["requested", "expired"]);
    // The admitted lease's expiry is the binding deadline.
    let mut f = Fixture::with(all_scopes(), 100);
    f.open("call_1", FileKind);
    f.clock.advance(100);
    assert_eq!(
        f.store.expire_approvals().unwrap()[0].terminal_reason,
        Some(ApprovalReason::LeaseExpired)
    );
    // Every other reason invalidates the undecided and makes the unconfirmed accept unknown.
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    let (accepted, _) = f.fresh(&a, Accept);
    let b = f.open("call_2", FileKind);
    // A deadline no longer concerns an approval that was already accepted.
    f.clock.advance(APPROVAL_TTL_SECONDS);
    let settled = f.store.expire_approvals().unwrap();
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].managed_approval_id, b.managed_approval_id);
    assert_eq!(f.current(&a), accepted);
    let c = f.open("call_3", FileKind);
    assert_eq!(
        f.store
            .settle_approvals(
                Some(&ManagedTurnId::parse(uuid().as_str()).unwrap()),
                ApprovalReason::RuntimeLost
            )
            .unwrap(),
        vec![]
    );
    let turn = ManagedTurnId::parse(&f.turn.managed_turn_id.as_str().to_uppercase()).unwrap();
    let settled = f
        .store
        .settle_approvals(Some(&turn), ApprovalReason::RuntimeLost)
        .unwrap();
    assert_eq!(settled.len(), 2);
    assert_eq!(
        (
            settled[0].state,
            settled[0].effect,
            settled[0].decision,
            settled[0].terminal_reason
        ),
        (
            ApprovalState::Indeterminate,
            ApprovalEffect::Unknown,
            Some(Accept),
            Some(ApprovalReason::RuntimeLost)
        )
    );
    assert_eq!(settled[0].terminal_at, accepted.terminal_at);
    assert_eq!(
        (
            settled[1].managed_approval_id.clone(),
            settled[1].state,
            settled[1].terminal_reason
        ),
        (
            c.managed_approval_id.clone(),
            ApprovalState::Invalidated,
            Some(ApprovalReason::RuntimeLost)
        )
    );
    assert_eq!(
        kinds(&f.store, &a),
        ["requested", "accepted", "indeterminate"]
    );
    assert_eq!(kinds(&f.store, &c), ["requested", "invalidated"]);
    assert_eq!(
        f.store
            .settle_approvals(None, ApprovalReason::ClientDecision)
            .unwrap_err(),
        ControlError::InvalidRequest
    );
}
#[test]
fn restart_invalidates_undecided_marks_dispatched_accepts_unknown_and_replays_nothing() {
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    let request = respond(&a, Accept);
    let Decided::Fresh(_, receipt) = f
        .store
        .decide_approval(&request, &a.action_fingerprint)
        .unwrap()
    else {
        panic!()
    };
    let b = f.open("call_2", CommandExecution);
    let (confirmed, _) = f.fresh(&b, Accept);
    let confirmed = f
        .store
        .confirm_approval(
            &confirmed.managed_approval_id,
            &confirmed.managed_turn_id,
            &confirmed.runtime_instance_id,
            "call_2",
        )
        .unwrap();
    let c = f.open("call_3", FileKind);
    let (declined, _) = f.fresh(&c, Decline);
    let d = f.open("call_4", FileKind);
    f.reopen();
    // Opening alone changes nothing: recovery is an explicit, journaled step.
    assert_eq!(f.current(&d), d);
    f.store.recover_approvals().unwrap();
    let recovered = f.current(&a);
    assert_eq!(
        (
            recovered.state,
            recovered.effect,
            recovered.decision,
            recovered.terminal_reason
        ),
        (
            ApprovalState::Indeterminate,
            ApprovalEffect::Unknown,
            Some(Accept),
            Some(ApprovalReason::AuthorityRestarted)
        )
    );
    let invalidated = f.current(&d);
    assert_eq!(
        (
            invalidated.state,
            invalidated.terminal_reason,
            invalidated.decision
        ),
        (
            ApprovalState::Invalidated,
            Some(ApprovalReason::AuthorityRestarted),
            None
        )
    );
    assert_eq!(f.current(&b), confirmed);
    assert_eq!(f.current(&c), declined);
    assert_eq!(f.store.pending_approval().unwrap(), None);
    let head = f.store.status().unwrap().last_sequence;
    f.store.recover_approvals().unwrap();
    f.reopen();
    f.store.recover_approvals().unwrap();
    assert_eq!(f.store.status().unwrap().last_sequence, head);
    // The original intent still answers with its original receipt and causes nothing.
    let Decided::Replayed(again) = f
        .store
        .decide_approval(&request, &a.action_fingerprint)
        .unwrap()
    else {
        panic!("a recovered Accept must never be dispatched again")
    };
    assert_eq!(again, receipt);
    assert_eq!(
        f.decide(&recovered, Accept).unwrap_err(),
        ControlError::InvalidTransition
    );
    assert_eq!(f.store.status().unwrap().last_sequence, head);
    assert_eq!(
        kinds(&f.store, &a),
        ["requested", "accepted", "indeterminate"]
    );
    assert_eq!(kinds(&f.store, &d), ["requested", "invalidated"]);
}
#[test]
fn listing_is_paged_in_request_order_per_turn_and_bounded() {
    let mut f = Fixture::new();
    let first_turn = f.turn.managed_turn_id.clone();
    let mut created = Vec::new();
    for n in 0..5 {
        let a = f.open(&format!("call_{n}"), CommandExecution);
        f.fresh(&a, Decline);
        created.push(f.current(&a));
    }
    f.next_turn();
    let other = f.open("call_0", FileKind);
    let page = |f: &Fixture, after, limit| f.store.approvals(&first_turn, after, limit).unwrap();
    let (one, next) = page(&f, None, 2);
    assert_eq!(one, created[..2]);
    assert_eq!(next, Some(created[1].created_sequence));
    let (two, next) = page(&f, next, 2);
    assert_eq!(two, created[2..4]);
    let (three, next) = page(&f, next, 2);
    assert_eq!((three.as_slice(), next), (&created[4..], None));
    assert_eq!(page(&f, None, MAX_PAGE), (created.clone(), None));
    assert_eq!(
        page(&f, Some(created[4].created_sequence), 2),
        (vec![], None)
    );
    assert!(
        created
            .windows(2)
            .all(|w| w[0].created_sequence.get() < w[1].created_sequence.get())
    );
    let upper = ManagedTurnId::parse(&first_turn.as_str().to_uppercase()).unwrap();
    assert_eq!(f.store.approvals(&upper, None, 5).unwrap().0, created);
    assert_eq!(
        f.store.approvals(&f.turn.managed_turn_id, None, 5).unwrap(),
        (vec![other], None)
    );
    assert_eq!(
        f.store
            .approvals(&ManagedTurnId::parse(uuid().as_str()).unwrap(), None, 5)
            .unwrap(),
        (vec![], None)
    );
    for limit in [0, MAX_PAGE + 1] {
        assert_eq!(
            f.store.approvals(&first_turn, None, limit).unwrap_err(),
            ControlError::InvalidRequest
        );
    }
}
#[test]
fn concurrent_decisions_on_one_approval_commit_exactly_once() {
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = [Accept, Decline]
        .into_iter()
        .map(|decision| {
            let (path, clock, a, barrier) = (f.db(), f.clock.clone(), a.clone(), barrier.clone());
            std::thread::spawn(move || {
                let mut store = Store::open(&path, uuid()).unwrap();
                store.set_clock(clock);
                barrier.wait();
                store.decide_approval(&respond(&a, decision), &a.action_fingerprint)
            })
        })
        .collect();
    let outcomes: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    let winners = outcomes
        .iter()
        .filter(|o| matches!(o, Ok(Decided::Fresh(..))))
        .count();
    assert_eq!(winners, 1);
    assert!(
        outcomes
            .iter()
            .any(|o| matches!(o, Err(ControlError::InvalidTransition)))
    );
    let decided = f.current(&a);
    assert_eq!(decided.revision, 2);
    assert_eq!(kinds(&f.store, &a).len(), 2);
    assert_eq!(
        count(
            &f.store.conn,
            &format!("SELECT count(*) FROM processed_intents WHERE mutation LIKE '%{METHOD}%'")
        ),
        1
    );
    f.reopen();
    assert_eq!(f.current(&a), decided);
}
/// A store with one unconfirmed accept, one decline and one pending approval.
fn mixed() -> (Fixture, [ManagedApprovalRecord; 3]) {
    let mut f = Fixture::new();
    let a = f.open("call_1", CommandExecution);
    let a = f.fresh(&a, Accept).0;
    let b = f.open("call_2", FileKind);
    let b = f.fresh(&b, Decline).0;
    let c = f.open("call_3", FileKind);
    (f, [a, b, c])
}
/// Applies one change behind the schema triggers, as a damaged or forged file would be.
fn tamper(f: &Fixture, change: impl FnOnce(&Connection)) -> ControlResult<()> {
    let c = Connection::open(f.db()).unwrap();
    c.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER,
        false,
    )
    .unwrap();
    change(&c);
    drop(c);
    Store::open(&f.db(), uuid()).map(|_| ())
}
#[test]
fn reopen_checks_every_approval_against_events_receipts_turn_and_lease() {
    let (f, _) = mixed();
    assert_eq!(tamper(&f, |_| ()), Ok(()));
    let sql: [&str; 9] = [
        "DELETE FROM processed_intents WHERE mutation LIKE '%managedTurn/approval/respond%'",
        "DELETE FROM processed_intents WHERE result LIKE '%\"decline\"%'",
        "UPDATE managed_approvals SET revision=revision+1 WHERE state='pending'",
        "UPDATE managed_approvals SET state='cancelled',decision='cancel' WHERE state='declined'",
        "UPDATE managed_approvals SET effect='confirmed' WHERE state='accepted'",
        "UPDATE managed_approvals SET created_sequence='00000000000000000001' WHERE state='pending'",
        "UPDATE managed_approvals SET runtime_instance_id='11111111-1111-4111-8111-111111111111' WHERE state='pending'",
        "UPDATE managed_approvals SET action_fingerprint='0000000000000000000000000000000000000000000000000000000000000000' WHERE state='pending'",
        "UPDATE managed_approvals SET expires_at='2026-10-08T00:00:00Z' WHERE state='pending'",
    ];
    for sql in sql {
        let (f, _) = mixed();
        assert_eq!(
            tamper(&f, |c| assert!(c.execute(sql, []).unwrap() > 0)),
            Err(ControlError::StorageIncompatible),
            "{sql}"
        );
    }
    // A structurally perfect receipt for a decision that was never taken.
    let (f, [_, _, pending]) = mixed();
    let forged = respond(&pending, Decline);
    let receipt = ApprovalReceipt {
        managed_approval_id: pending.managed_approval_id.clone(),
        managed_turn_id: pending.managed_turn_id.clone(),
        kind: pending.kind,
        decision: Decline,
        state: ApprovalState::Declined,
        revision: 2,
        action_fingerprint: pending.action_fingerprint.clone(),
        decided_at: pending.updated_at.clone(),
    };
    assert_eq!(
        tamper(&f, |c| {
            c.execute(
                "INSERT INTO processed_intents VALUES(?1,?2,?3)",
                params![
                    forged.intent_id.as_str(),
                    mutation(&forged).unwrap(),
                    serde_json::to_string(&receipt).unwrap()
                ],
            )
            .unwrap();
        }),
        Err(ControlError::StorageIncompatible)
    );
    // The same forgery with a real receipt removed: the counts still add up, the rows do not.
    let (f, [_, _, pending]) = mixed();
    let forged = respond(&pending, Decline);
    let receipt = ApprovalReceipt {
        managed_approval_id: pending.managed_approval_id.clone(),
        decided_at: pending.updated_at.clone(),
        ..receipt
    };
    assert_eq!(
        tamper(&f, |c| {
            assert_eq!(
                c.execute(
                    "DELETE FROM processed_intents WHERE result LIKE '%\"decline\"%'",
                    []
                )
                .unwrap(),
                1
            );
            c.execute(
                "INSERT INTO processed_intents VALUES(?1,?2,?3)",
                params![
                    forged.intent_id.as_str(),
                    mutation(&forged).unwrap(),
                    serde_json::to_string(&receipt).unwrap()
                ],
            )
            .unwrap();
        }),
        Err(ControlError::StorageIncompatible)
    );
    // An approval row that no event ever announced, and an event without its row.
    let (f, [_, declined, _]) = mixed();
    assert_eq!(
        tamper(&f, |c| {
            c.execute(
                "DELETE FROM managed_approvals WHERE managed_approval_id=?1",
                [declined.managed_approval_id.as_str()],
            )
            .unwrap();
            c.execute(
                "DELETE FROM processed_intents WHERE result LIKE '%\"decline\"%'",
                [],
            )
            .unwrap();
        }),
        Err(ControlError::StorageIncompatible)
    );
}
#[test]
fn audit_validator_accepts_every_real_event_and_refuses_every_malformed_one() {
    let (mut f, [accepted, _, _]) = mixed();
    f.store
        .confirm_approval(
            &accepted.managed_approval_id,
            &accepted.managed_turn_id,
            &accepted.runtime_instance_id,
            "call_1",
        )
        .unwrap();
    f.store
        .settle_approvals(None, ApprovalReason::TurnEnded)
        .unwrap();
    let a = f.open("call_4", CommandExecution);
    f.fresh(&a, Cancel);
    let a = f.open("call_5", CommandExecution);
    f.fresh(&a, Accept);
    f.open("call_6", CommandExecution);
    f.clock.advance(APPROVAL_TTL_SECONDS);
    f.store.expire_approvals().unwrap();
    f.store
        .settle_approvals(None, ApprovalReason::IsolationViolation)
        .unwrap();
    let all: Vec<Value> = events(&f.store)
        .into_iter()
        .filter(|e| {
            e["event_type"]
                .as_str()
                .unwrap()
                .starts_with("authority.managed_approval.")
        })
        .collect();
    let mut seen: Vec<String> = kinds_all(&f.store);
    seen.sort();
    seen.dedup();
    let mut every: Vec<&str> = EVENTS.iter().map(|(k, _, _)| *k).collect();
    every.sort_unstable();
    assert_eq!(seen, every);
    let mut heads = AuditHeads::new();
    for e in &all {
        validate_audit(e).unwrap();
        fold_audit(&mut heads, e).unwrap();
    }
    assert_eq!(heads.len(), 6);
    assert!(
        count(
            &f.store.conn,
            "SELECT max(length(envelope)) FROM control_events"
        ) <= 1400,
        "an approval event must stay far below the 2048 character envelope bound"
    );
    let find = |kind: &str| -> Value {
        all.iter()
            .find(|e| e["event_type"] == format!("authority.managed_approval.{kind}"))
            .unwrap()
            .clone()
    };
    let broken = |base: &Value, change: &dyn Fn(&mut Value)| {
        let mut e = base.clone();
        change(&mut e);
        validate_audit(&e).is_err()
    };
    let accepted = find("accepted");
    let requested = find("requested");
    let changes: [&dyn Fn(&mut Value); 14] = [
        &|e| e["payload"]["command"] = json!("printf"),
        &|e| {
            e["payload"].as_object_mut().unwrap().remove("reason");
        },
        &|e| e["payload"]["state"] = json!("declined"),
        &|e| e["payload"]["effect"] = json!("none"),
        &|e| e["payload"]["effect"] = json!("confirmed"),
        &|e| e["payload"]["decision"] = Value::Null,
        &|e| e["payload"]["reason"] = json!("runtime_lost"),
        &|e| e["payload"]["reason"] = Value::Null,
        &|e| e["correlation_id"] = Value::Null,
        &|e| e["payload"]["action_fingerprint"] = json!("zz"),
        &|e| e["payload"]["revision"] = json!(0),
        &|e| e["event_type"] = json!("authority.managed_approval.updated"),
        &|e| e["sensitivity"] = json!("public"),
        &|e| e["causation_id"] = json!("11111111-1111-4111-8111-111111111111"),
    ];
    for (n, change) in changes.iter().enumerate() {
        assert!(broken(&accepted, *change), "accepted change {n}");
    }
    let changes: [&dyn Fn(&mut Value); 5] = [
        &|e| e["correlation_id"] = json!("11111111-1111-4111-8111-111111111111"),
        &|e| e["payload"]["revision"] = json!(2),
        &|e| e["payload"]["reason"] = json!("approval_timeout"),
        &|e| e["payload"]["kind"] = json!("network_access"),
        &|e| e["payload"]["managed_turn_id"] = json!("not-a-uuid"),
    ];
    for (n, change) in changes.iter().enumerate() {
        assert!(broken(&requested, *change), "requested change {n}");
    }
    // Reconstruction refuses a log that repeats, skips or rebinds an approval.
    let mut heads = AuditHeads::new();
    assert!(fold_audit(&mut heads, &accepted).is_err());
    fold_audit(&mut heads, &requested).unwrap();
    assert!(fold_audit(&mut heads, &requested).is_err());
    let mut rebound = accepted.clone();
    rebound["payload"]["action_fingerprint"] = json!(digest("other"));
    assert!(fold_audit(&mut heads, &rebound).is_err());
    fold_audit(&mut heads, &accepted).unwrap();
}
const COMMAND_SENTINEL: &str = "WYSS-RAW-COMMAND-SENTINEL-7f3a91";
const OUTPUT_SENTINEL: &str = "WYSS-RAW-OUTPUT-SENTINEL-2c85de";
const DIFF_SENTINEL: &str = "WYSS-RAW-DIFF-SENTINEL-b04e17";
const PATH_SENTINEL: &str = "WYSS-RAW-PATH-SENTINEL-55aa0c";
fn occurrences(root: &Path, needle: &str) -> usize {
    let mut total = 0;
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            total += occurrences(&path, needle);
        } else {
            let bytes = fs::read(&path).unwrap();
            total += bytes
                .windows(needle.len())
                .filter(|w| *w == needle.as_bytes())
                .count();
        }
    }
    total
}
#[test]
fn no_command_output_path_or_diff_reaches_any_durable_surface() {
    let mut f = Fixture::new();
    let command = CommandAction::new(
        "call_command",
        &format!("printf {COMMAND_SENTINEL}"),
        &format!("/work/{PATH_SENTINEL}"),
        vec![format!("run {COMMAND_SENTINEL}")],
    )
    .unwrap();
    let patch = FileAction::new(
        "call_patch",
        vec![FileChange {
            path: format!("/work/{PATH_SENTINEL}/a.txt"),
            kind: FileChangeKind::Move,
            move_path: Some(format!("/work/{PATH_SENTINEL}/b.txt")),
            diff: format!("+{DIFF_SENTINEL}\n"),
        }],
    )
    .unwrap();
    let turn = f.turn.managed_turn_id.clone();
    let runtime = f.runtime.clone();
    let a = f
        .store
        .open_approval(
            &turn,
            &runtime,
            CommandExecution,
            command.provider_item_id(),
            &command.fingerprint(),
            command.structure(),
        )
        .unwrap();
    f.fresh(&a, Accept);
    // The store has no parameter that could carry what the command printed.
    let _output = format!("stdout {OUTPUT_SENTINEL}");
    f.store
        .confirm_approval(&a.managed_approval_id, &turn, &runtime, "call_command")
        .unwrap();
    let b = f
        .store
        .open_approval(
            &turn,
            &runtime,
            FileKind,
            patch.provider_item_id(),
            &patch.fingerprint(),
            patch.structure(),
        )
        .unwrap();
    f.fresh(&b, Decline);
    f.next_turn();
    let turn = f.turn.managed_turn_id.clone();
    let c = f
        .store
        .open_approval(
            &turn,
            &runtime,
            FileKind,
            patch.provider_item_id(),
            &patch.fingerprint(),
            patch.structure(),
        )
        .unwrap();
    f.fresh(&c, Accept);
    f.reopen();
    f.store.recover_approvals().unwrap();
    f.store.recover_turns().unwrap();
    // Main database, WAL and shared-memory files are all on disk while the store is open.
    let names: Vec<String> = fs::read_dir(&f.root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.contains(&"db".to_owned()) && names.contains(&"db-wal".to_owned()),
        "{names:?}"
    );
    for sentinel in [
        COMMAND_SENTINEL,
        OUTPUT_SENTINEL,
        DIFF_SENTINEL,
        PATH_SENTINEL,
    ] {
        assert_eq!(occurrences(&f.root, sentinel), 0, "{sentinel}");
    }
    // Control: the same scan does find what is deliberately durable.
    assert!(occurrences(&f.root, &command.fingerprint()) >= 1);
    assert!(occurrences(&f.root, "call_patch") >= 1);
    // Every text cell of every table, including receipts and the audit log.
    let tables: Vec<String> = f
        .store
        .conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut dump = String::new();
    for table in &tables {
        let mut q = f
            .store
            .conn
            .prepare(&format!("SELECT * FROM {table}"))
            .unwrap();
        let columns = q.column_count();
        let mut rows = q.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..columns {
                if let rusqlite::types::ValueRef::Text(text) = row.get_ref(i).unwrap() {
                    dump.push_str(&String::from_utf8_lossy(text));
                    dump.push('\n');
                }
            }
        }
    }
    assert!(tables.len() >= 9 && dump.contains(&patch.fingerprint()));
    for sentinel in [
        COMMAND_SENTINEL,
        OUTPUT_SENTINEL,
        DIFF_SENTINEL,
        PATH_SENTINEL,
        "printf",
        "a.txt",
    ] {
        assert!(!dump.contains(sentinel), "{sentinel}");
    }
    // Stored structure is counts only.
    let structure: String = f
        .store
        .conn
        .query_row(
            "SELECT structure FROM managed_approvals WHERE kind='file_change' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        structure,
        format!(
            "{{\"command_bytes\":0,\"cwd_bytes\":0,\"parsed_actions\":0,\"files\":1,\"diff_bytes\":{}}}",
            DIFF_SENTINEL.len() + 2
        )
    );
    let widest = ApprovalStructure {
        command_bytes: u32::MAX,
        cwd_bytes: u32::MAX,
        parsed_actions: u16::MAX,
        files: u16::MAX,
        diff_bytes: u32::MAX,
    };
    assert!(serde_json::to_string(&widest).unwrap().len() <= 160);
}

// ---- schema v6 -------------------------------------------------------------------------

type Objects = &'static [(&'static str, &'static str)];
const TABLES: [&str; 8] = [
    "managed_sessions",
    "managed_runtimes",
    "execution_leases",
    "managed_session_bindings",
    "managed_session_operations",
    "managed_turns",
    "processed_intents",
    "control_events",
];
fn snapshot(c: &Connection, tables: &[&str]) -> Vec<String> {
    tables
        .iter()
        .map(|table| {
            let mut q = c
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
                .unwrap();
            let columns = q.column_count();
            q.query_map([], |r| {
                Ok((0..columns)
                    .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
            .join("\n")
        })
        .collect()
}
fn master(c: &Connection) -> Vec<(String, String)> {
    c.prepare("SELECT name,sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}
fn expected_master() -> Vec<(String, String)> {
    let mut objects: Vec<_> = schema::OBJECTS
        .iter()
        .map(|(n, s)| ((*n).to_owned(), (*s).to_owned()))
        .collect();
    objects.sort();
    objects
}
fn metadata(c: &Connection) -> (i64, String, String) {
    c.query_row(
        "SELECT schema_version,stream_id,last_sequence FROM authority_metadata",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}
fn version(c: &Connection) -> i64 {
    c.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}
fn pragma(c: &Connection, name: &str) -> i64 {
    c.pragma_query_value(None, name, |r| r.get(0)).unwrap()
}
/// A database as the previous release left it, carrying every row of a live store.
fn older(
    f: &Fixture,
    name: &str,
    objects: &[(&str, &str)],
    level: i64,
    tables: &[&str],
) -> PathBuf {
    let path = f.root.join(name);
    let c = Connection::open(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    for (_, sql) in objects {
        c.execute_batch(sql).unwrap();
    }
    c.execute("ATTACH DATABASE ?1 AS source", [f.db().to_str().unwrap()])
        .unwrap();
    for table in tables {
        c.execute_batch(&format!("INSERT INTO {table} SELECT * FROM source.{table}"))
            .unwrap();
    }
    c.execute_batch(&format!("INSERT INTO authority_metadata SELECT id,{level},stream_id,last_sequence FROM source.authority_metadata; PRAGMA user_version={level};")).unwrap();
    path
}
/// A live v6 store without approvals, with every pre-v6 table populated.
fn populated() -> Fixture {
    let mut f = Fixture::with(
        vec![
            ExecutionScope::SessionCreate,
            ExecutionScope::SessionResume,
            ExecutionScope::TurnStart,
        ],
        3600,
    );
    f.next_turn();
    f.store
        .transition_turn(
            &f.turn.managed_turn_id,
            None,
            TurnState::Completed,
            Some(TurnReason::Completed),
        )
        .unwrap();
    f.store
        .apply(&Operation::RuntimeStart(RuntimeMutation {
            intent_id: uuid(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
            expected_revision: None,
        }))
        .unwrap();
    for table in TABLES {
        assert!(
            count(&f.store.conn, &format!("SELECT count(*) FROM {table}")) > 0,
            "{table}"
        );
    }
    assert!(count(&f.store.conn, "SELECT count(*) FROM managed_turns") == 2);
    f
}
#[test]
fn v5_migration_preserves_every_row_byte_and_sequence_adds_no_approval_and_rolls_back() {
    let f = populated();
    let path = older(&f, "v5", schema::V5_OBJECTS, 5, &TABLES);
    let mut c = Connection::open(&path).unwrap();
    let before = snapshot(&c, &TABLES);
    let old_master = master(&c);
    let (_, stream, head) = metadata(&c);
    let enforced = pragma(&c, "foreign_keys");
    // The v5 lease table really could not hold the five-scope encoding.
    let widest = serde_json::to_string(&all_scopes()).unwrap();
    assert!(
        c.execute("UPDATE execution_leases SET scopes=?1", [&widest])
            .is_err()
    );
    assert!(schema::fail_migration(&mut c).is_err());
    assert_eq!(version(&c), 5);
    assert_eq!(master(&c), old_master);
    assert_eq!(snapshot(&c, &TABLES), before);
    assert_eq!(metadata(&c), (5, stream.clone(), head.clone()));
    assert_eq!(
        (pragma(&c, "foreign_keys"), pragma(&c, "legacy_alter_table")),
        (enforced, 0)
    );
    schema::migrate(&mut c).unwrap();
    assert_eq!(version(&c), 6);
    assert_eq!(master(&c), expected_master());
    assert_eq!(snapshot(&c, &TABLES), before);
    assert_eq!(metadata(&c), (6, stream, head));
    assert_eq!(count(&c, "SELECT count(*) FROM managed_approvals"), 0);
    assert_eq!(
        count(&c, "SELECT count(*) FROM pragma_foreign_key_check"),
        0
    );
    assert_eq!(
        (pragma(&c, "foreign_keys"), pragma(&c, "legacy_alter_table")),
        (enforced, 0)
    );
    // The rebuilt parent kept its name in every child reference.
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM sqlite_master WHERE sql LIKE '%execution_leases_old%' OR name LIKE '%_old'"
        ),
        0
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM sqlite_master WHERE name='managed_turns' AND sql LIKE '%REFERENCES execution_leases(lease_id)%'"
        ),
        1
    );
    // Idempotent: migrating a current database is validation only.
    schema::migrate(&mut c).unwrap();
    assert_eq!(snapshot(&c, &TABLES), before);
    drop(c);
    // The migrated database is a fully working authority store, approvals included.
    let mut store = Store::open(&path, uuid()).unwrap();
    store.set_clock(f.clock.clone());
    assert_eq!(store.status().unwrap().storage_schema_version, 6);
    assert_eq!(snapshot(&store.conn, &TABLES), before);
    let narrow = leases::active(&store.conn, f.session.managed_session_id.as_str())
        .unwrap()
        .unwrap();
    store
        .apply(&Operation::LeaseRevoke(LeaseRevoke {
            intent_id: uuid(),
            lease_id: narrow.lease_id,
            expected_revision: narrow.revision,
        }))
        .unwrap();
    let lease = grant(&mut store, &f.session, all_scopes(), 3600);
    let turn = start_turn(&mut store, &f.session, &f.binding, &lease, &f.runtime, 9);
    let a = store
        .open_approval(
            &turn.managed_turn_id,
            &f.runtime,
            CommandExecution,
            "call_1",
            &digest("call_1"),
            shape(CommandExecution),
        )
        .unwrap();
    assert!(matches!(
        store
            .decide_approval(&respond(&a, Accept), &a.action_fingerprint)
            .unwrap(),
        Decided::Fresh(..)
    ));
    drop(store);
    Store::open(&path, uuid()).unwrap();
}
#[test]
fn a_v4_database_upgraded_through_v5_reaches_v6_with_every_row_preserved() {
    let v4_tables: Vec<&str> = TABLES
        .iter()
        .copied()
        .filter(|t| *t != "managed_turns")
        .collect();
    // The turn journal did not exist in v4: take a source without turns.
    let source = Fixture::with(
        vec![ExecutionScope::SessionCreate, ExecutionScope::TurnStart],
        600,
    );
    source
        .store
        .conn
        .set_db_config(
            rusqlite::config::DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER,
            false,
        )
        .unwrap();
    source
        .store
        .conn
        .execute_batch("DELETE FROM control_events WHERE envelope LIKE '%authority.managed_turn.%'; DELETE FROM processed_intents WHERE mutation LIKE '%managedSession/turn/start%'; DELETE FROM managed_turns;")
        .unwrap();
    let path = older(&source, "v4", schema::V4_OBJECTS, 4, &v4_tables);
    let mut c = Connection::open(&path).unwrap();
    let before = snapshot(&c, &v4_tables);
    let (_, stream, head) = metadata(&c);
    // The v4 -> v5 step exactly as the v5 release performed it.
    c.execute_batch("BEGIN; ALTER TABLE authority_metadata RENAME TO authority_metadata_old;")
        .unwrap();
    for (name, sql) in schema::V5_OBJECTS {
        if *name == "authority_metadata" || !schema::V4_OBJECTS.iter().any(|(n, _)| n == name) {
            c.execute_batch(sql).unwrap();
        }
    }
    c.execute_batch("INSERT INTO authority_metadata SELECT id,5,stream_id,last_sequence FROM authority_metadata_old; DROP TABLE authority_metadata_old; PRAGMA user_version=5; COMMIT;").unwrap();
    let mut v5: Vec<_> = schema::V5_OBJECTS
        .iter()
        .map(|(n, s)| ((*n).to_owned(), (*s).to_owned()))
        .collect();
    v5.sort();
    assert_eq!(master(&c), v5);
    assert_eq!(snapshot(&c, &v4_tables), before);
    schema::migrate(&mut c).unwrap();
    assert_eq!((version(&c), master(&c)), (6, expected_master()));
    assert_eq!(snapshot(&c, &v4_tables), before);
    assert_eq!(metadata(&c), (6, stream, head));
    assert_eq!(count(&c, "SELECT count(*) FROM managed_approvals"), 0);
    assert_eq!(count(&c, "SELECT count(*) FROM managed_turns"), 0);
}
#[test]
fn every_supported_version_converges_on_the_one_v6_schema_and_others_fail_closed() {
    let root = std::env::temp_dir().join(format!("wyss-approval-schema-{}", uuid().as_str()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let stream = uuid();
    let build = |level: i64, objects: &[(&str, &str)]| -> Connection {
        let c = Connection::open(root.join(format!("v{level}-{}", uuid().as_str()))).unwrap();
        for (_, sql) in objects {
            c.execute_batch(sql).unwrap();
        }
        if level > 0 {
            c.execute(
                &format!("INSERT INTO authority_metadata VALUES(1,{level},?1,'0')"),
                [stream.as_str()],
            )
            .unwrap();
        }
        c.pragma_update(None, "user_version", level).unwrap();
        c
    };
    let chain: [(i64, Objects); 7] = [
        (0, &[]),
        (1, schema::V1_OBJECTS),
        (2, schema::V2_OBJECTS),
        (3, schema::V3_OBJECTS),
        (4, schema::V4_OBJECTS),
        (5, schema::V5_OBJECTS),
        (6, schema::OBJECTS),
    ];
    for (level, objects) in chain {
        let mut c = build(level, objects);
        schema::migrate(&mut c).unwrap();
        assert_eq!(
            (version(&c), master(&c)),
            (6, expected_master()),
            "v{level}"
        );
        assert_eq!(count(&c, "SELECT count(*) FROM managed_approvals"), 0);
        assert_eq!(
            count(&c, "SELECT count(*) FROM pragma_foreign_key_check"),
            0
        );
        let (schema_version, migrated_stream, head) = metadata(&c);
        assert_eq!((schema_version, head.as_str()), (6, "0"));
        if level > 0 {
            assert_eq!(migrated_stream, stream.as_str());
        }
    }
    // Future versions, and any schema that is not exactly the one it claims to be.
    for level in [7, 99, -1] {
        let mut c = build(6, schema::OBJECTS);
        c.pragma_update(None, "user_version", level).unwrap();
        assert_eq!(
            schema::migrate(&mut c),
            Err(ControlError::StorageIncompatible)
        );
    }
    let corruptions: [(i64, Objects, &str); 8] = [
        (
            6,
            schema::OBJECTS,
            "DROP TRIGGER managed_approvals_transition",
        ),
        (
            6,
            schema::OBJECTS,
            "DROP TRIGGER managed_approvals_no_delete",
        ),
        (
            6,
            schema::OBJECTS,
            "DROP INDEX managed_approvals_one_pending",
        ),
        (6, schema::OBJECTS, "DROP TABLE managed_approvals"),
        (
            6,
            schema::OBJECTS,
            "CREATE TABLE approval_content (command TEXT)",
        ),
        (
            6,
            schema::OBJECTS,
            "CREATE INDEX extra ON managed_approvals(state)",
        ),
        (
            5,
            schema::V5_OBJECTS,
            "CREATE TABLE managed_approvals (id TEXT)",
        ),
        (5, schema::V5_OBJECTS, "DROP INDEX managed_turns_one_active"),
    ];
    for (level, objects, sql) in corruptions {
        let mut c = build(level, objects);
        c.execute_batch(sql).unwrap();
        assert_eq!(
            schema::migrate(&mut c),
            Err(ControlError::StorageIncompatible),
            "{sql}"
        );
        assert_eq!(version(&c), level, "{sql}");
    }
    // A v5 file whose lease table already has another shape is not silently rebuilt.
    let mut c = build(0, &[]);
    for (name, sql) in schema::V5_OBJECTS {
        let sql = if *name == "execution_leases" {
            sql.replace("length(scopes)<=64", "length(scopes)<=4096")
        } else {
            (*sql).to_owned()
        };
        c.execute_batch(&sql).unwrap();
    }
    c.execute(
        "INSERT INTO authority_metadata VALUES(1,5,?1,'0')",
        [stream.as_str()],
    )
    .unwrap();
    c.pragma_update(None, "user_version", 5).unwrap();
    assert_eq!(
        schema::migrate(&mut c),
        Err(ControlError::StorageIncompatible)
    );
    drop(c);
    fs::remove_dir_all(&root).unwrap();
}
#[test]
fn scope_bound_is_the_exact_canonical_maximum_and_overflow_is_refused() {
    let all = all_scopes();
    let mut longest = 0;
    let mut combinations = 0;
    for mask in 1u32..(1 << all.len()) {
        let scopes: Vec<_> = all
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, s)| *s)
            .collect();
        validate_scopes(&scopes).unwrap();
        longest = longest.max(serde_json::to_string(&scopes).unwrap().len());
        combinations += 1;
    }
    assert_eq!(combinations, 31);
    assert_eq!(longest, MAX_SCOPES_TEXT);
    assert_eq!(serde_json::to_string(&all).unwrap().len(), MAX_SCOPES_TEXT);
    assert_eq!(
        serde_json::to_string(&all).unwrap(),
        "[\"session_create\",\"session_resume\",\"turn_start\",\"command_execute\",\"file_change\"]"
    );
    assert!(schema::OBJECTS.iter().any(|(n, s)| *n == "execution_leases"
        && s.contains(&format!("CHECK(length(scopes)<={MAX_SCOPES_TEXT})"))));
    // Exact maximum through the real grant path, and read back intact after reopening.
    let mut f = Fixture::new();
    assert_eq!(f.lease.scopes, all);
    assert_eq!(
        count(&f.store.conn, "SELECT length(scopes) FROM execution_leases"),
        MAX_SCOPES_TEXT as i64
    );
    f.reopen();
    assert_eq!(
        leases::read(&f.store.conn, f.lease.lease_id.as_str())
            .unwrap()
            .scopes,
        all
    );
    // One character more is refused by the schema itself, and so is any unknown scope.
    let padded = format!("{} ", serde_json::to_string(&all).unwrap());
    assert_eq!(padded.len(), MAX_SCOPES_TEXT + 1);
    let error = f
        .store
        .conn
        .execute("UPDATE execution_leases SET scopes=?1", [&padded])
        .unwrap_err();
    assert!(error.to_string().contains("CHECK"), "{error}");
    let mut six = all.clone();
    six.push(ExecutionScope::FileChange);
    assert_eq!(validate_scopes(&six), Err(ControlError::InvalidRequest));
    assert!(serde_json::from_str::<Vec<ExecutionScope>>("[\"network_access\"]").is_err());
}
