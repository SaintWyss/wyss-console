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
    s.conn.pragma_update(None, "user_version", 2).unwrap();
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
            "authority/hello",
            "authority/status",
            "managedSession/prepare",
            "managedSession/get",
            "managedSession/list",
            "managedSession/close",
            "audit/read"
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
        client::exchange(&mut a, &r),
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
