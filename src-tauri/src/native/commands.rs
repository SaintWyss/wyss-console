//! The window's commands for native sessions. Every request names something the host already
//! holds (an agent from the closed catalogue, a session, a profile, a project the user chose) or
//! carries a profile's closed words. None carries an executable, an argument list, an
//! environment or a path (C-01): the folder of a project is only ever a folder the native
//! dialog returned. The sessions themselves live in the host (F9): these commands only
//! validate the request and pass it on, and the answer comes back as the host made it.
use super::{catalogue::AgentId, profile::Body, service::StartRequest};
use crate::host::{client::HostLink, protocol::HostError};
use crate::native_dialog::{choose_file_titled, choose_folder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, State};
use wyss_console::terminal::contracts::TerminalSize;

fn well_formed(id: &str) -> Result<&str, HostError> {
    (id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then_some(id)
    .ok_or_else(|| HostError::new("request_invalid"))
}

/// A profile id: 32 hex digits, or a template's own word.
fn profile_id(id: &str) -> Result<&str, HostError> {
    if id.starts_with("template-")
        && id.len() <= 40
        && id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
    {
        return Ok(id);
    }
    well_formed(id)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, HostError> + Send + 'static,
) -> Result<T, HostError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| HostError::new("internal"))?
}

/// The host's answer, for the page to check against its own contract.
type Answer = Result<Value, HostError>;

#[tauri::command]
pub async fn native_launcher(link: State<'_, HostLink>) -> Answer {
    let link = link.inner().clone();
    blocking(move || link.call("native_launcher", Value::Null)).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    agent_id: AgentId,
    profile_id: Option<String>,
}

#[tauri::command]
pub async fn native_preview(link: State<'_, HostLink>, request: PreviewRequest) -> Answer {
    if let Some(id) = request.profile_id.as_deref() {
        profile_id(id)?;
    }
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "native_preview",
            json!({"agent_id": request.agent_id, "profile_id": request.profile_id}),
        )
    })
    .await
}

#[tauri::command]
pub async fn native_start(link: State<'_, HostLink>, request: StartRequest) -> Answer {
    if let Some(id) = request.profile_id.as_deref() {
        profile_id(id)?;
    }
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "native_start",
            json!({
                "agent_id": request.agent_id,
                "project": request.project,
                "profile_id": request.profile_id,
                "size": request.size,
                "brief": request.brief,
            }),
        )
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {
    sample: bool,
}

#[tauri::command]
pub async fn native_sessions(link: State<'_, HostLink>, request: ListRequest) -> Answer {
    let link = link.inner().clone();
    blocking(move || link.call("native_sessions", json!({"sample": request.sample}))).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    id: String,
}

#[tauri::command]
pub async fn native_stop(link: State<'_, HostLink>, request: SessionRequest) -> Answer {
    well_formed(&request.id)?;
    let link = link.inner().clone();
    blocking(move || link.call("native_stop", json!({"id": request.id}))).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartRequest {
    id: String,
    size: TerminalSize,
}

#[tauri::command]
pub async fn native_restart(link: State<'_, HostLink>, request: RestartRequest) -> Answer {
    well_formed(&request.id)?;
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "native_restart",
            json!({"id": request.id, "size": request.size}),
        )
    })
    .await
}

#[tauri::command]
pub async fn native_forget(link: State<'_, HostLink>, request: SessionRequest) -> Answer {
    well_formed(&request.id)?;
    let link = link.inner().clone();
    blocking(move || link.call("native_forget", json!({"id": request.id}))).await
}

#[derive(Debug, Clone, Serialize)]
pub struct Chosen {
    path: Option<String>,
}

/// The native folder dialog. What it returns is remembered by the host for its lifetime and
/// is then the only kind of folder besides the home and the known projects a start may name.
#[tauri::command]
pub async fn native_choose_project(
    app: AppHandle,
    link: State<'_, HostLink>,
) -> Result<Chosen, HostError> {
    let link = link.inner().clone();
    blocking(move || {
        let Some(path) = choose_folder(&app) else {
            return Ok(Chosen { path: None });
        };
        let text = path
            .into_os_string()
            .into_string()
            .ok()
            .filter(|t| t.starts_with('/') && t.len() <= 4096 && !t.chars().any(char::is_control))
            .ok_or_else(|| HostError::new("project_invalid"))?;
        link.call("native_remember_project", json!({"path": text}))?;
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
pub async fn native_project_default(link: State<'_, HostLink>, request: DefaultRequest) -> Answer {
    if let Some(id) = request.profile_id.as_deref() {
        profile_id(id)?;
    }
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "native_project_default",
            json!({"project": request.project, "profile_id": request.profile_id}),
        )
    })
    .await
}

// ---- profiles -------------------------------------------------------------------------------

#[tauri::command]
pub async fn native_profiles(link: State<'_, HostLink>) -> Answer {
    let link = link.inner().clone();
    blocking(move || link.call("profiles", Value::Null)).await
}

#[tauri::command]
pub async fn native_profile_create(link: State<'_, HostLink>, body: Body) -> Answer {
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "profile_create",
            serde_json::to_value(body).map_err(|_| HostError::new("internal"))?,
        )
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    id: String,
    body: Body,
}

#[tauri::command]
pub async fn native_profile_save(link: State<'_, HostLink>, request: SaveRequest) -> Answer {
    profile_id(&request.id)?;
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "profile_save",
            json!({"id": request.id, "body": request.body}),
        )
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileRequest {
    id: String,
}

#[tauri::command]
pub async fn native_profile_delete(link: State<'_, HostLink>, request: ProfileRequest) -> Answer {
    profile_id(&request.id)?;
    let link = link.inner().clone();
    blocking(move || link.call("profile_delete", json!({"id": request.id}))).await
}

#[tauri::command]
pub async fn native_profile_duplicate(
    link: State<'_, HostLink>,
    request: ProfileRequest,
) -> Answer {
    profile_id(&request.id)?;
    let link = link.inner().clone();
    blocking(move || link.call("profile_duplicate", json!({"id": request.id}))).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    id: String,
    version: u32,
}

#[tauri::command]
pub async fn native_profile_restore(link: State<'_, HostLink>, request: RestoreRequest) -> Answer {
    profile_id(&request.id)?;
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "profile_restore",
            json!({"id": request.id, "version": request.version}),
        )
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct Done {
    done: bool,
}

#[tauri::command]
pub async fn native_profile_export(
    app: AppHandle,
    link: State<'_, HostLink>,
    request: ProfileRequest,
) -> Result<Done, HostError> {
    profile_id(&request.id)?;
    let link = link.inner().clone();
    blocking(move || {
        let exported = link.call("profile_export", json!({"id": request.id}))?;
        let text = exported["text"].as_str().unwrap_or_default().to_owned();
        let Some(path) = choose_file_titled(&app, true, "wyss-profile.json", "Export profile")
        else {
            return Ok(Done { done: false });
        };
        crate::configuration::write_private_file(&path, text.as_bytes())
            .map_err(|()| HostError::new("export_failed"))?;
        Ok(Done { done: true })
    })
    .await
}

#[tauri::command]
pub async fn native_profile_import(app: AppHandle, link: State<'_, HostLink>) -> Answer {
    let link = link.inner().clone();
    blocking(move || {
        let Some(path) = choose_file_titled(&app, false, "", "Import profile") else {
            return Ok(Value::Null);
        };
        let bytes = crate::configuration::read_small_file(&path)
            .map_err(|()| HostError::new("profile_import_invalid"))?;
        let text =
            String::from_utf8(bytes).map_err(|_| HostError::new("profile_import_invalid"))?;
        link.call("profile_import", json!({"text": text}))
    })
    .await
}

/// The brief for handing a session's work to another agent, to be reviewed. Starts nothing.
#[tauri::command]
pub async fn native_handoff_prepare(link: State<'_, HostLink>, request: SessionRequest) -> Answer {
    well_formed(&request.id)?;
    let link = link.inner().clone();
    blocking(move || link.call("handoff_prepare", json!({"id": request.id}))).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveBriefRequest {
    text: String,
}

/// The reviewed brief written to a file the user names in the native chooser.
#[tauri::command]
pub async fn native_handoff_save(
    app: AppHandle,
    request: SaveBriefRequest,
) -> Result<Done, HostError> {
    if request.text.len() > super::service::MAX_BRIEF {
        return Err(HostError::new("brief_invalid"));
    }
    blocking(move || {
        let Some(path) = choose_file_titled(&app, true, "handoff.md", "Save handoff brief") else {
            return Ok(Done { done: false });
        };
        crate::configuration::write_private_file(&path, request.text.as_bytes())
            .map_err(|()| HostError::new("export_failed"))?;
        Ok(Done { done: true })
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
    "native_handoff_prepare",
    "native_handoff_save",
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
            native_profile_import,
            native_handoff_prepare,
            native_handoff_save
        ]),
        // The state they use is the host link, made by the terminal commands' extension.
        install: Box::new(|builder| builder),
        on_exit: None,
    }
}
