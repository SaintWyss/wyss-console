//! The window's commands for native sessions. Every request names something the host already
//! holds (an agent from the closed catalogue, a session, a profile, a project the user chose) or
//! carries a profile's closed words. None carries an executable, an argument list, an
//! environment or a path (C-01): the folder of a project is only ever a folder the native
//! dialog returned. The sessions themselves live in the host (F9): these commands only
//! validate the request and pass it on, and the answer comes back as the host made it.
use super::{
    capabilities::{self, Evidence, Status},
    catalogue::{self, AgentId},
    profile::Body,
    service::StartRequest,
};
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

fn agent_word(agent: AgentId) -> &'static str {
    match agent {
        AgentId::ClaudeCode => "claude_code",
        AgentId::Codex => "codex",
    }
}

fn launcher_agent(link: &HostLink, agent: AgentId) -> Result<Value, HostError> {
    let launcher = link.call("native_launcher", Value::Null)?;
    launcher["agents"]
        .as_array()
        .and_then(|agents| {
            agents
                .iter()
                .find(|entry| entry["agent_id"].as_str() == Some(agent_word(agent)))
        })
        .cloned()
        .ok_or_else(|| HostError::new("agent_not_installed"))
}

fn current_evidence(
    link: &HostLink,
    agent: AgentId,
) -> Result<(Evidence, Option<String>), HostError> {
    let entry = launcher_agent(link, agent)?;
    let version = entry["path"]
        .as_str()
        .and_then(|path| catalogue::probe_version(std::path::Path::new(path)));
    let evidence = capabilities::evidence(agent, version.as_deref());
    Ok((evidence, version))
}

fn effectful_preview(preview: &Value) -> bool {
    ["applied", "with_cost"].iter().any(|key| {
        preview[*key]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    })
}

fn profile_requires_effort_from_list(profiles: &Value, profile_id: &str) -> Option<bool> {
    let profile = profiles
        .as_array()?
        .iter()
        .find(|profile| profile["id"].as_str() == Some(profile_id))?;
    Some(
        profile["body"]["effort"].as_str().is_some()
            || profile["from"]["effort"].as_str().is_some(),
    )
}

fn profile_requires_effort(link: &HostLink, profile_id: &str) -> Result<bool, HostError> {
    let profiles = link.call("profiles", Value::Null)?;
    profile_requires_effort_from_list(&profiles, profile_id)
        .ok_or_else(|| HostError::new("profile_unknown"))
}

fn push_string(value: &mut Value, field: &str, text: String) {
    if !value[field].is_array() {
        value[field] = Value::Array(Vec::new());
    }
    if let Some(items) = value[field].as_array_mut() {
        items.push(Value::String(text));
    }
}

fn preview_capability_message(evidence: Evidence, version: Option<&str>) -> String {
    match evidence.status {
        Status::SyntaxVerified => format!(
            "Profile override syntax is verified for {}. This does not prove provider-resolved or effective use.",
            version.unwrap_or("the reported build")
        ),
        Status::Unverified => format!(
            "Profile overrides are not verified for {}. Choose As configured to launch the Native CLI without overrides.",
            version.unwrap_or("an unknown provider version")
        ),
    }
}

fn check_start_capability(
    link: &HostLink,
    agent: AgentId,
    profile_id: Option<&str>,
    plan_token: Option<&str>,
    expected_cli_version: Option<&str>,
) -> Result<(), HostError> {
    let Some(profile_id) = profile_id else {
        return Ok(());
    };
    let preview = link.call(
        "native_preview",
        json!({"agent_id": agent, "profile_id": profile_id}),
    )?;
    if link.has_feature("native_profile_plan_token") {
        let actual_token = preview["plan_token"]
            .as_str()
            .ok_or_else(|| HostError::new("host_outdated"))?;
        if plan_token != Some(actual_token) {
            return Err(HostError::new("profile_preview_stale"));
        }
    } else if profile_requires_effort(link, profile_id)? {
        // A legacy host can still resolve and launch fields it already knew, but cannot pin
        // an effort-bearing plan to the preview or guard the shell-resolved CLI build.
        return Err(HostError::new("host_outdated"));
    }
    if !effectful_preview(&preview) {
        return Ok(());
    }
    let (_, version) = current_evidence(link, agent)?;
    if version.as_deref() != expected_cli_version
        || !capabilities::allows_start_with_profile(agent, expected_cli_version, true)
    {
        return Err(HostError::new("provider_version_unverified"));
    }
    Ok(())
}

fn annotate_launcher(mut launcher: Value) -> Result<Value, HostError> {
    let agents = launcher["agents"]
        .as_array_mut()
        .ok_or_else(|| HostError::new("internal"))?;
    for entry in agents {
        let agent = serde_json::from_value::<AgentId>(entry["agent_id"].clone())
            .map_err(|_| HostError::new("internal"))?;
        let version = entry["path"]
            .as_str()
            .and_then(|path| catalogue::probe_version(std::path::Path::new(path)));
        entry["version"] = version.clone().map_or(Value::Null, Value::String);
        let evidence = capabilities::evidence(agent, version.as_deref());
        entry["profile_capability"] =
            serde_json::to_value(evidence).map_err(|_| HostError::new("internal"))?;
    }
    Ok(launcher)
}

fn annotate_preview(
    link: &HostLink,
    mut preview: Value,
    agent: AgentId,
    profile_id: Option<&str>,
) -> Result<Value, HostError> {
    let (evidence, version) = current_evidence(link, agent)?;
    push_string(
        &mut preview,
        "warnings",
        preview_capability_message(evidence, version.as_deref()),
    );
    preview["provider_version"] = version.clone().map_or(Value::Null, Value::String);
    if profile_id.is_some()
        && link.has_feature("native_profile_plan_token")
        && preview["plan_token"].as_str().is_none()
    {
        preview["can_start"] = Value::Bool(false);
        push_string(
            &mut preview,
            "warnings",
            "The Native host cannot bind profile preview to start. Update the host before using profiles.".to_owned(),
        );
        return Ok(preview);
    }
    if let Some(profile_id) = profile_id
        && !link.has_feature("native_profile_plan_token")
    {
        if profile_requires_effort(link, profile_id)? {
            preview["can_start"] = Value::Bool(false);
            push_string(
                &mut preview,
                "warnings",
                "This Native host cannot bind an effort-bearing profile to its preview. Update the host before starting it.".to_owned(),
            );
            return Ok(preview);
        }
        push_string(
            &mut preview,
            "warnings",
            "Legacy Native host: it cannot bind this profile to the displayed preview revision and may apply the latest saved version at start.".to_owned(),
        );
    }
    if profile_id.is_some()
        && !capabilities::allows_start_with_profile(
            agent,
            version.as_deref(),
            effectful_preview(&preview),
        )
    {
        preview["can_start"] = Value::Bool(false);
        push_string(
            &mut preview,
            "not_applicable",
            "Profile overrides are refused for an unverified provider build.".to_owned(),
        );
    }
    Ok(preview)
}

#[tauri::command]
pub async fn native_launcher(link: State<'_, HostLink>) -> Answer {
    let link = link.inner().clone();
    blocking(move || annotate_launcher(link.call("native_launcher", Value::Null)?)).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    agent_id: AgentId,
    profile_id: Option<String>,
}

fn preview_with_link(link: &HostLink, request: PreviewRequest) -> Answer {
    let preview = link.call(
        "native_preview",
        json!({"agent_id": request.agent_id, "profile_id": request.profile_id}),
    )?;
    annotate_preview(
        link,
        preview,
        request.agent_id,
        request.profile_id.as_deref(),
    )
}

#[tauri::command]
pub async fn native_preview(link: State<'_, HostLink>, request: PreviewRequest) -> Answer {
    if let Some(id) = request.profile_id.as_deref() {
        profile_id(id)?;
    }
    let link = link.inner().clone();
    blocking(move || preview_with_link(&link, request)).await
}

fn start_with_link(link: &HostLink, request: StartRequest) -> Answer {
    check_start_capability(
        link,
        request.agent_id,
        request.profile_id.as_deref(),
        request.plan_token.as_deref(),
        request.expected_cli_version.as_deref(),
    )?;
    let has_profile = request.profile_id.is_some();
    let mut params = json!({
        "agent_id": request.agent_id,
        "project": request.project,
        "profile_id": request.profile_id,
        "size": request.size,
        "brief": request.brief,
    });
    if has_profile && link.has_feature("native_profile_plan_token") {
        params["plan_token"] = json!(request.plan_token);
        params["expected_cli_version"] = json!(request.expected_cli_version);
    }
    link.call("native_start", params)
}

#[tauri::command]
pub async fn native_start(link: State<'_, HostLink>, request: StartRequest) -> Answer {
    if let Some(id) = request.profile_id.as_deref() {
        profile_id(id)?;
    }
    let link = link.inner().clone();
    blocking(move || start_with_link(&link, request)).await
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
        let sessions = link.call("native_sessions", json!({"sample": false}))?;
        let old = sessions.as_array().and_then(|items| {
            items
                .iter()
                .find(|item| item["id"].as_str() == Some(&request.id))
        });
        if let Some(old) = old {
            let agent = serde_json::from_value::<AgentId>(old["agent_id"].clone())
                .map_err(|_| HostError::new("internal"))?;
            let profile_id = old["profile"]["id"].as_str();
            let (plan_token, expected_cli_version) = if let Some(profile_id) = profile_id {
                let supports_plan_token = link.has_feature("native_profile_plan_token");
                let preview = link.call(
                    "native_preview",
                    json!({"agent_id": agent, "profile_id": profile_id}),
                )?;
                let plan_token = if supports_plan_token {
                    Some(
                        preview["plan_token"]
                            .as_str()
                            .ok_or_else(|| HostError::new("host_outdated"))?
                            .to_owned(),
                    )
                } else {
                    None
                };
                let (_, version) = current_evidence(&link, agent)?;
                let expected_cli_version = if effectful_preview(&preview) {
                    let Some(version) = version else {
                        return Err(HostError::new("provider_version_unverified"));
                    };
                    if !capabilities::allows_start_with_profile(agent, Some(&version), true) {
                        return Err(HostError::new("provider_version_unverified"));
                    }
                    Some(version)
                } else {
                    None
                };
                (plan_token, expected_cli_version)
            } else {
                (None, None)
            };
            check_start_capability(
                &link,
                agent,
                profile_id,
                plan_token.as_deref(),
                expected_cli_version.as_deref(),
            )?;
            let mut params = json!({"id": request.id, "size": request.size});
            if link.has_feature("native_profile_plan_token") && profile_id.is_some() {
                params["plan_token"] = json!(plan_token);
                params["expected_cli_version"] = json!(expected_cli_version);
            }
            link.call("native_restart", params)
        } else {
            link.call(
                "native_restart",
                json!({"id": request.id, "size": request.size}),
            )
        }
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

/// What a profile body needs of the host beyond protocol 1: nothing for a body that existed
/// before efforts did, so that an older host kept running across an upgrade still takes it.
pub(crate) fn needed_feature(body: &Body) -> Option<&'static str> {
    body.effort.is_some().then_some("profile_effort")
}

/// Says so plainly when the host that is running cannot keep what the body carries, instead of
/// letting it answer with a request it does not understand.
fn host_can_keep(link: &HostLink, body: &Body) -> Result<(), HostError> {
    match needed_feature(body) {
        Some(feature) if !link.has_feature(feature) => Err(HostError::new("host_outdated")),
        _ => Ok(()),
    }
}

#[tauri::command]
pub async fn native_profile_create(link: State<'_, HostLink>, body: Body) -> Answer {
    let link = link.inner().clone();
    blocking(move || {
        host_can_keep(&link, &body)?;
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
        host_can_keep(&link, &request.body)?;
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

#[cfg(test)]
mod capability_tests {
    use super::{
        PreviewRequest, effectful_preview, preview_with_link, profile_requires_effort_from_list,
        start_with_link,
    };
    use crate::{
        host::{
            client::HostLink,
            protocol::{Answer as WireAnswer, Request, read_frame, write_frame},
        },
        native::{catalogue::AgentId, service::StartRequest},
    };
    use serde_json::Value;
    use serde_json::json;
    use std::{
        fs,
        os::unix::{fs::PermissionsExt, net::UnixListener},
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };
    use wyss_console::terminal::contracts::TerminalSize;

    const PROFILE_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const VERSION: &str = "2.1.309 (Claude Code)";

    struct LegacyHost {
        root: PathBuf,
        stop: Arc<AtomicBool>,
        seen: Arc<Mutex<Vec<(String, Value)>>>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl LegacyHost {
        fn new(inherited_effort: bool) -> Self {
            let root = std::env::temp_dir().join(format!(
                "wyss-native-legacy-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir(&root).unwrap();
            let socket = root.join("host.sock");
            let listener = UnixListener::bind(&socket).unwrap();
            listener.set_nonblocking(true).unwrap();

            // The only executable the production command probes. Any launch attempt fails.
            let cli = root.join("fixture-claude");
            fs::write(
                &cli,
                format!(
                    "#!/bin/sh\n[ \"$1\" = \"--version\" ] || exit 81\nprintf '%s\\n' '{VERSION}'\n"
                ),
            )
            .unwrap();
            fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();

            let stop = Arc::new(AtomicBool::new(false));
            let seen = Arc::new(Mutex::new(Vec::new()));
            let (server_stop, server_seen) = (stop.clone(), seen.clone());
            let cli = cli.to_string_lossy().into_owned();
            let thread = thread::spawn(move || {
                while !server_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let Ok(request) = read_frame::<Request>(&mut stream) else {
                                continue;
                            };
                            server_seen
                                .lock()
                                .unwrap()
                                .push((request.op.clone(), request.params.clone()));
                            let value = match request.op.as_str() {
                                // A protocol-1 host without the additive `features` field.
                                "hello" => json!({"protocol": 1}),
                                "native_launcher" => json!({
                                    "agents": [{"agent_id": "claude_code", "path": cli}],
                                    "projects": [], "shell": null
                                }),
                                "native_preview" => json!({
                                    "applied": ["Model: opus"], "with_cost": [],
                                    "not_applicable": [], "can_start": true,
                                    "through_shell": true, "plan_token": null
                                }),
                                "profiles" if inherited_effort => json!([{
                                    "id": PROFILE_ID,
                                    "body": {"effort": null},
                                    "from": {"effort": "High"}
                                }]),
                                "profiles" => json!([{
                                    "id": PROFILE_ID,
                                    "body": {"effort": null}, "from": {}
                                }]),
                                "native_start" => json!({"fixture_started": true}),
                                _ => Value::Null,
                            };
                            let answer = WireAnswer::Ok(value);
                            let _ = write_frame(&mut stream, &answer);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                root,
                stop,
                seen,
                thread: Some(thread),
            }
        }

        fn link(&self) -> HostLink {
            HostLink::remote_for_test(self.root.join("host.sock"))
        }

        fn requests(&self) -> Vec<(String, Value)> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl Drop for LegacyHost {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn start_request() -> StartRequest {
        StartRequest {
            agent_id: AgentId::ClaudeCode,
            project: "/tmp/fixture-project".into(),
            profile_id: Some(PROFILE_ID.into()),
            size: TerminalSize { rows: 24, cols: 80 },
            brief: None,
            plan_token: Some("old-preview-token".into()),
            expected_cli_version: Some(VERSION.into()),
        }
    }

    #[test]
    fn preview_needs_version_evidence_for_applied_or_costly_profile_overrides() {
        assert!(effectful_preview(&json!({"applied": ["Model: gpt-test"]})));
        assert!(effectful_preview(&json!({"with_cost": ["Hooks are off"]})));
        assert!(!effectful_preview(&json!({"applied": [], "with_cost": []})));
        // Unsupported profile fields are refused by the existing host plan, which also sets
        // can_start=false; this gate must not reinterpret them as silently applied.
        assert!(!effectful_preview(
            &json!({"not_applicable": ["unsupported field"]})
        ));
    }

    #[test]
    fn a_legacy_host_keeps_non_effort_profiles_compatible_but_detects_inherited_effort() {
        let profiles = json!([
            {"id":"plain", "body":{"effort":null}, "from":{}},
            {"id":"own", "body":{"effort":"high"}, "from":{}},
            {"id":"inherited", "body":{"effort":null}, "from":{"effort":"Parent"}}
        ]);
        assert_eq!(
            profile_requires_effort_from_list(&profiles, "plain"),
            Some(false)
        );
        assert_eq!(
            profile_requires_effort_from_list(&profiles, "own"),
            Some(true)
        );
        assert_eq!(
            profile_requires_effort_from_list(&profiles, "inherited"),
            Some(true)
        );
        assert_eq!(
            profile_requires_effort_from_list(&profiles, "missing"),
            None
        );
    }

    #[test]
    fn protocol_one_host_preserves_non_effort_preview_and_start_compatibility() {
        let host = LegacyHost::new(false);
        let link = host.link();
        let preview = preview_with_link(
            &link,
            PreviewRequest {
                agent_id: AgentId::ClaudeCode,
                profile_id: Some(PROFILE_ID.into()),
            },
        )
        .unwrap();
        assert_eq!(preview["can_start"], true);
        assert_eq!(preview["provider_version"], VERSION);
        assert!(
            preview["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning.as_str().unwrap().contains("Legacy Native host"))
        );

        assert_eq!(
            start_with_link(&link, start_request()).unwrap()["fixture_started"],
            true
        );
        let requests = host.requests();
        let start = requests
            .iter()
            .find(|(op, _)| op == "native_start")
            .map(|(_, params)| params)
            .expect("legacy native_start forwarded");
        assert_eq!(start["profile_id"], PROFILE_ID);
        assert!(start.get("plan_token").is_none());
        assert!(start.get("expected_cli_version").is_none());
        assert!(
            requests
                .iter()
                .any(|(op, params)| op == "hello" && params.is_null())
        );
    }

    #[test]
    fn protocol_one_host_refuses_an_inherited_effort_profile_without_starting() {
        let host = LegacyHost::new(true);
        let link = host.link();
        let preview = preview_with_link(
            &link,
            PreviewRequest {
                agent_id: AgentId::ClaudeCode,
                profile_id: Some(PROFILE_ID.into()),
            },
        )
        .unwrap();
        assert_eq!(preview["can_start"], false);
        assert!(
            preview["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning.as_str().unwrap().contains("effort-bearing profile"))
        );

        let error = start_with_link(&link, start_request()).unwrap_err();
        assert_eq!(error.code, "host_outdated");
        assert!(!host.requests().iter().any(|(op, _)| op == "native_start"));
    }
}
