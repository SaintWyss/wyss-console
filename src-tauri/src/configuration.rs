//! The two window commands of the inventory (F1-09). Both are reads of what the service
//! already holds: neither reads a file, takes a path, or starts anything, so a window that
//! asks as often as it likes costs the providers' files nothing.
use crate::agentconfig::{Bounds, Changes, Environment, Inventory, InventoryService};
use serde::Serialize;

/// The service as the desktop holds it. `None` when there is no home to read from, which
/// the window is told rather than shown an empty inventory (an empty one would say the
/// user has no configuration).
pub struct Configuration(Option<InventoryService>);

impl Configuration {
    pub fn start() -> Self {
        Self(
            Environment::from_process(None)
                .and_then(|environment| InventoryService::start(environment, Bounds::PROPOSED)),
        )
    }

    fn service(&self) -> Result<&InventoryService, ConfigurationError> {
        self.0.as_ref().ok_or(ConfigurationError::Unavailable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationError {
    Unavailable,
}

/// The whole inventory at a revision.
#[derive(Debug, Serialize)]
pub struct InventoryReply {
    pub revision: u64,
    pub inventory: Inventory,
}

pub fn inventory_reply(
    configuration: &Configuration,
) -> Result<InventoryReply, ConfigurationError> {
    let snapshot = configuration.service()?.snapshot();
    Ok(InventoryReply {
        revision: snapshot.revision,
        inventory: (*snapshot.inventory).clone(),
    })
}

pub fn changes_reply(
    configuration: &Configuration,
    since: u64,
) -> Result<Changes, ConfigurationError> {
    Ok(configuration.service()?.changes_since(since))
}

#[tauri::command]
fn configuration_inventory(
    configuration: tauri::State<'_, Configuration>,
) -> Result<InventoryReply, ConfigurationError> {
    inventory_reply(&configuration)
}

#[tauri::command]
fn configuration_changes(
    configuration: tauri::State<'_, Configuration>,
    since: u64,
) -> Result<Changes, ConfigurationError> {
    changes_reply(&configuration, since)
}

const COMMANDS: &[&str] = &["configuration_inventory", "configuration_changes"];

/// What the desktop binary hands to the library: the commands, the state they read, and
/// nothing else.
pub fn extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: COMMANDS,
        handler: Box::new(tauri::generate_handler![
            configuration_inventory,
            configuration_changes
        ]),
        install: Box::new(|builder| builder.manage(Configuration::start())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::DirBuilderExt};

    fn configuration_over(files: &[(&str, &str)]) -> (Configuration, std::path::PathBuf) {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let home = base.join(format!("wyss-cmd-{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        for (relative, text) in files {
            let path = home.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let environment = Environment {
            home: home.clone(),
            claude_config_dir: None,
            codex_home: None,
            project: None,
            system: home.join("etc"),
        };
        let service = InventoryService::start(environment, Bounds::PROPOSED).unwrap();
        (Configuration(Some(service)), home)
    }

    #[test]
    fn the_inventory_reply_has_the_shape_the_window_is_written_against() {
        let (configuration, home) = configuration_over(&[(
            ".claude/settings.json",
            r#"{ "model": "opus", "env": { "K": "SEKRET" } }"#,
        )]);
        let reply = serde_json::to_value(inventory_reply(&configuration).unwrap()).unwrap();
        assert_eq!(reply["revision"], 1);
        assert_eq!(reply["inventory"]["truncated"], false);
        let item = &reply["inventory"]["items"][0];
        assert_eq!(item["provider"], "claude_code");
        assert_eq!(item["kind"], "model_preference");
        assert_eq!(item["persisted"], "present_enabled");
        assert_eq!(item["effective"], "documented");
        assert_eq!(item["session_use"], "unknown");
        assert_eq!(item["not_writable"], "not_in_this_phase");
        let source = reply["inventory"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["key"] == "claude_user_settings")
            .unwrap();
        assert_eq!(source["outcome"], "read");
        assert_eq!(source["unrecognised"], 0);
        assert!(source["hash"].as_str().unwrap().len() == 64);
        assert!(!reply.to_string().contains("SEKRET"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn the_changes_reply_names_its_result() {
        let (configuration, home) = configuration_over(&[]);
        assert_eq!(
            serde_json::to_value(changes_reply(&configuration, 1).unwrap()).unwrap(),
            serde_json::json!({ "result": "same", "revision": 1 })
        );
        assert_eq!(
            serde_json::to_value(changes_reply(&configuration, 9).unwrap()).unwrap(),
            serde_json::json!({ "result": "resync", "revision": 1 })
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn without_a_service_both_commands_say_unavailable_and_serialize_so() {
        let none = Configuration(None);
        assert_eq!(
            inventory_reply(&none).unwrap_err(),
            ConfigurationError::Unavailable
        );
        assert_eq!(
            changes_reply(&none, 0).unwrap_err(),
            ConfigurationError::Unavailable
        );
        assert_eq!(
            serde_json::to_value(ConfigurationError::Unavailable).unwrap(),
            serde_json::json!("unavailable")
        );
    }

    #[test]
    fn the_commands_the_extension_routes_are_the_ones_the_manifest_and_capability_name() {
        let routed = extension().commands;
        let build = include_str!("../build.rs");
        let capability = include_str!("../capabilities/main.json");
        for command in routed {
            assert!(
                build.contains(&format!("\"{command}\"")),
                "{command} not in build.rs"
            );
            let permission = format!("allow-{}", command.replace('_', "-"));
            assert!(
                capability.contains(&permission),
                "{permission} not in the capability"
            );
        }
    }

    /// The contract the window is written against, as one file both sides read:
    /// `tests/fixtures/configuration-inventory-v1.json`. Set `WYSS_UPDATE_FIXTURES=1` to
    /// write it again after a deliberate change, then run Prettier on it.
    #[test]
    fn the_inventory_reply_is_exactly_the_shared_fixture() {
        use std::os::unix::fs::PermissionsExt;
        let (configuration, home) = configuration_over(&[
            (
                ".claude/settings.json",
                r#"{ "model": "opus",
                    "skillOverrides": { "noisy": "off" },
                    "hooks": { "PreToolUse": [ { "matcher": "Bash", "hooks": [
                      { "type": "command", "command": "guard.sh --token SEKRET", "timeout": 5 } ] } ] } }"#,
            ),
            (".claude/skills/useful/SKILL.md", "# useful"),
            (".claude/skills/noisy/SKILL.md", "# noisy"),
            (".claude/CLAUDE.md", "rules"),
            (
                ".claude.json",
                r#"{ "mcpServers": { "context7": { "command": "npx", "args": ["-y", "ctx"],
                     "env": { "CONTEXT7_API_KEY": "SEKRET" } } } }"#,
            ),
            (
                ".codex/config.toml",
                "model = \"gpt-6\"\n[mcp_servers.off]\ncommand = \"x\"\nenabled = false\n",
            ),
            (".codex/hooks.json", "{ not json"),
        ]);
        // A provider directory a group can write: read, and never writable.
        fs::set_permissions(home.join(".codex"), fs::Permissions::from_mode(0o775)).unwrap();
        configuration
            .service()
            .unwrap()
            .select_project(Some(home.clone()));
        let reply = inventory_reply(&configuration).unwrap();
        let mut value = serde_json::to_value(&reply).unwrap();
        // The revision depends on how many readings the setup caused; the fixture pins
        // the shape, not that count.
        value["revision"] = serde_json::json!(1);
        let text = serde_json::to_string_pretty(&value).unwrap() + "\n";
        assert!(!text.contains("SEKRET") && !text.contains(home.to_str().unwrap()));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/configuration-inventory-v1.json");
        if std::env::var_os("WYSS_UPDATE_FIXTURES").is_some() {
            fs::write(&path, &text).unwrap();
        }
        // Compared as values: the file is formatted by Prettier, not by this test.
        let expected: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            value, expected,
            "the contract changed: update the fixture on purpose"
        );
        let _ = fs::remove_dir_all(home);
    }
}
