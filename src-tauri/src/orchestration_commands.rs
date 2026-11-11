//! Desktop IPC for the optional, owner-started OL3 coordinator.
//! Every effect still goes through the private coordinator socket and F8 authority.
use serde_json::Value;
use wyss_console::orchestration::daemon::{self, DaemonError, DaemonRequest};

#[tauri::command]
async fn orchestration_call(request: DaemonRequest) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || daemon::call(&request))
        .await
        .map_err(|_| error_code(DaemonError::Unavailable))?
        .map_err(error_code)
}

fn error_code(error: DaemonError) -> String {
    serde_json::to_value(error)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unavailable".to_owned())
}

const COMMANDS: &[&str] = &["orchestration_call"];

pub fn extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: COMMANDS,
        handler: Box::new(tauri::generate_handler![orchestration_call]),
        install: Box::new(|builder| builder),
        on_exit: None,
    }
}

/// ENG-01. The framework's own mediation, with the access-control list the build resolved from
/// the real manifest and capability files: nothing here is a mocked `invoke`. No coordinator is
/// started: a request the command would forward is never sent, because every case stops either at
/// the framework's refusal or at the closed request shape.
#[cfg(test)]
mod acl {
    use super::orchestration_call;
    use serde_json::{Value, json};
    use tauri::{
        Manager, WebviewWindowBuilder,
        ipc::{CallbackFn, InvokeBody},
        test::{INVOKE_KEY, get_ipc_response, mock_builder},
        webview::InvokeRequest,
    };

    fn app() -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .invoke_handler(tauri::generate_handler![orchestration_call])
            .build(tauri::generate_context!())
            .expect("the application builds with the real configuration")
    }

    fn ask(app: &tauri::App<tauri::test::MockRuntime>, label: &str, body: Value) -> String {
        let window = match app.get_webview_window(label) {
            Some(window) => window,
            None => WebviewWindowBuilder::new(app, label, Default::default())
                .build()
                .unwrap(),
        };
        get_ipc_response(
            &window,
            InvokeRequest {
                cmd: "orchestration_call".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|_| "answered".to_owned())
        .unwrap_or_else(|error| serde_json::to_value(error).unwrap().to_string())
    }

    /// A request outside the closed set: the command's own argument check refuses it, which
    /// only happens once the framework has let the call through.
    fn outside_the_closed_set() -> Value {
        json!({"request": {"method": "run_shell", "params": {"argv": ["id"]}}})
    }

    #[test]
    fn the_main_window_reaches_the_command_and_its_closed_request_shape() {
        let app = app();
        let refused = ask(&app, "main", outside_the_closed_set());
        assert!(!refused.contains("not allowed"), "{refused}");
        assert!(
            refused.contains("invalid args") && refused.contains("orchestration_call"),
            "the command's own argument check answers main: {refused}"
        );
        // A known method with a field it does not have is refused the same way.
        let extra = ask(
            &app,
            "main",
            json!({"request": {"method": "list", "params": {"shell": "id"}}}),
        );
        assert!(extra.contains("invalid args"), "{extra}");
    }

    #[test]
    fn a_project_window_and_any_other_window_are_refused_before_the_command() {
        let app = app();
        for label in ["w-1", "w-0f3a", "intruder"] {
            // Both a request the command would refuse and one it would forward stop at the
            // framework: the answer is its refusal, never the command's argument check.
            for body in [
                outside_the_closed_set(),
                json!({"request": {"method": "list"}}),
            ] {
                let denied = ask(&app, label, body);
                assert!(denied.contains("not allowed"), "{label}: {denied}");
                assert!(!denied.contains("invalid args"), "{label}: {denied}");
            }
        }
    }

    #[test]
    fn a_project_window_keeps_what_its_own_capability_grants() {
        // The refusal above is about this command, not about the window being unknown: the
        // `projects` capability covers `w-*`, and grants no orchestration permission.
        let projects: Value =
            serde_json::from_str(include_str!("../capabilities/projects.json")).unwrap();
        assert_eq!(projects["windows"], json!(["main", "w-*"]));
        assert!(
            projects["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|permission| permission != "allow-orchestration-call")
        );
    }
}
