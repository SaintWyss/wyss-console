//! The commands the window can call, through a mock Tauri application with the managed
//! state a real one has, against a real prepared application where one is needed.
use super::*;
use crate::{
    control::store::uuid,
    f8::gui::harness::{Harness, eventually},
};
use serde_json::{Value, json};
use tauri::Manager;

const UUID_A: &str = "11111111-1111-4111-8111-111111111111";
const UUID_B: &str = "22222222-2222-4222-8222-222222222222";
const UUID_C: &str = "33333333-3333-4333-8333-333333333333";

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    tauri::async_runtime::block_on(future)
}
fn app(gui: F8Gui) -> tauri::App<tauri::test::MockRuntime> {
    let app = tauri::test::mock_app();
    app.manage(gui);
    app.manage(WorkspaceSelections::default());
    app
}
fn fence() -> Value {
    json!({"session": UUID_A, "generation": 1, "instance": UUID_B})
}
fn code<T: std::fmt::Debug>(result: Result<T, F8Error>) -> Value {
    serde_json::to_value(result.unwrap_err()).unwrap()["code"].clone()
}

#[test]
fn requests_accept_exactly_their_narrow_fields_and_no_control_method() {
    let page = serde_json::from_value::<SessionsRequest>;
    assert!(page(json!({})).unwrap().before.is_none());
    assert_eq!(
        page(json!({"before": "7"})).unwrap().before.unwrap().get(),
        7
    );
    for rejected in [
        json!({"before": 7}),
        json!({"before": "0"}),
        json!({"before": "1", "limit": 5000}),
        json!({"method": "managedSession/list", "params": {}}),
    ] {
        assert!(page(rejected.clone()).is_err(), "{rejected}");
    }
    // A fence carries exactly the observation: no extra key, no wildcard generation type.
    let close = serde_json::from_value::<CloseRequest>;
    let ok = json!({"intent_id": UUID_C, "fence": fence(), "expected_revision": 1});
    assert!(close(ok.clone()).is_ok());
    for bad in [
        json!({"intent_id": UUID_C, "fence": fence(), "expected_revision": 1, "adapter_id": "codex"}),
        json!({"intent_id": UUID_C, "fence": {"session": UUID_A, "generation": "1", "instance": null}, "expected_revision": 1}),
        json!({"intent_id": UUID_C, "fence": {"session": UUID_A, "generation": 1, "instance": UUID_B, "pid": 7}, "expected_revision": 1}),
        json!({"intent_id": "not-a-uuid", "fence": fence(), "expected_revision": 1}),
    ] {
        assert!(close(bad.clone()).is_err(), "{bad}");
    }
    // Neither a model, an effort, a tool list nor a safety profile can be named for a start.
    let start = serde_json::from_value::<StartRuntimeRequest>;
    let base = json!({"intent_id": UUID_C, "session_id": UUID_A, "expected_revision": 0,
        "expected_generation": 0, "mode": "inference", "workspace": null});
    assert!(start(base.clone()).is_ok());
    for key in [
        "model",
        "effort",
        "safety_profile",
        "argv",
        "env",
        "workspace_root",
    ] {
        let mut extra = base.clone();
        extra[key] = json!("x");
        assert!(start(extra).is_err(), "{key}");
    }
    let mut unknown_mode = base.clone();
    unknown_mode["mode"] = json!("codex_local_tools_v1");
    assert!(start(unknown_mode).is_err());
    // A decision is never data.
    let decide = serde_json::from_value::<DecideRequest>;
    let mut decision = json!({"intent_id": UUID_C, "fence": fence(), "approval_id": UUID_A, "expected_revision": 1});
    assert!(decide(decision.clone()).is_ok());
    decision["decision"] = json!("accept");
    assert!(decide(decision).is_err());
    // Only the closed scopes exist.
    let grant = serde_json::from_value::<GrantRequest>;
    let mut lease = json!({"intent_id": UUID_C, "fence": fence(), "expected_session_revision": 1,
        "scopes": ["turn_start", "command_execute"], "ttl_seconds": 60});
    assert!(grant(lease.clone()).is_ok());
    lease["scopes"] = json!(["turn_start", "root"]);
    assert!(grant(lease).is_err());
    // The prompt never prints.
    let turn: StartTurnRequest = serde_json::from_value(
        json!({"intent_id": UUID_C, "fence": fence(), "expected_session_revision": 1, "prompt": "SECRET PROMPT"}),
    )
    .unwrap();
    assert!(!format!("{turn:?}").contains("SECRET"));
}

#[test]
fn an_inactive_desktop_refuses_every_command_before_anything_is_sent() {
    let app = app(F8Gui::inactive());
    let gui = || app.state::<F8Gui>();
    let state = block(f8_state(gui())).unwrap();
    assert!(!state.active);
    assert_eq!(
        code(block(f8_sessions(gui(), SessionsRequest { before: None }))),
        "inactive"
    );
    assert_eq!(
        code(block(f8_session_prepare(
            gui(),
            PrepareRequest { intent_id: uuid() }
        ))),
        "inactive"
    );
    assert_eq!(
        code(block(f8_audit(gui(), AuditRequest { older: None }))),
        "inactive"
    );
    // Even a start the dialog never produced a folder for is refused as inactive first
    // only after the folder check: the folder is checked before anything else.
    let request: StartRuntimeRequest = serde_json::from_value(json!({
        "intent_id": UUID_C, "session_id": UUID_A, "expected_revision": 0, "expected_generation": 0,
        "mode": "local_tools", "workspace": "/tmp"}))
    .unwrap();
    assert_eq!(
        code(block(f8_runtime_start(
            gui(),
            app.state::<WorkspaceSelections>(),
            request
        ))),
        "invalid"
    );
    assert_eq!(gui().stats().connections, 0);
}

#[test]
fn the_commands_drive_a_real_application_and_only_a_chosen_folder_is_a_workspace() {
    let harness = Harness::start(true, 1);
    let app = app(harness.gui.clone());
    let gui = || app.state::<F8Gui>();
    let state = block(f8_state(gui())).unwrap();
    assert!(state.active && state.authority.is_some());
    let session = block(f8_session_prepare(
        gui(),
        PrepareRequest { intent_id: uuid() },
    ))
    .unwrap()
    .id;
    let page = block(f8_sessions(gui(), SessionsRequest { before: None })).unwrap();
    assert_eq!(page.sessions[0].id, session);
    let workspace = harness.workspace.to_str().unwrap().to_owned();
    let start = |workspace: &str| {
        serde_json::from_value::<StartRuntimeRequest>(json!({
            "intent_id": uuid(), "session_id": session, "expected_revision": 0, "expected_generation": 0,
            "mode": "local_tools", "workspace": workspace})).unwrap()
    };
    // The dialog has not returned this folder: nothing reaches the authority.
    let before = harness.gui.stats().connections;
    assert_eq!(
        code(block(f8_runtime_start(
            gui(),
            app.state::<WorkspaceSelections>(),
            start(&workspace)
        ))),
        "invalid"
    );
    assert_eq!(harness.gui.stats().connections, before);
    app.state::<WorkspaceSelections>()
        .remember_for_test(&workspace);
    let receipt = block(f8_runtime_start(
        gui(),
        app.state::<WorkspaceSelections>(),
        start(&workspace),
    ))
    .unwrap();
    assert_eq!(receipt.generation, 1);
    let detail = eventually(|| {
        let d = block(f8_session(
            gui(),
            SessionRequest {
                session_id: session.clone(),
            },
        ))
        .ok()?;
        d.session.runtime.instance.is_some().then_some(d)
    });
    // A stale fence is the authority's refusal, and the link survives it.
    let stale = serde_json::from_value::<StopRuntimeRequest>(json!({
        "intent_id": uuid(),
        "fence": {"session": session, "generation": 1, "instance": uuid()},
        "expected_revision": detail.session.runtime.revision}))
    .unwrap();
    assert_eq!(code(block(f8_runtime_stop(gui(), stale))), "stale");
    assert!(block(f8_state(gui())).unwrap().authority.is_some());
}
