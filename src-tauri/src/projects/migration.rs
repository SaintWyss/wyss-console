//! The deterministic import of the older registry and saved layout (contract §4).
//!
//! The preview reads the two older files and the host without changing, locking, opening or
//! starting any of them, and builds the whole new store in memory. Identities come from the
//! migration's own namespace and the older keys, so the same inputs give the same store. A
//! root that is missing is not invented; a session that is not alive is not remade; a tab that
//! mixes projects is split by the origin each terminal can be proven to have; what cannot be
//! proven stays in Scratch or in an empty placement. Nothing is truncated: a plan over a bound
//! is refused whole.
//!
//! Commit preserves the older bytes in a private snapshot with a hash manifest first, and
//! publishes the new store last, atomically, so a crash leaves the old state or the complete
//! new one. The older files are never written.
use super::{
    apply::{Env, store_failure},
    command::{ErrorCode, Failure},
    owner::{HostView, Owner, RootError, RootInfo},
    store::{Store, StoreError},
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use wyss_console::contracts::{
    UtcTimestamp,
    b1::Sha256Hex,
    m1::{
        MAX_BINDINGS_PER_PROJECT, MAX_LAYOUT_DEPTH, MAX_LEAVES, MAX_PROJECTS,
        MAX_WORKSPACES_PER_PROJECT,
        records::{
            Association, AssociationState, Binding, BindingState, Leaf, MapSourceKind, Migration,
            MigrationMapRow, MigrationSource, MigrationSourceKind, MigrationState,
            MigrationVersion, Node, Observation, Preferences, ProfileDefaults, Project, SplitDir,
            StoreV1, Target, Workspace,
        },
        scalar::{
            AbsolutePath, AssociationId, BindingId, Counter, EpochText, HostId, LeafId, MappedId,
            MigrationId, NativeId, ProjectId, StoreId, TerminalId, Text, WorkspaceId,
        },
    },
};

/// The older registry's bounds (schema 1): size, sessions and known folders.
const REGISTRY_MAX_BYTES: u64 = 2 * 1024 * 1024;
const REGISTRY_MAX_SESSIONS: usize = 200;
const REGISTRY_MAX_PROJECTS: usize = 100;
/// The saved layout's bounds (version 1): size, tabs, panes per tab.
const LAYOUT_MAX_BYTES: u64 = 64 * 1024;
const LAYOUT_MAX_TABS: usize = 16;
/// How many times an unstable source is read again before the preview gives up.
const READ_TRIES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// A older file is there but cannot be read as the schema it claims. Blocks.
    SourceUnreadable,
    /// A older file changed while it was being read. Blocks.
    SourceUnstable,
    /// A older file is bigger than its bound. Blocks.
    SourceOversized,
    /// A registered path is a spelling of another registered root. Blocks until accepted.
    Alias,
    /// A registered folder is missing or not eligible: not imported, never invented.
    RootUnavailable,
    /// A registered folder can be used by another account (an ACL or the mode bits), or that
    /// could not be checked: not imported, unless the owner's trust file names the account.
    RootShared,
    /// The host could not be read: no live session is imported, no shell is remade.
    HostOffline,
    /// A live terminal or tab cannot be placed within the bounds. Blocks, nothing truncated.
    OverBound,
    /// An identity the older owner gave is not one the contract can hold. Not coerced.
    IncompatibleId,
    /// The assembled store failed the contract. Blocks.
    Invalid,
}

impl ConflictKind {
    pub fn blocks_commit(self) -> bool {
        self.blocks()
    }

    pub fn blocks(self) -> bool {
        matches!(
            self,
            Self::SourceUnreadable
                | Self::SourceUnstable
                | Self::SourceOversized
                | Self::OverBound
                | Self::Invalid
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub projects: usize,
    pub workspaces: usize,
    pub sessions: usize,
    pub placed_leaves: usize,
    pub empty_leaves: usize,
    pub tabs_skipped: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// There is no store yet: this builds it.
    Initial,
    /// There is a store; this adds only what appeared in the older state since.
    Incremental,
}

/// What a window may see of a preview: counts and conflicts, never a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub preview_id: String,
    pub mode: Mode,
    pub counts: Counts,
    pub conflicts: Vec<Conflict>,
    pub sources: Vec<SourceDigest>,
    /// Whether committing would be accepted now (aliases need an explicit acceptance).
    pub can_commit: bool,
    pub needs_alias_acceptance: bool,
    /// The older state changed since the last committed import.
    pub older_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceDigest {
    pub kind: &'static str,
    pub digest: String,
}

fn source_kind(kind: MigrationSourceKind) -> &'static str {
    match kind {
        MigrationSourceKind::Registry => "registry",
        MigrationSourceKind::Layout => "layout",
    }
}

/// The bytes read from the older files and the digests of exactly those bytes.
#[derive(Clone, Debug)]
pub struct Sources {
    pub registry: Option<Vec<u8>>,
    pub layout: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SourceError {
    Unreadable,
    Oversized,
    Unstable,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn read_one(path: &Path, most: u64) -> Result<Option<Vec<u8>>, SourceError> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(SourceError::Unreadable),
    };
    let meta = file.metadata().map_err(|_| SourceError::Unreadable)?;
    if !meta.is_file() || meta.uid() != nix::unistd::geteuid().as_raw() {
        return Err(SourceError::Unreadable);
    }
    if meta.len() > most {
        return Err(SourceError::Oversized);
    }
    let mut bytes = Vec::new();
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SourceError::Unreadable)?;
    Ok(Some(bytes))
}

/// Reads a source twice and returns the bytes only if both reads agree: a file that is being
/// written is read again, and a file that never settles is `Unstable`, never half-read.
fn read_consistent(path: &Path, most: u64) -> Result<Option<Vec<u8>>, SourceError> {
    for _ in 0..READ_TRIES {
        let first = read_one(path, most)?;
        let second = read_one(path, most)?;
        if first == second {
            return Ok(first);
        }
    }
    Err(SourceError::Unstable)
}

/// The older files, in the Console's own state directory. Reading them changes nothing:
/// the registry is read as bytes, never opened through the Native service, whose open writes.
pub fn read_sources(state_dir: &Path) -> Result<Sources, (ConflictKind, &'static str)> {
    let conflict = |e: SourceError, which: &'static str| {
        (
            match e {
                SourceError::Oversized => ConflictKind::SourceOversized,
                SourceError::Unstable => ConflictKind::SourceUnstable,
                SourceError::Unreadable => ConflictKind::SourceUnreadable,
            },
            which,
        )
    };
    Ok(Sources {
        registry: read_consistent(
            &state_dir.join("native").join("registry.json"),
            REGISTRY_MAX_BYTES,
        )
        .map_err(|e| conflict(e, "registry"))?,
        layout: read_consistent(&state_dir.join("layout.json"), LAYOUT_MAX_BYTES)
            .map_err(|e| conflict(e, "layout"))?,
    })
}

// ---- parsing the older documents, strictly --------------------------------------------------

struct RegistryDoc {
    projects: Vec<String>,
    /// Session ID to its project folder.
    sessions: Vec<(String, String)>,
}

fn only_keys(value: &Value, allowed: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.keys().all(|k| allowed.contains(&k.as_str())))
}

fn parse_registry(bytes: &[u8]) -> Option<RegistryDoc> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    if !only_keys(&value, &["schema", "sessions", "projects"]) || value["schema"] != 1 {
        return None;
    }
    let projects = value["projects"].as_array()?;
    let sessions = value["sessions"].as_array()?;
    if projects.len() > REGISTRY_MAX_PROJECTS || sessions.len() > REGISTRY_MAX_SESSIONS {
        return None;
    }
    Some(RegistryDoc {
        projects: projects
            .iter()
            .map(|p| p["path"].as_str().map(str::to_owned))
            .collect::<Option<_>>()?,
        sessions: sessions
            .iter()
            .map(|s| {
                Some((
                    s["id"].as_str()?.to_owned(),
                    s["project"].as_str()?.to_owned(),
                ))
            })
            .collect::<Option<_>>()?,
    })
}

#[derive(Clone)]
enum Tree {
    Leaf(String),
    Split {
        dir: SplitDir,
        ratio: f64,
        a: Box<Tree>,
        b: Box<Tree>,
    },
}

struct Tab {
    root: Tree,
    focus: Option<String>,
}

struct LayoutDoc {
    tabs: Vec<Tab>,
    titles: HashMap<String, String>,
}

fn parse_tree(value: &Value, level: usize, seen: &mut HashSet<String>) -> Option<Tree> {
    if level > MAX_LAYOUT_DEPTH {
        return None;
    }
    match value["kind"].as_str()? {
        "leaf" => {
            let id = value["id"].as_str().filter(|id| id.len() <= 64)?.to_owned();
            seen.insert(id.clone()).then_some(Tree::Leaf(id))
        }
        "split" => {
            let ratio = value["ratio"].as_f64().filter(|r| r.is_finite())?;
            Some(Tree::Split {
                dir: match value["dir"].as_str()? {
                    "row" => SplitDir::Row,
                    "col" => SplitDir::Col,
                    _ => return None,
                },
                // The window clamps a ratio when it reads one; so does the import.
                ratio: ratio.clamp(0.1, 0.9),
                a: Box::new(parse_tree(&value["a"], level + 1, seen)?),
                b: Box::new(parse_tree(&value["b"], level + 1, seen)?),
            })
        }
        _ => None,
    }
}

fn parse_layout(bytes: &[u8]) -> Option<LayoutDoc> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    if !only_keys(&value, &["version", "tabs", "active", "terminals"]) || value["version"] != 1 {
        return None;
    }
    let tabs = value["tabs"].as_array()?;
    if tabs.len() > LAYOUT_MAX_TABS {
        return None;
    }
    let mut seen_all = HashSet::new();
    let mut parsed = Vec::new();
    for tab in tabs {
        let mut seen = HashSet::new();
        let root = parse_tree(&tab["root"], 0, &mut seen)?;
        if seen.len() > MAX_LEAVES || seen.iter().any(|id| !seen_all.insert(id.clone())) {
            return None;
        }
        parsed.push(Tab {
            root,
            focus: tab["focus"].as_str().map(str::to_owned),
        });
    }
    let mut titles = HashMap::new();
    if let Some(known) = value["terminals"].as_object() {
        for (id, t) in known {
            if let Some(title) = t["title"].as_str() {
                titles.insert(id.clone(), title.to_owned());
            }
        }
    }
    Some(LayoutDoc {
        tabs: parsed,
        titles,
    })
}

fn leaves_of(tree: &Tree, out: &mut Vec<String>) {
    match tree {
        Tree::Leaf(id) => out.push(id.clone()),
        Tree::Split { a, b, .. } => {
            leaves_of(a, out);
            leaves_of(b, out);
        }
    }
}

/// The tree without the leaves `keep` refuses; a sibling takes the space of a removed one.
fn prune(tree: &Tree, keep: &dyn Fn(&str) -> bool) -> Option<Tree> {
    match tree {
        Tree::Leaf(id) => keep(id).then(|| tree.clone()),
        Tree::Split { dir, ratio, a, b } => match (prune(a, keep), prune(b, keep)) {
            (Some(a), Some(b)) => Some(Tree::Split {
                dir: *dir,
                ratio: *ratio,
                a: Box::new(a),
                b: Box::new(b),
            }),
            (one, other) => one.or(other),
        },
    }
}

// ---- identities ----------------------------------------------------------------------------

/// A name-based UUID inside the migration's namespace (SHA-256, version 8 layout): the same
/// namespace, kind and key always give the same identity.
pub fn derive(namespace: &str, kind: &str, key: &str) -> String {
    let digest = Sha256::digest(format!("{namespace}\0{kind}\0{key}"));
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h = hex(&bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

fn key_of(kind: &str, older: &str) -> String {
    sha256(format!("{kind}\0{older}").as_bytes())
}

// ---- the plan ------------------------------------------------------------------------------

pub struct Plan {
    pub store: StoreV1,
    pub summary: Summary,
    /// Exactly the bytes that were read, to be preserved on commit.
    pub snapshot: Vec<(&'static str, Vec<u8>)>,
    pub epoch: Option<String>,
}

struct Builder {
    namespace: String,
    store: StoreV1,
    conflicts: BTreeMap<u8, (ConflictKind, usize)>,
    counts: Counts,
    map: Vec<MigrationMapRow>,
}

impl Builder {
    fn conflict(&mut self, kind: ConflictKind) {
        self.conflicts.entry(kind as u8).or_insert((kind, 0)).1 += 1;
    }

    fn id<T>(
        &self,
        kind: &str,
        key: &str,
        parse: impl Fn(&str) -> Result<T, wyss_console::contracts::m1::M1Error>,
    ) -> Option<T> {
        parse(&derive(&self.namespace, kind, key)).ok()
    }
}

fn text(value: &str, fallback: &str) -> Text {
    let cleaned: String = value
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(64)
        .collect();
    Text::parse(&cleaned).unwrap_or_else(|_| Text::parse(fallback).expect("static text"))
}

fn counter1() -> Counter {
    Counter::new(1).expect("one")
}

/// The terminals of one group of a tab, each with the association that shows there.
type Placed = Vec<(String, Option<AssociationId>)>;

/// The pure assembly of the new store from the older documents and a read of the host.
#[allow(clippy::too_many_arguments)]
pub fn plan_initial(
    env: &dyn Env,
    owner: &dyn Owner,
    sources: &Sources,
    host: Option<&HostView>,
    migration_id: &str,
) -> Result<Plan, Failure> {
    let bad = || Failure::new(ErrorCode::InvalidRecord).with_field("migration");
    let host_id = HostId::parse(&env.uuid()).map_err(|_| bad())?;
    let mut b = Builder {
        namespace: migration_id.to_owned(),
        store: StoreV1 {
            schema: wyss_console::contracts::m1::records::SchemaV1,
            store_id: StoreId::parse(&env.uuid()).map_err(|_| bad())?,
            revision: counter1(),
            host_id: host_id.clone(),
            projects: Vec::new(),
            bindings: Vec::new(),
            workspaces: Vec::new(),
            sessions: Vec::new(),
            operations: Vec::new(),
            closed_operations: Vec::new(),
            migration: None,
        },
        conflicts: BTreeMap::new(),
        counts: Counts::default(),
        map: Vec::new(),
    };
    let now = UtcTimestamp::try_from(env.now()).map_err(|_| bad())?;

    let registry = match &sources.registry {
        Some(bytes) => match parse_registry(bytes) {
            Some(doc) => Some(doc),
            None => {
                b.conflict(ConflictKind::SourceUnreadable);
                None
            }
        },
        None => None,
    };
    let layout = match &sources.layout {
        Some(bytes) => match parse_layout(bytes) {
            Some(doc) => Some(doc),
            None => {
                b.conflict(ConflictKind::SourceUnreadable);
                None
            }
        },
        None => None,
    };
    if host.is_none() {
        b.conflict(ConflictKind::HostOffline);
    }

    // Projects: one per canonical root, from the folders the registry knows. Exact duplicate
    // roots coalesce; a registered path that is another spelling of a root is an alias.
    let mut roots: BTreeMap<String, (RootInfo, Vec<String>)> = BTreeMap::new();
    if let Some(doc) = &registry {
        let mut paths = doc.projects.clone();
        paths.sort();
        paths.dedup();
        for path in paths {
            match owner.root(&path) {
                Ok(root) => {
                    if root.path != path {
                        b.conflict(ConflictKind::Alias);
                    }
                    roots
                        .entry(root.path.clone())
                        .or_insert_with(|| (root, Vec::new()))
                        .1
                        .push(path);
                }
                Err(RootError::Missing | RootError::Ineligible) => {
                    b.conflict(ConflictKind::RootUnavailable);
                }
                Err(RootError::Shared) => b.conflict(ConflictKind::RootShared),
            }
        }
    }
    // The canonical root of each folder, to find a session's project by exact root.
    let mut project_of_root: HashMap<String, ProjectId> = HashMap::new();
    let mut binding_of_project: HashMap<String, (BindingId, Counter)> = HashMap::new();
    for (canonical, (root, older)) in &roots {
        let ids = (
            b.id("project", canonical, ProjectId::parse),
            b.id("binding", canonical, BindingId::parse),
            b.id("default_workspace", canonical, WorkspaceId::parse),
            AbsolutePath::parse(&root.path),
            Counter::new(root.device),
            Counter::new(root.inode),
        );
        let (Some(project), Some(binding), Some(workspace), Ok(path), Ok(device), Ok(inode)) = ids
        else {
            b.conflict(ConflictKind::IncompatibleId);
            continue;
        };
        for older_path in older {
            b.map.push(MigrationMapRow {
                source_kind: MapSourceKind::RegistryProject,
                source_key: text(&key_of("registry_project", older_path), "key"),
                target_id: MappedId::parse(project.as_str()).expect("a project ID is a UUID"),
            });
        }
        b.store.bindings.push(Binding {
            id: binding.clone(),
            project_id: project.clone(),
            host_id: host_id.clone(),
            revision: counter1(),
            path,
            observation: Observation { device, inode },
            checkout_kind: root.kind,
            state: BindingState::Valid,
            previous_binding_id: None,
        });
        b.store.projects.push(Project {
            id: project.clone(),
            label: text(
                canonical
                    .rsplit('/')
                    .find(|s| !s.is_empty())
                    .unwrap_or("Project"),
                "Project",
            ),
            created_at: now.clone(),
            updated_at: now.clone(),
            revision: counter1(),
            archived: false,
            tombstone: false,
            default_binding_id: Some(binding.clone()),
            default_workspace_id: Some(workspace),
            profile_defaults: ProfileDefaults {
                claude: None,
                codex: None,
            },
        });
        project_of_root.insert(canonical.clone(), project.clone());
        binding_of_project.insert(project.as_str().to_owned(), (binding, counter1()));
    }

    // Live sessions and shells, from the host, each with an origin only when it can be proven.
    let mut origin_of_terminal: HashMap<String, Option<ProjectId>> = HashMap::new();
    let mut session_project: HashMap<&str, &str> = HashMap::new();
    if let Some(doc) = &registry {
        for (id, project) in &doc.sessions {
            session_project.insert(id, project);
        }
    }
    let canonical_of = |path: &str| owner.root(path).ok().map(|r| r.path);
    if let Some(view) = host {
        let epoch = EpochText::parse(&view.epoch).map_err(|_| bad())?;
        for row in view.terminals.iter().filter(|t| t.running) {
            let Ok(terminal) = TerminalId::parse(&row.terminal_id) else {
                b.conflict(ConflictKind::IncompatibleId);
                continue;
            };
            let (origin, target) = match &row.native_session {
                Some(session) => {
                    let Ok(native) = NativeId::parse(session) else {
                        b.conflict(ConflictKind::IncompatibleId);
                        continue;
                    };
                    let origin = session_project
                        .get(session.as_str())
                        .and_then(|p| canonical_of(p))
                        .and_then(|root| project_of_root.get(&root).cloned());
                    if registry.is_some()
                        && session_project.contains_key(session.as_str())
                        && let Some(id) = b.id("session", session, AssociationId::parse)
                    {
                        // The association's identity is derived from the older session.
                        b.map.push(MigrationMapRow {
                            source_kind: MapSourceKind::RegistrySession,
                            source_key: text(&key_of("registry_session", session), "key"),
                            target_id: MappedId::parse(id.as_str()).expect("UUID"),
                        });
                    }
                    (
                        origin,
                        Target::Native {
                            session_id: native,
                            terminal_id: terminal.clone(),
                            host_epoch: epoch.clone(),
                        },
                    )
                }
                None => {
                    // A plain shell belongs to a project only if it started in its exact root.
                    let origin = row
                        .cwd
                        .as_deref()
                        .and_then(canonical_of)
                        .and_then(|root| project_of_root.get(&root).cloned());
                    (
                        origin,
                        Target::Shell {
                            terminal_id: terminal.clone(),
                            host_epoch: epoch.clone(),
                        },
                    )
                }
            };
            origin_of_terminal.insert(row.terminal_id.clone(), origin.clone());
            let older_key = match &row.native_session {
                Some(session) => session.clone(),
                None => row.terminal_id.clone(),
            };
            let kind = if row.native_session.is_some() {
                "session"
            } else {
                "shell"
            };
            let Some(id) = b.id(kind, &older_key, AssociationId::parse) else {
                b.conflict(ConflictKind::IncompatibleId);
                continue;
            };
            let binding = origin
                .as_ref()
                .and_then(|p| binding_of_project.get(p.as_str()).cloned());
            b.store.sessions.push(Association {
                id,
                origin_project_id: origin,
                host_id: host_id.clone(),
                binding_id: binding.as_ref().map(|(id, _)| id.clone()),
                binding_revision: binding.map(|(_, r)| r),
                target,
                state: AssociationState::Detached,
                operation_id: None,
            });
        }
    }
    b.counts.sessions = b.store.sessions.len();

    // Tabs become workspaces: one per tab and origin, in tab order.
    let mut scratch_leaves: Vec<(Tree, Placed)> = Vec::new();
    let mut default_of: HashMap<String, WorkspaceId> = HashMap::new();
    let mut per_project_count: HashMap<String, usize> = HashMap::new();
    if let Some(doc) = &layout {
        for (index, tab) in doc.tabs.iter().enumerate() {
            let mut all = Vec::new();
            leaves_of(&tab.root, &mut all);
            // The origin of each live terminal; an absent one is placed with the tab's majority.
            let live_origin = |id: &str| origin_of_terminal.get(id).cloned();
            let mut tally: BTreeMap<Option<String>, usize> = BTreeMap::new();
            for id in &all {
                if let Some(origin) = live_origin(id) {
                    *tally
                        .entry(origin.map(|p| p.as_str().to_owned()))
                        .or_default() += 1;
                }
            }
            let Some(majority) = tally
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .map(|(k, _)| k.clone())
            else {
                // Nothing in this tab is alive: there is nothing to place and nothing to remake.
                b.counts.tabs_skipped += 1;
                continue;
            };
            let origin_of = |id: &str| -> Option<String> {
                match live_origin(id) {
                    Some(origin) => origin.map(|p| p.as_str().to_owned()),
                    None => majority.clone(),
                }
            };
            let mut groups: BTreeMap<Option<String>, ()> = BTreeMap::new();
            for id in &all {
                groups.insert(origin_of(id), ());
            }
            for group in groups.keys() {
                let Some(tree) = prune(&tab.root, &|id| origin_of(id) == *group) else {
                    continue;
                };
                let mut ids = Vec::new();
                leaves_of(&tree, &mut ids);
                let mut leaves = Vec::new();
                for terminal in &ids {
                    let Some(leaf_id) = b.id("layout_leaf", terminal, LeafId::parse) else {
                        b.conflict(ConflictKind::IncompatibleId);
                        continue;
                    };
                    let association = b
                        .store
                        .sessions
                        .iter()
                        .find(|a| match &a.target {
                            Target::Native { terminal_id, .. }
                            | Target::Shell { terminal_id, .. } => terminal_id.as_str() == terminal,
                            Target::Managed { .. } => false,
                        })
                        .map(|a| a.id.clone());
                    b.map.push(MigrationMapRow {
                        source_kind: MapSourceKind::LayoutLeaf,
                        source_key: text(&key_of("layout_leaf", terminal), "key"),
                        target_id: MappedId::parse(leaf_id.as_str()).expect("UUID"),
                    });
                    leaves.push((terminal.clone(), association));
                }
                match group {
                    None => scratch_leaves.push((tree, leaves)),
                    Some(project) => {
                        let Some(workspace) = b.id(
                            "tab_workspace",
                            &format!("{index}\0{project}"),
                            WorkspaceId::parse,
                        ) else {
                            b.conflict(ConflictKind::IncompatibleId);
                            continue;
                        };
                        *per_project_count.entry(project.clone()).or_default() += 1;
                        default_of
                            .entry(project.clone())
                            .or_insert_with(|| workspace.clone());
                        let title = format!("Tab {}", index + 1);
                        push_workspace(
                            &mut b,
                            workspace,
                            Some(ProjectId::parse(project).expect("a project ID")),
                            &title,
                            &tree,
                            leaves,
                            tab.focus.as_deref(),
                            &doc.titles,
                        );
                    }
                }
            }
        }
    }

    // The one Scratch workspace holds every unassigned group, in one balanced tree.
    let scratch_id = b
        .id("scratch", "scratch", WorkspaceId::parse)
        .ok_or_else(bad)?;
    let mut scratch = Workspace {
        id: scratch_id,
        project_id: None,
        host_id: host_id.clone(),
        revision: counter1(),
        label: text("Scratch", "Scratch"),
        layout: None,
        focus: None,
        pinned: false,
        leaves: Vec::new(),
        preferences: native_preferences(),
    };
    let mut scratch_all: Vec<(String, Option<AssociationId>)> = Vec::new();
    for (_, leaves) in &scratch_leaves {
        scratch_all.extend(leaves.iter().cloned());
    }
    if scratch_all.len() > MAX_LEAVES {
        b.conflict(ConflictKind::OverBound);
    } else if !scratch_all.is_empty() {
        let mut nodes = Vec::new();
        for (terminal, association) in &scratch_all {
            let Some(id) = b.id("layout_leaf", terminal, LeafId::parse) else {
                continue;
            };
            scratch.leaves.push(Leaf {
                id: id.clone(),
                title: text(
                    layout
                        .as_ref()
                        .and_then(|d| d.titles.get(terminal))
                        .map_or("Terminal", |t| t.as_str()),
                    "Terminal",
                ),
                association_id: association.clone(),
            });
            nodes.push(Node::Leaf { id });
        }
        scratch.layout = balanced(nodes);
    }
    for leaf in &scratch.leaves {
        if leaf.association_id.is_some() {
            b.counts.placed_leaves += 1;
        } else {
            b.counts.empty_leaves += 1;
        }
    }
    b.store.workspaces.push(scratch);

    // Every project has a default workspace, an empty one when no tab was imported for it.
    let projects: Vec<(ProjectId, Option<WorkspaceId>)> = b
        .store
        .projects
        .iter()
        .map(|p| (p.id.clone(), p.default_workspace_id.clone()))
        .collect();
    for (project, planned) in projects {
        let imported = default_of.get(project.as_str()).cloned();
        let default = match (imported, planned) {
            (Some(imported), _) => imported,
            (None, Some(planned)) => {
                b.store.workspaces.push(Workspace {
                    id: planned.clone(),
                    project_id: Some(project.clone()),
                    host_id: host_id.clone(),
                    revision: counter1(),
                    label: text("Main", "Main"),
                    layout: None,
                    focus: None,
                    pinned: false,
                    leaves: Vec::new(),
                    preferences: native_preferences(),
                });
                planned
            }
            (None, None) => continue,
        };
        if let Some(found) = b.store.projects.iter_mut().find(|p| p.id == project) {
            found.default_workspace_id = Some(default);
        }
    }

    // Sessions that a leaf shows are attached; the rest stay discoverable.
    let shown: HashSet<AssociationId> = b
        .store
        .workspaces
        .iter()
        .flat_map(|w| w.leaves.iter().filter_map(|l| l.association_id.clone()))
        .collect();
    for association in &mut b.store.sessions {
        if shown.contains(&association.id) {
            association.state = AssociationState::Attached;
        }
    }
    b.counts.projects = b.store.projects.len();
    b.counts.workspaces = b.store.workspaces.len();

    // Bounds are refused whole.
    if b.store.projects.len() > MAX_PROJECTS
        || per_project_count
            .values()
            .any(|n| *n + 1 > MAX_WORKSPACES_PER_PROJECT)
        || b.store.bindings.len() > MAX_PROJECTS * MAX_BINDINGS_PER_PROJECT
    {
        b.conflict(ConflictKind::OverBound);
    }

    // The migration record: the exact bytes' digests and the deterministic map.
    let mut digests = Vec::new();
    let mut snapshot: Vec<(&'static str, Vec<u8>)> = Vec::new();
    if let Some(bytes) = &sources.registry {
        digests.push((MigrationSourceKind::Registry, sha256(bytes)));
        snapshot.push(("registry.json", bytes.clone()));
    }
    if let Some(bytes) = &sources.layout {
        digests.push((MigrationSourceKind::Layout, sha256(bytes)));
        snapshot.push(("layout.json", bytes.clone()));
    }
    if !digests.is_empty() {
        let mut sources_field = Vec::new();
        for (kind, digest) in &digests {
            sources_field.push(MigrationSource {
                kind: *kind,
                digest: Sha256Hex::try_from(digest.clone()).map_err(|_| bad())?,
            });
        }
        let mut map = std::mem::take(&mut b.map);
        map.sort_by(|a, c| {
            (a.source_kind as u8, a.source_key.as_str())
                .cmp(&(c.source_kind as u8, c.source_key.as_str()))
        });
        map.dedup_by(|a, c| a.source_kind == c.source_kind && a.source_key == c.source_key);
        b.store.migration = Some(Migration {
            version: MigrationVersion,
            id: MigrationId::parse(migration_id).map_err(|_| bad())?,
            sources: sources_field,
            map,
            state: MigrationState::Committed,
        });
    }

    // The whole store must satisfy the contract before it is offered.
    if let Err(error) = wyss_console::contracts::m1::encode(&b.store) {
        b.conflict(match error.code {
            wyss_console::contracts::m1::M1ErrorCode::CapacityExceeded => ConflictKind::OverBound,
            _ => ConflictKind::Invalid,
        });
    }
    let aliases = b
        .conflicts
        .get(&(ConflictKind::Alias as u8))
        .is_some_and(|c| c.1 > 0);
    let blocked = b
        .conflicts
        .values()
        .any(|(kind, n)| *n > 0 && kind.blocks());
    let conflicts: Vec<Conflict> = b
        .conflicts
        .values()
        .map(|(kind, count)| Conflict {
            kind: *kind,
            count: *count,
        })
        .collect();
    let sources_view = digests
        .iter()
        .map(|(kind, digest)| SourceDigest {
            kind: source_kind(*kind),
            digest: digest.clone(),
        })
        .collect();
    Ok(Plan {
        summary: Summary {
            preview_id: migration_id.to_owned(),
            mode: Mode::Initial,
            counts: b.counts,
            conflicts,
            sources: sources_view,
            can_commit: !blocked && !aliases,
            needs_alias_acceptance: aliases,
            older_changed: false,
        },
        store: b.store,
        snapshot,
        epoch: host.map(|h| h.epoch.clone()),
    })
}

fn native_preferences() -> Preferences {
    Preferences {
        provider: None,
        mode: wyss_console::contracts::m1::records::Mode::Native,
        native_profile: None,
        managed_safety_profile: None,
    }
}

/// A balanced tree over `nodes`, alternating direction; none for no nodes.
fn balanced(mut nodes: Vec<Node>) -> Option<Node> {
    fn build(nodes: &mut Vec<Node>, row: bool) -> Node {
        if nodes.len() == 1 {
            return nodes.remove(0);
        }
        let mut right = nodes.split_off(nodes.len() / 2);
        let left = build(nodes, !row);
        let right = build(&mut right, !row);
        Node::Split {
            dir: if row { SplitDir::Row } else { SplitDir::Col },
            ratio: 0.5,
            a: Box::new(left),
            b: Box::new(right),
        }
    }
    (!nodes.is_empty()).then(|| build(&mut nodes, true))
}

fn to_node(tree: &Tree, ids: &HashMap<String, LeafId>) -> Option<Node> {
    Some(match tree {
        Tree::Leaf(id) => Node::Leaf {
            id: ids.get(id)?.clone(),
        },
        Tree::Split { dir, ratio, a, b } => Node::Split {
            dir: *dir,
            ratio: *ratio,
            a: Box::new(to_node(a, ids)?),
            b: Box::new(to_node(b, ids)?),
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn push_workspace(
    b: &mut Builder,
    id: WorkspaceId,
    project: Option<ProjectId>,
    title: &str,
    tree: &Tree,
    leaves: Vec<(String, Option<AssociationId>)>,
    focus: Option<&str>,
    titles: &HashMap<String, String>,
) {
    let mut ids = HashMap::new();
    let mut records = Vec::new();
    for (terminal, association) in &leaves {
        let Some(leaf) = b.id("layout_leaf", terminal, LeafId::parse) else {
            continue;
        };
        ids.insert(terminal.clone(), leaf.clone());
        if association.is_some() {
            b.counts.placed_leaves += 1;
        } else {
            b.counts.empty_leaves += 1;
        }
        records.push(Leaf {
            id: leaf,
            title: text(
                titles.get(terminal).map_or("Terminal", |t| t.as_str()),
                "Terminal",
            ),
            association_id: association.clone(),
        });
    }
    let layout = to_node(tree, &ids);
    let host = b.store.host_id.clone();
    b.store.workspaces.push(Workspace {
        id,
        project_id: project,
        host_id: host,
        revision: counter1(),
        label: text(title, "Tab"),
        focus: focus.and_then(|f| ids.get(f).cloned()),
        layout,
        pinned: false,
        leaves: records,
        preferences: native_preferences(),
    });
}

// ---- reading, previewing, committing ---------------------------------------------------------

#[derive(Serialize)]
struct Manifest<'a> {
    migration: &'a str,
    files: Vec<ManifestFile>,
}

#[derive(Serialize)]
struct ManifestFile {
    name: &'static str,
    sha256: String,
    bytes: usize,
}

/// Preserves the exact older bytes beside the store, privately, with a manifest of their
/// hashes. It contains user paths: it is never logged, committed or attached to a report.
pub fn write_snapshot(
    store_dir: &Path,
    migration_id: &str,
    files: &[(&'static str, Vec<u8>)],
) -> io::Result<()> {
    let final_dir = store_dir.join(format!("migration-{migration_id}"));
    // The same preview committed again finds its snapshot already there, byte for byte.
    if final_dir.is_dir() {
        let same = files
            .iter()
            .all(|(name, bytes)| fs::read(final_dir.join(name)).is_ok_and(|found| &found == bytes));
        return if same {
            Ok(())
        } else {
            Err(io::Error::other("a different snapshot is already there"))
        };
    }
    let temp = store_dir.join(format!(".migration-tmp-{}", uuid::Uuid::new_v4().simple()));
    crate::native::store::private_dir(&temp)?;
    let result = (|| {
        let mut manifest = Manifest {
            migration: migration_id,
            files: Vec::new(),
        };
        for (name, bytes) in files {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(temp.join(name))?;
            file.write_all(bytes)?;
            file.sync_all()?;
            manifest.files.push(ManifestFile {
                name,
                sha256: sha256(bytes),
                bytes: bytes.len(),
            });
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(temp.join("MANIFEST.json"))?;
        file.write_all(&serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?)?;
        file.sync_all()?;
        fs::File::open(&temp)?.sync_all()?;
        fs::rename(&temp, &final_dir)?;
        fs::File::open(store_dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    result
}

/// Commits an initial plan: the snapshot first, the store last. A store that appeared meanwhile
/// is never replaced.
pub fn commit_initial(store: &Store, plan: Plan) -> Result<StoreV1, Failure> {
    // `can_commit` is the whole answer: accepting spellings makes it true only when nothing blocks.
    if !plan.summary.can_commit {
        return Err(Failure::new(ErrorCode::InvalidRecord).with_field("migration"));
    }
    if !plan.snapshot.is_empty() {
        write_snapshot(store.dir(), &plan.summary.preview_id, &plan.snapshot)
            .map_err(|_| Failure::new(ErrorCode::IoFailure))?;
    }
    store.create(plan.store).map_err(|error| match error {
        StoreError::StoreState(_) => Failure::new(ErrorCode::RevisionConflict).with_field("store"),
        other => store_failure(other),
    })
}

/// What the store holds for a older source, to tell whether the older state moved on.
pub fn committed_digest(store: &StoreV1, kind: MigrationSourceKind) -> Option<String> {
    store
        .migration
        .as_ref()?
        .sources
        .iter()
        .find(|s| s.kind == kind)
        .map(|s| s.digest.as_str().to_owned())
}

// ---- importing what appeared later ---------------------------------------------------------------

/// What an incremental import adds. It never changes or removes a record the new store has:
/// the new store owns its workspaces, and the old layout stays with the old window.
pub struct Additions {
    projects: Vec<Project>,
    bindings: Vec<Binding>,
    workspaces: Vec<Workspace>,
    sessions: Vec<Association>,
    map: Vec<MigrationMapRow>,
    sources: Vec<(MigrationSourceKind, String)>,
    migration_id: String,
}

pub struct IncrementalPlan {
    /// The revision of the store the additions were worked out against.
    pub revision: Counter,
    pub summary: Summary,
    pub additions: Additions,
    pub snapshot: Vec<(&'static str, Vec<u8>)>,
    pub epoch: Option<String>,
}

fn terminal_key(target: &Target) -> Option<(String, String)> {
    match target {
        Target::Native {
            terminal_id,
            host_epoch,
            ..
        }
        | Target::Shell {
            terminal_id,
            host_epoch,
        } => Some((
            terminal_id.as_str().to_owned(),
            host_epoch.as_str().to_owned(),
        )),
        Target::Managed { .. } => None,
    }
}

/// Builds the additions that bring `existing` up to the older state of now: registered folders
/// the store has no binding for, and live sessions and shells no association holds. Tabs are
/// not imported again.
pub fn plan_incremental(
    env: &dyn Env,
    owner: &dyn Owner,
    sources: &Sources,
    host: Option<&HostView>,
    existing: &StoreV1,
    migration_id: &str,
) -> Result<IncrementalPlan, Failure> {
    let bad = || Failure::new(ErrorCode::InvalidRecord).with_field("migration");
    let without_layout = Sources {
        registry: sources.registry.clone(),
        layout: None,
    };
    let candidate = plan_initial(env, owner, &without_layout, host, migration_id)?;
    let mut summary = candidate.summary;
    summary.mode = Mode::Incremental;
    let bound: HashMap<&str, &Binding> = existing
        .bindings
        .iter()
        .filter(|b| b.state != BindingState::Retired)
        .map(|b| (b.path.as_str(), b))
        .collect();
    let mut remap: HashMap<ProjectId, (ProjectId, Option<(BindingId, Counter)>)> = HashMap::new();
    let mut additions = Additions {
        projects: Vec::new(),
        bindings: Vec::new(),
        workspaces: Vec::new(),
        sessions: Vec::new(),
        map: Vec::new(),
        sources: Vec::new(),
        migration_id: migration_id.to_owned(),
    };
    for project in &candidate.store.projects {
        let binding = candidate
            .store
            .bindings
            .iter()
            .find(|b| b.project_id == project.id)
            .ok_or_else(bad)?;
        match bound.get(binding.path.as_str()) {
            Some(known) => {
                remap.insert(
                    project.id.clone(),
                    (
                        known.project_id.clone(),
                        Some((known.id.clone(), known.revision)),
                    ),
                );
            }
            None => {
                additions.projects.push(project.clone());
                additions.bindings.push(binding.clone());
                additions.workspaces.extend(
                    candidate
                        .store
                        .workspaces
                        .iter()
                        .filter(|w| w.project_id.as_ref() == Some(&project.id))
                        .cloned(),
                );
            }
        }
    }
    let held: HashSet<(String, String)> = existing
        .sessions
        .iter()
        .filter_map(|a| terminal_key(&a.target))
        .collect();
    let held_native: HashSet<&str> = existing
        .sessions
        .iter()
        .filter_map(|a| match &a.target {
            Target::Native { session_id, .. } => Some(session_id.as_str()),
            _ => None,
        })
        .collect();
    for session in &candidate.store.sessions {
        let key = terminal_key(&session.target);
        let native_known = matches!(&session.target, Target::Native { session_id, .. } if held_native.contains(session_id.as_str()));
        if key.is_some_and(|k| held.contains(&k)) || native_known {
            continue;
        }
        let mut session = session.clone();
        if let Some(origin) = &session.origin_project_id
            && let Some((project, binding)) = remap.get(origin)
        {
            session.origin_project_id = Some(project.clone());
            session.binding_id = binding.as_ref().map(|(id, _)| id.clone());
            session.binding_revision = binding.as_ref().map(|(_, r)| *r);
        }
        session.state = AssociationState::Detached;
        additions.sessions.push(session);
    }
    // The records join this store: they belong to its host, not to the candidate's.
    for binding in &mut additions.bindings {
        binding.host_id = existing.host_id.clone();
    }
    for workspace in &mut additions.workspaces {
        workspace.host_id = existing.host_id.clone();
    }
    for session in &mut additions.sessions {
        session.host_id = existing.host_id.clone();
    }
    additions.map = candidate.store.migration.map(|m| m.map).unwrap_or_default();
    let known_rows: HashSet<(u8, String)> = existing
        .migration
        .as_ref()
        .map(|m| {
            m.map
                .iter()
                .map(|r| (r.source_kind as u8, r.source_key.as_str().to_owned()))
                .collect()
        })
        .unwrap_or_default();
    additions.map.retain(|r| {
        r.source_kind != MapSourceKind::LayoutLeaf
            && !known_rows.contains(&(r.source_kind as u8, r.source_key.as_str().to_owned()))
    });
    for (kind, bytes) in [
        (MigrationSourceKind::Registry, &sources.registry),
        (MigrationSourceKind::Layout, &sources.layout),
    ] {
        if let Some(bytes) = bytes {
            additions.sources.push((kind, sha256(bytes)));
        }
    }
    summary.older_changed = additions.sources.iter().any(|(kind, digest)| {
        committed_digest(existing, *kind).as_deref() != Some(digest.as_str())
    });
    summary.counts = Counts {
        projects: additions.projects.len(),
        workspaces: additions.workspaces.len(),
        sessions: additions.sessions.len(),
        ..Counts::default()
    };
    summary.sources = additions
        .sources
        .iter()
        .map(|(kind, digest)| SourceDigest {
            kind: source_kind(*kind),
            digest: digest.clone(),
        })
        .collect();
    let mut snapshot = Vec::new();
    if let Some(bytes) = &sources.registry {
        snapshot.push(("registry.json", bytes.clone()));
    }
    if let Some(bytes) = &sources.layout {
        snapshot.push(("layout.json", bytes.clone()));
    }
    Ok(IncrementalPlan {
        revision: existing.revision,
        summary,
        additions,
        snapshot,
        epoch: candidate.epoch,
    })
}

fn apply_additions(store: &mut StoreV1, additions: &Additions) -> Result<(), Failure> {
    let bad = || Failure::new(ErrorCode::InvalidRecord).with_field("migration");
    store.projects.extend(additions.projects.iter().cloned());
    store.bindings.extend(additions.bindings.iter().cloned());
    store
        .workspaces
        .extend(additions.workspaces.iter().cloned());
    store.sessions.extend(additions.sessions.iter().cloned());
    let sources: Vec<MigrationSource> = additions
        .sources
        .iter()
        .map(|(kind, digest)| {
            Ok(MigrationSource {
                kind: *kind,
                digest: Sha256Hex::try_from(digest.clone()).map_err(|_| bad())?,
            })
        })
        .collect::<Result<_, Failure>>()?;
    match &mut store.migration {
        Some(migration) => {
            for source in sources {
                match migration.sources.iter_mut().find(|s| s.kind == source.kind) {
                    Some(found) => found.digest = source.digest,
                    None => migration.sources.push(source),
                }
            }
            migration.map.extend(additions.map.iter().cloned());
        }
        None if !sources.is_empty() => {
            store.migration = Some(Migration {
                version: MigrationVersion,
                id: MigrationId::parse(&additions.migration_id).map_err(|_| bad())?,
                sources,
                map: additions.map.clone(),
                state: MigrationState::Committed,
            });
        }
        None => {}
    }
    Ok(())
}

/// Commits additions at the revision the preview was made against. Nothing already in the store
/// is touched; a store that moved on in the meantime is a conflict and a new preview.
pub fn commit_incremental(
    store: &Store,
    expected: Counter,
    plan: &IncrementalPlan,
) -> Result<StoreV1, Failure> {
    // `can_commit` is the whole answer: accepting spellings makes it true only when nothing blocks.
    if !plan.summary.can_commit {
        return Err(Failure::new(ErrorCode::InvalidRecord).with_field("migration"));
    }
    if !plan.snapshot.is_empty() {
        write_snapshot(store.dir(), &plan.summary.preview_id, &plan.snapshot)
            .map_err(|_| Failure::new(ErrorCode::IoFailure))?;
    }
    store
        .transact(expected, |s| apply_additions(s, &plan.additions))
        .map_err(store_failure)?
}
