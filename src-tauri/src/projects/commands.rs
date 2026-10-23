//! The window's commands for the project-centered workspace.
//!
//! None takes a path, a command line, an environment, a prompt or a credential. A folder arrives
//! as a one-use token for a pick the native dialog made; a change arrives as the closed envelope
//! of the contract; the owners are asked through the checked, non-starting socket. Everything is
//! behind the release gate (`projects_set_enabled`, confirmed in a native window), so a build with
//! this module installed does nothing new until the owner adopts it.
use super::{
    apply::{Purpose, SystemEnv},
    claim::Claims,
    command::{Envelope, ErrorCode, Failure, Receipt},
    migration::Summary,
    owner::{
        HostSocket, LaunchError, ManagedLaunch, ManagedLaunched, ManagedPort, ManagedRow,
        OwnerError, RealOwner,
    },
    service::{
        HostStatus, Overview, Page, PickView, ProjectView, ProjectsService, RollbackReport,
        SessionView, StoreStatus, WorkspaceView,
    },
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use wyss_console::{
    contracts::control::ActualState,
    contracts::{
        WireUuid,
        control::RuntimeSafetyProfile,
        m1::records::{ManagedProtocol, ManagedSafetyProfile},
    },
    f8::gui::{F8Failure, F8Gui},
    sessions::{SessionFailure, SessionMode, SessionService},
};

// ---- the Managed owners, read and prepared through what the desktop already holds -----------

/// P1 (the installed authority's sessions) and P2 (the protocol-2 interface, when the desktop
/// was started against one). It can list and prepare; it has no way to bind, lease or run.
struct ManagedFromApp {
    app: AppHandle,
}

fn profile_of(profile: RuntimeSafetyProfile) -> ManagedSafetyProfile {
    match profile {
        RuntimeSafetyProfile::CodexInferenceV1 => ManagedSafetyProfile::Inference,
        RuntimeSafetyProfile::CodexLocalToolsV1 => ManagedSafetyProfile::LocalTools,
    }
}

impl ManagedPort for ManagedFromApp {
    fn sessions(&self) -> Result<Vec<ManagedRow>, OwnerError> {
        let mut rows = Vec::new();
        let mut read_any = false;
        if let Some(gui) = self.app.try_state::<F8Gui>()
            && gui.is_active()
        {
            let stream = gui
                .state()
                .authority
                .map(|a| a.stream_id.as_str().to_owned())
                .ok_or(OwnerError::HostOffline)?;
            let view = gui.sessions(None).map_err(|_| OwnerError::HostOffline)?;
            read_any = true;
            for row in view.sessions {
                let ready = row.runtime.actual == ActualState::Ready;
                rows.push(ManagedRow {
                    protocol: ManagedProtocol::P2,
                    store_stream_id: stream.clone(),
                    session_id: row.id.as_str().to_owned(),
                    // The profile of the generation that is running, only while it runs.
                    effective_profile: row.runtime.profile.filter(|_| ready).map(profile_of),
                    attached: ready,
                });
            }
        }
        if let Some(service) = self.app.try_state::<SessionService>()
            && let Ok(stream) = service.stream_id()
            && let Ok(page) = service.page(None)
        {
            read_any = true;
            for summary in page.sessions {
                let attached = summary.binding
                    == wyss_console::contracts::binding::BindingState::BoundAttached;
                rows.push(ManagedRow {
                    protocol: ManagedProtocol::P1,
                    store_stream_id: stream.as_str().to_owned(),
                    session_id: summary.id.as_str().to_owned(),
                    effective_profile: match summary.mode {
                        SessionMode::Inference => Some(ManagedSafetyProfile::Inference),
                        SessionMode::LocalTools => Some(ManagedSafetyProfile::LocalTools),
                        SessionMode::Unknown => None,
                    },
                    attached,
                });
            }
        }
        if read_any {
            Ok(rows)
        } else {
            Err(OwnerError::HostOffline)
        }
    }

    /// Prepares a session, nothing more. With a protocol-2 interface the session is its; without
    /// one it is the installed authority's. The operation is the intent, so a retry is the same.
    fn prepare(&self, launch: &ManagedLaunch) -> Result<ManagedLaunched, LaunchError> {
        let intent = WireUuid::try_from(launch.intent.clone())
            .map_err(|_| LaunchError::NotDispatched(OwnerError::Refused))?;
        if let Some(gui) = self.app.try_state::<F8Gui>()
            && gui.is_active()
        {
            let stream = gui
                .state()
                .authority
                .map(|a| a.stream_id.as_str().to_owned())
                .ok_or(LaunchError::NotDispatched(OwnerError::HostOffline))?;
            return match gui.prepare(intent) {
                Ok(receipt) => Ok(ManagedLaunched {
                    protocol: ManagedProtocol::P2,
                    store_stream_id: stream,
                    session_id: receipt.id.as_str().to_owned(),
                }),
                // The authority may have taken the intent; the same intent settles it later.
                Err(error) => Err(match error.code {
                    F8Failure::Unavailable | F8Failure::Uncertain | F8Failure::Internal => {
                        LaunchError::Ambiguous
                    }
                    F8Failure::Inactive => LaunchError::NotDispatched(OwnerError::Unsupported),
                    _ => LaunchError::NotDispatched(OwnerError::Refused),
                }),
            };
        }
        let service = self
            .app
            .try_state::<SessionService>()
            .ok_or(LaunchError::NotDispatched(OwnerError::Unsupported))?;
        let stream = service
            .stream_id()
            .map_err(|_| LaunchError::NotDispatched(OwnerError::HostOffline))?;
        match service.prepare(intent) {
            Ok(receipt) => Ok(ManagedLaunched {
                protocol: ManagedProtocol::P1,
                store_stream_id: stream.as_str().to_owned(),
                session_id: receipt.id.as_str().to_owned(),
            }),
            // The authority may have taken the intent: never retried from here.
            Err(error) => Err(match error.code {
                SessionFailure::Unavailable
                | SessionFailure::Uncertain
                | SessionFailure::Internal => LaunchError::Ambiguous,
                SessionFailure::Incompatible => LaunchError::NotDispatched(OwnerError::Unsupported),
                _ => LaunchError::NotDispatched(OwnerError::Refused),
            }),
        }
    }
}

// ---- the service, made once -------------------------------------------------------------------

#[derive(Default)]
pub struct Projects {
    service: OnceLock<Arc<ProjectsService>>,
}

impl Projects {
    fn get(&self, app: &AppHandle) -> Arc<ProjectsService> {
        self.service
            .get_or_init(|| {
                let owner = RealOwner::new(
                    HostSocket::from_environment(),
                    ManagedFromApp { app: app.clone() },
                );
                let service = ProjectsService::new(
                    super::store::default_dir(),
                    Arc::new(owner),
                    Arc::new(SystemEnv::new()),
                    Claims::from_environment(),
                );
                // A launch whose process is gone is made ambiguous once, here. Never replayed.
                service.recover();
                Arc::new(service)
            })
            .clone()
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| Failure::new(ErrorCode::IoFailure))?
}

// ---- requests ---------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorRequest {
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionsRequest {
    /// Absent: every session. `{ "project": null }`: the unassigned ones.
    #[serde(default, with = "double_option")]
    project: Option<Option<String>>,
    cursor: Option<String>,
}

mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChooseRequest {
    purpose: ChoosePurpose,
    selection_revision: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChoosePurpose {
    Register,
    Parent,
    Relink,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitRequest {
    preview_id: String,
    accept_aliases: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssociationRequest {
    association_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadRequest {
    association_id: String,
    from: u64,
    wait_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteRequest {
    association_id: String,
    data: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResizeRequest {
    association_id: String,
    rows: u16,
    cols: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnableRequest {
    on: bool,
}

// ---- replies ----------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    data: Vec<u8>,
    next: u64,
    eof: bool,
    truncated: bool,
}

// ---- commands ---------------------------------------------------------------------------------

#[tauri::command]
pub async fn projects_overview(
    app: AppHandle,
    projects: State<'_, Projects>,
) -> Result<Overview, Failure> {
    let service = projects.get(&app);
    blocking(move || Ok(service.overview())).await
}

#[tauri::command]
pub async fn projects_list(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: CursorRequest,
) -> Result<Page<ProjectView>, Failure> {
    let service = projects.get(&app);
    blocking(move || service.list_projects(request.cursor.as_deref())).await
}

#[tauri::command]
pub async fn projects_project(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: IdRequest,
) -> Result<ProjectView, Failure> {
    let service = projects.get(&app);
    blocking(move || service.get_project(&request.id)).await
}

#[tauri::command]
pub async fn projects_workspace(
    app: AppHandle,
    window: WebviewWindow,
    projects: State<'_, Projects>,
    request: IdRequest,
) -> Result<WorkspaceView, Failure> {
    let service = projects.get(&app);
    let label = window.label().to_owned();
    blocking(move || service.get_workspace(&request.id, Some(&label))).await
}

#[tauri::command]
pub async fn projects_sessions(
    app: AppHandle,
    window: WebviewWindow,
    projects: State<'_, Projects>,
    request: SessionsRequest,
) -> Result<Page<SessionView>, Failure> {
    let service = projects.get(&app);
    let label = window.label().to_owned();
    blocking(move || {
        service.list_sessions(
            request.project.as_ref().map(|p| p.as_deref()),
            request.cursor.as_deref(),
            Some(&label),
        )
    })
    .await
}

/// The native folder dialog. Cancelling returns nothing and changes nothing; a folder returns a
/// one-use token and its name, never its path.
#[tauri::command]
pub async fn projects_choose_folder(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: ChooseRequest,
) -> Result<Option<PickView>, Failure> {
    let service = projects.get(&app);
    blocking(move || {
        service.gated_for_pick()?;
        let Some(path) = crate::native_dialog::choose_folder(&app) else {
            return Ok(None);
        };
        let text = path
            .into_os_string()
            .into_string()
            .ok()
            .filter(|t| t.starts_with('/') && t.len() <= 4096 && !t.chars().any(char::is_control))
            .ok_or_else(|| Failure::new(ErrorCode::RootIneligible))?;
        let purpose = match request.purpose {
            ChoosePurpose::Register => Purpose::Register,
            ChoosePurpose::Parent => Purpose::Parent,
            ChoosePurpose::Relink => Purpose::Relink,
        };
        service
            .issue_pick(&text, purpose, request.selection_revision)
            .map(Some)
    })
    .await
}

#[tauri::command]
pub async fn projects_forget_picks(
    app: AppHandle,
    projects: State<'_, Projects>,
) -> Result<(), Failure> {
    let service = projects.get(&app);
    blocking(move || {
        service.forget_picks();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn projects_change(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: Envelope,
) -> Result<Receipt, Failure> {
    let service = projects.get(&app);
    blocking(move || service.execute(request)).await
}

#[tauri::command]
pub async fn projects_initialize_empty(
    app: AppHandle,
    projects: State<'_, Projects>,
) -> Result<StoreStatus, Failure> {
    let service = projects.get(&app);
    blocking(move || {
        service.initialize_empty()?;
        Ok(service.overview().store)
    })
    .await
}

#[tauri::command]
pub async fn projects_migration_preview(
    app: AppHandle,
    projects: State<'_, Projects>,
) -> Result<Summary, Failure> {
    let service = projects.get(&app);
    blocking(move || service.preview_migration()).await
}

#[tauri::command]
pub async fn projects_migration_commit(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: CommitRequest,
) -> Result<StoreStatus, Failure> {
    let service = projects.get(&app);
    blocking(move || service.commit_migration(&request.preview_id, request.accept_aliases)).await
}

#[tauri::command]
pub async fn projects_input_claim(
    app: AppHandle,
    window: WebviewWindow,
    projects: State<'_, Projects>,
    request: AssociationRequest,
) -> Result<(), Failure> {
    let service = projects.get(&app);
    let label = window.label().to_owned();
    blocking(move || service.claim_input(&label, &request.association_id)).await
}

#[tauri::command]
pub async fn projects_input_release(
    app: AppHandle,
    window: WebviewWindow,
    projects: State<'_, Projects>,
    request: AssociationRequest,
) -> Result<(), Failure> {
    let service = projects.get(&app);
    let label = window.label().to_owned();
    blocking(move || {
        service.release_input(&label, &request.association_id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn projects_term_read(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: ReadRequest,
) -> Result<Output, Failure> {
    let service = projects.get(&app);
    blocking(move || {
        service
            .read_output(&request.association_id, request.from, request.wait_ms)
            .map(|read| Output {
                data: read.data,
                next: read.next,
                eof: read.eof,
                truncated: read.truncated,
            })
    })
    .await
}

#[tauri::command]
pub async fn projects_term_write(
    app: AppHandle,
    window: WebviewWindow,
    projects: State<'_, Projects>,
    request: WriteRequest,
) -> Result<(), Failure> {
    let service = projects.get(&app);
    let label = window.label().to_owned();
    blocking(move || service.write_input(&label, &request.association_id, &request.data)).await
}

#[tauri::command]
pub async fn projects_term_resize(
    app: AppHandle,
    window: WebviewWindow,
    projects: State<'_, Projects>,
    request: ResizeRequest,
) -> Result<(), Failure> {
    let service = projects.get(&app);
    let label = window.label().to_owned();
    blocking(move || service.resize(&label, &request.association_id, request.rows, request.cols))
        .await
}

#[tauri::command]
pub async fn projects_rollback_report(
    app: AppHandle,
    projects: State<'_, Projects>,
) -> Result<RollbackReport, Failure> {
    let service = projects.get(&app);
    blocking(move || service.rollback_report()).await
}

#[tauri::command]
pub async fn projects_compact_receipts(
    app: AppHandle,
    projects: State<'_, Projects>,
) -> Result<usize, Failure> {
    let service = projects.get(&app);
    blocking(move || service.compact_receipts()).await
}

/// Turns the project-centered workspace on or off. Both ways are confirmed in a native window
/// the page cannot draw on, so the page cannot adopt (or drop) it by itself. Turning it off is
/// refused while a launch of unknown outcome is unaccounted for. Nothing running is touched.
#[tauri::command]
pub async fn projects_set_enabled(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: EnableRequest,
) -> Result<Overview, Failure> {
    let service = projects.get(&app);
    blocking(move || {
        if !request.on && service.rollback_report().is_ok_and(|r| !r.can_disable) {
            return Err(Failure::new(ErrorCode::LeafBusy).with_field("unfinished_launches"));
        }
        let (heading, body) = if request.on {
            (
                "Use project workspaces",
                "Turn on projects and workspaces. Your sessions and the older windows are not changed or stopped.",
            )
        } else {
            (
                "Go back to the older windows",
                "Turn off projects and workspaces. Nothing is stopped; the project data is kept.",
            )
        };
        if !crate::native_dialog::confirm_text(&app, heading, body) {
            return Err(Failure::new(ErrorCode::OwnerRefused).with_field("declined"));
        }
        service.set_enabled(request.on)?;
        Ok(service.overview())
    })
    .await
}

/// Another window onto the same store: it shows the project workspace and nothing else.
#[tauri::command]
pub async fn projects_open_window(
    app: AppHandle,
    projects: State<'_, Projects>,
) -> Result<String, Failure> {
    let service = projects.get(&app);
    if !service.enabled() {
        return Err(Failure::new(ErrorCode::UnsupportedCapability).with_field("disabled"));
    }
    let label = format!("w-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::App("index.html#workspace".into()),
    )
    .title("Wyss Console — workspace")
    .inner_size(1100.0, 720.0)
    .build()
    .map_err(|_| Failure::new(ErrorCode::IoFailure))?;
    Ok(label)
}

const COMMANDS: &[&str] = &[
    "projects_overview",
    "projects_list",
    "projects_project",
    "projects_workspace",
    "projects_sessions",
    "projects_choose_folder",
    "projects_forget_picks",
    "projects_change",
    "projects_initialize_empty",
    "projects_migration_preview",
    "projects_migration_commit",
    "projects_input_claim",
    "projects_input_release",
    "projects_term_read",
    "projects_term_write",
    "projects_term_resize",
    "projects_rollback_report",
    "projects_compact_receipts",
    "projects_set_enabled",
    "projects_open_window",
];

pub fn extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: COMMANDS,
        handler: Box::new(tauri::generate_handler![
            projects_overview,
            projects_list,
            projects_project,
            projects_workspace,
            projects_sessions,
            projects_choose_folder,
            projects_forget_picks,
            projects_change,
            projects_initialize_empty,
            projects_migration_preview,
            projects_migration_commit,
            projects_input_claim,
            projects_input_release,
            projects_term_read,
            projects_term_write,
            projects_term_resize,
            projects_rollback_report,
            projects_compact_receipts,
            projects_set_enabled,
            projects_open_window
        ]),
        install: Box::new(|builder| {
            builder
                .manage(Projects::default())
                // A window that closes or crashes lets go of the input it held.
                .on_window_event(|window, event| {
                    if let tauri::WindowEvent::Destroyed = event
                        && let Some(projects) = window.app_handle().try_state::<Projects>()
                        && let Some(service) = projects.service.get()
                    {
                        service.release_window(window.label());
                    }
                })
        }),
        on_exit: None,
    }
}

// Silence unused imports that only the gate helper needs when it is compiled out.
#[allow(dead_code)]
fn _types(_: HostStatus) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commands_the_extension_routes_are_in_the_manifest_and_only_in_the_projects_capability() {
        let routed = extension().commands;
        let build = include_str!("../../build.rs");
        let projects: serde_json::Value =
            serde_json::from_str(include_str!("../../capabilities/projects.json")).unwrap();
        let granted: Vec<&str> = projects["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect();
        let main = include_str!("../../capabilities/main.json");
        let expected: Vec<String> = routed
            .iter()
            .map(|c| format!("allow-{}", c.replace('_', "-")))
            .collect();
        assert_eq!(
            granted, expected,
            "the capability is exactly the routed commands"
        );
        for command in routed {
            assert!(
                build.contains(&format!("\"{command}\"")),
                "{command} not in build.rs"
            );
            assert!(
                !main.contains(&format!("allow-{}", command.replace('_', "-"))),
                "{command} leaked into main"
            );
        }
        // Another window carries this capability and nothing else.
        assert_eq!(projects["identifier"], "projects");
        assert_eq!(projects["windows"], serde_json::json!(["main", "w-*"]));
        let config = include_str!("../../tauri.conf.json");
        assert!(config.contains("\"projects\""));
    }

    #[test]
    fn no_command_takes_a_path_a_command_line_a_prompt_or_a_credential() {
        // Every request type is closed (`deny_unknown_fields`) and none has a field that could
        // carry one: the only free text in the surface is a label (Text, 64 scalars) and a
        // folder name, which the owner accepts only as one plain basename.
        for forbidden in [
            "path",
            "argv",
            "args",
            "env",
            "prompt",
            "token_secret",
            "password",
            "cwd",
        ] {
            let source = include_str!("commands.rs");
            let requests = &source
                [source.find("// ---- requests").unwrap()..source.find("// ---- replies").unwrap()];
            assert!(
                !requests.contains(&format!("    {forbidden}:")),
                "a request names `{forbidden}`"
            );
        }
        let parsed: Result<CursorRequest, _> =
            serde_json::from_value(serde_json::json!({"cursor": null, "path": "/x"}));
        assert!(parsed.is_err());
        let parsed: Result<WriteRequest, _> = serde_json::from_value(
            serde_json::json!({"associationId": "a", "data": [1], "terminalId": "t"}),
        );
        assert!(parsed.is_err());
    }
}
