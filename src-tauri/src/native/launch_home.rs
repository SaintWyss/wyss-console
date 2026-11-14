//! The configuration folder one Claude Code launch runs with (`CLAUDE_CONFIG_DIR`), built from the
//! owner's own `~/.claude` and the environment an Arsenal preset chose. The owner's folder is only
//! read: nothing in it is written, moved or switched. The launch home links what a preset does not
//! change (agents, skills, plugins, output styles, hook scripts), writes its own `settings.json`
//! with only the chosen hooks, and copies `.claude.json` once so first-run state carries over.
//! It never holds `.credentials.json`: the session signs in with a long-lived token from the
//! system key ring (`CLAUDE_CODE_OAUTH_TOKEN`), so it never refreshes, and never rotates, the
//! owner's own sign-in.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::store;

/// Entries of `~/.claude` a launch home links as they are. Everything else stays out: the owner's
/// sessions, history and caches belong to the owner's own folder, and the sign-in never moves.
pub const LINKED: [&str; 9] = [
    "skills",
    "plugins",
    "output-styles",
    "hooks",
    "commands",
    "references",
    "bin",
    "get-shit-done",
    "gsd-core",
];
/// The instruction files a preset can keep or leave out as a whole.
pub const INSTRUCTIONS: [&str; 2] = ["CLAUDE.md", "AGENTS.md"];
const CREDENTIALS: &str = ".credentials.json";
/// How Claude Code names a skill the account syncs from claude.ai.
pub const ACCOUNT_PREFIX: &str = "anthropic-skills:";
const MAX_SETTINGS: u64 = 4 * 1024 * 1024;
pub const MAX_NAMES: usize = 512;
/// Where the token is kept in the system key ring (`secret-tool`).
pub const TOKEN_ATTRIBUTES: [(&str, &str); 2] =
    [("service", "wyss-console"), ("kind", "claude-oauth-token")];

/// What a preset changes in the launch environment. `None` keeps that part as the owner's own
/// configuration has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeSpec {
    /// `CLAUDE.md` / `AGENTS.md` and `rules/`.
    #[serde(default = "yes")]
    pub instructions: bool,
    /// Files of `~/.claude/rules` left out, by file name.
    #[serde(default)]
    pub rules_off: Vec<String>,
    /// Hooks left out, by [`hook_key`].
    #[serde(default)]
    pub hooks_off: Vec<String>,
    /// MCP servers of `~/.claude.json` kept. `Some` starts the session with only these
    /// (`--strict-mcp-config`), which also leaves out plugin and claude.ai servers.
    #[serde(default)]
    pub mcp: Option<Vec<String>>,
    /// Plugins switched off, by their `name@marketplace` id.
    #[serde(default)]
    pub plugins_off: Vec<String>,
    #[serde(default = "yes")]
    pub memory: bool,
    /// The owner's `~/.claude/agents` (and the agents of packages linked there).
    #[serde(default = "yes")]
    pub agents: bool,
    /// Skills the account syncs from claude.ai left out, by their loaded name
    /// (`anthropic-skills:<name>`).
    #[serde(default)]
    pub account_skills_off: Vec<String>,
}

fn yes() -> bool {
    true
}

/// The owner's own configuration, unchanged: what a preset that chose nothing gets.
impl Default for HomeSpec {
    fn default() -> Self {
        Self {
            instructions: true,
            rules_off: Vec::new(),
            hooks_off: Vec::new(),
            mcp: None,
            plugins_off: Vec::new(),
            memory: true,
            agents: true,
            account_skills_off: Vec::new(),
        }
    }
}

/// A plain file or plugin name: no path, no control characters.
fn name_ok(name: &str) -> bool {
    (1..=200).contains(&name.len())
        && !name.starts_with('.')
        && !name.contains('/')
        && !name.chars().any(char::is_control)
}

fn key_ok(key: &str) -> bool {
    key.len() == 16
        && key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl HomeSpec {
    pub fn validate(&self) -> Result<(), &'static str> {
        let mcp = self.mcp.as_deref().unwrap_or(&[]);
        let unique = |list: &[String]| list.iter().collect::<BTreeSet<_>>().len() == list.len();
        if self.rules_off.len()
            + self.hooks_off.len()
            + mcp.len()
            + self.plugins_off.len()
            + self.account_skills_off.len()
            > MAX_NAMES
            || !self
                .account_skills_off
                .iter()
                .all(|n| n.strip_prefix(ACCOUNT_PREFIX).is_some_and(name_ok))
            || !unique(&self.account_skills_off)
            || !self.rules_off.iter().all(|n| name_ok(n))
            || !mcp.iter().all(|n| name_ok(n))
            || !self.plugins_off.iter().all(|n| name_ok(n))
            || !self.hooks_off.iter().all(|k| key_ok(k))
            || ![&self.rules_off, &self.hooks_off, &self.plugins_off]
                .iter()
                .all(|l| unique(l))
            || !unique(mcp)
        {
            return Err("home_invalid");
        }
        Ok(())
    }

    /// The preset changes nothing: the launch can use the owner's own folder.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// A hook's stable identity: the first 16 hex digits of SHA-256 over event, matcher and command.
pub fn hook_key(event: &str, matcher: &str, command: &str) -> String {
    let digest = Sha256::digest(format!("{event}\0{matcher}\0{command}").as_bytes());
    format!("{digest:x}")[..16].to_owned()
}

/// The settings a launch home starts with: the owner's, without the hooks left out and with the
/// plugins switched off. Every other key is kept exactly.
pub fn settings(owner: &Value, spec: &HomeSpec) -> Value {
    let mut settings = owner.as_object().cloned().unwrap_or_default();
    let off: BTreeSet<&str> = spec.hooks_off.iter().map(String::as_str).collect();
    if let Some(Value::Object(events)) = settings.get_mut("hooks") {
        for (event, groups) in events.iter_mut() {
            let Value::Array(list) = groups else { continue };
            for group in list.iter_mut() {
                let matcher = group
                    .get("matcher")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                if let Some(Value::Array(hooks)) = group.get_mut("hooks") {
                    hooks.retain(|hook| {
                        let command = hook.get("command").and_then(Value::as_str).unwrap_or("");
                        !off.contains(hook_key(event, &matcher, command).as_str())
                    });
                }
            }
            list.retain(|group| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_none_or(|hooks| !hooks.is_empty())
            });
        }
        events.retain(|_, groups| groups.as_array().is_none_or(|l| !l.is_empty()));
    }
    if !spec.account_skills_off.is_empty() {
        let skills = settings
            .entry("skillOverrides")
            .or_insert_with(|| Value::Object(Map::new()));
        if !skills.is_object() {
            *skills = Value::Object(Map::new());
        }
        if let Some(skills) = skills.as_object_mut() {
            for name in &spec.account_skills_off {
                skills.insert(name.clone(), Value::String("off".to_owned()));
            }
        }
    }
    if !spec.plugins_off.is_empty() {
        let plugins = settings
            .entry("enabledPlugins")
            .or_insert_with(|| Value::Object(Map::new()));
        if !plugins.is_object() {
            *plugins = Value::Object(Map::new());
        }
        if let Some(plugins) = plugins.as_object_mut() {
            for id in &spec.plugins_off {
                plugins.insert(id.clone(), Value::Bool(false));
            }
        }
    }
    Value::Object(settings)
}

/// The `--mcp-config` file: only the kept servers of the owner's `~/.claude.json`. A kept name the
/// owner no longer has is an error, never silently dropped.
pub fn mcp_config(owner_json: &Value, kept: &[String]) -> Result<Value, &'static str> {
    let servers = owner_json
        .get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut chosen = Map::new();
    for name in kept {
        let server = servers.get(name).ok_or("home_mcp_missing")?;
        chosen.insert(name.clone(), server.clone());
    }
    Ok(serde_json::json!({ "mcpServers": chosen }))
}

pub struct Owner {
    /// `~/.claude`
    pub dir: PathBuf,
    /// `~/.claude.json`
    pub json: PathBuf,
}

impl Owner {
    pub fn from_home(home: &Path) -> Self {
        Self {
            dir: home.join(".claude"),
            json: home.join(".claude.json"),
        }
    }
}

pub struct Built {
    pub dir: PathBuf,
    pub mcp_config: Option<PathBuf>,
}

fn read_json(path: &Path) -> io::Result<Value> {
    let bytes =
        store::read_bounded(path, MAX_SETTINGS).ok_or_else(|| io::Error::other("unreadable"))?;
    serde_json::from_slice(&bytes).map_err(|_| io::Error::other("malformed"))
}

/// Replaces `link` with a link to `target`, or removes it when `target` is `None`. Only ever
/// removes a link or a file of the launch home, never a folder.
fn place(link: &Path, target: Option<&Path>) -> io::Result<()> {
    match fs::symlink_metadata(link) {
        Ok(meta) if meta.is_dir() => return Err(io::Error::other("unexpected folder")),
        Ok(_) => {
            if let Some(target) = target
                && fs::read_link(link).ok().as_deref() == Some(target)
            {
                return Ok(());
            }
            fs::remove_file(link)?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    match target {
        Some(target) => symlink(target, link),
        None => Ok(()),
    }
}

/// Builds (or brings up to date) the launch home at `dir`. Safe to run again for the next launch
/// of the same preset: it keeps that home's sessions and history, so they can be resumed.
pub fn build(owner: &Owner, dir: &Path, spec: &HomeSpec) -> io::Result<Built> {
    spec.validate().map_err(io::Error::other)?;
    store::private_dir(dir)?;
    // The sign-in never lives here, not even a link to it.
    match fs::symlink_metadata(dir.join(CREDENTIALS)) {
        Ok(meta) if meta.is_dir() => return Err(io::Error::other("unexpected folder")),
        Ok(_) => fs::remove_file(dir.join(CREDENTIALS))?,
        Err(_) => {}
    }
    for name in LINKED {
        let source = owner.dir.join(name);
        let target = fs::symlink_metadata(&source).is_ok().then_some(source);
        place(&dir.join(name), target.as_deref())?;
    }
    let agents = owner.dir.join("agents");
    let target = (spec.agents && fs::symlink_metadata(&agents).is_ok()).then_some(agents);
    place(&dir.join("agents"), target.as_deref())?;
    for name in INSTRUCTIONS {
        let source = owner.dir.join(name);
        let target = (spec.instructions && fs::symlink_metadata(&source).is_ok()).then_some(source);
        place(&dir.join(name), target.as_deref())?;
    }
    // Rules: a folder of its own, one link per kept file.
    let rules = dir.join("rules");
    if fs::symlink_metadata(&rules).is_ok_and(|m| !m.is_dir()) {
        fs::remove_file(&rules)?;
    }
    store::private_dir(&rules)?;
    for entry in fs::read_dir(&rules)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            fs::remove_file(entry.path())?;
        }
    }
    if spec.instructions
        && let Ok(entries) = fs::read_dir(owner.dir.join("rules"))
    {
        let off: BTreeSet<&str> = spec.rules_off.iter().map(String::as_str).collect();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".md") && name_ok(&name) && !off.contains(name.as_str()) {
                symlink(entry.path(), rules.join(&name))?;
            }
        }
    }
    let owner_settings =
        read_json(&owner.dir.join("settings.json")).unwrap_or(Value::Object(Map::new()));
    let text =
        serde_json::to_vec_pretty(&settings(&owner_settings, spec)).map_err(io::Error::other)?;
    store::write_atomic(&dir.join("settings.json"), &text)?;
    match store::read_bounded(&owner.dir.join("settings.local.json"), MAX_SETTINGS) {
        Some(bytes) => store::write_atomic(&dir.join("settings.local.json"), &bytes)?,
        None => place(&dir.join("settings.local.json"), None)?,
    }
    // First-run state (onboarding, folder trust) carries over once; later launches keep their own.
    let state = dir.join(".claude.json");
    if fs::symlink_metadata(&state).is_err()
        && let Some(bytes) = store::read_bounded(&owner.json, MAX_SETTINGS)
    {
        store::write_atomic(&state, &bytes)?;
    }
    let mcp_config = match &spec.mcp {
        None => None,
        Some(kept) => {
            let owner_json = read_json(&owner.json).unwrap_or(Value::Object(Map::new()));
            let config = mcp_config(&owner_json, kept).map_err(io::Error::other)?;
            let path = dir.join("mcp.json");
            store::write_atomic(
                &path,
                &serde_json::to_vec_pretty(&config).map_err(io::Error::other)?,
            )?;
            Some(path)
        }
    };
    Ok(Built {
        dir: dir.to_path_buf(),
        mcp_config,
    })
}

/// One hook of the owner's settings, as the environment chooser lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookOption {
    pub key: String,
    pub event: String,
    pub matcher: String,
    pub command: String,
}

/// What a preset can choose from: the owner's own hooks, MCP servers, enabled plugins and rule
/// files, read now. Commands are shown so the owner can tell hooks apart; they are the owner's own
/// settings, never a secret store.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Options {
    pub hooks: Vec<HookOption>,
    pub mcp: Vec<String>,
    pub plugins: Vec<String>,
    pub rules: Vec<String>,
    /// Skills the account syncs from claude.ai, by their loaded name.
    pub account_skills: Vec<String>,
    pub instructions: bool,
    /// Whether the long-lived token is stored, so a launch home can sign in.
    pub token_stored: bool,
    /// Bytes of what each choice puts in the context when a session starts, for an estimate:
    /// `instructions` (the instruction files and what they import with `@`), `rule:<name>`, and
    /// `account:<name>` (the name and description an account skill is listed with) and
    /// `plugin:<id>` (what an enabled plugin lists: each skill, agent and command, with the
    /// words around each entry counted as 96 bytes).
    pub sizes: std::collections::BTreeMap<String, u64>,
}

/// Most bytes read of a file only to size it.
const SIZE_READ: u64 = 512 * 1024;

fn file_bytes(path: &Path) -> u64 {
    fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.is_file() || m.file_type().is_symlink())
        .and_then(|_| fs::metadata(path).ok())
        .filter(|m| m.is_file())
        .map_or(0, |m| m.len().min(SIZE_READ))
}

/// The instruction files and the files they import with a line `@path` (one level), as Claude
/// Code loads them. A file reached twice (CLAUDE.md as a link to AGENTS.md) counts once.
fn instruction_bytes(dir: &Path, home: Option<&Path>) -> u64 {
    let mut seen = BTreeSet::new();
    let mut total = 0;
    let mut count = |path: &Path, total: &mut u64| {
        if let Ok(real) = fs::canonicalize(path)
            && seen.insert(real)
        {
            *total += file_bytes(path);
        }
    };
    for name in INSTRUCTIONS {
        let path = dir.join(name);
        count(&path, &mut total);
        let Some(text) = crate::native::store::read_bounded(&path, SIZE_READ) else {
            continue;
        };
        for line in String::from_utf8_lossy(&text).lines() {
            let Some(target) = line.trim().strip_prefix('@') else {
                continue;
            };
            let target = target.trim();
            let resolved = match (target.strip_prefix("~/"), home) {
                (Some(rest), Some(home)) => home.join(rest),
                _ if target.starts_with('/') => std::path::PathBuf::from(target),
                _ => dir.join(target),
            };
            count(&resolved, &mut total);
        }
    }
    total
}

/// The name and description an account skill is listed with: its `SKILL.md` front matter.
fn listed_bytes(skill: &Path) -> u64 {
    front_matter_bytes(&skill.join("SKILL.md"))
}

/// What a plugin folder lists when it loads: the front matter of each skill, agent and command.
fn plugin_bytes(root: &Path) -> u64 {
    const ENTRY: u64 = 96;
    let mut total = 0;
    for skill in fs::read_dir(root.join("skills"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let file = skill.path().join("SKILL.md");
        if file.is_file() {
            total += front_matter_bytes(&file) + ENTRY;
        }
    }
    for folder in ["agents", "commands"] {
        for entry in fs::read_dir(root.join(folder))
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                total += front_matter_bytes(&path) + ENTRY;
            }
        }
    }
    total
}

/// Where each plugin Claude Code installed lives, from its own registry.
fn plugin_roots(owner: &Owner) -> std::collections::BTreeMap<String, std::path::PathBuf> {
    let mut roots = std::collections::BTreeMap::new();
    let registry =
        read_json(&owner.dir.join("plugins/installed_plugins.json")).unwrap_or(Value::Null);
    if let Some(plugins) = registry.get("plugins").and_then(Value::as_object) {
        for (id, installs) in plugins {
            let path = installs
                .as_array()
                .and_then(|list| list.first())
                .and_then(|install| install.get("installPath"))
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from)
                .filter(|p| p.starts_with(owner.dir.join("plugins")));
            if let Some(path) = path {
                roots.insert(id.clone(), path);
            }
        }
    }
    for org in fs::read_dir(owner.dir.join("plugins/synced"))
        .into_iter()
        .flatten()
        .flatten()
    {
        for plugin in fs::read_dir(org.path()).into_iter().flatten().flatten() {
            if let Some(name) = read_json(&plugin.path().join(".claude-plugin/plugin.json"))
                .ok()
                .and_then(|m| m.get("name").and_then(Value::as_str).map(str::to_owned))
            {
                roots
                    .entry(format!("{name}@synced"))
                    .or_insert(plugin.path());
            }
        }
    }
    roots
}

fn front_matter_bytes(file: &Path) -> u64 {
    let Some(text) = crate::native::store::read_bounded(file, SIZE_READ) else {
        return 0;
    };
    let text = String::from_utf8_lossy(&text);
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return 0;
    }
    lines
        .take_while(|l| l.trim() != "---")
        .filter(|l| l.starts_with("name:") || l.starts_with("description:"))
        .map(|l| l.len() as u64)
        .sum()
}

pub fn options(owner: &Owner) -> Options {
    let settings = read_json(&owner.dir.join("settings.json")).unwrap_or(Value::Null);
    let mut hooks = Vec::new();
    if let Some(events) = settings.get("hooks").and_then(Value::as_object) {
        for (event, groups) in events {
            for group in groups.as_array().into_iter().flatten() {
                let matcher = group.get("matcher").and_then(Value::as_str).unwrap_or("");
                for hook in group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let command = hook.get("command").and_then(Value::as_str).unwrap_or("");
                    hooks.push(HookOption {
                        key: hook_key(event, matcher, command),
                        event: event.clone(),
                        matcher: matcher.to_owned(),
                        command: command.to_owned(),
                    });
                }
            }
        }
    }
    let plugins: Vec<String> = settings
        .get("enabledPlugins")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter(|(id, on)| on.as_bool() == Some(true) && name_ok(id))
                .map(|(id, _)| id.clone())
                .collect()
        })
        .unwrap_or_default();
    let mut plugins: Vec<String> = plugins;
    // Plugins the account syncs from claude.ai load as `<name>@synced` and can be switched off
    // the same way; they are not listed in `enabledPlugins`.
    for org in fs::read_dir(owner.dir.join("plugins/synced"))
        .into_iter()
        .flatten()
        .flatten()
    {
        for plugin in fs::read_dir(org.path()).into_iter().flatten().flatten() {
            let manifest = plugin.path().join(".claude-plugin/plugin.json");
            if let Some(name) = read_json(&manifest)
                .ok()
                .and_then(|m| m.get("name").and_then(Value::as_str).map(str::to_owned))
            {
                let id = format!("{name}@synced");
                if name_ok(&id) && !plugins.contains(&id) {
                    plugins.push(id);
                }
            }
        }
    }
    plugins.sort();
    let mcp = read_json(&owner.json)
        .ok()
        .and_then(|json| json.get("mcpServers").and_then(Value::as_object).cloned())
        .map(|servers| servers.keys().filter(|n| name_ok(n)).cloned().collect())
        .unwrap_or_default();
    let mut rules: Vec<String> = fs::read_dir(owner.dir.join("rules"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".md") && name_ok(name))
        .collect();
    rules.sort();
    let mut account_skills: Vec<String> = fs::read_dir(owner.dir.join("skills/synced"))
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|org| fs::read_dir(org.path()).into_iter().flatten().flatten())
        .filter(|skill| skill.path().join("SKILL.md").is_file())
        .map(|skill| format!("{ACCOUNT_PREFIX}{}", skill.file_name().to_string_lossy()))
        .filter(|name| name.strip_prefix(ACCOUNT_PREFIX).is_some_and(name_ok))
        .collect();
    account_skills.sort();
    account_skills.dedup();
    let mut sizes = std::collections::BTreeMap::new();
    let home = owner.dir.parent();
    sizes.insert(
        "instructions".to_owned(),
        instruction_bytes(&owner.dir, home),
    );
    for rule in &rules {
        sizes.insert(
            format!("rule:{rule}"),
            file_bytes(&owner.dir.join("rules").join(rule)),
        );
    }
    let roots = plugin_roots(owner);
    for plugin in &plugins {
        if let Some(root) = roots.get(plugin) {
            sizes.insert(format!("plugin:{plugin}"), plugin_bytes(root));
        }
    }
    for org in fs::read_dir(owner.dir.join("skills/synced"))
        .into_iter()
        .flatten()
        .flatten()
    {
        for skill in fs::read_dir(org.path()).into_iter().flatten().flatten() {
            let name = format!("{ACCOUNT_PREFIX}{}", skill.file_name().to_string_lossy());
            if account_skills.contains(&name) {
                sizes.insert(format!("account:{name}"), listed_bytes(&skill.path()));
            }
        }
    }
    Options {
        hooks,
        mcp,
        plugins,
        rules,
        account_skills,
        instructions: INSTRUCTIONS
            .iter()
            .any(|name| fs::symlink_metadata(owner.dir.join(name)).is_ok()),
        token_stored: false,
        sizes,
    }
}

/// The long-lived token from the system key ring, or `None` when it is not stored. Never logged.
pub fn token() -> Option<zeroize::Zeroizing<String>> {
    let mut command = Command::new("secret-tool");
    command.arg("lookup");
    for (key, value) in TOKEN_ATTRIBUTES {
        command.args([key, value]);
    }
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = zeroize::Zeroizing::new(String::from_utf8(output.stdout).ok()?);
    let token = text.trim();
    (token.len() >= 20
        && token.len() <= 4096
        && !token.chars().any(|c| c.is_whitespace() || c.is_control()))
    .then(|| zeroize::Zeroizing::new(token.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(root: &Path) -> Owner {
        let dir = root.join(".claude");
        fs::create_dir_all(dir.join("skills/design")).unwrap();
        fs::create_dir_all(dir.join("agents")).unwrap();
        fs::create_dir_all(dir.join("rules")).unwrap();
        fs::write(dir.join("rules/ux.md"), "ux").unwrap();
        fs::write(dir.join("rules/invariantes.md"), "inv").unwrap();
        fs::write(dir.join("AGENTS.md"), "kernel").unwrap();
        symlink("AGENTS.md", dir.join("CLAUDE.md")).unwrap();
        fs::write(dir.join(".credentials.json"), "{\"secret\":1}").unwrap();
        fs::create_dir_all(dir.join("skills/synced/org/pdf")).unwrap();
        fs::write(dir.join("skills/synced/org/pdf/SKILL.md"), "x").unwrap();
        let synced = dir.join("plugins/synced/org/abc/.claude-plugin");
        fs::create_dir_all(&synced).unwrap();
        fs::write(synced.join("plugin.json"), "{\"name\":\"engineering\"}").unwrap();
        fs::write(
            dir.join("settings.json"),
            serde_json::json!({
                "model": "opus",
                "env": {"A": "1"},
                "hooks": {
                    "PreToolUse": [
                        {"matcher": "Bash", "hooks": [
                            {"type": "command", "command": "gate.py"},
                            {"type": "command", "command": "rtk"}
                        ]}
                    ],
                    "SessionStart": [{"hooks": [{"type": "command", "command": "orient.sh"}]}]
                },
                "enabledPlugins": {"caveman@caveman": true}
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            root.join(".claude.json"),
            serde_json::json!({
                "hasCompletedOnboarding": true,
                "mcpServers": {"context7": {"url": "https://x"}, "notebooklm": {"command": "nb"}}
            })
            .to_string(),
        )
        .unwrap();
        Owner::from_home(root)
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("launch-home-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn spec() -> HomeSpec {
        HomeSpec {
            instructions: true,
            memory: true,
            agents: true,
            ..HomeSpec::default()
        }
    }

    #[test]
    fn a_full_home_links_the_owner_folder_copies_settings_and_never_holds_the_sign_in() {
        let root = temp("full");
        let owner = owner(&root);
        let home = root.join("homes/p");
        let built = build(&owner, &home, &spec()).unwrap();
        assert!(built.mcp_config.is_none());
        assert_eq!(
            fs::read_link(home.join("skills")).unwrap(),
            owner.dir.join("skills")
        );
        assert_eq!(
            fs::read_link(home.join("agents")).unwrap(),
            owner.dir.join("agents")
        );
        assert!(fs::read_link(home.join("CLAUDE.md")).is_ok());
        assert!(home.join("rules/ux.md").exists() && home.join("rules/invariantes.md").exists());
        assert!(fs::symlink_metadata(home.join(".credentials.json")).is_err());
        let settings: Value =
            serde_json::from_slice(&fs::read(home.join("settings.json")).unwrap()).unwrap();
        let original: Value =
            serde_json::from_slice(&fs::read(owner.dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings, original);
        let state: Value =
            serde_json::from_slice(&fs::read(home.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(state["hasCompletedOnboarding"], true);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&home).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(home.join("settings.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        // The owner's folder is untouched.
        assert!(owner.dir.join(".credentials.json").is_file());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_minimal_home_leaves_out_instructions_rules_hooks_plugins_and_unchosen_servers() {
        let root = temp("min");
        let owner = owner(&root);
        let home = root.join("homes/p");
        // A first full build, then the same home made minimal: stale links go.
        build(&owner, &home, &spec()).unwrap();
        let minimal = HomeSpec {
            instructions: false,
            hooks_off: vec![
                hook_key("PreToolUse", "Bash", "gate.py"),
                hook_key("SessionStart", "", "orient.sh"),
            ],
            mcp: Some(vec!["context7".into()]),
            plugins_off: vec!["caveman@caveman".into()],
            memory: false,
            agents: false,
            account_skills_off: vec!["anthropic-skills:pdf".into()],
            ..HomeSpec::default()
        };
        let built = build(&owner, &home, &minimal).unwrap();
        assert!(fs::symlink_metadata(home.join("agents")).is_err());
        assert!(fs::symlink_metadata(home.join("CLAUDE.md")).is_err());
        assert!(fs::symlink_metadata(home.join("AGENTS.md")).is_err());
        assert_eq!(fs::read_dir(home.join("rules")).unwrap().count(), 0);
        let settings: Value =
            serde_json::from_slice(&fs::read(home.join("settings.json")).unwrap()).unwrap();
        assert_eq!(
            settings["hooks"],
            serde_json::json!({"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "rtk"}]}]})
        );
        assert_eq!(settings["enabledPlugins"]["caveman@caveman"], false);
        assert_eq!(settings["skillOverrides"]["anthropic-skills:pdf"], "off");
        assert_eq!(settings["model"], "opus");
        assert_eq!(settings["env"]["A"], "1");
        let mcp: Value =
            serde_json::from_slice(&fs::read(built.mcp_config.unwrap()).unwrap()).unwrap();
        assert_eq!(
            mcp,
            serde_json::json!({"mcpServers": {"context7": {"url": "https://x"}}})
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rules_are_chosen_by_file_and_a_missing_server_or_a_bad_spec_is_refused() {
        let root = temp("rules");
        let owner = owner(&root);
        let home = root.join("homes/p");
        build(
            &owner,
            &home,
            &HomeSpec {
                rules_off: vec!["ux.md".into()],
                ..spec()
            },
        )
        .unwrap();
        assert!(!home.join("rules/ux.md").exists());
        assert!(home.join("rules/invariantes.md").exists());
        assert!(
            build(
                &owner,
                &home,
                &HomeSpec {
                    mcp: Some(vec!["gone".into()]),
                    ..spec()
                }
            )
            .is_err()
        );
        for bad in [
            HomeSpec {
                rules_off: vec!["../x.md".into()],
                ..spec()
            },
            HomeSpec {
                hooks_off: vec!["zz".into()],
                ..spec()
            },
            HomeSpec {
                plugins_off: vec!["a".into(), "a".into()],
                ..spec()
            },
            HomeSpec {
                account_skills_off: vec!["pdf".into()],
                ..spec()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        // A sign-in placed in the home by anyone is removed before the launch.
        fs::write(home.join(".credentials.json"), "x").unwrap();
        build(&owner, &home, &spec()).unwrap();
        assert!(fs::symlink_metadata(home.join(".credentials.json")).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_plugin_is_sized_by_what_it_lists() {
        let root = temp("plugin");
        fs::create_dir_all(root.join("skills/a")).unwrap();
        fs::create_dir_all(root.join("agents")).unwrap();
        fs::write(
            root.join("skills/a/SKILL.md"),
            "---\nname: a\ndescription: does a\n---\nbody that is not listed\n",
        )
        .unwrap();
        fs::write(root.join("agents/b.md"), "---\nname: b\n---\n").unwrap();
        fs::write(root.join("agents/notes.txt"), "ignored").unwrap();
        // "name: a" (7) + "description: does a" (19) + 96, and "name: b" (7) + 96.
        assert_eq!(plugin_bytes(&root), 7 + 19 + 96 + 7 + 96);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_options_list_each_hook_with_the_key_that_leaves_it_out() {
        let root = temp("options");
        let owner = owner(&root);
        let options = options(&owner);
        assert_eq!(options.hooks.len(), 3);
        let gate = options
            .hooks
            .iter()
            .find(|h| h.command == "gate.py")
            .unwrap();
        assert_eq!(gate.key, hook_key("PreToolUse", "Bash", "gate.py"));
        assert_eq!(
            options.mcp,
            vec!["context7".to_owned(), "notebooklm".to_owned()]
        );
        assert_eq!(
            options.plugins,
            vec![
                "caveman@caveman".to_owned(),
                "engineering@synced".to_owned()
            ]
        );
        assert_eq!(
            options.rules,
            vec!["invariantes.md".to_owned(), "ux.md".to_owned()]
        );
        assert!(options.instructions);
        // Every rule and account skill is sized; instructions count their files.
        for rule in &options.rules {
            assert!(
                options.sizes.contains_key(&format!("rule:{rule}")),
                "{rule}"
            );
        }
        assert!(options.sizes["instructions"] > 0);
        assert_eq!(
            options.account_skills,
            vec!["anthropic-skills:pdf".to_owned()]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_default_spec_is_recognised_and_json_round_trips_with_defaults() {
        assert!(spec().is_default());
        assert!(
            !HomeSpec {
                memory: false,
                ..spec()
            }
            .is_default()
        );
        let parsed: HomeSpec = serde_json::from_str("{}").unwrap();
        assert!(parsed.is_default());
        assert!(serde_json::from_str::<HomeSpec>("{\"other\":1}").is_err());
    }
}
