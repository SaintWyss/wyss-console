use crate::{application, contracts::AppInfo, terminal::TerminalService};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::Manager;
mod authority;
mod events;
#[cfg(target_os = "linux")]
mod f8;
mod glass;
mod lifecycle;
#[cfg(target_os = "linux")]
pub mod select;
mod sessions;
mod settings;
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

/// What the binary adds to the desktop: commands and the state they need. They live in the
/// binary because the code behind them must not be in the library the authority links
/// (C-02, TS-SEC-03). The library only routes: a command named in `commands` goes to
/// `handler`, every other one to the library's own.
/// Called when the application exits.
pub type ExitHook = Box<dyn Fn(&tauri::AppHandle) + Send + Sync + 'static>;

pub struct Extension {
    pub commands: &'static [&'static str],
    pub handler: Box<dyn Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static>,
    pub install: Box<dyn FnOnce(tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry>>,
    /// Called when the application is exiting, after its terminals were asked to stop.
    pub on_exit: Option<ExitHook>,
}

impl Extension {
    /// Two extensions as one: a command goes to the extension that names it.
    pub fn merge(self, other: Self) -> Self {
        let commands: Vec<&'static str> = self
            .commands
            .iter()
            .chain(other.commands.iter())
            .copied()
            .collect();
        let first = self.commands;
        let (first_handler, second_handler) = (self.handler, other.handler);
        let (first_install, second_install) = (self.install, other.install);
        let on_exit: Option<ExitHook> = match (self.on_exit, other.on_exit) {
            (Some(a), Some(b)) => Some(Box::new(move |app| {
                a(app);
                b(app);
            })),
            (a, b) => a.or(b),
        };
        Self {
            commands: Box::leak(commands.into_boxed_slice()),
            handler: Box::new(move |invoke| {
                if first.contains(&invoke.message.command()) {
                    first_handler(invoke)
                } else {
                    second_handler(invoke)
                }
            }),
            install: Box::new(move |builder| second_install(first_install(builder))),
            on_exit,
        }
    }
}

/// The protocol-2 service the desktop manages. Inactive unless a caller owning a connector
/// says otherwise; the installed entry points never do.
#[cfg(target_os = "linux")]
pub type F8 = crate::f8::gui::F8Gui;
#[cfg(not(target_os = "linux"))]
pub type F8 = ();
pub fn inactive_f8() -> F8 {
    #[cfg(target_os = "linux")]
    {
        crate::f8::gui::F8Gui::inactive()
    }
}

pub fn run(authority_digest: Option<&'static str>, extension: Option<Extension>, f8_gui: F8) {
    let stopping = Arc::new(AtomicBool::new(false));
    let managed_sessions = sessions::SessionService::from_environment();
    let authority = authority::AuthorityService::from_environment();
    // The desktop's own preferences: read once here, kept in memory, written on change.
    let preferences = settings::SettingsService::from_environment();
    // Makes an authority available where none answers. It follows the observer above
    // and never stands in for it: connected is what the observer says.
    let bootstrap = lifecycle::start(&authority, authority_digest);
    // The confirmation of what lets a managed runtime act: the toolkit's own window, which
    // learns of the application once it exists.
    #[cfg(target_os = "linux")]
    let gtk_confirm = Arc::new(f8::GtkConfirmer::default());
    #[cfg(target_os = "linux")]
    let f8_confirm = f8::F8Confirm::new(gtk_confirm.clone());
    #[cfg(not(target_os = "linux"))]
    let (gtk_confirm, f8_confirm) = ((), ());
    type Handler = Box<dyn Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync>;
    let ours: Handler = Box::new(tauri::generate_handler![
        get_app_info,
        authority::authority_state,
        lifecycle::authority_lifecycle,
        lifecycle::authority_retry,
        sessions::managed_sessions_page,
        sessions::managed_model_catalogue,
        sessions::managed_session_prepare,
        sessions::managed_session_resume,
        sessions::managed_session_close,
        sessions::managed_session_activate,
        sessions::managed_session_abandon,
        sessions::managed_session_mode,
        workspace::choose_workspace,
        turns::managed_turns,
        turns::managed_turns_older,
        turns::managed_turn_read,
        turns::managed_turn_start,
        turns::managed_turn_cancel,
        turns::managed_approvals,
        turns::managed_approval_read,
        turns::managed_approval_approve,
        turns::managed_approval_deny,
        events::managed_events,
        events::managed_events_older,
        #[cfg(target_os = "linux")]
        f8::f8_state,
        #[cfg(target_os = "linux")]
        f8::f8_reconnect,
        #[cfg(target_os = "linux")]
        f8::f8_sessions,
        #[cfg(target_os = "linux")]
        f8::f8_session,
        #[cfg(target_os = "linux")]
        f8::f8_session_prepare,
        #[cfg(target_os = "linux")]
        f8::f8_session_close,
        #[cfg(target_os = "linux")]
        f8::f8_runtime_start,
        #[cfg(target_os = "linux")]
        f8::f8_runtime_stop,
        #[cfg(target_os = "linux")]
        f8::f8_models,
        #[cfg(target_os = "linux")]
        f8::f8_lease_grant,
        #[cfg(target_os = "linux")]
        f8::f8_lease_renew,
        #[cfg(target_os = "linux")]
        f8::f8_lease_revoke,
        #[cfg(target_os = "linux")]
        f8::f8_binding_create,
        #[cfg(target_os = "linux")]
        f8::f8_binding_resume,
        #[cfg(target_os = "linux")]
        f8::f8_binding_abandon,
        #[cfg(target_os = "linux")]
        f8::f8_turn_start,
        #[cfg(target_os = "linux")]
        f8::f8_turn_cancel,
        #[cfg(target_os = "linux")]
        f8::f8_turn_read,
        #[cfg(target_os = "linux")]
        f8::f8_approval_read,
        #[cfg(target_os = "linux")]
        f8::f8_approval_accept,
        #[cfg(target_os = "linux")]
        f8::f8_approval_decline,
        #[cfg(target_os = "linux")]
        f8::f8_approval_cancel,
        #[cfg(target_os = "linux")]
        f8::f8_audit,
        #[cfg(target_os = "linux")]
        f8::f8_audit_wait,
        settings::settings_read,
        settings::settings_update,
        settings::settings_reset,
        terminal::create_terminal,
        terminal::terminal_write,
        terminal::terminal_read_output,
        terminal::terminal_resize,
        terminal::close_terminal,
        terminal::terminal_status
    ]);
    let (routed, theirs, install, on_exit) = match extension {
        Some(extension) => (
            extension.commands,
            Some(extension.handler),
            Some(extension.install),
            extension
                .on_exit
                .map(|f| Arc::<dyn Fn(&tauri::AppHandle) + Send + Sync>::from(f)),
        ),
        None => (&[][..], None, None, None),
    };
    let builder = tauri::Builder::default();
    let builder = match install {
        Some(install) => install(builder),
        None => builder,
    };
    builder
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            gtk_confirm.attach(app.handle().clone());
            #[cfg(not(target_os = "linux"))]
            let _ = &gtk_confirm;
            open_main_window(app)
        })
        .manage(TerminalService::default())
        // Inactive unless the command line named an authority (`select`): the window cannot.
        .manage(f8_gui)
        .manage(f8_confirm)
        // Read-only and independent: no terminal operation waits on, or learns of, it.
        .manage(authority)
        .manage(bootstrap)
        .manage(preferences)
        // Its own short-lived connections: the observer above is never borrowed.
        // Turns send their intents through the session service and stop with it.
        .manage(turns::TurnService::new(managed_sessions.clone()))
        // History: reads only, on connections of its own. It holds nothing between two.
        .manage(events::EventService::new(managed_sessions.clone()))
        .manage(managed_sessions)
        // What the native folder dialog returned: the only paths an activation may name.
        .manage(workspace::WorkspaceSelections::default())
        .invoke_handler(move |invoke| match &theirs {
            Some(theirs) if routed.contains(&invoke.message.command()) => theirs(invoke),
            _ => ours(invoke),
        })
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
                #[cfg(target_os = "linux")]
                let f8_gui = handle.state::<F8>().inner().clone();
                let app = handle.clone();
                let on_exit = on_exit.clone();
                let for_exit = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let result = tauri::async_runtime::spawn_blocking(move || {
                        // A session call in flight is cancelled; none is admitted after.
                        sessions.shutdown();
                        #[cfg(target_os = "linux")]
                        f8_gui.shutdown();
                        // Terminals first: the link's bounded stop never delays them.
                        let result = service.shutdown();
                        if let Some(on_exit) = &on_exit {
                            on_exit(&for_exit);
                        }
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
