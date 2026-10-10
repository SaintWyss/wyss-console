use crate::{application, contracts::AppInfo, terminal::TerminalService};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::Manager;
mod authority;
mod events;
mod glass;
mod lifecycle;
mod sessions;
mod terminal;
mod turns;
mod workspace;

#[tauri::command]
fn get_app_info() -> AppInfo {
    application::app_info().into()
}

/// The main window is created here, not from config, because transparency is
/// decided before the window exists. A transparent window the display refuses
/// falls back to the opaque one.
fn open_main_window(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or("main window configuration is missing")?;
    let planned = glass::plan(glass::probe());
    let (window, mode) = match build_main_window(app, config, planned) {
        Ok(window) => (window, planned),
        Err(_) if planned.transparent() => (
            build_main_window(app, config, glass::GlassMode::Opaque)?,
            glass::GlassMode::Opaque,
        ),
        Err(error) => return Err(error.into()),
    };
    glass::apply(&window, mode);
    Ok(())
}

fn build_main_window(
    app: &tauri::App,
    config: &tauri::utils::config::WindowConfig,
    mode: glass::GlassMode,
) -> tauri::Result<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::from_config(app, config)?
        .transparent(mode.transparent())
        .initialization_script(mode.bootstrap_script())
        .build()
}

pub fn run(authority_digest: Option<&'static str>) {
    let stopping = Arc::new(AtomicBool::new(false));
    let managed_sessions = sessions::SessionService::from_environment();
    let authority = authority::AuthorityService::from_environment();
    // Makes an authority available where none answers. It follows the observer above
    // and never stands in for it: connected is what the observer says.
    let bootstrap = lifecycle::start(&authority, authority_digest);
    tauri::Builder::default()
        .setup(|app| open_main_window(app))
        .manage(TerminalService::default())
        // Read-only and independent: no terminal operation waits on, or learns of, it.
        .manage(authority)
        .manage(bootstrap)
        // Its own short-lived connections: the observer above is never borrowed.
        // Turns send their intents through the session service and stop with it.
        .manage(turns::TurnService::new(managed_sessions.clone()))
        // History: reads only, on connections of its own. It holds nothing between two.
        .manage(events::EventService::new(managed_sessions.clone()))
        .manage(managed_sessions)
        // What the native folder dialog returned: the only paths an activation may name.
        .manage(workspace::WorkspaceSelections::default())
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            authority::authority_state,
            lifecycle::authority_lifecycle,
            lifecycle::authority_retry,
            sessions::managed_sessions_page,
            sessions::managed_session_prepare,
            sessions::managed_session_resume,
            sessions::managed_session_close,
            sessions::managed_session_activate,
            sessions::managed_session_abandon,
            sessions::managed_session_mode,
            workspace::choose_workspace,
            turns::managed_turns,
            turns::managed_turn_read,
            turns::managed_turn_start,
            turns::managed_turn_cancel,
            turns::managed_approvals,
            turns::managed_approval_read,
            turns::managed_approval_approve,
            turns::managed_approval_deny,
            events::managed_events,
            events::managed_events_older,
            terminal::create_terminal,
            terminal::terminal_write,
            terminal::terminal_read_output,
            terminal::terminal_resize,
            terminal::close_terminal,
            terminal::terminal_status
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Wyss Console")
        .run(move |handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event
                && !stopping.swap(true, Ordering::AcqRel)
            {
                api.prevent_exit();
                let service = handle.state::<TerminalService>().inner().clone();
                let authority = handle
                    .state::<authority::AuthorityService>()
                    .inner()
                    .clone();
                let bootstrap = handle
                    .state::<lifecycle::AuthorityBootstrap>()
                    .inner()
                    .clone();
                let sessions = handle.state::<sessions::SessionService>().inner().clone();
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let result = tauri::async_runtime::spawn_blocking(move || {
                        // A session call in flight is cancelled; none is admitted after.
                        sessions.shutdown();
                        // Terminals first: the link's bounded stop never delays them.
                        let result = service.shutdown();
                        authority.shutdown();
                        // Only the following ends. The authority is not stopped: its
                        // lifetime is the service manager's, not this window's.
                        bootstrap.shutdown();
                        result
                    })
                    .await;
                    let code = if matches!(result, Ok(Ok(()))) { 0 } else { 1 };
                    app.exit(code);
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::get_app_info;

    #[test]
    fn command_serializes_exact_app_info_v1_contract() {
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/app-info-v1.json")).unwrap();
        let actual = serde_json::to_value(get_app_info()).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual["version"], env!("CARGO_PKG_VERSION"));
    }
}
