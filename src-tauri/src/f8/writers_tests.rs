use super::*;
use crate::contracts::control::Operation;
use crate::{
    contracts::approval::ApprovalStructure,
    control::f8_fixture,
    f8::{
        allocator::{Admission, Allocator},
        migration,
        protocol::Request,
        v8::validate_v8,
    },
};
use serde_json::Value;

const A: &str = "00000000-0000-4000-8000-0000000000a1";
const B: &str = "00000000-0000-4000-8000-0000000000b2";
const FP: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn uuid() -> String {
    crate::control::store::uuid().as_str().to_owned()
}
fn wire(s: &str) -> WireUuid {
    WireUuid::try_from(s.to_owned()).unwrap()
}
fn intent(rev: u64) -> TurnIntent {
    TurnIntent {
        intent_id: wire(&uuid()),
        expected_session_revision: rev,
        prompt_fingerprint: FP.into(),
        prompt_byte_length: 12,
    }
}
fn structure() -> ApprovalStructure {
    ApprovalStructure {
        command_bytes: 20,
        cwd_bytes: 10,
        parsed_actions: 1,
        files: 0,
        diff_bytes: 0,
    }
}
fn head(c: &Connection) -> u64 {
    c.query_row("SELECT last_sequence FROM authority_metadata", [], |r| {
        r.get::<_, String>(0)
    })
    .unwrap()
    .parse()
    .unwrap()
}

struct World {
    conn: Connection,
    session: String,
    launch: RuntimeRef,
    lease: String,
}
/// A migrated V8 image with a ready launch of authority A and a fresh active lease of the session.
fn world() -> World {
    let mut source = f8_fixture::v7_with_completed_turn();
    let mut conn = migration::prepare(&mut source).unwrap().into_connection();
    let session: String = conn
        .query_row("SELECT session_id FROM managed_sessions", [], |r| r.get(0))
        .unwrap();
    let request = Request::parse(
        &serde_json::to_vec(&json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":A,
            "method":"runtime/start","params":{"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":session,"expected_generation":0,"expected_revision":0}}))
        .unwrap(),
    )
    .unwrap();
    let out = Allocator::new(&mut conn, wire(A))
        .unwrap()
        .start(
            &request,
            &Admission {
                live: None,
                slot_available: true,
            },
        )
        .unwrap();
    let launch = RuntimeRef {
        schema: 2,
        authority_instance_id: wire(A),
        managed_session_id: out.managed_session_id.clone(),
        runtime_instance_id: out.runtime_instance_id.clone().unwrap(),
        generation: out.generation,
    };
    Allocator::new(&mut conn, wire(A))
        .unwrap()
        .record_ready(&launch, "0.159.0", "compatible")
        .unwrap();
    let lease = clone_active_lease(&conn, &session);
    World {
        conn,
        session,
        launch,
        lease,
    }
}

/// An active lease built from the fixture's own grant (row, receipt and audit event), so the
/// baseline verifier sees exactly what a real grant leaves behind. Written by authority A.
fn clone_active_lease(c: &Connection, session: &str) -> String {
    let old: String = c
        .query_row("SELECT lease_id FROM execution_leases", [], |r| r.get(0))
        .unwrap();
    let (new, intent) = (uuid(), uuid());
    let old_intent: String = c
        .query_row(
            "SELECT intent_id FROM processed_intents WHERE json_extract(mutation,'$.method')='executionLease/grant'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let swap =
        |col: &str| format!("replace(replace({col},'{old}','{new}'),'{old_intent}','{intent}')");
    c.execute_batch(&format!(
        "INSERT INTO execution_leases SELECT {},session_id,'active',scopes,1,issued_at,expires_at,issued_at,NULL FROM execution_leases WHERE lease_id='{old}';
         INSERT INTO processed_intents SELECT {},{},replace({},'\"revoked\"','\"active\"') FROM processed_intents WHERE intent_id='{old_intent}';",
        swap("lease_id"), swap("intent_id"), swap("mutation"), swap("result")
    ))
    .unwrap();
    // The new lease also carries the action scopes.
    let wide = |col: &str| {
        format!(
            "replace({col},'\"turn_start\"]','\"turn_start\",\"command_execute\",\"file_change\"]')"
        )
    };
    c.execute_batch(&format!(
        "UPDATE execution_leases SET scopes={} WHERE lease_id='{new}'; UPDATE processed_intents SET mutation={},result={} WHERE intent_id='{intent}';",
        wide("scopes"), wide("mutation"), wide("result")
    ))
    .unwrap();
    // The cloned receipt result must say what a grant returned: active, revision 1, same expiry.
    let result: String = c
        .query_row(
            "SELECT result FROM processed_intents WHERE intent_id=?1",
            [&intent],
            |r| r.get(0),
        )
        .unwrap();
    let mut lease: Value = serde_json::from_str(&result).unwrap();
    lease["state"] = json!("active");
    lease["revision"] = json!(1);
    lease["updated_at"] = lease["issued_at"].clone();
    lease["revocation_reason"] = Value::Null;
    c.execute(
        "UPDATE processed_intents SET result=?2 WHERE intent_id=?1",
        params![intent, lease.to_string()],
    )
    .unwrap();
    let raw: String = c
        .query_row(
            "SELECT envelope FROM control_events WHERE json_extract(envelope,'$.event_type')='authority.execution_lease.granted' AND json_extract(envelope,'$.payload.lease_id')=?1",
            [&old],
            |r| r.get(0),
        )
        .unwrap();
    let mut v: Value = serde_json::from_str(&raw).unwrap();
    let n = head(c) + 1;
    let id = uuid();
    v["event_id"] = json!(id);
    v["sequence"] = json!(n.to_string());
    v["payload"]["lease_id"] = json!(new);
    v["payload"]["scopes"] = json!([
        "session_create",
        "session_resume",
        "turn_start",
        "command_execute",
        "file_change"
    ]);
    v["correlation_id"] = json!(intent);
    v["source"]["instance_id"] = json!(A);
    c.execute(
        "INSERT INTO control_events VALUES(?1,?2,?3,?4,?5)",
        params![
            id,
            v["stream_id"].as_str().unwrap(),
            format!("{n:020}"),
            session,
            v.to_string()
        ],
    )
    .unwrap();
    c.execute(
        "UPDATE authority_metadata SET last_sequence=?1",
        [n.to_string()],
    )
    .unwrap();
    new
}

fn rows(c: &Connection) -> (i64, i64, i64, i64, i64, u64) {
    let n = |t: &str| {
        c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    };
    (
        n("managed_turns"),
        n("managed_turn_launches"),
        n("managed_approvals"),
        n("managed_approval_launches"),
        n("processed_intents"),
        head(c),
    )
}
fn running_turn(w: &mut World) -> ManagedTurnRecord {
    let mut wr = Writer::new(&mut w.conn, wire(A)).unwrap();
    let t = wr.admit_turn(&w.launch, &intent(1), &w.lease).unwrap();
    wr.transition_turn(
        &t.managed_turn_id,
        Some("provider-turn"),
        TurnState::Running,
        None,
    )
    .unwrap()
}

#[test]
fn turn_lifecycle_is_durable_audited_replayable_and_validates() {
    let mut w = world();
    validate_v8(&w.conn).unwrap();
    let i = intent(1);
    let t = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .admit_turn(&w.launch, &i, &w.lease)
        .unwrap();
    assert_eq!((t.state, t.revision), (TurnState::Starting, 1));
    validate_v8(&w.conn).unwrap();
    // The identical intent replays the original record with no side effect.
    let before = rows(&w.conn);
    let again = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .admit_turn(&w.launch, &i, &w.lease)
        .unwrap();
    assert_eq!(again, t);
    assert_eq!(rows(&w.conn), before);
    let mut changed = i.clone();
    changed.prompt_byte_length = 13;
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&w.launch, &changed, &w.lease)
            .err(),
        Some(E::IntentConflict)
    );
    // One active turn per session.
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&w.launch, &intent(1), &w.lease)
            .err(),
        Some(E::ActiveTurnExists)
    );
    let mut wr = Writer::new(&mut w.conn, wire(A)).unwrap();
    let running = wr
        .transition_turn(
            &t.managed_turn_id,
            Some("provider-turn"),
            TurnState::Running,
            None,
        )
        .unwrap();
    assert_eq!(running.revision, 2);
    assert_eq!(
        wr.transition_turn(&t.managed_turn_id, None, TurnState::Running, None)
            .err(),
        Some(E::InvalidTransition)
    );
    let done = wr
        .transition_turn(
            &t.managed_turn_id,
            None,
            TurnState::Completed,
            Some(TurnReason::Completed),
        )
        .unwrap();
    assert_eq!((done.state, done.revision), (TurnState::Completed, 3));
    assert_eq!(
        wr.transition_turn(
            &t.managed_turn_id,
            None,
            TurnState::Failed,
            Some(TurnReason::RuntimeFailed)
        )
        .err(),
        Some(E::InvalidTransition)
    );
    drop(wr);
    validate_v8(&w.conn).unwrap();
}

#[test]
fn only_the_live_owner_can_admit_or_progress_work() {
    let mut w = world();
    // A different authority, a stale generation, a foreign instance, a foreign session lease.
    assert_eq!(
        Writer::new(&mut w.conn, wire(B))
            .unwrap()
            .admit_turn(&w.launch, &intent(1), &w.lease)
            .err(),
        Some(E::StaleRevision)
    );
    let mut stale = w.launch.clone();
    stale.generation += 1;
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&stale, &intent(1), &w.lease)
            .err(),
        Some(E::StaleRevision)
    );
    let mut foreign = w.launch.clone();
    foreign.runtime_instance_id =
        crate::contracts::identities::AgentRuntimeInstanceId::parse(&uuid()).unwrap();
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&foreign, &intent(1), &w.lease)
            .err(),
        Some(E::StaleRevision)
    );
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&w.launch, &intent(2), &w.lease)
            .err(),
        Some(E::StaleRevision)
    );
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&w.launch, &intent(1), &uuid())
            .err(),
        Some(E::LeaseNotFound)
    );
    // An expired lease, evaluated at write time with the injected clock.
    let late = OffsetDateTime::now_utc() + Duration::days(365 * 100);
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .with_clock(move || late)
            .admit_turn(&w.launch, &intent(1), &w.lease)
            .err(),
        Some(E::GateDenied)
    );
    // Progress is the owner's: a successor cannot mark a turn running or completed.
    let t = running_turn(&mut w);
    assert_eq!(
        Writer::new(&mut w.conn, wire(B))
            .unwrap()
            .transition_turn(
                &t.managed_turn_id,
                None,
                TurnState::Completed,
                Some(TurnReason::Completed)
            )
            .err(),
        Some(E::GateDenied)
    );
    validate_v8(&w.conn).unwrap();
}

#[test]
fn every_turn_write_boundary_rolls_back_completely() {
    for failing in 0..3 {
        let mut w = world();
        let before = rows(&w.conn);
        let i = intent(1);
        let err = Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .with_fault(move |n| {
                if n == failing {
                    Err(E::InternalError)
                } else {
                    Ok(())
                }
            })
            .admit_turn(&w.launch, &i, &w.lease)
            .err();
        assert_eq!(err, Some(E::InternalError), "step {failing}");
        assert_eq!(rows(&w.conn), before, "step {failing}");
        validate_v8(&w.conn).unwrap();
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&w.launch, &i, &w.lease)
            .unwrap();
        validate_v8(&w.conn).unwrap();
    }
}

#[test]
fn approval_lifecycle_accept_confirm_decline_and_single_pending() {
    let mut w = world();
    let t = running_turn(&mut w);
    let mut wr = Writer::new(&mut w.conn, wire(A)).unwrap();
    let a = wr
        .open_approval(
            &t.managed_turn_id,
            ApprovalKind::CommandExecution,
            "item-1",
            FP,
            structure(),
        )
        .unwrap();
    assert_eq!(a.state, ApprovalState::Pending);
    // One pending approval per session, one request per provider item.
    assert_eq!(
        wr.open_approval(
            &t.managed_turn_id,
            ApprovalKind::CommandExecution,
            "item-2",
            FP,
            structure()
        )
        .err(),
        Some(E::CapacityExceeded)
    );
    let intent_id = wire(&uuid());
    assert_eq!(
        wr.decide_approval(
            &a.managed_approval_id,
            &intent_id,
            1,
            ApprovalDecision::Accept,
            &"f".repeat(64)
        )
        .err(),
        Some(E::CorrelationError)
    );
    assert_eq!(
        wr.decide_approval(
            &a.managed_approval_id,
            &intent_id,
            2,
            ApprovalDecision::Accept,
            FP
        )
        .err(),
        Some(E::StaleRevision)
    );
    let Decided::Fresh(accepted, receipt) = wr
        .decide_approval(
            &a.managed_approval_id,
            &intent_id,
            1,
            ApprovalDecision::Accept,
            FP,
        )
        .unwrap()
    else {
        panic!("fresh")
    };
    assert_eq!(
        (accepted.state, accepted.effect),
        (ApprovalState::Accepted, ApprovalEffect::Dispatched)
    );
    // The same intent replays its receipt and never entitles a second provider answer.
    assert!(
        matches!(wr.decide_approval(&a.managed_approval_id, &intent_id, 1, ApprovalDecision::Accept, FP).unwrap(), Decided::Replayed(r) if r == receipt)
    );
    assert_eq!(
        wr.decide_approval(
            &a.managed_approval_id,
            &intent_id,
            1,
            ApprovalDecision::Decline,
            FP
        )
        .err(),
        Some(E::IntentConflict)
    );
    assert_eq!(
        wr.confirm_approval(&a.managed_approval_id).err(),
        Some(E::GateDenied)
    );
    wr.record_delivery(&a.managed_approval_id, ApprovalDelivery::KernelAccepted)
        .unwrap();
    let confirmed = wr.confirm_approval(&a.managed_approval_id).unwrap();
    assert_eq!(confirmed.effect, ApprovalEffect::Confirmed);
    assert_eq!(
        wr.confirm_approval(&a.managed_approval_id)
            .unwrap()
            .revision,
        confirmed.revision
    );
    drop(wr);
    validate_v8(&w.conn).unwrap();
    // A decline needs no dispatch; ending the turn settles nothing else pending.
    let mut wr = Writer::new(&mut w.conn, wire(A)).unwrap();
    let b = wr
        .open_approval(
            &t.managed_turn_id,
            ApprovalKind::CommandExecution,
            "item-2",
            FP,
            structure(),
        )
        .unwrap();
    let Decided::Fresh(declined, _) = wr
        .decide_approval(
            &b.managed_approval_id,
            &wire(&uuid()),
            1,
            ApprovalDecision::Decline,
            FP,
        )
        .unwrap()
    else {
        panic!("fresh")
    };
    assert_eq!(
        (declined.state, declined.effect),
        (ApprovalState::Declined, ApprovalEffect::None)
    );
    wr.transition_turn(
        &t.managed_turn_id,
        None,
        TurnState::Completed,
        Some(TurnReason::Completed),
    )
    .unwrap();
    drop(wr);
    validate_v8(&w.conn).unwrap();
}

#[test]
fn ending_a_turn_settles_its_open_approval_and_non_owners_cannot_decide() {
    let mut w = world();
    let t = running_turn(&mut w);
    let a = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .open_approval(
            &t.managed_turn_id,
            ApprovalKind::CommandExecution,
            "item-1",
            FP,
            structure(),
        )
        .unwrap();
    assert_eq!(
        Writer::new(&mut w.conn, wire(B))
            .unwrap()
            .decide_approval(
                &a.managed_approval_id,
                &wire(&uuid()),
                1,
                ApprovalDecision::Accept,
                FP
            )
            .err(),
        Some(E::GateDenied)
    );
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .transition_turn(
            &t.managed_turn_id,
            None,
            TurnState::Failed,
            Some(TurnReason::ProviderFailed),
        )
        .unwrap();
    let state: String = w
        .conn
        .query_row("SELECT state FROM managed_approvals", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "invalidated");
    validate_v8(&w.conn).unwrap();
}

#[test]
fn f8_16_approval_outcome_fails_closed_when_audit_storage_fails() {
    let mut w = world();
    let t = running_turn(&mut w);
    let a = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .open_approval(
            &t.managed_turn_id,
            ApprovalKind::CommandExecution,
            "item-1",
            FP,
            structure(),
        )
        .unwrap();
    let before = rows(&w.conn);
    w.conn.execute_batch("CREATE TEMP TRIGGER f8_storage_full BEFORE INSERT ON main.control_events BEGIN SELECT RAISE(ABORT,'database or disk is full'); END").unwrap();
    // The decision cannot be recorded, so the caller is never entitled to answer the provider.
    let intent_id = wire(&uuid());
    assert!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .decide_approval(
                &a.managed_approval_id,
                &intent_id,
                1,
                ApprovalDecision::Accept,
                FP
            )
            .is_err()
    );
    assert_eq!(rows(&w.conn), before);
    w.conn
        .execute_batch("DROP TRIGGER temp.f8_storage_full")
        .unwrap();
    let Decided::Fresh(..) = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .decide_approval(
            &a.managed_approval_id,
            &intent_id,
            1,
            ApprovalDecision::Accept,
            FP,
        )
        .unwrap()
    else {
        panic!("fresh after storage returns")
    };
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .record_delivery(&a.managed_approval_id, ApprovalDelivery::KernelAccepted)
        .unwrap();
    // The answer went out but its confirmation cannot be recorded: the unknown effect is kept.
    w.conn.execute_batch("CREATE TEMP TRIGGER f8_storage_full BEFORE INSERT ON main.control_events BEGIN SELECT RAISE(ABORT,'database or disk is full'); END").unwrap();
    assert!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .confirm_approval(&a.managed_approval_id)
            .is_err()
    );
    w.conn
        .execute_batch("DROP TRIGGER temp.f8_storage_full")
        .unwrap();
    let (state, effect): (String, String) = w
        .conn
        .query_row("SELECT state,effect FROM managed_approvals", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(
        (state.as_str(), effect.as_str()),
        ("accepted", "dispatched")
    );
    validate_v8(&w.conn).unwrap();
}

#[test]
fn crash_recovery_marks_stranded_work_lost_without_replay_or_inheritance() {
    let mut w = world();
    let t = running_turn(&mut w);
    let (pending, accepted) = {
        let mut wr = Writer::new(&mut w.conn, wire(A)).unwrap();
        let first = wr
            .open_approval(
                &t.managed_turn_id,
                ApprovalKind::CommandExecution,
                "item-1",
                FP,
                structure(),
            )
            .unwrap();
        let Decided::Fresh(first, _) = wr
            .decide_approval(
                &first.managed_approval_id,
                &wire(&uuid()),
                1,
                ApprovalDecision::Accept,
                FP,
            )
            .unwrap()
        else {
            panic!("fresh")
        };
        let second = wr
            .open_approval(
                &t.managed_turn_id,
                ApprovalKind::CommandExecution,
                "item-2",
                FP,
                structure(),
            )
            .unwrap();
        (second, first)
    };
    // Authority A crashes: nothing is written for it. B starts, announces itself and recovers.
    let (approvals, turns) = Writer::new(&mut w.conn, wire(B))
        .unwrap()
        .recover()
        .unwrap();
    assert_eq!((approvals, turns), (2, 1));
    let states: Vec<(String, String)> = w
        .conn
        .prepare("SELECT state,effect FROM managed_approvals ORDER BY created_sequence")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        states,
        vec![
            ("indeterminate".into(), "unknown".into()),
            ("invalidated".into(), "none".into())
        ]
    );
    let turn_state: String = w
        .conn
        .query_row(
            "SELECT state FROM managed_turns WHERE managed_turn_id=?1",
            [t.managed_turn_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(turn_state, "indeterminate");
    validate_v8(&w.conn).unwrap();
    // Nothing is left to recover, nothing is revived, and A's pending decision can never be made.
    assert_eq!(
        Writer::new(&mut w.conn, wire(B))
            .unwrap()
            .recover()
            .unwrap(),
        (0, 0)
    );
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .decide_approval(
                &pending.managed_approval_id,
                &wire(&uuid()),
                1,
                ApprovalDecision::Accept,
                FP
            )
            .err(),
        Some(E::InvalidTransition)
    );
    let _ = accepted;
}

fn revoke_op(lease: &str, rev: u64) -> Operation {
    Operation::LeaseRevoke(crate::contracts::execution::LeaseRevoke {
        intent_id: wire(&uuid()),
        lease_id: crate::contracts::identities::ExecutionLeaseId::parse(lease).unwrap(),
        expected_revision: rev,
    })
}
fn grant_op(session: &str, scopes: Vec<crate::contracts::execution::ExecutionScope>) -> Operation {
    Operation::LeaseGrant(crate::contracts::execution::LeaseGrant {
        intent_id: wire(&uuid()),
        session_id: crate::contracts::identities::ManagedSessionId::parse(session).unwrap(),
        expected_session_revision: 1,
        scopes,
        ttl_seconds: 600,
    })
}

#[test]
fn durable_leases_grant_renew_revoke_and_replay() {
    use crate::contracts::execution::ExecutionScope as S;
    let mut w = world();
    // The session already has an active lease (the fixture's).
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .lease(&grant_op(&w.session, vec![S::TurnStart]))
            .err(),
        Some(E::LeaseAlreadyActive)
    );
    let revoke = revoke_op(&w.lease, 1);
    let revoked = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .lease(&revoke)
        .unwrap();
    assert_eq!((revoked.state, revoked.revision), (LeaseState::Revoked, 2));
    // Exact replay returns the original; any change under the same intent conflicts.
    let before = rows(&w.conn);
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .lease(&revoke)
            .unwrap(),
        revoked
    );
    assert_eq!(rows(&w.conn), before);
    validate_v8(&w.conn).unwrap();
    let grant = grant_op(&w.session, vec![S::TurnStart, S::SessionCreate]);
    let lease = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .lease(&grant)
        .unwrap();
    assert_eq!(lease.scopes, vec![S::SessionCreate, S::TurnStart]);
    let Operation::LeaseGrant(g) = &grant else {
        unreachable!()
    };
    let mut changed = g.clone();
    changed.ttl_seconds = 601;
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .lease(&Operation::LeaseGrant(changed))
            .err(),
        Some(E::IntentConflict)
    );
    let renew = |rev, ttl| {
        Operation::LeaseRenew(crate::contracts::execution::LeaseRenew {
            intent_id: wire(&uuid()),
            lease_id: lease.lease_id.clone(),
            expected_revision: rev,
            ttl_seconds: ttl,
        })
    };
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .lease(&renew(2, 60))
            .err(),
        Some(E::StaleRevision)
    );
    let renewed = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .lease(&renew(1, 60))
        .unwrap();
    assert_eq!(renewed.revision, 2);
    validate_v8(&w.conn).unwrap();
}

#[test]
fn losing_a_lease_ends_its_turns_and_approvals_in_the_same_transaction() {
    let mut w = world();
    let t = running_turn(&mut w);
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .open_approval(
            &t.managed_turn_id,
            ApprovalKind::CommandExecution,
            "item-1",
            FP,
            structure(),
        )
        .unwrap();
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .lease(&revoke_op(&w.lease, 1))
        .unwrap();
    let (turn, reason): (String, String) = w
        .conn
        .query_row(
            "SELECT state,terminal_reason FROM managed_turns WHERE managed_turn_id=?1",
            [t.managed_turn_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (turn.as_str(), reason.as_str()),
        ("interrupted", "lease_revoked")
    );
    let (approval, reason): (String, String) = w
        .conn
        .query_row(
            "SELECT state,terminal_reason FROM managed_approvals",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (approval.as_str(), reason.as_str()),
        ("invalidated", "lease_revoked")
    );
    validate_v8(&w.conn).unwrap();
    // The lost lease can never admit anything again.
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&w.launch, &intent(1), &w.lease)
            .err(),
        Some(E::GateDenied)
    );
}

#[test]
fn an_expired_lease_is_settled_by_the_clock_not_by_hope() {
    let mut w = world();
    let t = running_turn(&mut w);
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .open_approval(
            &t.managed_turn_id,
            ApprovalKind::CommandExecution,
            "item-1",
            FP,
            structure(),
        )
        .unwrap();
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .expire_leases()
            .unwrap(),
        0
    );
    let later = OffsetDateTime::now_utc() + Duration::hours(3);
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .with_clock(move || later)
            .expire_leases()
            .unwrap(),
        1
    );
    let (turn, reason): (String, String) = w
        .conn
        .query_row(
            "SELECT state,terminal_reason FROM managed_turns WHERE managed_turn_id=?1",
            [t.managed_turn_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (turn.as_str(), reason.as_str()),
        ("interrupted", "lease_expired")
    );
    let approval: String = w
        .conn
        .query_row("SELECT state FROM managed_approvals", [], |r| r.get(0))
        .unwrap();
    assert_eq!(approval, "expired");
    validate_v8(&w.conn).unwrap();
}

#[test]
fn replayed_turn_intent_cannot_be_retargeted_to_a_new_launch() {
    let mut w = world();
    let original_intent = intent(1);
    let original = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .admit_turn(&w.launch, &original_intent, &w.lease)
        .unwrap();
    Allocator::new(&mut w.conn, wire(A))
        .unwrap()
        .record_exit(&w.launch, "child_exited", false, None)
        .unwrap();
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .recover()
        .unwrap();
    let request = Request::parse(
        &serde_json::to_vec(&json!({
        "schema_version":2,"request_id":uuid(),"authority_instance_id":A,
        "method":"runtime/start","params":{"intent_id":uuid(),"adapter_id":"codex",
        "runtime_kind":"codex","managed_session_id":w.session,
        "expected_generation":1,"expected_revision":1}}))
        .unwrap(),
    )
    .unwrap();
    let out = Allocator::new(&mut w.conn, wire(A))
        .unwrap()
        .start(
            &request,
            &Admission {
                live: None,
                slot_available: true,
            },
        )
        .unwrap();
    let replacement = RuntimeRef {
        schema: 2,
        authority_instance_id: wire(A),
        managed_session_id: out.managed_session_id,
        runtime_instance_id: out.runtime_instance_id.unwrap(),
        generation: out.generation,
    };
    Allocator::new(&mut w.conn, wire(A))
        .unwrap()
        .record_ready(&replacement, "0.159.0", "compatible")
        .unwrap();
    let before = rows(&w.conn);
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&replacement, &original_intent, &w.lease)
            .err(),
        Some(E::IntentConflict)
    );
    assert_eq!(rows(&w.conn), before);
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .admit_turn(&w.launch, &original_intent, &w.lease)
            .unwrap(),
        original
    );
    validate_v8(&w.conn).unwrap();
}

#[test]
fn delivery_evidence_separates_decision_kernel_provider_and_external_effect() {
    let mut w = world();
    let turn = running_turn(&mut w);
    let approval = Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .open_approval(
            &turn.managed_turn_id,
            ApprovalKind::CommandExecution,
            "delivery-item",
            FP,
            structure(),
        )
        .unwrap();
    let id = wire(&uuid());
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .decide_approval(
            &approval.managed_approval_id,
            &id,
            1,
            ApprovalDecision::Accept,
            FP,
        )
        .unwrap();
    assert_eq!(
        read_delivery(&w.conn, approval.managed_approval_id.as_str()).unwrap(),
        Some(ApprovalDelivery::DecisionAccepted)
    );
    assert_eq!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .confirm_approval(&approval.managed_approval_id)
            .err(),
        Some(E::GateDenied)
    );
    validate_v8(&w.conn).unwrap();
    let before = rows(&w.conn);
    w.conn.execute_batch("CREATE TEMP TRIGGER reject_delivery_audit BEFORE INSERT ON main.control_events WHEN json_extract(NEW.envelope,'$.event_type')='authority.approval_delivery.changed' BEGIN SELECT RAISE(ABORT,'delivery audit full'); END;").unwrap();
    assert!(
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .record_delivery(
                &approval.managed_approval_id,
                ApprovalDelivery::KernelAccepted
            )
            .is_err()
    );
    w.conn
        .execute_batch("DROP TRIGGER temp.reject_delivery_audit")
        .unwrap();
    assert_eq!(
        rows(&w.conn),
        before,
        "audit refusal rolls back delivery evidence"
    );
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .record_delivery(
            &approval.managed_approval_id,
            ApprovalDelivery::KernelAccepted,
        )
        .unwrap();
    assert_eq!(
        read_delivery(&w.conn, approval.managed_approval_id.as_str()).unwrap(),
        Some(ApprovalDelivery::KernelAccepted)
    );
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .record_delivery(
            &approval.managed_approval_id,
            ApprovalDelivery::ProviderConsumed,
        )
        .unwrap();
    assert_eq!(
        read_delivery(&w.conn, approval.managed_approval_id.as_str()).unwrap(),
        Some(ApprovalDelivery::ProviderConsumed)
    );
    assert_eq!(
        read_approval(
            &w.conn.unchecked_transaction().unwrap(),
            approval.managed_approval_id.as_str()
        )
        .unwrap()
        .effect,
        ApprovalEffect::Dispatched,
        "provider consumption alone cannot confirm execution"
    );
    Writer::new(&mut w.conn, wire(A))
        .unwrap()
        .confirm_approval(&approval.managed_approval_id)
        .unwrap();
    assert_eq!(
        read_delivery(&w.conn, approval.managed_approval_id.as_str()).unwrap(),
        Some(ApprovalDelivery::EffectConfirmed)
    );
    validate_v8(&w.conn).unwrap();
}

#[test]
fn decline_or_cancel_delivery_loss_is_recovered_and_never_redispatched() {
    for decision in [ApprovalDecision::Decline, ApprovalDecision::Cancel] {
        let mut w = world();
        let turn = running_turn(&mut w);
        let approval = Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .open_approval(
                &turn.managed_turn_id,
                ApprovalKind::CommandExecution,
                "delivery-item",
                FP,
                structure(),
            )
            .unwrap();
        let id = wire(&uuid());
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .decide_approval(&approval.managed_approval_id, &id, 1, decision, FP)
            .unwrap();
        Allocator::new(&mut w.conn, wire(A))
            .unwrap()
            .record_exit(&w.launch, "child_exited", false, None)
            .unwrap();
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .recover()
            .unwrap();
        assert_eq!(
            read_delivery(&w.conn, approval.managed_approval_id.as_str()).unwrap(),
            Some(ApprovalDelivery::Indeterminate)
        );
        let before = rows(&w.conn);
        assert!(matches!(
            Writer::new(&mut w.conn, wire(A))
                .unwrap()
                .decide_approval(&approval.managed_approval_id, &id, 1, decision, FP)
                .unwrap(),
            Decided::Replayed(_)
        ));
        Writer::new(&mut w.conn, wire(A))
            .unwrap()
            .recover()
            .unwrap();
        assert_eq!(rows(&w.conn), before);
        validate_v8(&w.conn).unwrap();
    }
}
