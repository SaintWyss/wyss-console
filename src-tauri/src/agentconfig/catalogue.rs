//! The capabilities the account and the open project keep, read for Arsenal: skill folders, agent
//! and rule files, the read-only entries of the inventory (hooks, MCP servers, plugins), and the
//! composite collections (GSD Core) that linked skills and agents are installed from. Each folder is
//! read through the same approved roots, bounded reads and link rules as the inventory. Nothing is
//! executed, nothing is written, and no link is followed out of the folder that holds it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::inventory::{self, Environment};
use super::model::{Bounds, ItemKind, PathTrust, ProviderId, Scope, SourceFailure};
use super::reader::{ConfigReader, EntryKind};
use super::redact::{self, SecretClass};
use super::roots::ApprovedRoots;

/// Most entries one folder reports. The rest are counted as truncated.
pub const MAX_ENTRIES: usize = 256;
/// Most files one skill may hold. A skill that holds more cannot be imported.
pub const MAX_FILES: usize = 256;
/// Largest single file of a capability. A larger one blocks it.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// Largest whole skill. A larger one blocks it.
pub const MAX_TREE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 6;
const FRONT_MATTER_BYTES: usize = 8 * 1024;
const DESCRIPTION_CHARS: usize = 500;
const SCRIPT_EXTENSIONS: [&str; 11] = [
    "sh", "bash", "zsh", "py", "js", "mjs", "cjs", "ts", "rb", "pl", "ps1",
];
/// Where GSD Core installs its payload, relative to the account's home.
const GSD_BASE: &str = ".local/share/wyss/gsd-core";
const GSD_SCHEMA: &str = "wyss.gsd-core-additive-payload-manifest.v1";
/// Why a read-only entry is listed but never imported.
const READ_ONLY: &str = "read_only_kind";
/// The skills Codex installs and replaces itself: listed, never read or imported.
const SYSTEM_SKILLS: &str = "codex_system_skills";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Read,
    Absent,
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    pub source: &'static str,
    pub state: SourceState,
}

/// A file or folder that was not copied, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Excluded {
    pub path: String,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDigest {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// The composite collection a link is installed from, found from the link's target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManagedBy {
    pub composite: &'static str,
    pub version: String,
    pub artifacts: u64,
}

/// One capability as it stands right now. `content` is what an import copies; it is never
/// serialized, so the window only ever sees the digests.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    /// `skill`, `agent`, `rule`, or a read-only kind: `hook`, `mcp_server`, `plugin`.
    pub kind: &'static str,
    pub provider: &'static str,
    pub source: String,
    /// `user` for the account's folders, `project` for the open project's.
    pub scope: &'static str,
    pub name: String,
    pub description: String,
    /// Where it was read, with the home folder shown as `~`. For display only.
    pub origin: String,
    pub files: Vec<FileDigest>,
    pub excluded: Vec<Excluded>,
    /// Copied files whose text mentions a credential word. They do not block the import.
    pub review: Vec<Excluded>,
    pub executes_code: bool,
    /// Why it cannot be imported; `None` when it can.
    pub blocked: Option<&'static str>,
    pub managed_by: Option<ManagedBy>,
    #[serde(skip)]
    pub content: Vec<(String, Vec<u8>)>,
}

/// A composite collection: a GSD Core payload found through its members' links, or a Claude Code
/// plugin package found through the provider's own install record. It is described, never copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Composite {
    /// Stable key: `gsd_core@<version>`, `claude_plugin:<plugin>@<marketplace>` or
    /// `codex_plugin:<plugin>@<marketplace>`.
    pub key: String,
    /// `gsd_core`, `claude_plugin` or `codex_plugin`.
    pub kind: &'static str,
    pub provider: &'static str,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Artifacts its manifest declares; 0 when it has no manifest of that kind.
    pub artifacts: u64,
    /// How many of this scan's entries are installed from it.
    pub members: usize,
    /// Where its payload lives, with the home folder shown as `~`.
    pub origin: String,
    /// The provider's own switch for it, when it has one and states it.
    pub enabled: Option<bool>,
    /// It controls how work is run (plan, execute, verify), so two of them in one session conflict.
    pub methodology: bool,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub commands: usize,
    pub hooks: bool,
    pub mcp: bool,
    /// Manifest artifact counts by category, for a payload with a manifest.
    pub categories: Vec<(String, u64)>,
    pub limitations: Vec<&'static str>,
}

/// Composites that control how work runs. Named in the Arsenal design (2.1, `controlling`); the
/// list only marks a conflict when two are kept together, it enables nothing.
const METHODOLOGIES: [&str; 2] = ["GSD Core", "superpowers"];
/// Most plugin packages read from one install record.
const MAX_PLUGINS: usize = 64;

#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    pub sources: Vec<SourceReport>,
    pub candidates: Vec<Candidate>,
    pub composites: Vec<Composite>,
    pub truncated: bool,
}

/// The capabilities of this process's account, and of `project` when one is open.
pub fn scan_process(project: Option<PathBuf>) -> Option<Scan> {
    Environment::from_process(project).map(|env| scan(&env))
}

/// One entry, read from disk now, the way a full scan lists it: the same folders, reads and checks,
/// for the entries of `kind` in `source` called `name`. Import revalidates through this, not a full scan.
pub fn scan_entry_process(
    project: Option<PathBuf>,
    kind: &str,
    source: &str,
    name: &str,
) -> Option<Scan> {
    let only = Only { kind, source, name };
    Environment::from_process(project).map(|env| scan_with(&env, Some(only)))
}

/// What one provider would load for a session: its skill folders (account and project), the
/// composites they link into, its plugin packages, and the MCP servers it has configured.
#[derive(Debug, Clone)]
pub struct LaunchScan {
    pub skills: Vec<Candidate>,
    pub composites: Vec<Composite>,
    pub mcp_servers: std::collections::BTreeSet<String>,
    /// A folder listed more entries than are read. The exclusion would be incomplete.
    pub truncated: bool,
    /// Skill folders that exist but could not be read. The exclusion would be incomplete.
    pub unreadable: Vec<&'static str>,
    /// For each of `skills`, in order: the `SKILL.md` the provider reads, as an absolute path through
    /// the folder it is installed in (a link is named, not resolved). `None` for an entry without one.
    pub skill_files: Vec<Option<PathBuf>>,
    /// For each of `skills`, in order: further `SKILL.md` files inside its folder. Codex lists each
    /// of them as a skill of its own, so switching the folder's skill off must name them too.
    pub nested_files: Vec<Vec<PathBuf>>,
    /// Installed skills the provider's own configuration already switches off: Claude Code's
    /// `skillOverrides` (account, project, local), Codex's `[[skills.config]]` with `enabled = false`.
    pub base_off: std::collections::BTreeSet<String>,
}

/// A project folder accepted by the same checks Configuration applies, or `None`.
pub fn validate_project_process(path: &Path) -> Option<PathBuf> {
    let env = Environment::from_process(None)?;
    super::project::validate(&env, path).ok()
}

pub fn launch_scan_process(project: Option<PathBuf>, provider: &str) -> Option<LaunchScan> {
    Environment::from_process(project).map(|env| launch_scan(&env, provider))
}

pub fn launch_scan(env: &Environment, provider: &str) -> LaunchScan {
    let mut scan = Scan {
        sources: Vec::new(),
        candidates: Vec::new(),
        composites: Vec::new(),
        truncated: false,
    };
    let manifests = Mutex::new(BTreeMap::new());
    let mut unreadable = Vec::new();
    for folder in folders(env) {
        if folder.kind != "skill" || folder.provider != provider {
            continue;
        }
        if scan_folder(env, &folder, None, &mut scan, &manifests) == SourceState::Unreadable {
            unreadable.push(folder.source);
        }
    }
    let mut composites = composites(env, &scan.candidates);
    if provider == "claude_code" {
        composites.extend(plugin_packages(env));
    } else {
        composites.extend(codex_plugin_packages(env));
    }
    let wanted = match provider {
        "claude_code" => ProviderId::ClaudeCode,
        _ => ProviderId::Codex,
    };
    let mcp_servers = inventory::inventory(env, Bounds::PROPOSED)
        .map(|inventory| {
            inventory
                .items
                .iter()
                .filter(|item| item.kind == ItemKind::McpServer && item.provider == wanted)
                .map(|item| item.name.as_str().to_owned())
                .collect()
        })
        .unwrap_or_default();
    let skill_files: Vec<Option<PathBuf>> = scan
        .candidates
        .iter()
        .map(|candidate| {
            (candidate.blocked != Some("no_skill_file")).then(|| {
                let folder = candidate.origin.strip_prefix("~/").map_or_else(
                    || PathBuf::from(&candidate.origin),
                    |rest| env.home.join(rest),
                );
                folder.join("SKILL.md")
            })
        })
        .collect();
    let mut truncated = scan.truncated;
    let nested_files: Vec<Vec<PathBuf>> = if provider == "codex" {
        scan.candidates
            .iter()
            .zip(&skill_files)
            .map(|(candidate, file)| match (candidate.blocked, file) {
                (Some("symbolic_link"), _) | (_, None) => Vec::new(),
                (_, Some(file)) => file.parent().map_or_else(Vec::new, |folder| {
                    let (found, complete) = nested_skill_files(folder);
                    truncated |= !complete;
                    found
                }),
            })
            .collect()
    } else {
        vec![Vec::new(); scan.candidates.len()]
    };
    let base_off = if provider == "claude_code" {
        claude_base_off(env)
    } else {
        codex_base_off(env, &scan.candidates, &skill_files)
    };
    LaunchScan {
        skills: scan.candidates,
        composites,
        mcp_servers,
        truncated,
        unreadable,
        skill_files,
        nested_files,
        base_off,
    }
}

/// Most entries read while looking for skills nested in one skill's folder.
const MAX_NESTED_WALK: usize = 4096;
/// How deep below a skill's folder a nested skill is looked for. Deeper folders make the walk
/// incomplete rather than being skipped.
const MAX_NESTED_DEPTH: usize = 12;

/// `SKILL.md` files below a skill's own folder: in its subfolders, not following links and not
/// entering hidden folders or `node_modules`. Returns them and whether the walk is complete: a folder
/// that cannot be read, one past the depth or entry bounds, or a nested `SKILL.md` that is a link
/// (it could name a file elsewhere) make it incomplete, and an incomplete walk is never applied.
fn nested_skill_files(folder: &Path) -> (Vec<PathBuf>, bool) {
    let mut found = Vec::new();
    let mut seen = 0_usize;
    let mut stack = vec![(folder.to_path_buf(), 0_usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return (found, false);
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return (found, false);
            };
            seen += 1;
            if seen > MAX_NESTED_WALK {
                return (found, false);
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !kind.is_dir() || name.starts_with('.') || name == "node_modules" {
                continue;
            }
            let path = entry.path();
            let skill = path.join("SKILL.md");
            match std::fs::symlink_metadata(&skill) {
                Ok(meta) if meta.file_type().is_file() => found.push(skill),
                Ok(_) => return (found, false),
                Err(_) => {}
            }
            if depth + 1 >= MAX_NESTED_DEPTH {
                return (found, false);
            }
            stack.push((path, depth + 1));
        }
    }
    found.sort();
    (found, true)
}

/// Skill names Claude Code's own settings switch off, the most specific file winning: the account's
/// `settings.json`, then the project's `.claude/settings.json`, then its `.claude/settings.local.json`.
fn claude_base_off(env: &Environment) -> std::collections::BTreeSet<String> {
    let mut files = vec![env.claude_dir().join("settings.json")];
    if let Some(project) = &env.project {
        files.push(project.join(".claude/settings.json"));
        files.push(project.join(".claude/settings.local.json"));
    }
    let mut value: BTreeMap<String, bool> = BTreeMap::new();
    for file in files {
        let Some(parent) = file.parent() else {
            continue;
        };
        let Some(reader) = ApprovedRoots::new([parent.to_path_buf()])
            .ok()
            .and_then(|roots| ConfigReader::new(roots, Bounds::PROPOSED).ok())
        else {
            continue;
        };
        let Ok(read) = reader.read_file(&file) else {
            continue;
        };
        let Ok(settings) = serde_json::from_slice::<serde_json::Value>(read.bytes()) else {
            continue;
        };
        if let Some(overrides) = settings["skillOverrides"].as_object() {
            for (name, state) in overrides.iter().take(MAX_ENTRIES) {
                if let Some(state) = state.as_str() {
                    value.insert(name.clone(), state == "off");
                }
            }
        }
    }
    value
        .into_iter()
        .filter_map(|(name, off)| off.then_some(name))
        .collect()
}

/// Installed Codex skills its `config.toml` switches off: a `[[skills.config]]` entry whose `path` is
/// the skill's `SKILL.md` (through its folder or its resolved file) with `enabled = false`. A folder
/// path is ignored, as Codex ignores it; the last entry for a file wins.
fn codex_base_off(
    env: &Environment,
    skills: &[Candidate],
    files: &[Option<PathBuf>],
) -> std::collections::BTreeSet<String> {
    let codex = env.codex_dir();
    let Some(reader) = ApprovedRoots::new([codex.clone()])
        .ok()
        .and_then(|roots| ConfigReader::new(roots, Bounds::PROPOSED).ok())
    else {
        return Default::default();
    };
    let Ok(config) = reader.read_file(&codex.join("config.toml")) else {
        return Default::default();
    };
    let Ok(Some(config)) = super::reader::parse_toml(config.bytes(), &Bounds::PROPOSED) else {
        return Default::default();
    };
    let entries: Vec<(PathBuf, bool)> = config["skills"]["config"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some((
                PathBuf::from(entry["path"].as_str()?),
                entry["enabled"].as_bool()?,
            ))
        })
        .collect();
    skills
        .iter()
        .zip(files)
        .filter_map(|(skill, file)| {
            let file = file.as_ref()?;
            let real = std::fs::canonicalize(file).ok();
            // The last entry naming this file, by either of its paths, decides.
            let off = entries
                .iter()
                .rev()
                .find(|(path, _)| path == file || Some(path) == real.as_ref())
                .is_some_and(|(_, on)| !on);
            off.then(|| skill.name.clone())
        })
        .collect()
}

/// The same scan with `home` as the account folder. Used by tests and fixtures.
pub fn scan_home(home: PathBuf, project: Option<PathBuf>) -> Scan {
    scan(&Environment {
        home,
        claude_config_dir: None,
        codex_home: None,
        project,
        system: PathBuf::from("/etc"),
        declared_writers: Vec::new(),
    })
}

/// A capability's name: 1 to 64 letters, digits, `.`, `_` or `-`, not hidden.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// How a folder holds its capabilities.
#[derive(Clone, Copy)]
enum Shape {
    /// One folder per skill, each with a `SKILL.md`.
    Skills,
    /// One file per capability, with this extension.
    Files(&'static str),
    /// One file with this exact name.
    Named(&'static str),
}

struct Folder {
    kind: &'static str,
    provider: &'static str,
    source: &'static str,
    scope: &'static str,
    path: PathBuf,
    shape: Shape,
}

fn folders(env: &Environment) -> Vec<Folder> {
    let claude = env.claude_dir();
    let codex = env.codex_dir();
    let mut folders = vec![
        folder(
            "skill",
            "claude_code",
            "claude_user_skills",
            "user",
            claude.join("skills"),
            Shape::Skills,
        ),
        folder(
            "skill",
            "codex",
            "codex_user_skills",
            "user",
            env.agents_skills_dir(),
            Shape::Skills,
        ),
        // Codex also reads skills from its own home: the folder of the account's skills, and a
        // hidden `.system` folder of skills Codex installs and replaces itself.
        folder(
            "skill",
            "codex",
            "codex_home_skills",
            "user",
            codex.join("skills"),
            Shape::Skills,
        ),
        folder(
            "skill",
            "codex",
            "codex_system_skills",
            "user",
            codex.join("skills/.system"),
            Shape::Skills,
        ),
        folder(
            "agent",
            "claude_code",
            "claude_user_agents",
            "user",
            claude.join("agents"),
            Shape::Files("md"),
        ),
        folder(
            "agent",
            "codex",
            "codex_user_agents",
            "user",
            codex.join("agents"),
            Shape::Files("toml"),
        ),
        folder(
            "rule",
            "claude_code",
            "claude_user_rules",
            "user",
            claude.join("rules"),
            Shape::Files("md"),
        ),
        folder(
            "rule",
            "claude_code",
            "claude_user_rules",
            "user",
            claude.clone(),
            Shape::Named("CLAUDE.md"),
        ),
        folder(
            "rule",
            "codex",
            "codex_user_rules",
            "user",
            codex,
            Shape::Named("AGENTS.md"),
        ),
    ];
    if let Some(project) = &env.project {
        folders.extend([
            folder(
                "skill",
                "claude_code",
                "claude_project_skills",
                "project",
                project.join(".claude/skills"),
                Shape::Skills,
            ),
            folder(
                "skill",
                "codex",
                "codex_project_skills",
                "project",
                project.join(".agents/skills"),
                Shape::Skills,
            ),
            folder(
                "agent",
                "claude_code",
                "claude_project_agents",
                "project",
                project.join(".claude/agents"),
                Shape::Files("md"),
            ),
            folder(
                "agent",
                "codex",
                "codex_project_agents",
                "project",
                project.join(".codex/agents"),
                Shape::Files("toml"),
            ),
            folder(
                "rule",
                "claude_code",
                "claude_project_rules",
                "project",
                project.join(".claude/rules"),
                Shape::Files("md"),
            ),
            folder(
                "rule",
                "claude_code",
                "claude_project_rules",
                "project",
                project.clone(),
                Shape::Named("CLAUDE.md"),
            ),
            folder(
                "rule",
                "codex",
                "codex_project_rules",
                "project",
                project.clone(),
                Shape::Named("AGENTS.md"),
            ),
        ]);
    }
    folders
}

fn folder(
    kind: &'static str,
    provider: &'static str,
    source: &'static str,
    scope: &'static str,
    path: PathBuf,
    shape: Shape,
) -> Folder {
    Folder {
        kind,
        provider,
        source,
        scope,
        path,
        shape,
    }
}

/// Which entries a scan keeps: one entry, by its kind, its source and its name.
#[derive(Clone, Copy)]
struct Only<'a> {
    kind: &'a str,
    source: &'a str,
    name: &'a str,
}

fn scan(env: &Environment) -> Scan {
    scan_with(env, None)
}

fn scan_with(env: &Environment, only: Option<Only<'_>>) -> Scan {
    let mut scan = Scan {
        sources: Vec::new(),
        candidates: Vec::new(),
        composites: Vec::new(),
        truncated: false,
    };
    let mut states: BTreeMap<&'static str, SourceState> = BTreeMap::new();
    let manifests = Mutex::new(BTreeMap::new());
    for folder in folders(env) {
        if only.is_some_and(|only| only.kind != folder.kind || only.source != folder.source) {
            continue;
        }
        let name = only.map(|only| only.name);
        let state = scan_folder(env, &folder, name, &mut scan, &manifests);
        let merged = states.entry(folder.source).or_insert(SourceState::Absent);
        *merged = merge(*merged, state);
    }
    scan.sources = states
        .into_iter()
        .map(|(source, state)| SourceReport { source, state })
        .collect();
    if only.is_none() {
        scan.candidates.extend(read_only_entries(env));
        scan.composites = composites(env, &scan.candidates);
        scan.composites.extend(plugin_packages(env));
        scan.composites.extend(codex_plugin_packages(env));
    }
    scan
}

/// A source read in any of its folders is read; one that was never present is absent.
fn merge(current: SourceState, next: SourceState) -> SourceState {
    match (current, next) {
        (SourceState::Read, _) | (_, SourceState::Read) => SourceState::Read,
        (SourceState::Unreadable, _) | (_, SourceState::Unreadable) => SourceState::Unreadable,
        _ => SourceState::Absent,
    }
}

/// One listed entry the scan keeps, before its candidate is built.
struct Entry {
    name: String,
    stem: String,
    kind: EntryKind,
}

/// Threads that build one folder's candidates at once. Reads are syscall bound, so this is more than
/// the cores a single thread would use, and no more: past it the threads only contend.
const SCAN_THREADS: usize = 6;

/// The listing is read in order and cut at `MAX_ENTRIES` before `only` filters it, so a scan of one
/// entry keeps exactly the entries a full scan keeps.
fn scan_folder(
    env: &Environment,
    folder: &Folder,
    only: Option<&str>,
    scan: &mut Scan,
    manifests: &Mutex<BTreeMap<PathBuf, Option<ManagedBy>>>,
) -> SourceState {
    let Some(reader) = ApprovedRoots::new([folder.path.clone()])
        .ok()
        .and_then(|roots| ConfigReader::new(roots, Bounds::PROPOSED).ok())
    else {
        return SourceState::Unreadable;
    };
    let listing = match reader.list_directory(&folder.path) {
        Ok(listing) => listing,
        Err(SourceFailure::Missing) => return SourceState::Absent,
        Err(_) => return SourceState::Unreadable,
    };
    let mut entries = Vec::new();
    let mut found = 0;
    for entry in listing.entries {
        let Some(name) = entry.name.to_str() else {
            continue;
        };
        let matches = match folder.shape {
            Shape::Skills => matches!(entry.kind, EntryKind::Directory | EntryKind::Link),
            Shape::Files(extension) => {
                name.strip_suffix(extension)
                    .is_some_and(|stem| stem.ends_with('.'))
                    && matches!(entry.kind, EntryKind::File | EntryKind::Link)
            }
            Shape::Named(exact) => {
                name == exact && matches!(entry.kind, EntryKind::File | EntryKind::Link)
            }
        };
        if !matches {
            continue;
        }
        let stem = match folder.shape {
            Shape::Files(extension) => name.trim_end_matches(extension).trim_end_matches('.'),
            Shape::Named(exact) => exact.trim_end_matches(".md"),
            Shape::Skills => name,
        };
        if !name_ok(stem) {
            continue;
        }
        if found == MAX_ENTRIES {
            scan.truncated = true;
            break;
        }
        found += 1;
        if only.is_some_and(|wanted| wanted != stem) {
            continue;
        }
        entries.push(Entry {
            name: name.to_owned(),
            stem: stem.to_owned(),
            kind: entry.kind,
        });
    }
    let built = parallel_map(&entries, |entry| {
        candidate_of(env, &reader, folder, entry, manifests)
    });
    scan.candidates.extend(built.into_iter().flatten());
    SourceState::Read
}

/// The candidate of one entry. A link is never followed: it is blocked, and it names the composite it
/// links into. An entry of another kind is not a capability and yields nothing.
fn candidate_of(
    env: &Environment,
    reader: &ConfigReader,
    folder: &Folder,
    entry: &Entry,
    manifests: &Mutex<BTreeMap<PathBuf, Option<ManagedBy>>>,
) -> Option<Candidate> {
    let origin = origin(&env.home, &folder.path, &entry.name);
    let link = folder.path.join(&entry.name);
    if folder.source == SYSTEM_SKILLS && entry.kind != EntryKind::Other {
        return Some(blocked(folder, &entry.stem, origin, "provider_managed"));
    }
    match entry.kind {
        EntryKind::Link => {
            let mut candidate = blocked(folder, &entry.stem, origin, "symbolic_link");
            candidate.managed_by = composite_of(env, &link, manifests);
            Some(candidate)
        }
        EntryKind::Directory => Some(read_skill(reader, folder, &entry.stem, &link, origin)),
        EntryKind::File => Some(file_candidate(
            reader,
            folder,
            &entry.stem,
            &entry.name,
            &link,
            origin,
        )),
        EntryKind::Other => None,
    }
}

/// Maps `items` on up to `SCAN_THREADS` threads, and returns the results in the order of `items`.
/// Threads take every `threads`-th item, so one large skill does not hold back a whole chunk.
fn parallel_map<T: Sync, U: Send>(items: &[T], map: impl Fn(&T) -> U + Sync) -> Vec<U> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |count| count.get())
        .min(SCAN_THREADS);
    if threads < 2 || items.len() < 2 {
        return items.iter().map(map).collect();
    }
    std::thread::scope(|scope| {
        let map = &map;
        let workers: Vec<_> = (0..threads)
            .map(|first| {
                scope.spawn(move || {
                    items
                        .iter()
                        .enumerate()
                        .skip(first)
                        .step_by(threads)
                        .map(|(index, item)| (index, map(item)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut done: Vec<(usize, U)> = workers
            .into_iter()
            .flat_map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect();
        done.sort_by_key(|(index, _)| *index);
        done.into_iter().map(|(_, value)| value).collect()
    })
}

fn blocked(folder: &Folder, name: &str, origin: String, reason: &'static str) -> Candidate {
    Candidate {
        kind: folder.kind,
        provider: folder.provider,
        source: folder.source.to_owned(),
        scope: folder.scope,
        name: name.to_owned(),
        description: String::new(),
        origin,
        files: Vec::new(),
        excluded: Vec::new(),
        review: Vec::new(),
        executes_code: false,
        blocked: Some(reason),
        managed_by: None,
        content: Vec::new(),
    }
}

/// A single file that is a capability by itself: an agent definition or a rule.
fn file_candidate(
    reader: &ConfigReader,
    folder: &Folder,
    name: &str,
    file_name: &str,
    path: &Path,
    origin: String,
) -> Candidate {
    let mut tree = Tree::default();
    take_file(reader, path, file_name.to_owned(), &mut tree);
    let description = tree
        .files
        .first()
        .map(|(_, bytes)| description(bytes))
        .unwrap_or_default();
    assemble(folder, name, origin, description, tree)
}

/// The limit a tree has passed, if it has passed one. Reading on past it could only confirm the block.
fn tree_limit(tree: &Tree) -> Option<&'static str> {
    if tree.files.len() > MAX_FILES {
        Some("too_many_files")
    } else if tree.bytes > MAX_TREE_BYTES {
        Some("tree_too_large")
    } else {
        None
    }
}

/// A skill folder: every file under it is read, and a link anywhere under it is never followed. Once
/// the tree passes a limit it is blocked, so the walk stops there: a folder of thousands of files is
/// not read in full only to be refused.
fn read_skill(
    reader: &ConfigReader,
    folder: &Folder,
    name: &str,
    dir: &Path,
    origin: String,
) -> Candidate {
    let mut tree = Tree::default();
    let mut pending = vec![(dir.to_path_buf(), String::new(), 0_usize)];
    'walk: while let Some((current, prefix, depth)) = pending.pop() {
        let listing = match reader.list_directory(&current) {
            Ok(listing) => listing,
            Err(failure) if prefix.is_empty() => {
                tree.blocked.get_or_insert(failure.as_str());
                continue;
            }
            Err(failure) => {
                tree.excluded.push(Excluded {
                    path: prefix,
                    reason: failure.as_str(),
                });
                continue;
            }
        };
        for entry in listing.entries {
            if let Some(reason) = tree_limit(&tree) {
                tree.blocked.get_or_insert(reason);
                break 'walk;
            }
            let child = entry.name.to_string_lossy().into_owned();
            let relative = join(&prefix, &child);
            if entry.name.to_str().is_none() {
                tree.excluded.push(Excluded {
                    path: relative,
                    reason: "non_utf8_name",
                });
                continue;
            }
            if child.starts_with('.') {
                tree.excluded.push(Excluded {
                    path: relative,
                    reason: "hidden_entry",
                });
                continue;
            }
            match entry.kind {
                EntryKind::Directory if depth + 1 > MAX_DEPTH => tree.excluded.push(Excluded {
                    path: relative,
                    reason: "too_deep",
                }),
                EntryKind::Directory => pending.push((current.join(&child), relative, depth + 1)),
                EntryKind::File => take_file(reader, &current.join(&child), relative, &mut tree),
                EntryKind::Link => tree.excluded.push(Excluded {
                    path: relative,
                    reason: "symbolic_link",
                }),
                EntryKind::Other => tree.excluded.push(Excluded {
                    path: relative,
                    reason: "special_file",
                }),
            }
        }
    }
    if tree.files.len() > MAX_FILES {
        tree.blocked.get_or_insert("too_many_files");
    }
    if tree.bytes > MAX_TREE_BYTES {
        tree.blocked.get_or_insert("tree_too_large");
    }
    if !tree.files.iter().any(|(path, _)| path == "SKILL.md") {
        tree.blocked.get_or_insert("no_skill_file");
    }
    let description = tree
        .files
        .iter()
        .find(|(path, _)| path == "SKILL.md")
        .map(|(_, bytes)| description(bytes))
        .unwrap_or_default();
    assemble(folder, name, origin, description, tree)
}

fn assemble(
    folder: &Folder,
    name: &str,
    origin: String,
    description: String,
    tree: Tree,
) -> Candidate {
    let files = tree
        .files
        .iter()
        .map(|(path, bytes)| FileDigest {
            path: path.clone(),
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        })
        .collect();
    Candidate {
        kind: folder.kind,
        provider: folder.provider,
        source: folder.source.to_owned(),
        scope: folder.scope,
        name: name.to_owned(),
        description,
        origin,
        files,
        excluded: tree.excluded,
        review: tree.review,
        executes_code: tree.executes_code,
        // A project read through a shared folder is listed, never copied: its folder is writable by its group,
        // and the group was accepted only as read-only discovery (project.rs). Account folders keep their policy.
        blocked: tree
            .blocked
            .or((tree.shared && folder.scope == "project").then_some("shared_path")),
        managed_by: None,
        content: tree.files,
    }
}

#[derive(Default)]
struct Tree {
    files: Vec<(String, Vec<u8>)>,
    excluded: Vec<Excluded>,
    review: Vec<Excluded>,
    blocked: Option<&'static str>,
    executes_code: bool,
    bytes: usize,
    /// A file was read through a folder another account can write. See `assemble`.
    shared: bool,
}

fn take_file(reader: &ConfigReader, path: &Path, relative: String, tree: &mut Tree) {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if credential_name(name) {
        tree.excluded.push(Excluded {
            path: relative,
            reason: "credential_name",
        });
        return;
    }
    let bytes = match reader.read_file(path) {
        Ok(file) => {
            if matches!(file.trust(), PathTrust::ReadOnly(_)) {
                tree.shared = true;
            }
            file.bytes().to_vec()
        }
        Err(failure) => {
            tree.excluded.push(Excluded {
                path: relative,
                reason: failure.as_str(),
            });
            return;
        }
    };
    if bytes.len() > MAX_FILE_BYTES {
        tree.blocked.get_or_insert("file_too_large");
        tree.excluded.push(Excluded {
            path: relative,
            reason: "too_large",
        });
        return;
    }
    let verdict = std::str::from_utf8(&bytes).ok().map(verdict);
    match verdict {
        None => tree.excluded.push(Excluded {
            path: relative,
            reason: "binary_file",
        }),
        // A credential value blocks the whole capability: a partial copy would still carry it.
        Some((true, _)) => {
            tree.blocked.get_or_insert("needs_secret");
            tree.excluded.push(Excluded {
                path: relative,
                reason: "may_hold_secret",
            });
        }
        Some((false, mentions)) => {
            if mentions {
                tree.review.push(Excluded {
                    path: relative.clone(),
                    reason: "mentions_credential",
                });
            }
            tree.executes_code |= is_script(&relative);
            tree.bytes += bytes.len();
            tree.files.push((relative, bytes));
        }
    }
}

/// Whether the text carries a credential by its shape, and whether it mentions one at all. The text
/// is redacted once. A token, a password inside a URL, or a private key carries one: a provider's token,
/// a password in a URL, or a private key. A credential word in prose, such as `password` or `token`,
/// only mentions one, so the file is listed for review instead of blocking its capability.
fn verdict(text: &str) -> (bool, bool) {
    let found = redact::text(text).found;
    let private_key = text.contains("-----BEGIN") && text.contains("PRIVATE KEY-----");
    let url_password = found.contains(&SecretClass::UrlCredentials);
    let provider_token = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        .any(|word| {
            word.len() >= 16
                && redact::TOKEN_PREFIXES
                    .iter()
                    .any(|prefix| word.starts_with(prefix))
        });
    (
        private_key || url_password || provider_token,
        !found.is_empty(),
    )
}

/// The `description:` of a file's YAML front matter, read from its first kilobytes.
fn description(bytes: &[u8]) -> String {
    let head = &bytes[..bytes.len().min(FRONT_MATTER_BYTES)];
    let Ok(text) = std::str::from_utf8(head) else {
        return String::new();
    };
    let Some(rest) = text.strip_prefix("---\n") else {
        return String::new();
    };
    let front = rest.split("\n---").next().unwrap_or_default();
    front
        .lines()
        .find_map(|line| line.strip_prefix("description:"))
        .map(|value| {
            value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .chars()
                .take(DESCRIPTION_CHARS)
                .collect()
        })
        .unwrap_or_default()
}

/// Why a file received from another computer may not be stored: the reasons discovery leaves a file
/// out or blocks a capability for, applied to bytes that did not come from a scan. `None` is safe.
pub(crate) fn unsafe_file(path: &str, bytes: &[u8]) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if credential_name(name) {
        return Some("credential_name");
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Some("file_too_large");
    }
    match std::str::from_utf8(bytes) {
        Err(_) => Some("binary_file"),
        Ok(text) if verdict(text).0 => Some("may_hold_secret"),
        Ok(_) => None,
    }
}

/// A file that is credentials by its name. Narrow on purpose: design token files are not credentials,
/// and what a file says is checked as text in `take_file`.
fn credential_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("credentials")
        || lower == "auth.json"
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
        || lower.contains("id_rsa")
        || lower.contains("id_ed25519")
}

/// Whether a file of a capability is code a provider would run. Wyss never runs it.
pub fn is_script(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| SCRIPT_EXTENSIONS.contains(&extension))
}

/// The composite a link is installed from, when its target is inside a GSD Core payload. The link's
/// target text is read, and the payload's own manifest; nothing behind the link is followed.
fn composite_of(
    env: &Environment,
    link: &Path,
    manifests: &Mutex<BTreeMap<PathBuf, Option<ManagedBy>>>,
) -> Option<ManagedBy> {
    let target = std::fs::read_link(link).ok()?;
    let target = if target.is_absolute() {
        target
    } else {
        link.parent()?.join(target)
    };
    let resolved = std::fs::canonicalize(&target).ok()?;
    let base = std::fs::canonicalize(env.home.join(GSD_BASE)).ok()?;
    let version = resolved
        .strip_prefix(&base)
        .ok()?
        .components()
        .next()?
        .as_os_str()
        .to_str()?
        .to_owned();
    if !name_ok(&version) {
        return None;
    }
    let root = base.join(&version);
    // The lock guards the cache only: two threads that miss at once read the same manifest, which is harmless.
    let known = manifests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&root)
        .cloned();
    if let Some(known) = known {
        return known;
    }
    let found = gsd_manifest(&root, &version);
    manifests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(root, found.clone());
    found
}

fn gsd_manifest(root: &Path, version: &str) -> Option<ManagedBy> {
    let roots = ApprovedRoots::new([root.to_path_buf()]).ok()?;
    let reader = ConfigReader::new(roots, Bounds::PROPOSED).ok()?;
    let file = reader.read_file(&root.join("manifest.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_slice(file.bytes()).ok()?;
    if manifest.get("schema")?.as_str()? != GSD_SCHEMA {
        return None;
    }
    Some(ManagedBy {
        composite: "GSD Core",
        version: version.to_owned(),
        artifacts: manifest.get("artifact_count")?.as_u64()?,
    })
}

/// Composites found through this scan's links, each with its linked members and its manifest's
/// artifact categories.
fn composites(env: &Environment, candidates: &[Candidate]) -> Vec<Composite> {
    type Found = (u64, Vec<String>, Vec<String>, BTreeMap<&'static str, usize>);
    let mut found: BTreeMap<(String, String), Found> = BTreeMap::new();
    for candidate in candidates {
        let Some(managed) = candidate.managed_by.as_ref() else {
            continue;
        };
        let entry = found
            .entry((managed.composite.to_owned(), managed.version.clone()))
            .or_insert_with(|| (managed.artifacts, Vec::new(), Vec::new(), BTreeMap::new()));
        match candidate.kind {
            "skill" => entry.1.push(candidate.name.clone()),
            "agent" => entry.2.push(candidate.name.clone()),
            _ => {}
        }
        *entry.3.entry(candidate.provider).or_insert(0) += 1;
    }
    found
        .into_iter()
        .map(|((name, version), (artifacts, mut skills, mut agents, providers))| {
            skills.sort();
            skills.dedup();
            agents.sort();
            agents.dedup();
            let members = providers.values().sum();
            let provider = match (providers.contains_key("claude_code"), providers.contains_key("codex")) {
                (true, true) => "shared",
                (false, true) => "codex",
                _ => "claude_code",
            };
            Composite {
                key: format!("gsd_core@{version}"),
                kind: "gsd_core",
                provider,
                origin: origin(&env.home, &env.home.join(GSD_BASE), &version),
                categories: gsd_categories(&env.home.join(GSD_BASE).join(&version)),
                methodology: METHODOLOGIES.contains(&name.as_str()),
                name: format!("{name} {version}"),
                description: "Spec-driven workflow: phases, plans, executor and verifier agents.".into(),
                version,
                artifacts,
                members,
                enabled: None,
                commands: 0,
                hooks: false,
                mcp: false,
                skills,
                agents,
                limitations: vec![
                    "Installed by GSD Core as links; Arsenal lists its members and never copies them.",
                    "Its hooks and engine are managed by the GSD Core install, not by Arsenal.",
                    "Per-agent skills for GSD stages are project scoped (.planning/config.json) and are not set here.",
                ],
            }
        })
        .collect()
}

/// Artifact counts by category from a GSD Core manifest, read through the payload's own root.
fn gsd_categories(root: &Path) -> Vec<(String, u64)> {
    let Some(reader) = ApprovedRoots::new([root.to_path_buf()])
        .ok()
        .and_then(|roots| ConfigReader::new(roots, Bounds::PROPOSED).ok())
    else {
        return Vec::new();
    };
    let Ok(file) = reader.read_file(&root.join("manifest.json")) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(file.bytes()) else {
        return Vec::new();
    };
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for artifact in manifest["artifacts"].as_array().into_iter().flatten() {
        if let Some(category) = artifact["category"].as_str()
            && category.len() <= 64
        {
            *counts.entry(category.to_owned()).or_insert(0) += 1;
        }
    }
    counts.into_iter().collect()
}

/// Claude Code plugin packages from the provider's install record (`plugins/installed_plugins.json`).
/// Each install path must lie inside `plugins/cache`; its manifest and the names of its skills,
/// agents and commands are read there, through the same bounded, link-refusing reader. Nothing in a
/// package is copied or run, and whether it is enabled comes from `settings.json` as written.
pub fn plugin_packages(env: &Environment) -> Vec<Composite> {
    let plugins = env.claude_dir().join("plugins");
    let cache = plugins.join("cache");
    let Some(reader) = ApprovedRoots::new([plugins.clone()])
        .ok()
        .and_then(|roots| ConfigReader::new(roots, Bounds::PROPOSED).ok())
    else {
        return Vec::new();
    };
    let Ok(record) = reader.read_file(&plugins.join("installed_plugins.json")) else {
        return Vec::new();
    };
    let Ok(record) = serde_json::from_slice::<serde_json::Value>(record.bytes()) else {
        return Vec::new();
    };
    let enabled = enabled_plugins(env);
    let Ok(cache_real) = std::fs::canonicalize(&cache) else {
        return Vec::new();
    };
    let mut packages = Vec::new();
    let Some(entries) = record["plugins"].as_object() else {
        return Vec::new();
    };
    for (key, installs) in entries.iter().take(MAX_PLUGINS) {
        let Some((plugin, _marketplace)) = key.split_once('@') else {
            continue;
        };
        if !name_ok(plugin) || key.len() > 128 {
            continue;
        }
        let Some(install) = installs.as_array().and_then(|list| list.first()) else {
            continue;
        };
        let Some(path) = install["installPath"].as_str().map(PathBuf::from) else {
            continue;
        };
        // The record is the provider's, but a path it names is still only read inside the cache.
        let Ok(real) = std::fs::canonicalize(&path) else {
            continue;
        };
        if !real.starts_with(&cache_real) || real == cache_real {
            continue;
        }
        let manifest = reader
            .read_file(&real.join(".claude-plugin/plugin.json"))
            .ok()
            .and_then(|file| serde_json::from_slice::<serde_json::Value>(file.bytes()).ok());
        let text = |field: &str| {
            manifest
                .as_ref()
                .and_then(|m| m[field].as_str())
                .map(|value| value.chars().take(DESCRIPTION_CHARS).collect::<String>())
        };
        let version = install["version"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| text("version"))
            .unwrap_or_default();
        let names = |folder: &str, files: bool| -> Vec<String> {
            let Ok(listing) = reader.list_directory(&real.join(folder)) else {
                return Vec::new();
            };
            let mut names: Vec<String> = listing
                .entries
                .iter()
                .filter_map(|entry| {
                    let name = entry.name.to_str()?;
                    let name = if files {
                        name.strip_suffix(".md")?
                    } else {
                        if !matches!(entry.kind, EntryKind::Directory) {
                            return None;
                        }
                        name
                    };
                    name_ok(name).then(|| name.to_owned())
                })
                .collect();
            names.sort();
            names.truncate(MAX_ENTRIES);
            names
        };
        let exists = |relative: &str| reader.read_file(&real.join(relative)).is_ok();
        let skills = names("skills", false);
        let agents = names("agents", true);
        let commands = names("commands", true).len();
        let hooks = exists("hooks/hooks.json");
        let mcp = exists(".mcp.json");
        let mut limitations = vec![
            "Plugin skills are not switched off by skillOverrides; a plugin is enabled or disabled as a whole.",
            "Importing a plugin is not available: it brings code, hooks or servers.",
        ];
        if hooks {
            limitations.push("It registers hooks, which run code at session events.");
        }
        if mcp {
            limitations.push("It declares MCP servers.");
        }
        packages.push(Composite {
            key: format!("claude_plugin:{key}"),
            kind: "claude_plugin",
            provider: "claude_code",
            name: plugin.to_owned(),
            description: text("description").unwrap_or_default(),
            version,
            artifacts: 0,
            members: skills.len() + agents.len() + commands,
            origin: origin(
                &env.home,
                real.parent().unwrap_or(&real),
                real.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            ),
            enabled: enabled.get(key.as_str()).copied(),
            methodology: METHODOLOGIES.contains(&plugin),
            skills,
            agents,
            commands,
            hooks,
            mcp,
            categories: Vec::new(),
            limitations,
        });
    }
    packages
}

/// Codex plugins named in the account's `config.toml` (`[plugins."<plugin>@<marketplace>"]`), each
/// described from its manifest in Codex's plugin cache. Only the switch is read from the
/// configuration, and of a manifest only its text fields and which kinds of parts it declares: an
/// MCP or app definition can hold credentials, so its contents are never read into the scan.
pub fn codex_plugin_packages(env: &Environment) -> Vec<Composite> {
    let codex = env.codex_dir();
    let cache = codex.join("plugins/cache");
    let Some(reader) = ApprovedRoots::new([codex.clone()])
        .ok()
        .and_then(|roots| ConfigReader::new(roots, Bounds::PROPOSED).ok())
    else {
        return Vec::new();
    };
    let Ok(config) = reader.read_file(&codex.join("config.toml")) else {
        return Vec::new();
    };
    let Ok(Some(config)) = super::reader::parse_toml(config.bytes(), &Bounds::PROPOSED) else {
        return Vec::new();
    };
    let Some(configured) = config["plugins"].as_object() else {
        return Vec::new();
    };
    let cache_real = std::fs::canonicalize(&cache).ok();
    let mut packages = Vec::new();
    for (key, switch) in configured.iter().take(MAX_PLUGINS) {
        let Some((plugin, marketplace)) = key.split_once('@') else {
            continue;
        };
        if !name_ok(plugin) || !name_ok(marketplace) || key.len() > 128 {
            continue;
        }
        let enabled = switch["enabled"].as_bool();
        // The one cached version, read only inside the cache: a link out of it is not followed.
        let installed = cache_real.as_ref().and_then(|cache_real| {
            let folder = std::fs::canonicalize(cache.join(marketplace).join(plugin)).ok()?;
            if !folder.starts_with(cache_real) {
                return None;
            }
            let listing = reader.list_directory(&folder).ok()?;
            let mut versions: Vec<String> = listing
                .entries
                .iter()
                .filter(|entry| matches!(entry.kind, EntryKind::Directory))
                .filter_map(|entry| entry.name.to_str().filter(|name| name_ok(name)))
                .map(str::to_owned)
                .collect();
            versions.sort();
            let count = versions.len();
            Some((folder.join(versions.pop()?), count))
        });
        let mut limitations = vec![
            "Codex enables or disables a plugin as a whole in config.toml; Arsenal does not switch it per terminal.",
            "Importing a plugin is not available: it brings code, hooks or servers.",
        ];
        let Some((real, versions)) = installed else {
            limitations.push("It is named in config.toml but is not in Codex's plugin cache.");
            packages.push(Composite {
                key: format!("codex_plugin:{key}"),
                kind: "codex_plugin",
                provider: "codex",
                name: plugin.to_owned(),
                version: String::new(),
                description: String::new(),
                artifacts: 0,
                members: 0,
                origin: format!(
                    "{} [plugins.\"{key}\"]",
                    origin(&env.home, &codex, "config.toml")
                ),
                enabled,
                methodology: false,
                skills: Vec::new(),
                agents: Vec::new(),
                commands: 0,
                hooks: false,
                mcp: false,
                categories: Vec::new(),
                limitations,
            });
            continue;
        };
        if versions > 1 {
            limitations.push("Several versions are cached; the newest name is shown.");
        }
        let manifest = reader
            .read_file(&real.join(".codex-plugin/plugin.json"))
            .ok()
            .and_then(|file| serde_json::from_slice::<serde_json::Value>(file.bytes()).ok())
            .unwrap_or(serde_json::Value::Null);
        let text = |field: &str| {
            manifest[field]
                .as_str()
                .map(|value| value.chars().take(DESCRIPTION_CHARS).collect::<String>())
                .unwrap_or_default()
        };
        let declares = |field: &str| !manifest[field].is_null();
        let skills: Vec<String> = reader
            .list_directory(&real.join("skills"))
            .map(|listing| {
                let mut names: Vec<String> = listing
                    .entries
                    .iter()
                    .filter(|entry| matches!(entry.kind, EntryKind::Directory))
                    .filter_map(|entry| entry.name.to_str().filter(|name| name_ok(name)))
                    .map(str::to_owned)
                    .collect();
                names.sort();
                names.truncate(MAX_ENTRIES);
                names
            })
            .unwrap_or_default();
        let hooks = declares("hooks");
        let mcp = declares("mcpServers");
        if hooks {
            limitations.push("It registers hooks, which run code at session events.");
        }
        if mcp {
            limitations.push("It declares MCP servers.");
        }
        if declares("apps") {
            limitations.push("It connects apps through the account's connectors.");
        }
        let version = Some(text("version"))
            .filter(|version| !version.is_empty())
            .unwrap_or_else(|| {
                real.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned()
            });
        packages.push(Composite {
            key: format!("codex_plugin:{key}"),
            kind: "codex_plugin",
            provider: "codex",
            name: plugin.to_owned(),
            version,
            description: text("description"),
            artifacts: 0,
            members: skills.len(),
            origin: origin(
                &env.home,
                real.parent().unwrap_or(&real),
                real.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            ),
            enabled,
            methodology: false,
            skills,
            agents: Vec::new(),
            commands: 0,
            hooks,
            mcp,
            categories: Vec::new(),
            limitations,
        });
    }
    packages
}

/// `enabledPlugins` from the account's Claude Code settings, as written there.
fn enabled_plugins(env: &Environment) -> BTreeMap<String, bool> {
    let claude = env.claude_dir();
    let Some(reader) = ApprovedRoots::new([claude.clone()])
        .ok()
        .and_then(|roots| ConfigReader::new(roots, Bounds::PROPOSED).ok())
    else {
        return BTreeMap::new();
    };
    let Ok(file) = reader.read_file(&claude.join("settings.json")) else {
        return BTreeMap::new();
    };
    let Ok(settings) = serde_json::from_slice::<serde_json::Value>(file.bytes()) else {
        return BTreeMap::new();
    };
    settings["enabledPlugins"]
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_bool().map(|on| (key.clone(), on)))
                .take(MAX_PLUGINS)
                .collect()
        })
        .unwrap_or_default()
}

/// Hooks, MCP servers and plugins from the inventory. They are listed so the owner can see them, and
/// never imported: a hook runs code, an MCP definition can hold credentials, a plugin brings both.
fn read_only_entries(env: &Environment) -> Vec<Candidate> {
    let Ok(inventory) = inventory::inventory(env, Bounds::PROPOSED) else {
        return Vec::new();
    };
    inventory
        .items
        .iter()
        .filter_map(|item| {
            let kind = match item.kind {
                ItemKind::Hook => "hook",
                ItemKind::McpServer => "mcp_server",
                ItemKind::Plugin => "plugin",
                ItemKind::RuleFile | ItemKind::Skill | ItemKind::ModelPreference => return None,
            };
            let provider = match item.provider {
                ProviderId::ClaudeCode => "claude_code",
                ProviderId::Codex => "codex",
                // OpenCode is outside Arsenal's scope in AM1a; its entries are not listed.
                ProviderId::Opencode => return None,
            };
            let scope = match item.scope {
                Scope::Project => "project",
                Scope::Global | Scope::System => "user",
            };
            let source = serde_json::to_value(item.source)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default();
            Some(Candidate {
                kind,
                provider,
                source,
                scope,
                name: item.name.as_str().to_owned(),
                description: String::new(),
                origin: String::new(),
                files: Vec::new(),
                excluded: Vec::new(),
                review: Vec::new(),
                executes_code: false,
                blocked: Some(READ_ONLY),
                managed_by: None,
                content: Vec::new(),
            })
        })
        .collect()
}

/// Where a capability was read, with the home folder shown as `~`.
fn origin(home: &Path, folder: &Path, name: &str) -> String {
    let shown = folder.strip_prefix(home).map_or_else(
        |_| folder.display().to_string(),
        |rest| format!("~/{}", rest.display()),
    );
    format!("{shown}/{name}")
}

fn join(prefix: &str, child: &str) -> String {
    if prefix.is_empty() {
        child.to_owned()
    } else {
        format!("{prefix}/{child}")
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn credential_value(text: &str) -> bool {
        verdict(text).0
    }

    fn home() -> PathBuf {
        let home =
            std::env::temp_dir().join(format!("catalogue-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(home.join(".claude/skills")).unwrap();
        home
    }

    fn find<'a>(scan: &'a Scan, kind: &str, name: &str) -> &'a Candidate {
        scan.candidates
            .iter()
            .find(|candidate| candidate.kind == kind && candidate.name == name)
            .unwrap()
    }

    #[test]
    fn reads_a_skill_tree_and_reports_what_it_leaves_out() {
        let home = home();
        let skill = home.join(".claude/skills/design");
        fs::create_dir_all(skill.join("refs")).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: design\ndescription: \"Plans screens\"\n---\nbody",
        )
        .unwrap();
        fs::write(skill.join("refs/tokens.md"), "spacing 8px").unwrap();
        fs::write(skill.join("credentials.json"), "placeholder").unwrap();
        fs::write(skill.join(".hidden"), "x").unwrap();
        let scan = scan_home(home.clone(), None);
        let found = find(&scan, "skill", "design");
        assert_eq!(found.blocked, None);
        assert_eq!(found.description, "Plans screens");
        let paths: Vec<&str> = found.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["SKILL.md", "refs/tokens.md"]);
        assert!(
            found
                .excluded
                .iter()
                .any(|entry| entry.path == "credentials.json" && entry.reason == "credential_name")
        );
        assert!(
            found
                .excluded
                .iter()
                .any(|entry| entry.path == ".hidden" && entry.reason == "hidden_entry")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_provider_token_blocks_and_a_missing_skill_file_is_reported() {
        let home = home();
        let leaky = home.join(".claude/skills/leaky");
        fs::create_dir_all(&leaky).unwrap();
        fs::write(
            leaky.join("SKILL.md"),
            format!("export TOKEN={}0123456789abcdefghijklmnop\n", "ghp_"),
        )
        .unwrap();
        let bare = home.join(".claude/skills/bare");
        fs::create_dir_all(&bare).unwrap();
        fs::write(bare.join("notes.md"), "no front matter").unwrap();
        let scan = scan_home(home.clone(), None);
        assert_eq!(find(&scan, "skill", "leaky").blocked, Some("needs_secret"));
        assert_eq!(find(&scan, "skill", "bare").blocked, Some("no_skill_file"));
        assert!(
            scan.sources
                .iter()
                .any(|source| source.source == "codex_user_skills"
                    && source.state == SourceState::Absent)
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn codex_home_skills_are_read_and_its_system_skills_are_listed_without_reading() {
        let home = home();
        let own = home.join(".codex/skills/tracer");
        fs::create_dir_all(&own).unwrap();
        fs::write(
            own.join("SKILL.md"),
            "---\nname: tracer\ndescription: Traces\n---\n",
        )
        .unwrap();
        let system = home.join(".codex/skills/.system/imagegen");
        fs::create_dir_all(&system).unwrap();
        fs::write(system.join("SKILL.md"), "---\nname: imagegen\n---\n").unwrap();
        let scan = scan_home(home.clone(), None);
        let from = |source: &str| -> Vec<&Candidate> {
            scan.candidates
                .iter()
                .filter(|candidate| candidate.source == source)
                .collect()
        };
        let own = from("codex_home_skills");
        assert_eq!(
            own.len(),
            1,
            "the hidden .system folder is not a skill of its own"
        );
        assert_eq!((own[0].name.as_str(), own[0].provider), ("tracer", "codex"));
        assert_eq!(own[0].blocked, None);
        assert_eq!(own[0].description, "Traces");
        let system = from("codex_system_skills");
        assert_eq!(system.len(), 1);
        assert_eq!(system[0].name, "imagegen");
        assert_eq!(system[0].blocked, Some("provider_managed"));
        assert!(system[0].files.is_empty() && system[0].content.is_empty());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn codex_plugins_come_from_config_and_their_cached_manifest_and_never_leave_the_cache() {
        let home = home();
        let codex = home.join(".codex");
        fs::create_dir_all(&codex).unwrap();
        fs::write(
            codex.join("config.toml"),
            concat!(
                "[plugins.\"pdf@runtime\"]\nenabled = true\n\n",
                "[plugins.\"gone@curated\"]\nenabled = false\n\n",
                "[plugins.\"escape@curated\"]\nenabled = true\n",
            ),
        )
        .unwrap();
        let pdf = codex.join("plugins/cache/runtime/pdf/1.2.3");
        fs::create_dir_all(pdf.join(".codex-plugin")).unwrap();
        fs::create_dir_all(pdf.join("skills/pdf")).unwrap();
        fs::write(pdf.join("skills/pdf/SKILL.md"), "---\nname: pdf\n---\n").unwrap();
        fs::write(
            pdf.join(".codex-plugin/plugin.json"),
            r#"{"name":"pdf","version":"1.2.3","description":"Reads PDFs","mcpServers":{"x":{"env":{"TOKEN":"never-read"}}}}"#,
        )
        .unwrap();
        let outside = home.join("outside/escape/9.9.9");
        fs::create_dir_all(outside.join(".codex-plugin")).unwrap();
        fs::write(
            outside.join(".codex-plugin/plugin.json"),
            r#"{"name":"escape"}"#,
        )
        .unwrap();
        fs::create_dir_all(codex.join("plugins/cache/curated")).unwrap();
        std::os::unix::fs::symlink(
            home.join("outside/escape"),
            codex.join("plugins/cache/curated/escape"),
        )
        .unwrap();
        let scan = scan_home(home.clone(), None);
        let plugin = |name: &str| {
            scan.composites
                .iter()
                .find(|composite| composite.kind == "codex_plugin" && composite.name == name)
                .unwrap_or_else(|| panic!("{name} is listed"))
        };
        let pdf = plugin("pdf");
        assert_eq!(pdf.key, "codex_plugin:pdf@runtime");
        assert_eq!(pdf.provider, "codex");
        assert_eq!((pdf.version.as_str(), pdf.enabled), ("1.2.3", Some(true)));
        assert_eq!(pdf.description, "Reads PDFs");
        assert_eq!(pdf.skills, ["pdf"]);
        assert!(pdf.mcp && !pdf.hooks);
        assert!(!serde_json::to_string(pdf).unwrap().contains("never-read"));
        let gone = plugin("gone");
        assert_eq!((gone.enabled, gone.members), (Some(false), 0));
        assert!(
            gone.limitations
                .iter()
                .any(|l| l.contains("not in Codex's plugin cache"))
        );
        let escape = plugin("escape");
        assert!(escape.version.is_empty() && escape.skills.is_empty());
        fs::remove_dir_all(home).unwrap();
    }

    fn launch_env(home: &Path, project: Option<PathBuf>) -> Environment {
        Environment {
            home: home.to_path_buf(),
            claude_config_dir: None,
            codex_home: None,
            project,
            system: PathBuf::from("/etc"),
            declared_writers: Vec::new(),
        }
    }

    #[test]
    fn a_launch_scan_reads_what_each_providers_own_configuration_already_switches_off() {
        let home = home();
        for name in ["design", "tokens"] {
            let skill = home.join(".claude/skills").join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(skill.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
        }
        fs::write(
            home.join(".claude/settings.json"),
            r#"{"skillOverrides":{"design":"off","tokens":"on"}}"#,
        )
        .unwrap();
        let project = home.join("app");
        fs::create_dir_all(project.join(".claude")).unwrap();
        fs::write(
            project.join(".claude/settings.local.json"),
            r#"{"skillOverrides":{"tokens":"off"}}"#,
        )
        .unwrap();
        let claude = launch_scan(&launch_env(&home, Some(project.clone())), "claude_code");
        assert_eq!(
            claude.base_off.iter().collect::<Vec<_>>(),
            ["design", "tokens"]
        );
        assert!(claude.composites.iter().all(|c| c.kind != "codex_plugin"));

        for name in ["alpha", "beta", "gamma"] {
            let skill = home.join(".agents/skills").join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(skill.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
        }
        let file = |name: &str| home.join(".agents/skills").join(name).join("SKILL.md");
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::write(
            home.join(".codex/config.toml"),
            format!(
                "[[skills.config]]\npath = {:?}\nenabled = false\n\n[[skills.config]]\npath = {:?}\nenabled = false\n\n[[skills.config]]\npath = {:?}\nenabled = false\n\n[[skills.config]]\npath = {:?}\nenabled = true\n",
                file("alpha").display().to_string(),
                home.join(".agents/skills/beta").display().to_string(),
                file("gamma").display().to_string(),
                file("gamma").display().to_string(),
            ),
        )
        .unwrap();
        let pack = home.join(".codex/skills/pack");
        fs::create_dir_all(pack.join("inner")).unwrap();
        fs::create_dir_all(pack.join("node_modules/dep")).unwrap();
        fs::write(pack.join("SKILL.md"), "---\nname: pack\n---\n").unwrap();
        fs::write(pack.join("inner/SKILL.md"), "---\nname: inner\n---\n").unwrap();
        fs::write(pack.join("node_modules/dep/SKILL.md"), "x").unwrap();
        let codex = launch_scan(&launch_env(&home, None), "codex");
        let at = codex.skills.iter().position(|s| s.name == "pack").unwrap();
        // Codex lists a SKILL.md inside a skill's folder as a skill of its own.
        assert_eq!(codex.nested_files[at], [pack.join("inner/SKILL.md")]);
        assert!(!codex.truncated);
        // A nested SKILL.md that is a link could name a file elsewhere: the inventory is incomplete.
        fs::create_dir_all(pack.join("odd")).unwrap();
        std::os::unix::fs::symlink(file("alpha"), pack.join("odd/SKILL.md")).unwrap();
        assert!(launch_scan(&launch_env(&home, None), "codex").truncated);
        fs::remove_file(pack.join("odd/SKILL.md")).unwrap();
        // So is a folder that cannot be read.
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(pack.join("inner"), fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = launch_scan(&launch_env(&home, None), "codex").truncated;
        fs::set_permissions(pack.join("inner"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(unreadable);
        // A folder path is ignored, as Codex ignores it; the last entry for a file wins.
        assert_eq!(codex.base_off.iter().collect::<Vec<_>>(), ["alpha"]);
        // The order of the entries decides, whichever of a skill's paths each one names.
        let real = fs::canonicalize(file("beta")).unwrap();
        fs::write(
            home.join(".codex/config.toml"),
            format!(
                "[[skills.config]]\npath = {:?}\nenabled = false\n\n[[skills.config]]\npath = {:?}\nenabled = true\n",
                file("beta").display().to_string(),
                real.display().to_string(),
            ),
        )
        .unwrap();
        assert!(
            launch_scan(&launch_env(&home, None), "codex")
                .base_off
                .is_empty()
        );
        let alpha = codex
            .skills
            .iter()
            .position(|skill| skill.name == "alpha")
            .unwrap();
        assert_eq!(
            codex.skill_files[alpha].as_deref(),
            Some(file("alpha").as_path())
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_credential_word_in_prose_is_reviewed_and_does_not_block() {
        let home = home();
        let prose = home.join(".claude/skills/prose");
        fs::create_dir_all(&prose).unwrap();
        fs::write(
            prose.join("SKILL.md"),
            "---\ndescription: Design tokens\n---\npassword = \"hunter2hunter2\"\n",
        )
        .unwrap();
        let scan = scan_home(home.clone(), None);
        let found = find(&scan, "skill", "prose");
        assert_eq!(found.blocked, None);
        assert_eq!(found.content.len(), 1);
        assert!(
            found
                .review
                .iter()
                .any(|entry| entry.path == "SKILL.md" && entry.reason == "mentions_credential")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_symbolic_link_skill_is_blocked_and_never_followed() {
        let home = home();
        let outside = home.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("SKILL.md"), "---\ndescription: x\n---").unwrap();
        std::os::unix::fs::symlink(&outside, home.join(".claude/skills/linked")).unwrap();
        let scan = scan_home(home.clone(), None);
        let linked = find(&scan, "skill", "linked");
        assert_eq!(linked.blocked, Some("symbolic_link"));
        assert!(linked.content.is_empty());
        assert_eq!(linked.managed_by, None);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn agents_and_rules_are_single_files_with_their_own_kind_and_scope() {
        let home = home();
        fs::create_dir_all(home.join(".claude/agents")).unwrap();
        fs::write(
            home.join(".claude/agents/reviewer.md"),
            "---\nname: reviewer\ndescription: Reviews diffs\n---\nbody",
        )
        .unwrap();
        fs::create_dir_all(home.join(".claude/rules")).unwrap();
        fs::write(home.join(".claude/rules/style.md"), "Use two spaces.").unwrap();
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::write(home.join(".codex/AGENTS.md"), "Be brief.").unwrap();
        fs::write(home.join(".codex/config.toml"), "model = \"x\"").unwrap();
        let project = home.join("work/app");
        fs::create_dir_all(project.join(".claude/agents")).unwrap();
        fs::write(
            project.join(".claude/agents/local.md"),
            "---\ndescription: Local helper\n---",
        )
        .unwrap();
        fs::write(project.join("CLAUDE.md"), "Project rule.").unwrap();
        let scan = scan_home(home.clone(), Some(project));
        let reviewer = find(&scan, "agent", "reviewer");
        assert_eq!(reviewer.description, "Reviews diffs");
        assert_eq!(reviewer.source, "claude_user_agents");
        assert_eq!(reviewer.blocked, None);
        assert_eq!(find(&scan, "rule", "style").source, "claude_user_rules");
        assert_eq!(find(&scan, "rule", "AGENTS").source, "codex_user_rules");
        let local = find(&scan, "agent", "local");
        assert_eq!(
            (local.scope, local.source.as_str()),
            ("project", "claude_project_agents")
        );
        assert_eq!(find(&scan, "rule", "CLAUDE").scope, "project");
        assert!(
            !scan
                .candidates
                .iter()
                .any(|candidate| candidate.name == "config")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_symbolic_link_to_a_gsd_payload_is_a_member_of_that_composite() {
        let home = home();
        let payload = home.join(GSD_BASE).join("1.15.0");
        fs::create_dir_all(payload.join("skills/gsd-help")).unwrap();
        fs::write(
            payload.join("skills/gsd-help/SKILL.md"),
            "---\ndescription: Help\n---",
        )
        .unwrap();
        fs::write(
            payload.join("manifest.json"),
            format!(r#"{{"schema":"{GSD_SCHEMA}","artifact_count":1950}}"#),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            payload.join("skills/gsd-help"),
            home.join(".claude/skills/gsd-help"),
        )
        .unwrap();
        let scan = scan_home(home.clone(), None);
        let member = find(&scan, "skill", "gsd-help");
        assert_eq!(member.blocked, Some("symbolic_link"));
        assert_eq!(
            member.managed_by,
            Some(ManagedBy {
                composite: "GSD Core",
                version: "1.15.0".into(),
                artifacts: 1950
            })
        );
        assert_eq!(scan.composites.len(), 1);
        assert_eq!(scan.composites[0].members, 1);
        assert_eq!(scan.composites[0].name, "GSD Core 1.15.0");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_link_into_something_that_is_not_a_payload_is_not_a_composite() {
        let home = home();
        let outside = home.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(
            outside.join("manifest.json"),
            format!(r#"{{"schema":"{GSD_SCHEMA}","artifact_count":3}}"#),
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, home.join(".claude/skills/fake")).unwrap();
        let scan = scan_home(home.clone(), None);
        assert_eq!(find(&scan, "skill", "fake").managed_by, None);
        assert!(scan.composites.is_empty());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_private_key_blocks_and_a_design_token_file_name_does_not() {
        assert!(credential_value(&format!(
            "-----BEGIN {} PRIVATE KEY-----\nabc\n-----END {} PRIVATE KEY-----",
            "OPENSSH", "OPENSSH"
        )));
        assert!(credential_value(&format!(
            "curl https://{}:{}@example.com/x",
            "deploy", "hunter2"
        )));
        assert!(!credential_value("tokens: spacing 8px, radius 10px"));
        assert!(!credential_name("tokens.md"));
        assert!(credential_name("credentials.json"));
    }

    #[test]
    fn names_outside_the_grammar_are_not_capabilities() {
        assert!(name_ok("design-system.v2"));
        assert!(!name_ok(".hidden"));
        assert!(!name_ok("with space"));
        assert!(!name_ok(&"a".repeat(65)));
    }

    fn entry_home(home: PathBuf, only: Only<'_>) -> Scan {
        scan_with(
            &Environment {
                home,
                claude_config_dir: None,
                codex_home: None,
                project: None,
                system: PathBuf::from("/etc"),
                declared_writers: Vec::new(),
            },
            Some(only),
        )
    }

    fn paths(candidate: &Candidate) -> Vec<&str> {
        candidate
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect()
    }

    #[test]
    fn a_scan_of_one_entry_lists_that_entry_as_the_full_scan_does() {
        let home = home();
        for name in ["design", "brief", "tokens"] {
            let skill = home.join(".claude/skills").join(name);
            fs::create_dir_all(skill.join("refs")).unwrap();
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\n---\nbody"),
            )
            .unwrap();
            fs::write(skill.join("refs/notes.md"), "notes").unwrap();
        }
        let full = scan_home(home.clone(), None);
        let one = entry_home(
            home.clone(),
            Only {
                kind: "skill",
                source: "claude_user_skills",
                name: "brief",
            },
        );
        let expected = find(&full, "skill", "brief");
        assert_eq!(one.candidates.len(), 1);
        assert_eq!(one.candidates[0].name, "brief");
        assert_eq!(one.candidates[0].blocked, expected.blocked);
        assert_eq!(paths(&one.candidates[0]), paths(expected));
        assert!(one.composites.is_empty());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn an_entry_past_the_listing_limit_is_not_listed_by_a_partial_scan_either() {
        let home = home();
        let names: Vec<String> = (0..MAX_ENTRIES + 4)
            .map(|index| format!("skill-{index:03}"))
            .collect();
        for name in &names {
            let skill = home.join(".claude/skills").join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(skill.join("SKILL.md"), "---\nname: x\n---\nbody").unwrap();
        }
        let full = scan_home(home.clone(), None);
        assert!(full.truncated);
        let cut = names
            .iter()
            .find(|name| {
                full.candidates
                    .iter()
                    .all(|candidate| candidate.name != **name)
            })
            .unwrap();
        let one = entry_home(
            home.clone(),
            Only {
                kind: "skill",
                source: "claude_user_skills",
                name: cut,
            },
        );
        assert!(one.candidates.is_empty());
        assert!(one.truncated);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_tree_past_the_file_limit_stops_being_read_and_is_blocked() {
        let home = home();
        let skill = home.join(".claude/skills/huge");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "---\nname: huge\n---\nbody").unwrap();
        for index in 0..(MAX_FILES + 100) {
            fs::write(skill.join(format!("f{index:04}.md")), "x").unwrap();
        }
        let scan = scan_home(home.clone(), None);
        let found = find(&scan, "skill", "huge");
        assert_eq!(found.blocked, Some("too_many_files"));
        // Reading stops at the first entry past the limit: the rest of the folder is never opened.
        assert_eq!(found.files.len(), MAX_FILES + 1);
        fs::remove_dir_all(home).unwrap();
    }

    fn folder_of(scope: &'static str, path: &str) -> Folder {
        Folder {
            kind: "rule",
            provider: "claude_code",
            source: "claude_project_rules",
            scope,
            path: PathBuf::from(path),
            shape: Shape::Named("CLAUDE.md"),
        }
    }

    /// One rule file read through a folder another account can write, or not.
    fn read_rule(shared: bool) -> Tree {
        Tree {
            files: vec![("CLAUDE.md".to_owned(), b"Use the house style.".to_vec())],
            shared,
            ..Tree::default()
        }
    }

    #[test]
    fn a_project_read_through_a_shared_folder_is_listed_but_never_copied() {
        let project = folder_of("project", "/home/u/repo");
        let shared = assemble(
            &project,
            "CLAUDE",
            "~/repo/CLAUDE.md".to_owned(),
            String::new(),
            read_rule(true),
        );
        assert_eq!(shared.blocked, Some("shared_path"));
        // Still listed, with its file, so the owner sees what the project holds.
        assert_eq!(shared.name, "CLAUDE");
        assert_eq!(shared.files.len(), 1);
        // Negative controls: the same file in a folder only this account can write is copied as usual,
        // and an account folder keeps its own policy when it is read through a shared path.
        let own = assemble(
            &project,
            "CLAUDE",
            "~/repo/CLAUDE.md".to_owned(),
            String::new(),
            read_rule(false),
        );
        assert_eq!(own.blocked, None);
        let account = folder_of("user", "/home/u/.claude/rules");
        let account_rule = assemble(
            &account,
            "CLAUDE",
            "~/.claude/rules/CLAUDE.md".to_owned(),
            String::new(),
            read_rule(true),
        );
        assert_eq!(account_rule.blocked, None);
    }

    #[test]
    fn a_reason_found_before_the_sharing_keeps_its_place() {
        let project = folder_of("project", "/home/u/repo");
        let mut tree = read_rule(true);
        tree.blocked = Some("needs_secret");
        let candidate = assemble(
            &project,
            "CLAUDE",
            "~/repo/CLAUDE.md".to_owned(),
            String::new(),
            tree,
        );
        assert_eq!(candidate.blocked, Some("needs_secret"));
    }

    #[test]
    fn parallel_map_returns_its_results_in_the_order_of_its_items() {
        let items: Vec<u32> = (0..500).collect();
        let doubled = parallel_map(&items, |item| item * 2);
        assert_eq!(
            doubled,
            items.iter().map(|item| item * 2).collect::<Vec<_>>()
        );
    }
}
