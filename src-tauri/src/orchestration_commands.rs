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
