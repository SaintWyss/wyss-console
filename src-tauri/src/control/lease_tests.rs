use super::*;
use crate::{
    contracts::{
        agent_adapter::{AgentAdapterId, AgentRuntimeKind, CompatibilityState},
        identities::AgentRuntimeInstanceId,
    },
    control::gate::{ExecutionDecision, ExecutionGate},
};
use std::{fs, os::unix::fs::DirBuilderExt, sync::Mutex};
struct TestClock(Mutex<OffsetDateTime>);
impl Clock for TestClock {
    fn now(&self) -> OffsetDateTime {
        *self.0.lock().unwrap()
    }
}
impl TestClock {
    fn advance(&self, seconds: i64) {
        *self.0.lock().unwrap() += time::Duration::seconds(seconds);
    }
}
struct Fixture {
    root: std::path::PathBuf,
    store: Store,
    clock: Arc<TestClock>,
    session: ManagedSessionRecord,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-lease-{}", uuid().as_str()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let mut store = Store::open(&root.join("control.sqlite3"), uuid()).unwrap();
        let clock = Arc::new(TestClock(Mutex::new(
            OffsetDateTime::parse("2026-10-07T00:00:00Z", &Rfc3339).unwrap(),
        )));
        store.set_clock(clock.clone());
        let session = prepare(&mut store);
        Self {
            root,
            store,
            clock,
            session,
        }
    }
    fn grant_op(&self, scopes: Vec<ExecutionScope>, ttl: u32) -> Operation {
        Operation::LeaseGrant(LeaseGrant {
            intent_id: uuid(),
            session_id: self.session.managed_session_id.clone(),
            expected_session_revision: 1,
            scopes,
            ttl_seconds: ttl,
        })
    }
    fn grant(&mut self) -> ExecutionLease {
        let op = self.grant_op(
            vec![ExecutionScope::SessionCreate, ExecutionScope::TurnStart],
            60,
        );
        lease(self.store.apply(&op).unwrap())
    }
    fn gate(&self, scope: ExecutionScope) -> GateReason {
        ExecutionGate::evaluate(
            &self.store,
            &self.session.managed_session_id,
            scope,
            &ready(),
        )
        .reason()
    }
    fn reopen(&mut self) {
        let next = Store::open(&self.root.join("control.sqlite3"), uuid()).unwrap();
        self.store = next;
        self.store.set_clock(self.clock.clone());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn prepare(s: &mut Store) -> ManagedSessionRecord {
    match s
        .apply(&Operation::Prepare(Prepare {
            intent_id: uuid(),
            adapter_id: AgentAdapterId::parse("codex").unwrap(),
            runtime_kind: AgentRuntimeKind::Codex,
        }))
        .unwrap()
    {
        ResultValue::Session(s) => s,
        _ => panic!(),
    }
}
fn lease(r: ResultValue) -> ExecutionLease {
    match r {
        ResultValue::Lease(l) => l,
        _ => panic!(),
    }
}
fn renew(l: &ExecutionLease, ttl: u32) -> Operation {
    Operation::LeaseRenew(LeaseRenew {
        intent_id: uuid(),
        lease_id: l.lease_id.clone(),
        expected_revision: l.revision,
        ttl_seconds: ttl,
    })
}
fn revoke(l: &ExecutionLease) -> Operation {
    Operation::LeaseRevoke(LeaseRevoke {
        intent_id: uuid(),
        lease_id: l.lease_id.clone(),
        expected_revision: l.revision,
    })
}
fn close_op(s: &ManagedSessionRecord) -> Operation {
    Operation::Close(Close {
        intent_id: uuid(),
        managed_session_id: s.managed_session_id.clone(),
        expected_revision: s.revision,
    })
}
fn audit(s: &Store) -> Vec<Value> {
    s.conn
        .prepare("SELECT envelope FROM control_events ORDER BY sequence")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
        .collect()
}
fn ready() -> RuntimeStatus {
    RuntimeStatus {
        adapter_id: AgentAdapterId::parse("codex").unwrap(),
        runtime_kind: AgentRuntimeKind::Codex,
        desired_state: DesiredState::Running,
        revision: 1,
        actual_state: ActualState::Ready,
        runtime_instance_id: Some(AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap()),
        runtime_version: Some("0.159.0".into()),
        compatibility: CompatibilityState::Compatible,
        model_count: 1,
        diagnostic: None,
        runtime_safety_profile: Some(RuntimeSafetyProfile::CodexInferenceV1),
        workspace_root: None,
        outbound_methods: RuntimeMethods {
            initialize: 1,
            initialized: 1,
            model_list: 1,
            ..RuntimeMethods::default()
        },
    }
}
fn fail_audit(s: &Store) {
    s.conn.execute_batch("CREATE TEMP TRIGGER fail_lease_audit BEFORE INSERT ON control_events BEGIN SELECT RAISE(ABORT,'test'); END").unwrap();
}
#[test]
fn default_deny_and_grant_has_distinct_identity_revision_ttl_sorted_scopes() {
    let mut f = Fixture::new();
    assert_eq!(
        f.gate(ExecutionScope::SessionCreate),
        GateReason::LeaseMissing
    );
    let l = f.grant();
    assert_ne!(l.lease_id.as_str(), f.session.managed_session_id.as_str());
    assert_eq!(l.revision, 1);
    assert_eq!(
        timestamp(&l.expires_at).unwrap() - timestamp(&l.issued_at).unwrap(),
        time::Duration::seconds(60)
    );
    assert_eq!(f.gate(ExecutionScope::SessionCreate), GateReason::Allowed);
    assert_eq!(
        f.gate(ExecutionScope::TurnStart),
        GateReason::BindingDetached
    );
    assert_eq!(
        f.gate(ExecutionScope::SessionResume),
        GateReason::ScopeNotGranted
    );
    assert_eq!(
        get_session(&f.store.conn, f.session.managed_session_id.as_str()).unwrap(),
        f.session
    );
    assert!(f.store.runtime_control().unwrap().is_none());
}
#[test]
fn validation_rejects_empty_duplicate_invalid_scopes_ttl_and_free_form_fields() {
    let mut f = Fixture::new();
    for (scopes, ttl) in [
        (vec![], 1),
        (vec![ExecutionScope::TurnStart; 2], 1),
        (vec![ExecutionScope::TurnStart], 0),
        (vec![ExecutionScope::TurnStart], 3601),
    ] {
        let op = f.grant_op(scopes, ttl);
        assert!(matches!(
            f.store.apply(&op),
            Err(ControlError::InvalidRequest)
        ));
    }
    let op = f.grant_op(vec![ExecutionScope::TurnStart], 1);
    let mut v = serde_json::to_value(op).unwrap();
    for scope in ["*", "all", "admin", "shell", "tool_execution"] {
        v["params"]["scopes"] = json!([scope]);
        assert!(serde_json::from_value::<Operation>(v.clone()).is_err());
    }
    v["params"]["scopes"] = json!(["turn_start"]);
    v["params"]["prompt"] = json!("forbidden");
    assert!(serde_json::from_value::<Operation>(v).is_err());
    assert_eq!(audit(&f.store).len(), 1);
}
#[test]
fn grant_rejects_missing_closed_and_stale_session() {
    let mut f = Fixture::new();
    let mut p = match f.grant_op(vec![ExecutionScope::TurnStart], 60) {
        Operation::LeaseGrant(p) => p,
        _ => panic!(),
    };
    p.expected_session_revision = 2;
    assert!(matches!(
        f.store.apply(&Operation::LeaseGrant(p.clone())),
        Err(ControlError::StaleRevision)
    ));
    p.expected_session_revision = 1;
    p.session_id = ManagedSessionId::parse(uuid().as_str()).unwrap();
    assert!(matches!(
        f.store.apply(&Operation::LeaseGrant(p)),
        Err(ControlError::SessionNotFound)
    ));
    f.store.apply(&close_op(&f.session)).unwrap();
    assert!(matches!(
        f.store
            .apply(&f.grant_op(vec![ExecutionScope::TurnStart], 60)),
        Err(ControlError::InvalidTransition)
    ));
}
#[test]
fn grant_retry_normalizes_scope_order_uuid_case_and_conflicts_across_methods() {
    let mut f = Fixture::new();
    let op = f.grant_op(
        vec![ExecutionScope::TurnStart, ExecutionScope::SessionCreate],
        60,
    );
    let original = lease(f.store.apply(&op).unwrap());
    let mut reordered = match op.clone() {
        Operation::LeaseGrant(p) => p,
        _ => panic!(),
    };
    reordered.scopes.reverse();
    reordered.intent_id = WireUuid::try_from(reordered.intent_id.as_str().to_uppercase()).unwrap();
    reordered.session_id =
        ManagedSessionId::parse(&reordered.session_id.as_str().to_uppercase()).unwrap();
    f.reopen();
    assert_eq!(
        lease(
            f.store
                .apply(&Operation::LeaseGrant(reordered.clone()))
                .unwrap()
        ),
        original
    );
    assert_eq!(audit(&f.store).len(), 2);
    reordered.ttl_seconds = 61;
    assert!(matches!(
        f.store.apply(&Operation::LeaseGrant(reordered.clone())),
        Err(ControlError::IntentConflict)
    ));
    let cross = Operation::LeaseRevoke(LeaseRevoke {
        intent_id: reordered.intent_id,
        lease_id: original.lease_id,
        expected_revision: 1,
    });
    assert!(matches!(
        f.store.apply(&cross),
        Err(ControlError::IntentConflict)
    ));
}
#[test]
fn one_active_lease_is_enforced_by_control_and_database() {
    let mut f = Fixture::new();
    let l = f.grant();
    assert!(matches!(
        f.store
            .apply(&f.grant_op(vec![ExecutionScope::TurnStart], 60)),
        Err(ControlError::LeaseAlreadyActive)
    ));
    assert!(f.store.conn.execute("INSERT INTO execution_leases SELECT ?1,session_id,state,scopes,revision,issued_at,expires_at,updated_at,revocation_reason FROM execution_leases WHERE lease_id=?2",params![uuid().as_str(),l.lease_id.as_str()]).is_err());
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT count(*) FROM execution_leases WHERE state='active'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}
#[test]
fn renew_uses_now_not_prior_expiry_preserves_scopes_and_retries_original_receipt() {
    let mut f = Fixture::new();
    let l = f.grant();
    f.clock.advance(10);
    let op = renew(&l, 30);
    let renewed = lease(f.store.apply(&op).unwrap());
    assert_eq!(renewed.lease_id, l.lease_id);
    assert_eq!(renewed.scopes, l.scopes);
    assert_eq!(renewed.revision, 2);
    assert_eq!(
        timestamp(&renewed.expires_at).unwrap(),
        f.clock.now() + time::Duration::seconds(30)
    );
    f.clock.advance(5);
    f.reopen();
    assert_eq!(lease(f.store.apply(&op).unwrap()), renewed);
    assert_eq!(audit(&f.store).len(), 3);
    assert!(matches!(
        f.store.apply(&renew(&l, 60)),
        Err(ControlError::StaleRevision)
    ));
    let mut v = serde_json::to_value(renew(&renewed, 60)).unwrap();
    v["params"]["scopes"] = json!(["session_resume"]);
    assert!(serde_json::from_value::<Operation>(v).is_err());
}
#[test]
fn revoke_is_terminal_idempotent_and_does_not_close_or_stop() {
    let mut f = Fixture::new();
    let l = f.grant();
    let mut stale = l.clone();
    stale.revision = 2;
    assert!(matches!(
        f.store.apply(&revoke(&stale)),
        Err(ControlError::StaleRevision)
    ));
    let op = revoke(&l);
    let revoked = lease(f.store.apply(&op).unwrap());
    assert_eq!(revoked.state, LeaseState::Revoked);
    assert_eq!(revoked.revision, 2);
    assert_eq!(
        revoked.revocation_reason,
        Some(RevocationReason::UserRequest)
    );
    assert!(matches!(
        f.store.apply(&revoke(&revoked)),
        Err(ControlError::InvalidTransition)
    ));
    assert!(matches!(
        f.store.apply(&renew(&revoked, 60)),
        Err(ControlError::InvalidTransition)
    ));
    assert_eq!(lease(f.store.apply(&op).unwrap()), revoked);
    assert_eq!(
        f.gate(ExecutionScope::TurnStart),
        GateReason::LeaseNotActive
    );
    assert_eq!(
        get_session(&f.store.conn, f.session.managed_session_id.as_str()).unwrap(),
        f.session
    );
    assert!(f.store.runtime_control().unwrap().is_none());
    let fresh = f.grant();
    assert_ne!(fresh.lease_id, l.lease_id);
    assert_eq!(fresh.revision, 1);
}
#[test]
fn expiry_denies_at_deadline_before_reconciliation_and_audits_once() {
    let mut f = Fixture::new();
    let l = f.grant();
    f.clock.advance(59);
    assert_eq!(
        f.gate(ExecutionScope::TurnStart),
        GateReason::BindingDetached
    );
    f.clock.advance(1);
    assert_eq!(
        read(&f.store.conn, l.lease_id.as_str()).unwrap().state,
        LeaseState::Active
    );
    assert_eq!(f.gate(ExecutionScope::TurnStart), GateReason::LeaseExpired);
    assert!(matches!(
        f.store.apply(&renew(&l, 60)),
        Err(ControlError::InvalidTransition)
    ));
    assert!(matches!(
        f.store.apply(&revoke(&l)),
        Err(ControlError::InvalidTransition)
    ));
    let get = lease(
        f.store
            .apply(&Operation::LeaseGet(LeaseGet {
                lease_id: l.lease_id.clone(),
            }))
            .unwrap(),
    );
    assert_eq!(get.state, LeaseState::Expired);
    assert_eq!(get.revision, 1);
    assert_eq!(audit(&f.store).len(), 2);
    assert!(matches!(
        f.store
            .apply(&Operation::LeaseActive(LeaseActive {
                session_id: l.session_id
            }))
            .unwrap(),
        ResultValue::ActiveLease(None)
    ));
    f.store.expire_due().unwrap();
    let expired = read(&f.store.conn, l.lease_id.as_str()).unwrap();
    assert_eq!(expired.state, LeaseState::Expired);
    assert_eq!(expired.revision, 2);
    f.reopen();
    f.store.expire_due().unwrap();
    let events = audit(&f.store);
    assert_eq!(events.len(), 3);
    assert_eq!(events[2]["event_type"], "authority.execution_lease.expired");
    assert!(events[2]["correlation_id"].is_null());
    let fresh = f.grant();
    assert_ne!(fresh.lease_id, l.lease_id);
}
#[test]
fn grant_reconciles_overdue_active_in_same_transaction() {
    let mut f = Fixture::new();
    let old = f.grant();
    f.clock.advance(60);
    let new = f.grant();
    assert_ne!(old.lease_id, new.lease_id);
    assert_eq!(
        read(&f.store.conn, old.lease_id.as_str()).unwrap().state,
        LeaseState::Expired
    );
    let a = audit(&f.store);
    assert_eq!(a[2]["event_type"], "authority.execution_lease.expired");
    assert_eq!(a[3]["event_type"], "authority.execution_lease.granted");
}
#[test]
fn restart_keeps_remaining_window_identity_revision_and_deadline() {
    let mut f = Fixture::new();
    let l = f.grant();
    f.clock.advance(20);
    f.reopen();
    f.store.expire_due().unwrap();
    assert_eq!(read(&f.store.conn, l.lease_id.as_str()).unwrap(), l);
    assert_eq!(
        f.gate(ExecutionScope::TurnStart),
        GateReason::BindingDetached
    );
    f.clock.advance(40);
    f.reopen();
    assert_eq!(f.gate(ExecutionScope::TurnStart), GateReason::LeaseExpired);
    f.store.expire_due().unwrap();
    assert_eq!(
        read(&f.store.conn, l.lease_id.as_str()).unwrap().revision,
        2
    );
}
#[test]
fn close_revokes_then_closes_atomically_and_retries_without_new_events() {
    let mut f = Fixture::new();
    let l = f.grant();
    let op = close_op(&f.session);
    f.store.apply(&op).unwrap();
    let revoked = read(&f.store.conn, l.lease_id.as_str()).unwrap();
    assert_eq!(
        revoked.revocation_reason,
        Some(RevocationReason::SessionClosed)
    );
    assert_eq!(revoked.revision, 2);
    let a = audit(&f.store);
    assert_eq!(a[2]["event_type"], "authority.execution_lease.revoked");
    assert_eq!(a[3]["event_type"], "authority.managed_session.closed");
    assert_eq!(a[2]["correlation_id"], a[3]["correlation_id"]);
    f.store.apply(&op).unwrap();
    assert_eq!(audit(&f.store).len(), 4);
    assert_eq!(
        f.gate(ExecutionScope::TurnStart),
        GateReason::SessionNotPrepared
    );
    assert!(matches!(
        f.store.apply(&renew(&l, 60)),
        Err(ControlError::InvalidTransition)
    ));
}
#[test]
fn close_overdue_expires_instead_of_revoking() {
    let mut f = Fixture::new();
    let l = f.grant();
    f.clock.advance(60);
    f.store.apply(&close_op(&f.session)).unwrap();
    let expired = read(&f.store.conn, l.lease_id.as_str()).unwrap();
    assert_eq!(expired.state, LeaseState::Expired);
    assert_eq!(expired.revocation_reason, None);
    assert_eq!(
        audit(&f.store)[2]["event_type"],
        "authority.execution_lease.expired"
    );
}
#[test]
fn close_second_audit_failure_rolls_back_session_lease_audit_and_receipt() {
    let mut f = Fixture::new();
    let l = f.grant();
    f.store.conn.execute_batch("CREATE TEMP TRIGGER fail_close_audit BEFORE INSERT ON control_events WHEN NEW.envelope LIKE '%authority.managed_session.closed%' BEGIN SELECT RAISE(ABORT,'test'); END").unwrap();
    let op = close_op(&f.session);
    assert!(f.store.apply(&op).is_err());
    assert_eq!(
        get_session(&f.store.conn, f.session.managed_session_id.as_str()).unwrap(),
        f.session
    );
    assert_eq!(read(&f.store.conn, l.lease_id.as_str()).unwrap(), l);
    assert_eq!(audit(&f.store).len(), 2);
    f.store
        .conn
        .execute_batch("DROP TRIGGER fail_close_audit")
        .unwrap();
    f.store.apply(&op).unwrap();
    assert_eq!(audit(&f.store).len(), 4);
}
#[test]
fn grant_renew_revoke_and_expiry_audit_failures_roll_back() {
    let mut f = Fixture::new();
    fail_audit(&f.store);
    let op = f.grant_op(vec![ExecutionScope::TurnStart], 60);
    assert!(f.store.apply(&op).is_err());
    assert!(
        active(&f.store.conn, f.session.managed_session_id.as_str())
            .unwrap()
            .is_none()
    );
    f.store
        .conn
        .execute_batch("DROP TRIGGER fail_lease_audit")
        .unwrap();
    let l = lease(f.store.apply(&op).unwrap());
    fail_audit(&f.store);
    assert!(f.store.apply(&renew(&l, 120)).is_err());
    assert!(f.store.apply(&revoke(&l)).is_err());
    assert_eq!(read(&f.store.conn, l.lease_id.as_str()).unwrap(), l);
    f.clock.advance(60);
    assert!(f.store.expire_due().is_err());
    assert_eq!(f.gate(ExecutionScope::TurnStart), GateReason::LeaseExpired);
    assert!(crate::control::server::refresh_expiry(&mut f.store).is_none());
    assert!(!f.store.execution_available);
    assert_eq!(
        f.gate(ExecutionScope::TurnStart),
        GateReason::AuthorityUnavailable
    );
    assert_eq!(audit(&f.store).len(), 2);
}
#[test]
fn gate_checks_each_runtime_precondition_independently() {
    let mut f = Fixture::new();
    f.grant();
    let r = ready();
    let sid = &f.session.managed_session_id;
    assert!(matches!(
        ExecutionGate::evaluate(&f.store, sid, ExecutionScope::TurnStart, &r),
        ExecutionDecision::Denied(GateReason::BindingDetached)
    ));
    let mut cases = Vec::new();
    let mut v = r.clone();
    v.desired_state = DesiredState::Stopped;
    cases.push((v, GateReason::RuntimeNotRunning));
    for state in [
        ActualState::Stopped,
        ActualState::Starting,
        ActualState::Initializing,
        ActualState::Failed,
        ActualState::Stopping,
    ] {
        let mut v = r.clone();
        v.actual_state = state;
        cases.push((v, GateReason::RuntimeNotReady));
    }
    let mut v = r.clone();
    v.runtime_instance_id = None;
    cases.push((v, GateReason::RuntimeIdentityMissing));
    for c in [
        CompatibilityState::Unknown,
        CompatibilityState::Incompatible,
        CompatibilityState::Degraded,
    ] {
        let mut v = r.clone();
        v.compatibility = c;
        cases.push((v, GateReason::RuntimeIncompatible));
    }
    let mut v = r.clone();
    v.adapter_id = AgentAdapterId::parse("claude").unwrap();
    cases.push((v, GateReason::RuntimeMismatch));
    for (v, want) in cases {
        assert_eq!(
            ExecutionGate::evaluate(&f.store, sid, ExecutionScope::TurnStart, &v).reason(),
            want
        );
    }
    assert_eq!(
        ExecutionGate::evaluate(
            &f.store,
            &ManagedSessionId::parse(uuid().as_str()).unwrap(),
            ExecutionScope::TurnStart,
            &r
        )
        .reason(),
        GateReason::SessionNotFound
    );
    f.store.execution_available = false;
    assert_eq!(
        f.gate(ExecutionScope::TurnStart),
        GateReason::AuthorityUnavailable
    );
}
#[test]
fn scopes_have_no_implied_hierarchy_and_runtime_availability_does_not_change_permission() {
    let mut f = Fixture::new();
    let op = f.grant_op(vec![ExecutionScope::SessionCreate], 3600);
    let l = lease(f.store.apply(&op).unwrap());
    assert_eq!(f.gate(ExecutionScope::SessionCreate), GateReason::Allowed);
    assert_eq!(
        f.gate(ExecutionScope::TurnStart),
        GateReason::ScopeNotGranted
    );
    assert_eq!(
        f.gate(ExecutionScope::SessionResume),
        GateReason::ScopeNotGranted
    );
    let p = RuntimeMutation {
        intent_id: uuid(),
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: AgentRuntimeKind::Codex,
        expected_revision: None,
        safety_profile: None,
        workspace_root: None,
    };
    f.store.apply(&Operation::RuntimeStop(p.clone())).unwrap();
    assert_eq!(read(&f.store.conn, l.lease_id.as_str()).unwrap(), l);
    let mut r = ready();
    r.desired_state = DesiredState::Stopped;
    assert_eq!(
        ExecutionGate::evaluate(&f.store, &l.session_id, ExecutionScope::SessionCreate, &r)
            .reason(),
        GateReason::RuntimeNotRunning
    );
    f.store
        .apply(&Operation::RuntimeStart(RuntimeMutation {
            intent_id: uuid(),
            ..p
        }))
        .unwrap();
    assert_eq!(read(&f.store.conn, l.lease_id.as_str()).unwrap(), l);
}
#[test]
fn independent_sessions_keep_independent_scopes_and_revocation() {
    let mut f = Fixture::new();
    let a = f.grant();
    let second = prepare(&mut f.store);
    let b = lease(
        f.store
            .apply(&Operation::LeaseGrant(LeaseGrant {
                intent_id: uuid(),
                session_id: second.managed_session_id.clone(),
                expected_session_revision: 1,
                scopes: vec![ExecutionScope::SessionResume],
                ttl_seconds: 120,
            }))
            .unwrap(),
    );
    assert_ne!(a.lease_id, b.lease_id);
    f.store.apply(&revoke(&a)).unwrap();
    assert_eq!(read(&f.store.conn, b.lease_id.as_str()).unwrap(), b);
    f.store.apply(&close_op(&f.session)).unwrap();
    assert_eq!(read(&f.store.conn, b.lease_id.as_str()).unwrap(), b);
    assert_eq!(
        f.store.next_expiry().unwrap(),
        Some(timestamp(&b.expires_at).unwrap())
    );
}
#[test]
fn audit_payload_is_minimal_ordered_monotonic_and_reopens() {
    let mut f = Fixture::new();
    let l = f.grant();
    let l = lease(f.store.apply(&renew(&l, 60)).unwrap());
    f.store.apply(&revoke(&l)).unwrap();
    f.reopen();
    for (i, e) in audit(&f.store).iter().enumerate() {
        assert_eq!(e["sequence"], json!((i + 1).to_string()));
        if i > 0 {
            assert_eq!(e["session_id"], json!(f.session.managed_session_id));
            let p = e["payload"].as_object().unwrap();
            assert_eq!(p.len(), if i == 3 { 6 } else { 5 });
            for forbidden in [
                "prompt",
                "model",
                "stderr",
                "pid",
                "environment",
                "runtime_frame",
            ] {
                assert!(!p.contains_key(forbidden));
            }
        }
    }
}
#[test]
fn corrupt_lease_scopes_and_closed_active_state_reject_reopen() {
    for corruption in [
        "UPDATE execution_leases SET scopes='[\"all\"]'",
        "UPDATE managed_sessions SET control_state='closed',revision=2",
    ] {
        let mut f = Fixture::new();
        f.grant();
        f.store.conn.execute_batch(corruption).unwrap();
        assert!(matches!(
            Store::open(&f.root.join("control.sqlite3"), uuid()),
            Err(ControlError::StorageIncompatible)
        ));
    }
}
fn v2_fixture(f: &mut Fixture) -> std::path::PathBuf {
    let p = RuntimeMutation {
        intent_id: uuid(),
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: AgentRuntimeKind::Codex,
        expected_revision: None,
        safety_profile: None,
        workspace_root: None,
    };
    f.store.apply(&Operation::RuntimeStart(p)).unwrap();
    let path = f.root.join("v2.sqlite3");
    paths::private_file(&path).unwrap();
    let c = Connection::open(&path).unwrap();
    for (_, sql) in schema::V2_OBJECTS {
        c.execute_batch(sql).unwrap();
    }
    c.execute(
        "ATTACH DATABASE ?1 AS source",
        [f.root.join("control.sqlite3").to_str().unwrap()],
    )
    .unwrap();
    c.execute_batch("INSERT INTO authority_metadata SELECT id,2,stream_id,last_sequence FROM source.authority_metadata; INSERT INTO managed_sessions SELECT * FROM source.managed_sessions; INSERT INTO managed_runtimes SELECT * FROM source.managed_runtimes; INSERT INTO processed_intents SELECT * FROM source.processed_intents; INSERT INTO control_events SELECT * FROM source.control_events; PRAGMA user_version=2;").unwrap();
    path
}
fn snapshot(c: &Connection) -> Value {
    let rows = |table: &str| {
        let mut q = c
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = q.column_count();
        q.query_map([], |r| {
            let mut values = Vec::new();
            for i in 0..columns {
                let v: rusqlite::types::Value = r.get(i)?;
                values.push(format!("{v:?}"));
            }
            Ok(values)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    json!({"sessions":rows("managed_sessions"),"runtime":rows("managed_runtimes"),"intents":rows("processed_intents"),"events":rows("control_events"),"metadata":c.query_row("SELECT stream_id,last_sequence FROM authority_metadata",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).unwrap()})
}
#[test]
fn real_v2_upgrade_preserves_all_bytes_metadata_runtime_receipts_and_creates_zero_leases() {
    let mut f = Fixture::new();
    let second = prepare(&mut f.store);
    f.store.apply(&close_op(&second)).unwrap();
    let path = v2_fixture(&mut f);
    let c = Connection::open(&path).unwrap();
    let before = snapshot(&c);
    drop(c);
    let mut s = Store::open(&path, uuid()).unwrap();
    assert_eq!(s.status().unwrap().storage_schema_version, 7);
    assert_eq!(snapshot(&s.conn), before);
    assert_eq!(s.next_expiry().unwrap(), None);
    assert_eq!(
        s.conn
            .query_row("SELECT count(*) FROM execution_leases", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let raw: String = s
        .conn
        .query_row(
            "SELECT mutation FROM processed_intents WHERE mutation LIKE '%runtime/start%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let op: Operation = serde_json::from_str(&raw).unwrap();
    s.apply(&op).unwrap();
    assert_eq!(snapshot(&s.conn), before);
    drop(s);
    assert_eq!(snapshot(&Store::open(&path, uuid()).unwrap().conn), before);
}
#[test]
fn failed_v2_migration_rolls_back_ddl_data_and_version() {
    let mut f = Fixture::new();
    let path = v2_fixture(&mut f);
    let mut c = Connection::open(&path).unwrap();
    let before = snapshot(&c);
    let sql = |c: &Connection| {
        c.query_row(
            "SELECT group_concat(sql,';') FROM sqlite_master WHERE sql IS NOT NULL",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };
    let old = sql(&c);
    assert!(schema::fail_migration(&mut c).is_err());
    assert_eq!(sql(&c), old);
    assert_eq!(snapshot(&c), before);
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        2
    );
    schema::migrate(&mut c).unwrap();
    assert_eq!(snapshot(&c), before);
}
#[test]
fn corrupt_v2_and_v3_and_future_version_fail_closed() {
    for version in [2, 3, 4, 5] {
        let mut f = Fixture::new();
        let path = if version == 2 {
            v2_fixture(&mut f)
        } else {
            f.root.join("control.sqlite3")
        };
        let c = Connection::open(&path).unwrap();
        if version == 5 {
            c.pragma_update(None, "user_version", 8).unwrap();
        } else {
            c.execute_batch("DROP TRIGGER control_events_no_delete")
                .unwrap();
        }
        drop(c);
        assert!(matches!(
            Store::open(&path, uuid()),
            Err(ControlError::StorageIncompatible)
        ));
    }
}
