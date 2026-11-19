
use super::{
    store::{Error, Selection, Store, Summary},
    terminal,
};

/// Maximum stored bytes read for dependency analysis in one operation.
const ANALYSIS_BYTES: usize = 64 * 1024 * 1024;

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

/// Checks which library revisions are already installed, without calling the host or
/// asking for provider enforcement. This is the item-only part of Workspace's preview.
pub fn installed_items(
    store: &Store,
    provider: &str,
    directory: &std::path::Path,
    chosen: &[super::store::Item],
    library: &[Summary],
) -> Option<Vec<terminal::PlanItem>> {
    use crate::agentconfig::catalogue;

    let project = catalogue::validate_project_process(directory);
    let installed = catalogue::launch_scan_process(project, provider)?;
    let items = library
        .iter()
        .filter(|item| item.provider == provider && item.kind == "skill")
        .map(|item| {
            chosen
                .iter()
                .find(|pin| pin.capability == item.id)
                .cloned()
                .unwrap_or_else(|| super::store::Item {
                    capability: item.id.clone(),
                    revision: None,
                })
        })
        .collect::<Vec<_>>();
    let mut results = Vec::new();
    for batch in items.chunks(512) {
        let selection = Selection {
            id: String::new(),
            name: String::new(),
            provider: provider.into(),
            items: batch.to_vec(),
            composites: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let (items, _) = resolve_items(store, &selection).ok()?;
        results.extend(
            terminal::plan(terminal::Input {
                selection: &selection,
                items,
                provider,
                version: None,
                evidence: None,
                installed: &installed,
                project: None,
            })
            .items,
        );
    }
    Some(results)
}
