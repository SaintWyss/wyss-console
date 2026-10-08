use crate::{application, contracts::AppInfo, terminal::TerminalService};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::Manager;
mod glass;
mod terminal;

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

pub fn run() {
    let stopping = Arc::new(AtomicBool::new(false));
    tauri::Builder::default()
        .setup(|app| open_main_window(app))
        .manage(TerminalService::default())
        .invoke_handler(tauri::generate_handler![
            get_app_info,
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
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let result =
                        tauri::async_runtime::spawn_blocking(move || service.shutdown()).await;
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
