// Maintained regressions from the independent AUD-01 security review.

fn audit_review_guard() -> (
    WorkspaceFixture,
    Connection,
    crate::f8::workspace_guard::WorkspaceGuard,
    String,
) {
    let fixture = WorkspaceFixture::new();
    let mut conn = migrate(&v9_image(0), |_| Ok(())).unwrap();
    let coordinator = uuid::Uuid::new_v4().to_string();
    let guard = crate::f8::workspace_guard::WorkspaceGuard::open(
        &mut conn,
        uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    guard
        .import_claims(&mut conn, &coordinator, 1, &[])
        .unwrap();
    (fixture, conn, guard, coordinator)
}

#[test]
fn audit_review_positive_same_root_blocks_and_siblings_progress() {
    let (f, mut c, mut g, owner) = audit_review_guard();
    let a = f.path("a");
    let b = f.path("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    let ra = g.observe_root(&mut c, &a).unwrap();
    let rb = g.observe_root(&mut c, &b).unwrap();
    g.acquire_claim(
        &mut c,
        &owner,
        &uuid::Uuid::new_v4().to_string(),
        1,
        &"a".repeat(64),
        &ra,
    )
    .unwrap();
    assert!(
        g.acquire_claim(
            &mut c,
            &owner,
            &uuid::Uuid::new_v4().to_string(),
            1,
            &"b".repeat(64),
            &ra
        )
        .is_err()
    );
    assert!(
        g.acquire_claim(
            &mut c,
            &owner,
            &uuid::Uuid::new_v4().to_string(),
            1,
            &"b".repeat(64),
            &rb
        )
        .is_ok()
    );
}

#[test]
fn audit_review_replacement_locator_cannot_admit_second_claim() {
    let (f, mut c, mut g, owner) = audit_review_guard();
    let a = f.path("a");
    let moved = f.path("a-moved");
    fs::create_dir(&a).unwrap();
    fs::write(a.join("same"), b"same-content").unwrap();
    let ra = g.observe_root(&mut c, &a).unwrap();
    let first = g
        .acquire_claim(
            &mut c,
            &owner,
            &uuid::Uuid::new_v4().to_string(),
            1,
            &"a".repeat(64),
            &ra,
        )
        .unwrap();
    fs::rename(&a, &moved).unwrap();
    fs::create_dir(&a).unwrap();
    fs::write(a.join("same"), b"same-content").unwrap();
    let replacement = g.observe_root(&mut c, &a).unwrap();
    assert_ne!(ra, replacement);
    let second = g.acquire_claim(
        &mut c,
        &owner,
        &uuid::Uuid::new_v4().to_string(),
        1,
        &"b".repeat(64),
        &replacement,
    );
    eprintln!(
        "original={:?}; replacement_admission={:?}",
        g.claim_status(&c, &first.claim_id),
        second
    );
    assert!(
        second.is_err(),
        "unresolved original locator must stay fenced"
    );
}

#[test]
fn audit_review_moved_ancestor_cannot_make_live_nested_roots_disjoint() {
    use crate::f8::workspace_guard::{Overlap, RootPin};
    use std::os::unix::fs::symlink;
    let (f, mut c, mut g, owner) = audit_review_guard();
    let a = f.path("a");
    let work = a.join("work");
    let parent = f.path("parent");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir(&parent).unwrap();
    let rw = g.observe_root(&mut c, &work).unwrap();
    g.acquire_claim(
        &mut c,
        &owner,
        &uuid::Uuid::new_v4().to_string(),
        1,
        &"a".repeat(64),
        &rw,
    )
    .unwrap();
    fs::rename(&a, parent.join("a")).unwrap();
    symlink(parent.join("a"), &a).unwrap();
    let uid = || uuid::Uuid::new_v4().to_string();
    let live_child = RootPin::open(&work, uid(), uid(), uid()).unwrap();
    let live_parent = RootPin::open(&parent, uid(), uid(), uid()).unwrap();
    assert_eq!(
        live_child.overlap(&live_parent),
        Overlap::Conflict,
        "fresh proof confirms actual parent/child overlap"
    );
    let rp = g.observe_root(&mut c, &parent).unwrap();
    let second = g.acquire_claim(&mut c, &owner, &uid(), 1, &"b".repeat(64), &rp);
    eprintln!("actual_overlap=Conflict; new_parent_admission={second:?}");
    assert!(
        second.is_err(),
        "cached ancestry must not authorize overlapping roots"
    );
}

#[test]
fn sr01_drift_before_l1_and_between_l1_l2_revokes_dispatch() {
    for after_l1 in [false, true] {
        use crate::{
            contracts::{WireUuid, execution::ExecutionScope},
            f8::{
                authority::{Authority, Client},
                guard_protocol::{
                    StartEffect, StartProfile, StartPurpose, StartTuple, response_value,
                },
            },
        };
        use sha2::Digest;

        let workspace_fixture = WorkspaceFixture::new();
        let workspace = workspace_fixture.path("bound-root");
        let mut source = f8_fixture::v7_with_sessions_at(1, workspace.clone());
        let v8 = migration::prepare(&mut source).unwrap().into_connection();
        let legacy = v9::migrate(&v8, |_| Ok(())).unwrap();
        let (session, workspace): (String, String) = legacy
            .query_row(
                "SELECT s.session_id,b.workspace_root FROM managed_sessions s JOIN managed_session_bindings b ON b.managed_session_id=s.session_id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            PathBuf::from(&workspace),
            workspace_fixture.path("bound-root")
        );
        fs::create_dir_all(&workspace).unwrap();
        let connection = migrate(&legacy, |_| Ok(())).unwrap();
        let authority_id = WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
        let registry = Registry::with_history(
            authority_id.clone(),
            Vec::<String>::new(),
            Vec::<(String, u64)>::new(),
        )
        .unwrap();
        let engine = Engine::new(connection, authority_id.clone(), registry, None).unwrap();
        let mut authority = Authority::new_guarded(engine).unwrap();
        // This fixture is a freshly prepared test session. The legacy migration test separately
        // asserts that untouched sessions remain `unproven` and cannot use this path.
        authority
            .engine_mut()
            .connection_mut()
            .execute(
                "UPDATE managed_session_freshness SET freshness_state='proven',authority_epoch=?1,preparation_generation=1,ever_started=0 WHERE session_id=?2",
                rusqlite::params![authority_id.as_str(), session],
            )
            .unwrap();
        let frames = GuardedFrames(Arc::default());
        let mut spawner = GuardedSpawner(frames.clone());
        let mut client = Client::default();

        let hello = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "request_id": uuid::Uuid::new_v4().to_string(),
            "method": "authority/hello",
            "params": {"protocol_version": 2, "client_instance_id": uuid::Uuid::new_v4().to_string()}
        }))
        .unwrap();
        let hello_response =
            authority.handle(&mut client, &hello, &mut spawner, std::time::Instant::now());
        crate::f8::protocol::Response::parse(&hello_response).unwrap();

        let coordinator = "00000000-0000-4000-8000-000000000030";
        let imported = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/claims/import",
            serde_json::json!({"coordinator_store_id":coordinator,"snapshot_revision":1,"claims":[]}),
        );
        assert!(imported.get("error").is_none(), "{imported}");

        let capability = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "authority/capabilities",
            serde_json::json!({}),
        );
        let capability_id =
            WireUuid::try_from(capability["request_id"].as_str().unwrap().to_owned()).unwrap();
        let capability = response_value(
            &serde_json::to_vec(&capability).unwrap(),
            &capability_id,
            &authority_id,
        )
        .unwrap();
        assert_eq!(capability["guard_ready"], true);
        assert_eq!(capability["ready"], false);
        assert_eq!(capability["managed_execution_enabled"], false);

        let observed = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/root/observe",
            serde_json::json!({"locator":workspace}),
        );
        assert!(observed.get("error").is_none(), "{observed}");
        let root_binding_id = observed["result"]["value"]["root_binding_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let run_id = uuid::Uuid::new_v4().to_string();
        let plan_digest = "a".repeat(64);
        let claim = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/claim/acquire",
            serde_json::json!({"coordinator_store_id":coordinator,"run_id":run_id,"plan_revision":1,"plan_digest":plan_digest,"root_binding_id":root_binding_id}),
        );
        assert!(claim.get("error").is_none(), "{claim}");
        let claim = claim["result"]["value"].clone();

        let (generation, revision): (u64, u64) = authority
            .engine()
            .connection()
            .query_row(
                "SELECT COALESCE((SELECT generation FROM managed_runtimes WHERE managed_session_id=?1),0),COALESCE((SELECT revision FROM managed_runtimes WHERE managed_session_id=?1),0)",
                [&session],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64)),
            )
            .unwrap();
        let started = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "runtime/start",
            serde_json::json!({
                "intent_id":uuid::Uuid::new_v4().to_string(),"adapter_id":"codex","runtime_kind":"codex",
                "managed_session_id":session,"expected_generation":generation,"expected_revision":revision,
                "safety_profile":"codex_local_tools_v1","workspace_root":workspace
            }),
        );
        assert!(started.get("error").is_none(), "{started}");
        let reference = authority.engine().reference(&session).unwrap();
        authority
            .engine_mut()
            .complete_handshake(
                &reference,
                "0.159.0",
                Attachment {
                    workspace: workspace.clone(),
                    profile: "codex_local_tools_v1".into(),
                    revision: 1,
                },
            )
            .unwrap();
        let lease = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "executionLease/grant",
            serde_json::json!({
                "intent_id":uuid::Uuid::new_v4().to_string(),"session_id":session,"expected_session_revision":1,
                "scopes":[ExecutionScope::SessionResume,ExecutionScope::TurnStart],"ttl_seconds":600,"runtime_fence":reference.fence()
            }),
        );
        assert!(lease.get("error").is_none(), "{lease}");
        let lease_id = lease["result"]["value"]["lease_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let lease_revision = lease["result"]["value"]["revision"].as_u64().unwrap();
        let resume_id = uuid::Uuid::new_v4().to_string();
        let resumed = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "managedSession/runtime/resume",
            serde_json::json!({"intent_id":resume_id,"managed_session_id":session,"expected_session_revision":1,"runtime_fence":reference.fence()}),
        );
        assert_eq!(
            resumed["error"]["code"], "runtime_operation_in_progress",
            "{resumed}"
        );
        let thread: String = authority
            .engine()
            .connection()
            .query_row(
                "SELECT runtime_session_id FROM managed_session_bindings WHERE managed_session_id=?1",
                [&session],
                |row| row.get(0),
            )
            .unwrap();
        crate::f8::operations::confirm_binding(
            authority.engine_mut(),
            &reference,
            &WireUuid::try_from(resume_id).unwrap(),
            &thread,
        )
        .unwrap();
        let binding_revision: u64 = authority
            .engine()
            .connection()
            .query_row(
                "SELECT binding_revision FROM managed_session_bindings WHERE managed_session_id=?1",
                [&session],
                |row| row.get::<_, i64>(0).map(|v| v as u64),
            )
            .unwrap();

        let prompt = "synthetic guarded test prompt";
        let intent_id = uuid::Uuid::new_v4().to_string();
        let tuple = StartTuple {
            authority_store_id: capability["authority_store_id"]
                .as_str()
                .unwrap()
                .to_owned(),
            authority_epoch: authority_id.as_str().to_owned(),
            coordinator_store_id: coordinator.into(),
            run_id,
            plan_revision: 1,
            plan_digest,
            step_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            intent_id,
            effect_class: StartEffect::ManagedLocalTools,
            prompt_digest: format!("{:x}", sha2::Sha256::digest(prompt.as_bytes())),
            model_id: None,
            model_profile: StartProfile::CodexLocalToolsV1,
            root_binding_id,
            claim_id: claim["claim_id"].as_str().unwrap().to_owned(),
            claim_generation: claim["claim_generation"].as_u64().unwrap(),
            claim_revision: claim["claim_revision"].as_u64().unwrap(),
            root_observation_revision: 1,
            dispatch_gate_revision: claim["gate_revision"].as_u64().unwrap(),
            session_id: session.clone(),
            session_revision: 1,
            attachment_revision: binding_revision,
            runtime_instance_id: reference.runtime_instance_id.as_str().into(),
            runtime_generation: reference.generation,
            lease_id,
            lease_revision,
            approval_reference: None,
            purpose: StartPurpose::PipelineStep,
        };
        let prepare = || {
            guard_frame(
                &authority_id,
                "orchestration/start/prepare",
                serde_json::json!({"tuple":tuple}),
            )
        };
        let disabled = authority.handle(
            &mut client,
            &prepare(),
            &mut spawner,
            std::time::Instant::now(),
        );
        let disabled_id = WireUuid::try_from(
            serde_json::from_slice::<JsonValue>(&disabled).unwrap()["request_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
        .unwrap();
        assert!(
            response_value(&disabled, &disabled_id, &authority_id).is_err(),
            "disabled Managed execution must fail closed"
        );
        authority.fixture_enable_guarded_execution();
        let prepared = authority.handle(
            &mut client,
            &prepare(),
            &mut spawner,
            std::time::Instant::now(),
        );
        let prepared_json: JsonValue = serde_json::from_slice(&prepared).unwrap();
        let prepared_id =
            WireUuid::try_from(prepared_json["request_id"].as_str().unwrap().to_owned()).unwrap();
        let prepared = response_value(&prepared, &prepared_id, &authority_id).unwrap();
        let receipt_id = prepared["receipt_id"].as_str().unwrap().to_owned();
        let receipt_revision = prepared["receipt_revision"].as_u64().unwrap();
        let commit_params = serde_json::json!({"tuple":tuple,"receipt_id":receipt_id,"receipt_revision":receipt_revision,"prompt":prompt});
        let commit_frame = || {
            guard_frame(
                &authority_id,
                "orchestration/start/commit",
                commit_params.clone(),
            )
        };

        let original = PathBuf::from(&workspace);
        let new_parent = workspace_fixture.path("changed-parent");
        fs::create_dir(&new_parent).unwrap();
        let moved = new_parent.join("original");
        let move_original = move || {
            fs::rename(&original, &moved).unwrap();
            std::os::unix::fs::symlink(&moved, &original).unwrap();
        };
        if after_l1 {
            authority.fixture_after_guarded_reservation(move_original);
        } else {
            move_original();
        }
        let answer = authority.handle(
            &mut client,
            &commit_frame(),
            &mut spawner,
            std::time::Instant::now(),
        );
        let answer: JsonValue = serde_json::from_slice(&answer).unwrap();
        assert!(
            answer.get("error").is_some(),
            "drifted ancestry must refuse before provider bytes: {answer}"
        );
        let (receipt_state,gate_state,claim_phase,proof_revision):(String,String,String,i64)=authority.engine().connection().query_row(
            "SELECT r.state,c.gate_state,c.phase,b.proof_revision FROM guarded_start_receipts r JOIN workspace_claims c USING(claim_id) JOIN workspace_root_bindings b ON b.root_binding_id=c.root_binding_id WHERE r.receipt_id=?1",[&receipt_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        assert_eq!(gate_state, "stopped");
        assert_eq!(proof_revision, 2);
        assert_eq!(
            receipt_state,
            if after_l1 { "consumed" } else { "invalidated" }
        );
        assert_eq!(claim_phase, if after_l1 { "unsettled" } else { "held" });
        let _ = authority.handle(
            &mut client,
            &commit_frame(),
            &mut spawner,
            std::time::Instant::now(),
        );
        assert_eq!(
            frames
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|x| serde_json::from_slice::<JsonValue>(x)
                    .is_ok_and(|v| v["method"] == "turn/start"))
                .count(),
            0,
            "no stale receipt or repeated tick can send the drifted start"
        );
    }
}

#[test]
fn sr01_replacement_alias_is_fenced_while_independent_root_progresses() {
    let (f, mut c, mut g, owner) = audit_review_guard();
    let original = f.path("original");
    let sibling = f.path("sibling");
    fs::create_dir(&original).unwrap();
    fs::create_dir(&sibling).unwrap();
    let bound = g.observe_root(&mut c, &original).unwrap();
    let first = g
        .acquire_claim(
            &mut c,
            &owner,
            &uuid::Uuid::new_v4().to_string(),
            1,
            &"a".repeat(64),
            &bound,
        )
        .unwrap();
    fs::rename(&original, f.path("moved")).unwrap();
    fs::create_dir(&original).unwrap();
    let alias = f.path("replacement-alias");
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    let alias_root = g.observe_root(&mut c, &alias).unwrap();
    assert!(
        g.acquire_claim(
            &mut c,
            &owner,
            &uuid::Uuid::new_v4().to_string(),
            1,
            &"b".repeat(64),
            &alias_root
        )
        .is_err()
    );
    let independent = g.observe_root(&mut c, &sibling).unwrap();
    assert!(
        g.acquire_claim(
            &mut c,
            &owner,
            &uuid::Uuid::new_v4().to_string(),
            1,
            &"c".repeat(64),
            &independent
        )
        .is_ok()
    );
    let sealed = g.claim_status(&c, &first.claim_id).unwrap();
    assert_eq!(sealed.gate_state, "stopped");
    assert_eq!(sealed.phase, "held");
}
