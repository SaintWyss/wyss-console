//! The window's commands for Arsenal. None takes a path. A discovered capability is named by its
//! identity, and the desktop scans again, with the project it accepted, before it reads or copies
//! anything, so the window can never point the importer at a file of its own choosing.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;

use super::Arsenal;
use super::store::{
    self, Detail, Error, FileInfo, Imported, NewCapability, Preset, PresetInput, Selection,
    SelectionInput, Store, Summary,
};
use crate::agentconfig::catalogue::{
    self, Candidate, Composite, Excluded, FileDigest, ManagedBy, Scan, SourceReport,
};
use crate::configuration::Configuration;

const PREVIEW_BYTES: usize = 1024 * 1024;
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
    /// `supported`, `partial`, `unsupported` or `unverified`.
    pub compatibility: &'static str,
    pub reason: String,
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
    /// Dependencies are not analysed yet; the window says so instead of showing none.
    pub dependencies: &'static str,
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
            arsenal_preview
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

/// What is known about a kind, for its provider. A format is known for a skill, an agent or a rule;
/// whether a launched session uses it is not verified until AM1b.
fn compatibility_of(candidate: &Candidate) -> &'static str {
    match candidate.kind {
        "skill" | "agent" | "rule" if candidate.executes_code => "partial",
        "skill" | "agent" | "rule" => "unverified",
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
    let names: Vec<String> = scan
        .candidates
        .iter()
        .map(|candidate| candidate.name.clone())
        .collect();
    let importable: Vec<bool> = scan.candidates.iter().map(is_importable).collect();
    let mut candidates = Vec::with_capacity(scan.candidates.len());
    let mut details = BTreeMap::new();
    for (index, candidate) in scan.candidates.into_iter().enumerate() {
        let duplicates = others(&ids, index, |other| {
            importable[index] && importable[other] && trees[other] == trees[index]
        });
        let name_collisions = others(&ids, index, |other| {
            importable[index] && importable[other] && names[other] == names[index]
        });
        let (status, capability) =
            if IMPORTABLE.contains(&candidate.kind) || candidate.blocked.is_none() {
                match store.current(&ids[index])? {
                    None => ("not_imported", None),
                    Some(current) if current.tree_hash == trees[index] => {
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

#[tauri::command]
fn arsenal_capabilities(arsenal: State<'_, Arsenal>) -> Result<Vec<Summary>, ArsenalError> {
    arsenal
        .with_store(|store| store.list())
        .map_err(ArsenalError::from)
}

#[tauri::command]
fn arsenal_capability(
    arsenal: State<'_, Arsenal>,
    request: CapabilityRequest,
) -> Result<Detail, ArsenalError> {
    arsenal
        .with_store(|store| store.detail(&request.capability, request.revision))
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
}

#[tauri::command]
fn arsenal_restore(
    arsenal: State<'_, Arsenal>,
    request: RestoreRequest,
) -> Result<Detail, ArsenalError> {
    arsenal
        .with_store(|store| {
            store.restore(&request.capability, request.revision)?;
            store.detail(&request.capability, None)
        })
        .map_err(ArsenalError::from)
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
}

#[tauri::command]
fn arsenal_preset_delete(
    arsenal: State<'_, Arsenal>,
    request: IdRequest,
) -> Result<(), ArsenalError> {
    arsenal
        .with_store(|store| store.delete_preset(&request.id))
        .map_err(ArsenalError::from)
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
}

#[tauri::command]
fn arsenal_selection_delete(
    arsenal: State<'_, Arsenal>,
    request: IdRequest,
) -> Result<(), ArsenalError> {
    arsenal
        .with_store(|store| store.delete_selection(&request.id))
        .map_err(ArsenalError::from)
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

/// A capability of a selection that resolves, for the conflict checks.
struct Resolved {
    capability: String,
    kind: String,
    provider: String,
    name: String,
    tree: String,
}

/// The configuration a selection would make, stated exactly and never as active.
pub fn preview(store: &Store, selection_id: &str) -> Result<Preview, Error> {
    let selection = store
        .selections()?
        .into_iter()
        .find(|selection| selection.id == selection_id)
        .ok_or(Error::NotFound)?;
    let mut items = Vec::with_capacity(selection.items.len());
    let mut resolved: Vec<Resolved> = Vec::new();
    for item in &selection.items {
        let pinned = item.revision.is_some();
        match store.detail(&item.capability, item.revision) {
            Ok(detail) => {
                let tree_hash = detail
                    .revisions
                    .iter()
                    .find(|revision| revision.number == detail.revision)
                    .map(|revision| revision.tree_hash.clone())
                    .unwrap_or_default();
                let (compatibility, reason) =
                    compatibility(&detail.summary.provider, &selection.provider, &detail.files);
                resolved.push(Resolved {
                    capability: item.capability.clone(),
                    kind: detail.summary.kind.clone(),
                    provider: detail.summary.provider.clone(),
                    name: detail.summary.name.clone(),
                    tree: tree_hash.clone(),
                });
                items.push(PreviewItem {
                    capability: item.capability.clone(),
                    state: "resolved",
                    kind: detail.summary.kind,
                    name: detail.summary.name,
                    provider: detail.summary.provider,
                    revision: Some(detail.revision),
                    pinned,
                    tree_hash: Some(tree_hash),
                    compatibility,
                    reason,
                });
            }
            Err(Error::NotFound) => {
                let state = match store.detail(&item.capability, None) {
                    Ok(_) => "revision_missing",
                    Err(_) => "missing",
                };
                items.push(PreviewItem {
                    capability: item.capability.clone(),
                    state,
                    kind: String::new(),
                    name: String::new(),
                    provider: String::new(),
                    revision: item.revision,
                    pinned,
                    tree_hash: None,
                    compatibility: "unverified",
                    reason: "The library no longer holds this capability or revision.".into(),
                });
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Preview {
        status: "not_applied",
        conflicts: conflicts(&resolved),
        items,
        dependencies: "not_analysed",
        selection,
    })
}

/// Compatibility is stated per provider from what is known: the skill format, and whether it holds scripts.
fn compatibility(provider: &str, selected: &str, files: &[FileInfo]) -> (&'static str, String) {
    if provider != selected {
        return (
            "unsupported",
            format!("Built for {provider}; this selection is for {selected}."),
        );
    }
    if files.iter().any(|file| catalogue::is_script(&file.path)) {
        return (
            "partial",
            "Contains scripts. They are stored and never run by Wyss; use in a launched session is not verified.".into(),
        );
    }
    (
        "unverified",
        "The format is known. Use in a launched session is not verified yet.".into(),
    )
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
    fn a_selection_previews_as_not_applied_and_names_its_conflicts() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let first = import(&mut store, &mut candidate("skill", "design", b"same"));
        let second = import(&mut store, &mut candidate("skill", "copy", b"same"));
        let selection = store
            .save_selection(&store::SelectionInput {
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
        assert_eq!(preview.dependencies, "not_analysed");
        assert_eq!(preview.items[0].compatibility, "unverified");
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
        assert_eq!(preview.items[0].compatibility, "unsupported");
    }

    #[test]
    fn a_script_in_a_capability_is_partial_because_it_is_never_run() {
        let files = vec![FileInfo {
            path: "run.sh".to_owned(),
            sha256: String::new(),
            size: 0,
        }];
        let (label, reason) = compatibility("claude_code", "claude_code", &files);
        assert_eq!(label, "partial");
        assert!(reason.contains("never run"));
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
