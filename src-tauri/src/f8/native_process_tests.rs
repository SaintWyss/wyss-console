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

#[test]
fn local_tools_cannot_use_the_inference_factory() {
    let (_root, _, mut plan) = fixture_plan();
    plan.attachment.profile = "codex_local_tools_v1".into();
    let reference = RuntimeRef {
        schema: 2,
        authority_instance_id: WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap(),
        managed_session_id: ManagedSessionId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        runtime_instance_id: AgentRuntimeInstanceId::parse(&uuid::Uuid::new_v4().to_string())
            .unwrap(),
        generation: 1,
    };
    let mut plan = Some(plan);
    let mut factory =
        NativeInferenceSpawner::new(move |_: &RuntimeRef, _: Option<&mut LaunchStaging>| {
            Ok(plan.take().unwrap())
        });
    assert!(matches!(
        factory.spawn(&reference, None),
        Err(E::Unsupported)
    ));
}

fn local_fixture_plan(
    scenario: &str,
) -> (Root, Attachment, crate::f8::launcher::NativeLocalToolsPlan) {
    let (root, attachment, plan) = local_fixture_plan_result(scenario);
    (root, attachment, plan.unwrap())
}
fn local_fixture_plan_result(
    scenario: &str,
) -> (
    Root,
    Attachment,
    ControlResult<crate::f8::launcher::NativeLocalToolsPlan>,
) {
    use crate::adapters::codex::probe::isolation::Isolation;
    use crate::f8::launcher::NativeLocalToolsPlan;
    let root =
        Root(std::env::temp_dir().join(format!("wyss-native-local-{}", uuid::Uuid::new_v4())));
    fs::create_dir(&root.0).unwrap();
    fs::set_permissions(&root.0, fs::Permissions::from_mode(0o700)).unwrap();
    for dir in [
        "user",
        "bin",
        "repo/.git",
        "repo/work",
        "run/wyss-console",
        "state/wyss-console",
        "cwd",
    ] {
        fs::create_dir_all(root.0.join(dir)).unwrap();
    }
    fs::set_permissions(root.0.join("cwd"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.0.join("user/auth.json"), "FIXTURE-CREDENTIAL").unwrap();
    fs::set_permissions(
        root.0.join("user/auth.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let catalog = root.0.join("catalog.json");
    fs::write(&catalog, br#"{"models":[{"slug":"fixture","tool_mode":"direct","experimental_supported_tools":[],"multi_agent_version":null,"apply_patch_tool_type":"freeform"}]}"#).unwrap();
    fs::set_permissions(&catalog, fs::Permissions::from_mode(0o600)).unwrap();
    let executable = root.0.join("bin/codex-fixture");
    let script = format!(
        "#!/usr/bin/python3\nimport os,runpy\nos.environ['WYSS_RUNTIME_FIXTURE']={}\nos.environ['WYSS_FIXTURE_USER_HOME']={}\nos.environ['WYSS_RUNTIME_SCENARIO']={}\nrunpy.run_path({},run_name='__main__')\n",
        json!(root.0.to_str().unwrap()),
        json!(root.0.join("user").to_str().unwrap()),
        json!(scenario),
        json!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codex_runtime.py"
        ))
    );
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let isolation = Isolation {
        workspace: root.0.join("repo/work"),
        state: root.0.join("state/wyss-console/codex-local-tools"),
        control: vec![
            root.0.join("run/wyss-console"),
            root.0.join("state/wyss-console"),
        ],
    };
    let attachment = Attachment {
        workspace: isolation.workspace.to_str().unwrap().into(),
        profile: "codex_local_tools_v1".into(),
        revision: 1,
    };
    let mut plan = NativeLocalToolsPlan::prepare(
        isolation,
        &executable,
        &root.0.join("user"),
        &root.0.join("cwd"),
        catalog,
        "0.159.0",
    );
    // Synthetic switches only; production plans expose no environment mutation.
    if let Ok(plan) = &mut plan {
        plan.fixture_env("WYSS_F8_NATIVE_FIXTURE", std::ffi::OsStr::new("1"));
    }
    (root, attachment, plan)
}

#[test]
fn native_local_tools_application_socket_accept_decline_cancel() {
    use crate::{
        control::{application::PreparedApplication, f8_fixture},
        f8::{
            launcher::NativeLocalToolsSpawner,
            migration,
            process_integration_tests::{call, poll_until},
            v8::validate_v8,
        },
    };
    use std::os::unix::net::UnixStream;
    for (scenario, decision) in [
        ("approval_command", "accept"),
        ("approval_command", "decline"),
        ("approval_command", "cancel"),
        ("approval_file", "accept"),
        ("approval_file", "decline"),
        ("approval_file", "cancel"),
    ] {
        let id = || uuid::Uuid::new_v4().to_string();
        eprintln!("native lifecycle {scenario}/{decision}");
        let (root, attachment, plan) = local_fixture_plan(scenario);
        let factory = NativeLocalToolsSpawner::new(plan);
        let mut source = f8_fixture::v7_with_completed_turn();
        let c = migration::prepare(&mut source).unwrap().into_connection();
        let path = root.0.join("authority.sqlite3");
        c.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        drop(c);
        let c = rusqlite::Connection::open(path).unwrap();
        let auth = WireUuid::try_from("00000000-0000-4000-8000-0000000000a1".to_owned()).unwrap();
        let (mut app, handle) = PreparedApplication::new(c, auth, None, factory).unwrap();
        let (owned, mut socket) = UnixStream::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        handle.attach(owned).unwrap();
        assert!(
            call(
                &mut app,
                &mut socket,
                "authority/hello",
                json!({"protocol_version":2,"client_instance_id":id()})
            )["error"]
                .is_null()
        );
        let prepared = call(
            &mut app,
            &mut socket,
            "managedSession/prepare",
            json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex"}),
        );
        let session = prepared["result"]["value"]["managed_session_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let start = call(
            &mut app,
            &mut socket,
            "runtime/start",
            json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_revision":0,"expected_generation":0,"safety_profile":"codex_local_tools_v1","workspace_root":attachment.workspace}),
        );
        assert!(start["error"].is_null(), "{start}");
        poll_until(&mut app, |a| {
            a.service()
                .authority()
                .engine()
                .runtime_view(&session)
                .is_some_and(|(state, _)| state == "ready")
        });
        let reference = app
            .service()
            .authority()
            .engine()
            .reference(&session)
            .unwrap();
        let fence = json!(reference.fence());
        let lease = call(
            &mut app,
            &mut socket,
            "executionLease/grant",
            json!({"intent_id":id(),"session_id":session,"expected_session_revision":1,"scopes":["session_create","turn_start","command_execute","file_change"],"ttl_seconds":300,"runtime_fence":fence}),
        );
        assert!(lease["error"].is_null(), "{lease}");
        let create = json!({"intent_id":id(),"managed_session_id":session,"expected_session_revision":1,"workspace_root":attachment.workspace,"runtime_fence":fence});
        until(|| {
            let r = call(
                &mut app,
                &mut socket,
                "managedSession/runtime/create",
                create.clone(),
            );
            if r["error"]["code"] == "runtime_operation_in_progress" {
                return false;
            }
            assert!(r["error"].is_null(), "{r}");
            true
        });
        let start = json!({"intent_id":id(),"managed_session_id":session,"expected_session_revision":1,"prompt":"synthetic","runtime_fence":fence});
        let mut turn = String::new();
        until(|| {
            let r = call(
                &mut app,
                &mut socket,
                "managedSession/turn/start",
                start.clone(),
            );
            if r["error"]["code"] == "runtime_operation_in_progress" {
                return false;
            }
            assert!(r["error"].is_null(), "{r}");
            turn = r["result"]["value"]["managed_turn_id"]
                .as_str()
                .unwrap()
                .into();
            true
        });
        poll_until(&mut app, |a| {
            a.service().authority().engine().pending_approvals(&session) == 1
        });
        let pending = call(
            &mut app,
            &mut socket,
            "managedTurn/approval/pending",
            json!({"managed_turn_id":turn}),
        );
        assert!(
            pending["result"]["value"]["approval"].is_object(),
            "{pending}"
        );
        let approval: String = app
            .service()
            .authority()
            .engine()
            .connection()
            .query_row(
                "SELECT managed_approval_id FROM managed_approvals WHERE managed_turn_id=?1",
                [&turn],
                |r| r.get(0),
            )
            .unwrap();
        let status = call(
            &mut app,
            &mut socket,
            "managedTurn/approval/status",
            json!({"managed_approval_id":approval}),
        );
        assert_eq!(status["result"]["value"]["acceptable"], true, "{status}");
        let respond = json!({"intent_id":id(),"managed_approval_id":approval,"expected_revision":1,"decision":decision,"runtime_fence":fence});
        let mut receipt = Value::Null;
        until(|| {
            let r = call(
                &mut app,
                &mut socket,
                "managedTurn/approval/respond",
                respond.clone(),
            );
            if r["error"]["code"] == "runtime_operation_in_progress" {
                return false;
            }
            assert!(r["error"].is_null(), "{r}");
            receipt = r;
            true
        });
        poll_until(&mut app, |a| {
            a.service().authority().engine().connection().query_row("SELECT state NOT IN ('starting','running','cancel_requested') FROM managed_turns WHERE managed_turn_id=?1",[&turn],|r|r.get::<_,bool>(0)).unwrap()
        });
        let replay = call(
            &mut app,
            &mut socket,
            "managedTurn/approval/respond",
            respond,
        );
        assert_eq!(replay["result"], receipt["result"]);
        assert_eq!(
            fs::read_to_string(root.0.join("decisions"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        let count = fs::read_to_string(root.0.join(if scenario == "approval_file" {
            "mutations"
        } else {
            "executions"
        }))
        .unwrap_or_default()
        .lines()
        .count();
        assert_eq!(count, usize::from(decision == "accept"));
        let stop = call(
            &mut app,
            &mut socket,
            "runtime/stop",
            json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_revision":1,"expected_generation":1,"expected_runtime_instance_id":reference.runtime_instance_id}),
        );
        assert!(stop["error"].is_null(), "{stop}");
        poll_until(&mut app, |a| {
            a.service()
                .authority()
                .engine()
                .reference(&session)
                .is_none()
        });
        let close = call(
            &mut app,
            &mut socket,
            "managedSession/close",
            json!({"intent_id":id(),"managed_session_id":session,"expected_revision":1,"runtime_fence":{"managed_session_id":session,"generation":1,"runtime_instance_id":null}}),
        );
        assert!(close["error"].is_null(), "{close}");
        validate_v8(app.service().authority().engine().connection()).unwrap();
        assert!(app.service().authority().engine().cleanup_complete());
    }
}

struct LocalRig<S: Spawner> {
    _handle: crate::control::application::ApplicationHandle,
    root: Root,
    app: crate::control::application::PreparedApplication<S>,
    socket: std::os::unix::net::UnixStream,
    reference: RuntimeRef,
    session: String,
    turn: String,
    lease: String,
}
fn local_rig(scenario: &str) -> LocalRig<impl Spawner> {
    local_rig_fault(scenario, None)
}
fn local_rig_fault(scenario: &str, fault: Option<NativeCriticalFault>) -> LocalRig<impl Spawner> {
    use crate::{
        control::{application::PreparedApplication, f8_fixture},
        f8::{
            launcher::NativeLocalToolsSpawner,
            migration,
            process_integration_tests::{call, poll_until},
        },
    };
    use std::os::unix::net::UnixStream;
    let ttl = if scenario == "approval_command_expired_lease" {
        1
    } else {
        300
    };
    let id = || uuid::Uuid::new_v4().to_string();
    let (root, attachment, plan) = local_fixture_plan(scenario);
    let factory = NativeLocalToolsSpawner::new(plan).with_critical_fault(fault);
    let mut source = f8_fixture::v7_with_completed_turn();
    let c = migration::prepare(&mut source).unwrap().into_connection();
    let path = root.0.join("authority.sqlite3");
    c.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    drop(c);
    let c = rusqlite::Connection::open(&path).unwrap();
    let auth = WireUuid::try_from("00000000-0000-4000-8000-0000000000a1".to_owned()).unwrap();
    let (mut app, handle) = PreparedApplication::new(c, auth, None, factory).unwrap();
    let (owned, mut socket) = UnixStream::pair().unwrap();
    socket.set_nonblocking(true).unwrap();
    handle.attach(owned).unwrap();
    assert!(
        call(
            &mut app,
            &mut socket,
            "authority/hello",
            json!({"protocol_version":2,"client_instance_id":id()})
        )["error"]
            .is_null()
    );
    let prepared = call(
        &mut app,
        &mut socket,
        "managedSession/prepare",
        json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex"}),
    );
    let session = prepared["result"]["value"]["managed_session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let start = call(
        &mut app,
        &mut socket,
        "runtime/start",
        json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_revision":0,"expected_generation":0,"safety_profile":"codex_local_tools_v1","workspace_root":attachment.workspace}),
    );
    assert!(start["error"].is_null(), "{start}");
    poll_until(&mut app, |a| {
        a.service()
            .authority()
            .engine()
            .runtime_view(&session)
            .is_some_and(|(state, _)| state == "ready")
    });
    let reference = app
        .service()
        .authority()
        .engine()
        .reference(&session)
        .unwrap();
    let fence = json!(reference.fence());
    let lease = call(
        &mut app,
        &mut socket,
        "executionLease/grant",
        json!({"intent_id":id(),"session_id":session,"expected_session_revision":1,"scopes":["session_create","turn_start","command_execute","file_change"],"ttl_seconds":ttl,"runtime_fence":fence}),
    );
    assert!(lease["error"].is_null(), "{lease}");
    let create = json!({"intent_id":id(),"managed_session_id":session,"expected_session_revision":1,"workspace_root":attachment.workspace,"runtime_fence":fence});
    until(|| {
        let r = call(
            &mut app,
            &mut socket,
            "managedSession/runtime/create",
            create.clone(),
        );
        if r["error"]["code"] == "runtime_operation_in_progress" {
            return false;
        }
        assert!(r["error"].is_null(), "{r}");
        true
    });
    let start = json!({"intent_id":id(),"managed_session_id":session,"expected_session_revision":1,"prompt":"synthetic","runtime_fence":fence});
    let mut turn = String::new();
    until(|| {
        let r = call(
            &mut app,
            &mut socket,
            "managedSession/turn/start",
            start.clone(),
        );
        if r["error"]["code"] == "runtime_operation_in_progress" {
            return false;
        }
        assert!(r["error"].is_null(), "{r}");
        turn = r["result"]["value"]["managed_turn_id"]
            .as_str()
            .unwrap()
            .into();
        true
    });

    let lease = lease["result"]["value"]["lease_id"]
        .as_str()
        .unwrap()
        .to_owned();
    LocalRig {
        _handle: handle,
        root,
        app,
        socket,
        reference,
        session,
        turn,
        lease,
    }
}
impl<S: Spawner> LocalRig<S> {
    fn call(&mut self, method: &str, params: Value) -> Value {
        crate::f8::process_integration_tests::call(&mut self.app, &mut self.socket, method, params)
    }
    fn c(&self) -> &rusqlite::Connection {
        self.app.service().authority().engine().connection()
    }
    fn authority_mut(&mut self) -> &mut crate::f8::authority::Authority {
        self.app.transport_mut().service_mut().authority_mut()
    }
    fn pending(&mut self) -> String {
        let session = self.session.clone();
        crate::f8::process_integration_tests::poll_until(&mut self.app, |a| {
            a.service().authority().engine().pending_approvals(&session) == 1
        });
        self.c()
            .query_row(
                "SELECT managed_approval_id FROM managed_approvals WHERE managed_turn_id=?1",
                [&self.turn],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn respond(&mut self, params: Value) -> Value {
        let mut result = Value::Null;
        until(|| {
            result = self.call("managedTurn/approval/respond", params.clone());
            result["error"]["code"] != "runtime_operation_in_progress"
        });
        result
    }
    fn decision(&self, approval: &str) -> Value {
        json!({"intent_id":uuid::Uuid::new_v4().to_string(),"managed_approval_id":approval,"expected_revision":1,"decision":"accept","runtime_fence":self.reference.fence()})
    }
    fn reap(&mut self) {
        self.authority_mut().engine_mut().shutdown();
        crate::f8::process_integration_tests::poll_until(&mut self.app, |a| {
            a.service().authority().engine().cleanup_complete()
        });
        crate::f8::v8::validate_v8(self.c()).unwrap();
    }
    fn no_acceptance(&self) {
        assert!(!self.root.0.join("executions").exists());
        assert!(!self.root.0.join("mutations").exists());
        assert!(!self.root.0.join("decisions").exists());
    }
}

#[test]
fn native_socket_rejects_mismatched_duplicate_withdrawn_and_unapproved_actions() {
    for scenario in [
        "approval_command_wrong_thread",
        "approval_command_wrong_turn",
        "approval_command_changed",
        "approval_command_changed_actions",
        "approval_command_double",
        "approval_command_withdrawn",
        "approval_command_early",
        "approval_command_unasked",
        "approval_command_hidden_terminal",
        "approval_command_permissions",
        "approval_command_network",
        "approval_command_remote",
        "approval_file_grant",
        "approval_command_crash_before",
    ] {
        let mut rig = local_rig(scenario);
        let session = rig.session.clone();
        crate::f8::process_integration_tests::poll_until(&mut rig.app, |a| {
            a.service()
                .authority()
                .engine()
                .reference(&session)
                .is_none()
        });
        rig.no_acceptance();
        let pending:i64=rig.c().query_row("SELECT count(*) FROM managed_approvals WHERE state='pending' AND managed_turn_id=?1",[&rig.turn],|r|r.get(0)).unwrap();
        assert_eq!(pending, 0, "{scenario}: no surviving executable approval");
        assert_eq!(
            rig.app
                .service()
                .authority()
                .engine()
                .pending_approvals(&rig.session),
            0
        );
        let state: String = rig
            .c()
            .query_row(
                "SELECT state FROM managed_turns WHERE managed_turn_id=?1",
                [&rig.turn],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            !matches!(state.as_str(), "starting" | "running" | "cancel_requested"),
            "{scenario}: {state}"
        );
        assert!(rig.app.service().authority().engine().cleanup_complete());
        crate::f8::v8::validate_v8(rig.c()).unwrap();
    }
}

#[test]
fn native_socket_decisions_are_fenced_by_session_generation_and_intent() {
    let mut rig = local_rig("approval_command");
    let approval = rig.pending();
    for field in ["session", "generation", "instance"] {
        let mut decision = rig.decision(&approval);
        match field {
            "session" => {
                decision["runtime_fence"]["managed_session_id"] =
                    json!(uuid::Uuid::new_v4().to_string())
            }
            "generation" => decision["runtime_fence"]["generation"] = json!(2),
            _ => {
                decision["runtime_fence"]["runtime_instance_id"] =
                    json!(uuid::Uuid::new_v4().to_string())
            }
        }
        assert!(rig.respond(decision)["error"].is_object());
        rig.no_acceptance();
        assert_eq!(
            rig.app
                .service()
                .authority()
                .engine()
                .pending_approvals(&rig.session),
            1
        );
    }
    let decision = rig.decision(&approval);
    let response = rig.respond(decision.clone());
    assert!(response["error"].is_null(), "{response}");
    let mut conflict = decision.clone();
    conflict["decision"] = json!("cancel");
    assert_eq!(rig.respond(conflict)["error"]["code"], "intent_conflict");
    assert_eq!(rig.respond(decision)["result"], response["result"]);
    until(|| rig.root.0.join("decisions").exists());
    assert_eq!(
        fs::read_to_string(rig.root.0.join("decisions"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    rig.reap();
}

#[test]
fn native_socket_lease_revocation_expiry_and_cancellation_remove_authorization() {
    for mode in [
        "revoke",
        "expiry",
        "lease_expiry",
        "cancel",
        "fingerprint",
        "policy",
    ] {
        let mut rig = local_rig(if mode == "lease_expiry" {
            "approval_command_expired_lease"
        } else {
            "approval_command"
        });
        let approval = rig.pending();
        let decision = rig.decision(&approval);
        match mode {
            "lease_expiry" => std::thread::sleep(Duration::from_millis(1100)),
            "revoke" => {
                let r=rig.call("executionLease/revoke",json!({"intent_id":uuid::Uuid::new_v4().to_string(),"lease_id":rig.lease,"expected_revision":1}));
                assert!(r["error"].is_null(), "{r}");
            }
            "expiry" => {
                // Exercise the production expiry writer with a future clock,
                // rather than tampering with validated history or sleeping 5m.
                let auth = rig.reference.authority_instance_id.clone();
                let c = rig.authority_mut().engine_mut().connection_mut();
                crate::f8::writers::Writer::new(c, auth)
                    .unwrap()
                    .with_clock(|| time::OffsetDateTime::now_utc() + time::Duration::seconds(600))
                    .expire_approvals()
                    .unwrap();
            }
            "cancel" => {
                let r=rig.call("managedSession/turn/cancel",json!({"intent_id":uuid::Uuid::new_v4().to_string(),"managed_turn_id":rig.turn,"expected_revision":2,"runtime_fence":rig.reference.fence()}));
                assert!(r["error"].is_null(), "{r}");
            }
            "fingerprint" => {
                // Corrupt only the transient provider claim, never the ledger.
                let origin = rig.reference.clone();
                rig.authority_mut()
                    .engine_mut()
                    .fixture_corrupt_fingerprint(&origin, "call-1");
            }
            _ => {
                fs::create_dir(rig.root.0.join("repo/work/.codex")).unwrap();
                fs::write(
                    rig.root.0.join("repo/work/.codex/config.toml"),
                    "untrusted-change",
                )
                .unwrap();
            }
        }
        let response = rig.respond(decision);
        assert!(response["error"].is_object(), "{mode}: {response}");
        rig.no_acceptance();
        rig.reap();
        let status = rig.call(
            "managedTurn/approval/status",
            json!({"managed_approval_id":approval}),
        );
        assert_eq!(
            status["result"]["value"]["acceptable"], false,
            "{mode}: {status}"
        );
    }
}

#[test]
fn native_local_tools_attestation_failures_never_publish_ready_or_hold_capacity() {
    use crate::{
        control::{application::PreparedApplication, f8_fixture},
        f8::{
            launcher::NativeLocalToolsSpawner,
            migration,
            process_integration_tests::{call, poll_until},
        },
    };
    use std::os::unix::net::UnixStream;
    for scenario in [
        "local_wrong_home",
        "local_no_requirement",
        "local_tmp_writable",
        "local_mcp",
        "hooks_enabled",
        "host_stale_grant",
        "host_exec_failure",
        "host_namespace_failure",
    ] {
        let id = || uuid::Uuid::new_v4().to_string();
        let (root, attachment, mut plan) = local_fixture_plan(scenario);
        match scenario {
            "host_stale_grant" => {
                let auth = root.0.join("user/auth.json");
                fs::rename(&auth, auth.with_extension("old")).unwrap();
                fs::write(&auth, "REPLACEMENT-FIXTURE").unwrap();
                fs::set_permissions(auth, fs::Permissions::from_mode(0o600)).unwrap();
            }
            "host_exec_failure" => plan.fixture_exec_failure(),
            "host_namespace_failure" => plan.fixture_without_namespace(),
            _ => (),
        }
        let factory = NativeLocalToolsSpawner::new(plan);
        let mut source = f8_fixture::v7_with_completed_turn();
        let c = migration::prepare(&mut source).unwrap().into_connection();
        let path = root.0.join("authority.sqlite3");
        c.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        drop(c);
        let c = rusqlite::Connection::open(&path).unwrap();
        let auth = WireUuid::try_from("00000000-0000-4000-8000-0000000000a1".to_owned()).unwrap();
        let (mut app, handle) = PreparedApplication::new(c, auth, None, factory).unwrap();
        let (owned, mut socket) = UnixStream::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        handle.attach(owned).unwrap();
        assert!(
            call(
                &mut app,
                &mut socket,
                "authority/hello",
                json!({"protocol_version":2,"client_instance_id":id()})
            )["error"]
                .is_null()
        );
        let prepared = call(
            &mut app,
            &mut socket,
            "managedSession/prepare",
            json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex"}),
        );
        let session = prepared["result"]["value"]["managed_session_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let start = call(
            &mut app,
            &mut socket,
            "runtime/start",
            json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_revision":0,"expected_generation":0,"safety_profile":"codex_local_tools_v1","workspace_root":attachment.workspace}),
        );
        if matches!(scenario, "host_stale_grant" | "host_namespace_failure") {
            assert!(start["error"].is_object(), "{start}");
        } else {
            assert!(start["error"].is_null(), "{start}");
        }

        poll_until(&mut app, |a| {
            a.service()
                .authority()
                .engine()
                .reference(&session)
                .is_none()
        });
        let c = app.service().authority().engine().connection();
        let ready:i64=c.query_row("SELECT count(*) FROM control_events WHERE session_id=?1 AND json_extract(envelope,'$.event_type')='authority.runtime.instance_ready'",[&session],|r|r.get(0)).unwrap();
        assert_eq!(ready, 0, "{scenario}");
        assert!(app.service().authority().engine().cleanup_complete());
        assert!(!root.0.join("decisions").exists());
        crate::f8::v8::validate_v8(c).unwrap();
    }
}

#[test]
fn native_critical_backpressure_prefix_and_lost_write_results_never_redispatch() {
    for fault in [
        NativeCriticalFault::Backpressure,
        NativeCriticalFault::PrefixThenFail,
        NativeCriticalFault::AfterWriteFail,
    ] {
        let mut rig = local_rig_fault("approval_command", Some(fault));
        let approval = rig.pending();
        let decision = rig.decision(&approval);
        let result = rig.respond(decision.clone());
        assert_eq!(
            result["error"]["code"], "external_effect_unknown",
            "{result}"
        );
        rig.reap();
        let state: String = rig
            .c()
            .query_row(
                "SELECT state FROM managed_approvals WHERE managed_approval_id=?1",
                [&approval],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "indeterminate");
        let replay = rig.respond(decision);
        assert!(replay["error"].is_null(), "{replay}");
        let lines = fs::read_to_string(rig.root.0.join("decisions"))
            .unwrap_or_default()
            .lines()
            .count();
        assert!(lines <= 1);
        assert_eq!(
            rig.app
                .service()
                .authority()
                .engine()
                .pending_approvals(&rig.session),
            0
        );
    }
}

#[test]
fn native_audit_failure_after_critical_write_recovers_without_resending() {
    let mut rig = local_rig("approval_command_slow");
    let approval = rig.pending();
    let decision = rig.decision(&approval);
    rig.authority_mut().fixture_after_approval_write(|c|c.execute_batch(
        "CREATE TEMP TRIGGER native_delivery_audit_failure BEFORE INSERT ON control_events WHEN json_extract(NEW.envelope,'$.payload.delivery')='kernel_accepted' BEGIN SELECT RAISE(ABORT,'synthetic audit failure'); END;"
    ).unwrap());
    let response = rig.respond(decision.clone());
    assert!(response["error"].is_object(), "{response}");
    rig.reap();
    assert_eq!(
        rig.c()
            .query_row(
                "SELECT state FROM managed_approvals WHERE managed_approval_id=?1",
                [&approval],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "indeterminate"
    );
    let replay = rig.respond(decision);
    assert!(replay["error"].is_null(), "{replay}");
    assert!(
        fs::read_to_string(rig.root.0.join("decisions"))
            .unwrap_or_default()
            .lines()
            .count()
            <= 1
    );
    let delivery = crate::f8::writers::read_delivery(rig.c(), &approval).unwrap();
    assert_eq!(
        delivery,
        Some(crate::contracts::approval::ApprovalDelivery::Indeterminate)
    );
}

#[test]
fn native_numeric_ids_crash_after_decision_and_active_turn_shutdown() {
    for scenario in [
        "approval_command_numeric",
        "approval_command_crash_after",
        "approval_command_slow",
    ] {
        let mut rig = local_rig(scenario);
        let approval = rig.pending();
        let blocked=rig.call("runtime/stop",json!({"intent_id":uuid::Uuid::new_v4().to_string(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":rig.session,"expected_revision":1,"expected_generation":1,"expected_runtime_instance_id":rig.reference.runtime_instance_id}));
        assert_eq!(blocked["error"]["code"], "active_turn_exists");
        let decision = rig.decision(&approval);
        let response = rig.respond(decision.clone());
        assert!(response["error"].is_null(), "{response}");
        until(|| {
            rig.app.poll(Instant::now()).unwrap();
            rig.root.0.join("critical-ids").exists()
        });
        let ids = fs::read_to_string(rig.root.0.join("critical-ids")).unwrap();
        assert_eq!(
            ids.trim(),
            if scenario == "approval_command_numeric" {
                "17"
            } else {
                "\"approval-1\""
            }
        );
        if scenario == "approval_command_numeric" {
            crate::f8::process_integration_tests::poll_until(&mut rig.app, |a| {
                a.service()
                    .authority()
                    .engine()
                    .active_turn(&rig.session)
                    .is_none()
            });
        }
        rig.reap();
        let state: String = rig
            .c()
            .query_row(
                "SELECT state FROM managed_approvals WHERE managed_approval_id=?1",
                [&approval],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            state,
            if scenario == "approval_command_numeric" {
                "accepted"
            } else {
                "indeterminate"
            }
        );
        assert!(rig.respond(decision)["error"].is_null());
        assert_eq!(
            fs::read_to_string(rig.root.0.join("critical-ids")).unwrap(),
            ids
        );
    }
}

#[test]
fn native_restart_retains_history_but_no_approval_authority() {
    use crate::{
        control::application::PreparedApplication,
        f8::{launcher::NativeLocalToolsSpawner, process_integration_tests::call},
    };
    use std::os::unix::net::UnixStream;
    let mut rig = local_rig("approval_command_slow");
    let approval = rig.pending();
    let decision = rig.decision(&approval);
    let response = rig.respond(decision.clone());
    assert!(response["error"].is_null());
    rig.reap();
    let path = rig.root.0.join("authority.sqlite3");
    let LocalRig {
        root, app, _handle, ..
    } = rig;
    drop(app);
    drop(_handle);
    let c = rusqlite::Connection::open(path).unwrap();
    let factory = NativeLocalToolsSpawner::empty();
    let auth = WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
    let (mut app, handle) = PreparedApplication::new(c, auth, None, factory).unwrap();
    let (owned, mut socket) = UnixStream::pair().unwrap();
    socket.set_nonblocking(true).unwrap();
    handle.attach(owned).unwrap();
    assert!(
        call(
            &mut app,
            &mut socket,
            "authority/hello",
            json!({"protocol_version":2,"client_instance_id":uuid::Uuid::new_v4().to_string()})
        )["error"]
            .is_null()
    );
    let history = call(
        &mut app,
        &mut socket,
        "managedTurn/approval/status",
        json!({"managed_approval_id":approval}),
    );
    assert_eq!(history["result"]["value"]["acceptable"], false);
    assert!(history["result"]["value"]["attestation"].is_null());
    let replay = call(
        &mut app,
        &mut socket,
        "managedTurn/approval/respond",
        decision.clone(),
    );
    assert_eq!(replay["result"], response["result"]);
    let mut new_intent = decision;
    new_intent["intent_id"] = json!(uuid::Uuid::new_v4().to_string());
    assert!(
        call(
            &mut app,
            &mut socket,
            "managedTurn/approval/respond",
            new_intent
        )["error"]
            .is_object()
    );
    assert!(app.service().authority().engine().cleanup_complete());
    crate::f8::v8::validate_v8(app.service().authority().engine().connection()).unwrap();
    assert!(
        fs::read_to_string(root.0.join("decisions"))
            .unwrap_or_default()
            .lines()
            .count()
            <= 1
    );
}

#[test]
fn native_host_rejects_version_not_matching_sealed_executable() {
    let (_root, _attachment, plan) = local_fixture_plan_result("f8_wrong_version");
    assert!(matches!(plan, Err(E::Unsupported)));
}

#[test]
fn native_incomplete_reap_retains_capacity_and_never_attests_a_replacement() {
    let mut rig = local_rig_fault("approval_command", Some(NativeCriticalFault::ReapStuck));
    let _approval = rig.pending();
    rig.authority_mut().engine_mut().shutdown();
    for _ in 0..25 {
        let _ = rig.app.poll(Instant::now());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!rig.app.service().authority().engine().cleanup_complete());
    assert!(
        rig.app
            .service()
            .authority()
            .engine()
            .reference(&rig.session)
            .is_some()
    );
    let prepared=rig.call("managedSession/prepare",json!({"intent_id":uuid::Uuid::new_v4().to_string(),"adapter_id":"codex","runtime_kind":"codex"}));
    let session = prepared["result"]["value"]["managed_session_id"]
        .as_str()
        .unwrap();
    let start=rig.call("runtime/start",json!({"intent_id":uuid::Uuid::new_v4().to_string(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_revision":0,"expected_generation":0,"safety_profile":"codex_local_tools_v1","workspace_root":rig.root.0.join("repo/work")}));
    assert_eq!(start["error"]["code"], "capacity_exceeded", "{start}");
    rig.no_acceptance();
    crate::f8::v8::validate_v8(rig.c()).unwrap();
}

#[test]
fn native_completed_item_must_match_original_action_and_ids_cannot_be_reused() {
    let mut rig = local_rig("approval_command_changed_completion");
    let approval = rig.pending();
    let response = rig.respond(rig.decision(&approval));
    assert!(response["error"].is_null());
    let session = rig.session.clone();
    crate::f8::process_integration_tests::poll_until(&mut rig.app, |a| {
        a.service()
            .authority()
            .engine()
            .reference(&session)
            .is_none()
    });
    assert_eq!(
        rig.c()
            .query_row(
                "SELECT effect FROM managed_approvals WHERE managed_approval_id=?1",
                [&approval],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "unknown"
    );
    rig.reap();
    let mut rig = local_rig("approval_command");
    let approval = rig.pending();
    assert!(rig.respond(rig.decision(&approval))["error"].is_null());
    let session = rig.session.clone();
    crate::f8::process_integration_tests::poll_until(&mut rig.app, |a| {
        a.service()
            .authority()
            .engine()
            .active_turn(&session)
            .is_none()
    });
    let params = json!({"intent_id":uuid::Uuid::new_v4().to_string(),"managed_session_id":rig.session,"expected_session_revision":1,"prompt":"second synthetic turn","runtime_fence":rig.reference.fence()});
    let mut response = Value::Null;
    until(|| {
        response = rig.call("managedSession/turn/start", params.clone());
        response["error"]["code"] != "runtime_operation_in_progress"
    });
    assert!(response["error"].is_null(), "{response}");
    crate::f8::process_integration_tests::poll_until(&mut rig.app, |a| {
        a.service()
            .authority()
            .engine()
            .reference(&session)
            .is_none()
    });
    let count: i64 = rig
        .c()
        .query_row(
            "SELECT count(*) FROM managed_approvals WHERE managed_session_id=?1",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 1,
        "a reused provider request ID cannot open another durable approval"
    );
    assert_eq!(
        fs::read_to_string(rig.root.0.join("decisions"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    rig.reap();
}
