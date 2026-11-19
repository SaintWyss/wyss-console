//! The Arsenal Guide: every skill and agent each tool reads, read from disk for the owner to
//! understand them. Unlike discovery (`catalogue`), a link is resolved here: a skill linked into
//! the account's folder is what the tool loads, so the Guide shows its text. A link is followed
//! only to a place inside the approved roots (the providers' folders and the GSD Core payload),
//! and what it reaches is read through the same bounded reader. Nothing is written or executed.
//!
//! Where each entry comes from is decided from its resolved place, never from its name alone:
//! under the GSD Core payload → GSD Core; a folder that is a Git checkout → Git repos; the account's
//! synced folder → the Claude account; a plugin's folder → that plugin; otherwise → the owner's own.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::catalogue::full_description_within;
use super::inventory::Environment;
use super::model::{Bounds, SourceFailure, TooLarge};
use super::reader::{ConfigReader, EntryKind};
use super::redact;
use super::roots::ApprovedRoots;

/// Longest description kept. The Guide splits it; a longer one is cut and says so.
pub const DESCRIPTION_CHARS: usize = 16_000;
/// How much of a file is read for its front matter.
const FRONT_MATTER_BYTES: usize = 64 * 1024;
/// Most entries one walk of a skill's folder visits (files and folders), and most bytes it hashes.
const WALK_ENTRIES: usize = 4096;
const DIGEST_BYTES: u64 = 16 * 1024 * 1024;
/// Largest instructions file shown. Past it, the text is cut and says so.
pub const BODY_BYTES: usize = 256 * 1024;
/// Most files one skill lists in its card; the rest are counted.
const MOST_FILES: usize = 256;
const MOST_DEPTH: usize = 6;
/// Most entries read from one folder.
const MOST_ENTRIES: usize = 1024;
/// Largest file taken into the digest that compares Claude's copy with Codex's.
const DIGEST_FILE_BYTES: u64 = 1024 * 1024;
const GSD_BASE: &str = ".local/share/wyss/gsd-core";
/// A skill's translation for the screen, beside its `SKILL.md`.
const TRANSLATION: &str = "SKILL.es.md";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GuideEntry {
    /// Stable for one scan: `<tool>/<kind>/<set>/<name>`.
    pub id: String,
    /// `claude_code` or `codex`.
    pub tool: &'static str,
    /// `skill` or `agent`.
    pub kind: &'static str,
    pub name: String,
    /// The whole description of its front matter (redacted, at most 4000 characters).
    pub description: String,
    /// The key of the set it belongs to: `mine` for the owner's own, `agents` for agents.
    pub set: String,
    /// Where the tool finds it, with the home shown as `~`.
    pub origin: String,
    /// Where its link leads, when it is one.
    pub linked_to: Option<String>,
    /// Which tools see a skill of this name in their own folders: `claude`, `codex` or `both`.
    /// `None` when it cannot be told from the folders (plugins, agents).
    pub scope: Option<&'static str>,
    /// Both tools have it, and Codex's copy is not the same as Claude's.
    pub codex_differs: bool,
    /// Why its text could not be read; `None` when it was.
    pub unreadable: Option<&'static str>,
    /// The description is longer than is kept: what is shown is its beginning.
    pub description_cut: bool,
    /// The description of a `SKILL.es.md` beside it: a translation shown on screen only. The
    /// tools read `SKILL.md` alone, so it changes nothing they do.
    pub description_es: Option<String>,
}

/// Where a group of entries comes from, for the Guide's "Sets".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GuideSet {
    pub key: String,
    pub tool: &'static str,
    /// `gsd`, `repos`, `plugin`, `account`, `bundle` or `system`.
    pub kind: &'static str,
    pub name: String,
    pub version: String,
    /// What it says it is, from its own manifest; empty when it says nothing.
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Guide {
    pub entries: Vec<GuideEntry>,
    pub sets: Vec<GuideSet>,
    /// A folder held more than is read: the lists are incomplete.
    pub truncated: bool,
}

/// The instructions of one entry and the names of its files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GuideBody {
    pub id: String,
    /// The file's text without its front matter, redacted.
    pub text: String,
    /// The text was longer than `BODY_BYTES` and was cut.
    pub cut: bool,
    pub files: Vec<String>,
    /// Files past `MOST_FILES`, counted and not named.
    pub more_files: usize,
    /// The folder holds more than the walk reads: `more_files` counts only what was walked.
    pub files_cut: bool,
    /// The text of a `SKILL.es.md` beside it, without its front matter: shown on screen only.
    pub text_es: Option<String>,
}

/// What a scan resolved for each entry, kept beside the reply so a later read names an entry by
/// its id and never by a path from the window.
#[derive(Debug, Clone, Default)]
pub struct Places(BTreeMap<String, Place>);

#[derive(Debug, Clone)]
struct Place {
    /// The resolved folder of a skill, or the resolved file of an agent.
    path: PathBuf,
    skill: bool,
}

/// Reads through the roots, after resolving a link to a place inside them.
struct Reader {
    reader: ConfigReader,
    roots: Vec<PathBuf>,
    home: PathBuf,
    gsd: Option<PathBuf>,
}

impl Reader {
    fn new(env: &Environment) -> Option<Self> {
        // The closed list: the providers' folders and the GSD Core payload. Where a link leads
        // never becomes a root; a folder linked out of these is not read and is reported.
        let wanted = [
            env.claude_dir(),
            env.home.join(".agents"),
            env.codex_dir(),
            env.home.join(GSD_BASE),
        ];
        let roots: Vec<PathBuf> = wanted
            .iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .collect();
        let reader =
            ConfigReader::new(ApprovedRoots::new(roots.clone()).ok()?, Bounds::PROPOSED).ok()?;
        Some(Self {
            reader,
            roots,
            home: env.home.clone(),
            gsd: std::fs::canonicalize(env.home.join(GSD_BASE)).ok(),
        })
    }

    /// The place a path leads to, when it is inside a root. A link out of them is refused.
    fn resolve(&self, path: &Path) -> Result<PathBuf, &'static str> {
        let real = std::fs::canonicalize(path).map_err(|_| "broken_link")?;
        if self.roots.iter().any(|root| real.starts_with(root)) {
            Ok(real)
        } else {
            Err("outside_roots")
        }
    }

    /// Reads a file at the place its path resolves to: the reader follows only a final link, and a
    /// folder on the way may be one (`~/.claude/plugins` linked elsewhere).
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        let real = self.resolve(path).ok()?;
        self.reader
            .read_file(&real)
            .ok()
            .map(|read| read.bytes().to_vec())
    }

    /// The entries of a folder, without hidden ones, sorted. `None` when it cannot be listed.
    fn list(&self, path: &Path) -> Option<(Vec<(String, EntryKind)>, bool)> {
        let real = self.resolve(path).ok()?;
        let listing = match self.reader.list_directory(&real) {
            Ok(listing) => listing,
            // More entries than the reader lists: nothing of it is shown, and it says so.
            Err(SourceFailure::TooLarge {
                bound: TooLarge::Listing,
            }) => return Some((Vec::new(), true)),
            Err(_) => return None,
        };
        let mut cut = false;
        let mut found = Vec::new();
        for entry in listing.entries {
            let Some(name) = entry.name.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            if found.len() >= MOST_ENTRIES {
                cut = true;
                break;
            }
            found.push((name.to_owned(), entry.kind));
        }
        Some((found, cut))
    }

    fn shown(&self, path: &Path) -> String {
        path.strip_prefix(&self.home).map_or_else(
            |_| path.display().to_string(),
            |rest| format!("~/{}", rest.display()),
        )
    }
}

/// The text redacted and cut to `DESCRIPTION_CHARS`, and whether it was cut.
fn clean(text: &str) -> (String, bool) {
    let shown = redact::text(text);
    let all = shown.text.as_str();
    let kept: String = all.chars().take(DESCRIPTION_CHARS).collect();
    let cut = kept.len() < all.len();
    (kept, cut)
}

/// The description of a file and whether it is longer than is read or kept: a front matter still
/// open at the end of what is read is cut too.
fn description_of(bytes: &[u8]) -> (String, bool) {
    let (text, cut) = clean(&full_description_within(bytes, FRONT_MATTER_BYTES));
    let head = &bytes[..bytes.len().min(FRONT_MATTER_BYTES)];
    let open = bytes.len() > FRONT_MATTER_BYTES
        && head.starts_with(b"---\n")
        && !head[4..].windows(4).any(|window| window == b"\n---");
    (text, cut || open)
}

/// Every skill and agent of both tools, and the sets they come from.
pub fn scan(env: &Environment) -> (Guide, Places) {
    let mut guide = Guide::default();
    let mut places = Places::default();
    let Some(reader) = Reader::new(env) else {
        return (guide, places);
    };
    let mut scan = Scan {
        reader: &reader,
        guide: &mut guide,
        places: &mut places,
        sets: BTreeMap::new(),
        plugin_dirs: Vec::new(),
    };

    let claude_skills = env.claude_dir().join("skills");
    let agents_skills = env.agents_skills_dir();
    let claude_names = names(&reader, &claude_skills);
    let codex_names = names(&reader, &agents_skills);

    // Plugins first: the account's folders are then classified against where they live.
    scan.claude_plugins(env);
    scan.codex_plugins(env);
    scan.user_skills("claude_code", &claude_skills, &codex_names);
    scan.user_skills("codex", &agents_skills, &claude_names);
    scan.user_skills("codex", &env.codex_dir().join("skills"), &BTreeSet::new());
    scan.system_skills(&env.codex_dir().join("skills/.system"));
    scan.agents("claude_code", &env.claude_dir().join("agents"), "md");
    scan.agents("codex", &env.codex_dir().join("agents"), "toml");

    // Whether Codex's copy of a skill both tools hold is the same as Claude's.
    let digests: BTreeMap<(String, &'static str), Option<String>> = scan
        .guide
        .entries
        .iter()
        .filter(|entry| {
            entry.scope == Some("both") && entry.set != "gsd" && !entry.set.starts_with("gsd:")
        })
        .filter_map(|entry| {
            let place = scan.places.0.get(&entry.id)?;
            Some((
                (entry.name.clone(), entry.tool),
                tree_digest(&reader, &place.path),
            ))
        })
        .collect();
    for entry in &mut scan.guide.entries {
        if entry.scope != Some("both") {
            continue;
        }
        let mine = digests.get(&(entry.name.clone(), "claude_code"));
        let theirs = digests.get(&(entry.name.clone(), "codex"));
        if let (Some(Some(mine)), Some(Some(theirs))) = (mine, theirs) {
            entry.codex_differs = mine != theirs;
        }
    }
    let sets = std::mem::take(&mut scan.sets);
    guide.sets = sets.into_values().collect();
    (guide, places)
}

/// The names in a skills folder that hold a skill: the ones each tool would list.
fn names(reader: &Reader, folder: &Path) -> BTreeSet<String> {
    reader
        .list(folder)
        .map(|(entries, _)| {
            entries
                .into_iter()
                .filter(|(_, kind)| matches!(kind, EntryKind::Directory | EntryKind::Link))
                .map(|(name, _)| name)
                .collect()
        })
        .unwrap_or_default()
}

struct Scan<'a> {
    reader: &'a Reader,
    guide: &'a mut Guide,
    places: &'a mut Places,
    sets: BTreeMap<String, GuideSet>,
    /// The resolved folder of each plugin read, with its set: a skill of the account's folder that
    /// resolves inside one belongs to that plugin.
    plugin_dirs: Vec<(PathBuf, String)>,
}

impl Scan<'_> {
    /// A folder's entries. One that is there but cannot be read (a link out of the roots, a
    /// folder the reader refuses) leaves the lists incomplete, and the Guide says so.
    fn listed(&mut self, folder: &Path) -> Option<(Vec<(String, EntryKind)>, bool)> {
        let found = self.reader.list(folder);
        if found.is_none() && std::fs::symlink_metadata(folder).is_ok() {
            self.guide.truncated = true;
        }
        found
    }

    fn set(
        &mut self,
        key: &str,
        tool: &'static str,
        kind: &'static str,
        name: &str,
        version: &str,
        description: &str,
    ) {
        self.sets.entry(key.to_owned()).or_insert_with(|| GuideSet {
            key: key.to_owned(),
            tool,
            kind,
            name: name.to_owned(),
            version: version.to_owned(),
            description: clean(description).0,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        tool: &'static str,
        kind: &'static str,
        set: String,
        name: &str,
        origin: &Path,
        resolved: Result<PathBuf, &'static str>,
        scope: Option<&'static str>,
    ) {
        let linked = std::fs::symlink_metadata(origin)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false);
        let mut id = format!("{tool}/{kind}/{set}/{name}");
        let mut n = 2;
        while self.places.0.contains_key(&id) {
            id = format!("{tool}/{kind}/{set}/{name}#{n}");
            n += 1;
        }
        let ((description, description_cut), unreadable) = match &resolved {
            Ok(path) => {
                let file = if kind == "skill" {
                    path.join("SKILL.md")
                } else {
                    path.clone()
                };
                match self.reader.read(&file) {
                    Some(bytes) => (description_of(&bytes), None),
                    None if kind == "skill" && !file.exists() => {
                        ((String::new(), false), Some("no_skill_file"))
                    }
                    None => ((String::new(), false), Some("unreadable")),
                }
            }
            Err(reason) => ((String::new(), false), Some(*reason)),
        };
        let description_es = match (&resolved, kind) {
            (Ok(path), "skill") => self
                .reader
                .read(&path.join(TRANSLATION))
                .map(|bytes| description_of(&bytes).0)
                .filter(|d| !d.is_empty()),
            _ => None,
        };
        if let Ok(path) = &resolved {
            self.places.0.insert(
                id.clone(),
                Place {
                    path: path.clone(),
                    skill: kind == "skill",
                },
            );
        } else {
            // An entry that cannot be read is listed, never opened: no place is kept.
            self.places.0.insert(
                id.clone(),
                Place {
                    path: PathBuf::new(),
                    skill: false,
                },
            );
        }
        self.guide.entries.push(GuideEntry {
            id,
            tool,
            kind,
            name: name.to_owned(),
            description,
            set,
            origin: self.reader.shown(origin),
            linked_to: match (&resolved, linked) {
                (Ok(path), true) => Some(self.reader.shown(path)),
                _ => None,
            },
            scope,
            codex_differs: false,
            unreadable,
            description_cut,
            description_es,
        });
    }

    /// An account's skills folder: each entry, classified by where it resolves.
    fn user_skills(&mut self, tool: &'static str, folder: &Path, other: &BTreeSet<String>) {
        let Some((entries, cut)) = self.listed(folder) else {
            return;
        };
        self.guide.truncated |= cut;
        let own_folder = folder.ends_with(".claude/skills") || folder.ends_with(".agents/skills");
        for (name, kind) in entries {
            if !matches!(kind, EntryKind::Directory | EntryKind::Link) {
                continue;
            }
            let origin = folder.join(&name);
            let resolved = self.reader.resolve(&origin);
            let scope = if !own_folder {
                Some("codex")
            } else if other.contains(&name) {
                Some("both")
            } else if tool == "claude_code" {
                Some("claude")
            } else {
                Some("codex")
            };
            let set = match &resolved {
                Ok(path) => self.classify(tool, &name, path),
                Err(_) => "mine".to_owned(),
            };
            if set == "account" {
                if let Ok(path) = &resolved {
                    self.account_skills(path);
                }
                continue;
            }
            self.push(tool, "skill", set, &name, &origin, resolved, scope);
        }
    }

    fn classify(&mut self, tool: &'static str, name: &str, path: &Path) -> String {
        if let Some(version) = self
            .reader
            .gsd
            .as_ref()
            .and_then(|gsd| path.strip_prefix(gsd).ok())
            .and_then(|rest| rest.components().next())
            .and_then(|first| first.as_os_str().to_str())
        {
            let key = format!("gsd:{tool}");
            self.set(&key, tool, "gsd", "GSD Core", version, "");
            return key;
        }
        if let Some((_, key)) = self
            .plugin_dirs
            .iter()
            .find(|(dir, key)| path.starts_with(dir) && key.contains(&format!(":{tool}:")))
        {
            return key.clone();
        }
        if path.join(".git").exists() {
            let key = format!("repos:{tool}");
            self.set(&key, tool, "repos", "Git repos", "", "");
            return key;
        }
        if tool == "claude_code" && name == "synced" && !path.join("SKILL.md").exists() {
            return "account".to_owned();
        }
        if tool == "codex" && name.starts_with("gstack-") {
            let key = "bundle:codex:gstack".to_owned();
            self.set(&key, tool, "bundle", "gstack", "", "");
            return key;
        }
        if tool == "codex" && name.contains("hyperframes") {
            let key = "bundle:codex:hyperframes".to_owned();
            self.set(&key, tool, "bundle", "HyperFrames", "", "");
            return key;
        }
        "mine".to_owned()
    }

    /// The Claude account's skills, synced into `skills/synced/<account>/<skill>`.
    fn account_skills(&mut self, synced: &Path) {
        let Some((accounts, _)) = self.reader.list(synced) else {
            return;
        };
        let key = "account:claude_code".to_owned();
        for (account, kind) in accounts {
            if kind != EntryKind::Directory {
                continue;
            }
            let folder = synced.join(&account);
            let Some((skills, cut)) = self.reader.list(&folder) else {
                continue;
            };
            self.guide.truncated |= cut;
            for (name, kind) in skills {
                if kind == EntryKind::Directory {
                    self.set(&key, "claude_code", "account", "Claude account", "", "");
                    self.directory_skill("claude_code", &key, &folder, &name);
                }
            }
        }
    }

    fn codex_system_set(&mut self) -> String {
        let key = "system:codex".to_owned();
        self.set(&key, "codex", "system", "Included with Codex", "", "");
        key
    }

    fn system_skills(&mut self, folder: &Path) {
        let Some((entries, _)) = self.reader.list(folder) else {
            return;
        };
        let key = self.codex_system_set();
        for (name, kind) in entries {
            if kind != EntryKind::Directory {
                continue;
            }
            self.directory_skill("codex", &key, folder, &name);
        }
    }

    /// The skills of one plugin folder (`<root>/skills/<name>/SKILL.md`).
    fn plugin_skills(&mut self, tool: &'static str, key: &str, root: &Path) {
        let skills = root.join("skills");
        let Some((entries, cut)) = self.reader.list(&skills) else {
            return;
        };
        self.guide.truncated |= cut;
        for (name, kind) in entries {
            if kind != EntryKind::Directory {
                continue;
            }
            self.directory_skill(tool, key, &skills, &name);
        }
    }

    fn directory_skill(&mut self, tool: &'static str, key: &str, folder: &Path, name: &str) {
        let origin = folder.join(name);
        let resolved = self.reader.resolve(&origin);
        self.push(
            tool,
            "skill",
            key.to_owned(),
            name,
            &origin,
            resolved,
            Some(if tool == "codex" { "codex" } else { "claude" }),
        );
    }

    /// A plugin's name, version and description from its own manifest.
    fn manifest(&self, root: &Path, dir: &str) -> Option<(String, String, String)> {
        let bytes = self.reader.read(&root.join(dir).join("plugin.json"))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
        Some((text("name"), text("version"), text("description")))
    }

    fn claude_plugins(&mut self, env: &Environment) {
        let plugins = env.claude_dir().join("plugins");
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(record) = self.reader.read(&plugins.join("installed_plugins.json"))
            && let Ok(record) = serde_json::from_slice::<serde_json::Value>(&record)
            && let Some(entries) = record["plugins"].as_object()
        {
            for installs in entries.values() {
                if let Some(path) = installs
                    .as_array()
                    .and_then(|all| all.last())
                    .and_then(|install| install["installPath"].as_str())
                {
                    roots.push(PathBuf::from(path));
                }
            }
        }
        // The plugins of the Claude account, synced under `plugins/synced/<account>/<plugin>`.
        let synced = plugins.join("synced");
        if let Some((accounts, _)) = self.reader.list(&synced) {
            for (account, kind) in accounts {
                if kind != EntryKind::Directory {
                    continue;
                }
                if let Some((found, _)) = self.reader.list(&synced.join(&account)) {
                    for (plugin, kind) in found {
                        if kind == EntryKind::Directory {
                            roots.push(synced.join(&account).join(plugin));
                        }
                    }
                }
            }
        }
        for root in roots {
            let Ok(real) = self.reader.resolve(&root) else {
                continue;
            };
            let Some((name, version, description)) = self.manifest(&real, ".claude-plugin") else {
                continue;
            };
            if name.is_empty() || !real.join("skills").is_dir() {
                continue;
            }
            let key = format!("plugin:claude_code:{name}");
            self.set(&key, "claude_code", "plugin", &name, &version, &description);
            self.plugin_dirs.push((real.clone(), key.clone()));
            self.plugin_skills("claude_code", &key, &real);
        }
    }

    /// Codex's plugins: the newest cached version of each `<marketplace>/<plugin>`.
    fn codex_plugins(&mut self, env: &Environment) {
        let cache = env.codex_dir().join("plugins/cache");
        let Some((markets, _)) = self.reader.list(&cache) else {
            return;
        };
        for (market, kind) in markets {
            if kind != EntryKind::Directory {
                continue;
            }
            let Some((plugins, _)) = self.reader.list(&cache.join(&market)) else {
                continue;
            };
            for (plugin, kind) in plugins {
                if kind != EntryKind::Directory {
                    continue;
                }
                let folder = cache.join(&market).join(&plugin);
                let Some((versions, _)) = self.reader.list(&folder) else {
                    continue;
                };
                let Some((version, _)) = versions
                    .into_iter()
                    .rfind(|(_, kind)| *kind == EntryKind::Directory)
                else {
                    continue;
                };
                let Ok(real) = self.reader.resolve(&folder.join(&version)) else {
                    continue;
                };
                if !real.join("skills").is_dir() {
                    continue;
                }
                let (name, shown_version, description) = self
                    .manifest(&real, ".codex-plugin")
                    .unwrap_or_else(|| (plugin.clone(), version.clone(), String::new()));
                let name = if name.is_empty() {
                    plugin.clone()
                } else {
                    name
                };
                let key = format!("plugin:codex:{name}");
                self.set(&key, "codex", "plugin", &name, &shown_version, &description);
                self.plugin_dirs.push((real.clone(), key.clone()));
                self.plugin_skills("codex", &key, &real);
            }
        }
    }

    fn agents(&mut self, tool: &'static str, folder: &Path, extension: &str) {
        let Some((entries, cut)) = self.listed(folder) else {
            return;
        };
        self.guide.truncated |= cut;
        for (file, kind) in entries {
            if !matches!(kind, EntryKind::File | EntryKind::Link) {
                continue;
            }
            let Some(name) = file.strip_suffix(&format!(".{extension}")) else {
                continue;
            };
            let origin = folder.join(&file);
            let resolved = self.reader.resolve(&origin);
            self.push(
                tool,
                "agent",
                "agents".to_owned(),
                name,
                &origin,
                resolved,
                None,
            );
        }
    }
}

/// A digest of a skill's files (relative names and contents), to tell whether two copies match.
/// `None` when the folder could not be read whole within the walk's budget.
fn tree_digest(reader: &Reader, root: &Path) -> Option<String> {
    let mut hash = Sha256::new();
    let mut walk = Walk::new(usize::MAX);
    walk.folder(reader, root, "", 0)?;
    if walk.exhausted {
        return None;
    }
    let mut files = walk.found;
    files.sort();
    let mut hashed = 0_u64;
    for relative in files {
        // The screen's translation is not the skill: it never makes two copies differ.
        if relative == TRANSLATION {
            continue;
        }
        let path = root.join(&relative);
        let size = std::fs::metadata(&path).ok()?.len();
        if size > DIGEST_FILE_BYTES {
            hash.update(relative.as_bytes());
            hash.update(size.to_le_bytes());
            continue;
        }
        hashed += size;
        if hashed > DIGEST_BYTES {
            return None;
        }
        let bytes = reader.read(&path)?;
        hash.update(relative.as_bytes());
        hash.update([0]);
        hash.update(&bytes);
    }
    Some(format!("{:x}", hash.finalize()))
}

/// One walk of a skill's folder: its files as relative names, without hidden ones and without
/// following links. It names at most `limit` files and counts the rest, and visits at most
/// `WALK_ENTRIES` entries in all; past that it stops and is `exhausted`.
struct Walk {
    found: Vec<String>,
    more: usize,
    limit: usize,
    budget: usize,
    exhausted: bool,
}

impl Walk {
    fn new(limit: usize) -> Self {
        Self {
            found: Vec::new(),
            more: 0,
            limit,
            budget: WALK_ENTRIES,
            exhausted: false,
        }
    }

    fn folder(&mut self, reader: &Reader, root: &Path, prefix: &str, depth: usize) -> Option<()> {
        let folder = if prefix.is_empty() {
            root.to_path_buf()
        } else {
            root.join(prefix)
        };
        let (entries, cut) = reader.list(&folder)?;
        if cut {
            self.exhausted = true;
        }
        for (name, kind) in entries {
            if self.budget == 0 {
                self.exhausted = true;
                return Some(());
            }
            self.budget -= 1;
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            match kind {
                EntryKind::File if self.found.len() < self.limit => self.found.push(relative),
                EntryKind::File => self.more += 1,
                EntryKind::Directory if depth < MOST_DEPTH => {
                    // A subfolder that cannot be read is left out of the names.
                    let _ = self.folder(reader, root, &relative, depth + 1);
                }
                _ => {}
            }
        }
        Some(())
    }
}

/// The text after a file's front matter.
fn without_front_matter(text: &str) -> &str {
    text.strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---").map(|(_, body)| body))
        .map_or(text, |body| {
            body.strip_prefix('\n')
                .unwrap_or(body)
                .trim_start_matches('\n')
        })
}

/// The instructions and files of the entry with this id, from the last scan. `None` when the id
/// is unknown or its place cannot be read now.
pub fn body(env: &Environment, places: &Places, id: &str) -> Option<GuideBody> {
    let place = places.0.get(id)?;
    if place.path.as_os_str().is_empty() {
        return None;
    }
    let reader = Reader::new(env)?;
    // The place was resolved inside the roots when scanned; it is checked again now.
    let real = reader.resolve(&place.path).ok()?;
    let file = if place.skill {
        real.join("SKILL.md")
    } else {
        real.clone()
    };
    let bytes = reader.read(&file)?;
    let cut = bytes.len() > BODY_BYTES;
    let head = &bytes[..bytes.len().min(BODY_BYTES)];
    let text = std::str::from_utf8(head)
        .or_else(|error| std::str::from_utf8(&head[..error.valid_up_to()]))
        .ok()?;
    let text = redact::text(without_front_matter(text))
        .text
        .as_str()
        .to_owned();
    let text_es = if place.skill {
        reader.read(&real.join(TRANSLATION)).and_then(|bytes| {
            let head = &bytes[..bytes.len().min(BODY_BYTES)];
            let text = std::str::from_utf8(head)
                .or_else(|error| std::str::from_utf8(&head[..error.valid_up_to()]))
                .ok()?;
            Some(
                redact::text(without_front_matter(text))
                    .text
                    .as_str()
                    .to_owned(),
            )
        })
    } else {
        None
    };
    let mut walk = Walk::new(MOST_FILES);
    if place.skill {
        let _ = walk.folder(&reader, &real, "", 0);
    }
    let (mut files, more_files, files_cut) = (walk.found, walk.more, walk.exhausted);
    files.sort();
    Some(GuideBody {
        id: id.to_owned(),
        text,
        cut,
        files,
        more_files,
        files_cut,
        text_es,
    })
}

/// The Guide of this process's account.
pub fn scan_process() -> Option<(Guide, Places)> {
    Environment::from_process(None).map(|env| scan(&env))
}

pub fn body_process(places: &Places, id: &str) -> Option<GuideBody> {
    Environment::from_process(None).and_then(|env| body(&env, places, id))
}

#[cfg(test)]
#[path = "guide_tests.rs"]
mod tests;
