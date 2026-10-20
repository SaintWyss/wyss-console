use super::*;
use crate::contracts::{
    agent_adapter::{AgentAdapterId, AgentRuntimeKind},
    control::{Operation, Prepare},
};
fn populated(version: i64) -> Connection {
    let mut store = Store::in_memory_fixture();
    store
        .apply(&Operation::Prepare(Prepare {
            intent_id: crate::control::store::uuid(),
            adapter_id: AgentAdapterId::parse("codex").unwrap(),
            runtime_kind: AgentRuntimeKind::Codex,
        }))
        .unwrap();
    if version >= 2 {
        store
            .apply(&Operation::RuntimeStart(
                crate::contracts::control::RuntimeMutation {
                    intent_id: crate::control::store::uuid(),
                    adapter_id: AgentAdapterId::parse("codex").unwrap(),
                    runtime_kind: AgentRuntimeKind::Codex,
                    expected_revision: Some(0),
                    safety_profile: None,
                    workspace_root: None,
                },
            ))
            .unwrap();
    }
    if version >= 3 {
        let session: String = store
            .conn
            .query_row("SELECT session_id FROM managed_sessions", [], |r| r.get(0))
            .unwrap();
        let result = store
            .apply(&Operation::LeaseGrant(
                crate::contracts::control::LeaseGrant {
                    intent_id: crate::control::store::uuid(),
                    session_id: crate::contracts::identities::ManagedSessionId::parse(&session)
                        .unwrap(),
                    expected_session_revision: 1,
                    scopes: vec![crate::contracts::control::ExecutionScope::TurnStart],
                    ttl_seconds: 60,
                },
            ))
            .unwrap();
        let crate::contracts::control::ResultValue::Lease(lease) = result else {
            panic!("lease fixture");
        };
        store
            .apply(&Operation::LeaseRevoke(
                crate::contracts::control::LeaseRevoke {
                    intent_id: crate::control::store::uuid(),
                    lease_id: lease.lease_id,
                    expected_revision: lease.revision,
                },
            ))
            .unwrap();
    }
    let source = store.conn;
    if version == 7 {
        return source;
    }
    let c = Connection::open_in_memory().unwrap();
    let objects = match version {
        1 => schema::V1_OBJECTS,
        2 => schema::V2_OBJECTS,
        3 => schema::V3_OBJECTS,
        4 => schema::V4_OBJECTS,
        5 => schema::V5_OBJECTS,
        6 => schema::V6_OBJECTS,
        _ => panic!("fixture version"),
    };
    for (_, sql) in objects
        .iter()
        .filter(|(_, s)| s.starts_with("CREATE TABLE"))
    {
        c.execute_batch(sql).unwrap();
    }
    let data = rows(&source).unwrap();
    for (name, sql) in objects {
        if !sql.starts_with("CREATE TABLE") {
            continue;
        }
        let mut data = data[*name].clone();
        if *name == "authority_metadata" {
            data[0][1] = Value::Integer(version);
            data[0].pop();
        }
        insert_rows(&c, name, &data).unwrap();
    }
    for (_, sql) in objects
        .iter()
        .filter(|(_, s)| !s.starts_with("CREATE TABLE"))
    {
        c.execute_batch(sql).unwrap();
    }
    c.pragma_update(None, "user_version", version).unwrap();
    c
}
fn snapshot(c: &Connection) -> String {
    format!(
        "{:?}:{:?}:{:?}",
        objects(c).unwrap(),
        rows(c).unwrap(),
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap()
    )
}
#[test]
fn every_supported_schema_migrates_canonical_records_without_writing_source() {
    for version in 0..=7 {
        let mut c = if version == 0 {
            Connection::open_in_memory().unwrap()
        } else {
            populated(version)
        };
        let before = snapshot(&c);
        let mut step = None;
        let result = prepare_with(&mut c, |i| {
            step = Some(i);
            Ok(())
        })
        .unwrap_or_else(|e| panic!("version {version} step {step:?}: {e:?}"));
        result.validate().unwrap();
        assert_eq!(snapshot(&c), before, "source {version}");
        if version != 0 {
            let head: String = c
                .query_row("SELECT last_sequence FROM authority_metadata", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(result.metadata().unwrap().1, head);
        }
        assert_eq!(
            result
                .connection
                .query_row::<i64, _, _>("SELECT count(*) FROM managed_runtimes", [], |r| r.get(0))
                .unwrap(),
            0
        );
    }
}
#[test]
fn every_failure_boundary_preserves_every_supported_source() {
    for version in 0..=7 {
        for fail in 0..=statements().len() {
            let mut c = if version == 0 {
                Connection::open_in_memory().unwrap()
            } else {
                populated(version)
            };
            let before = snapshot(&c);
            let mut reached = None;
            assert!(
                prepare_with(&mut c, |i| {
                    reached = Some(i);
                    if i == fail {
                        Err(E::StorageUnavailable)
                    } else {
                        Ok(())
                    }
                })
                .is_err()
            );
            assert_eq!(
                reached,
                Some(fail),
                "fault hook must run v{version} step{fail}"
            );
            assert_eq!(snapshot(&c), before, "v{version} step{fail}");
            assert!(c.is_autocommit());
        }
    }
}
#[test]
fn corrupt_records_extra_objects_partial_migrations_and_future_schemas_refuse() {
    let mut c = populated(7);
    c.execute_batch(
        "DROP TRIGGER control_events_no_update; UPDATE control_events SET envelope='{}';",
    )
    .unwrap();
    let before = snapshot(&c);
    assert!(prepare(&mut c).is_err());
    assert_eq!(snapshot(&c), before);
    let mut c = populated(7);
    c.execute_batch("UPDATE processed_intents SET result='{}'")
        .unwrap();
    assert!(prepare(&mut c).is_err());
    let mut c = populated(7);
    c.execute_batch("CREATE TABLE extra(x TEXT)").unwrap();
    assert!(prepare(&mut c).is_err());
    let mut c = populated(7);
    c.execute_batch("ALTER TABLE managed_runtimes RENAME TO managed_runtimes_legacy_v7")
        .unwrap();
    assert!(prepare(&mut c).is_err());
    let mut c = populated(7);
    c.pragma_update(None, "user_version", 9).unwrap();
    assert!(prepare(&mut c).is_err());
    let mut c = populated(7);
    let result = prepare(&mut c).unwrap();
    result
        .connection
        .execute_batch("DROP TRIGGER managed_runtime_launches_current")
        .unwrap();
    assert!(result.validate().is_err());
}
#[test]
fn quiescence_and_unknown_v8_work_fail_closed() {
    let mut c = populated(7);
    let id: String = c
        .query_row("SELECT session_id FROM managed_sessions", [], |r| r.get(0))
        .unwrap();
    c.execute(
        "INSERT INTO execution_leases VALUES('bad',?1,'active','[]',1,'t','z','t',NULL)",
        [&id],
    )
    .unwrap();
    assert!(prepare(&mut c).is_err());
    let mut c = populated(7);
    let result = prepare(&mut c).unwrap();
    let id: String = result
        .connection
        .query_row("SELECT session_id FROM managed_sessions", [], |r| r.get(0))
        .unwrap();
    result
        .connection
        .execute(
            "INSERT INTO managed_runtimes VALUES(?1,'codex','codex','running',1,1,'t','t')",
            [id],
        )
        .unwrap();
    assert!(result.validate().is_err());
}
#[test]
fn canonical_schema_with_unlogged_session_and_invalid_receipt_refuses() {
    let mut c = populated(7);
    c.execute_batch("INSERT INTO managed_sessions VALUES('11111111-1111-4111-8111-111111111111','codex','codex','prepared',1,'2026-10-20T00:00:00Z','2026-10-20T00:00:00Z','2')").unwrap();
    assert!(prepare(&mut c).is_err());
    let mut c = populated(7);
    c.execute("UPDATE processed_intents SET mutation='{}'", [])
        .unwrap();
    assert!(prepare(&mut c).is_err());
}
#[test]
fn temp_shadow_and_attached_databases_refuse_without_source_changes() {
    let mut c = populated(7);
    let before = snapshot(&c);
    c.execute_batch("CREATE TEMP TABLE control_events(x TEXT)")
        .unwrap();
    assert!(prepare(&mut c).is_err());
    c.execute_batch("DROP TABLE temp.control_events").unwrap();
    assert_eq!(snapshot(&c), before);
    c.execute_batch("ATTACH DATABASE ':memory:' AS other")
        .unwrap();
    assert!(prepare(&mut c).is_err());
    assert_eq!(snapshot(&c), before);
}
#[test]
fn canonical_active_lease_reaches_quiescence_guard_and_preserves_source() {
    use crate::contracts::{
        control::{ExecutionScope, LeaseGrant, ResultValue},
        identities::ManagedSessionId,
    };
    let mut store = Store::in_memory_fixture();
    let prepared = store
        .apply(&Operation::Prepare(Prepare {
            intent_id: crate::control::store::uuid(),
            adapter_id: AgentAdapterId::parse("codex").unwrap(),
            runtime_kind: AgentRuntimeKind::Codex,
        }))
        .unwrap();
    let ResultValue::Session(session) = prepared else {
        panic!("session fixture");
    };
    store
        .apply(&Operation::LeaseGrant(LeaseGrant {
            intent_id: crate::control::store::uuid(),
            session_id: ManagedSessionId::parse(session.managed_session_id.as_str()).unwrap(),
            expected_session_revision: 1,
            scopes: vec![ExecutionScope::TurnStart],
            ttl_seconds: 60,
        }))
        .unwrap();
    let mut c = store.conn;
    let before = snapshot(&c);
    let mut reached = None;
    assert!(
        prepare_with(&mut c, |i| {
            reached = Some(i);
            Ok(())
        })
        .is_err()
    );
    assert_eq!(
        reached,
        Some(0),
        "canonical verification passes; quiescent SQL refuses"
    );
    assert_eq!(snapshot(&c), before);
}
