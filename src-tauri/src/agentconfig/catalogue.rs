//! The capabilities the account and the open project keep, read for Arsenal: skill folders, agent
//! and rule files, the read-only entries of the inventory (hooks, MCP servers, plugins), and the
//! composite collections (GSD Core) that linked skills and agents are installed from. Each folder is
//! read through the same approved roots, bounded reads and link rules as the inventory. Nothing is
//! executed, nothing is written, and no link is followed out of the folder that holds it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::inventory::{self, Environment};
use super::model::{Bounds, ItemKind, ProviderId, Scope, SourceFailure};
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

/// A composite collection found through its members' links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Composite {
    pub name: String,
    pub version: String,
    pub artifacts: u64,
    /// How many of this scan's entries are installed from it.
    pub members: usize,
    /// Where its payload lives, with the home folder shown as `~`.
    pub origin: String,
}

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

fn scan(env: &Environment) -> Scan {
    let mut scan = Scan {
        sources: Vec::new(),
        candidates: Vec::new(),
        composites: Vec::new(),
        truncated: false,
    };
    let mut states: BTreeMap<&'static str, SourceState> = BTreeMap::new();
    let mut manifests = BTreeMap::new();
    for folder in folders(env) {
        let state = scan_folder(env, &folder, &mut scan, &mut manifests);
        let merged = states.entry(folder.source).or_insert(SourceState::Absent);
        *merged = merge(*merged, state);
    }
    scan.sources = states
        .into_iter()
        .map(|(source, state)| SourceReport { source, state })
        .collect();
    scan.candidates.extend(read_only_entries(env));
    scan.composites = composites(env, &scan.candidates);
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

fn scan_folder(
    env: &Environment,
    folder: &Folder,
    scan: &mut Scan,
    manifests: &mut BTreeMap<PathBuf, Option<ManagedBy>>,
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
        let origin = origin(&env.home, &folder.path, name);
        let link = folder.path.join(name);
        let candidate = match (entry.kind, folder.shape) {
            (EntryKind::Link, _) => {
                let mut candidate = blocked(folder, stem, origin, "symbolic_link");
                candidate.managed_by = composite_of(env, &link, manifests);
                candidate
            }
            (EntryKind::Directory, _) => read_skill(&reader, folder, stem, &link, origin),
            (EntryKind::File, _) => file_candidate(&reader, folder, stem, name, &link, origin),
            (EntryKind::Other, _) => continue,
        };
        scan.candidates.push(candidate);
    }
    SourceState::Read
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

/// A skill folder: every file under it is read, and a link anywhere under it is never followed.
fn read_skill(
    reader: &ConfigReader,
    folder: &Folder,
    name: &str,
    dir: &Path,
    origin: String,
) -> Candidate {
    let mut tree = Tree::default();
    let mut pending = vec![(dir.to_path_buf(), String::new(), 0_usize)];
    while let Some((current, prefix, depth)) = pending.pop() {
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
        blocked: tree.blocked,
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
        Ok(file) => file.bytes().to_vec(),
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
    manifests: &mut BTreeMap<PathBuf, Option<ManagedBy>>,
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
    if let Some(known) = manifests.get(&root) {
        return known.clone();
    }
    let found = gsd_manifest(&root, &version);
    manifests.insert(root, found.clone());
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

/// Composites found through this scan's links, each with how many of its links are here.
fn composites(env: &Environment, candidates: &[Candidate]) -> Vec<Composite> {
    let mut found: BTreeMap<(String, String), (u64, usize)> = BTreeMap::new();
    for managed in candidates
        .iter()
        .filter_map(|candidate| candidate.managed_by.as_ref())
    {
        let entry = found
            .entry((managed.composite.to_owned(), managed.version.clone()))
            .or_insert((managed.artifacts, 0));
        entry.1 += 1;
    }
    found
        .into_iter()
        .map(|((name, version), (artifacts, members))| Composite {
            origin: origin(&env.home, &env.home.join(GSD_BASE), &version),
            name: format!("{name} {version}"),
            version,
            artifacts,
            members,
        })
        .collect()
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
}
