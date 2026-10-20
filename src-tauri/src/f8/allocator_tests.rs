use super::*;
use crate::{
    control::f8_fixture,
    f8::{
        migration,
        protocol::{Response, RuntimeRef},
        registry::Registry,
        v8::validate_v8,
    },
};

fn uuid() -> String {
    crate::control::store::uuid().as_str().to_owned()
}
fn wire(s: &str) -> WireUuid {
    WireUuid::try_from(s.to_owned()).unwrap()
}
fn image() -> (Connection, String) {
    let mut source = f8_fixture::v7_with_completed_turn();
    let c = migration::prepare(&mut source).unwrap().into_connection();
    let session = c
        .query_row("SELECT session_id FROM managed_sessions", [], |r| r.get(0))
        .unwrap();
    (c, session)
}
fn request(method: &str, params: Value) -> Request {
    Request::parse(
        &serde_json::to_vec(&json!({"schema_version":2,"request_id":uuid(),
        "authority_instance_id":A,"method":method,"params":params}))
        .unwrap(),
    )
    .unwrap()
}
fn start(session: &str, intent: &str, generation: u64, revision: u64) -> Request {
    request(
        "runtime/start",
        json!({"intent_id":intent,"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":session,"expected_generation":generation,"expected_revision":revision}),
    )
}
fn stop(
    session: &str,
    intent: &str,
    generation: u64,
    revision: u64,
    instance: Option<&str>,
) -> Request {
    request(
        "runtime/stop",
        json!({"intent_id":intent,"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":session,"expected_generation":generation,"expected_revision":revision,
        "expected_runtime_instance_id":instance}),
    )
}
fn free() -> Admission {
    Admission {
        live: None,
        slot_available: true,
    }
}
fn live(authority: &str, o: &Outcome) -> Admission {
    Admission {
        live: Some(RuntimeRef {
            schema: 2,
            authority_instance_id: wire(authority),
            managed_session_id: o.managed_session_id.clone(),
            runtime_instance_id: o.runtime_instance_id.clone().unwrap(),
            generation: o.generation,
        }),
        slot_available: true,
    }
}
fn counts(c: &Connection) -> (i64, i64, i64, i64, String) {
    let n = |t: &str| {
        c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    };
    (
        n("control_events"),
        n("processed_intents"),
        n("managed_runtimes"),
        n("managed_runtime_launches"),
        c.query_row("SELECT last_sequence FROM authority_metadata", [], |r| {
            r.get(0)
        })
        .unwrap(),
    )
}
fn reference(authority: &str, o: &Outcome) -> RuntimeRef {
    live(authority, o).live.unwrap()
}
const A: &str = "00000000-0000-4000-8000-0000000000a1";
const B: &str = "00000000-0000-4000-8000-0000000000b2";

#[test]
fn first_start_reserves_atomically_and_exact_replay_has_zero_side_effects() {
    let (mut c, s) = image();
    let intent = uuid();
    let r = start(&s, &intent, 0, 0);
    let o = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&r, &free())
        .unwrap();
    assert_eq!(
        (o.generation, o.record.revision, o.reserved, o.replayed),
        (1, 1, true, false)
    );
    validate_v8(&c).unwrap();
    let before = counts(&c);
    // Replay precedes capacity, state and registry checks: it works with no free slot at all.
    let full = Admission {
        live: None,
        slot_available: false,
    };
    let again = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&r, &full)
        .unwrap();
    assert!(again.replayed && !again.reserved);
    assert_eq!(
        (
            again.generation,
            again.runtime_instance_id.clone(),
            again.record.clone()
        ),
        (1, o.runtime_instance_id.clone(), o.record.clone())
    );
    assert_eq!(counts(&c), before);
    // A changed fence or payload under the same intent is a conflict.
    for changed in [start(&s, &intent, 1, 1), start(&s, &intent, 0, 1)] {
        assert_eq!(
            Allocator::new(&mut c, wire(A))
                .unwrap()
                .start(&changed, &free())
                .err(),
            Some(E::IntentConflict)
        );
    }
    assert_eq!(counts(&c), before);
    // The wire response built from the outcome parses and correlates with its request.
    let value = json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":"running","revision":1,
        "created_at":o.record.created_at,"updated_at":o.record.updated_at,"managed_session_id":s,"generation":1});
    let response = Response::parse(
        &serde_json::to_vec(&json!({"schema_version":2,"request_id":r.request_id(),
        "authority_instance_id":A,"result":{"kind":"runtime_control","value":value}}))
        .unwrap(),
    )
    .unwrap();
    response.correlate(&r, &wire(A)).unwrap();
}

#[test]
fn live_launch_is_retained_but_a_dead_one_consumes_a_new_generation() {
    let (mut c, s) = image();
    let first = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&start(&s, &uuid(), 0, 0), &free())
        .unwrap();
    // Retention: same generation, same instance, revision +1, nothing reserved.
    let retained = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&start(&s, &uuid(), 1, 1), &live(A, &first))
        .unwrap();
    assert_eq!(
        (
            retained.generation,
            retained.record.revision,
            retained.reserved
        ),
        (1, 2, false)
    );
    assert_eq!(retained.runtime_instance_id, first.runtime_instance_id);
    validate_v8(&c).unwrap();
    // The launch failed before readiness: the registry reports nothing live. The failed
    // attempt keeps its generation; the next one gets exactly the next, with a fresh identity.
    let second = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&start(&s, &uuid(), 1, 2), &free())
        .unwrap();
    assert_eq!(
        (second.generation, second.record.revision, second.reserved),
        (2, 3, true)
    );
    assert_ne!(second.runtime_instance_id, first.runtime_instance_id);
    validate_v8(&c).unwrap();
    let launches: i64 = c
        .query_row("SELECT count(*) FROM managed_runtime_launches", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(launches, 2);
    // A registry claim of liveness for a different generation never retains.
    let stale = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&start(&s, &uuid(), 2, 3), &live(A, &first))
        .unwrap();
    assert!(stale.reserved && stale.generation == 3);
    validate_v8(&c).unwrap();
}

#[test]
fn stale_expectations_capacity_overflow_and_closed_sessions_refuse_without_writes() {
    let (mut c, s) = image();
    let o = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&start(&s, &uuid(), 0, 0), &free())
        .unwrap();
    let before = counts(&c);
    let attempts = [
        (start(&s, &uuid(), 0, 1), free(), E::StaleRevision),
        (start(&s, &uuid(), 1, 0), free(), E::StaleRevision),
        (start(&s, &uuid(), 2, 1), free(), E::StaleRevision),
        (
            start(&s, &uuid(), 1, 1),
            Admission {
                live: None,
                slot_available: false,
            },
            E::CapacityExceeded,
        ),
        (start(&uuid(), &uuid(), 0, 0), free(), E::SessionNotFound),
    ];
    for (r, admission, expected) in attempts {
        assert_eq!(
            Allocator::new(&mut c, wire(A))
                .unwrap()
                .start(&r, &admission)
                .err(),
            Some(expected)
        );
        assert_eq!(counts(&c), before);
    }
    // Generation overflow refuses before any durable mutation.
    c.execute_batch("PRAGMA foreign_keys=off").unwrap();
    for t in c.prepare("SELECT sql FROM sqlite_master WHERE type='trigger' AND name='managed_runtimes_transition'").unwrap()
        .query_map([], |r| r.get::<_, String>(0)).unwrap().map(Result::unwrap).collect::<Vec<_>>() {
        c.execute_batch("DROP TRIGGER managed_runtimes_transition").unwrap();
        c.execute("UPDATE managed_runtimes SET generation=9223372036854775807", []).unwrap();
        c.execute_batch(&t).unwrap();
    }
    let before = counts(&c);
    assert_eq!(
        Allocator::new(&mut c, wire(A))
            .unwrap()
            .start(&start(&s, &uuid(), i64::MAX as u64, 1), &free())
            .err(),
        Some(E::CapacityExceeded)
    );
    assert_eq!(counts(&c), before);
    let _ = o;
    // A closed session never reserves another launch.
    c.execute_batch("UPDATE managed_sessions SET control_state='closed',revision=2")
        .unwrap();
    assert_eq!(
        Allocator::new(&mut c, wire(A))
            .unwrap()
            .start(&start(&s, &uuid(), i64::MAX as u64, 1), &free())
            .err(),
        Some(E::InvalidTransition)
    );
}

#[test]
fn runtime_row_capacity_is_enforced_before_the_extra_row() {
    let (mut c, s) = image();
    c.execute_batch("PRAGMA foreign_keys=off").unwrap();
    for n in 0..128u32 {
        let id = format!("00000000-0000-4000-8000-{:012x}", 0x9000 + n);
        c.execute("INSERT INTO managed_sessions SELECT ?1,adapter_id,runtime_kind,control_state,revision,created_at,updated_at,?2 FROM managed_sessions WHERE session_id=?3", params![id, format!("{:020}", 100 + n), s]).unwrap();
        c.execute(
            "INSERT INTO managed_runtimes VALUES(?1,'codex','codex','stopped',1,0,'t','t')",
            [&id],
        )
        .unwrap();
    }
    let before = counts(&c);
    assert_eq!(
        Allocator::new(&mut c, wire(A))
            .unwrap()
            .start(&start(&s, &uuid(), 0, 0), &free())
            .err(),
        Some(E::CapacityExceeded)
    );
    assert_eq!(counts(&c), before);
}

#[test]
fn stop_fences_the_observed_launch_and_never_treats_null_as_a_wildcard() {
    let (mut c, s) = image();
    // Stopping a never-launched runtime creates the stopped row at generation 0.
    let absent = Allocator::new(&mut c, wire(A))
        .unwrap()
        .stop(&stop(&s, &uuid(), 0, 0, None), &free())
        .unwrap();
    assert_eq!(
        (
            absent.generation,
            absent.record.revision,
            absent.runtime_instance_id.clone()
        ),
        (0, 1, None)
    );
    validate_v8(&c).unwrap();
    let o = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&start(&s, &uuid(), 0, 1), &free())
        .unwrap();
    assert_eq!((o.generation, o.record.revision), (1, 2));
    let instance = o.runtime_instance_id.clone().unwrap();
    let before = counts(&c);
    for (r, why) in [
        (stop(&s, &uuid(), 1, 2, None), "null while a launch exists"),
        (stop(&s, &uuid(), 1, 2, Some(&uuid())), "another instance"),
        (
            stop(&s, &uuid(), 1, 1, Some(instance.as_str())),
            "stale revision",
        ),
    ] {
        assert_eq!(
            Allocator::new(&mut c, wire(A))
                .unwrap()
                .stop(&r, &free())
                .err(),
            Some(E::StaleRevision),
            "{why}"
        );
        assert_eq!(counts(&c), before);
    }
    let stopped = Allocator::new(&mut c, wire(A))
        .unwrap()
        .stop(&stop(&s, &uuid(), 1, 2, Some(instance.as_str())), &free())
        .unwrap();
    assert_eq!(
        (
            stopped.generation,
            stopped.record.revision,
            stopped.runtime_instance_id.clone()
        ),
        (1, 3, None)
    );
    validate_v8(&c).unwrap();
    // After the stop a launch fence names nothing: only null is current.
    let again = Allocator::new(&mut c, wire(A))
        .unwrap()
        .stop(&stop(&s, &uuid(), 1, 3, None), &free())
        .unwrap();
    assert_eq!(again.record.revision, 4);
    validate_v8(&c).unwrap();
}

#[test]
fn every_transaction_boundary_rolls_back_and_a_retry_succeeds_exactly_once() {
    for failing in 0..4 {
        let (mut c, s) = image();
        let intent = uuid();
        let before = counts(&c);
        let r = start(&s, &intent, 0, 0);
        let err = Allocator::new(&mut c, wire(A))
            .unwrap()
            .with_fault(move |n| {
                if n == failing {
                    Err(E::InternalError)
                } else {
                    Ok(())
                }
            })
            .start(&r, &free())
            .err();
        assert_eq!(err, Some(E::InternalError), "step {failing}");
        assert_eq!(counts(&c), before, "step {failing}");
        validate_v8(&c).unwrap();
        let o = Allocator::new(&mut c, wire(A))
            .unwrap()
            .start(&r, &free())
            .unwrap();
        assert_eq!((o.generation, o.reserved), (1, true));
        validate_v8(&c).unwrap();
    }
}

#[test]
fn readiness_and_exit_are_once_only_owner_observations() {
    let (mut c, s) = image();
    let o = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&start(&s, &uuid(), 0, 0), &free())
        .unwrap();
    let r = reference(A, &o);
    // A successor authority cannot observe, fabricate or exit the predecessor's launch.
    {
        let mut b = Allocator::new(&mut c, wire(B)).unwrap();
        assert_eq!(
            b.record_ready(&r, "0.159.0", "compatible").err(),
            Some(E::CorrelationError)
        );
        let foreign = RuntimeRef {
            authority_instance_id: wire(B),
            ..r.clone()
        };
        assert_eq!(
            b.record_exit(&foreign, "child_exited", false, None).err(),
            Some(E::StaleRevision)
        );
    }
    let mut a = Allocator::new(&mut c, wire(A)).unwrap();
    assert_eq!(
        a.record_ready(&r, "v1", "compatible").err(),
        Some(E::StorageIncompatible)
    );
    a.record_ready(&r, "0.159.0", "compatible").unwrap();
    assert_eq!(
        a.record_ready(&r, "0.159.0", "compatible").err(),
        Some(E::InvalidTransition)
    );
    let stale = RuntimeRef {
        generation: 2,
        ..r.clone()
    };
    assert_eq!(
        a.record_ready(&stale, "0.159.0", "compatible").err(),
        Some(E::StaleRevision)
    );
    assert!(
        a.record_exit(&r, "budget_exceeded", false, Some("nonsense"))
            .is_err()
    );
    a.record_exit(&r, "budget_exceeded", false, Some("turn_output_bytes"))
        .unwrap();
    assert_eq!(
        a.record_exit(&r, "child_exited", false, None).err(),
        Some(E::InvalidTransition)
    );
    assert_eq!(
        a.record_ready(&r, "0.159.0", "compatible").err(),
        Some(E::InvalidTransition)
    );
    drop(a);
    validate_v8(&c).unwrap();
}

#[test]
fn restart_leaves_predecessor_launches_unowned_and_reuse_impossible() {
    let (mut c, s) = image();
    let o = Allocator::new(&mut c, wire(A))
        .unwrap()
        .start(&start(&s, &uuid(), 0, 0), &free())
        .unwrap();
    // Authority restarts as B: the old launch is unowned history, not adopted, not exited.
    let b = Allocator::new(&mut c, wire(B)).unwrap();
    let unowned = b.unowned_launches().unwrap();
    assert_eq!(unowned, vec![reference(A, &o)]);
    let (used, generations) = b.registry_history().unwrap();
    assert_eq!((used.len(), generations.clone()), (1, vec![(s.clone(), 1)]));
    // The primed registry refuses the dead identity and any generation not beyond history.
    drop(b);
    let mut registry = Registry::with_history(wire(B), used, generations).unwrap();
    assert_eq!(registry.active_slots(), 0);
    let old = RuntimeRef {
        authority_instance_id: wire(B),
        ..reference(A, &o)
    };
    assert_eq!(registry.reserve(old).err(), Some(E::StaleRevision));
    // The restarted authority cannot retain the old launch even if told it is live.
    let retained = Allocator::new(&mut c, wire(B))
        .unwrap()
        .start(&start(&s, &uuid(), 1, 1), &live(A, &o))
        .unwrap();
    assert!(retained.reserved && retained.generation == 2);
    assert_ne!(retained.runtime_instance_id, o.runtime_instance_id);
    validate_v8(&c).unwrap();
}

#[test]
fn every_authority_instance_announces_itself_once_naming_its_predecessor() {
    let (mut c, s) = image();
    let started = |c: &Connection| -> Vec<(String, Option<String>)> {
        c.prepare("SELECT json_extract(envelope,'$.source.instance_id'),json_extract(envelope,'$.payload.predecessor_instance_id') FROM control_events WHERE json_extract(envelope,'$.event_type')='authority.instance.started' ORDER BY sequence")
            .unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    };
    assert!(started(&c).is_empty());
    Allocator::new(&mut c, wire(A)).unwrap().start(&start(&s, &uuid(), 0, 0), &free()).unwrap();
    // A replay (no write) and further writes by the same instance announce nothing more.
    let instance = instance_of(&c);
    Allocator::new(&mut c, wire(A)).unwrap().stop(&stop(&s, &uuid(), 1, 1, Some(&instance)), &free()).unwrap();
    let first = started(&c);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, A);
    assert!(first[0].1.is_some(), "the migrated store's last writer is named");
    Allocator::new(&mut c, wire(B)).unwrap().start(&start(&s, &uuid(), 1, 2), &free()).unwrap();
    assert_eq!(started(&c), vec![first[0].clone(), (B.to_owned(), Some(A.to_owned()))]);
    validate_v8(&c).unwrap();
}
fn instance_of(c: &Connection) -> String {
    c.query_row("SELECT runtime_instance_id FROM managed_runtime_launches", [], |r| r.get(0)).unwrap()
}
