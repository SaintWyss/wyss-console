//! The window's commands for Arsenal. None takes a path. A discovered capability is named by its
//! identity, and the desktop scans again, with the project it accepted, before it reads or copies
//! anything, so the window can never point the importer at a file of its own choosing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;

use super::Arsenal;
use super::analysis::{self, Compatibility, Dependency};
use super::store::{
    self, Binding, Detail, Error, Imported, NewCapability, Preset, PresetInput, Selection,
    SelectionInput, Store, Summary,
};
use super::sync;
use super::terminal::{self, TerminalPlan};
use crate::agentconfig::catalogue::{
    self, Candidate, Composite, Excluded, FileDigest, LaunchScan, ManagedBy, Scan, SourceReport,
};
use crate::configuration::Configuration;
use crate::host::{client::HostLink, protocol::HostError};
use crate::native::{arsenal::ArsenalLaunch, capabilities, catalogue::AgentId};

const PREVIEW_BYTES: usize = 1024 * 1024;
/// Most stored bytes one preview or plan reads to analyse dependencies. Past it, the remaining
/// items are judged by their file names only and their dependencies are left unknown.
const ANALYSIS_BYTES: usize = 64 * 1024 * 1024;
/// The kinds Arsenal can copy. Hooks, MCP servers and plugins are listed and never imported.
const IMPORTABLE: [&str; 3] = ["skill", "agent", "rule"];

#[derive(Debug, Serialize)]
pub struct ArsenalError {
    code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

impl ArsenalError {
    fn code(code: &'static str) -> Self {
        Self { code, reason: None }
    }

    #[cfg(test)]
    pub(crate) fn code_of(&self) -> &'static str {
        self.code
    }
}

impl From<Error> for ArsenalError {
    fn from(error: Error) -> Self {
        Self::code(match error {
            Error::Invalid => "invalid_request",
            Error::NotFound => "not_found",
            Error::Busy => "storage_busy",
            Error::Corrupt => "storage_corrupt",
            Error::Storage => "storage_unavailable",
            Error::Unsupported => "storage_unsupported",
        })
    }
}

/// One discovered capability as the list shows it. The files and exclusions of the chosen one come
/// from `arsenal_candidate`, so a long scan stays a small reply.
#[derive(Debug, Clone, Serialize)]
pub struct CandidateView {
    /// Stable identity: the kind, provider, source and name, and the project for a project's own.
    pub id: String,
    pub tree_hash: String,
    /// `not_imported`, `imported`, `source_changed`, or `read_only` for hooks, MCP servers and plugins.
    pub status: &'static str,
    pub importable: bool,
    /// `unverified` when the format is known, `partial` when it holds scripts, `unsupported` when
    /// Arsenal does not handle the kind yet. None of them says a session uses it.
    pub compatibility: &'static str,
    pub capability: Option<String>,
    /// Other importable capabilities with identical content.
    pub duplicates: Vec<String>,
    /// Other importable capabilities with the same name.
    pub name_collisions: Vec<String>,
    pub kind: &'static str,
    pub provider: &'static str,
    pub source: String,
    pub scope: &'static str,
    pub name: String,
    pub description: String,
    pub origin: String,
    pub blocked: Option<&'static str>,
    pub managed_by: Option<ManagedBy>,
    pub executes_code: bool,
    pub file_count: usize,
    pub excluded_count: usize,
    pub review_count: usize,
}

/// What a discovered capability holds: its files, what was left out, and what to review.
#[derive(Debug, Clone, Serialize)]
pub struct CandidateDetail {
    pub id: String,
    pub files: Vec<FileDigest>,
    pub excluded: Vec<Excluded>,
    pub review: Vec<Excluded>,
}

#[derive(Debug, Serialize)]
pub struct DiscoverReply {
    /// The selected project's folder name, when a project is open.
    pub project: Option<String>,
    pub sources: Vec<SourceReport>,
    pub candidates: Vec<CandidateView>,
    pub composites: Vec<Composite>,
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupRequest {
    identity: String,
    group_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRequest {
    provider: String,
    items: Vec<store::Item>,
    composites: Vec<String>,
}

/// The entry an import names, as the last scan showed it. The id is what must match, read again now.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    id: String,
    kind: String,
    source: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequest {
    capability: String,
    revision: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRequest {
    capability: String,
    revision: i64,
    path: String,
}

#[derive(Debug, Serialize)]
pub struct FileText {
    path: String,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    capability: String,
    revision: i64,
}

#[derive(Debug, Serialize)]
pub struct PreviewItem {
    pub capability: String,
    /// `resolved`, `missing` (the capability is gone) or `revision_missing` (the pin is gone).
    pub state: &'static str,
    pub kind: String,
    pub name: String,
    pub provider: String,
    pub revision: Option<i64>,
    pub pinned: bool,
    pub tree_hash: Option<String>,
    /// `supported`, `unsupported`, `unverified`, `requires_qualification` or `missing_dependency`.
    pub compatibility: &'static str,
    pub reason: String,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Serialize)]
pub struct Conflict {
    /// `name_collision`, `duplicate_content`, or `instruction_overlap` (two rules for one provider).
    pub code: &'static str,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub selection: Selection,
    /// Always `not_applied`. AM1a launches nothing, so no saved selection is ever active.
    pub status: &'static str,
    pub items: Vec<PreviewItem>,
    pub conflicts: Vec<Conflict>,
    /// `analysed` when every item's files were read, `partial` when the reading budget ran out.
    pub dependencies: &'static str,
}

/// A library entry with what its format means for its own provider.
#[derive(Debug, Serialize)]
pub struct LibraryItem {
    #[serde(flatten)]
    pub summary: Summary,
    pub compatibility: Compatibility,
}

/// One capability with its compatibility and the dependencies its files declare or point at.
#[derive(Debug, Serialize)]
pub struct DetailView {
    #[serde(flatten)]
    pub detail: Detail,
    pub compatibility: Compatibility,
    pub dependencies: Vec<Dependency>,
}

/// The terminal a plan is for: a saved selection, the agent, and the launcher's project.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalRequest {
    pub selection: String,
    pub agent_id: AgentId,
    /// A project path as the launcher lists it; `None` or the home entry means no project.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindRequest {
    session_id: String,
    selection: String,
    config_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSelectionRequest {
    project: String,
    agent_id: AgentId,
}

const COMMANDS: &[&str] = &[
    "arsenal_discover",
    "arsenal_candidate",
    "arsenal_capabilities",
    "arsenal_capability",
    "arsenal_file",
    "arsenal_import",
    "arsenal_restore",
    "arsenal_presets",
    "arsenal_preset_write",
    "arsenal_preset_delete",
    "arsenal_selections",
    "arsenal_selection_write",
    "arsenal_selection_delete",
    "arsenal_preview",
    "arsenal_preset_plan",
    "arsenal_groups",
    "arsenal_group_move",
    "arsenal_categories",
    "arsenal_category_write",
    "arsenal_terminal_plan",
    "arsenal_terminal_bind",
    "arsenal_terminal_bindings",
    "arsenal_project_selection",
    "arsenal_sync_status",
    "arsenal_sync_now",
];

/// What the desktop binary hands to the library for Arsenal: the commands and the state they use.
pub fn extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: COMMANDS,
        handler: Box::new(tauri::generate_handler![
            arsenal_discover,
            arsenal_candidate,
            arsenal_capabilities,
            arsenal_capability,
            arsenal_file,
            arsenal_import,
            arsenal_restore,
            arsenal_presets,
            arsenal_preset_write,
            arsenal_preset_delete,
            arsenal_selections,
            arsenal_selection_write,
            arsenal_selection_delete,
            arsenal_preview,
            arsenal_preset_plan,
            arsenal_groups,
            arsenal_group_move,
            arsenal_categories,
            arsenal_environment,
            arsenal_environment_options,
            arsenal_environment_write,
            arsenal_roles,
            arsenal_role_write,
            arsenal_category_write,
            arsenal_terminal_plan,
            arsenal_terminal_bind,
            arsenal_terminal_bindings,
            arsenal_project_selection,
            arsenal_sync_status,
            arsenal_sync_now
        ]),
        install: Box::new(|builder| builder.manage(Arsenal::start())),
        on_exit: None,
    }
}

/// The identity a capability is stored and named by. A project's capability carries a digest of the
/// project folder, so two projects with the same file name never share one.
pub fn identity_of(candidate: &Candidate, project: Option<&Path>) -> String {
    let source = match (candidate.scope, project) {
        ("project", Some(project)) => format!("{}@{}", candidate.source, scope_key(project)),
        _ => candidate.source.clone(),
    };
    store::identity(candidate.provider, candidate.kind, &source, &candidate.name)
}

/// The identity of each scanned capability, unique within the scan. Two entries can share one (two
/// hooks with one name in one file); the later ones take their occurrence as a suffix, so the list never
/// repeats a key and an import names the same entry the scan showed. Entries keep their order, so a
/// scan of only one identity numbers its entries the same way.
fn ids_of(candidates: &[Candidate], project: Option<&Path>) -> Vec<String> {
    let mut occurrences: BTreeMap<String, usize> = BTreeMap::new();
    candidates
        .iter()
        .map(|candidate| {
            let identity = identity_of(candidate, project);
            let seen = occurrences.entry(identity.clone()).or_insert(0);
            let id = if *seen == 0 {
                identity
            } else {
                format!("{identity}#{seen}")
            };
            *seen += 1;
            id
        })
        .collect()
}

fn scope_key(project: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(project.as_os_str().as_encoded_bytes())
    )
    .chars()
    .take(16)
    .collect()
}

fn is_importable(candidate: &Candidate) -> bool {
    candidate.blocked.is_none() && IMPORTABLE.contains(&candidate.kind)
}

/// What is known about a kind, for its own provider: the provider's own format, read from its own
/// folder, is `supported`; one holding scripts is `unverified` (their requirements are not checked);
/// hooks, MCP servers and plugins are `unsupported` because Arsenal does not hold them.
fn compatibility_of(candidate: &Candidate) -> &'static str {
    match candidate.kind {
        "skill" | "agent" | "rule" if candidate.executes_code => "unverified",
        "skill" | "agent" | "rule" => "supported",
        _ => "unsupported",
    }
}

#[tauri::command]
async fn arsenal_discover(
    arsenal: State<'_, Arsenal>,
    configuration: State<'_, Configuration>,
) -> Result<DiscoverReply, ArsenalError> {
    let project = configuration.selected_project();
    let store = arsenal.handle().map_err(ArsenalError::from)?;
    let kept = arsenal.details_handle();
    blocking(move || {
        let scan = catalogue::scan_process(project.clone())
            .ok_or(ArsenalError::code("discovery_unavailable"))?;
        let (reply, details) = {
            let store = store
                .lock()
                .map_err(|_| ArsenalError::from(Error::Storage))?;
            discover_reply(&store, scan, project.as_deref()).map_err(ArsenalError::from)?
        };
        *kept.lock().map_err(|_| ArsenalError::code("internal"))? = details;
        Ok(reply)
    })
    .await
}

/// The files and exclusions of a capability from the last scan. It is read, never rescanned.
#[tauri::command]
fn arsenal_candidate(
    arsenal: State<'_, Arsenal>,
    request: IdRequest,
) -> Result<CandidateDetail, ArsenalError> {
    arsenal
        .candidate_detail(&request.id)
        .ok_or(ArsenalError::code("not_found"))
}

/// Runs a scan or an import off the window's thread: reading a large account takes seconds.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ArsenalError> + Send + 'static,
) -> Result<T, ArsenalError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| ArsenalError::code("internal"))?
}

/// Joins a scan with the library: the status of each capability, what it duplicates or collides with.
/// The list keeps counts; the details of each capability are returned beside it.
fn discover_reply(
    store: &Store,
    scan: Scan,
    project: Option<&Path>,
) -> Result<(DiscoverReply, BTreeMap<String, CandidateDetail>), Error> {
    let ids: Vec<String> = ids_of(&scan.candidates, project);
    let trees: Vec<String> = scan
        .candidates
        .iter()
        .map(|candidate| store::tree_digest(&candidate.content))
        .collect();
    // The library slot a capability would occupy: provider, kind, source, scope and name. Two
    // candidates in one slot with different content would overwrite each other; the same name
    // under another provider, kind, source or scope is a different capability.
    let slots: Vec<String> = scan
        .candidates
        .iter()
        .map(|candidate| {
            format!(
                "{}\0{}\0{}\0{}\0{}",
                candidate.provider,
                candidate.kind,
                candidate.source,
                candidate.scope,
                candidate.name
            )
        })
        .collect();
    let importable: Vec<bool> = scan.candidates.iter().map(is_importable).collect();
    let mut candidates = Vec::with_capacity(scan.candidates.len());
    let mut details = BTreeMap::new();
    for (index, candidate) in scan.candidates.into_iter().enumerate() {
        let duplicates = others(&ids, index, |other| {
            importable[index] && importable[other] && trees[other] == trees[index]
        });
        let name_collisions = others(&ids, index, |other| {
            importable[index]
                && importable[other]
                && slots[other] == slots[index]
                && trees[other] != trees[index]
        });
        let (status, capability) =
            if IMPORTABLE.contains(&candidate.kind) || candidate.blocked.is_none() {
                match store.current(&ids[index])? {
                    None => ("not_imported", None),
                    // A blank stored description the scan can now read is worth importing again.
                    Some(current)
                        if current.tree_hash == trees[index]
                            && (!current.description.is_empty()
                                || candidate.description.is_empty()) =>
                    {
                        ("imported", Some(current.id))
                    }
                    Some(current) => ("source_changed", Some(current.id)),
                }
            } else {
                ("read_only", None)
            };
        candidates.push(CandidateView {
            id: ids[index].clone(),
            tree_hash: trees[index].clone(),
            status,
            importable: importable[index],
            compatibility: compatibility_of(&candidate),
            capability,
            duplicates,
            name_collisions,
            kind: candidate.kind,
            provider: candidate.provider,
            source: candidate.source.clone(),
            scope: candidate.scope,
            name: candidate.name.clone(),
            description: candidate.description.clone(),
            origin: candidate.origin.clone(),
            blocked: candidate.blocked,
            managed_by: candidate.managed_by.clone(),
            executes_code: candidate.executes_code,
            file_count: candidate.files.len(),
            excluded_count: candidate.excluded.len(),
            review_count: candidate.review.len(),
        });
        details.insert(
            ids[index].clone(),
            CandidateDetail {
                id: ids[index].clone(),
                files: candidate.files,
                excluded: candidate.excluded,
                review: candidate.review,
            },
        );
    }
    let reply = DiscoverReply {
        project: project
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned),
        sources: scan.sources,
        candidates,
        composites: scan.composites,
        truncated: scan.truncated,
    };
    Ok((reply, details))
}

fn others(ids: &[String], index: usize, same: impl Fn(usize) -> bool) -> Vec<String> {
    (0..ids.len())
        .filter(|&other| other != index && same(other))
        .map(|other| ids[other].clone())
        .collect()
}

/// The library, each entry with the compatibility its detail shows: judged from its files and
/// their dependencies while the reading budget lasts, from its file names after that.
pub fn library(store: &Store) -> Result<Vec<LibraryItem>, Error> {
    let mut budget = ANALYSIS_BYTES;
    store
        .list()?
        .into_iter()
        .map(|summary| {
            let detail = store.detail(&summary.id, None)?;
            let size: usize = detail
                .files
                .iter()
                .map(|f| usize::try_from(f.size).unwrap_or(usize::MAX))
                .sum();
            let dependencies = if size <= budget {
                budget -= size;
                analysis::dependencies(&store.revision_content(&summary.id, detail.revision)?, None)
            } else {
                Vec::new()
            };
            let paths: Vec<&str> = detail.files.iter().map(|f| f.path.as_str()).collect();
            let compatibility = analysis::compatibility(
                &summary.kind,
                &summary.provider,
                &summary.provider,
                &paths,
                &dependencies,
            );
            Ok(LibraryItem {
                summary,
                compatibility,
            })
        })
        .collect()
}

#[tauri::command]
fn arsenal_capabilities(arsenal: State<'_, Arsenal>) -> Result<Vec<LibraryItem>, ArsenalError> {
    arsenal
        .with_store(|store| library(store))
        .map_err(ArsenalError::from)
}

pub fn detail_view(
    store: &Store,
    capability: &str,
    revision: Option<i64>,
) -> Result<DetailView, Error> {
    let detail = store.detail(capability, revision)?;
    let content = store.revision_content(capability, detail.revision)?;
    let dependencies = analysis::dependencies(&content, None);
    let paths: Vec<&str> = detail.files.iter().map(|f| f.path.as_str()).collect();
    let compatibility = analysis::compatibility(
        &detail.summary.kind,
        &detail.summary.provider,
        &detail.summary.provider,
        &paths,
        &dependencies,
    );
    Ok(DetailView {
        detail,
        compatibility,
        dependencies,
    })
}

#[tauri::command]
fn arsenal_capability(
    arsenal: State<'_, Arsenal>,
    request: CapabilityRequest,
) -> Result<DetailView, ArsenalError> {
    arsenal
        .with_store(|store| detail_view(store, &request.capability, request.revision))
        .map_err(ArsenalError::from)
}

#[tauri::command]
fn arsenal_file(
    arsenal: State<'_, Arsenal>,
    request: FileRequest,
) -> Result<FileText, ArsenalError> {
    let bytes = arsenal
        .with_store(|store| store.file(&request.capability, request.revision, &request.path))
        .map_err(ArsenalError::from)?;
    if bytes.len() > PREVIEW_BYTES {
        return Err(ArsenalError::code("too_large"));
    }
    let text = String::from_utf8(bytes).map_err(|_| ArsenalError::code("binary_file"))?;
    Ok(FileText {
        path: request.path,
        text,
    })
}

/// Imports the capability with this identity. It is read from disk now, as a full scan would read it;
/// only the entry the request names is read. One that cannot be imported says why.
#[tauri::command]
async fn arsenal_import(
    arsenal: State<'_, Arsenal>,
    configuration: State<'_, Configuration>,
    request: ImportRequest,
) -> Result<Imported, ArsenalError> {
    if !IMPORTABLE.contains(&request.kind.as_str()) {
        return Err(ArsenalError::code("not_importable"));
    }
    let project = configuration.selected_project();
    let store = arsenal.handle().map_err(ArsenalError::from)?;
    blocking(move || {
        let mut scan = catalogue::scan_entry_process(
            project.clone(),
            &request.kind,
            &request.source,
            &request.name,
        )
        .ok_or(ArsenalError::code("discovery_unavailable"))?;
        let index = ids_of(&scan.candidates, project.as_deref())
            .iter()
            .position(|id| *id == request.id)
            .ok_or(ArsenalError::code("not_found"))?;
        let mut candidate = scan.candidates.swap_remove(index);
        if !IMPORTABLE.contains(&candidate.kind) {
            return Err(ArsenalError {
                code: "not_importable",
                reason: Some("read_only_kind"),
            });
        }
        if let Some(reason) = candidate.blocked {
            return Err(ArsenalError {
                code: "blocked",
                reason: Some(reason),
            });
        }
        let new = NewCapability {
            identity: request.id,
            provider: candidate.provider.to_owned(),
            source: candidate.source.clone(),
            kind: candidate.kind.to_owned(),
            name: candidate.name.clone(),
            description: candidate.description.clone(),
            origin: candidate.origin.clone(),
            files: std::mem::take(&mut candidate.content),
        };
        let mut store = store
            .lock()
            .map_err(|_| ArsenalError::from(Error::Storage))?;
        store.import(&new).map_err(ArsenalError::from)
    })
    .await
    .inspect(|_| arsenal.nudge_sync())
}

#[tauri::command]
fn arsenal_restore(
    arsenal: State<'_, Arsenal>,
    request: RestoreRequest,
) -> Result<DetailView, ArsenalError> {
    arsenal
        .with_store(|store| {
            store.restore(&request.capability, request.revision)?;
            detail_view(store, &request.capability, None)
        })
        .map_err(ArsenalError::from)
        .inspect(|_| arsenal.nudge_sync())
}

#[tauri::command]
fn arsenal_presets(arsenal: State<'_, Arsenal>) -> Result<Vec<Preset>, ArsenalError> {
    arsenal
        .with_store(|store| store.presets())
        .map_err(ArsenalError::from)
}

#[tauri::command]
fn arsenal_preset_write(
    arsenal: State<'_, Arsenal>,
    request: PresetInput,
) -> Result<Preset, ArsenalError> {
    arsenal
        .with_store(|store| store.save_preset(&request))
        .map_err(ArsenalError::from)
        .inspect(|_| arsenal.nudge_sync())
}

#[tauri::command]
fn arsenal_preset_delete(
    arsenal: State<'_, Arsenal>,
    request: IdRequest,
) -> Result<(), ArsenalError> {
    arsenal
        .with_store(|store| store.delete_preset(&request.id))
        .map_err(ArsenalError::from)
        .inspect(|_| arsenal.nudge_sync())
}

#[tauri::command]
fn arsenal_selections(arsenal: State<'_, Arsenal>) -> Result<Vec<Selection>, ArsenalError> {
    arsenal
        .with_store(|store| store.selections())
        .map_err(ArsenalError::from)
}

#[tauri::command]
fn arsenal_selection_write(
    arsenal: State<'_, Arsenal>,
    request: SelectionInput,
) -> Result<Selection, ArsenalError> {
    arsenal
        .with_store(|store| store.save_selection(&request))
        .map_err(ArsenalError::from)
        .inspect(|_| arsenal.nudge_sync())
}

#[tauri::command]
fn arsenal_selection_delete(
    arsenal: State<'_, Arsenal>,
    request: IdRequest,
) -> Result<(), ArsenalError> {
    arsenal
        .with_store(|store| store.delete_selection(&request.id))
        .map_err(ArsenalError::from)
        .inspect(|_| arsenal.nudge_sync())
}

/// Synchronization with the owner's other computers: what this computer published and applied,
/// what waits, what conflicted and what was refused. Reading it changes nothing.
#[tauri::command]
fn arsenal_sync_status(arsenal: State<'_, Arsenal>) -> sync::Status {
    arsenal.sync_status()
}

/// Asks synchronization to run now. The answer is the status before that run; the next read shows it.
#[tauri::command]
fn arsenal_sync_now(arsenal: State<'_, Arsenal>) -> sync::Status {
    arsenal.nudge_sync();
    arsenal.sync_status()
}

#[tauri::command]
fn arsenal_preview(
    arsenal: State<'_, Arsenal>,
    request: IdRequest,
) -> Result<Preview, ArsenalError> {
    arsenal
        .with_store(|store| preview(store, &request.id))
        .map_err(ArsenalError::from)
}

/// Corrections are local to this computer and are not part of Arsenal sync.
#[tauri::command]
fn arsenal_groups(arsenal: State<'_, Arsenal>) -> Result<BTreeMap<String, String>, ArsenalError> {
    arsenal.groups().map_err(ArsenalError::from)
}

#[tauri::command]
fn arsenal_group_move(
    arsenal: State<'_, Arsenal>,
    request: GroupRequest,
) -> Result<(), ArsenalError> {
    match request.group_id {
        Some(group) => arsenal.set_group(&request.identity, &group),
        None => arsenal.clear_group(&request.identity),
    }
    .map_err(ArsenalError::from)
}

/// The launch environment a preset chose; the owner's own configuration when it chose nothing.
#[tauri::command]
fn arsenal_environment(
    selection: String,
) -> Result<crate::native::launch_home::HomeSpec, ArsenalError> {
    let directory = super::environment::directory().ok_or(ArsenalError::from(Error::Storage))?;
    Ok(super::environment::get(&directory, &selection).unwrap_or(
        crate::native::launch_home::HomeSpec {
            instructions: true,
            memory: true,
            ..Default::default()
        },
    ))
}

/// What a preset's launch environment can choose from, read from the owner's `~/.claude` now.
#[tauri::command]
fn arsenal_environment_options() -> Result<crate::native::launch_home::Options, ArsenalError> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or(ArsenalError::from(Error::Storage))?;
    let mut options =
        crate::native::launch_home::options(&crate::native::launch_home::Owner::from_home(&home));
    options.token_stored = crate::native::launch_home::token().is_some();
    Ok(options)
}

/// Records the launch environment of a preset. Local to this computer.
#[tauri::command]
fn arsenal_environment_write(
    selection: String,
    environment: crate::native::launch_home::HomeSpec,
) -> Result<(), ArsenalError> {
    let directory = super::environment::directory().ok_or(ArsenalError::from(Error::Storage))?;
    super::environment::set(&directory, &selection, &environment)
        .map_err(|_| ArsenalError::from(Error::Invalid))
}

/// The roles of Workspace and the role of each launch. Local to this computer.
#[tauri::command]
fn arsenal_roles() -> Result<super::roles::Roles, ArsenalError> {
    let directory = super::environment::directory().ok_or(ArsenalError::from(Error::Storage))?;
    Ok(super::roles::read(&directory))
}

/// Makes or deletes a role of the owner's, or sets, moves or forgets a launch's role.
#[tauri::command]
fn arsenal_role_write(change: super::roles::Change) -> Result<super::roles::Roles, ArsenalError> {
    let directory = super::environment::directory().ok_or(ArsenalError::from(Error::Storage))?;
    super::roles::write(&directory, change).map_err(|refusal| match refusal {
        super::roles::Refusal::Invalid(_) => ArsenalError::from(Error::Invalid),
        super::roles::Refusal::Io => ArsenalError::from(Error::Storage),
    })
}

/// The categories the owner made. Local to this computer, like the corrections.
#[tauri::command]
fn arsenal_categories(
    arsenal: State<'_, Arsenal>,
) -> Result<Vec<super::groups::Custom>, ArsenalError> {
    arsenal.categories().map_err(ArsenalError::from)
}

/// Makes, renames or deletes a category of the owner's; answers the categories after the change.
#[tauri::command]
fn arsenal_category_write(
    arsenal: State<'_, Arsenal>,
    change: super::CategoryChange,
) -> Result<Vec<super::groups::Custom>, ArsenalError> {
    arsenal.write_category(change).map_err(ArsenalError::from)
}

#[tauri::command]
fn arsenal_preset_plan(
    arsenal: State<'_, Arsenal>,
    request: DraftRequest,
) -> Result<Preview, ArsenalError> {
    arsenal
        .with_store(|store| draft_preview(store, request))
        .map_err(ArsenalError::from)
}

fn draft_preview(store: &Store, request: DraftRequest) -> Result<Preview, Error> {
    if !store::is_provider(&request.provider) {
        return Err(Error::Invalid);
    }
    store::check_items(&request.items)?;
    store::check_composites(&request.composites)?;
    preview_selection(
        store,
        Selection {
            id: String::new(),
            name: String::new(),
            provider: request.provider,
            items: request.items,
            composites: request.composites,
            created_at: 0,
            updated_at: 0,
        },
    )
}

/// A capability of a selection that resolves, for the conflict checks.
struct Resolved {
    capability: String,
    kind: String,
    provider: String,
    name: String,
    tree: String,
}

/// The selection's items as the library holds them now, with their files while the reading budget
/// lasts. The second value says whether every resolved item's files were read.
pub fn resolve_items(
    store: &Store,
    selection: &Selection,
) -> Result<(Vec<terminal::Resolved>, bool), Error> {
    let mut budget = ANALYSIS_BYTES;
    let mut complete = true;
    let mut resolved = Vec::with_capacity(selection.items.len());
    for item in &selection.items {
        let pinned = item.revision.is_some();
        match store.detail(&item.capability, item.revision) {
            Ok(detail) => {
                let size: usize = detail
                    .files
                    .iter()
                    .map(|f| usize::try_from(f.size).unwrap_or(usize::MAX))
                    .sum();
                let files = if size <= budget {
                    budget -= size;
                    store.revision_content(&item.capability, detail.revision)?
                } else {
                    complete = false;
                    // Names only: compatibility can still see scripts, dependencies stay unknown.
                    detail
                        .files
                        .iter()
                        .map(|f| (f.path.clone(), Vec::new()))
                        .collect()
                };
                let tree_hash = detail
                    .revisions
                    .iter()
                    .find(|revision| revision.number == detail.revision)
                    .map(|revision| revision.tree_hash.clone())
                    .unwrap_or_default();
                resolved.push(terminal::Resolved {
                    capability: item.capability.clone(),
                    state: "resolved",
                    name: detail.summary.name,
                    kind: detail.summary.kind,
                    provider: detail.summary.provider,
                    revision: Some(detail.revision),
                    pinned,
                    tree_hash,
                    files,
                });
            }
            Err(Error::NotFound) => {
                let state = match store.detail(&item.capability, None) {
                    Ok(_) => "revision_missing",
                    Err(_) => "missing",
                };
                resolved.push(terminal::Resolved {
                    capability: item.capability.clone(),
                    state,
                    name: String::new(),
                    kind: String::new(),
                    provider: String::new(),
                    revision: item.revision,
                    pinned,
                    tree_hash: String::new(),
                    files: Vec::new(),
                });
            }
            Err(error) => return Err(error),
        }
    }
    Ok((resolved, complete))
}

/// The configuration a selection would make, stated exactly and never as active.
pub fn preview(store: &Store, selection_id: &str) -> Result<Preview, Error> {
    let selection = store
        .selections()?
        .into_iter()
        .find(|selection| selection.id == selection_id)
        .ok_or(Error::NotFound)?;
    preview_selection(store, selection)
}

fn preview_selection(store: &Store, selection: Selection) -> Result<Preview, Error> {
    let (resolved_items, complete) = resolve_items(store, &selection)?;
    let mut items = Vec::with_capacity(resolved_items.len());
    let mut resolved: Vec<Resolved> = Vec::new();
    for item in resolved_items {
        if item.state != "resolved" {
            items.push(PreviewItem {
                capability: item.capability,
                state: item.state,
                kind: String::new(),
                name: String::new(),
                provider: String::new(),
                revision: item.revision,
                pinned: item.pinned,
                tree_hash: None,
                compatibility: "unverified",
                reason: "The library no longer holds this capability or revision.".into(),
                dependencies: Vec::new(),
            });
            continue;
        }
        let dependencies = analysis::dependencies(&item.files, None);
        let paths: Vec<&str> = item.files.iter().map(|(path, _)| path.as_str()).collect();
        let verdict = analysis::compatibility(
            &item.kind,
            &item.provider,
            &selection.provider,
            &paths,
            &dependencies,
        );
        resolved.push(Resolved {
            capability: item.capability.clone(),
            kind: item.kind.clone(),
            provider: item.provider.clone(),
            name: item.name.clone(),
            tree: item.tree_hash.clone(),
        });
        items.push(PreviewItem {
            capability: item.capability,
            state: "resolved",
            kind: item.kind,
            name: item.name,
            provider: item.provider,
            revision: item.revision,
            pinned: item.pinned,
            tree_hash: Some(item.tree_hash),
            compatibility: verdict.status,
            reason: verdict.reason,
            dependencies,
        });
    }
    let conflicts = conflicts(&resolved);
    // A capability in a blocking group is conflicting, whatever its format says.
    for conflict in conflicts.iter().filter(|c| c.code == "name_collision") {
        for item in items
            .iter_mut()
            .filter(|item| conflict.capabilities.contains(&item.capability))
        {
            item.compatibility = "conflicting";
            item.reason = "Another capability of this selection has the same name.".into();
        }
    }
    Ok(Preview {
        status: "not_applied",
        conflicts,
        items,
        dependencies: if complete { "analysed" } else { "partial" },
        selection,
    })
}

/// Groups that matter: one name, one content, or two rules for one provider.
fn conflicts(resolved: &[Resolved]) -> Vec<Conflict> {
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut by_content: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut by_rules: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for entry in resolved {
        by_name
            .entry(entry.name.as_str())
            .or_default()
            .push(entry.capability.as_str());
        by_content
            .entry(entry.tree.as_str())
            .or_default()
            .push(entry.capability.as_str());
        if entry.kind == "rule" {
            by_rules
                .entry(entry.provider.as_str())
                .or_default()
                .push(entry.capability.as_str());
        }
    }
    let mut conflicts = Vec::new();
    for (code, groups) in [
        ("name_collision", by_name),
        ("duplicate_content", by_content),
        ("instruction_overlap", by_rules),
    ] {
        for mut ids in groups.into_values() {
            ids.sort_unstable();
            ids.dedup();
            if ids.len() > 1 {
                conflicts.push(Conflict {
                    code,
                    capabilities: ids.into_iter().map(str::to_owned).collect(),
                });
            }
        }
    }
    conflicts
}

fn host_failure(error: HostError) -> ArsenalError {
    ArsenalError {
        code: "host_unavailable",
        reason: match error.code.as_str() {
            "agent_not_installed" => Some("agent_not_installed"),
            "host_outdated" => Some("host_outdated"),
            _ => None,
        },
    }
}

fn provider_word(agent: AgentId) -> Option<&'static str> {
    match agent {
        AgentId::ClaudeCode => Some("claude_code"),
        AgentId::Codex => Some("codex"),
        AgentId::Opencode | AgentId::Hermes => None,
    }
}

/// The terminal's project, accepted only when the host lists it for a launch (or it is the project
/// selected in Configuration), and only after the same root checks Configuration applies. The home
/// entry, or none, means no project folders are read.
pub(crate) fn terminal_project(
    link: &HostLink,
    selected: Option<&Path>,
    requested: Option<&str>,
) -> Result<Option<PathBuf>, ArsenalError> {
    let Some(requested) = requested else {
        return Ok(None);
    };
    if requested.len() > 4096 || requested.contains('\0') {
        return Err(ArsenalError::code("invalid_request"));
    }
    let known = super::super::native::commands::known_projects(link).map_err(host_failure)?;
    let entry = known.iter().find(|(path, _)| path == requested);
    if entry.is_some_and(|(_, home)| *home) {
        return Ok(None);
    }
    let selected_matches = selected.is_some_and(|path| path == Path::new(requested));
    if entry.is_none() && !selected_matches {
        return Err(ArsenalError::code("project_unknown"));
    }
    catalogue::validate_project_process(Path::new(requested))
        .map(Some)
        .ok_or(ArsenalError::code("project_refused"))
}

/// One terminal plan, computed now: the selection as stored, the installed skills as read now, and
/// the provider build as the host would launch it. Returns the plan and the installed skills.
fn plan_now(
    store: &Arc<Mutex<Store>>,
    link: &HostLink,
    selected: Option<&Path>,
    request: &TerminalRequest,
) -> Result<(TerminalPlan, LaunchScan), ArsenalError> {
    let provider =
        provider_word(request.agent_id).ok_or(ArsenalError::code("provider_unsupported"))?;
    let project = terminal_project(link, selected, request.project.as_deref())?;
    let version = super::super::native::commands::provider_version(link, request.agent_id)
        .map_err(host_failure)?;
    let (selection, items) = {
        let store = store
            .lock()
            .map_err(|_| ArsenalError::from(Error::Storage))?;
        let selection = store
            .selections()?
            .into_iter()
            .find(|selection| selection.id == request.selection)
            .ok_or(ArsenalError::code("not_found"))?;
        let (items, _) = resolve_items(&store, &selection)?;
        (selection, items)
    };
    if selection.provider != provider {
        return Err(ArsenalError {
            code: "invalid_request",
            reason: Some("selection_for_another_provider"),
        });
    }
    let installed = catalogue::launch_scan_process(project.clone(), provider)
        .ok_or(ArsenalError::code("discovery_unavailable"))?;
    let evidence = capabilities::arsenal_exclusion(request.agent_id, version.as_deref());
    let plan = terminal::plan(terminal::Input {
        selection: &selection,
        items,
        provider,
        version,
        evidence,
        installed: &installed,
        project: project.as_ref().map(|path| {
            (
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("project")
                    .to_owned(),
                path.to_string_lossy().into_owned(),
            )
        }),
    });
    let mut plan = plan;
    if provider == "claude_code"
        && let Some(spec) = super::environment::directory()
            .and_then(|dir| super::environment::get(&dir, &plan.selection))
    {
        let material = serde_json::to_vec(&(&plan.config_hash, &spec))
            .map_err(|_| ArsenalError::from(Error::Storage))?;
        plan.config_hash = format!("{:x}", sha2::Sha256::digest(material));
        plan.environment = Some(spec);
    }
    Ok((plan, installed))
}

/// The exclusion a start carries, rebuilt from the library and the disk right before the start.
/// It must be the configuration the window previewed (same hash), applicable on this exact build,
/// and pass the allow-list check; otherwise the start is refused and nothing is launched.
pub(crate) fn launch_for(
    store: Option<Arc<Mutex<Store>>>,
    link: &HostLink,
    selected: Option<&Path>,
    request: &TerminalRequest,
    config_hash: &str,
) -> Result<ArsenalLaunch, HostError> {
    let store = store.ok_or_else(|| HostError::new("arsenal_unavailable"))?;
    let (plan, installed) = plan_now(&store, link, selected, request).map_err(|error| {
        HostError::new(match error.code {
            "not_found" => "arsenal_selection_unknown",
            "project_unknown" | "project_refused" => "arsenal_project_refused",
            _ => "arsenal_unavailable",
        })
    })?;
    if plan.config_hash != config_hash {
        return Err(HostError::new("arsenal_plan_stale"));
    }
    if plan.enforcement.state != "available" {
        return Err(HostError::new("arsenal_unqualified"));
    }
    if !plan.can_apply {
        return Err(HostError::new("arsenal_blocked"));
    }
    terminal::validate_exclusion(&plan.excluded, &installed)
        .map_err(|_| HostError::new("arsenal_blocked"))?;
    terminal::validate_files(&plan.excluded_files, &installed)
        .map_err(|_| HostError::new("arsenal_blocked"))?;
    Ok(ArsenalLaunch {
        selection: plan.selection,
        selection_name: plan.selection_name,
        config_hash: plan.config_hash,
        skills_off: plan.excluded,
        skill_files_off: plan.excluded_files,
        home: plan.environment,
    })
}

#[tauri::command]
async fn arsenal_terminal_plan(
    arsenal: State<'_, Arsenal>,
    configuration: State<'_, Configuration>,
    link: State<'_, HostLink>,
    request: TerminalRequest,
) -> Result<TerminalPlan, ArsenalError> {
    let store = arsenal.handle().map_err(ArsenalError::from)?;
    let selected = configuration.selected_project();
    let link = link.inner().clone();
    blocking(move || plan_now(&store, &link, selected.as_deref(), &request).map(|(plan, _)| plan))
        .await
}

fn hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A binding as the window reads it: the record, its plan as data, and what was observed.
fn binding_view(binding: Binding) -> serde_json::Value {
    let plan: serde_json::Value = serde_json::from_str(&binding.plan).unwrap_or_default();
    let mut value = serde_json::to_value(&binding).unwrap_or_default();
    value["plan"] = plan;
    // No provider listing is read back from a launched session yet, so nothing is "verified".
    value["verification"] = serde_json::Value::String("not_observed".into());
    value
}

/// Records what a terminal started with. Whether the selection was applied is read from the host's
/// own session record, never taken from the window: the session must carry this exact exclusion.
#[tauri::command]
async fn arsenal_terminal_bind(
    arsenal: State<'_, Arsenal>,
    configuration: State<'_, Configuration>,
    link: State<'_, HostLink>,
    request: BindRequest,
) -> Result<serde_json::Value, ArsenalError> {
    if !hex(&request.session_id, 32) || !hex(&request.config_hash, 64) {
        return Err(ArsenalError::code("invalid_request"));
    }
    let store = arsenal.handle().map_err(ArsenalError::from)?;
    let selected = configuration.selected_project();
    let link = link.inner().clone();
    blocking(move || {
        let params = serde_json::json!({"sample": false});
        let sessions = match link.call_attached("native_sessions_extended", params.clone()) {
            Err(error) if error.code == "unknown_op" => {
                link.call_attached("native_sessions", params)
            }
            answer => answer,
        }
        .map_err(host_failure)?;
        let session = sessions
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["id"].as_str() == Some(request.session_id.as_str()))
            })
            .cloned()
            .ok_or(ArsenalError::code("session_unknown"))?;
        let agent: AgentId = serde_json::from_value(session["agent_id"].clone())
            .map_err(|_| ArsenalError::code("provider_unsupported"))?;
        let project = session["project"]
            .as_str()
            .ok_or(ArsenalError::code("session_unknown"))?
            .to_owned();
        let terminal_request = TerminalRequest {
            selection: request.selection.clone(),
            agent_id: agent,
            project: Some(project.clone()),
        };
        let (plan, _) = plan_now(&store, &link, selected.as_deref(), &terminal_request)?;
        let launched = &session["arsenal"];
        let applied = launched["config_hash"].as_str() == Some(request.config_hash.as_str())
            && launched["selection"].as_str() == Some(request.selection.as_str());
        let mut reason = if applied {
            "Started with this exact exclusion. Whether the provider used it is not observed."
                .to_owned()
        } else if plan.enforcement.state != "available" {
            plan.enforcement.reason.clone()
        } else {
            "Started without applying the selection.".to_owned()
        };
        if plan.config_hash != request.config_hash {
            reason.push_str(" The configuration changed between the preview and this record.");
        }
        let binding = Binding {
            session_id: request.session_id,
            selection_id: plan.selection.clone(),
            selection_name: plan.selection_name.clone(),
            provider: plan.provider.clone(),
            provider_version: plan.provider_version.clone(),
            project,
            config_hash: request.config_hash,
            state: if applied { "applied" } else { "not_applied" }.to_owned(),
            reason: reason.chars().take(500).collect(),
            plan: serde_json::to_string(&plan).map_err(|_| ArsenalError::code("internal"))?,
            created_at: i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            )
            .unwrap_or(0),
        };
        store
            .lock()
            .map_err(|_| ArsenalError::from(Error::Storage))?
            .bind_terminal(&binding)?;
        Ok(binding_view(binding))
    })
    .await
}

/// Reconcile only host-attested restarts with an existing immutable launch snapshot.
/// A restart is a new session; it must not disappear from Arsenal merely because the
/// Workspace's original Start button was not involved. Never infer provider use.
fn record_restarted_bindings(store: &mut Store, sessions: &serde_json::Value) -> Result<(), Error> {
    let Some(sessions) = sessions.as_array() else {
        return Ok(());
    };
    let mut bindings = store.bindings()?;
    let selections = store.selections()?;
    // Repeated passes also cover a chain returned in newest-first order.
    for _ in 0..sessions.len() {
        let mut added = false;
        for session in sessions {
            let Some(id) = session["id"].as_str().filter(|id| hex(id, 32)) else {
                continue;
            };
            if bindings.iter().any(|b| b.session_id == id) {
                continue;
            }
            let Some(parent) = session["restart_of"].as_str() else {
                continue;
            };
            let Some(previous) = bindings.iter().find(|b| b.session_id == parent) else {
                continue;
            };
            if session["project"].as_str() != Some(previous.project.as_str())
                || session["agent_id"].as_str() != Some(previous.provider.as_str())
                || session["version"].as_str() != previous.provider_version.as_deref()
            {
                continue;
            }
            let plan: serde_json::Value = serde_json::from_str(&previous.plan).unwrap_or_default();
            let launched = &session["arsenal"];
            let same_launch = if previous.state == "applied" {
                launched["selection"].as_str() == Some(previous.selection_id.as_str())
                    && launched["config_hash"].as_str() == Some(previous.config_hash.as_str())
                    && plan["config_hash"].as_str() == Some(previous.config_hash.as_str())
                    && launched["skills_off"].is_array()
                    && launched["skills_off"] == plan["excluded"]
            } else {
                launched.is_null()
            };
            if !same_launch {
                continue;
            }
            let mut binding = previous.clone();
            binding.session_id = id.to_owned();
            binding.created_at = session["started_at"]
                .as_i64()
                .unwrap_or(previous.created_at);
            // A deleted selection must not break listing the surviving historical records.
            if !selections.iter().any(|s| s.id == binding.selection_id) {
                continue;
            }
            store.bind_restarted_terminal(&binding)?;
            bindings.push(binding);
            added = true;
        }
        if !added {
            break;
        }
    }
    Ok(())
}

#[tauri::command]
async fn arsenal_terminal_bindings(
    arsenal: State<'_, Arsenal>,
    link: State<'_, HostLink>,
) -> Result<Vec<serde_json::Value>, ArsenalError> {
    let store = arsenal.handle().map_err(ArsenalError::from)?;
    let link = link.inner().clone();
    blocking(move || {
        let params = serde_json::json!({"sample": false});
        let sessions = match link.call_attached("native_sessions_extended", params.clone()) {
            Err(error) if error.code == "unknown_op" => {
                link.call_attached("native_sessions", params)
            }
            answer => answer,
        };
        let mut store = store
            .lock()
            .map_err(|_| ArsenalError::from(Error::Storage))?;
        // An offline host does not make the already recorded configurations disappear.
        if let Ok(sessions) = sessions {
            record_restarted_bindings(&mut store, &sessions)?;
        }
        store
            .bindings()
            .map(|rows| rows.into_iter().map(binding_view).collect())
            .map_err(ArsenalError::from)
    })
    .await
}

/// The selection this project last started a terminal of this provider with. A lookup by name only:
/// nothing is read from the project folder.
#[tauri::command]
fn arsenal_project_selection(
    arsenal: State<'_, Arsenal>,
    request: ProjectSelectionRequest,
) -> Result<Option<String>, ArsenalError> {
    let provider =
        provider_word(request.agent_id).ok_or(ArsenalError::code("provider_unsupported"))?;
    if request.project.len() > 4096 {
        return Err(ArsenalError::code("invalid_request"));
    }
    arsenal
        .with_store(|store| store.project_selection(&request.project, provider))
        .map_err(ArsenalError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn candidate(kind: &'static str, name: &str, body: &[u8]) -> Candidate {
        Candidate {
            kind,
            provider: "claude_code",
            source: format!("claude_user_{}s", kind),
            scope: "user",
            name: name.to_owned(),
            description: String::new(),
            origin: format!("~/.claude/{kind}s/{name}"),
            files: Vec::new(),
            excluded: Vec::new(),
            review: Vec::new(),
            executes_code: false,
            blocked: None,
            managed_by: None,
            content: vec![(
                if kind == "skill" {
                    "SKILL.md"
                } else {
                    "file.md"
                }
                .to_owned(),
                body.to_vec(),
            )],
        }
    }

    fn import(store: &mut Store, candidate: &mut Candidate) -> store::Imported {
        store
            .import(&NewCapability {
                identity: identity_of(candidate, None),
                provider: candidate.provider.to_owned(),
                source: candidate.source.clone(),
                kind: candidate.kind.to_owned(),
                name: candidate.name.clone(),
                description: candidate.description.clone(),
                origin: candidate.origin.clone(),
                files: std::mem::take(&mut candidate.content),
            })
            .unwrap()
    }

    fn scan(candidates: Vec<Candidate>) -> Scan {
        Scan {
            sources: Vec::new(),
            candidates,
            composites: Vec::new(),
            truncated: false,
        }
    }

    #[test]
    fn restarted_terminals_keep_only_host_attested_configuration_snapshots() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let selection = store
            .save_selection(&store::SelectionInput {
                id: None,
                name: "Restart fixture".into(),
                provider: "claude_code".into(),
                items: vec![],
                composites: vec![],
            })
            .unwrap();
        let original = Binding {
            session_id: "a".repeat(32),
            selection_id: selection.id.clone(),
            selection_name: selection.name,
            provider: "claude_code".into(),
            provider_version: Some("2.1.314 (Claude Code)".into()),
            project: "/tmp/fixture".into(),
            config_hash: "b".repeat(64),
            state: "applied".into(),
            reason: "Use not observed".into(),
            plan: serde_json::json!({"config_hash": "b".repeat(64), "excluded": ["other"]})
                .to_string(),
            created_at: 1,
        };
        store.bind_terminal(&original).unwrap();
        let restarted = serde_json::json!({
            "id": "c".repeat(32), "restart_of": original.session_id,
            "project": original.project, "agent_id": original.provider,
            "version": original.provider_version, "started_at": 2,
            "arsenal": {"selection": original.selection_id,
                "config_hash": original.config_hash, "skills_off": ["other"]}
        });
        // Every authority/binding field must agree; unrelated sessions cannot inherit APPLIED.
        for (pointer, value) in [
            ("/restart_of", serde_json::json!("d".repeat(32))),
            ("/project", serde_json::json!("/tmp/elsewhere")),
            ("/agent_id", serde_json::json!("codex")),
            ("/version", serde_json::json!("2.1.315 (Claude Code)")),
            ("/arsenal/selection", serde_json::json!("other")),
            ("/arsenal/config_hash", serde_json::json!("d".repeat(64))),
            ("/arsenal/skills_off", serde_json::json!([])),
            ("/arsenal", serde_json::Value::Null),
        ] {
            let mut changed = restarted.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            record_restarted_bindings(&mut store, &serde_json::json!([changed])).unwrap();
            assert_eq!(
                store.bindings().unwrap(),
                vec![original.clone()],
                "{pointer}"
            );
        }
        let mut successor = restarted.clone();
        successor["id"] = serde_json::json!("d".repeat(32));
        successor["restart_of"] = restarted["id"].clone();
        let latest = store
            .save_selection(&store::SelectionInput {
                id: None,
                name: "Later choice".into(),
                provider: "claude_code".into(),
                items: vec![],
                composites: vec![],
            })
            .unwrap();
        let mut later = original.clone();
        later.session_id = "1".repeat(32);
        later.selection_id = latest.id.clone();
        later.created_at = 100;
        store.bind_terminal(&later).unwrap();
        let sessions = serde_json::json!([successor, restarted]);
        record_restarted_bindings(&mut store, &sessions).unwrap();
        record_restarted_bindings(&mut store, &sessions).unwrap();
        let rows = store.bindings().unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(
            store
                .project_selection(&original.project, &original.provider)
                .unwrap(),
            Some(latest.id)
        );
        let recovered = rows
            .iter()
            .find(|r| r.session_id == "c".repeat(32))
            .unwrap();
        store.bind_restarted_terminal(recovered).unwrap();
        assert_eq!(store.bindings().unwrap().len(), 4);
        assert!(
            rows.iter()
                .all(|r| r.state == "applied" && r.plan == original.plan)
        );
        assert!(
            rows.iter()
                .all(|r| binding_view(r.clone())["verification"] == "not_observed")
        );
        // NOT APPLIED remains NOT APPLIED on restart, with no fabricated enforcement.
        let mut unqualified = original.clone();
        unqualified.session_id = "e".repeat(32);
        unqualified.state = "not_applied".into();
        store.bind_terminal(&unqualified).unwrap();
        let mut session = sessions[1].clone();
        session["id"] = serde_json::json!("f".repeat(32));
        session["restart_of"] = serde_json::json!(unqualified.session_id);
        session["arsenal"] = serde_json::Value::Null;
        record_restarted_bindings(&mut store, &serde_json::json!([session])).unwrap();
        assert!(
            store
                .bindings()
                .unwrap()
                .iter()
                .any(|r| r.session_id == "f".repeat(32) && r.state == "not_applied")
        );
    }

    #[test]
    fn a_scan_is_joined_with_the_library_by_content_not_by_name() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        import(&mut store, &mut candidate("skill", "design", b"one"));
        let (reply, _) = discover_reply(
            &store,
            scan(vec![
                candidate("skill", "design", b"one"),
                candidate("skill", "tokens", b"two"),
            ]),
            None,
        )
        .unwrap();
        let status: Vec<&str> = reply.candidates.iter().map(|view| view.status).collect();
        assert_eq!(status, ["imported", "not_imported"]);
        assert!(reply.candidates[0].capability.is_some());
        // The same name with changed content is the same capability in a later scan.
        let (changed, _) = discover_reply(
            &store,
            scan(vec![candidate("skill", "design", b"changed")]),
            None,
        )
        .unwrap();
        assert_eq!(changed.candidates[0].status, "source_changed");
    }

    #[test]
    fn a_blank_stored_description_the_scan_can_now_read_is_worth_importing_again() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let mut blank = candidate("agent", "reviewer", b"same");
        blank.description = String::new();
        import(&mut store, &mut blank);
        let mut read = candidate("agent", "reviewer", b"same");
        read.description = "Reviews code".into();
        let (reply, _) = discover_reply(&store, scan(vec![read.clone()]), None).unwrap();
        assert_eq!(reply.candidates[0].status, "source_changed");
        let rescan = read.clone();
        import(&mut store, &mut read);
        let (again, _) = discover_reply(&store, scan(vec![rescan]), None).unwrap();
        assert_eq!(again.candidates[0].status, "imported");
    }

    #[test]
    fn read_only_kinds_are_listed_never_imported_and_say_so() {
        let store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let mut hook = candidate("hook", "pre-commit", b"");
        hook.blocked = Some("read_only_kind");
        hook.content.clear();
        let (reply, _) = discover_reply(&store, scan(vec![hook]), None).unwrap();
        assert_eq!(reply.candidates[0].status, "read_only");
        assert!(!reply.candidates[0].importable);
        assert_eq!(reply.candidates[0].compatibility, "unsupported");
    }

    #[test]
    fn identical_capabilities_in_one_scan_duplicate_each_other_and_a_project_is_its_own_identity() {
        let store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let (reply, _) = discover_reply(
            &store,
            scan(vec![
                candidate("skill", "design", b"same"),
                candidate("skill", "copy", b"same"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(
            reply.candidates[0].duplicates,
            vec![reply.candidates[1].id.clone()]
        );
        let mut local = candidate("skill", "design", b"same");
        local.scope = "project";
        local.source = "claude_project_skills".into();
        let project_a = identity_of(&local, Some(Path::new("/home/u/a")));
        let project_b = identity_of(&local, Some(Path::new("/home/u/b")));
        assert_ne!(project_a, project_b);
        assert_ne!(
            project_a,
            identity_of(&candidate("skill", "design", b"same"), None)
        );
    }

    #[test]
    fn a_name_is_a_collision_only_inside_one_library_slot_with_different_content() {
        let store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let mut other_provider = candidate("skill", "design", b"other");
        other_provider.provider = "codex";
        let (reply, _) = discover_reply(
            &store,
            scan(vec![
                candidate("skill", "design", b"claude"),
                other_provider,
                candidate("skill", "design", b"claude"),
                candidate("skill", "design", b"changed"),
            ]),
            None,
        )
        .unwrap();
        let [claude, codex, same, changed] = &reply.candidates[..] else {
            panic!("four candidates expected");
        };
        // Another provider is its own namespace: no conflict, not even by name.
        assert!(codex.name_collisions.is_empty());
        assert!(!claude.name_collisions.contains(&codex.id));
        // Identical content in the same slot is a duplicate, not a conflict.
        assert_eq!(claude.duplicates, vec![same.id.clone()]);
        assert!(!claude.name_collisions.contains(&same.id));
        // Different content in the same slot would overwrite each other: a real conflict.
        assert_eq!(claude.name_collisions, vec![changed.id.clone()]);
        assert_eq!(changed.name_collisions.len(), 2);
    }

    #[test]
    fn draft_preview_matches_saved_preview_without_writing() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let capability = import(&mut store, &mut candidate("skill", "design", b"same")).capability;
        let items = vec![store::Item {
            capability,
            revision: None,
        }];
        let selection = store
            .save_selection(&store::SelectionInput {
                id: None,
                name: "Frontend".into(),
                provider: "claude_code".into(),
                items: items.clone(),
                composites: vec![],
            })
            .unwrap();
        let before = store.selections().unwrap();
        let draft = draft_preview(
            &store,
            DraftRequest {
                provider: "claude_code".into(),
                items,
                composites: vec![],
            },
        )
        .unwrap();
        let saved = preview(&store, &selection.id).unwrap();
        let mut a = serde_json::to_value(draft).unwrap();
        let mut b = serde_json::to_value(saved).unwrap();
        a.as_object_mut().unwrap().remove("selection");
        b.as_object_mut().unwrap().remove("selection");
        assert_eq!(a, b);
        assert_eq!(store.selections().unwrap(), before);
        assert!(matches!(
            draft_preview(
                &store,
                DraftRequest {
                    provider: "unknown".into(),
                    items: vec![],
                    composites: vec![],
                }
            ),
            Err(Error::Invalid)
        ));
    }

    #[test]
    fn a_selection_previews_as_not_applied_and_names_its_conflicts() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let first = import(&mut store, &mut candidate("skill", "design", b"same"));
        let second = import(&mut store, &mut candidate("skill", "copy", b"same"));
        let selection = store
            .save_selection(&store::SelectionInput {
                composites: Vec::new(),
                id: None,
                name: "Frontend".into(),
                provider: "claude_code".into(),
                items: vec![
                    store::Item {
                        capability: first.capability.clone(),
                        revision: None,
                    },
                    store::Item {
                        capability: second.capability.clone(),
                        revision: None,
                    },
                ],
            })
            .unwrap();
        let preview = preview(&store, &selection.id).unwrap();
        assert_eq!(preview.status, "not_applied");
        assert_eq!(preview.dependencies, "analysed");
        assert_eq!(preview.items[0].compatibility, "supported");
        assert_eq!(preview.conflicts.len(), 1);
        assert_eq!(preview.conflicts[0].code, "duplicate_content");
    }

    #[test]
    fn two_rules_for_one_provider_are_an_instruction_overlap() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let mut style = candidate("rule", "style", b"two spaces");
        let mut brief = candidate("rule", "brief", b"be brief");
        let first = import(&mut store, &mut style);
        let second = import(&mut store, &mut brief);
        let selection = store
            .save_selection(&store::SelectionInput {
                composites: Vec::new(),
                id: None,
                name: "Rules".into(),
                provider: "claude_code".into(),
                items: vec![
                    store::Item {
                        capability: first.capability,
                        revision: None,
                    },
                    store::Item {
                        capability: second.capability,
                        revision: None,
                    },
                ],
            })
            .unwrap();
        let preview = preview(&store, &selection.id).unwrap();
        assert!(
            preview
                .conflicts
                .iter()
                .any(|conflict| conflict.code == "instruction_overlap")
        );
    }

    #[test]
    fn a_selection_for_another_provider_is_unsupported_for_its_items() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let design = import(&mut store, &mut candidate("skill", "design", b"one"));
        let selection = store
            .save_selection(&store::SelectionInput {
                composites: Vec::new(),
                id: None,
                name: "Codex".into(),
                provider: "codex".into(),
                items: vec![store::Item {
                    capability: design.capability,
                    revision: None,
                }],
            })
            .unwrap();
        let preview = preview(&store, &selection.id).unwrap();
        // Codex reads the same SKILL.md format; this copy still needs a qualified way in.
        assert_eq!(preview.items[0].compatibility, "requires_qualification");
    }

    #[test]
    fn duplicates_are_numbered_within_their_identity_so_a_one_entry_scan_names_the_same_ids() {
        let full = [
            candidate("skill", "design", b"one"),
            candidate("skill", "tokens", b"two"),
            candidate("skill", "design", b"three"),
        ];
        let full_ids = ids_of(&full, None);
        assert_eq!(full_ids[2], format!("{}#1", full_ids[0]));
        let one_identity = [
            candidate("skill", "design", b"one"),
            candidate("skill", "design", b"three"),
        ];
        assert_eq!(
            ids_of(&one_identity, None),
            [full_ids[0].clone(), full_ids[2].clone()]
        );
    }
}
