//! The adapters against private temporary homes shaped like the real ones. No test reads
//! a real provider directory (NFR-MNT-03); every secret in a fixture is the word `SEKRET`
//! so a leak is one `contains` away.
use super::inventory::{Environment, Outcome, SourceKey, inventory};
use super::model::{
    Bounds, Effective, ItemKind, NotWritable, PathTrust, Persisted, ProviderId, SourceFailure,
    TooLarge, UntrustedReason,
};
use super::{Inventory, Item};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let home = base.join(format!("wyss-inv-{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        Self(home)
    }
    fn put(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }
    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn env(&self) -> Environment {
        Environment {
            home: self.0.clone(),
            claude_config_dir: None,
            codex_home: None,
            project: None,
            // No `/etc/codex` here: the owner's machine has one, a test must not.
            system: self.0.join("etc"),
        }
    }
    fn read(&self) -> Inventory {
        inventory(&self.env(), Bounds::PROPOSED).unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        fn open_up(dir: &Path) {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if entry.file_type().is_ok_and(|t| t.is_dir()) {
                        open_up(&entry.path());
                    }
                }
            }
        }
        open_up(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn items(inventory: &Inventory, kind: ItemKind) -> Vec<&Item> {
    inventory
        .items
        .iter()
        .filter(|item| item.kind == kind)
        .collect()
}

fn named<'a>(inventory: &'a Inventory, kind: ItemKind, name: &str) -> &'a Item {
    items(inventory, kind)
        .into_iter()
        .find(|item| item.name.as_str() == name)
        .unwrap_or_else(|| panic!("no {kind:?} named {name}"))
}

fn outcome(inventory: &Inventory, key: SourceKey) -> &Outcome {
    &inventory
        .sources
        .iter()
        .find(|source| source.key == key)
        .unwrap()
        .outcome
}

fn shown(inventory: &Inventory) -> String {
    serde_json::to_string(inventory).unwrap()
}

const CLAUDE_SETTINGS: &str = r#"{
  "env": { "ANTHROPIC_API_KEY": "SEKRET-env" },
  "model": "opus",
  "effortLevel": "high",
  "skillOverrides": { "noisy": "off" },
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [
        { "type": "command", "command": "guard.sh --token SEKRET-flag", "timeout": 5 } ] }
    ],
    "SessionStart": [
      { "hooks": [ { "type": "command", "command": "TOKEN=SEKRET-assign start.sh" },
                   { "type": "http", "url": "https://u:SEKRET-url@host/x",
                     "headers": { "Authorization": "Bearer SEKRET-header" } } ] }
    ]
  }
}"#;

fn claude_home() -> Home {
    let home = Home::new();
    home.put(".claude/settings.json", CLAUDE_SETTINGS);
    home.put(".claude/skills/useful/SKILL.md", "# useful");
    home.put(".claude/skills/noisy/SKILL.md", "# noisy");
    home.put(".claude/CLAUDE.md", "personal rules with SEKRET-prose");
    home.put(".claude/rules/style.md", "style");
    home.put(".claude/.credentials.json", r#"{"token":"SEKRET-cred"}"#);
    home.put(
        ".claude.json",
        r#"{ "numStartups": 7, "oauthAccount": {"emailAddress": "SEKRET-mail"},
             "mcpServers": {
               "context7": { "type": "stdio", "command": "npx", "args": ["-y","ctx"],
                             "env": { "CONTEXT7_API_KEY": "SEKRET-mcp" } },
               "remote": { "type": "http", "url": "https://x/y",
                           "headers": { "X-Api-Key": "SEKRET-hdr" } } } }"#,
    );
    home
}

#[test]
fn claude_code_hooks_skills_servers_rules_and_models_are_found() {
    let home = claude_home();
    let found = home.read();

    assert_eq!(items(&found, ItemKind::Hook).len(), 3);
    let hook = named(&found, ItemKind::Hook, "PreToolUse · Bash");
    assert_eq!(hook.persisted, Persisted::PresentEnabled);
    assert_eq!(hook.effective, Effective::Documented);
    assert_eq!(hook.provider, ProviderId::ClaudeCode);
    assert_eq!(
        named(&found, ItemKind::Hook, "SessionStart · *").source,
        SourceKey::ClaudeUserSettings
    );

    assert_eq!(
        named(&found, ItemKind::Skill, "useful").persisted,
        Persisted::PresentEnabled
    );
    let noisy = named(&found, ItemKind::Skill, "noisy");
    assert_eq!(noisy.persisted, Persisted::PresentDisabled);
    assert_eq!(noisy.effective, Effective::NotVerifiable);

    // Only the servers are read out of the provider-owned file: nothing of the rest.
    let servers = items(&found, ItemKind::McpServer);
    assert_eq!(servers.len(), 2);
    assert!(
        servers
            .iter()
            .all(|s| s.effective == Effective::NotVerifiable)
    );
    assert!(
        servers
            .iter()
            .all(|s| s.not_writable == NotWritable::ProviderOwned)
    );
    assert!(!shown(&found).contains("numStartups") && !shown(&found).contains("emailAddress"));

    assert_eq!(items(&found, ItemKind::ModelPreference).len(), 2);
    let rules = items(&found, ItemKind::RuleFile);
    assert_eq!(rules.len(), 2);
    assert!(rules.iter().all(|r| r.hash.is_some() && r.detail.is_none()));
}

#[test]
fn no_secret_of_any_provider_file_reaches_the_inventory() {
    let home = claude_home();
    home.put(
        ".codex/config.toml",
        r#"model = "gpt"
[mcp_servers.wyss]
command = "wyss"
env = { WYSS_TOKEN = "SEKRET-codex-env" }
[mcp_servers.web]
url = "https://x"
http_headers = { Authorization = "Bearer SEKRET-codex-hdr" }
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "notify --api-key SEKRET-codex-flag"
"#,
    );
    home.put(
        ".codex/auth.json",
        r#"{"tokens":{"access_token":"SEKRET-auth"}}"#,
    );
    let found = home.read();
    let text = shown(&found);
    assert!(
        !text.contains("SEKRET"),
        "a secret reached the inventory: {text}"
    );
    // What surrounds a secret stays: the command, the flag, the server.
    assert!(text.contains("guard.sh") && text.contains("notify") && text.contains("context7"));
}

#[test]
fn the_inventory_names_no_path_of_the_machine() {
    let home = claude_home();
    home.put(".codex/config.toml", "model = \"gpt\"\n");
    let text = shown(&home.read());
    assert!(
        !text.contains(home.0.to_str().unwrap()),
        "a path reached the inventory"
    );
}

#[test]
fn a_credential_file_is_neither_read_nor_reported_as_a_source() {
    let home = claude_home();
    home.put(".codex/auth.json", "{}");
    let found = home.read();
    assert!(
        found
            .sources
            .iter()
            .all(|s| !format!("{:?}", s.key).to_lowercase().contains("credential"))
    );
    assert!(!shown(&found).contains("SEKRET-cred"));
}

#[test]
fn disable_all_hooks_marks_every_hook_disabled() {
    let home = Home::new();
    home.put(
        ".claude/settings.json",
        r#"{ "disableAllHooks": true, "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "x" } ] } ] } }"#,
    );
    let found = home.read();
    assert_eq!(
        named(&found, ItemKind::Hook, "Stop · *").persisted,
        Persisted::PresentDisabled
    );
}

#[test]
fn project_settings_servers_and_rules_are_read_for_the_selected_project_only() {
    let home = Home::new();
    let project = home.dir("work/app");
    home.put(
        "work/app/.claude/settings.json",
        r#"{ "disabledMcpjsonServers": ["off"] }"#,
    );
    home.put(
        "work/app/.mcp.json",
        r#"{ "mcpServers": { "on": {"command":"a"}, "off": {"command":"b"} } }"#,
    );
    home.put("work/app/CLAUDE.md", "rules");
    home.put(
        "work/other/.mcp.json",
        r#"{ "mcpServers": { "elsewhere": {"command":"c"} } }"#,
    );
    let mut env = home.env();
    env.project = Some(project);
    let found = inventory(&env, Bounds::PROPOSED).unwrap();

    assert_eq!(
        named(&found, ItemKind::McpServer, "on").persisted,
        Persisted::PresentEnabled
    );
    assert_eq!(
        named(&found, ItemKind::McpServer, "off").persisted,
        Persisted::PresentDisabled
    );
    assert_eq!(
        named(&found, ItemKind::McpServer, "on").scope,
        super::Scope::Project
    );
    assert!(
        items(&found, ItemKind::McpServer)
            .iter()
            .all(|s| s.name.as_str() != "elsewhere")
    );
    assert_eq!(items(&found, ItemKind::RuleFile).len(), 1);
}

#[test]
fn codex_hooks_skills_servers_and_models_are_found_and_never_verified() {
    let home = Home::new();
    home.put(
        ".codex/config.toml",
        r#"model = "gpt-6"
model_reasoning_effort = "high"
[features]
hooks = true
[[skills.config]]
path = "/home/x/.codex/skills/alpha/SKILL.md"
enabled = false
[[skills.config]]
path = "/home/x/.codex/skills/beta"
[mcp_servers.one]
command = "one"
[mcp_servers.two]
command = "two"
enabled = false
[[hooks.PreToolUse]]
matcher = "Bash"
[[hooks.PreToolUse.hooks]]
type = "command"
command = "check"
[hooks.state.abc]
trusted_hash = "sha256:0123"
"#,
    );
    home.put(
        ".codex/hooks.json",
        r#"{ "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "done" } ] } ] } }"#,
    );
    home.put(".codex/AGENTS.md", "instructions");
    let found = home.read();

    assert_eq!(
        items(&found, ItemKind::Hook).len(),
        2,
        "the hook state table is not a hook"
    );
    assert!(
        items(&found, ItemKind::Hook)
            .iter()
            .all(|h| h.effective == Effective::NotVerifiable)
    );
    assert_eq!(
        named(&found, ItemKind::Skill, "alpha").persisted,
        Persisted::PresentDisabled
    );
    assert_eq!(
        named(&found, ItemKind::Skill, "beta").persisted,
        Persisted::PresentEnabled
    );
    assert_eq!(
        named(&found, ItemKind::McpServer, "two").persisted,
        Persisted::PresentDisabled
    );
    assert_eq!(
        named(&found, ItemKind::McpServer, "one").persisted,
        Persisted::PresentEnabled
    );
    assert_eq!(items(&found, ItemKind::ModelPreference).len(), 2);
    assert_eq!(items(&found, ItemKind::RuleFile).len(), 1);
    assert!(
        found
            .items
            .iter()
            .filter(|i| i.provider == ProviderId::Codex)
            .count()
            >= 9
    );
}

#[test]
fn the_administrators_pin_of_the_hook_engine_wins_over_the_users_choice() {
    let home = Home::new();
    home.put(".codex/config.toml", "[features]\nhooks = true\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = \"command\"\ncommand = \"x\"\n");
    home.put("etc/codex/requirements.toml", "[features]\nhooks = false\n");
    let found = home.read();
    assert_eq!(
        named(&found, ItemKind::Hook, "Stop · *").persisted,
        Persisted::PresentDisabled
    );
    assert!(matches!(
        outcome(&found, SourceKey::CodexSystemRequirements),
        Outcome::Read { .. }
    ));
    let source = found
        .sources
        .iter()
        .find(|s| s.key == SourceKey::CodexSystemRequirements)
        .unwrap();
    assert_eq!(source.not_writable, NotWritable::System);
}

#[test]
fn a_projects_codex_configuration_is_read_only_when_the_user_trusts_the_project() {
    let home = Home::new();
    let project = home.dir("work/app");
    home.put("work/app/.codex/config.toml", "model = \"project-model\"\n");
    let mut env = home.env();
    env.project = Some(project.clone());

    home.put(".codex/config.toml", "model = \"user\"\n");
    let untrusted = inventory(&env, Bounds::PROPOSED).unwrap();
    assert_eq!(
        outcome(&untrusted, SourceKey::CodexProjectConfig),
        &Outcome::ProjectNotTrusted
    );
    assert!(!shown(&untrusted).contains("project-model"));

    home.put(
        ".codex/config.toml",
        &format!(
            "model = \"user\"\n[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            project.display()
        ),
    );
    let trusted = inventory(&env, Bounds::PROPOSED).unwrap();
    assert!(matches!(
        outcome(&trusted, SourceKey::CodexProjectConfig),
        Outcome::Read { .. }
    ));
    assert!(shown(&trusted).contains("project-model"));
}

#[test]
fn an_empty_machine_has_sources_absent_and_no_items() {
    let found = Home::new().read();
    assert!(found.items.is_empty() && !found.truncated);
    assert!(!found.sources.is_empty());
    assert!(
        found
            .sources
            .iter()
            .all(|s| matches!(s.outcome, Outcome::Absent))
    );
}

#[test]
fn one_provider_failing_leaves_the_other_whole() {
    let home = Home::new();
    home.put(".claude/settings.json", "{ not json");
    home.put(".codex/config.toml", "model = \"gpt\"\n");
    let found = home.read();
    assert!(matches!(
        outcome(&found, SourceKey::ClaudeUserSettings),
        Outcome::Failed {
            failure: SourceFailure::Malformed { line: 1, .. }
        }
    ));
    assert_eq!(items(&found, ItemKind::ModelPreference).len(), 1);
}

#[test]
fn a_malformed_toml_says_where_and_nothing_of_what_it_held() {
    let home = Home::new();
    home.put(
        ".codex/config.toml",
        "model = \"ok\"\n\nkey = SEKRET-oops\n",
    );
    let found = home.read();
    let Outcome::Failed {
        failure: SourceFailure::Malformed { line, .. },
    } = outcome(&found, SourceKey::CodexUserConfig)
    else {
        panic!("not malformed");
    };
    assert_eq!(*line, 3);
    assert!(!shown(&found).contains("SEKRET"));
}

#[test]
fn a_settings_link_to_outside_the_roots_is_refused_without_reading_it() {
    let home = Home::new();
    let outside = home.put("elsewhere/real.json", r#"{ "model": "SEKRET-outside" }"#);
    home.dir(".claude");
    symlink(&outside, home.0.join(".claude/settings.json")).unwrap();
    let found = home.read();
    assert!(matches!(
        outcome(&found, SourceKey::ClaudeUserSettings),
        Outcome::Failed {
            failure: SourceFailure::UntrustedPath {
                reason: UntrustedReason::OutsideRoots
            }
        }
    ));
    assert!(!shown(&found).contains("SEKRET"));
}

#[test]
fn a_group_writable_provider_directory_is_read_and_never_writable() {
    let home = Home::new();
    home.put(".claude/settings.json", r#"{ "model": "opus" }"#);
    fs::set_permissions(home.0.join(".claude"), fs::Permissions::from_mode(0o775)).unwrap();
    let found = home.read();
    let source = found
        .sources
        .iter()
        .find(|s| s.key == SourceKey::ClaudeUserSettings)
        .unwrap();
    assert!(matches!(
        source.outcome,
        Outcome::Read {
            trust: PathTrust::ReadOnly(_),
            ..
        }
    ));
    assert_eq!(source.not_writable, NotWritable::SharedPath);
    assert_eq!(items(&found, ItemKind::ModelPreference).len(), 1);
}

#[test]
fn relocation_variables_move_the_roots_and_only_them() {
    let home = Home::new();
    home.put(".claude/settings.json", r#"{ "model": "home-model" }"#);
    home.put(
        "moved/claude/settings.json",
        r#"{ "model": "moved-model" }"#,
    );
    home.put(
        "moved/claude/.claude.json",
        r#"{ "mcpServers": { "moved": {"command":"m"} } }"#,
    );
    home.put("moved/codex/config.toml", "model = \"moved-codex\"\n");
    let mut env = home.env();
    env.claude_config_dir = Some(home.0.join("moved/claude"));
    env.codex_home = Some(home.0.join("moved/codex"));
    let text = shown(&inventory(&env, Bounds::PROPOSED).unwrap());
    assert!(text.contains("moved-model") && text.contains("moved-codex") && text.contains("moved"));
    assert!(!text.contains("home-model"));
}

#[test]
fn reading_changes_no_file_of_any_provider() {
    let home = claude_home();
    home.put(".codex/config.toml", "model = \"gpt\"\n");
    let snapshot = |root: &Path| {
        let mut all = Vec::new();
        let mut pending = vec![root.to_owned()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).unwrap().flatten() {
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    pending.push(entry.path());
                } else {
                    all.push((
                        entry.path(),
                        fs::read(entry.path()).unwrap(),
                        meta.modified().unwrap(),
                        meta.permissions().mode(),
                    ));
                }
            }
        }
        all.sort();
        all
    };
    let before = snapshot(&home.0);
    let _ = home.read();
    assert_eq!(before, snapshot(&home.0));
}

#[test]
fn more_items_than_the_bound_are_cut_and_said() {
    let home = Home::new();
    let servers: String = (0..20)
        .map(|n| format!("\"s{n}\": {{\"command\":\"c\"}}"))
        .collect::<Vec<_>>()
        .join(",");
    home.put(
        ".claude.json",
        &format!("{{ \"mcpServers\": {{ {servers} }} }}"),
    );
    let bounds = Bounds::new(4 * 1024 * 1024, 5, 262_144, 32, 4, 2048, 3).unwrap();
    let found = inventory(&home.env(), bounds).unwrap();
    assert_eq!(found.items.len(), 5);
    assert!(found.truncated);
}

#[test]
fn hostile_toml_is_refused_by_a_bound_and_never_panics_or_stalls() {
    let home = Home::new();
    // Nesting a parser could recurse on, and a very wide table.
    let nested = format!("a = {}1{}\n", "[".repeat(5000), "]".repeat(5000));
    home.put(".codex/config.toml", &nested);
    let started = std::time::Instant::now();
    let found = home.read();
    assert!(matches!(
        outcome(&found, SourceKey::CodexUserConfig),
        Outcome::Failed { .. }
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));

    let wide: String = (0..300_000).map(|n| format!("k{n} = 1\n")).collect();
    home.put(".codex/config.toml", &wide);
    let found = home.read();
    assert!(matches!(
        outcome(&found, SourceKey::CodexUserConfig),
        Outcome::Failed {
            failure: SourceFailure::TooLarge {
                bound: TooLarge::Elements
            }
        }
    ));
}

#[test]
fn unplaceable_entries_are_counted_and_not_guessed_at() {
    let home = Home::new();
    home.put(".claude/settings.json", r#"{ "hooks": { "Stop": "oops", "Pre": [ {"hooks": ["x", {"type":"command","command":"ok"}]} ] } }"#);
    let found = home.read();
    assert_eq!(items(&found, ItemKind::Hook).len(), 1);
    assert!(matches!(
        outcome(&found, SourceKey::ClaudeUserSettings),
        Outcome::Read {
            unrecognised: 2,
            ..
        }
    ));
}

#[test]
fn item_ids_are_stable_across_reads_and_distinct_across_items() {
    let home = claude_home();
    let (first, second) = (home.read(), home.read());
    assert_eq!(first, second);
    let mut ids: Vec<_> = first.items.iter().map(|i| i.id.as_str()).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count);
}

/// Run by hand against the machine it runs on: `cargo test -- --ignored real_machine
/// --nocapture`. It prints counts and outcomes, never a value, and writes nothing.
#[test]
#[ignore = "reads the real configuration of the user running it"]
fn real_machine_counts() {
    let env = Environment::from_process(None).unwrap();
    let started = std::time::Instant::now();
    let found = inventory(&env, Bounds::PROPOSED).unwrap();
    println!(
        "elapsed {:?}, items {}, truncated {}",
        started.elapsed(),
        found.items.len(),
        found.truncated
    );
    for source in &found.sources {
        let word = match &source.outcome {
            Outcome::Read { unrecognised, .. } => format!("read (unrecognised {unrecognised})"),
            other => format!("{other:?}"),
        };
        println!("{:?}: {word}", source.key);
    }
    for kind in [
        ItemKind::Hook,
        ItemKind::Skill,
        ItemKind::McpServer,
        ItemKind::RuleFile,
        ItemKind::ModelPreference,
    ] {
        for provider in [ProviderId::ClaudeCode, ProviderId::Codex] {
            let n = found
                .items
                .iter()
                .filter(|i| i.kind == kind && i.provider == provider)
                .count();
            println!("{provider:?} {kind:?}: {n}");
        }
    }
    let text = serde_json::to_string(&found).unwrap();
    println!(
        "serialised bytes {}, home path present: {}",
        text.len(),
        text.contains(env.home.to_str().unwrap())
    );
}
