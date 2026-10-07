use super::*;
use std::{fs, os::unix::fs::DirBuilderExt};
struct Fixture {
    root: std::path::PathBuf,
    store: Store,
    session: ManagedSessionRecord,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-bind-{}", uuid().as_str()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let mut store = Store::open(&root.join("db"), uuid()).unwrap();
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
        Self {
            root,
            store,
            session,
        }
    }
    fn create(&self) -> Operation {
        Operation::BindingCreate(BindingCreate {
            intent_id: uuid(),
            managed_session_id: self.session.managed_session_id.clone(),
            expected_session_revision: 1,
            workspace_root: self.root.to_str().unwrap().to_owned(),
        })
    }
    fn reserve(&mut self, op: &Operation) -> Reservation {
        self.store.reserve_binding(op).unwrap()
    }
    fn materialize(&mut self, r: &Reservation, id: &str) {
        let id = CodexThreadId::parse(id).unwrap();
        for stage in [
            super::super::super::runtime::MaterializationStage::Started,
            super::super::super::runtime::MaterializationStage::Archived,
            super::super::super::runtime::MaterializationStage::Unarchived,
        ] {
            self.store.checkpoint_binding(r, &id, stage).unwrap();
        }
    }
    fn runtime() -> AgentRuntimeInstanceId {
        AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap()
    }
    fn bind(&mut self) -> (Operation, SessionBinding) {
        let op = self.create();
        let r = self.reserve(&op);
        self.store.dispatch_binding(&r).unwrap();
        self.materialize(&r, "opaque:conversation-A");
        let b = self
            .store
            .commit_binding(
                &r,
                &CodexThreadId::parse("opaque:conversation-A").unwrap(),
                &Self::runtime(),
            )
            .unwrap();
        (op, b)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn durable_identity_revision_receipt_and_independent_session() {
    let mut f = Fixture::new();
    let (op, b) = f.bind();
    assert_eq!(b.binding_revision, 1);
    assert_ne!(b.runtime_session_id.as_str(), b.managed_session_id.as_str());
    assert!(matches!(f.store.binding_receipt(&op).unwrap(),Some(ResultValue::Binding(x)) if x==b));
    let mut conflict = op.clone();
    if let Operation::BindingCreate(p) = &mut conflict {
        p.workspace_root.push_str("/other")
    };
    assert!(matches!(
        f.store.binding_receipt(&conflict),
        Err(ControlError::IntentConflict)
    ));
    assert!(matches!(
        f.store.reserve_binding(&f.create()),
        Err(ControlError::BindingAlreadyExists)
    ));
    assert_eq!(
        f.store.gate_session(&f.session.managed_session_id).unwrap(),
        f.session
    );
    let s = Store::open(&f.root.join("db"), uuid()).unwrap();
    assert_eq!(s.binding(&b.managed_session_id).unwrap(), Some(b));
    assert_eq!(
        s.binding_status(&f.session.managed_session_id)
            .unwrap()
            .state,
        BindingState::BoundDetached
    );
}
#[test]
fn uppercase_session_input_returns_canonical_binding_and_status() {
    let mut f = Fixture::new();
    let upper =
        ManagedSessionId::parse(&f.session.managed_session_id.as_str().to_ascii_uppercase())
            .unwrap();
    let mut op = f.create();
    if let Operation::BindingCreate(p) = &mut op {
        p.managed_session_id = upper.clone();
    }
    let r = f.reserve(&op);
    f.store.dispatch_binding(&r).unwrap();
    f.materialize(&r, "opaque:uppercase-input");
    let b = f
        .store
        .commit_binding(
            &r,
            &CodexThreadId::parse("opaque:uppercase-input").unwrap(),
            &Fixture::runtime(),
        )
        .unwrap();
    assert_eq!(b.managed_session_id, f.session.managed_session_id);
    let status = f.store.binding_status(&upper).unwrap();
    status.validate().unwrap();
    assert_eq!(status.managed_session_id, f.session.managed_session_id);
    assert_eq!(status.binding, Some(b));
}
#[test]
fn workspace_absolute_directory_canonical_and_bounded() {
    let f = Fixture::new();
    for p in ["relative", "/does-not-exist", "/dev/null"] {
        assert!(canonical_workspace(p).is_err())
    }
    let link = f.root.join("link");
    std::os::unix::fs::symlink(&f.root, &link).unwrap();
    assert_eq!(
        canonical_workspace(link.to_str().unwrap()).unwrap(),
        f.root.to_str().unwrap()
    );
    assert!(validate_workspace_text(&format!("/{}", "a".repeat(256))).is_err());
}
#[test]
fn startup_reserved_is_failed_dispatching_is_indeterminate_and_never_redispatched() {
    for dispatch in [false, true] {
        let mut f = Fixture::new();
        let op = f.create();
        let r = f.reserve(&op);
        if dispatch {
            f.store.dispatch_binding(&r).unwrap()
        }
        let mut s = Store::open(&f.root.join("db"), uuid()).unwrap();
        s.recover_binding_operations().unwrap();
        assert!(matches!(
            s.binding_receipt(&op),
            Err(ControlError::BindingIndeterminate | ControlError::InvalidTransition)
        ));
        if dispatch {
            assert_eq!(
                s.binding_status(&f.session.managed_session_id)
                    .unwrap()
                    .state,
                BindingState::Indeterminate
            );
            assert!(s.reserve_binding(&f.create()).is_err());
        }
        assert!(s.binding(&f.session.managed_session_id).unwrap().is_none());
    }
}
#[test]
fn explicit_abandon_is_audited_idempotent_and_does_not_claim_provider_deletion() {
    let mut f = Fixture::new();
    let op = f.create();
    let r = f.reserve(&op);
    f.store.dispatch_binding(&r).unwrap();
    f.store.fail_binding(&r, true).unwrap();
    let p = BindingAbandon {
        intent_id: uuid(),
        managed_session_id: f.session.managed_session_id.clone(),
        operation_id: r.operation_id,
    };
    f.store.abandon_binding(&p).unwrap();
    let n = f.store.status().unwrap().last_sequence;
    f.store.abandon_binding(&p).unwrap();
    assert_eq!(f.store.status().unwrap().last_sequence, n);
    assert!(f.store.reserve_binding(&f.create()).is_ok());
    let events: String = f
        .store
        .conn
        .query_row(
            "SELECT group_concat(envelope) FROM control_events",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(events.contains("runtime_operation_abandoned"));
    assert!(!events.contains(f.root.to_str().unwrap()));
}
#[test]
fn audit_failure_rolls_back_binding_receipt_and_success_marker() {
    let mut f = Fixture::new();
    let op = f.create();
    let r = f.reserve(&op);
    f.store.dispatch_binding(&r).unwrap();
    f.materialize(&r, "empty-provider-orphan");
    let n = f.store.status().unwrap().last_sequence;
    f.store.conn.execute_batch("CREATE TEMP TRIGGER injected BEFORE INSERT ON control_events BEGIN SELECT RAISE(ABORT,'injected'); END").unwrap();
    assert!(
        f.store
            .commit_binding(
                &r,
                &CodexThreadId::parse("empty-provider-orphan").unwrap(),
                &Fixture::runtime()
            )
            .is_err()
    );
    assert!(
        f.store
            .binding(&f.session.managed_session_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.store.status().unwrap().last_sequence, n);
    f.store.fail_binding(&r, true).unwrap();
    assert!(matches!(
        f.store.binding_receipt(&op),
        Err(ControlError::BindingIndeterminate)
    ));
}
#[test]
fn resume_uses_immutable_binding_and_preserves_binding_revision() {
    let mut f = Fixture::new();
    let (_, b) = f.bind();
    let op = Operation::BindingResume(BindingResume {
        intent_id: uuid(),
        managed_session_id: b.managed_session_id.clone(),
        expected_session_revision: 1,
    });
    let r = f.reserve(&op);
    assert_eq!(r.workspace, b.workspace_root);
    assert_eq!(r.thread_id, Some(b.runtime_session_id.clone()));
    f.store.dispatch_binding(&r).unwrap();
    let resumed = f
        .store
        .commit_binding(&r, &b.runtime_session_id, &Fixture::runtime())
        .unwrap();
    assert_eq!(resumed, b);
    let mut bad = f.create();
    if let Operation::BindingCreate(p) = &mut bad {
        p.expected_session_revision = 2
    };
    assert!(matches!(
        f.store.reserve_binding(&bad),
        Err(ControlError::StaleRevision)
    ));
    f.store
        .apply(&Operation::Close(Close {
            intent_id: uuid(),
            managed_session_id: b.managed_session_id.clone(),
            expected_revision: 1,
        }))
        .unwrap();
    assert_eq!(f.store.binding(&b.managed_session_id).unwrap(), Some(b));
    assert!(matches!(
        f.store.reserve_binding(&op),
        Err(ControlError::StaleRevision)
    ));
}
#[test]
fn journal_intents_cannot_be_reused_by_other_mutators() {
    let mut f = Fixture::new();
    let op = f.create();
    let r = f.reserve(&op);
    assert!(matches!(
        f.store.apply(&Operation::Prepare(Prepare {
            intent_id: r.operation_id,
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind
        })),
        Err(ControlError::IntentConflict)
    ));
}
fn snapshot(c: &Connection) -> Vec<Vec<String>> {
    [
        "managed_sessions",
        "managed_runtimes",
        "execution_leases",
        "processed_intents",
        "control_events",
    ]
    .into_iter()
    .map(|table| {
        let mut q = c
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let count = q.column_count();
        q.query_map([], |r| {
            let values = (0..count)
                .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                .collect::<Vec<_>>();
            Ok(values.join("|"))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    })
    .collect()
}
#[test]
fn real_v3_upgrade_preserves_all_data_event_bytes_stream_head_and_zero_bindings() {
    let mut f = Fixture::new();
    f.store
        .apply(&Operation::RuntimeStart(RuntimeMutation {
            intent_id: uuid(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
            expected_revision: None,
        }))
        .unwrap();
    f.store
        .apply(&Operation::LeaseGrant(LeaseGrant {
            intent_id: uuid(),
            session_id: f.session.managed_session_id.clone(),
            expected_session_revision: 1,
            scopes: vec![ExecutionScope::SessionResume],
            ttl_seconds: 3600,
        }))
        .unwrap();
    let path = f.root.join("v3");
    paths::private_file(&path).unwrap();
    let mut c = Connection::open(&path).unwrap();
    for (_, sql) in schema::V3_OBJECTS {
        c.execute_batch(sql).unwrap()
    }
    c.execute(
        "ATTACH DATABASE ?1 AS source",
        [f.root.join("db").to_str().unwrap()],
    )
    .unwrap();
    c.execute_batch("INSERT INTO authority_metadata SELECT id,3,stream_id,last_sequence FROM source.authority_metadata; INSERT INTO managed_sessions SELECT * FROM source.managed_sessions; INSERT INTO managed_runtimes SELECT * FROM source.managed_runtimes; INSERT INTO execution_leases SELECT * FROM source.execution_leases; INSERT INTO processed_intents SELECT * FROM source.processed_intents; INSERT INTO control_events SELECT * FROM source.control_events; PRAGMA user_version=3;").unwrap();
    let before = snapshot(&c);
    let meta = c
        .query_row(
            "SELECT stream_id,last_sequence FROM authority_metadata",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .unwrap();
    assert!(schema::fail_migration(&mut c).is_err());
    assert_eq!(snapshot(&c), before);
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        3
    );
    schema::migrate(&mut c).unwrap();
    assert_eq!(snapshot(&c), before);
    assert_eq!(
        c.query_row(
            "SELECT stream_id,last_sequence FROM authority_metadata",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        meta
    );
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM managed_session_bindings", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
    drop(c);
    assert_eq!(
        Store::open(&path, uuid())
            .unwrap()
            .status()
            .unwrap()
            .storage_schema_version,
        5
    );
}
#[test]
fn strict_binding_protocol_forbids_overrides_and_turn_input() {
    let f = Fixture::new();
    let op = f.create();
    let request = Request {
        request_id: uuid(),
        operation: op,
    };
    let bytes = request.encode().unwrap();
    for field in [
        "prompt",
        "input",
        "thread_id",
        "runtime_instance_id",
        "model",
        "sandbox",
        "approvalPolicy",
    ] {
        let mut v: Value = serde_json::from_slice(&bytes).unwrap();
        v["params"][field] = json!("forbidden");
        assert!(Request::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    }
}
#[test]
fn uncertain_resume_allows_new_explicit_intent_without_changing_binding() {
    let mut f = Fixture::new();
    let (_, b) = f.bind();
    let op = Operation::BindingResume(BindingResume {
        intent_id: uuid(),
        managed_session_id: b.managed_session_id.clone(),
        expected_session_revision: 1,
    });
    let r = f.reserve(&op);
    f.store.dispatch_binding(&r).unwrap();
    f.store.fail_binding(&r, true).unwrap();
    assert!(matches!(
        f.store.binding_receipt(&op),
        Err(ControlError::InvalidTransition)
    ));
    let next = Operation::BindingResume(BindingResume {
        intent_id: uuid(),
        managed_session_id: b.managed_session_id.clone(),
        expected_session_revision: 1,
    });
    assert!(f.store.reserve_binding(&next).is_ok());
    assert_eq!(f.store.binding(&b.managed_session_id).unwrap(), Some(b));
}

#[test]
fn durable_materialization_checkpoints_are_required_ordered_and_identity_exact() {
    use super::super::super::runtime::MaterializationStage::*;
    let mut f = Fixture::new();
    let op = f.create();
    let r = f.reserve(&op);
    let id = CodexThreadId::parse("journal-created-thread").unwrap();
    let other = CodexThreadId::parse("substitution").unwrap();
    f.store.dispatch_binding(&r).unwrap();
    for stage in [Started, Archived, Unarchived] {
        assert!(
            f.store
                .commit_binding(&r, &id, &Fixture::runtime())
                .is_err()
        );
        if !matches!(stage, Started) {
            assert!(f.store.checkpoint_binding(&r, &other, stage).is_err());
        }
        f.store.checkpoint_binding(&r, &id, stage).unwrap();
        assert!(f.store.checkpoint_binding(&r, &id, stage).is_err());
    }
    let b = f
        .store
        .commit_binding(&r, &id, &Fixture::runtime())
        .unwrap();
    let restored = Store::open(&f.root.join("db"), uuid()).unwrap();
    assert_eq!(restored.binding(&r.session_id).unwrap(), Some(b));
    assert!(matches!(
        restored.binding_receipt(&op).unwrap(),
        Some(ResultValue::Binding(_))
    ));
}

#[test]
fn recovery_at_every_persistence_checkpoint_retains_known_id_but_never_retries() {
    use super::super::super::runtime::MaterializationStage::*;
    for count in 0..=3 {
        let mut f = Fixture::new();
        let op = f.create();
        let r = f.reserve(&op);
        f.store.dispatch_binding(&r).unwrap();
        let id = CodexThreadId::parse("recover-known-empty-thread").unwrap();
        for stage in [Started, Archived, Unarchived].into_iter().take(count) {
            f.store.checkpoint_binding(&r, &id, stage).unwrap();
        }
        let mut restored = Store::open(&f.root.join("db"), uuid()).unwrap();
        restored.recover_binding_operations().unwrap();
        assert!(matches!(
            restored.binding_receipt(&op),
            Err(ControlError::BindingIndeterminate)
        ));
        assert!(restored.binding(&r.session_id).unwrap().is_none());
        let known: Option<String> = restored
            .conn
            .query_row(
                "SELECT provider_thread_id FROM managed_session_operations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            known,
            if count > 0 {
                Some(id.as_str().to_owned())
            } else {
                None
            }
        );
    }
}
