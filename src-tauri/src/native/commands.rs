//! The window's commands for native sessions. Every request names something the desktop
//! already holds (an agent from the closed catalogue, a session, a profile, a project the
//! user chose) or carries a profile's closed words. None carries an executable, an argument
//! list, an environment or a path (C-01): the folder of a project is only ever a folder the
//! native dialog returned.
use super::{
    catalogue::AgentId,
    profile::Body,
    profiles::Summary,
    service::{
        LauncherView, NativeError, NativeService, PreviewView, SessionView, StartReply,
        StartRequest,
    },
};
use crate::native_dialog::{choose_file_titled, choose_folder};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use wyss_console::terminal::{TerminalService, contracts::TerminalSize};

/// The service as the desktop holds it: `None` when there is no private state directory,
/// which the window is told instead of being shown an empty launcher.
pub struct Native {
    service: Option<NativeService>,
}

impl Native {
    pub fn start() -> Self {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute());
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.as_ref().map(|h| h.join(".local/state")))
            .map(|b| b.join("wyss-console").join("native"));
        let service = home
            .zip(base)
            .and_then(|(home, base)| NativeService::open(base, home, std::env::var_os("PATH")));
        Self { service }
    }

    fn service(&self) -> Result<&NativeService, NativeError> {
        self.service
            .as_ref()
            .ok_or_else(|| NativeError::new("unavailable"))
    }
}

fn well_formed(id: &str) -> Result<&str, NativeError> {
    (id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then_some(id)
    .ok_or_else(|| NativeError::new("request_invalid"))
}

/// A profile id: 32 hex digits, or a template's own word.
fn profile_id(id: &str) -> Result<&str, NativeError> {
    if id.starts_with("template-")
        && id.len() <= 40
        && id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
    {
        return Ok(id);
    }
    well_formed(id)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, NativeError> + Send + 'static,
) -> Result<T, NativeError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| NativeError::new("internal"))?
}

#[tauri::command]
pub async fn native_launcher(native: State<'_, Native>) -> Result<LauncherView, NativeError> {
    let service = native.service()?.clone();
    blocking(move || Ok(service.launcher())).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    agent_id: AgentId,
    profile_id: Option<String>,
}

#[tauri::command]
pub fn native_preview(
    native: State<'_, Native>,
    request: PreviewRequest,
) -> Result<PreviewView, NativeError> {
    let profile = request.profile_id.as_deref().map(profile_id).transpose()?;
    native.service()?.preview(request.agent_id, profile)
}

#[tauri::command]
pub async fn native_start(
    native: State<'_, Native>,
    terminal: State<'_, TerminalService>,
    request: StartRequest,
) -> Result<StartReply, NativeError> {
    let service = native.service()?.clone();
    let terminal = terminal.inner().clone();
    if let Some(id) = request.profile_id.as_deref() {
        profile_id(id)?;
    }
    blocking(move || service.start(&terminal, request)).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {
    sample: bool,
}

#[tauri::command]
pub async fn native_sessions(
    native: State<'_, Native>,
    terminal: State<'_, TerminalService>,
    request: ListRequest,
) -> Result<Vec<SessionView>, NativeError> {
    let service = native.service()?.clone();
    let terminal = terminal.inner().clone();
    blocking(move || Ok(service.sessions(&terminal, request.sample))).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    id: String,
}

#[tauri::command]
pub async fn native_stop(
    native: State<'_, Native>,
    terminal: State<'_, TerminalService>,
    request: SessionRequest,
) -> Result<(), NativeError> {
    let service = native.service()?.clone();
    let terminal = terminal.inner().clone();
    blocking(move || service.stop(&terminal, well_formed(&request.id)?)).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartRequest {
    id: String,
    size: TerminalSize,
}

#[tauri::command]
pub async fn native_restart(
    native: State<'_, Native>,
    terminal: State<'_, TerminalService>,
    request: RestartRequest,
) -> Result<StartReply, NativeError> {
    let service = native.service()?.clone();
    let terminal = terminal.inner().clone();
    blocking(move || service.restart(&terminal, well_formed(&request.id)?, request.size)).await
}

#[tauri::command]
pub fn native_forget(
    native: State<'_, Native>,
    request: SessionRequest,
) -> Result<(), NativeError> {
    native.service()?.forget(well_formed(&request.id)?)
}

#[derive(Debug, Clone, Serialize)]
pub struct Chosen {
    path: Option<String>,
}

/// The native folder dialog. What it returns is remembered for this run and is then the only
/// kind of folder besides the home and the known projects a start may name.
#[tauri::command]
pub async fn native_choose_project(
    app: AppHandle,
    native: State<'_, Native>,
) -> Result<Chosen, NativeError> {
    let service = native.service()?.clone();
    blocking(move || {
        let Some(path) = choose_folder(&app) else {
            return Ok(Chosen { path: None });
        };
        let text = path
            .into_os_string()
            .into_string()
            .ok()
            .filter(|t| t.starts_with('/') && t.len() <= 4096 && !t.chars().any(char::is_control))
            .ok_or_else(|| NativeError::new("project_invalid"))?;
        service.remember_chosen(text.clone());
        Ok(Chosen { path: Some(text) })
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultRequest {
    project: String,
    profile_id: Option<String>,
}

#[tauri::command]
pub fn native_project_default(
    native: State<'_, Native>,
    request: DefaultRequest,
) -> Result<(), NativeError> {
    let profile = request.profile_id.as_deref().map(profile_id).transpose()?;
    native
        .service()?
        .set_project_default(&request.project, profile.map(str::to_owned))
}

// ---- profiles -------------------------------------------------------------------------------

#[tauri::command]
pub fn native_profiles(native: State<'_, Native>) -> Result<Vec<Summary>, NativeError> {
    Ok(native.service()?.profiles().list())
}

#[derive(Debug, Serialize)]
pub struct Created {
    id: String,
}

#[tauri::command]
pub fn native_profile_create(
    native: State<'_, Native>,
    body: Body,
) -> Result<Created, NativeError> {
    Ok(Created {
        id: native.service()?.profiles().create(body)?,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    id: String,
    body: Body,
}

#[derive(Debug, Serialize)]
pub struct Saved {
    version: u32,
}

#[tauri::command]
pub fn native_profile_save(
    native: State<'_, Native>,
    request: SaveRequest,
) -> Result<Saved, NativeError> {
    let id = profile_id(&request.id)?;
    Ok(Saved {
        version: native.service()?.profiles().save(id, request.body)?,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileRequest {
    id: String,
}

#[tauri::command]
pub fn native_profile_delete(
    native: State<'_, Native>,
    request: ProfileRequest,
) -> Result<(), NativeError> {
    Ok(native
        .service()?
        .profiles()
        .delete(profile_id(&request.id)?)?)
}

#[tauri::command]
pub fn native_profile_duplicate(
    native: State<'_, Native>,
    request: ProfileRequest,
) -> Result<Created, NativeError> {
    Ok(Created {
        id: native
            .service()?
            .profiles()
            .duplicate(profile_id(&request.id)?)?,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    id: String,
    version: u32,
}

#[tauri::command]
pub fn native_profile_restore(
    native: State<'_, Native>,
    request: RestoreRequest,
) -> Result<(), NativeError> {
    Ok(native
        .service()?
        .profiles()
        .restore(profile_id(&request.id)?, request.version)?)
}

#[derive(Debug, Serialize)]
pub struct Done {
    done: bool,
}

#[tauri::command]
pub async fn native_profile_export(
    app: AppHandle,
    native: State<'_, Native>,
    request: ProfileRequest,
) -> Result<Done, NativeError> {
    let service = native.service()?.clone();
    blocking(move || {
        let bytes = service.profiles().export(profile_id(&request.id)?)?;
        let Some(path) = choose_file_titled(&app, true, "wyss-profile.json", "Export profile")
        else {
            return Ok(Done { done: false });
        };
        crate::configuration::write_private_file(&path, &bytes)
            .map_err(|()| NativeError::new("export_failed"))?;
        Ok(Done { done: true })
    })
    .await
}

#[tauri::command]
pub async fn native_profile_import(
    app: AppHandle,
    native: State<'_, Native>,
) -> Result<Option<Created>, NativeError> {
    let service = native.service()?.clone();
    blocking(move || {
        let Some(path) = choose_file_titled(&app, false, "", "Import profile") else {
            return Ok(None);
        };
        let bytes = crate::configuration::read_small_file(&path)
            .map_err(|()| NativeError::new("profile_import_invalid"))?;
        Ok(Some(Created {
            id: service.profiles().import(&bytes)?,
        }))
    })
    .await
}

const COMMANDS: &[&str] = &[
    "native_launcher",
    "native_preview",
    "native_start",
    "native_sessions",
    "native_stop",
    "native_restart",
    "native_forget",
    "native_choose_project",
    "native_project_default",
    "native_profiles",
    "native_profile_create",
    "native_profile_save",
    "native_profile_delete",
    "native_profile_duplicate",
    "native_profile_restore",
    "native_profile_export",
    "native_profile_import",
];

pub fn extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: COMMANDS,
        handler: Box::new(tauri::generate_handler![
            native_launcher,
            native_preview,
            native_start,
            native_sessions,
            native_stop,
            native_restart,
            native_forget,
            native_choose_project,
            native_project_default,
            native_profiles,
            native_profile_create,
            native_profile_save,
            native_profile_delete,
            native_profile_duplicate,
            native_profile_restore,
            native_profile_export,
            native_profile_import
        ]),
        install: Box::new(|builder| builder.manage(Native::start())),
    }
}
