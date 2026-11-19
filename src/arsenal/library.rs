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

/// The Arsenal part of one launch of `preset` (a selection's name) for `provider` in `directory`:
/// the same plan Workspace shows, against the skills `installed` now, with the environment and the
/// project instructions the preset chose in `data` (the Arsenal folder). Refused, with the reason,
/// when the selection is not there or an item of it cannot load.
pub fn launch_for(
    store: &Store,
    data: &Path,
    preset: &str,
    provider: &str,
    directory: &Path,
    installed: &LaunchScan,
) -> Result<crate::native::arsenal::ArsenalLaunch, String> {
    let unread = |e: Error| format!("The Arsenal library could not be read ({e:?}).");
    let selection = store
        .selections()
        .map_err(unread)?
        .into_iter()
        .find(|selection| selection.name == preset && selection.provider == provider)
        .ok_or_else(|| format!("There is no Arsenal preset \"{preset}\" for this agent."))?;
    let (items, _) = resolve_items(store, &selection).map_err(unread)?;
    let key = directory.to_string_lossy().into_owned();
    let plan = terminal::plan(terminal::Input {
        selection: &selection,
        items,
        provider,
        version: None,
        evidence: None,
        installed,
        project: Some((key.clone(), key)),
    });
    if plan.status == "blocked" {
        let reasons: Vec<String> = plan
            .items
            .iter()
            .filter(|item| matches!(item.state, "rejected" | "missing"))
            .map(|item| format!("{}: {}", item.name, item.reason))
            .chain(
                plan.conflicts
                    .iter()
                    .filter(|conflict| conflict.blocking)
                    .map(|conflict| format!("{}: {}", conflict.code, conflict.names.join(", "))),
            )
            .collect();
        return Err(if reasons.is_empty() {
            "Too many installed skills would be switched off.".to_owned()
        } else {
            reasons.join(" ")
        });
    }
    let settings = |e: std::io::Error| format!("The preset's settings could not be read ({e}).");
    let (home, codex_home) = if provider == "codex" {
        (
            None,
            super::environment::codex::get(data, &selection.id).map_err(settings)?,
        )
    } else {
        (
            super::environment::get(data, &selection.id).map_err(settings)?,
            None,
        )
    };
    let launch_instructions =
        match super::role_instructions::get(data, &selection.id).map_err(settings)? {
            super::role_instructions::Choice::Load => None,
            choice => Some(choice),
        };
    let launch = crate::native::arsenal::ArsenalLaunch {
        selection: selection.id,
        selection_name: selection.name,
        config_hash: plan.config_hash,
        skills_off: plan.excluded,
        skill_files_off: plan.excluded_files,
        home,
        codex_home,
        launch_instructions,
        materialize: plan.materialize,
    };
    launch
        .validate()
        .map_err(|_| "The Arsenal preset is not well formed.".to_owned())?;
    Ok(launch)
}

/// [`launch_for`] with the library, its folder and the skills installed now.
pub fn launch_for_process(
    preset: &str,
    provider: &str,
    directory: &Path,
) -> Result<crate::native::arsenal::ArsenalLaunch, String> {
    let closed = || "The Arsenal library could not be opened.".to_owned();
    let store = super::open_read_only().map_err(|_| closed())?;
    let data = super::environment::directory().ok_or_else(closed)?;
    let project = catalogue::validate_project_process(directory);
    let installed = catalogue::launch_scan_process(project, provider)
        .ok_or("The installed skills could not be read.")?;
    launch_for(&store, &data, preset, provider, directory, &installed)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agentconfig::catalogue::Candidate;
    use crate::arsenal::store::{self, Item, NewCapability, SelectionInput};
    use std::cell::Cell;

    fn installed(provider: &'static str, name: &str, body: &[u8]) -> Candidate {
        Candidate {
            kind: "skill",
            provider,
            source: format!("{}_user_skills", provider.trim_end_matches("_code")),
            scope: "user",
            name: name.to_owned(),
            description: String::new(),
            origin: String::new(),
            files: Vec::new(),
            excluded: Vec::new(),
            review: Vec::new(),
            executes_code: false,
            blocked: None,
            managed_by: None,
            content: vec![("SKILL.md".to_owned(), body.to_vec())],
        }
    }

    /// A library holding `keep` (installed with the same content) and a preset "Front" keeping it,
    /// with `design` and `gsd-help` installed too, for `provider`.
    pub(crate) fn fixture(provider: &'static str, kept_body: &[u8]) -> (Store, LaunchScan) {
        let mut store = Store::open(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        let source = format!("{}_user_skills", provider.trim_end_matches("_code"));
        let imported = store
            .import(&NewCapability {
                identity: store::identity(provider, "skill", &source, "keep"),
                provider: provider.into(),
                source: source.clone(),
                kind: "skill".into(),
                name: "keep".into(),
                description: "test".into(),
                origin: "~/skills/keep".into(),
                files: vec![("SKILL.md".into(), b"keep body".to_vec())],
            })
            .unwrap();
        store
            .save_selection(&SelectionInput {
                id: None,
                name: "Front".into(),
                provider: provider.into(),
                items: vec![Item {
                    capability: imported.capability,
                    revision: None,
                }],
                composites: vec![],
            })
            .unwrap();
        let skills = vec![
            installed(provider, "keep", kept_body),
            installed(provider, "design", b"d"),
            installed(provider, "gsd-help", b"g"),
        ];
        let files = skills
            .iter()
            .map(|s| Some(PathBuf::from(format!("/h/.codex/skills/{}/SKILL.md", s.name))))
            .collect();
        let nested = skills.iter().map(|_| Vec::new()).collect();
        let scan = LaunchScan {
            skills,
            composites: vec![],
            mcp_servers: Default::default(),
            truncated: false,
            unreadable: vec![],
            skill_files: files,
            nested_files: nested,
            base_off: Default::default(),
        };
        (store, scan)
    }

    fn data_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arsenal-launch-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_claude_preset_switches_off_every_other_installed_skill_by_name() {
        let (store, scan) = fixture("claude_code", b"keep body");
        let data = data_dir();
        let launch = launch_for(&store, &data, "Front", "claude_code", Path::new("/w"), &scan).unwrap();
        assert_eq!(launch.selection_name, "Front");
        assert_eq!(launch.skills_off, vec!["design", "gsd-help"]);
        assert!(launch.skill_files_off.is_empty(), "Claude switches skills off by name only");
        assert_eq!((launch.home.clone(), launch.codex_home.clone(), launch.launch_instructions.clone()), (None, None, None));
        assert!(launch.validate().is_ok());
        // The hash is the plan's: the same inputs give the same one.
        let again = launch_for(&store, &data, "Front", "claude_code", Path::new("/w"), &scan).unwrap();
        assert_eq!(again.config_hash, launch.config_hash);
    }

    #[test]
    fn a_codex_preset_names_the_skill_file_of_each_skill_switched_off_and_its_choices() {
        let (store, scan) = fixture("codex", b"keep body");
        let data = data_dir();
        let id = store.selections().unwrap()[0].id.clone();
        let choice = crate::arsenal::role_instructions::Choice::Exclude { role: String::new() };
        crate::arsenal::role_instructions::set(&data, &id, &choice).unwrap();
        let launch = launch_for(&store, &data, "Front", "codex", Path::new("/w"), &scan).unwrap();
        assert_eq!(launch.skills_off, vec!["design", "gsd-help"]);
        assert_eq!(
            launch.skill_files_off,
            vec!["/h/.codex/skills/design/SKILL.md", "/h/.codex/skills/gsd-help/SKILL.md"]
        );
        assert_eq!(launch.launch_instructions, Some(choice));
        assert!(launch.home.is_none(), "a Codex launch never carries Claude Code's home");
        assert!(launch.validate().is_ok());
    }

    #[test]
    fn a_missing_preset_or_another_providers_is_refused_with_the_reason() {
        let (store, scan) = fixture("claude_code", b"keep body");
        let data = data_dir();
        let error = launch_for(&store, &data, "Nope", "claude_code", Path::new("/w"), &scan).unwrap_err();
        assert!(error.contains("no Arsenal preset \"Nope\""), "{error}");
        let error = launch_for(&store, &data, "Front", "codex", Path::new("/w"), &scan).unwrap_err();
        assert!(error.contains("no Arsenal preset"), "another provider's preset is not this one: {error}");
    }

    #[test]
    fn a_preset_whose_installed_copy_differs_launches_with_the_stored_revision_copied() {
        let (store, scan) = fixture("claude_code", b"edited since");
        let data = data_dir();
        let launch = launch_for(&store, &data, "Front", "claude_code", Path::new("/w"), &scan).unwrap();
        assert_eq!(launch.materialize.len(), 1);
        assert_eq!((launch.materialize[0].name.as_str(), launch.materialize[0].revision), ("keep", 1));
        // Its name stays on: the copy in the session's folder is the one that loads.
        assert_eq!(launch.skills_off, vec!["design", "gsd-help"]);
        assert!(launch.validate().is_ok());
    }

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
