use super::*;
use crate::contracts::{
    agent_adapter::{AgentAdapterId, AgentRuntimeKind},
    control::*,
    identities::ManagedSessionId,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use store::{Store, uuid};

struct Temp {
    root: PathBuf,
}
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("wyss-control-test-{}", uuid().as_str()));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self { root: p }
    }
    fn store(&self) -> Store {
        Store::open(&self.root.join("control.sqlite3"), uuid()).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn prepare() -> Operation {
    Operation::Prepare(Prepare {
        intent_id: uuid(),
        adapter_id: AgentAdapterId::parse("codex").unwrap(),
        runtime_kind: AgentRuntimeKind::Codex,
    })
}
fn session(v: ResultValue) -> ManagedSessionRecord {
    if let ResultValue::Session(s) = v {
        s
    } else {
        panic!("session expected")
    }
}
fn close(s: &ManagedSessionRecord) -> Operation {
    Operation::Close(Close {
        intent_id: uuid(),
        managed_session_id: s.managed_session_id.clone(),
        expected_revision: s.revision,
    })
}
fn events(s: &mut Store) -> Vec<crate::contracts::event_envelope::EventEnvelopeV1> {
    let p = AuditRead {
        stream_id: s.status().unwrap().stream_id,
        after_sequence: None,
        limit: 50,
    };
    if let ResultValue::Audit { events, .. } = s.apply(&Operation::AuditRead(p)).unwrap() {
        events
    } else {
        panic!("audit expected")
    }
}
#[test]
fn fresh_schema_reopen_and_settings() {
    let t = Temp::new();
    let s = t.store();
    let stream = s.status().unwrap().stream_id;
    assert_eq!(
        s.conn
            .pragma_query_value::<String, _>(None, "journal_mode", |r| r.get(0))
            .unwrap(),
        "wal"
    );
    assert_eq!(
        s.conn
            .pragma_query_value::<i64, _>(None, "synchronous", |r| r.get(0))
            .unwrap(),
        2
    );
    assert_eq!(
        s.conn
            .pragma_query_value::<i64, _>(None, "foreign_keys", |r| r.get(0))
            .unwrap(),
        1
    );
    drop(s);
    assert_eq!(t.store().status().unwrap().stream_id, stream);
}
#[test]
fn unknown_version_fails_closed() {
    let t = Temp::new();
    let s = t.store();
    s.conn.pragma_update(None, "user_version", 8).unwrap();
    drop(s);
    assert!(matches!(
        Store::open(&t.root.join("control.sqlite3"), uuid()),
        Err(ControlError::StorageIncompatible)
    ));
}
#[test]
fn corrupt_database_not_recreated() {
    let t = Temp::new();
    let path = t.root.join("control.sqlite3");
    paths::private_file(&path).unwrap();
    fs::write(&path, b"corrupt not sqlite").unwrap();
    assert!(Store::open(&path, uuid()).is_err());
    assert_eq!(fs::read(path).unwrap(), b"corrupt not sqlite");
}
#[test]
fn changed_schema_fails_closed() {
    let t = Temp::new();
    let s = t.store();
    s.conn
        .execute_batch("DROP TRIGGER control_events_no_update")
        .unwrap();
    drop(s);
    assert!(matches!(
        Store::open(&t.root.join("control.sqlite3"), uuid()),
        Err(ControlError::StorageIncompatible)
    ));
}
#[test]
fn schema_migration_down_and_up_on_empty_and_existing_data() {
    for data in [false, true] {
        let t = Temp::new();
        let mut s = t.store();
        if data {
            s.apply(&prepare()).unwrap();
        }
        for (name, sql) in schema::OBJECTS.iter().rev() {
            let kind = if sql.starts_with("CREATE TRIGGER") {
                "TRIGGER"
            } else if sql.starts_with("CREATE UNIQUE INDEX") {
                "INDEX"
            } else {
                "TABLE"
            };
            s.conn
                .execute_batch(&format!("DROP {kind} {name}"))
                .unwrap();
        }
        s.conn.pragma_update(None, "user_version", 0).unwrap();
        schema::migrate(&mut s.conn).unwrap();
        assert_eq!(s.status().unwrap().last_sequence, None);
    }
}
#[test]
fn authority_allocates_prepared_identity_and_round_trip() {
    let t = Temp::new();
    let mut s = t.store();
    let a = session(s.apply(&prepare()).unwrap());
    let b = session(s.apply(&prepare()).unwrap());
    assert_ne!(a.managed_session_id, b.managed_session_id);
    assert_eq!(a.revision, 1);
    assert_eq!(a.control_state, ControlState::Prepared);
    assert_eq!(
        session(
            s.apply(&Operation::Get(Get {
                managed_session_id: a.managed_session_id.clone()
            }))
            .unwrap()
        ),
        a
    );
}
#[test]
fn close_only_advances_prepared_state() {
    let t = Temp::new();
    let mut s = t.store();
    let a = session(s.apply(&prepare()).unwrap());
    let b = session(s.apply(&close(&a)).unwrap());
    assert_eq!(b.control_state, ControlState::Closed);
    assert_eq!(b.revision, 2);
    assert_eq!(
        session(
            s.apply(&Operation::Get(Get {
                managed_session_id: b.managed_session_id.clone()
            }))
            .unwrap()
        ),
        b
    );
    let again = session(s.apply(&close(&b)).unwrap());
    assert_eq!(again, b);
    assert_eq!(events(&mut s).len(), 2);
}
#[test]
fn prepare_and_close_retries_survive_reopen() {
    let t = Temp::new();
    let mut s = t.store();
    let p = prepare();
    let a = session(s.apply(&p).unwrap());
    drop(s);
    let mut s = t.store();
    assert_eq!(session(s.apply(&p).unwrap()), a);
    assert_eq!(events(&mut s).len(), 1);
    let op = close(&a);
    let b = session(s.apply(&op).unwrap());
    drop(s);
    let mut s = t.store();
    assert_eq!(session(s.apply(&op).unwrap()), b);
    assert_eq!(events(&mut s).len(), 2);
    assert_eq!(session(s.apply(&p).unwrap()), a);
}
#[test]
fn conflicting_intent_reuse_and_stale_revision() {
    let t = Temp::new();
    let mut s = t.store();
    let p = prepare();
    let a = session(s.apply(&p).unwrap());
    let Operation::Prepare(mut conflict) = p else {
        unreachable!()
    };
    conflict.runtime_kind = AgentRuntimeKind::GenericCli;
    assert!(matches!(
        s.apply(&Operation::Prepare(conflict)),
        Err(ControlError::IntentConflict)
    ));
    s.apply(&close(&a)).unwrap();
    assert!(matches!(
        s.apply(&close(&a)),
        Err(ControlError::StaleRevision)
    ));
}
#[test]
fn unknown_session_not_found() {
    let t = Temp::new();
    let mut s = t.store();
    let id = ManagedSessionId::parse(uuid().as_str()).unwrap();
    assert!(matches!(
        s.apply(&Operation::Get(Get {
            managed_session_id: id
        })),
        Err(ControlError::SessionNotFound)
    ));
}
#[test]
fn sequence_and_instance_provenance_survive_restart() {
    let t = Temp::new();
    let mut s = t.store();
    let a = session(s.apply(&prepare()).unwrap());
    let old = serde_json::to_value(&events(&mut s)[0]).unwrap();
    let stream = s.status().unwrap().stream_id;
    drop(s);
    let mut s = t.store();
    s.apply(&close(&a)).unwrap();
    let e = events(&mut s);
    assert_eq!(e[1].sequence().get(), 2);
    let new = serde_json::to_value(&e[1]).unwrap();
    assert_eq!(new["stream_id"], old["stream_id"]);
    assert_eq!(s.status().unwrap().stream_id, stream);
    assert_ne!(new["source"]["instance_id"], old["source"]["instance_id"]);
}
#[test]
fn append_only_and_unique_audit_constraints() {
    let t = Temp::new();
    let mut s = t.store();
    s.apply(&prepare()).unwrap();
    for sql in [
        "DELETE FROM control_events",
        "UPDATE control_events SET event_id='changed'",
        "INSERT INTO control_events SELECT * FROM control_events",
        "INSERT INTO control_events SELECT event_id,stream_id,'00000000000000000099',session_id,envelope FROM control_events",
        "INSERT INTO control_events SELECT 'different',stream_id,sequence,session_id,envelope FROM control_events",
    ] {
        assert!(s.conn.execute_batch(sql).is_err());
    }
    assert_eq!(events(&mut s).len(), 1);
}
#[test]
fn audit_failure_rolls_back_prepare_close_sequence_and_receipt() {
    let t = Temp::new();
    let mut s = t.store();
    let p = prepare();
    let a = session(s.apply(&p).unwrap());
    s.conn.execute_batch("CREATE TRIGGER fail_audit BEFORE INSERT ON control_events BEGIN SELECT RAISE(ABORT,'injected'); END").unwrap();
    let q = prepare();
    let c = close(&a);
    assert!(s.apply(&q).is_err());
    assert!(s.apply(&c).is_err());
    assert_eq!(s.status().unwrap().prepared_count, 1);
    assert_eq!(s.status().unwrap().last_sequence.unwrap().get(), 1);
    assert_eq!(
        session(
            s.apply(&Operation::Get(Get {
                managed_session_id: a.managed_session_id.clone()
            }))
            .unwrap()
        ),
        a
    );
    assert_eq!(
        s.conn
            .query_row::<i64, _, _>("SELECT count(*) FROM processed_intents", [], |r| r.get(0))
            .unwrap(),
        1
    );
    s.conn.execute_batch("DROP TRIGGER fail_audit").unwrap();
    s.apply(&q).unwrap();
    s.apply(&c).unwrap();
    assert_eq!(events(&mut s).len(), 3);
}
#[test]
fn lists_and_audit_are_paginated_and_minimized() {
    let t = Temp::new();
    let mut s = t.store();
    for _ in 0..4 {
        s.apply(&prepare()).unwrap();
    }
    let ResultValue::Sessions { sessions, next } = s
        .apply(&Operation::List(List {
            after: None,
            limit: 2,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(sessions.len(), 2);
    let ResultValue::Sessions { sessions: tail, .. } = s
        .apply(&Operation::List(List {
            after: next,
            limit: 2,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(tail.len(), 2);
    assert_ne!(sessions[0].managed_session_id, tail[0].managed_session_id);
    let stream = s.status().unwrap().stream_id;
    let ResultValue::Audit { events, next } = s
        .apply(&Operation::AuditRead(AuditRead {
            stream_id: stream.clone(),
            after_sequence: None,
            limit: 2,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(events.len(), 2);
    let ResultValue::Audit { events: tail, .. } = s
        .apply(&Operation::AuditRead(AuditRead {
            stream_id: stream,
            after_sequence: next,
            limit: 2,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(tail[0].sequence().get(), 3);
    for event in events {
        let v = serde_json::to_value(event).unwrap();
        assert_eq!(
            v["payload"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["adapter_id", "control_state", "revision", "runtime_kind"]
        );
        assert_eq!(v["persistence"], "durable");
    }
    let mut q = s
        .conn
        .prepare("SELECT mutation,result FROM processed_intents")
        .unwrap();
    for row in q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
    {
        let (m, r) = row.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&m).unwrap()["params"]
                .as_object()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            serde_json::from_str::<Value>(&r)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            8
        );
    }
}
#[test]
fn sequence_exhaustion_fails_closed() {
    let t = Temp::new();
    let mut s = t.store();
    s.conn
        .execute(
            "UPDATE authority_metadata SET last_sequence=?1",
            [u64::MAX.to_string()],
        )
        .unwrap();
    assert!(matches!(
        s.apply(&prepare()),
        Err(ControlError::CapacityExceeded)
    ));
    assert_eq!(s.status().unwrap().prepared_count, 0);
}
#[test]
fn private_database_sidecars() {
    let t = Temp::new();
    let mut s = t.store();
    s.apply(&prepare()).unwrap();
    for name in [
        "control.sqlite3",
        "control.sqlite3-wal",
        "control.sqlite3-shm",
    ] {
        paths::validate_file(&t.root.join(name)).unwrap();
    }
}
#[test]
fn runtime_directory_policy() {
    let t = Temp::new();
    assert!(paths::Paths::new(std::path::Path::new("relative"), &t.root).is_err());
    fs::set_permissions(&t.root, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(paths::runtime_product(&t.root, true).is_err());
    fs::set_permissions(&t.root, fs::Permissions::from_mode(0o700)).unwrap();
    let product = paths::runtime_product(&t.root, true).unwrap();
    assert_eq!(fs::metadata(product).unwrap().mode() & 0o777, 0o700);
}
#[test]
fn socket_file_and_symlink_never_deleted() {
    let t = Temp::new();
    let path = t.root.join("control.sock");
    fs::write(&path, b"keep").unwrap();
    assert!(server::bind(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"keep");
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(t.root.join("missing"), &path).unwrap();
    assert!(server::bind(&path).is_err());
    assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
}
#[test]
fn stale_socket_recovery_and_mode() {
    let t = Temp::new();
    let path = t.root.join("control.sock");
    drop(UnixListener::bind(&path).unwrap());
    let (l, g) = server::bind(&path).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    assert!(matches!(
        server::bind(&path),
        Err(ControlError::AlreadyRunning)
    ));
    drop(l);
    drop(g);
    assert!(!path.exists());
}
#[test]
fn same_uid_kernel_peer_accepted_and_other_uid_rejected() {
    let (a, _b) = UnixStream::pair().unwrap();
    server::peer(&a).unwrap();
    assert_eq!(
        server::check_uid(nix::unistd::geteuid().as_raw().wrapping_add(1)),
        Err(ControlError::PeerRejected)
    );
}
#[test]
fn state_lock_is_exclusive_across_runtime_locations() {
    let t = Temp::new();
    let a = t.root.join("a");
    let b = t.root.join("b");
    for p in [&a, &b] {
        fs::DirBuilder::new().mode(0o700).create(p).unwrap();
    }
    let p = paths::Paths::new(&a, &t.root).unwrap();
    let q = paths::Paths::new(&b, &t.root).unwrap();
    let _lock = p.lock().unwrap();
    assert!(matches!(q.lock(), Err(ControlError::AlreadyRunning)));
}

fn request(op: Operation) -> Request {
    Request {
        request_id: uuid(),
        operation: op,
    }
}
#[test]
fn protocol_hello_and_method_allowlist() {
    let r = request(Operation::Hello(Hello {
        protocol_version: 1,
        client_instance_id: uuid(),
    }));
    let parsed = Request::parse(&r.encode().unwrap()).unwrap();
    assert_eq!(r.request_id, parsed.request_id);
    assert_eq!(
        METHODS,
        [
            "managedSession/turn/start",
            "managedSession/turn/status",
            "managedSession/turn/events",
            "managedSession/turn/cancel",
            "managedSession/runtime/create",
            "managedSession/runtime/resume",
            "managedSession/runtime/status",
            "managedSession/runtime/abandon",
            "authority/hello",
            "authority/status",
            "managedSession/prepare",
            "managedSession/get",
            "managedSession/list",
            "managedSession/close",
            "audit/read",
            "audit/watch",
            "runtime/start",
            "runtime/status",
            "runtime/model/list",
            "runtime/stop",
            "executionLease/grant",
            "executionLease/get",
            "executionLease/active",
            "executionLease/renew",
            "executionLease/revoke",
            "executionGate/status",
            "managedTurn/approval/pending",
            "managedTurn/approval/status",
            "managedTurn/approval/respond"
        ]
    );
    for forbidden in [
        "thread/start",
        "turn/start",
        "prompt/submit",
        "process/spawn",
        "approve",
        "permission/grant",
        "authority/shutdown",
        // No generic provider response, raw provider RPC or provider request identity.
        "managedTurn/approval/accept",
        "provider/respond",
        "provider/request/respond",
        "codex/rpc",
        "item/commandExecution/requestApproval",
        "item/fileChange/requestApproval",
        "config/write",
    ] {
        let v = json!({"schema_version":1,"request_id":uuid(),"method":forbidden,"params":{}});
        assert!(matches!(
            Request::parse(&serde_json::to_vec(&v).unwrap()),
            Err(ControlError::UnsupportedMethod)
        ));
    }
}
#[test]
fn protocol_rejects_unknown_versions_fields_and_content() {
    let r = request(prepare());
    let original: Value = serde_json::from_slice(&r.encode().unwrap()).unwrap();
    for change in ["version", "extra", "prompt", "id", "limit"] {
        let mut v = original.clone();
        match change {
            "version" => v["schema_version"] = json!(2),
            "extra" => v["extra"] = json!(true),
            "prompt" => v["params"]["prompt"] = json!("never accepted"),
            "id" => v["request_id"] = json!("bad"),
            _ => v["params"]["intent_id"] = Value::Null,
        };
        assert!(Request::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    }
}
#[test]
fn duplicate_members_and_bad_json_utf8_reject() {
    for b in [
        b"{\"schema_version\":1,\"schema_version\":1}".as_slice(),
        b"{\"params\":{\"intent_id\":1,\"intent_id\":2}}",
        b"{bad}",
        b"\xff",
        b"{}{}",
    ] {
        assert!(unique_json(b).is_err());
    }
}
#[test]
fn invalid_enum_and_bounds_reject() {
    for state in ["active", "running"] {
        assert!(serde_json::from_value::<ControlState>(json!(state)).is_err());
    }
    for limit in [0, 51, u16::MAX] {
        assert!(
            Operation::List(List { after: None, limit })
                .validate()
                .is_err()
        );
    }
    assert!(
        Operation::Hello(Hello {
            protocol_version: 2,
            client_instance_id: uuid()
        })
        .validate()
        .is_err()
    );
}
#[test]
fn response_is_exclusive_and_correlated() {
    let t = Temp::new();
    let s = t.store();
    let response = Response {
        request_id: uuid(),
        authority_instance_id: uuid(),
        outcome: Ok(ResultValue::Status(s.status().unwrap())),
    };
    let bytes = response.encode().unwrap();
    let parsed = Response::parse(&bytes).unwrap();
    assert_eq!(response.request_id, parsed.request_id);
    let mut v: Value = serde_json::from_slice(&bytes).unwrap();
    v["error"] = json!({"code":"internal_error"});
    assert!(Response::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    v.as_object_mut().unwrap().remove("result");
    assert!(
        Response::parse(&serde_json::to_vec(&v).unwrap())
            .unwrap()
            .outcome
            .is_err()
    );
}
#[test]
fn frame_rejects_zero_oversized_and_truncated() {
    for (bytes, expected) in [
        (vec![0, 0, 0, 0], ControlError::InvalidRequest),
        (
            (MAX_FRAME as u32 + 1).to_be_bytes().to_vec(),
            ControlError::FrameTooLarge,
        ),
        (vec![0, 0], ControlError::TruncatedFrame),
        (vec![0, 0, 0, 4, b'{'], ControlError::TruncatedFrame),
    ] {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        a.set_nonblocking(true).unwrap();
        b.write_all(&bytes).unwrap();
        drop(b);
        assert_eq!(
            framing::read_frame(
                &mut a,
                Instant::now() + Duration::from_secs(1),
                &AtomicBool::new(false)
            ),
            Err(expected)
        );
    }
}
#[test]
fn frame_exact_limit_and_absolute_timeout() {
    let (mut a, mut b) = UnixStream::pair().unwrap();
    a.set_nonblocking(true).unwrap();
    b.set_nonblocking(true).unwrap();
    let stop = AtomicBool::new(false);
    let end = Instant::now() + Duration::from_secs(1);
    framing::write_frame(&mut b, &vec![b'x'; MAX_FRAME], end, &stop).unwrap();
    assert_eq!(
        framing::read_frame(&mut a, end, &stop)
            .unwrap()
            .unwrap()
            .len(),
        MAX_FRAME
    );
    assert_eq!(
        framing::read_frame(&mut a, Instant::now() + Duration::from_millis(20), &stop),
        Err(ControlError::Timeout)
    );
}
#[test]
fn shutdown_cancels_partial_frame() {
    let (mut a, mut b) = UnixStream::pair().unwrap();
    a.set_nonblocking(true).unwrap();
    b.write_all(&[0, 0, 0, 4, b'{']).unwrap();
    let stop = AtomicBool::new(true);
    assert_eq!(
        framing::read_frame(&mut a, Instant::now() + Duration::from_secs(1), &stop),
        Err(ControlError::TransportError)
    );
}

#[test]
fn response_correlation_rejects_wrong_id() {
    let (mut a, mut b) = UnixStream::pair().unwrap();
    a.set_nonblocking(true).unwrap();
    b.set_nonblocking(true).unwrap();
    let worker = std::thread::spawn(move || {
        let stop = AtomicBool::new(false);
        let end = Instant::now() + Duration::from_secs(1);
        let r = Request::parse(&framing::read_frame(&mut b, end, &stop).unwrap().unwrap()).unwrap();
        let wrong = Response {
            request_id: uuid(),
            authority_instance_id: uuid(),
            outcome: Err(ControlError::InternalError),
        };
        assert_ne!(r.request_id, wrong.request_id);
        framing::write_frame(&mut b, &wrong.encode().unwrap(), end, &stop).unwrap();
    });
    let r = request(Operation::Status(Empty {}));
    assert!(matches!(
        client::exchange(&mut a, &r, &AtomicBool::new(false)),
        Err(ControlError::CorrelationError)
    ));
    worker.join().unwrap();
}
#[test]
fn private_files_reject_symlinks_hardlinks_and_public_modes() {
    let t = Temp::new();
    let original = t.root.join("original");
    paths::private_file(&original).unwrap();
    let link = t.root.join("link");
    std::os::unix::fs::symlink(&original, &link).unwrap();
    assert!(paths::private_file(&link).is_err());
    fs::remove_file(&link).unwrap();
    fs::hard_link(&original, &link).unwrap();
    assert!(paths::private_file(&original).is_err());
    fs::remove_file(link).unwrap();
    fs::set_permissions(&original, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(paths::private_file(&original).is_err());
}
#[test]
fn symlinked_runtime_root_and_product_reject() {
    let t = Temp::new();
    let link = t.root.join("link");
    std::os::unix::fs::symlink(&t.root, &link).unwrap();
    assert!(paths::runtime_product(&link, true).is_err());
    std::os::unix::fs::symlink(&t.root, t.root.join("wyss-console")).unwrap();
    assert!(paths::runtime_product(&t.root, true).is_err());
}
#[test]
fn oversized_socket_path_is_explicitly_unsafe() {
    let path = std::path::Path::new("/a").join("x".repeat(120));
    assert!(matches!(server::bind(&path), Err(ControlError::UnsafePath)));
}
#[test]
fn execution_parameters_are_not_accepted() {
    for name in [
        "model",
        "cwd",
        "executable",
        "argv",
        "permissions",
        "environment",
        "tools",
        "prompt",
    ] {
        let mut value: Value =
            serde_json::from_slice(&request(prepare()).encode().unwrap()).unwrap();
        value["params"][name] = json!("not allowed");
        assert!(Request::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}

fn runtime_mutation() -> RuntimeMutation {
    RuntimeMutation {
        intent_id: uuid(),
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: RuntimeTarget::codex().runtime_kind,
        expected_revision: None,
        safety_profile: None,
        workspace_root: None,
    }
}
fn runtime_record(value: ResultValue) -> RuntimeControlRecord {
    match value {
        ResultValue::RuntimeControl(r) => r,
        _ => panic!("runtime receipt"),
    }
}
#[test]
fn runtime_desired_state_idempotency_conflict_reopen_and_session_isolation() {
    let t = Temp::new();
    let mut s = t.store();
    let prepared = session(s.apply(&prepare()).unwrap());
    assert!(s.runtime_control().unwrap().is_none());
    let start = Operation::RuntimeStart(runtime_mutation());
    let first = runtime_record(s.apply(&start).unwrap());
    assert_eq!(first.desired_state, DesiredState::Running);
    assert_eq!(first.revision, 1);
    assert_eq!(runtime_record(s.apply(&start).unwrap()), first);
    let Operation::RuntimeStart(p) = &start else {
        panic!()
    };
    assert!(matches!(
        s.apply(&Operation::RuntimeStop(p.clone())),
        Err(ControlError::IntentConflict)
    ));
    let stop = Operation::RuntimeStop(runtime_mutation());
    let stopped = runtime_record(s.apply(&stop).unwrap());
    assert_eq!(stopped.revision, 2);
    assert_eq!(stopped.desired_state, DesiredState::Stopped);
    assert_eq!(runtime_record(s.apply(&stop).unwrap()), stopped);
    drop(s);
    let mut s = t.store();
    assert_eq!(runtime_record(s.apply(&start).unwrap()), first);
    assert_eq!(s.runtime_control().unwrap(), Some(stopped));
    let restarted = runtime_record(
        s.apply(&Operation::RuntimeStart(runtime_mutation()))
            .unwrap(),
    );
    assert_eq!(restarted.revision, 3);
    // A deliberate start while already running is one new revision/reconciliation opportunity.
    assert_eq!(
        runtime_record(
            s.apply(&Operation::RuntimeStart(runtime_mutation()))
                .unwrap()
        )
        .revision,
        4
    );
    assert_eq!(
        session(
            s.apply(&Operation::Get(Get {
                managed_session_id: prepared.managed_session_id.clone()
            }))
            .unwrap()
        ),
        prepared
    );
    let all = events(&mut s);
    assert_eq!(all.len(), 5);
    for (index, event) in all.iter().enumerate() {
        let v = serde_json::to_value(event).unwrap();
        assert_eq!(event.sequence().get(), index as u64 + 1);
        if index > 0 {
            assert!(v["session_id"].is_null());
            assert_eq!(v["event_type"], "authority.runtime.desired_state_changed");
            assert!(!v["correlation_id"].is_null());
            assert_eq!(v["payload"].as_object().unwrap().len(), 4);
        }
    }
}
#[test]
fn runtime_unsupported_stale_and_audit_failure_have_zero_side_effects() {
    let t = Temp::new();
    let mut s = t.store();
    let mut p = runtime_mutation();
    p.runtime_kind = AgentRuntimeKind::GenericCli;
    assert!(matches!(
        s.apply(&Operation::RuntimeStart(p)),
        Err(ControlError::Unsupported)
    ));
    let mut p = runtime_mutation();
    p.expected_revision = Some(1);
    assert!(matches!(
        s.apply(&Operation::RuntimeStart(p)),
        Err(ControlError::StaleRevision)
    ));
    s.conn.execute_batch("CREATE TRIGGER reject_runtime_audit BEFORE INSERT ON control_events BEGIN SELECT RAISE(ABORT,'injected_io_failure'); END").unwrap();
    assert!(matches!(
        s.apply(&Operation::RuntimeStart(runtime_mutation())),
        Err(ControlError::StorageUnavailable)
    ));
    assert!(s.runtime_control().unwrap().is_none());
    assert_eq!(s.status().unwrap().last_sequence, None);
    assert_eq!(
        s.conn
            .query_row("SELECT count(*) FROM processed_intents", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
fn v1_fixture(t: &Temp) -> (PathBuf, Vec<(String, String, String)>, ManagedSessionRecord) {
    let mut s = t.store();
    let op = prepare();
    let a = session(s.apply(&op).unwrap());
    s.apply(&close(&a)).unwrap();
    let mut q = s
        .conn
        .prepare("SELECT event_id,sequence,envelope FROM control_events ORDER BY sequence")
        .unwrap();
    let original = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let path = t.root.join("v1.sqlite3");
    paths::private_file(&path).unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    for (_, sql) in schema::V1_OBJECTS {
        c.execute_batch(sql).unwrap();
    }
    c.execute(
        "ATTACH DATABASE ?1 AS source",
        [t.root.join("control.sqlite3").to_str().unwrap()],
    )
    .unwrap();
    c.execute_batch("INSERT INTO authority_metadata SELECT id,1,stream_id,last_sequence FROM source.authority_metadata;
        INSERT INTO managed_sessions SELECT * FROM source.managed_sessions;
        INSERT INTO control_events SELECT * FROM source.control_events;
        INSERT INTO processed_intents SELECT * FROM source.processed_intents;
        PRAGMA user_version=1;").unwrap();
    (path, original, a)
}
#[test]
fn real_v1_migration_preserves_bytes_sessions_receipts_stream_and_head() {
    let t = Temp::new();
    let (path, original, prepared) = v1_fixture(&t);
    let old = t.store().status().unwrap();
    let mut s = Store::open(&path, uuid()).unwrap();
    assert_eq!(s.status().unwrap().storage_schema_version, 7);
    assert_eq!(s.status().unwrap().stream_id, old.stream_id);
    assert_eq!(s.status().unwrap().last_sequence, old.last_sequence);
    let saved: Vec<(String, String, String)> = s
        .conn
        .prepare("SELECT event_id,sequence,envelope FROM control_events ORDER BY sequence")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(saved, original);
    assert!(s.runtime_control().unwrap().is_none());
    let raw: String = s
        .conn
        .query_row(
            "SELECT mutation FROM processed_intents WHERE mutation LIKE '%managedSession/prepare%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let op: Operation = serde_json::from_str(&raw).unwrap();
    assert_eq!(session(s.apply(&op).unwrap()), prepared);
    assert_eq!(
        session(
            s.apply(&Operation::Get(Get {
                managed_session_id: prepared.managed_session_id
            }))
            .unwrap()
        )
        .control_state,
        ControlState::Closed
    );
    assert_eq!(
        s.conn
            .query_row("SELECT count(*) FROM processed_intents", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(s);
    assert_eq!(
        Store::open(&path, uuid())
            .unwrap()
            .status()
            .unwrap()
            .last_sequence,
        old.last_sequence
    );
}
#[test]
fn v1_migration_failure_rolls_back_all_ddl_and_version() {
    let t = Temp::new();
    let (path, _, _) = v1_fixture(&t);
    let mut c = rusqlite::Connection::open(path).unwrap();
    let snapshot = |c: &rusqlite::Connection| {
        c.query_row(
            "SELECT group_concat(sql,';') FROM sqlite_master WHERE sql IS NOT NULL",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };
    let before = snapshot(&c);
    assert!(schema::fail_migration(&mut c).is_err());
    assert_eq!(snapshot(&c), before);
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        1
    );
    schema::migrate(&mut c).unwrap();
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        7
    );
}
#[test]
fn corrupt_v1_schema_is_rejected_without_partial_upgrade() {
    let t = Temp::new();
    let (path, _, _) = v1_fixture(&t);
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("DROP TRIGGER control_events_no_update")
        .unwrap();
    drop(c);
    assert!(matches!(
        Store::open(&path, uuid()),
        Err(ControlError::StorageIncompatible)
    ));
    let c = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        1
    );
}
#[test]
fn runtime_control_rejects_conversation_inputs_and_unbounded_pages() {
    for method in [
        "runtime/start",
        "runtime/stop",
        "runtime/status",
        "runtime/model/list",
    ] {
        let op = match method {
            "runtime/start" => Operation::RuntimeStart(runtime_mutation()),
            "runtime/stop" => Operation::RuntimeStop(runtime_mutation()),
            "runtime/status" => Operation::RuntimeStatus(RuntimeTarget::codex()),
            _ => Operation::RuntimeModels(RuntimeModels {
                adapter_id: RuntimeTarget::codex().adapter_id,
                runtime_kind: RuntimeTarget::codex().runtime_kind,
                cursor: None,
                limit: 20,
            }),
        };
        let raw = request(op).encode().unwrap();
        assert!(Request::parse(&raw).is_ok());
        let mut v: Value = serde_json::from_slice(&raw).unwrap();
        v["params"]["prompt"] = json!("forbidden");
        assert!(Request::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    let mut p = RuntimeModels {
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: RuntimeTarget::codex().runtime_kind,
        cursor: None,
        limit: 21,
    };
    assert!(Operation::RuntimeModels(p.clone()).validate().is_err());
    p.limit = 0;
    assert!(Operation::RuntimeModels(p).validate().is_err());
}

#[test]
fn runtime_lifecycle_audit_is_minimal_monotonic_and_reopens() {
    let t = Temp::new();
    let mut s = t.store();
    let id = crate::contracts::identities::AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
    s.apply(&Operation::RuntimeStart(runtime_mutation()))
        .unwrap();
    let ready = json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":id,"runtime_version":"0.159.0","compatibility":"compatible"});
    s.runtime_event("authority.runtime.instance_ready", ready.clone())
        .unwrap();
    let mut unsafe_payload = ready;
    unsafe_payload["stderr"] = json!("synthetic private diagnostic");
    assert!(
        s.runtime_event("authority.runtime.instance_ready", unsafe_payload)
            .is_err()
    );
    s.runtime_event("authority.runtime.instance_exited",json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":id,"exit_kind":"protocol_failed","expected":false,"desired_state":"running"})).unwrap();
    let all = events(&mut s);
    assert_eq!(all.len(), 3);
    for (index, event) in all.iter().enumerate() {
        let v = serde_json::to_value(event).unwrap();
        assert_eq!(event.sequence().get(), index as u64 + 1);
        assert!(v["session_id"].is_null());
        assert_eq!(v["persistence"], "durable");
        assert_eq!(v["sensitivity"], "internal");
        if index > 0 {
            assert!(v["correlation_id"].is_null());
        }
        for field in [
            "pid",
            "models",
            "stderr",
            "stdout",
            "params",
            "environment",
            "prompt",
        ] {
            assert!(v["payload"].get(field).is_none());
        }
    }
    drop(s);
    let mut s = t.store();
    assert_eq!(events(&mut s).len(), 3);
    assert_eq!(
        s.runtime_control().unwrap().unwrap().desired_state,
        DesiredState::Running
    );
}

#[test]
fn storage_witness_is_a_strict_monotonic_lower_bound_of_one_stream() {
    let t = Temp::new();
    let db = t.root.join("control.sqlite3");
    let witness = t.root.join("control.anchor");
    // Nothing was ever here: the database may be created.
    assert_eq!(anchor::admit(&db), Ok(None));
    let mut s = t.store();
    let mut a = anchor::Anchor::confirm(&db, None, &s).unwrap();
    let read = |path: &std::path::Path| -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    };
    let stream = s.status().unwrap().stream_id;
    assert_eq!(
        read(&witness),
        json!({"schema_version": 1, "stream_id": stream, "head": "0"})
    );
    assert_eq!(fs::metadata(&witness).unwrap().mode() & 0o777, 0o600);
    for _ in 0..3 {
        s.apply(&prepare()).unwrap();
    }
    a.advance(s.head().unwrap()).unwrap();
    assert_eq!(read(&witness)["head"], "3");
    // It never moves back, and an unchanged head writes nothing.
    let inode = fs::metadata(&witness).unwrap().ino();
    a.advance(2).unwrap();
    a.advance(3).unwrap();
    assert_eq!(read(&witness)["head"], "3");
    assert_eq!(fs::metadata(&witness).unwrap().ino(), inode);
    drop(a);
    // The same store is admitted again; a store behind its witness, or of another stream,
    // is not.
    let witnessed = anchor::admit(&db).unwrap();
    assert!(witnessed.is_some());
    assert!(anchor::Anchor::confirm(&db, witnessed.clone(), &s).is_ok());
    let other = Temp::new();
    let mut o = other.store();
    assert_eq!(
        anchor::Anchor::confirm(&db, witnessed.clone(), &o).err(),
        Some(ControlError::StorageIncompatible)
    );
    for _ in 0..5 {
        o.apply(&prepare()).unwrap();
    }
    assert_eq!(
        anchor::Anchor::confirm(&db, witnessed, &o).err(),
        Some(ControlError::StorageIncompatible)
    );
    // Every witness that is not exactly one it wrote is refused, never taken for none.
    let good = fs::read_to_string(&witness).unwrap();
    for bad in [
        String::new(),
        "{".to_owned(),
        "[]".to_owned(),
        good.replace("\"head\":\"3\"", "\"head\":3"),
        good.replace("\"head\":\"3\"", "\"head\":\"03\""),
        good.replace("\"head\":\"3\"", "\"head\":\"-1\""),
        good.replace("\"schema_version\":1", "\"schema_version\":2"),
        good.replace('}', ",\"extra\":1}"),
        good.replace("\"head\":\"3\"", "\"head\":\"3\",\"head\":\"1\""),
        good.replace(stream.as_str(), "not-a-stream"),
        format!("{good}{}", " ".repeat(300)),
    ] {
        fs::write(&witness, &bad).unwrap();
        assert_eq!(
            anchor::admit(&db),
            Err(ControlError::StorageIncompatible),
            "{bad}"
        );
    }
    fs::write(&witness, &good).unwrap();
    assert!(anchor::admit(&db).unwrap().is_some());
    // With a witness, or with any file SQLite leaves next to a database, a missing or
    // empty database is never created.
    drop(s);
    let moved = t.root.join("moved.sqlite3");
    fs::rename(&db, &moved).unwrap();
    assert_eq!(anchor::admit(&db), Err(ControlError::StorageIncompatible));
    fs::remove_file(&witness).unwrap();
    for sidecar in [
        "control.sqlite3-wal",
        "control.sqlite3-shm",
        "control.sqlite3-journal",
    ] {
        let _ = fs::remove_file(t.root.join(sidecar));
    }
    assert_eq!(anchor::admit(&db), Ok(None));
    for sidecar in [
        "control.sqlite3-wal",
        "control.sqlite3-shm",
        "control.sqlite3-journal",
    ] {
        fs::write(t.root.join(sidecar), b"left behind").unwrap();
        assert_eq!(
            anchor::admit(&db),
            Err(ControlError::StorageIncompatible),
            "{sidecar}"
        );
        fs::remove_file(t.root.join(sidecar)).unwrap();
    }
    fs::write(&db, b"").unwrap();
    assert_eq!(anchor::admit(&db), Ok(None));
    fs::write(t.root.join("control.sqlite3-wal"), b"left behind").unwrap();
    assert_eq!(anchor::admit(&db), Err(ControlError::StorageIncompatible));
}

#[test]
fn a_head_is_witnessed_only_once_its_replacement_reached_the_directory() {
    let t = Temp::new();
    let db = t.root.join("control.sqlite3");
    let witness = t.root.join("control.anchor");
    let staging = t.root.join("control.anchor.tmp");
    let head = |path: &std::path::Path| -> String {
        let v: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        v["head"].as_str().unwrap().to_owned()
    };
    let inode = |path: &std::path::Path| fs::metadata(path).unwrap().ino();
    let mut s = t.store();
    // The first witness. Something that is not a staging file stands where one goes: it is
    // left alone, no witness appears and there is no anchor to serve on.
    fs::create_dir(&staging).unwrap();
    assert_eq!(
        anchor::Anchor::confirm(&db, None, &s).err(),
        Some(ControlError::UnsafePath)
    );
    assert!(!witness.exists() && staging.is_dir());
    fs::remove_dir(&staging).unwrap();
    // Nor is it made in a directory that is not the private one, or reached through a link.
    fs::set_permissions(&t.root, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        anchor::Anchor::confirm(&db, None, &s).err(),
        Some(ControlError::UnsafePath)
    );
    fs::set_permissions(&t.root, fs::Permissions::from_mode(0o700)).unwrap();
    let elsewhere = Temp::new();
    let link = elsewhere.root.join("state");
    std::os::unix::fs::symlink(&t.root, &link).unwrap();
    assert_eq!(
        anchor::Anchor::confirm(&link.join("control.sqlite3"), None, &s).err(),
        Some(ControlError::UnsafePath)
    );
    assert!(!witness.exists());
    let mut a = anchor::Anchor::confirm(&db, None, &s).unwrap();
    assert_eq!(head(&witness), "0");
    assert_eq!(fs::metadata(&witness).unwrap().mode() & 0o777, 0o600);
    assert!(!staging.exists());
    for _ in 0..4 {
        s.apply(&prepare()).unwrap();
    }
    // A replacement is a new file under the same name, and leaves nothing else behind.
    let first = inode(&witness);
    a.advance(1).unwrap();
    assert_eq!(head(&witness), "1");
    assert_ne!(inode(&witness), first);
    assert!(!staging.exists());
    // A failure before the rename keeps the previous witness, byte for byte and the same
    // file, and whatever was in the way: only a staging file is ever removed.
    let kept = (fs::read(&witness).unwrap(), inode(&witness));
    let target = t.root.join("unrelated");
    fs::write(&target, b"unrelated").unwrap();
    type Obstacle = fn(&std::path::Path, &std::path::Path);
    let obstacles: [(&str, Obstacle); 4] = [
        ("a directory", |staging, _| fs::create_dir(staging).unwrap()),
        ("a link", |staging, target| {
            std::os::unix::fs::symlink(target, staging).unwrap()
        }),
        ("a file readable by others", |staging, _| {
            fs::write(staging, b"{").unwrap();
            fs::set_permissions(staging, fs::Permissions::from_mode(0o644)).unwrap();
        }),
        ("a file with a second name", |staging, target| {
            fs::set_permissions(target, fs::Permissions::from_mode(0o600)).unwrap();
            fs::hard_link(target, staging).unwrap();
        }),
    ];
    for (name, obstacle) in obstacles {
        obstacle(&staging, &target);
        let found = fs::symlink_metadata(&staging).unwrap();
        assert_eq!(a.advance(2), Err(ControlError::UnsafePath), "{name}");
        assert_eq!(
            (fs::read(&witness).unwrap(), inode(&witness)),
            kept,
            "{name}"
        );
        let left = fs::symlink_metadata(&staging).unwrap();
        assert_eq!(
            (left.ino(), left.mode(), left.nlink()),
            (found.ino(), found.mode(), found.nlink()),
            "{name}"
        );
        assert_eq!(fs::read(&target).unwrap(), b"unrelated", "{name}");
        if found.is_dir() {
            fs::remove_dir(&staging).unwrap();
        } else {
            fs::remove_file(&staging).unwrap();
        }
    }
    // The staging file of an interrupted replacement is exactly what is removed. The head
    // the failures did not record is recorded now.
    fs::write(&staging, b"{\"schema_version\":1,\"stream_id\":\"").unwrap();
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o600)).unwrap();
    a.advance(2).unwrap();
    assert_eq!(head(&witness), "2");
    assert!(!staging.exists());
    // The directory is synced last, once, and it is the state directory: by then the new
    // witness is whole under its name and the staging name is gone.
    let state = fs::metadata(&t.root).unwrap();
    let mut seen = Vec::new();
    a.advance_with(3, |dir| {
        let m = dir.metadata()?;
        seen.push((
            (m.dev(), m.ino()),
            m.is_dir(),
            head(&witness),
            staging.exists(),
        ));
        dir.sync_all()
    })
    .unwrap();
    assert_eq!(
        seen,
        [((state.dev(), state.ino()), true, "3".to_owned(), false)]
    );
    // A head that is already witnessed touches nothing, the directory included.
    a.advance_with(3, |_| panic!("nothing was replaced"))
        .unwrap();
    // When the directory cannot be synced the rename has happened, and the head is still
    // not witnessed: the next attempt replaces the witness again, in full.
    let before = inode(&witness);
    assert_eq!(
        a.advance_with(4, |_| Err(std::io::Error::other("injected"))),
        Err(ControlError::StorageUnavailable)
    );
    let renamed = inode(&witness);
    assert_ne!(renamed, before);
    assert_eq!(head(&witness), "4");
    a.advance(4).unwrap();
    let durable = inode(&witness);
    assert_ne!(durable, renamed);
    assert_eq!(head(&witness), "4");
    a.advance(4).unwrap();
    assert_eq!(inode(&witness), durable);
    drop(a);
    // A restart on a valid witness of the same head replaces nothing; one on a database
    // that went further than its witness records where it is before anything else.
    let witnessed = anchor::admit(&db).unwrap();
    assert!(witnessed.is_some());
    drop(anchor::Anchor::confirm(&db, witnessed, &s).unwrap());
    assert_eq!(inode(&witness), durable);
    s.apply(&prepare()).unwrap();
    let witnessed = anchor::admit(&db).unwrap();
    drop(anchor::Anchor::confirm(&db, witnessed, &s).unwrap());
    assert_eq!(head(&witness), "5");
    assert!(!staging.exists());
}

#[test]
fn a_page_of_model_offers_matches_its_models_decodes_from_older_authorities_and_refuses_the_unknown()
 {
    use crate::contracts::control::ModelOffer;
    let id = crate::contracts::identities::AgentRuntimeInstanceId::parse(
        "22222222-2222-4222-8222-222222222222",
    )
    .unwrap();
    let model = |name: &str| {
        crate::contracts::agent_adapter::AgentModelDescriptorV1::from_value(json!({
            "schema_version": 1, "provider_id": null, "model_id": name,
            "display_name": null, "availability": "unknown"
        }))
        .unwrap()
    };
    let offer = || ModelOffer {
        is_default: true,
        default_effort: Some("medium".into()),
        efforts: vec!["low".into(), "medium".into()],
        replaced_by: None,
        retires_at: None,
    };
    let encoded = |models: Vec<_>, offers: Vec<ModelOffer>| {
        Response {
            request_id: uuid(),
            authority_instance_id: uuid(),
            outcome: Ok(ResultValue::RuntimeModels {
                runtime_instance_id: id.clone(),
                models,
                offers,
                complete: Some(true),
                next: None,
            }),
        }
        .encode()
        .and_then(|bytes| Response::parse(&bytes))
    };
    assert!(encoded(vec![model("a")], vec![offer()]).is_ok());
    assert!(encoded(vec![model("a")], vec![]).is_ok());
    assert!(encoded(vec![model("a"), model("b")], vec![offer()]).is_err());
    let mut bad = offer();
    bad.efforts = vec!["a\nb".into()];
    assert!(encoded(vec![model("a")], vec![bad]).is_err());
    // What an earlier authority sent still decodes: no offers, completeness not reported.
    let old = json!({"kind":"runtime_models","value":{
        "runtime_instance_id":"22222222-2222-4222-8222-222222222222","models":[],"next":null}});
    assert!(matches!(
        serde_json::from_value::<ResultValue>(old).unwrap(),
        ResultValue::RuntimeModels { complete: None, ref offers, .. } if offers.is_empty()
    ));
    let unknown = json!({"kind":"runtime_models","value":{
        "runtime_instance_id":"22222222-2222-4222-8222-222222222222","models":[],"next":null,"x":1}});
    assert!(serde_json::from_value::<ResultValue>(unknown).is_err());
}
