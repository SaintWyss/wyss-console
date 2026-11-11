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
        AuthorityFacts, AuthorityRef, HostSocket, LaunchError, ManagedAdvert, ManagedLaunch,
        ManagedLaunched, ManagedPort, ManagedRow, OwnerError, RealOwner, advertised_profiles,
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
    authority::Connection,
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

/// Pages of Managed sessions read for one listing.
const WALK_PAGES: usize = 8;

/// P1 (the installed authority's sessions) and P2 (the protocol-2 interface, when the desktop
/// was started against one). It can list and prepare; it has no way to bind, lease or run.
struct ManagedFromApp {
    app: AppHandle,
}

fn advert_of(protocol: ManagedProtocol, facts: AuthorityFacts, own: &str) -> ManagedAdvert {
    ManagedAdvert {
        authority: AuthorityRef {
            protocol,
            store_stream_id: facts.stream_id.clone(),
        },
        profiles: advertised_profiles(protocol, &facts, own),
    }
}

fn profile_of(profile: RuntimeSafetyProfile) -> ManagedSafetyProfile {
    match profile {
        RuntimeSafetyProfile::CodexInferenceV1 | RuntimeSafetyProfile::ClaudeInferenceV1 => {
            ManagedSafetyProfile::Inference
        }
        RuntimeSafetyProfile::CodexLocalToolsV1 | RuntimeSafetyProfile::ClaudeLocalToolsV1 => {
            ManagedSafetyProfile::LocalTools
        }
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
            // A bounded walk of the pages, newest first: a session older than that is not shown
            // as live, and attaching it is refused rather than guessed.
            let mut sessions = Vec::new();
            let mut before = None;
            for _ in 0..WALK_PAGES {
                let view = gui.sessions(before).map_err(|_| OwnerError::HostOffline)?;
                sessions.extend(view.sessions);
                before = view.older;
                if before.is_none() {
                    break;
                }
            }
            read_any = true;
            for row in sessions {
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
            && let Ok(summaries) = {
                let mut all = Vec::new();
                let mut before = None;
                let mut read = true;
                for _ in 0..WALK_PAGES {
                    match service.page(before) {
                        Ok(page) => {
                            all.extend(page.sessions);
                            before = page.older;
                            if before.is_none() {
                                break;
                            }
                        }
                        Err(_) => {
                            read = false;
                            break;
                        }
                    }
                }
                if read { Ok(all) } else { Err(()) }
            }
        {
            read_any = true;
            for summary in summaries {
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

    /// The authority a launch would go to, read now: the protocol-2 interface when it is active,
    /// the installed authority otherwise. What it can do is what its own status shows.
    fn advertised(&self) -> Result<ManagedAdvert, OwnerError> {
        let own = env!("CARGO_PKG_VERSION");
        if let Some(gui) = self.app.try_state::<F8Gui>()
            && gui.is_active()
        {
            let state = gui.state();
            if state.link.state != Connection::Connected {
                return Err(OwnerError::HostOffline);
            }
            let status = state.authority.ok_or(OwnerError::HostOffline)?;
            return Ok(advert_of(
                ManagedProtocol::P2,
                AuthorityFacts {
                    protocol_version: status.protocol_version,
                    storage_schema_version: status.storage_schema_version,
                    service_version: status.service_version,
                    stream_id: status.stream_id.as_str().to_owned(),
                },
                own,
            ));
        }
        let service = self
            .app
            .try_state::<SessionService>()
            .ok_or(OwnerError::Unsupported)?;
        let status = service.status().map_err(|error| match error.code {
            SessionFailure::Incompatible => OwnerError::Unsupported,
            _ => OwnerError::HostOffline,
        })?;
        Ok(advert_of(
            ManagedProtocol::P1,
            AuthorityFacts {
                protocol_version: status.protocol_version,
                storage_schema_version: status.storage_schema_version,
                service_version: status.service_version,
                stream_id: status.stream_id.as_str().to_owned(),
            },
            own,
        ))
    }

    /// Prepares a session, nothing more, in the authority the launch names and in no other. The
    /// operation is the intent, so a retry is the same.
    fn prepare(&self, launch: &ManagedLaunch) -> Result<ManagedLaunched, LaunchError> {
        let intent = WireUuid::try_from(launch.intent.clone())
            .map_err(|_| LaunchError::NotDispatched(OwnerError::Refused))?;
        // The authority that would answer now must be the one the launch is for, and which of the
        // two interfaces it is decided once, from that, not read again.
        let now = self.advertised().map_err(LaunchError::NotDispatched)?;
        if now.authority != launch.authority {
            return Err(LaunchError::NotDispatched(OwnerError::HostChanged));
        }
        let prepared = match launch.authority.protocol {
            ManagedProtocol::P2 => {
                let gui = self
                    .app
                    .try_state::<F8Gui>()
                    .filter(|gui| gui.is_active())
                    .ok_or(LaunchError::NotDispatched(OwnerError::HostChanged))?;
                match gui.prepare(intent) {
                    Ok(receipt) => Ok(receipt.id.as_str().to_owned()),
                    // The authority may have taken the intent; the same intent settles it later.
                    Err(error) => Err(match error.code {
                        F8Failure::Unavailable | F8Failure::Uncertain | F8Failure::Internal => {
                            LaunchError::Ambiguous
                        }
                        F8Failure::Inactive => LaunchError::NotDispatched(OwnerError::Unsupported),
                        _ => LaunchError::NotDispatched(OwnerError::Refused),
                    }),
                }
            }
            ManagedProtocol::P1 => {
                let service = self
                    .app
                    .try_state::<SessionService>()
                    .ok_or(LaunchError::NotDispatched(OwnerError::Unsupported))?;
                match service.prepare(intent) {
                    Ok(receipt) => Ok(receipt.id.as_str().to_owned()),
                    // The authority may have taken the intent: never retried from here.
                    Err(error) => Err(match error.code {
                        SessionFailure::Unavailable
                        | SessionFailure::Uncertain
                        | SessionFailure::Internal => LaunchError::Ambiguous,
                        SessionFailure::Incompatible => {
                            LaunchError::NotDispatched(OwnerError::Unsupported)
                        }
                        _ => LaunchError::NotDispatched(OwnerError::Refused),
                    }),
                }
            }
        }?;
        // The answer carries no identity of its own, so the authority is looked at again: if it is
        // not the one asked, the session may have been prepared elsewhere and is unknown here.
        match self.advertised() {
            Ok(after) if after.authority == launch.authority => Ok(ManagedLaunched {
                protocol: launch.authority.protocol,
                store_stream_id: launch.authority.store_stream_id.clone(),
                session_id: prepared,
            }),
            _ => Err(LaunchError::Ambiguous),
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

/// What the project is made of, read from its folder (technologies, architecture, README, types).
#[tauri::command]
pub async fn projects_insight(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: IdRequest,
) -> Result<super::insight::Insight, Failure> {
    let service = projects.get(&app);
    blocking(move || super::insight::inspect(&service.project_root(&request.id)?)).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadmeRequest {
    id: String,
    content: String,
}

/// Writes the project's README and nothing else; answers the file's name.
#[tauri::command]
pub async fn projects_readme_save(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: ReadmeRequest,
) -> Result<String, Failure> {
    let service = projects.get(&app);
    blocking(move || {
        super::insight::save_readme(&service.project_root(&request.id)?, &request.content)
    })
    .await
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

/// What became of one write, for the pane (REQ-03). `accepted` is what the host itself said
/// it wrote; it is absent when that is not known, and is never derived from an error word.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    /// `written`, `partial` or `unknown`.
    outcome: &'static str,
    accepted: Option<usize>,
}

impl From<super::owner::WriteOutcome> for WriteResult {
    fn from(outcome: super::owner::WriteOutcome) -> Self {
        use super::owner::WriteOutcome::{Partial, Unknown, Written};
        match outcome {
            Written(n) => Self {
                outcome: "written",
                accepted: Some(n),
            },
            Partial(n) => Self {
                outcome: "partial",
                accepted: Some(n),
            },
            Unknown => Self {
                outcome: "unknown",
                accepted: None,
            },
        }
    }
}

#[tauri::command]
pub async fn projects_term_write(
    app: AppHandle,
    window: WebviewWindow,
    projects: State<'_, Projects>,
    request: WriteRequest,
) -> Result<WriteResult, Failure> {
    let service = projects.get(&app);
    let label = window.label().to_owned();
    blocking(move || {
        service
            .write_input(&label, &request.association_id, &request.data)
            .map(WriteResult::from)
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachCommitRequest {
    association_id: String,
    /// The token the attach answer carried; none when that answer could not be read.
    token: Option<String>,
    /// The pane drew the screen it was given.
    drawn: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFrom {
    /// Where in the host's output this pane reads from.
    from: u64,
}

/// The screen a pane draws for a session that was already running, asked of the checked owner.
#[tauri::command]
pub async fn projects_term_attach(
    app: AppHandle,
    window: WebviewWindow,
    projects: State<'_, Projects>,
    request: AssociationRequest,
) -> Result<serde_json::Value, Failure> {
    let service = projects.get(&app);
    let label = window.label().to_owned();
    blocking(move || service.attach_screen(&label, &request.association_id)).await
}

/// The pane says whether it drew the screen; only then is it told where reading goes on.
#[tauri::command]
pub async fn projects_term_attach_commit(
    app: AppHandle,
    window: WebviewWindow,
    projects: State<'_, Projects>,
    request: AttachCommitRequest,
) -> Result<ReadFrom, Failure> {
    if request.token.as_deref().is_some_and(|t| t.len() > 64) {
        return Err(Failure::new(ErrorCode::InvalidRecord).with_field("token"));
    }
    let service = projects.get(&app);
    let label = window.label().to_owned();
    blocking(move || {
        service
            .commit_screen(
                &label,
                &request.association_id,
                request.token.as_deref(),
                request.drawn,
            )
            .map(|from| ReadFrom { from })
    })
    .await
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrphansRequest {
    launch_operation_id: String,
}

#[tauri::command]
pub async fn projects_orphans(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: OrphansRequest,
) -> Result<Vec<super::service::OrphanView>, Failure> {
    let service = projects.get(&app);
    blocking(move || service.orphans(&request.launch_operation_id)).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdoptionRequest {
    association_id: String,
    project_id: String,
}

/// Where a Scratch shell really is, before the user is asked to move it into a project.
#[tauri::command]
pub async fn projects_adoption_preview(
    app: AppHandle,
    projects: State<'_, Projects>,
    request: AdoptionRequest,
) -> Result<super::service::AdoptionView, Failure> {
    let service = projects.get(&app);
    blocking(move || service.preview_adoption(&request.association_id, &request.project_id)).await
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

/// What the native window says when the workspace is turned on or off: its heading, its text and
/// what the affirmative button is called. The button says what it does: it is never a word of
/// another confirmation (a window that asked "Delete" to turn something on was the defect).
fn gate_dialog(on: bool) -> (&'static str, &'static str, &'static str) {
    if on {
        (
            "Use project workspaces",
            "Turn on projects and workspaces. Your sessions and the older windows are not changed or stopped.",
            "Turn on",
        )
    } else {
        (
            "Go back to the older windows",
            "Turn off projects and workspaces. Nothing is stopped; the project data is kept.",
            "Turn off",
        )
    }
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
        let (heading, body, accept) = gate_dialog(request.on);
        if !crate::native_dialog::confirm_text(&app, heading, body, accept) {
            return Err(Failure::new(ErrorCode::OwnerRefused).with_field("declined"));
        }
        service.set_enabled(request.on)?;
        Ok(service.overview())
    })
    .await
}

/// Most extra workspace windows open at once.
const MOST_WINDOWS: usize = 8;

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
    // Each window reads the store on its own poll: eight is more than anyone uses at once.
    if app
        .webview_windows()
        .keys()
        .filter(|label| label.starts_with("w-"))
        .count()
        >= MOST_WINDOWS
    {
        return Err(Failure::new(ErrorCode::CapacityExceeded));
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
    "projects_insight",
    "projects_readme_save",
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
    "projects_term_attach",
    "projects_term_attach_commit",
    "projects_orphans",
    "projects_adoption_preview",
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
            projects_insight,
            projects_readme_save,
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
            projects_term_attach,
            projects_term_attach_commit,
            projects_orphans,
            projects_adoption_preview,
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

#[cfg(test)]
mod gate_dialog_tests {
    use super::gate_dialog;

    #[test]
    fn the_button_that_turns_the_workspace_on_or_off_says_so() {
        let (on_heading, on_body, on) = gate_dialog(true);
        let (off_heading, off_body, off) = gate_dialog(false);
        assert_eq!((on, off), ("Turn on", "Turn off"));
        for text in [on_heading, on_body, off_heading, off_body, on, off] {
            assert!(!text.is_empty());
            assert!(!text.to_lowercase().contains("delete"), "{text}");
        }
        // Both ways say that nothing running is stopped.
        assert!(on_body.contains("not changed or stopped"));
        assert!(off_body.contains("Nothing is stopped"));
    }
}
