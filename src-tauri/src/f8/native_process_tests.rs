//! Native schema-derived fixture over real owned pipes; no authenticated provider.
use super::*;
use crate::contracts::WireUuid;
use crate::f8::{
    engine::Spawner,
    launcher::{NativeInferenceSpawner, NativeLaunchPlan},
    protocol::RuntimeRef,
    staging::LaunchStaging,
};
use std::{fs, os::unix::fs::PermissionsExt, process::Command, time::Duration};

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "native fixture timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn fixture_plan() -> (Root, Attachment, NativeLaunchPlan) {
    let root = Root(std::env::temp_dir().join(format!("wyss-native-{}", uuid::Uuid::new_v4())));
    fs::create_dir(&root.0).unwrap();
    fs::set_permissions(&root.0, fs::Permissions::from_mode(0o700)).unwrap();
    let catalog = root.0.join("catalog.json");
    fs::write(&catalog, br#"{"models":[{"slug":"fixture","tool_mode":"direct","experimental_supported_tools":[],"multi_agent_version":null,"apply_patch_tool_type":null}]}"#).unwrap();
    fs::set_permissions(&catalog, fs::Permissions::from_mode(0o600)).unwrap();
    // The existing native fixture requires a previously durable thread for resume.
    fs::write(
        root.0.join("threads.json"),
        br#"{"fixture-thread":{"durable":true,"archived":false}}"#,
    )
    .unwrap();
    let names = BTreeSet::from(["fixture_mcp".to_owned()]);
    let mut command = Command::new("/usr/bin/python3");
    command
        .env_clear()
        .env("WYSS_RUNTIME_FIXTURE", &root.0)
        .arg("-u")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codex_runtime.py"
        ))
        .args(safety::process_args(&names, Some(&catalog)).unwrap());
    let attachment = Attachment {
        workspace: root.0.to_str().unwrap().into(),
        profile: "codex_inference_v1".into(),
        revision: 1,
    };
    let plan = NativeLaunchPlan {
        command,
        attachment: attachment.clone(),
        catalogue: catalog,
        observed_version: "0.159.0".into(),
    };
    (root, attachment, plan)
}

#[test]
fn existing_native_fixture_handshake_resume_turn_and_owned_shutdown() {
    let (_root, attachment, plan) = fixture_plan();
    let reference = RuntimeRef {
        schema: 2,
        authority_instance_id: WireUuid::try_from(
            "00000000-0000-4000-8000-0000000000a1".to_owned(),
        )
        .unwrap(),
        managed_session_id: ManagedSessionId::parse("00000000-0000-4000-8000-000000000001")
            .unwrap(),
        runtime_instance_id: AgentRuntimeInstanceId::parse("00000000-0000-4000-8000-000000000002")
            .unwrap(),
        generation: 1,
    };
    let mut plan = Some(plan);
    let mut factory =
        NativeInferenceSpawner::new(move |_: &RuntimeRef, _: Option<&mut LaunchStaging>| {
            Ok(plan.take().unwrap())
        });
    let mut native = factory.spawn(&reference, None).unwrap().worker;
    let mut events = Vec::new();
    until(|| {
        native.flush().unwrap();
        events.extend(
            native
                .inbound(1)
                .unwrap()
                .into_iter()
                .map(|b| serde_json::from_slice::<Value>(&b).unwrap()),
        );
        assert!(
            !native.has_exited(),
            "native fixture exited during handshake"
        );
        events.iter().any(|e| e["kind"] == "runtime_ready")
            && events.iter().any(|e| e["kind"] == "models")
    });
    assert_eq!(events.iter().filter(|e| e["kind"] == "models").count(), 1);
    assert!(events.iter().any(|e| e["kind"] == "runtime_ready"));
    until(|| match native.preflight(&attachment) {
        Ok(()) => true,
        Err(E::RuntimeOperationInProgress) => {
            native.flush().unwrap();
            native.inbound(1).unwrap();
            false
        }
        other => panic!("preflight: {other:?}"),
    });
    native.send(&serde_json::to_vec(&json!({"method":"thread/resume","operation_id":"fixture-operation","workspace":attachment.workspace,"thread_id":"fixture-thread"})).unwrap()).unwrap();
    until(|| {
        native.flush().unwrap();
        native
            .inbound(1)
            .unwrap()
            .iter()
            .any(|b| serde_json::from_slice::<Value>(b).unwrap()["kind"] == "binding_confirmed")
    });
    until(|| match native.preflight(&attachment) {
        Ok(()) => true,
        Err(E::RuntimeOperationInProgress) => {
            native.flush().unwrap();
            native.inbound(1).unwrap();
            false
        }
        other => panic!("preflight: {other:?}"),
    });
    native.send(br#"{"method":"turn/start","turn":"00000000-0000-4000-8000-000000000003","input":"fixture"}"#).unwrap();
    events.clear();
    until(|| {
        native.flush().unwrap();
        events.extend(
            native
                .inbound(1)
                .unwrap()
                .into_iter()
                .map(|b| serde_json::from_slice::<Value>(&b).unwrap()),
        );
        events.iter().any(|e| e["kind"] == "turn_completed")
    });
    assert_eq!(
        events
            .iter()
            .filter(|e| e["kind"] == "turn_running")
            .count(),
        1
    );
    assert!(events.iter().any(|e| e["text"] == "ASSISTANT_SENTINEL"));
    assert!(
        !events
            .iter()
            .any(|e| e.to_string().contains("RAW_PRIVATE_REASONING_SENTINEL"))
    );
    native.begin_stop(Instant::now());
    until(|| native.poll_stop(Instant::now()) == StopState::Reaped);
}

#[test]
fn native_factory_application_socket_durable_lifecycle() {
    use crate::{
        control::{application::PreparedApplication, f8_fixture},
        f8::{
            migration,
            process_integration_tests::{call, poll_until},
            v8::validate_v8,
        },
    };
    use std::os::unix::net::UnixStream;
    let id = || uuid::Uuid::new_v4().to_string();
    let (_root, attachment, plan) = fixture_plan();
    let workspace = attachment.workspace.clone();
    let mut plan = Some(plan);
    let factory =
        NativeInferenceSpawner::new(move |_: &RuntimeRef, _: Option<&mut LaunchStaging>| {
            Ok(plan.take().unwrap())
        });
    let mut source = f8_fixture::v7_with_completed_turn();
    let connection = migration::prepare(&mut source).unwrap().into_connection();
    let authority = WireUuid::try_from("00000000-0000-4000-8000-0000000000a1".to_owned()).unwrap();
    let (mut application, handle) =
        PreparedApplication::new(connection, authority, None, factory).unwrap();
    let (owned, mut socket) = UnixStream::pair().unwrap();
    socket.set_nonblocking(true).unwrap();
    handle.attach(owned).unwrap();
    let hello = call(
        &mut application,
        &mut socket,
        "authority/hello",
        json!({"protocol_version":2,"client_instance_id":id()}),
    );
    assert!(hello["result"].is_object(), "{hello}");
    let prepared = call(
        &mut application,
        &mut socket,
        "managedSession/prepare",
        json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex"}),
    );
    let session = prepared["result"]["value"]["managed_session_id"]
        .as_str()
        .expect("prepared session")
        .to_owned();
    let started = call(
        &mut application,
        &mut socket,
        "runtime/start",
        json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_revision":0,"expected_generation":0,"safety_profile":"codex_inference_v1"}),
    );
    assert!(started["error"].is_null(), "{started}");
    poll_until(&mut application, |a| {
        a.service()
            .authority()
            .engine()
            .runtime_view(&session)
            .is_some_and(|(state, _)| state == "ready")
    });
    let reference = application
        .service()
        .authority()
        .engine()
        .reference(&session)
        .unwrap();
    let fence = json!(reference.fence());
    let lease = call(
        &mut application,
        &mut socket,
        "executionLease/grant",
        json!({"intent_id":id(),"session_id":session,"expected_session_revision":1,"scopes":["session_create","session_resume","turn_start"],"ttl_seconds":300,"runtime_fence":fence}),
    );
    assert!(lease["error"].is_null(), "{lease}");
    let create = json!({"intent_id":id(),"managed_session_id":session,"expected_session_revision":1,"workspace_root":workspace,"runtime_fence":fence});
    until(|| {
        let answer = call(
            &mut application,
            &mut socket,
            "managedSession/runtime/create",
            create.clone(),
        );
        if answer["error"]["code"] == "runtime_operation_in_progress" {
            return false;
        }
        assert!(answer["error"].is_null(), "{answer}");
        true
    });
    let start_turn = json!({"intent_id":id(),"managed_session_id":session,"expected_session_revision":1,"prompt":"native fixture","runtime_fence":fence});
    let mut turn = String::new();
    until(|| {
        let answer = call(
            &mut application,
            &mut socket,
            "managedSession/turn/start",
            start_turn.clone(),
        );
        if answer["error"]["code"] == "runtime_operation_in_progress" {
            return false;
        }
        assert!(answer["error"].is_null(), "{answer}");
        turn = answer["result"]["value"]["managed_turn_id"]
            .as_str()
            .unwrap()
            .to_owned();
        true
    });
    poll_until(&mut application, |a| {
        a.service()
            .authority()
            .engine()
            .connection()
            .query_row(
                "SELECT state='completed' FROM managed_turns WHERE managed_turn_id=?1",
                [&turn],
                |r| r.get::<_, bool>(0),
            )
            .unwrap()
    });
    let stopped = call(
        &mut application,
        &mut socket,
        "runtime/stop",
        json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_revision":1,"expected_generation":1,"expected_runtime_instance_id":reference.runtime_instance_id}),
    );
    assert!(stopped["error"].is_null(), "{stopped}");
    poll_until(&mut application, |a| {
        a.service()
            .authority()
            .engine()
            .reference(&session)
            .is_none()
    });
    let closed = call(
        &mut application,
        &mut socket,
        "managedSession/close",
        json!({"intent_id":id(),"managed_session_id":session,"expected_revision":1,"runtime_fence":{"managed_session_id":session,"generation":1,"runtime_instance_id":null}}),
    );
    assert_eq!(
        closed["result"]["value"]["control_state"], "closed",
        "{closed}"
    );
    let stream: String = application
        .service()
        .authority()
        .engine()
        .connection()
        .query_row("SELECT stream_id FROM authority_metadata", [], |r| r.get(0))
        .unwrap();
    let audit = call(
        &mut application,
        &mut socket,
        "audit/read",
        json!({"stream_id":stream,"after_sequence":null,"limit":50}),
    );
    assert_eq!(audit["result"]["kind"], "audit", "{audit}");
    assert!(audit["result"]["value"]["events"].as_array().unwrap().len() >= 10);
    validate_v8(application.service().authority().engine().connection()).unwrap();
    assert!(
        application
            .service()
            .authority()
            .engine()
            .cleanup_complete()
    );
}
