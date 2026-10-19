//! What the adapters produce and how they produce it: sources that were read (or why not),
//! and the items found in them (06 §4, 07 §3).
//!
//! Nothing here holds a path or a byte of a file. A source is named by a closed key, an
//! item by a hash of where it was found, and every text an item carries went through
//! `redact` (C-07). The inventory is built from one `ConfigReader` per provider, so a
//! provider that fails leaves the other untouched (NFR-REL-03).
use super::model::{
    Bounds, ContentHash, Effective, ItemKind, NotWritable, PathTrust, Persisted, ProviderId, Scope,
    SessionUse, SourceClass, SourceFailure, writability,
};
use super::reader::{ConfigReader, EntryKind, Listing, ReadFile, parse_json, parse_toml};
use super::redact::{self, Projection, Shown};
use super::roots::InvalidRoot;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Where the adapters look. Everything else is derived from this, so a test can point
/// the whole engine at a temporary directory.
#[derive(Debug, Clone)]
pub struct Environment {
    pub home: PathBuf,
    /// `CLAUDE_CONFIG_DIR`, if the Console's own environment sets it.
    pub claude_config_dir: Option<PathBuf>,
    /// `CODEX_HOME`, if the Console's own environment sets it.
    pub codex_home: Option<PathBuf>,
    /// The folder of the project the user selected, if any.
    pub project: Option<PathBuf>,
    /// Where the systems' policy lives, `/etc`.
    pub system: PathBuf,
}

impl Environment {
    /// The environment of this process. `None` when `HOME` is not an absolute path.
    pub fn from_process(project: Option<PathBuf>) -> Option<Self> {
        let absolute = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        Some(Self {
            home: absolute("HOME")?,
            claude_config_dir: absolute("CLAUDE_CONFIG_DIR"),
            codex_home: absolute("CODEX_HOME"),
            project,
            system: PathBuf::from("/etc"),
        })
    }

    pub(super) fn claude_dir(&self) -> PathBuf {
        self.claude_config_dir
            .clone()
            .unwrap_or_else(|| self.home.join(".claude"))
    }

    pub(super) fn codex_dir(&self) -> PathBuf {
        self.codex_home
            .clone()
            .unwrap_or_else(|| self.home.join(".codex"))
    }
}

/// The closed list of places an adapter reads. It is what the page is told a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKey {
    ClaudeManagedSettings,
    ClaudeUserSettings,
    ClaudeUserState,
    ClaudeUserSkills,
    ClaudeUserRules,
    ClaudeProjectSettings,
    ClaudeProjectLocalSettings,
    ClaudeProjectMcp,
    ClaudeProjectSkills,
    ClaudeProjectRules,
    CodexSystemRequirements,
    CodexSystemManaged,
    CodexUserConfig,
    CodexUserHooks,
    CodexUserRules,
    CodexProjectConfig,
    CodexProjectRules,
}

impl SourceKey {
    pub const fn provider(self) -> ProviderId {
        match self {
            Self::ClaudeManagedSettings
            | Self::ClaudeUserSettings
            | Self::ClaudeUserState
            | Self::ClaudeUserSkills
            | Self::ClaudeUserRules
            | Self::ClaudeProjectSettings
            | Self::ClaudeProjectLocalSettings
            | Self::ClaudeProjectMcp
            | Self::ClaudeProjectSkills
            | Self::ClaudeProjectRules => ProviderId::ClaudeCode,
            _ => ProviderId::Codex,
        }
    }

    pub const fn scope(self) -> Scope {
        match self {
            Self::ClaudeManagedSettings
            | Self::CodexSystemRequirements
            | Self::CodexSystemManaged => Scope::System,
            Self::ClaudeProjectSettings
            | Self::ClaudeProjectLocalSettings
            | Self::ClaudeProjectMcp
            | Self::ClaudeProjectSkills
            | Self::ClaudeProjectRules
            | Self::CodexProjectConfig
            | Self::CodexProjectRules => Scope::Project,
            _ => Scope::Global,
        }
    }

    pub const fn class(self) -> SourceClass {
        match self.scope() {
            Scope::System => SourceClass::System,
            Scope::Project => SourceClass::Project,
            Scope::Global => match self {
                Self::ClaudeUserState => SourceClass::ProviderOwned,
                _ => SourceClass::User,
            },
        }
    }
}

/// What reading one source came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Outcome {
    /// Read and understood. `unrecognised` counts the entries in the parts the adapter
    /// reads that it could not place; they are kept in the file and never interpreted.
    Read {
        hash: ContentHash,
        trust: PathTrust,
        followed_links: usize,
        unrecognised: u32,
    },
    /// There is nothing at that place. Not an error: most sources are optional.
    Absent,
    /// A project's own Codex configuration, which Codex loads only for a trusted project.
    ProjectNotTrusted,
    Failed {
        failure: SourceFailure,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceReport {
    pub key: SourceKey,
    pub provider: ProviderId,
    pub scope: Scope,
    pub class: SourceClass,
    pub not_writable: NotWritable,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// One thing the configuration holds: a hook, a skill, a server, a rule file, a model
/// preference. `detail` is a redacted projection, never the raw value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub id: String,
    pub provider: ProviderId,
    pub source: SourceKey,
    pub scope: Scope,
    pub kind: ItemKind,
    pub name: Shown,
    pub persisted: Persisted,
    pub effective: Effective,
    pub session_use: SessionUse,
    pub not_writable: NotWritable,
    pub detail: Option<Projection>,
    /// For a file item, the digest of the file: the marker a watcher compares.
    pub hash: Option<ContentHash>,
}

/// The whole read: what was looked at and what was found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Inventory {
    pub sources: Vec<SourceReport>,
    pub items: Vec<Item>,
    /// More items than the bound allows were found; the rest were not kept.
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryError {
    InvalidBounds,
    InvalidRoot,
}

impl From<InvalidRoot> for InventoryError {
    fn from(_: InvalidRoot) -> Self {
        Self::InvalidRoot
    }
}

/// Reads the configuration of Claude Code and Codex. Writes nothing and starts nothing.
pub fn inventory(env: &Environment, bounds: Bounds) -> Result<Inventory, InventoryError> {
    let bounds = bounds
        .validated()
        .map_err(|_| InventoryError::InvalidBounds)?;
    let mut sink = Sink::new(bounds);
    let claude = ConfigReader::new(super::claude::roots(env)?, bounds)
        .map_err(|_| InventoryError::InvalidBounds)?;
    super::claude::read(&claude, env, &mut sink);
    let codex = ConfigReader::new(super::codex::roots(env)?, bounds)
        .map_err(|_| InventoryError::InvalidBounds)?;
    super::codex::read(&codex, env, &mut sink);
    Ok(sink.finish())
}

/// What an adapter reports into. It keeps the bound on items and the order of sources.
pub(super) struct Sink {
    bounds: Bounds,
    sources: Vec<SourceReport>,
    items: Vec<Item>,
    truncated: bool,
}

impl Sink {
    pub(super) fn new(bounds: Bounds) -> Self {
        Self {
            bounds,
            sources: Vec::new(),
            items: Vec::new(),
            truncated: false,
        }
    }

    pub(super) fn bounds(&self) -> &Bounds {
        &self.bounds
    }

    pub(super) fn finish(mut self) -> Inventory {
        self.sources.sort_by_key(|source| source.key);
        self.items
            .sort_by(|a, b| (a.source, &a.id).cmp(&(b.source, &b.id)));
        Inventory {
            sources: self.sources,
            items: self.items,
            truncated: self.truncated,
        }
    }

    pub(super) fn report(&mut self, key: SourceKey, trust: PathTrust, outcome: Outcome) {
        self.sources.push(SourceReport {
            key,
            provider: key.provider(),
            scope: key.scope(),
            class: key.class(),
            not_writable: writability(key.class(), trust),
            outcome,
        });
    }

    fn failed(&mut self, key: SourceKey, failure: SourceFailure) {
        let outcome = if failure == SourceFailure::Missing {
            Outcome::Absent
        } else {
            Outcome::Failed { failure }
        };
        self.report(key, PathTrust::Trusted, outcome);
    }

    pub(super) fn not_trusted(&mut self, key: SourceKey) {
        self.report(key, PathTrust::Trusted, Outcome::ProjectNotTrusted);
    }

    /// Reads a JSON or TOML source and records how that went.
    pub(super) fn document(
        &mut self,
        reader: &ConfigReader,
        path: &Path,
        key: SourceKey,
        format: Format,
    ) -> Option<Document> {
        let file = match reader.read_file(path) {
            Ok(file) => file,
            Err(failure) => {
                self.failed(key, failure);
                return None;
            }
        };
        let parsed = match format {
            Format::Json => parse_json(file.bytes(), self.bounds()),
            Format::Toml => parse_toml(file.bytes(), self.bounds()),
        };
        match parsed {
            Ok(value) => Some(Document {
                key,
                trust: file.trust(),
                hash: file.hash(),
                followed_links: file.links().len(),
                value: value.unwrap_or(Value::Object(serde_json::Map::new())),
            }),
            Err(failure) => {
                self.failed(key, failure);
                None
            }
        }
    }

    /// Closes a document's report once its adapter knows how much it did not place.
    pub(super) fn read(&mut self, document: &Document, unrecognised: u32) {
        self.report(
            document.key,
            document.trust,
            Outcome::Read {
                hash: document.hash,
                trust: document.trust,
                followed_links: document.followed_links,
                unrecognised,
            },
        );
    }

    /// Lists a directory and records a failure under `key`; a read directory is reported
    /// by its caller once it knows what was in it.
    pub(super) fn directory(
        &mut self,
        reader: &ConfigReader,
        path: &Path,
        key: SourceKey,
    ) -> Option<Listing> {
        match reader.list_directory(path) {
            Ok(listing) => Some(listing),
            Err(failure) => {
                self.failed(key, failure);
                None
            }
        }
    }

    /// Like `directory`, for a place that is allowed not to exist: that is not reported
    /// here, because the caller reports the source once it has looked at everything.
    pub(super) fn directory_optional(
        &mut self,
        reader: &ConfigReader,
        path: &Path,
        key: SourceKey,
    ) -> Option<Listing> {
        match reader.list_directory(path) {
            Ok(listing) => Some(listing),
            Err(SourceFailure::Missing) => None,
            Err(failure) => {
                self.failed(key, failure);
                None
            }
        }
    }

    pub(super) fn push(&mut self, item: Item) {
        if self.items.len() >= self.bounds.items {
            self.truncated = true;
        } else {
            self.items.push(item);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Format {
    Json,
    Toml,
}

/// A parsed source, kept until its adapter has read what it names.
pub(super) struct Document {
    pub key: SourceKey,
    pub trust: PathTrust,
    pub hash: ContentHash,
    pub followed_links: usize,
    pub value: Value,
}

/// A stable identifier for an item: a hash of where it was found, not of what it says, so
/// an edit that does not move it keeps it. It reveals nothing a path would not.
pub(super) fn item_id(parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    digest.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What an adapter knows about the place an item is found in.
#[derive(Clone, Copy)]
pub(super) struct Place {
    pub key: SourceKey,
    pub trust: PathTrust,
}

impl Place {
    pub(super) fn of(document: &Document) -> Self {
        Self {
            key: document.key,
            trust: document.trust,
        }
    }

    pub(super) fn item(
        self,
        kind: ItemKind,
        identity: &[&str],
        name: &str,
        persisted: Persisted,
        effective: Effective,
        detail: Option<Projection>,
    ) -> Item {
        let mut parts = vec![provider_word(self.key.provider()), source_word(self.key)];
        parts.extend_from_slice(identity);
        Item {
            id: item_id(&parts),
            provider: self.key.provider(),
            source: self.key,
            scope: self.key.scope(),
            kind,
            name: redact::text(name).text,
            persisted,
            effective,
            session_use: SessionUse::Unknown,
            not_writable: writability(self.key.class(), self.trust),
            detail,
            hash: None,
        }
    }
}

fn provider_word(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::ClaudeCode => "claude_code",
        ProviderId::Codex => "codex",
    }
}

fn source_word(key: SourceKey) -> &'static str {
    key_name(key)
}

fn key_name(key: SourceKey) -> &'static str {
    match key {
        SourceKey::ClaudeManagedSettings => "claude_managed_settings",
        SourceKey::ClaudeUserSettings => "claude_user_settings",
        SourceKey::ClaudeUserState => "claude_user_state",
        SourceKey::ClaudeUserSkills => "claude_user_skills",
        SourceKey::ClaudeUserRules => "claude_user_rules",
        SourceKey::ClaudeProjectSettings => "claude_project_settings",
        SourceKey::ClaudeProjectLocalSettings => "claude_project_local_settings",
        SourceKey::ClaudeProjectMcp => "claude_project_mcp",
        SourceKey::ClaudeProjectSkills => "claude_project_skills",
        SourceKey::ClaudeProjectRules => "claude_project_rules",
        SourceKey::CodexSystemRequirements => "codex_system_requirements",
        SourceKey::CodexSystemManaged => "codex_system_managed",
        SourceKey::CodexUserConfig => "codex_user_config",
        SourceKey::CodexUserHooks => "codex_user_hooks",
        SourceKey::CodexUserRules => "codex_user_rules",
        SourceKey::CodexProjectConfig => "codex_project_config",
        SourceKey::CodexProjectRules => "codex_project_rules",
    }
}

/// The projection of a value for an item, or nothing when it is deeper than the bound
/// (the item is then shown without detail, which is honest, and the page is not given a
/// partial tree).
pub(super) fn detail(value: &Value, bounds: &Bounds) -> Option<Projection> {
    redact::project(value, bounds).ok()
}

/// A hook handler found in a hooks map, with where in the map it was.
pub(super) struct HookEntry<'a> {
    pub event: &'a str,
    pub matcher: Option<&'a str>,
    pub group: usize,
    pub index: usize,
    pub handler: &'a Value,
}

/// Walks `{ event: [ { matcher?, hooks: [ handler ] } ] }`, the shape both providers use.
/// Anything that does not have that shape is counted, not guessed at.
pub(super) fn hook_entries(hooks: &Value) -> (Vec<HookEntry<'_>>, u32) {
    let mut entries = Vec::new();
    let mut unrecognised = 0_u32;
    let Some(events) = hooks.as_object() else {
        return (entries, 1);
    };
    for (event, groups) in events {
        // Codex keeps the trust of its hooks in the same table.
        if event == "state" {
            continue;
        }
        let Some(groups) = groups.as_array() else {
            unrecognised = unrecognised.saturating_add(1);
            continue;
        };
        for (group, body) in groups.iter().enumerate() {
            let Some(handlers) = body.get("hooks").and_then(Value::as_array) else {
                unrecognised = unrecognised.saturating_add(1);
                continue;
            };
            let matcher = body.get("matcher").and_then(Value::as_str);
            for (index, handler) in handlers.iter().enumerate() {
                if handler.is_object() {
                    entries.push(HookEntry {
                        event,
                        matcher,
                        group,
                        index,
                        handler,
                    });
                } else {
                    unrecognised = unrecognised.saturating_add(1);
                }
            }
        }
    }
    (entries, unrecognised)
}

/// Turns found hooks into items. `persisted` is what the file says about all of them.
pub(super) fn hook_items(
    place: Place,
    hooks: &Value,
    persisted: Persisted,
    effective: Effective,
    bounds: &Bounds,
    sink: &mut Sink,
) -> u32 {
    let (entries, unrecognised) = hook_entries(hooks);
    for entry in entries {
        let name = format!("{} · {}", entry.event, entry.matcher.unwrap_or("*"));
        let (group, index) = (entry.group.to_string(), entry.index.to_string());
        sink.push(place.item(
            ItemKind::Hook,
            &[entry.event, &group, &index],
            &name,
            persisted,
            effective,
            detail(entry.handler, bounds),
        ));
    }
    unrecognised
}

/// Skills are the directories of a skills folder: a name, never its files.
pub(super) fn skill_directories(listing: &Listing) -> impl Iterator<Item = &str> {
    listing
        .entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::Directory | EntryKind::Link))
        .filter_map(|entry| entry.name.to_str())
        .filter(|name| !name.starts_with('.'))
}

/// Records a rule or instructions file by its name and hash. Its text is never read
/// into an item.
pub(super) fn rule_item(place: Place, name: &str, read: Result<ReadFile, SourceFailure>) -> Item {
    let (persisted, hash) = match &read {
        Ok(file) => (Persisted::PresentEnabled, Some(file.hash())),
        Err(_) => (Persisted::Unreadable, None),
    };
    let mut item = place.item(
        ItemKind::RuleFile,
        &[name],
        name,
        persisted,
        Effective::Documented,
        None,
    );
    item.hash = hash;
    item
}

/// Reports a directory source once its adapter has read what it holds: the digest is
/// that of the names in it, so a name added or removed changes it.
pub(super) fn report_listing(
    sink: &mut Sink,
    key: SourceKey,
    listing: &Listing,
    unrecognised: u32,
) {
    let mut names = Vec::new();
    for entry in &listing.entries {
        names.extend_from_slice(entry.name.as_encoded_bytes());
        names.push(0);
    }
    sink.report(
        key,
        listing.trust,
        Outcome::Read {
            hash: ContentHash::of(&names),
            trust: listing.trust,
            followed_links: listing.links.len(),
            unrecognised,
        },
    );
}

/// Reads rule or instructions files by name and hash and reports them as one source:
/// nothing at all is `Absent`, a file that cannot be read is an unreadable item.
pub(super) fn rule_files(
    sink: &mut Sink,
    reader: &ConfigReader,
    key: SourceKey,
    files: &[(String, PathBuf)],
) {
    let mut digest = Vec::new();
    let mut trust = PathTrust::Trusted;
    let mut found = false;
    for (name, path) in files {
        let read = reader.read_file(path);
        match &read {
            Err(SourceFailure::Missing) => continue,
            Ok(file) => {
                digest.extend_from_slice(file.hash().hex().as_bytes());
                if let PathTrust::ReadOnly(_) = file.trust() {
                    trust = file.trust();
                }
            }
            Err(_) => (),
        }
        found = true;
        let place = Place {
            key,
            trust: read.as_ref().map_or(PathTrust::Trusted, ReadFile::trust),
        };
        sink.push(rule_item(place, name, read));
    }
    if found {
        sink.report(
            key,
            trust,
            Outcome::Read {
                hash: ContentHash::of(&digest),
                trust,
                followed_links: 0,
                unrecognised: 0,
            },
        );
    } else {
        sink.report(key, PathTrust::Trusted, Outcome::Absent);
    }
}
