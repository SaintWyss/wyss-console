//! The window's commands for Arsenal. None takes a path. A discovered skill is named by its
//! identity, and the desktop scans again before it reads or copies anything, so the window can
//! never point the importer at a file of its own choosing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tauri::State;

use super::Arsenal;
use super::store::{
    self, Detail, Error, FileInfo, Imported, NewCapability, Preset, PresetInput, Selection,
    SelectionInput, Store, Summary,
};
use crate::agentconfig::skills::{self, Scan, Skill, SourceReport};

const PREVIEW_BYTES: usize = 1024 * 1024;

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

/// One skill as the window shows it: what the scan read, and how it stands against the library.
#[derive(Debug, Serialize)]
pub struct SkillView {
    pub id: String,
    pub tree_hash: String,
    /// `not_imported`, `imported` (the library holds this content) or `source_changed`.
    pub status: &'static str,
    pub capability: Option<String>,
    /// Other skills in this scan whose content is identical.
    pub duplicates: Vec<String>,
    /// Other skills in this scan with the same name.
    pub name_collisions: Vec<String>,
    #[serde(flatten)]
    pub skill: Skill,
}

#[derive(Debug, Serialize)]
pub struct DiscoverReply {
    pub sources: Vec<SourceReport>,
    pub skills: Vec<SkillView>,
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdRequest {
    id: String,
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
    /// `name_collision`: two capabilities with one name. `duplicate_content`: two with one content.
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

fn identity_of(skill: &Skill) -> String {
    store::identity(skill.provider, "skill", skill.source, &skill.name)
}

fn scan_or_refuse() -> Result<Scan, ArsenalError> {
    skills::scan_process().ok_or(ArsenalError::code("discovery_unavailable"))
}

#[tauri::command]
fn arsenal_discover(arsenal: State<'_, Arsenal>) -> Result<DiscoverReply, ArsenalError> {
    let scan = scan_or_refuse()?;
    arsenal
        .with_store(|store| discover_reply(store, scan))
        .map_err(ArsenalError::from)
}

/// Joins a scan with the library: the status of each skill and what it duplicates or collides with.
fn discover_reply(store: &Store, scan: Scan) -> Result<DiscoverReply, Error> {
    let ids: Vec<String> = scan.skills.iter().map(identity_of).collect();
    let trees: Vec<String> = scan
        .skills
        .iter()
        .map(|skill| store::tree_digest(&skill.content))
        .collect();
    let names: Vec<String> = scan.skills.iter().map(|skill| skill.name.clone()).collect();
    let mut skills = Vec::with_capacity(scan.skills.len());
    for (index, skill) in scan.skills.into_iter().enumerate() {
        let duplicates = others(&ids, index, |other| trees[other] == trees[index]);
        let name_collisions = others(&ids, index, |other| names[other] == names[index]);
        let (status, capability) = match store.current(&ids[index])? {
            None => ("not_imported", None),
            Some(current) if current.tree_hash == trees[index] => ("imported", Some(current.id)),
            Some(current) => ("source_changed", Some(current.id)),
        };
        skills.push(SkillView {
            id: ids[index].clone(),
            tree_hash: trees[index].clone(),
            status,
            capability,
            duplicates,
            name_collisions,
            skill,
        });
    }
    Ok(DiscoverReply {
        sources: scan.sources,
        skills,
        truncated: scan.truncated,
    })
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

/// Imports the skill with this identity, from a fresh scan. A skill that cannot be imported says why.
#[tauri::command]
fn arsenal_import(
    arsenal: State<'_, Arsenal>,
    request: IdRequest,
) -> Result<Imported, ArsenalError> {
    let mut skill = scan_or_refuse()?
        .skills
        .into_iter()
        .find(|skill| identity_of(skill) == request.id)
        .ok_or(ArsenalError::code("not_found"))?;
    if let Some(reason) = skill.blocked {
        return Err(ArsenalError {
            code: "blocked",
            reason: Some(reason),
        });
    }
    let new = NewCapability {
        identity: request.id,
        provider: skill.provider.to_owned(),
        source: skill.source.to_owned(),
        kind: "skill".to_owned(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        origin: skill.origin.clone(),
        files: std::mem::take(&mut skill.content),
    };
    arsenal
        .with_store(|store| store.import(&new))
        .map_err(ArsenalError::from)
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

/// The configuration a selection would make, stated exactly and never as active.
pub fn preview(store: &Store, selection_id: &str) -> Result<Preview, Error> {
    let selection = store
        .selections()?
        .into_iter()
        .find(|selection| selection.id == selection_id)
        .ok_or(Error::NotFound)?;
    let mut items = Vec::with_capacity(selection.items.len());
    // (provider, name, content, capability) of every item that resolves, for the conflict checks.
    let mut resolved: Vec<(String, String, String, String)> = Vec::new();
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
                resolved.push((
                    detail.summary.provider.clone(),
                    detail.summary.name.clone(),
                    tree_hash.clone(),
                    item.capability.clone(),
                ));
                items.push(PreviewItem {
                    capability: item.capability.clone(),
                    state: "resolved",
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
    if files.iter().any(|file| skills::is_script(&file.path)) {
        return (
            "partial",
            "Contains scripts. They are stored and never run by Wyss; use in a launched session is not verified.".into(),
        );
    }
    (
        "unverified",
        "The skill format is known. Use in a launched session is not verified yet.".into(),
    )
}

/// Two or more resolved capabilities with one name, or with one content, are conflicts to show.
fn conflicts(resolved: &[(String, String, String, String)]) -> Vec<Conflict> {
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut by_content: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (_, name, tree, capability) in resolved {
        by_name
            .entry(name.as_str())
            .or_default()
            .push(capability.as_str());
        by_content
            .entry(tree.as_str())
            .or_default()
            .push(capability.as_str());
    }
    let mut conflicts = Vec::new();
    for (code, groups) in [
        ("name_collision", by_name),
        ("duplicate_content", by_content),
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
    use crate::agentconfig::skills::Scan;
    use rusqlite::Connection;

    fn skill(name: &str, body: &[u8]) -> Skill {
        Skill {
            provider: "claude_code",
            source: "claude_user_skills",
            name: name.to_owned(),
            description: String::new(),
            origin: format!("~/.claude/skills/{name}"),
            files: Vec::new(),
            excluded: Vec::new(),
            review: Vec::new(),
            executes_code: false,
            blocked: None,
            content: vec![("SKILL.md".to_owned(), body.to_vec())],
        }
    }

    fn import(store: &mut Store, name: &str, body: &[u8]) -> store::Imported {
        let found = skill(name, body);
        store
            .import(&NewCapability {
                identity: identity_of(&found),
                provider: found.provider.to_owned(),
                source: found.source.to_owned(),
                kind: "skill".to_owned(),
                name: found.name.clone(),
                description: found.description.clone(),
                origin: found.origin.clone(),
                files: found.content,
            })
            .unwrap()
    }

    fn scan(skills: Vec<Skill>) -> Scan {
        Scan {
            sources: Vec::new(),
            skills,
            truncated: false,
        }
    }

    #[test]
    fn a_scan_is_joined_with_the_library_by_content_not_by_name() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        import(&mut store, "design", b"one");
        let reply = discover_reply(
            &store,
            scan(vec![
                skill("design", b"one"),
                skill("tokens", b"two"),
                skill("design", b"changed"),
            ]),
        )
        .unwrap();
        let status: Vec<&str> = reply.skills.iter().map(|skill| skill.status).collect();
        assert_eq!(status, ["imported", "not_imported", "source_changed"]);
        assert!(reply.skills[0].capability.is_some());
    }

    #[test]
    fn identical_skills_in_one_scan_are_marked_as_duplicates_of_each_other() {
        let store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let reply = discover_reply(
            &store,
            scan(vec![skill("design", b"same"), skill("copy", b"same")]),
        )
        .unwrap();
        assert_eq!(reply.skills[0].duplicates, vec![reply.skills[1].id.clone()]);
        assert_eq!(reply.skills[1].duplicates, vec![reply.skills[0].id.clone()]);
        assert!(reply.skills[0].name_collisions.is_empty());
    }

    #[test]
    fn a_selection_previews_as_not_applied_and_names_its_conflicts() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let first = import(&mut store, "design", b"same");
        let second = import(&mut store, "copy", b"same");
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
        assert_eq!(preview.conflicts[0].capabilities.len(), 2);
    }

    #[test]
    fn a_selection_for_another_provider_is_unsupported_for_its_items() {
        let mut store = Store::open(Connection::open_in_memory().unwrap()).unwrap();
        let design = import(&mut store, "design", b"one");
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
    fn a_script_in_a_skill_is_partial_because_it_is_never_run() {
        let files = vec![FileInfo {
            path: "run.sh".to_owned(),
            sha256: String::new(),
            size: 0,
        }];
        let (label, reason) = compatibility("claude_code", "claude_code", &files);
        assert_eq!(label, "partial");
        assert!(reason.contains("never run"));
    }
}
