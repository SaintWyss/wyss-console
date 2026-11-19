use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::agentconfig::catalogue::{self, LaunchScan};

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

const SCAN_TTL: Duration = Duration::from_secs(30);
const MAX_CACHED_SCANS: usize = 16;

type ScanKey = (String, Option<PathBuf>);
type ScanEntries = BTreeMap<ScanKey, (Instant, Arc<LaunchScan>)>;

/// Workspace-only cache. Launch enforcement continues to read the live provider state.
#[derive(Default)]
pub struct InstalledScanCache {
    entries: Mutex<ScanEntries>,
}

impl InstalledScanCache {
    pub fn scan(&self, provider: &str, directory: &Path) -> Option<Arc<LaunchScan>> {
        let project = catalogue::validate_project_process(directory);
        self.get(
            provider,
            project,
            Instant::now,
            catalogue::launch_scan_process,
        )
    }

    pub fn invalidate(&self) {
        self.entries.lock().unwrap().clear();
    }

    fn get(
        &self,
        provider: &str,
        project: Option<PathBuf>,
        clock: impl Fn() -> Instant,
        scan: impl FnOnce(Option<PathBuf>, &str) -> Option<LaunchScan>,
    ) -> Option<Arc<LaunchScan>> {
        let key = (provider.to_owned(), project.clone());
        // Coalesce overlapping requests, including adjacent launches using the same project.
        let mut entries = self.entries.lock().unwrap();
        let now = clock();
        entries.retain(|_, (completed, _)| now.duration_since(*completed) < SCAN_TTL);
        if let Some((_, installed)) = entries.get(&key) {
            return Some(installed.clone());
        }
        let installed = Arc::new(scan(project, provider)?);
        if entries.len() == MAX_CACHED_SCANS
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (completed, _))| *completed)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(key, (clock(), installed.clone()));
        Some(installed)
    }
}

/// Checks which library revisions are already installed, without calling the host or
/// asking for provider enforcement. This is the item-only part of Workspace's preview.
pub fn installed_items(
    store: &Store,
    provider: &str,
    chosen: &[super::store::Item],
    library: &[Summary],
    installed: &LaunchScan,
) -> Option<Vec<terminal::PlanItem>> {
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
                installed,
                project: None,
            })
            .items,
        );
    }
    Some(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn installed_scans_are_reused_by_provider_and_project_until_expiry() {
        let cache = InstalledScanCache::default();
        let start = Instant::now();
        let calls = Cell::new(0);
        let fetch = |provider, project, seconds| {
            cache
                .get(
                    provider,
                    Some(PathBuf::from(project)),
                    || start + Duration::from_secs(seconds),
                    |_, _| {
                        calls.set(calls.get() + 1);
                        Some(LaunchScan {
                            skills: vec![],
                            composites: vec![],
                            mcp_servers: Default::default(),
                            truncated: false,
                            unreadable: vec![],
                            skill_files: vec![],
                            nested_files: vec![],
                            base_off: Default::default(),
                        })
                    },
                )
                .unwrap()
        };
        let first = fetch("codex", "/fixture/a", 0);
        assert!(
            Arc::ptr_eq(&first, &fetch("codex", "/fixture/a", 29)),
            "a second launch in the same provider/project scanned again"
        );
        fetch("claude_code", "/fixture/a", 29);
        fetch("codex", "/fixture/b", 29);
        assert_eq!(calls.get(), 3);
        assert!(!Arc::ptr_eq(&first, &fetch("codex", "/fixture/a", 30)));
        assert_eq!(calls.get(), 4);
    }
}
