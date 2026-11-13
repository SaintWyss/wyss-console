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
pub const LINKED: [&str; 10] = [
    "agents",
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
const MAX_SETTINGS: u64 = 4 * 1024 * 1024;
pub const MAX_NAMES: usize = 512;
/// Where the token is kept in the system key ring (`secret-tool`).
pub const TOKEN_ATTRIBUTES: [(&str, &str); 2] =
    [("service", "wyss-console"), ("kind", "claude-oauth-token")];

/// What a preset changes in the launch environment. `None` keeps that part as the owner's own
/// configuration has it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
}

fn yes() -> bool {
    true
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
        if self.rules_off.len() + self.hooks_off.len() + mcp.len() + self.plugins_off.len()
            > MAX_NAMES
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
        self == &Self {
            instructions: true,
            memory: true,
            ..Self::default()
        }
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
    pub instructions: bool,
    /// Whether the long-lived token is stored, so a launch home can sign in.
    pub token_stored: bool,
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
    let plugins = settings
        .get("enabledPlugins")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter(|(id, on)| on.as_bool() == Some(true) && name_ok(id))
                .map(|(id, _)| id.clone())
                .collect()
        })
        .unwrap_or_default();
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
    Options {
        hooks,
        mcp,
        plugins,
        rules,
        instructions: INSTRUCTIONS
            .iter()
            .any(|name| fs::symlink_metadata(owner.dir.join(name)).is_ok()),
        token_stored: false,
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
            ..HomeSpec::default()
        };
        let built = build(&owner, &home, &minimal).unwrap();
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
        assert_eq!(options.plugins, vec!["caveman@caveman".to_owned()]);
        assert_eq!(
            options.rules,
            vec!["invariantes.md".to_owned(), "ux.md".to_owned()]
        );
        assert!(options.instructions);
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
