//! The V8→V9 migration of ADR0055, rehearsed on a synthetic store.
//!
//! The V8 image is written by the authority's own (Codex-shaped) writers, driven through the same
//! protocol conversations the parity tests use, so it holds real work in every table the migration
//! touches: sessions, a runtime and its launch, a binding, completed and declined turns, accepted
//! and declined approvals with their delivery records, a lease, receipts and the event log. It is
//! an in-memory database made for each test. No installed store, path or authority is read, copied
//! or modified, and `v9_candidate` is compiled into test builds only.
use super::*;
use crate::{
    control::{schema, store::Store},
    f8::{
        migration::{boundary_digest, clone_database, rows},
        v8::validate_v8,
        v9_candidate::{Stage, expected_v9, migrate, quiescent, v8_view, validate_v9},
    },
};
use rusqlite::{Connection, types::Value as Sql};

fn finish_turn(rig: &mut Rig, l: &Live, turn: &str) {
    let mut result = fixture_event("turn_result");
    result["turn"] = json!(turn);
    l.bridge.push(result);
    l.bridge.push(json!({"v":1,"e":"turn_idle","turn":turn}));
    let t = turn.to_owned();
    tick_until(rig, "turn completed", |rig| {
        turn_state(rig, &t) == "completed"
    });
}
fn pending_approval(rig: &mut Rig, turn: &str) -> String {
    let pending = rig.call(
        "managedTurn/approval/pending",
        json!({"managed_turn_id":turn}),
    );
    pending["result"]["value"]["approval"]["managed_approval_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn decide(rig: &mut Rig, l: &Live, approval: &str, decision: &str) {
    let r = rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":approval,"expected_revision":1,
            "decision":decision,"runtime_fence":l.fence}),
    );
    assert_eq!(r["result"]["kind"], "approval_receipt", "{r}");
}

/// A V8 store with real work. `quiet`: everything is finished, the lease revoked and the runtime
/// stopped (what an offline migration requires); otherwise a turn is left waiting for a decision.
fn synthetic_v8(quiet: bool) -> Connection {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let l = start(&mut rig, &workers, 1);
    let lease = grant(&mut rig, &l);
    bind_new(&mut rig, &l);
    // A streamed turn.
    let t1 = begin_turn(&mut rig, &l, "explain the repository");
    attest(&l, &t1);
    l.bridge
        .push(json!({"v":1,"e":"text","turn":t1,"text":"a desktop console"}));
    finish_turn(&mut rig, &l, &t1);
    // A command, accepted, run, and confirmed.
    let t2 = begin_turn(&mut rig, &l, "run it");
    attest(&l, &t2);
    proposal(&l, &t2, "act-1", "echo hi");
    tick_until(&mut rig, "pending", |rig| {
        rig.authority.engine().pending_approvals(&l.session) == 1
    });
    let a = pending_approval(&mut rig, &t2);
    decide(&mut rig, &l, &a, "accept");
    tick_until(&mut rig, "consumed", |rig| {
        rig.authority
            .engine()
            .connection()
            .query_row::<String, _, _>(
                "SELECT state FROM managed_approval_delivery WHERE managed_approval_id=?1",
                [&a],
                |r| r.get(0),
            )
            .unwrap()
            == "provider_consumed"
    });
    l.exec.0.lock().unwrap().outcome = Some(Outcome::Completed("exit status 0\nhi\n".into()));
    tick_until(&mut rig, "confirmed", |rig| {
        rig.authority
            .engine()
            .connection()
            .query_row::<String, _, _>(
                "SELECT state FROM managed_approval_delivery WHERE managed_approval_id=?1",
                [&a],
                |r| r.get(0),
            )
            .unwrap()
            == "effect_confirmed"
    });
    finish_turn(&mut rig, &l, &t2);
    // A command, declined.
    let t3 = begin_turn(&mut rig, &l, "delete it");
    attest(&l, &t3);
    proposal(&l, &t3, "act-2", "rm -rf build");
    tick_until(&mut rig, "pending", |rig| {
        rig.authority.engine().pending_approvals(&l.session) == 1
    });
    if quiet {
        let a = pending_approval(&mut rig, &t3);
        decide(&mut rig, &l, &a, "decline");
        finish_turn(&mut rig, &l, &t3);
        let r = rig.call(
            "executionLease/revoke",
            json!({"intent_id":uuid(),"lease_id":lease,"expected_revision":1}),
        );
        assert_eq!(r["result"]["value"]["state"], "revoked", "{r}");
        let s = l.session.clone();
        let (g, rev) = rig.expect(&s);
        let stop = rig.call(
            "runtime/stop",
            json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
                "managed_session_id":s,"expected_generation":g,"expected_revision":rev,
                "expected_runtime_instance_id":l.reference.runtime_instance_id}),
        );
        assert_eq!(stop["result"]["kind"], "runtime_control", "{stop}");
        tick_until(&mut rig, "runtime stopped", |rig| {
            rig.authority.engine().reference(&s).is_none()
        });
    }
    let conn = rig.authority.engine().connection();
    validate_v8(conn).expect("the synthetic image is a valid V8 store");
    clone_database(conn).unwrap()
}

fn count(c: &Connection, table: &str) -> i64 {
    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn the_synthetic_v8_store_has_real_work_in_every_table_the_migration_rebuilds() {
    let v8 = synthetic_v8(true);
    for (table, at_least) in [
        ("managed_runtimes", 1),
        ("managed_runtime_launches", 1),
        ("managed_session_bindings", 1),
        ("managed_turns", 3),
        ("managed_approvals", 2),
        ("managed_approval_delivery", 1),
        ("managed_turn_launches", 3),
        ("managed_approval_launches", 2),
        ("managed_operation_launches", 1),
        ("control_events", 20),
        ("migration_boundary", 1),
    ] {
        assert!(
            count(&v8, table) >= at_least,
            "{table}: {}",
            count(&v8, table)
        );
    }
    assert!(quiescent(&v8).unwrap());
    // A store with a decision still owed is the other kind, and the migration refuses it.
    let busy = synthetic_v8(false);
    assert!(!quiescent(&busy).unwrap());
}

#[test]
fn migrating_preserves_every_row_the_event_log_byte_for_byte_and_the_v8_verifiers_verdict() {
    let v8 = synthetic_v8(true);
    let before = rows(&v8).unwrap();
    let summary_before = validate_v8(&v8).unwrap();
    let v9 = migrate(&v8, |_| Ok(())).unwrap();

    // The source is only read.
    assert_eq!(rows(&v8).unwrap(), before);
    assert_eq!(
        v8.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        8
    );

    // Version, schema and boundary moved; nothing else of the metadata did.
    assert_eq!(
        v9.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        9
    );
    let after = rows(&v9).unwrap();
    let (m8, m9) = (
        &before["authority_metadata"][0],
        &after["authority_metadata"][0],
    );
    assert_eq!((&m8[1], &m9[1]), (&Sql::Integer(8), &Sql::Integer(9)));
    assert_eq!((&m8[0], &m8[2..]), (&m9[0], &m9[2..]));

    // Every other table: equal, row for row, in order. That is the sessions, runtimes and their
    // generations, launches, bindings, turns, approvals, delivery records, receipts, leases, and
    // the frozen V7 boundary and the legacy runtime.
    for (table, data) in &before {
        if table != "authority_metadata" {
            assert_eq!(after.get(table), Some(data), "{table}");
        }
    }
    assert_eq!(
        after
            .keys()
            .filter(|k| !before.contains_key(*k))
            .collect::<Vec<_>>(),
        ["migration_boundary_v9"]
    );
    // The event history is the same bytes, so its hash chain is the same.
    let head: u64 = count(&v8, "control_events").try_into().unwrap();
    assert_eq!(
        boundary_digest(&v8, head).unwrap(),
        boundary_digest(&v9, head).unwrap()
    );
    // And the V8 verifier, looking at the V9 image through its V8 view, reaches the same verdict.
    assert_eq!(validate_v9(&v9).unwrap(), summary_before);
    let view = v8_view(&v9).unwrap();
    assert_eq!(
        rows(&view).unwrap(),
        before,
        "the rollback view is the original, row for row"
    );
}

#[test]
fn the_v9_schema_is_exactly_the_v8_one_with_the_three_checks_widened_and_one_new_boundary() {
    let v8 = synthetic_v8(true);
    let v9 = migrate(&v8, |_| Ok(())).unwrap();
    let old = crate::f8::migration::objects(&v8).unwrap();
    let new = crate::f8::migration::objects(&v9).unwrap();
    assert_eq!(new, expected_v9().unwrap());
    let changed: Vec<_> = old
        .iter()
        .filter(|(n, sql)| new.get(*n) != Some(*sql))
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(
        changed,
        [
            "authority_metadata",
            "managed_runtime_launches",
            "managed_runtimes",
            "managed_session_bindings"
        ]
    );
    // Every trigger and index of the rebuilt tables survived, character for character.
    let kept = old
        .iter()
        .filter(|(_, sql)| !sql.starts_with("CREATE TABLE"))
        .all(|(n, sql)| new.get(n) == Some(sql));
    assert!(kept);
    assert!(
        new.keys()
            .filter(|k| !old.contains_key(*k))
            .all(|k| k.starts_with("migration_boundary_v9"))
    );
    assert_eq!(new.len(), old.len() + 3);
}

#[test]
fn a_v9_image_holds_a_claude_runtime_that_a_v8_image_refuses_and_still_refuses_anyone_else() {
    let claude = |c: &Connection| -> Result<usize, rusqlite::Error> {
        c.execute(
            "INSERT INTO managed_sessions SELECT 'ffffffff-0000-4000-8000-000000000001','claude','claude',\
             control_state,revision,created_at,updated_at,'99999999999999999991' \
             FROM managed_sessions LIMIT 1",
            [],
        )?;
        c.execute(
            "INSERT INTO managed_runtimes(managed_session_id,adapter_id,runtime_kind,desired_state,revision,generation,created_at,updated_at) \
             VALUES('ffffffff-0000-4000-8000-000000000001','claude','claude','stopped',1,0,'t','t')",
            [],
        )
    };
    let v8 = synthetic_v8(true);
    // The control runs on a private copy: it leaves a session row behind that no event explains.
    let scratch = clone_database(&v8).unwrap();
    let refused = claude(&scratch).unwrap_err().to_string();
    assert!(refused.contains("CHECK constraint failed"), "{refused}");
    let v9 = migrate(&v8, |_| Ok(())).unwrap();
    assert_eq!(claude(&v9).unwrap(), 1);
    // The pairing CHECK and the closed set still hold.
    for (adapter, kind) in [
        ("openai", "openai"),
        ("codex", "claude"),
        ("claude", "codex"),
    ] {
        let bad = v9.execute(
            "INSERT INTO managed_runtimes(managed_session_id,adapter_id,runtime_kind,desired_state,revision,generation,created_at,updated_at) \
             VALUES('ffffffff-0000-4000-8000-000000000001',?1,?2,'stopped',1,0,'t','t')",
            [adapter, kind],
        );
        assert!(bad.is_err(), "{adapter}/{kind}");
    }
    // A store that now holds a Claude row is no longer a V8 store: there is nothing to roll back to.
    assert!(v8_view(&v9).is_err());
}

#[test]
fn an_interruption_at_every_stage_returns_nothing_and_leaves_the_source_exactly_as_it_was() {
    let v8 = synthetic_v8(true);
    let before = rows(&v8).unwrap();
    let mut stages = Vec::new();
    migrate(&v8, |s| {
        stages.push(s);
        Ok(())
    })
    .unwrap();
    assert!(stages.len() >= 25, "{} stages", stages.len());
    assert!(stages.contains(&Stage::Boundary) && stages.contains(&Stage::Validated));
    for (i, stage) in stages.iter().enumerate() {
        let mut seen = 0usize;
        let result = migrate(&v8, |_| {
            seen += 1;
            if seen == i + 1 {
                Err(E::StorageUnavailable)
            } else {
                Ok(())
            }
        });
        assert_eq!(result.err(), Some(E::StorageUnavailable), "{stage:?}");
        assert_eq!(rows(&v8).unwrap(), before, "after {stage:?}");
        validate_v8(&v8).unwrap();
    }
}

#[test]
fn a_store_with_work_in_flight_is_not_migrated() {
    let busy = synthetic_v8(false);
    assert_eq!(
        migrate(&busy, |_| Ok(())).err(),
        Some(E::StorageIncompatible)
    );
}

#[test]
fn an_older_binary_refuses_a_v9_store_by_every_door_it_has() {
    let v8 = synthetic_v8(true);
    let mut v9 = migrate(&v8, |_| Ok(())).unwrap();
    // The V8 validator and the V8-era offline opener.
    assert_eq!(validate_v8(&v9).err(), Some(E::StorageIncompatible));
    assert_eq!(
        Store::validate_offline_connection_v8(clone_database(&v9).unwrap()).err(),
        Some(E::StorageIncompatible)
    );
    // The V7 validator and the schema migrator every opener goes through.
    assert_eq!(
        Store::validate_offline_connection(clone_database(&v9).unwrap()).err(),
        Some(E::StorageIncompatible)
    );
    assert_eq!(schema::migrate(&mut v9).err(), Some(E::StorageIncompatible));
}

#[test]
fn rewritten_or_forged_history_and_a_moved_boundary_are_refused_by_the_v9_validator() {
    let v8 = synthetic_v8(true);
    let v9 = migrate(&v8, |_| Ok(())).unwrap();
    // The boundary row cannot be edited or removed.
    for sql in [
        "UPDATE migration_boundary_v9 SET v8_events_sha256=lower(hex(zeroblob(32)))",
        "DELETE FROM migration_boundary_v9",
    ] {
        let e = v9.execute(sql, []).unwrap_err().to_string();
        assert!(e.contains("append_only"), "{sql}: {e}");
    }
    // An event rewritten inside the frozen prefix breaks the digest.
    let image = clone_database(&v9).unwrap();
    image
        .execute_batch("DROP TRIGGER IF EXISTS control_events_no_update")
        .ok();
    image
        .execute(
            "UPDATE control_events SET envelope=envelope||' ' WHERE sequence=(SELECT min(sequence) FROM control_events)",
            [],
        )
        .ok();
    assert!(validate_v9(&image).is_err());
    // A boundary that names another stream is refused too.
    let other = clone_database(&v9).unwrap();
    other
        .execute_batch("DROP TRIGGER migration_boundary_v9_no_update")
        .unwrap();
    other
        .execute(
            "UPDATE migration_boundary_v9 SET v8_stream_id='someone-else'",
            [],
        )
        .unwrap();
    assert!(validate_v9(&other).is_err());
}

#[test]
fn the_row_count_triggers_survive_and_the_runtime_capacity_of_one_is_still_the_registrys() {
    // The store's own limits are row counts (128 runtimes, 4096 launches); the limit of one
    // runtime *running* is the registry's and the engine's, which this migration does not touch
    // and whose test (`the_global_capacity_of_one_runtime_holds_for_claude_and_a_stop_frees_the_slot`)
    // runs unchanged. Here: the triggers are present and identical.
    let v8 = synthetic_v8(true);
    let v9 = migrate(&v8, |_| Ok(())).unwrap();
    let objects = crate::f8::migration::objects(&v9).unwrap();
    for t in [
        "managed_runtimes_capacity",
        "managed_runtime_launches_capacity",
    ] {
        assert!(objects[t].contains("capacity_exceeded"), "{t}");
    }
    let old = crate::f8::migration::objects(&v8).unwrap();
    assert_eq!(
        objects["managed_runtimes_capacity"],
        old["managed_runtimes_capacity"]
    );
}

#[test]
fn the_rehearsal_reads_no_path_and_is_not_compiled_into_a_package() {
    // The words are assembled so that this test does not find itself.
    let forbidden = [
        ["std", "::fs"].concat(),
        ["Fil", "e::"].concat(),
        ["Pat", "h"].concat(),
        ["XD", "G"].concat(),
        ["home", "_dir"].concat(),
        ["env", "::var"].concat(),
    ];
    for (name, source) in [
        ("v9_candidate.rs", include_str!("../v9_candidate.rs")),
        (
            "v9_rehearsal_tests.rs",
            include_str!("v9_rehearsal_tests.rs"),
        ),
    ] {
        for (n, line) in source.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for word in &forbidden {
                assert!(!line.contains(word.as_str()), "{name}:{}: {word}", n + 1);
            }
        }
    }
    let f8 = include_str!("../../f8.rs");
    assert!(f8.contains("#[cfg(test)]\npub(crate) mod v9_candidate;"));
}
