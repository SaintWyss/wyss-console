use wyss_console::agentconfig::{Bounds, Environment, ProviderId, inventory};

use crate::t;

pub struct Row {
    pub provider: ProviderId,
    pub kind: String,
    pub name: String,
    pub value: String,
    pub event: Option<String>,
}

impl Row {
    pub fn load() -> Vec<Self> {
        let Some(inventory) = Environment::from_process(None)
            .and_then(|environment| inventory(&environment, Bounds::default()).ok())
        else {
            return Vec::new();
        };
        inventory
            .items
            .iter()
            .filter_map(|item| {
                let kind = serde_json::to_value(item.kind).ok()?.as_str()?.to_owned();
                let detail = serde_json::to_value(&item.detail).ok()?;
                let field = |name: &str| {
                    detail["value"]
                        .as_array()
                        .and_then(|fields| {
                            fields.iter().find(|field| field[0].as_str() == Some(name))
                        })
                        .map_or("", |field| {
                            if field[1]["kind"] == "hidden" {
                                crate::i18n::translate(crate::i18n::lang(), t!("[hidden]"))
                            } else {
                                field[1]["value"]["text"].as_str().unwrap_or_default()
                            }
                        })
                };
                let name = item.name.as_str();
                let event =
                    (kind == "hook").then(|| name.split(" · ").next().unwrap_or(name).to_owned());
                let value = match kind.as_str() {
                    "hook" => field("command"),
                    "rule_file" => t!("Rule file"),
                    "model_preference" => detail["value"]["text"]
                        .as_str()
                        .unwrap_or(t!("Model setting")),
                    "plugin" => t!("Plugin"),
                    "mcp_server" => field("url"),
                    _ => return None,
                };
                let shown = if event.is_some() {
                    script_name(value).unwrap_or(name)
                } else {
                    name
                };
                Some(Self {
                    provider: item.provider,
                    kind,
                    name: shown.to_owned(),
                    value: if matches!(value, "Rule file" | "Model setting" | "Plugin") {
                        crate::i18n::tr(value)
                    } else {
                        value.to_owned()
                    },
                    event,
                })
            })
            .collect()
    }
}

fn script_name(command: &str) -> Option<&str> {
    let words = || {
        command
            .split_whitespace()
            .map(|word| word.trim_matches(['\'', '"']))
    };
    words()
        .find(|word| {
            [".py", ".sh", ".js", ".mjs", ".ts", ".rb"]
                .iter()
                .any(|ext| word.ends_with(ext))
        })
        .or_else(|| {
            words().find(|word| {
                !word.is_empty()
                    && !word.starts_with("/usr/bin/")
                    && !matches!(*word, "bash" | "sh" | "python" | "python3" | "node")
            })
        })
        .and_then(|word| word.rsplit('/').next())
}

pub fn event_title(event: &str) -> &str {
    match event {
        "SessionStart" => t!("When a session opens"),
        "SessionEnd" => t!("When a session ends"),
        "UserPromptSubmit" => t!("When you send a message"),
        "PreToolUse" => t!("Before a tool runs"),
        "PostToolUse" => t!("After a tool runs"),
        "PostToolUseFailure" => t!("When a tool fails"),
        "PermissionRequest" => t!("When permission is requested"),
        "SubagentStart" => t!("When a subagent starts"),
        "Notification" => t!("On a notification"),
        "Stop" => t!("When the answer ends"),
        "SubagentStop" => t!("When a subagent ends"),
        "PreCompact" => t!("Before the context is compacted"),
        _ => event,
    }
}
