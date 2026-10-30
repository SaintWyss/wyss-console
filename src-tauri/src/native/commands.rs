//! The window's commands for native sessions. Every request names something the host already
//! holds (an agent from the closed catalogue, a session, a profile, a project the user chose) or
//! carries a profile's closed words. None carries an executable, an argument list, an
//! environment or a path (C-01): the folder of a project is only ever a folder the native
//! dialog returned. The sessions themselves live in the host (F9): these commands only
//! validate the request and pass it on, and the answer comes back as the host made it.
use super::{
    arsenal::ArsenalLaunch,
    capabilities::{self, Evidence, Status},
    catalogue::{self, AgentId},
    profile::Body,
    service::StartRequest,
};
use crate::arsenal::{Arsenal, commands::TerminalRequest};
use crate::configuration::Configuration;
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
        AgentId::Opencode => "opencode",
        AgentId::Hermes => "hermes",
    }
}

/// Protocol-1 hosts gain M9 through explicitly additive operations. The old operation names
/// remain legacy projections so an M1-M6 window never deserializes a new closed AgentId.
fn call_extended_or_legacy(
    link: &HostLink,
    extended: &str,
    legacy: &str,
    params: Value,
) -> Result<Value, HostError> {
    match link.call(extended, params.clone()) {
        Err(error) if error.code == "unknown_op" => link.call(legacy, params),
        result => result,
    }
}

fn launcher_view(link: &HostLink) -> Result<Value, HostError> {
    call_extended_or_legacy(
        link,
        "native_launcher_extended",
        "native_launcher",
        Value::Null,
    )
}

fn launcher_agent(link: &HostLink, agent: AgentId) -> Result<Value, HostError> {
    let launcher = launcher_view(link)?;
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
        .and_then(|path| catalogue::probe_provider_version(agent, std::path::Path::new(path)));
    let evidence = capabilities::evidence(agent, version.as_deref());
    Ok((evidence, version))
}

/// The exact build of `agent` the host would launch, as its `--version` reports it now.
pub(crate) fn provider_version(
    link: &HostLink,
    agent: AgentId,
) -> Result<Option<String>, HostError> {
    current_evidence(link, agent).map(|(_, version)| version)
}

/// The projects the host lists for a launch: each path, and whether it is the home folder.
pub(crate) fn known_projects(link: &HostLink) -> Result<Vec<(String, bool)>, HostError> {
    let launcher = launcher_view(link)?;
    Ok(launcher["projects"]
        .as_array()
        .map(|projects| {
            projects
                .iter()
                .filter_map(|project| {
                    Some((
                        project["path"].as_str()?.to_owned(),
                        project["home"].as_bool().unwrap_or(false),
                    ))
                })
                .collect()
        })
        .unwrap_or_default())
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
    let profiles = call_extended_or_legacy(link, "profiles_extended", "profiles", Value::Null)?;
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
    launch: Option<&ArsenalLaunch>,
    plan_token: Option<&str>,
    expected_cli_version: Option<&str>,
) -> Result<(), HostError> {
    if let Some(launch) = launch {
        return check_arsenal_start(
            link,
            agent,
            profile_id,
            launch,
            plan_token,
            expected_cli_version,
        );
    }
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

/// A start that carries an Arsenal exclusion: the host must know the field, the preview of exactly
/// this profile and exclusion must be the one shown, the build must be the one previewed and have
/// its own exclusion evidence, and a profile with effect still needs its own syntax evidence.
fn check_arsenal_start(
    link: &HostLink,
    agent: AgentId,
    profile_id: Option<&str>,
    launch: &ArsenalLaunch,
    plan_token: Option<&str>,
    expected_cli_version: Option<&str>,
) -> Result<(), HostError> {
    if !link.has_feature("native_arsenal_v1") || !link.has_feature("native_profile_plan_token") {
        return Err(HostError::new("host_outdated"));
    }
    let preview = link.call(
        "native_preview",
        json!({"agent_id": agent, "profile_id": profile_id, "arsenal": launch}),
    )?;
    let actual_token = preview["plan_token"]
        .as_str()
        .ok_or_else(|| HostError::new("host_outdated"))?;
    if plan_token != Some(actual_token) {
        return Err(HostError::new("profile_preview_stale"));
    }
    let (_, version) = current_evidence(link, agent)?;
    if version.as_deref() != expected_cli_version
        || !capabilities::permits_arsenal_exclusion(agent, expected_cli_version)
    {
        return Err(HostError::new("arsenal_unqualified"));
    }
    if let Some(profile_id) = profile_id {
        let profile_only = link.call(
            "native_preview",
            json!({"agent_id": agent, "profile_id": profile_id}),
        )?;
        if effectful_preview(&profile_only)
            && !capabilities::allows_start_with_profile(agent, expected_cli_version, true)
        {
            return Err(HostError::new("provider_version_unverified"));
        }
    }
    Ok(())
}

/// What the window may say about an Arsenal selection: which one, the plan it saw, and the project.
/// It never carries skill names; the desktop rebuilds the exclusion from the library.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArsenalStart {
    selection: String,
    config_hash: String,
    project: String,
}

fn arsenal_launch(
    store: Option<std::sync::Arc<std::sync::Mutex<crate::arsenal::store::Store>>>,
    selected: Option<std::path::PathBuf>,
    link: &HostLink,
    agent: AgentId,
    request: &ArsenalStart,
) -> Result<ArsenalLaunch, HostError> {
    crate::arsenal::commands::launch_for(
        store,
        link,
        selected.as_deref(),
        &TerminalRequest {
            selection: request.selection.clone(),
            agent_id: agent,
            project: Some(request.project.clone()),
        },
        &request.config_hash,
    )
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
            .and_then(|path| catalogue::probe_provider_version(agent, std::path::Path::new(path)));
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
    if preview["arsenal"].as_bool() == Some(true)
        && !capabilities::permits_arsenal_exclusion(agent, version.as_deref())
    {
        preview["can_start"] = Value::Bool(false);
        push_string(
            &mut preview,
            "not_applicable",
            "Skill exclusion is not qualified for this provider build.".to_owned(),
        );
    }
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
    blocking(move || annotate_launcher(launcher_view(&link)?)).await
}

/// Additive protocol-1 read. No fallback to discovery: older hosts cannot confirm M9.
fn readiness_with_link(link: &HostLink) -> Answer {
    match link.call_attached("native_readiness", Value::Null) {
        Err(error) if error.code == "unknown_op" => Err(HostError::new("host_outdated")),
        answer => answer,
    }
}

#[tauri::command]
pub async fn native_readiness(link: State<'_, HostLink>) -> Answer {
    let link = link.inner().clone();
    blocking(move || readiness_with_link(&link)).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    agent_id: AgentId,
    profile_id: Option<String>,
    #[serde(default)]
    arsenal: Option<ArsenalStart>,
}

fn preview_with_link(
    link: &HostLink,
    request: PreviewRequest,
    launch: Option<ArsenalLaunch>,
) -> Answer {
    let mut params = json!({"agent_id": request.agent_id, "profile_id": request.profile_id});
    if let Some(launch) = &launch {
        if !link.has_feature("native_arsenal_v1") {
            return Err(HostError::new("host_outdated"));
        }
        params["arsenal"] = json!(launch);
    }
    let mut preview = link.call("native_preview", params)?;
    if launch.is_some() {
        preview["arsenal"] = Value::Bool(true);
    }
    annotate_preview(
        link,
        preview,
        request.agent_id,
        request.profile_id.as_deref(),
    )
}

#[tauri::command]
pub async fn native_preview(
    link: State<'_, HostLink>,
    arsenal: State<'_, Arsenal>,
    configuration: State<'_, Configuration>,
    request: PreviewRequest,
) -> Answer {
    if let Some(id) = request.profile_id.as_deref() {
        profile_id(id)?;
    }
    let launch = match &request.arsenal {
        None => None,
        Some(start) => {
            let (store, selected) = (arsenal.launch_handle(), configuration.selected_project());
            let link = link.inner().clone();
            let (agent, start) = (request.agent_id, start.clone());
            Some(blocking(move || arsenal_launch(store, selected, &link, agent, &start)).await?)
        }
    };
    let link = link.inner().clone();
    blocking(move || preview_with_link(&link, request, launch)).await
}

/// A start as the window asks for it. It is `StartRequest` without the exclusion itself: the window
/// names a selection and the plan it previewed, and the desktop rebuilds what the host receives.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowStart {
    agent_id: AgentId,
    project: String,
    profile_id: Option<String>,
    size: TerminalSize,
    #[serde(default)]
    brief: Option<String>,
    #[serde(default)]
    plan_token: Option<String>,
    #[serde(default)]
    expected_cli_version: Option<String>,
    #[serde(default)]
    arsenal: Option<ArsenalStart>,
}

fn start_with_link(link: &HostLink, request: StartRequest) -> Answer {
    if !request.agent_id.is_legacy() && !link.has_feature("native_extended_catalogue_v1") {
        return Err(HostError::new("host_outdated"));
    }
    let expected_cli_version = if request.agent_id == AgentId::Opencode && request.brief.is_some() {
        let (_, version) = current_evidence(link, request.agent_id)?;
        if version.as_deref() != Some("1.19.4") {
            return Err(HostError::new("provider_version_unverified"));
        }
        Some(version.ok_or_else(|| HostError::new("provider_version_unverified"))?)
    } else {
        request.expected_cli_version.clone()
    };
    check_start_capability(
        link,
        request.agent_id,
        request.profile_id.as_deref(),
        request.arsenal.as_ref(),
        request.plan_token.as_deref(),
        expected_cli_version.as_deref(),
    )?;
    let has_profile = request.profile_id.is_some() || request.arsenal.is_some();
    let mut params = json!({
        "agent_id": request.agent_id,
        "project": request.project,
        "profile_id": request.profile_id,
        "size": request.size,
        "brief": request.brief,
    });
    let send_profile_plan = has_profile && link.has_feature("native_profile_plan_token");
    let send_handoff_version = request.agent_id == AgentId::Opencode && request.brief.is_some();
    if send_profile_plan {
        params["plan_token"] = json!(request.plan_token);
    }
    // OpenCode handoffs without a profile still bind the exact qualified CLI build. Do not
    // add the newer field to ordinary legacy starts: protocol-1 M1-M6 hosts reject unknown
    // StartRequest fields by design.
    if send_profile_plan || send_handoff_version {
        params["expected_cli_version"] = json!(expected_cli_version);
    }
    if let Some(launch) = &request.arsenal {
        params["arsenal"] = json!(launch);
    }
    link.call("native_start", params)
}

#[tauri::command]
pub async fn native_start(
    link: State<'_, HostLink>,
    arsenal: State<'_, Arsenal>,
    configuration: State<'_, Configuration>,
    request: WindowStart,
) -> Answer {
    if let Some(id) = request.profile_id.as_deref() {
        profile_id(id)?;
    }
    let launch = match &request.arsenal {
        None => None,
        Some(start) if start.project != request.project => {
            return Err(HostError::new("request_invalid"));
        }
        Some(start) => {
            let (store, selected) = (arsenal.launch_handle(), configuration.selected_project());
            let link = link.inner().clone();
            let (agent, start) = (request.agent_id, start.clone());
            Some(blocking(move || arsenal_launch(store, selected, &link, agent, &start)).await?)
        }
    };
    let request = StartRequest {
        agent_id: request.agent_id,
        project: request.project,
        profile_id: request.profile_id,
        size: request.size,
        brief: request.brief,
        plan_token: request.plan_token,
        expected_cli_version: request.expected_cli_version,
        arsenal: launch,
    };
    let link = link.inner().clone();
    blocking(move || start_with_link(&link, request)).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {
    sample: bool,
}

/// Polling may observe endings, but must never initialize or restart a host. Both protocol
/// projections use the attached-only transport, including the old-host fallback.
fn sessions_with_link(link: &HostLink, sample: bool) -> Answer {
    let params = json!({"sample": sample});
    match link.call_attached("native_sessions_extended", params.clone()) {
        Err(error) if error.code == "unknown_op" => link.call_attached("native_sessions", params),
        answer => answer,
    }
}

#[tauri::command]
pub async fn native_sessions(link: State<'_, HostLink>, request: ListRequest) -> Answer {
    let link = link.inner().clone();
    blocking(move || sessions_with_link(&link, request.sample)).await
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
pub async fn native_restart(
    link: State<'_, HostLink>,
    arsenal: State<'_, Arsenal>,
    configuration: State<'_, Configuration>,
    request: RestartRequest,
) -> Answer {
    well_formed(&request.id)?;
    let link = link.inner().clone();
    let (store, selected) = (arsenal.launch_handle(), configuration.selected_project());
    blocking(move || {
        let sessions = call_extended_or_legacy(
            &link,
            "native_sessions_extended",
            "native_sessions",
            json!({"sample": false}),
        )?;
        let old = sessions.as_array().and_then(|items| {
            items
                .iter()
                .find(|item| item["id"].as_str() == Some(&request.id))
        });
        if let Some(old) = old {
            let agent = serde_json::from_value::<AgentId>(old["agent_id"].clone())
                .map_err(|_| HostError::new("internal"))?;
            let profile_id = old["profile"]["id"].as_str();
            if old["arsenal"].is_object() {
                return restart_with_arsenal(&link, store, selected, &request, old, agent);
            }
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
                None,
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

/// A session launched with an Arsenal exclusion restarts with it only while the library and the
/// installed skills still give the same configuration; otherwise it is refused as stale, because a
/// skill installed since would be exposed by the old exclusion.
fn restart_with_arsenal(
    link: &HostLink,
    store: Option<std::sync::Arc<std::sync::Mutex<crate::arsenal::store::Store>>>,
    selected: Option<std::path::PathBuf>,
    request: &RestartRequest,
    old: &Value,
    agent: AgentId,
) -> Answer {
    let launched: ArsenalLaunch =
        serde_json::from_value(old["arsenal"].clone()).map_err(|_| HostError::new("internal"))?;
    let project = old["project"]
        .as_str()
        .ok_or_else(|| HostError::new("internal"))?;
    let launch = arsenal_launch(
        store,
        selected,
        link,
        agent,
        &ArsenalStart {
            selection: launched.selection.clone(),
            config_hash: launched.config_hash.clone(),
            project: project.to_owned(),
        },
    )?;
    if launch != launched {
        return Err(HostError::new("arsenal_plan_stale"));
    }
    let profile_id = old["profile"]["id"].as_str();
    let preview = link.call(
        "native_preview",
        json!({"agent_id": agent, "profile_id": profile_id, "arsenal": &launch}),
    )?;
    let plan_token = preview["plan_token"]
        .as_str()
        .ok_or_else(|| HostError::new("host_outdated"))?
        .to_owned();
    let (_, version) = current_evidence(link, agent)?;
    check_start_capability(
        link,
        agent,
        profile_id,
        Some(&launch),
        Some(&plan_token),
        version.as_deref(),
    )?;
    link.call(
        "native_restart",
        json!({
            "id": request.id,
            "size": request.size,
            "plan_token": plan_token,
            "expected_cli_version": version,
        }),
    )
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
    blocking(move || call_extended_or_legacy(&link, "profiles_extended", "profiles", Value::Null))
        .await
}

/// What a profile body needs of the host beyond protocol 1: nothing for a body that existed
/// before efforts did, so that an older host kept running across an upgrade still takes it.
pub(crate) fn needed_feature(body: &Body) -> Option<&'static str> {
    body.effort.is_some().then_some("profile_effort")
}

/// Says so plainly when the host that is running cannot keep what the body carries, instead of
/// letting it answer with a request it does not understand.
fn host_can_keep(link: &HostLink, body: &Body) -> Result<(), HostError> {
    if body.applies_to.iter().any(|agent| !agent.is_legacy())
        && !link.has_feature("native_extended_catalogue_v1")
    {
        return Err(HostError::new("host_outdated"));
    }
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
    "native_readiness",
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
            native_readiness,
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
        PreviewRequest, annotate_launcher, effectful_preview, host_can_keep, launcher_view,
        preview_with_link, profile_requires_effort_from_list, readiness_with_link,
        sessions_with_link, start_with_link,
    };
    use crate::{
        host::{
            client::HostLink,
            protocol::{Answer as WireAnswer, HostError, Request, read_frame, write_frame},
        },
        native::{catalogue::AgentId, profile::Body, service::StartRequest},
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
                            if matches!(
                                request.op.as_str(),
                                "native_launcher_extended"
                                    | "native_sessions_extended"
                                    | "profiles_extended"
                            ) {
                                let _ = write_frame(
                                    &mut stream,
                                    &WireAnswer::Err(HostError::new("unknown_op")),
                                );
                                continue;
                            }
                            let value = match request.op.as_str() {
                                // A protocol-1 host without the additive `features` field.
                                "native_readiness" => {
                                    let _ = write_frame(
                                        &mut stream,
                                        &WireAnswer::Err(HostError::new("unknown_op")),
                                    );
                                    continue;
                                }
                                "hello" => json!({"protocol": 1}),
                                "native_launcher" => json!({
                                    "agents": [{"agent_id": "claude_code", "path": cli}],
                                    "projects": [], "shell": null
                                }),
                                "native_sessions" => json!([]),
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
            arsenal: None,
            agent_id: AgentId::ClaudeCode,
            project: "/tmp/fixture-project".into(),
            profile_id: Some(PROFILE_ID.into()),
            size: TerminalSize { rows: 24, cols: 80 },
            brief: None,
            plan_token: Some("old-preview-token".into()),
            expected_cli_version: Some(VERSION.into()),
        }
    }

    fn arsenal_launch() -> crate::native::arsenal::ArsenalLaunch {
        crate::native::arsenal::ArsenalLaunch {
            selection: "0b1c4e1a-0000-4000-8000-000000000001".into(),
            selection_name: "Frontend".into(),
            config_hash: "b".repeat(64),
            skills_off: vec!["design".into()],
        }
    }

    #[test]
    fn a_host_without_the_arsenal_feature_never_receives_an_exclusion() {
        let host = LegacyHost::new(false);
        let link = host.link();
        let request = StartRequest {
            arsenal: Some(arsenal_launch()),
            profile_id: None,
            ..start_request()
        };
        assert_eq!(
            start_with_link(&link, request).unwrap_err().code,
            "host_outdated"
        );
        assert_eq!(
            preview_with_link(
                &link,
                PreviewRequest {
                    agent_id: AgentId::ClaudeCode,
                    profile_id: None,
                    arsenal: None,
                },
                Some(arsenal_launch()),
            )
            .unwrap_err()
            .code,
            "host_outdated"
        );
        // Nothing that could start or preview with the exclusion reached the host.
        assert!(
            host.requests()
                .iter()
                .all(|(op, params)| op != "native_start" && params.get("arsenal").is_none())
        );
    }

    #[test]
    fn the_window_cannot_send_skill_names_in_a_start_or_preview() {
        let start = serde_json::json!({
            "agent_id": "claude_code", "project": "/tmp/p", "profile_id": null,
            "size": {"rows": 24, "cols": 80},
            "arsenal": {"selection": "s", "config_hash": "h", "project": "/tmp/p",
                        "skills_off": ["design"]}
        });
        assert!(serde_json::from_value::<super::WindowStart>(start).is_err());
        let launch_shape = serde_json::json!({
            "agent_id": "claude_code", "project": "/tmp/p", "profile_id": null,
            "size": {"rows": 24, "cols": 80},
            "arsenal": arsenal_launch()
        });
        assert!(serde_json::from_value::<super::WindowStart>(launch_shape).is_err());
        let preview = serde_json::json!({
            "agent_id": "claude_code", "profile_id": null,
            "arsenal": {"selection": "s", "config_hash": "h", "project": "/tmp/p",
                        "skills_off": []}
        });
        assert!(serde_json::from_value::<PreviewRequest>(preview).is_err());
    }

    #[test]
    fn readiness_on_old_disconnected_or_uninitialized_hosts_never_falls_back_to_discovery() {
        let host = LegacyHost::new(false);
        let link = host.link();
        assert_eq!(
            readiness_with_link(&link).unwrap_err().code,
            "host_outdated"
        );
        assert_eq!(
            host.requests()
                .iter()
                .map(|(op, _)| op.as_str())
                .collect::<Vec<_>>(),
            vec!["native_readiness"]
        );
        // Default must not initialize a host; disconnected must not reconnect/spawn one.
        assert_eq!(
            readiness_with_link(&HostLink::new()).unwrap_err().code,
            "host_unavailable"
        );
        drop(host);
        assert_eq!(
            readiness_with_link(&link).unwrap_err().code,
            "host_unavailable"
        );
    }

    #[test]
    fn session_polling_uses_only_attached_hosts_including_the_legacy_fallback() {
        let host = LegacyHost::new(false);
        let link = host.link();
        assert_eq!(sessions_with_link(&link, false).unwrap(), json!([]));
        assert_eq!(
            host.requests()
                .iter()
                .map(|(op, _)| op.as_str())
                .collect::<Vec<_>>(),
            vec!["native_sessions_extended", "native_sessions"]
        );
        drop(host);
        assert_eq!(
            sessions_with_link(&link, false).unwrap_err().code,
            "host_unavailable"
        );
        assert_eq!(
            sessions_with_link(&HostLink::new(), false)
                .unwrap_err()
                .code,
            "host_unavailable"
        );
    }

    #[test]
    fn readiness_heartbeat_has_zero_version_probes_and_one_request_per_interval() {
        use crate::{
            host::core::HostCore,
            native::service_tests::{World, script},
        };
        use std::time::Instant;
        let (base, home, bin) = World::directories();
        let log = bin.join("invocations");
        for name in ["claude", "codex", "opencode", "hermes"] {
            script(
                &bin.join(name),
                &format!(
                    "printf '%s %s\\n' '{name}' \"$*\" >> '{}'\n[ \"$1\" = \"--version\" ] || exit 81\nprintf '%s\\n' 'fixture-version'",
                    log.display()
                ),
            );
        }
        let core = Arc::new(Mutex::new(HostCore::new(Some(World::open(
            &base, &home, &bin,
        )))));
        let socket = bin.join("readiness.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (server_core, server_stop, server_requests) =
            (core.clone(), stop.clone(), requests.clone());
        let server = thread::spawn(move || {
            while !server_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request: Request = read_frame(&mut stream).unwrap();
                        server_requests.lock().unwrap().push(request.op.clone());
                        let answer = match server_core
                            .lock()
                            .unwrap()
                            .call(&request.op, request.params)
                        {
                            Ok(value) => WireAnswer::Ok(value),
                            Err(error) => WireAnswer::Err(error),
                        };
                        let _ = write_frame(&mut stream, &answer);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_micros(100))
                    }
                    Err(e) => panic!("fixture socket: {e}"),
                }
            }
        });
        let link = HostLink::remote_for_test(socket);
        annotate_launcher(launcher_view(&link).unwrap()).unwrap();
        let initial = fs::read_to_string(&log).unwrap();
        assert!(initial.contains("opencode --version")); // positive control: discovery ran the fakes
        assert!(!initial.contains("hermes")); // AUD-01: no Hermes passive update probe
        requests.lock().unwrap().clear();
        // Make any accidentally invoked version executable fail and record the attempt.
        for name in ["claude", "codex", "opencode", "hermes"] {
            script(
                &bin.join(name),
                &format!("echo unexpected >> '{}'; exit 82", log.display()),
            );
        }
        let started = Instant::now();
        let mut readiness_elapsed = Duration::ZERO;
        for _ in 0..30 {
            assert_eq!(sessions_with_link(&link, false).unwrap(), json!([]));
            let check_started = Instant::now();
            assert_eq!(
                readiness_with_link(&link).unwrap()["m9_native_status"],
                "ready"
            );
            readiness_elapsed += check_started.elapsed();
        }
        let elapsed = started.elapsed();
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            ["native_sessions_extended", "native_readiness"].repeat(30)
        );
        assert_eq!(fs::read_to_string(&log).unwrap(), initial);
        assert!(
            elapsed < Duration::from_secs(2),
            "30 checks exceeded local fixture budget: {elapsed:?}"
        );
        // A replacement backend has actually reopened the store: readiness is not a client cache.
        let sidecar = base.join("registry/m9-native-sessions-v1.json");
        let original = fs::read(&sidecar).unwrap();
        fs::write(&sidecar, b"{ corrupt fixture").unwrap();
        *core.lock().unwrap() = HostCore::new(Some(World::open(&base, &home, &bin)));
        assert_eq!(
            readiness_with_link(&link).unwrap()["m9_native_status"],
            "storage_unavailable"
        );
        fs::write(&sidecar, original).unwrap();
        *core.lock().unwrap() = HostCore::new(Some(World::open(&base, &home, &bin)));
        assert_eq!(
            readiness_with_link(&link).unwrap()["m9_native_status"],
            "ready"
        );
        assert_eq!(fs::read_to_string(&log).unwrap(), initial);
        println!(
            "READINESS_COST intervals=30 simulated_ms=60000 requests=60 external_processes=0 version_probes=0 initial_version_probes={} total_us={} mean_us={} heartbeat_total_us={}",
            initial.lines().count(),
            readiness_elapsed.as_micros(),
            readiness_elapsed.as_micros() / 30,
            elapsed.as_micros()
        );
        stop.store(true, Ordering::Release);
        server.join().unwrap();
        fs::remove_dir_all(base.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_legacy_host_cannot_admit_m9_providers_or_profiles_by_direct_ipc() {
        let host = LegacyHost::new(false);
        let link = host.link();
        let body = Body {
            name: "OpenCode profile".to_owned(),
            applies_to: vec![AgentId::Opencode],
            ..Body::default()
        };
        assert_eq!(
            host_can_keep(&link, &body).unwrap_err().code,
            "host_outdated"
        );

        let error = start_with_link(
            &link,
            StartRequest {
                arsenal: None,
                agent_id: AgentId::Opencode,
                project: "/tmp/fixture-project".into(),
                profile_id: None,
                size: TerminalSize { rows: 24, cols: 80 },
                brief: None,
                plan_token: None,
                expected_cli_version: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "host_outdated");
        assert!(
            !host
                .requests()
                .iter()
                .any(|(operation, _)| operation == "native_start")
        );
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
                arsenal: None,
            },
            None,
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
                arsenal: None,
            },
            None,
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
