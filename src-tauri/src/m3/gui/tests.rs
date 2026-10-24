use super::*;

fn keys(v: &serde_json::Value) -> Vec<String> {
    let mut k: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

#[test]
fn the_inactive_service_tells_the_window_that_claude_is_not_available_and_why() {
    let s = M3Service::inactive().state(false);
    assert_eq!((s.capability, s.version), ("m3", "m3.1"));
    assert_eq!(s.profiles.len(), 4);
    assert_eq!(s.claude_install, InstallView::NotConfigured);
    for c in &s.profiles[2..] {
        assert!(!c.can_start, "{:?}", c.id);
        assert!(c.reasons.contains(&"sdk_not_configured".to_owned()));
        assert!(c.reasons.contains(&"adr0053_not_accepted".to_owned()));
    }
    // Codex is not connected to anything here either.
    assert!(
        s.profiles[..2]
            .iter()
            .all(|c| !c.can_start && c.reasons == ["authority_not_connected"])
    );
    assert!(s.fixture.is_none());
}

#[test]
fn a_connected_codex_authority_makes_the_codex_profiles_startable_and_never_the_claude_ones() {
    let s = M3Service::inactive().state(true);
    let startable: Vec<_> = s
        .profiles
        .iter()
        .filter(|c| c.can_start)
        .map(|c| c.name)
        .collect();
    assert_eq!(startable, ["codex_inference_v1", "codex_local_tools_v1"]);
}

#[test]
fn model_selection_and_history_are_off_with_their_reasons() {
    let s = M3Service::inactive().state(false);
    assert!(!s.model_selection.available);
    assert_eq!(s.model_selection.reasons, ["adr0054_not_accepted"]);
    assert_eq!(
        (s.model_selection.capability, s.model_selection.version),
        ("m3.model_selection", 1)
    );
    assert_eq!(
        (s.history.state, s.history.enabled, s.history.can_enable),
        ("off", false, false)
    );
    assert_eq!(s.history.reasons, ["metrics_policy_not_accepted"]);
    assert_eq!(
        (s.history.retention_days, s.history.max_records),
        (30, 10_000)
    );
}

#[test]
fn the_projection_is_a_closed_set_of_fields_with_no_path_credential_or_content() {
    let v = serde_json::to_value(M3Service::inactive().state(true)).unwrap();
    assert_eq!(
        keys(&v),
        [
            "capability",
            "claude_install",
            "fixture",
            "history",
            "model_selection",
            "profiles",
            "version"
        ]
    );
    assert_eq!(
        keys(&v["history"]),
        [
            "can_enable",
            "enabled",
            "max_records",
            "reasons",
            "records",
            "retention_days",
            "state"
        ]
    );
    let text = v.to_string();
    for forbidden in ["/home/", "/tmp/", "sk-ant", "api_key", "password", "secret"] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn an_unverifiable_installation_is_reported_as_verifying_then_refused_with_a_code() {
    let mut install = InstallConfig::new(std::path::PathBuf::from("/nonexistent/venv"));
    install.policy_dir = std::path::PathBuf::from("/nonexistent/policy");
    let svc = M3Service::with(Some(install), None);
    assert_eq!(svc.state(false).claude_install, InstallView::Verifying);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match svc.state(false).claude_install {
            InstallView::Refused { reason } => {
                assert!(
                    [
                        "unsafe_path",
                        "sdk_not_configured",
                        "not_a_virtual_environment"
                    ]
                    .contains(&reason.as_str()),
                    "{reason}"
                );
                break;
            }
            InstallView::Verifying => assert!(std::time::Instant::now() < end, "never finished"),
            other => panic!("{other:?}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // The cards then carry the launcher's own reason beside the decisions that block.
    let s = svc.state(false);
    assert!(s.profiles[2].reasons.len() >= 3);
}

#[test]
fn a_fixture_source_is_projected_as_given() {
    struct One;
    impl FixtureSource for One {
        fn view(&self) -> FixtureView {
            FixtureView {
                label: "x",
                phase: "stopped",
                profile: None,
                conversation: vec![Message {
                    role: "user",
                    text: "hi".into(),
                }],
                turn: None,
                approvals: vec![],
                report: None,
                detail: None,
            }
        }
    }
    let s = M3Service::with(None, Some(Arc::new(One))).state(false);
    let f = s.fixture.unwrap();
    assert_eq!((f.phase, f.conversation.len()), ("stopped", 1));
}

/// The framework itself, with the access-control list the build resolved from the real
/// configuration and capabilities. The window reads the state and that is all it may reach:
/// another window gets the framework's refusal, and the fixture commands are granted to nobody in
/// the package (only the fixture binary adds that grant, at run time, to its own window).
mod acl {
    use serde_json::{Value, json};
    use tauri::{
        Manager, WebviewWindowBuilder,
        ipc::{CallbackFn, InvokeBody},
        test::{INVOKE_KEY, get_ipc_response, mock_builder},
        webview::InvokeRequest,
    };

    fn ask(
        app: &tauri::App<tauri::test::MockRuntime>,
        label: &str,
        cmd: &str,
    ) -> Result<tauri::ipc::InvokeResponseBody, Value> {
        let window = match app.get_webview_window(label) {
            Some(window) => window,
            None => WebviewWindowBuilder::new(app, label, Default::default())
                .build()
                .unwrap(),
        };
        get_ipc_response(
            &window,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(json!({})),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map_err(|e| serde_json::to_value(e).unwrap())
    }

    fn app() -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .manage(super::M3Service::inactive())
            .manage(crate::f8::gui::F8Gui::inactive())
            .invoke_handler(tauri::generate_handler![super::m3_state])
            .build(tauri::generate_context!())
            .expect("the application builds with the real configuration")
    }

    #[test]
    fn the_main_window_may_read_the_state_and_no_other_window_may() {
        let app = app();
        let state = ask(&app, "main", "m3_state").expect("main may read the state");
        let state = state.deserialize::<Value>().unwrap();
        assert_eq!(state["capability"], json!("m3"));
        let denied = ask(&app, "intruder", "m3_state").unwrap_err().to_string();
        assert!(
            denied.contains("not allowed") || denied.contains("not found"),
            "{denied}"
        );
    }

    #[test]
    fn the_package_grants_no_fixture_command() {
        let app = app();
        for command in [
            "m3_fixture_start",
            "m3_fixture_stop",
            "m3_fixture_prompt",
            "m3_fixture_interrupt",
            "m3_fixture_decide",
        ] {
            let refused = ask(&app, "main", command).unwrap_err().to_string();
            assert!(refused.contains("not allowed"), "{command}: {refused}");
        }
    }
}
