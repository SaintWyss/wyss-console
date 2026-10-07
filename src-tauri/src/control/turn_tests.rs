use super::{
    gate::ExecutionGate,
    schema,
    store::{Store, turns::StartFingerprint, uuid},
    turn_stream::Streams,
};
use crate::{
    adapters::codex::{CodexThreadId, CodexTurnId, safety},
    contracts::{
        control::*,
        identities::{AgentRuntimeInstanceId, ManagedTurnId},
    },
};
use serde_json::json;
use std::{fs, os::unix::fs::DirBuilderExt};
struct Fixture {
    root: std::path::PathBuf,
    store: Store,
    session: ManagedSessionRecord,
    binding: SessionBinding,
    lease: ExecutionLease,
    runtime: AgentRuntimeInstanceId,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-turn-unit-{}", uuid().as_str()));
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
        let ResultValue::Lease(lease) = store
            .apply(&Operation::LeaseGrant(LeaseGrant {
                intent_id: uuid(),
                session_id: session.managed_session_id.clone(),
                expected_session_revision: 1,
                scopes: vec![
                    ExecutionScope::SessionCreate,
                    ExecutionScope::SessionResume,
                    ExecutionScope::TurnStart,
                ],
                ttl_seconds: 3600,
            }))
            .unwrap()
        else {
            panic!()
        };
        let runtime = AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
        let op = Operation::BindingCreate(BindingCreate {
            intent_id: uuid(),
            managed_session_id: session.managed_session_id.clone(),
            expected_session_revision: 1,
            workspace_root: root.to_str().unwrap().into(),
        });
        let r = store.reserve_binding(&op).unwrap();
        store.dispatch_binding(&r).unwrap();
        let id = CodexThreadId::parse("opaque-durable-thread").unwrap();
        for stage in [
            super::runtime::MaterializationStage::Started,
            super::runtime::MaterializationStage::Archived,
            super::runtime::MaterializationStage::Unarchived,
        ] {
            store.checkpoint_binding(&r, &id, stage).unwrap();
        }
        let binding = store.commit_binding(&r, &id, &runtime).unwrap();
        Self {
            root,
            store,
            session,
            binding,
            lease,
            runtime,
        }
    }
    fn prompt(&self, text: &str) -> TurnStart {
        TurnStart {
            intent_id: uuid(),
            managed_session_id: self.session.managed_session_id.clone(),
            expected_session_revision: 1,
            prompt: text.into(),
        }
    }
    fn reserve(&mut self, p: &TurnStart) -> ManagedTurnRecord {
        self.store
            .reserve_turn(
                &StartFingerprint::compute(p).unwrap(),
                &self.binding,
                &self.lease,
                &self.runtime,
            )
            .unwrap()
    }
    fn ready(&self) -> RuntimeStatus {
        RuntimeStatus {
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
            desired_state: DesiredState::Running,
            revision: 1,
            actual_state: ActualState::Ready,
            runtime_instance_id: Some(self.runtime.clone()),
            runtime_version: Some("0.159.0".into()),
            compatibility: crate::contracts::agent_adapter::CompatibilityState::Compatible,
            model_count: 1,
            diagnostic: None,
            outbound_methods: RuntimeMethods::default(),
            runtime_safety_profile: Some(RuntimeSafetyProfile::CodexInferenceV1),
        }
    }
    fn attachment(&self) -> BindingStatus {
        BindingStatus {
            managed_session_id: self.session.managed_session_id.clone(),
            state: BindingState::BoundAttached,
            binding: Some(self.binding.clone()),
            runtime_instance_id: Some(self.runtime.clone()),
            operation_id: None,
            attachment_profile: Some(AttachmentProfile::InferenceOnlyV1),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn prompt_bounds_exact_utf8_digest_and_idempotency_are_secret_safe() {
    let mut f = Fixture::new();
    for s in ["".to_owned(), "x".repeat(8193)] {
        assert!(StartFingerprint::compute(&f.prompt(&s)).is_err());
    }
    let p = f.prompt("  ñ😃\n");
    let finger = StartFingerprint::compute(&p).unwrap();
    assert_eq!(finger.prompt_byte_length, p.prompt.len());
    assert_eq!(
        StartFingerprint::compute(&f.prompt("hello"))
            .unwrap()
            .prompt_fingerprint,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    let t = f.reserve(&p);
    assert_eq!(t.revision, 1);
    assert_ne!(t.managed_turn_id.as_str(), t.managed_session_id.as_str());
    assert_eq!(f.store.turn_receipt(&finger).unwrap().unwrap(), t);
    let mut conflict = p.clone();
    conflict.prompt.push('!');
    assert!(matches!(
        f.store
            .turn_receipt(&StartFingerprint::compute(&conflict).unwrap()),
        Err(ControlError::IntentConflict)
    ));
    assert!(matches!(
        f.store.reserve_turn(
            &StartFingerprint::compute(&f.prompt("second")).unwrap(),
            &f.binding,
            &f.lease,
            &f.runtime
        ),
        Err(ControlError::ActiveTurnExists)
    ));
    let provider = CodexTurnId::parse("provider-turn").unwrap();
    let running = f
        .store
        .transition_turn(
            &t.managed_turn_id,
            Some(&provider),
            TurnState::Running,
            None,
        )
        .unwrap();
    assert_eq!(running.revision, 2);
    let done = f
        .store
        .transition_turn(
            &t.managed_turn_id,
            None,
            TurnState::Completed,
            Some(TurnReason::Completed),
        )
        .unwrap();
    assert_eq!(done.revision, 3);
    assert_eq!(
        f.store
            .transition_turn(
                &t.managed_turn_id,
                None,
                TurnState::Completed,
                Some(TurnReason::Completed)
            )
            .unwrap(),
        done
    );
}
#[test]
fn sentinel_never_enters_database_wal_shm_audit_or_receipts() {
    let mut f = Fixture::new();
    let sentinel = format!("WYSS_2H_SECRET_PROMPT_{}", uuid().as_str());
    let p = f.prompt(&sentinel);
    let t = f.reserve(&p);
    f.store
        .transition_turn(
            &t.managed_turn_id,
            Some(&CodexTurnId::parse("provider-turn").unwrap()),
            TurnState::Running,
            None,
        )
        .unwrap();
    f.store
        .cancel_turn_receipt(
            &TurnCancel {
                intent_id: uuid(),
                managed_turn_id: t.managed_turn_id.clone(),
                expected_revision: 2,
            },
            TurnReason::UserCancel,
        )
        .unwrap();
    f.store
        .transition_turn(
            &t.managed_turn_id,
            None,
            TurnState::Interrupted,
            Some(TurnReason::UserCancel),
        )
        .unwrap();
    let mut streams = Streams::default();
    streams.push(
        &t,
        StreamKind::AssistantMessageDelta,
        Some("ASSISTANT_SENTINEL"),
    );
    streams.push(
        &t,
        StreamKind::ReasoningSummaryDelta,
        Some("SUMMARY_SENTINEL"),
    );
    for name in ["db", "db-wal", "db-shm"] {
        let bytes = fs::read(f.root.join(name)).unwrap_or_default();
        for raw in [
            sentinel.as_bytes(),
            b"ASSISTANT_SENTINEL",
            b"SUMMARY_SENTINEL",
        ] {
            assert!(
                !bytes.windows(raw.len()).any(|w| w == raw),
                "durable content in {name}"
            );
        }
    }
    let next = Store::open(&f.root.join("db"), uuid()).unwrap();
    assert_eq!(
        next.turn(&t.managed_turn_id).unwrap().state,
        TurnState::Interrupted
    );
}
#[test]
fn turn_admission_and_terminal_audit_failures_rollback() {
    let mut f = Fixture::new();
    let p = f.prompt("test");
    f.store.conn.execute_batch("CREATE TEMP TRIGGER no_audit BEFORE INSERT ON control_events BEGIN SELECT RAISE(ABORT,'fixture'); END").unwrap();
    assert!(
        f.store
            .reserve_turn(
                &StartFingerprint::compute(&p).unwrap(),
                &f.binding,
                &f.lease,
                &f.runtime
            )
            .is_err()
    );
    assert!(f.store.active_turn().unwrap().is_none());
    assert!(
        f.store
            .turn_receipt(&StartFingerprint::compute(&p).unwrap())
            .unwrap()
            .is_none()
    );
    f.store.conn.execute_batch("DROP TRIGGER no_audit").unwrap();
    let t = f.reserve(&p);
    f.store.conn.execute_batch("CREATE TEMP TRIGGER no_audit BEFORE INSERT ON control_events BEGIN SELECT RAISE(ABORT,'fixture'); END").unwrap();
    assert!(
        f.store
            .transition_turn(
                &t.managed_turn_id,
                None,
                TurnState::Indeterminate,
                Some(TurnReason::DispatchUnknown)
            )
            .is_err()
    );
    assert_eq!(f.store.turn(&t.managed_turn_id).unwrap(), t);
}
#[test]
fn restart_reconciles_nonterminal_once_without_replay_or_new_turn() {
    let mut f = Fixture::new();
    let t = f.reserve(&f.prompt("never replay"));
    let mut next = Store::open(&f.root.join("db"), uuid()).unwrap();
    next.recover_turns().unwrap();
    let recovered = next.turn(&t.managed_turn_id).unwrap();
    assert_eq!(recovered.state, TurnState::Indeterminate);
    assert_eq!(recovered.revision, 2);
    let head = next.status().unwrap().last_sequence;
    next.recover_turns().unwrap();
    assert_eq!(head, next.status().unwrap().last_sequence);
    assert!(next.active_turn().unwrap().is_none());
    assert_eq!(
        next.binding(&t.managed_session_id).unwrap(),
        Some(f.binding.clone())
    );
}
#[test]
fn additional_turn_gate_matrix_denies_each_missing_fact() {
    let f = Fixture::new();
    let runtime = f.ready();
    let attachment = f.attachment();
    let gate = |r: &RuntimeStatus, b: &BindingStatus| {
        ExecutionGate::turn(&f.store, &f.session.managed_session_id, r, b).reason()
    };
    assert_eq!(gate(&runtime, &attachment), GateReason::Allowed);
    let mut b = attachment.clone();
    b.binding = None;
    assert_eq!(gate(&runtime, &b), GateReason::BindingNotFound);
    b = attachment.clone();
    b.state = BindingState::BoundDetached;
    assert_eq!(gate(&runtime, &b), GateReason::BindingDetached);
    b = attachment.clone();
    b.runtime_instance_id = Some(AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap());
    assert_eq!(gate(&runtime, &b), GateReason::AttachmentRuntimeMismatch);
    b = attachment.clone();
    b.attachment_profile = None;
    assert_eq!(gate(&runtime, &b), GateReason::InferenceProfileMissing);
    let mut r = runtime.clone();
    r.runtime_safety_profile = None;
    assert_eq!(
        gate(&r, &attachment),
        GateReason::RuntimeSafetyProfileMissing
    );
}
#[test]
fn stream_is_bounded_utf8_split_gap_and_retention_are_explicit() {
    let mut f = Fixture::new();
    let t = f.reserve(&f.prompt("text"));
    let mut streams = Streams::default();
    let text = "ñ😃".repeat(10_000);
    streams.push(&t, StreamKind::AssistantMessageDelta, Some(&text));
    let p = TurnEvents {
        managed_turn_id: t.managed_turn_id.clone(),
        after_sequence: None,
        limit: 50,
    };
    let page = streams.page(&t, &p);
    let mut reconstructed = String::new();
    let mut query = p.clone();
    loop {
        let part = streams.page(&t, &query);
        if part.events.is_empty() {
            break;
        }
        for event in &part.events {
            if let Some(text) = &event.text {
                reconstructed.push_str(text);
            }
        }
        query.after_sequence = part.next_sequence;
    }
    assert!(reconstructed == text, "UTF-8 reconstruction differs");
    assert!(
        page.events
            .iter()
            .all(|e| e.text.as_ref().is_none_or(|s| s.len() <= 4096))
    );
    for _ in 0..300 {
        streams.push(
            &t,
            StreamKind::AssistantMessageDelta,
            Some(&"x".repeat(4096)),
        );
    }
    let page = streams.page(&t, &p);
    assert!(page.gap && page.dropped_events > 0);
    assert_eq!(page.events[0].kind, StreamKind::StreamGap);
    assert!(serde_json::to_vec(&page).unwrap().len() < 65_000);
    for _ in 0..8 {
        let mut next = t.clone();
        next.managed_turn_id = ManagedTurnId::parse(uuid().as_str()).unwrap();
        streams.push(&next, StreamKind::TurnStarted, None);
    }
    assert!(!streams.page(&t, &p).retained);
    assert_eq!(streams.page(&t, &p).events[0].kind, StreamKind::StreamGap);
}
#[test]
fn v4_migration_preserves_all_rows_bytes_and_rolls_back() {
    let f = Fixture::new();
    let path = f.root.join("v4");
    let mut c = rusqlite::Connection::open(&path).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    for (_, sql) in schema::V4_OBJECTS {
        c.execute_batch(sql).unwrap();
    }
    c.execute(
        "ATTACH DATABASE ?1 AS source",
        [f.root.join("db").to_str().unwrap()],
    )
    .unwrap();
    for table in [
        "managed_sessions",
        "managed_runtimes",
        "execution_leases",
        "managed_session_bindings",
        "managed_session_operations",
        "processed_intents",
        "control_events",
    ] {
        c.execute_batch(&format!("INSERT INTO {table} SELECT * FROM source.{table}"))
            .unwrap();
    }
    c.execute_batch("INSERT INTO authority_metadata SELECT id,4,stream_id,last_sequence FROM source.authority_metadata;PRAGMA user_version=4;").unwrap();
    let snapshot = |c: &rusqlite::Connection| -> Vec<String> {
        [
            "managed_sessions",
            "managed_runtimes",
            "execution_leases",
            "managed_session_bindings",
            "managed_session_operations",
            "processed_intents",
            "control_events",
        ]
        .iter()
        .map(|table| {
            let mut q = c
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
                .unwrap();
            let count = q.column_count();
            let values = q
                .query_map([], |r| {
                    Ok((0..count)
                        .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                        .collect::<Vec<_>>()
                        .join("|"))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect::<Vec<_>>();
            values.join("\n")
        })
        .collect()
    };
    let before = snapshot(&c);
    let meta: (String, String) = c
        .query_row(
            "SELECT stream_id,last_sequence FROM authority_metadata",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(schema::fail_migration(&mut c).is_err());
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        4
    );
    assert_eq!(snapshot(&c), before);
    schema::migrate(&mut c).unwrap();
    assert_eq!(snapshot(&c), before);
    assert_eq!(
        c.query_row::<(String, String), _, _>(
            "SELECT stream_id,last_sequence FROM authority_metadata",
            [],
            |r| Ok((r.get(0)?, r.get(1)?))
        )
        .unwrap(),
        meta
    );
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM managed_turns", [], |r| r.get(0))
            .unwrap(),
        0
    );
    drop(c);
    let next = Store::open(&path, uuid()).unwrap();
    assert_eq!(next.status().unwrap().storage_schema_version, 5);
    assert_eq!(
        next.binding(&f.session.managed_session_id).unwrap(),
        Some(f.binding.clone())
    );
}
#[test]
fn inference_classifier_suppresses_raw_reasoning_and_rejects_external_actions() {
    use crate::adapters::codex::{Direction, WireMessage};
    for (method, p) in [
        ("hook/started", json!({})),
        ("mcpServer/startupStatus/updated", json!({})),
        ("turn/diff/updated", json!({"diff":"file change"})),
        ("item/started", json!({"item":{"type":"commandExecution"}})),
        ("item/completed", json!({"item":{"type":"fileChange"}})),
        ("item/started", json!({"item":{"type":"mcpToolCall"}})),
        (
            "item/started",
            json!({"item":{"type":"collabAgentToolCall"}}),
        ),
        ("item/started", json!({"item":{"type":"webSearch"}})),
    ] {
        let m = WireMessage::from_value(
            json!({"method":method,"params":p}),
            Direction::ServerToClient,
        );
        assert!(
            m.is_err() || safety::notification(&m.unwrap()).is_err(),
            "{method}"
        );
    }
    let raw = WireMessage::from_value(
        json!({"method":"item/reasoning/textDelta","params":{"delta":"RAW_PRIVATE"}}),
        Direction::ServerToClient,
    )
    .unwrap();
    assert!(safety::notification(&raw).unwrap().is_none());
}

#[test]
fn config_origin_bound_is_separate_exactly_correlated_and_other_frames_stay_strict() {
    use crate::adapters::codex::{CodexRequestId, Direction, WireMessage};
    let origins: serde_json::Map<String, serde_json::Value> = (0..312)
        .map(|i| (format!("setting_{i}"), json!({"version":"fixture"})))
        .collect();
    let v = json!({"id":"config-only","result":{"config":{},"origins":origins}});
    let bytes = serde_json::to_vec(&v).unwrap();
    let id = CodexRequestId::String("config-only".into());
    assert!(WireMessage::parse(&bytes, Direction::ServerToClient).is_err());
    assert!(
        WireMessage::parse_diagnosed_for_config(&bytes, Direction::ServerToClient, Some(&id))
            .is_ok()
    );
    assert!(
        WireMessage::parse_diagnosed_for_config(
            &bytes,
            Direction::ServerToClient,
            Some(&CodexRequestId::String("other".into()))
        )
        .is_err()
    );
    let mut v = v;
    for i in 312..513 {
        v["result"]["origins"][format!("setting_{i}")] = json!({});
    }
    assert!(
        WireMessage::parse_diagnosed_for_config(
            &serde_json::to_vec(&v).unwrap(),
            Direction::ServerToClient,
            Some(&id)
        )
        .is_err()
    );
}
#[test]
fn safety_preflight_rejects_reflected_capability_drift_and_suppresses_private_reasoning() {
    use crate::adapters::codex::{Direction, WireMessage};
    let mut config = json!({"config":{"web_search":"disabled"}});
    for key in safety::INFERENCE_DISABLED_KEYS {
        let mut at = &mut config["config"];
        for part in key.split('.') {
            if !at.is_object() {
                *at = json!({});
            }
            at = &mut at[part];
        }
        *at = json!(false);
    }
    // A managed engine setting is allowed only together with the separately checked zero inventory.
    config["config"]["features"]["hooks"] = json!(true);
    let catalog = std::path::Path::new("/run/user/1000/wyss-console/codex-model-catalog.json");
    config["config"]["agents"] = json!({"enabled": false});
    config["config"]["model_catalog_json"] = json!(catalog.to_str().unwrap());
    assert!(safety::verify_config(&config, catalog).is_ok());
    for key in [
        "shell_tool",
        "unified_exec",
        "plugins",
        "apps",
        "multi_agent",
        "image_generation",
    ] {
        let mut bad = config.clone();
        bad["config"]["features"][key] = json!(true);
        assert!(safety::verify_config(&bad, catalog).is_err());
    }
    let message = |method: &str| {
        WireMessage::from_value(json!({"method":method,"params":{"threadId":"thread","turnId":"turn","itemId":"item","delta":"PRIVATE_SENTINEL","contentIndex":0,"summaryIndex":0}}),Direction::ServerToClient).unwrap()
    };
    assert!(
        safety::notification(&message("item/reasoning/textDelta"))
            .unwrap()
            .is_none()
    );
    assert!(
        safety::notification(&message("item/reasoning/summaryTextDelta"))
            .unwrap()
            .is_some()
    );
    let part=WireMessage::from_value(json!({"method":"item/reasoning/summaryPartAdded","params":{"threadId":"thread","turnId":"turn","itemId":"item","summaryIndex":0}}),Direction::ServerToClient).unwrap();
    assert!(safety::notification(&part).unwrap().is_some());
}
#[test]
fn turn_journal_corruption_of_prompt_metadata_fails_on_reopen() {
    let mut f = Fixture::new();
    let t = f.reserve(&f.prompt("transient"));
    f.store
        .conn
        .execute(
            "UPDATE managed_turns SET prompt_fingerprint=?1 WHERE managed_turn_id=?2",
            rusqlite::params!["0".repeat(64), t.managed_turn_id.as_str()],
        )
        .unwrap();
    assert!(Store::open(&f.root.join("db"), uuid()).is_err());
}
