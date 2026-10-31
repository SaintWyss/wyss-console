//! The two worker adapters: how a delegated Codex or Claude Code process is started without a
//! terminal, what its output stream says, and how the configuration it actually received is
//! checked. Pure functions over strings and JSON, so each provider's behaviour is tested here.
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Reasoning levels Claude Code's `--effort` accepts (`claude --help`, 2.1.316).
pub const CLAUDE_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// What one worker is started with, beside its executable.
pub struct Launch<'a> {
    pub model: &'a str,
    pub effort: &'a str,
    pub workspace: &'a Path,
    pub task: &'a str,
}

/// `codex exec` (0.159.0): JSON events on stdout, the last message in a file, writes confined to
/// the task workspace, no network for its commands, never asking for approval (a request it would
/// need is refused instead), and the switches Arsenal computed for this agent's skills. The user's
/// MCP servers and plugins are switched off for this one process; their configuration is not read
/// or changed.
pub fn codex_argv(
    launch: &Launch<'_>,
    last_message: &Path,
    skill_words: &[String],
    mcp_servers: &BTreeSet<String>,
    plugins: &BTreeSet<String>,
) -> Vec<String> {
    let mut argv: Vec<String> = [
        "exec",
        "--json",
        "--skip-git-repo-check",
        "-m",
        launch.model,
        "-c",
    ]
    .iter()
    .map(|w| (*w).to_owned())
    .collect();
    argv.push(format!(
        "model_reasoning_effort={}",
        toml::Value::String(launch.effort.to_owned())
    ));
    argv.extend(["-s".to_owned(), "workspace-write".to_owned()]);
    argv.extend(["-C".to_owned(), launch.workspace.display().to_string()]);
    argv.extend(["-c".to_owned(), "approval_policy=\"never\"".to_owned()]);
    argv.extend([
        "-c".to_owned(),
        "sandbox_workspace_write.network_access=false".to_owned(),
    ]);
    for name in mcp_servers {
        argv.extend([
            "-c".to_owned(),
            format!("mcp_servers.{}.enabled=false", toml_key(name)),
        ]);
    }
    for key in plugins {
        argv.extend([
            "-c".to_owned(),
            format!("plugins.{}.enabled=false", toml_key(key)),
        ]);
    }
    argv.extend(skill_words.iter().cloned());
    argv.extend(["-o".to_owned(), last_message.display().to_string()]);
    argv.push("--".to_owned());
    argv.push(launch.task.to_owned());
    argv
}

/// `claude -p` (2.1.316): stream-json events, the model and effort asked for, the session settings
/// Arsenal wrote (skills switched off, plugins switched off), file edits accepted only inside the
/// workspace, no MCP server at all, and no shell or web tools unless the policy allows the shell.
pub fn claude_argv(launch: &Launch<'_>, settings: &Path, allow_bash: bool) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--model".into(),
        launch.model.into(),
        "--effort".into(),
        launch.effort.into(),
        "--settings".into(),
        settings.display().to_string(),
        "--permission-mode".into(),
        "acceptEdits".into(),
        "--strict-mcp-config".into(),
        "--disallowedTools".into(),
    ];
    argv.push(if allow_bash {
        "WebFetch,WebSearch".into()
    } else {
        "Bash,WebFetch,WebSearch".into()
    });
    argv.push("--".into());
    argv.push(launch.task.to_owned());
    argv
}

/// A TOML key that is a quoted string, so no name can add a path segment.
fn toml_key(name: &str) -> String {
    toml::Value::String(name.to_owned()).to_string()
}

/// What one line of a worker's output says, folded into what the record shows.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Observed {
    pub thread_id: Option<String>,
    pub model: Option<String>,
    pub skills: Option<Vec<String>>,
    pub plugins: Option<Vec<String>>,
    pub mcp_servers: Option<Vec<String>>,
    pub last_message: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub usage: Option<Value>,
    pub steps: u64,
    pub finished: bool,
}

/// Reads one JSON line of a worker. Lines that are not JSON or not known are counted as nothing.
pub fn observe(provider: &str, line: &str, seen: &mut Observed) {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let kind = event["type"].as_str().unwrap_or_default();
    match provider {
        "codex" => match kind {
            "thread.started" => seen.thread_id = event["thread_id"].as_str().map(str::to_owned),
            "item.completed" => {
                seen.steps += 1;
                if event["item"]["type"] == "agent_message" {
                    seen.last_message = event["item"]["text"].as_str().map(clip);
                }
            }
            "turn.completed" => {
                seen.usage = Some(event["usage"].clone());
                seen.finished = true;
            }
            "turn.failed" | "error" => {
                seen.error = Some(clip(
                    event["error"]["message"]
                        .as_str()
                        .or_else(|| event["message"].as_str())
                        .unwrap_or("Codex reported an error."),
                ));
            }
            _ => {}
        },
        _ => match kind {
            "system" if event["subtype"] == "init" => {
                seen.model = event["model"].as_str().map(str::to_owned);
                seen.skills = names(&event["skills"]);
                seen.plugins = names(&event["plugins"]);
                seen.mcp_servers = names(&event["mcp_servers"]);
            }
            "assistant" => seen.steps += 1,
            "result" => {
                seen.finished = true;
                seen.usage = Some(serde_json::json!({
                    "usage": event["usage"],
                    "total_cost_usd": event["total_cost_usd"],
                    "duration_ms": event["duration_ms"],
                    "num_turns": event["num_turns"],
                }));
                if event["is_error"] == true {
                    seen.error = Some(clip(
                        event["result"]
                            .as_str()
                            .unwrap_or("Claude Code reported an error."),
                    ));
                } else {
                    seen.result = event["result"].as_str().map(clip);
                }
            }
            _ => {}
        },
    }
}

/// Names from a list of strings or of objects with a `name`.
fn names(value: &Value) -> Option<Vec<String>> {
    value.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| item["name"].as_str())
                    .map(str::to_owned)
            })
            .collect()
    })
}

/// Most bytes of a message kept in the record.
pub const MAX_TEXT: usize = 64 * 1024;

fn clip(text: &str) -> String {
    if text.len() <= MAX_TEXT {
        return text.to_owned();
    }
    let mut end = MAX_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// Whether the model Claude Code reports is the one asked for: the exact name, or the newest model
/// of an alias (`opus` → `claude-opus-…`).
pub fn claude_model_matches(requested: &str, reported: &str) -> bool {
    reported == requested
        || (!requested.starts_with("claude-")
            && reported.starts_with(&format!("claude-{requested}-")))
}

/// The configuration a Claude Code worker reported against what Wyss asked for: the model, no
/// switched-off skill listed, every prepared skill listed, no plugin and no MCP server.
pub fn verify_claude(
    requested_model: &str,
    seen: &Observed,
    excluded: &[String],
    kept: &[String],
) -> Vec<String> {
    let mut problems = Vec::new();
    match &seen.model {
        Some(model) if claude_model_matches(requested_model, model) => {}
        Some(model) => problems.push(format!("model {model} is not {requested_model}")),
        None => problems.push("Claude Code did not report its configuration".to_owned()),
    }
    if let Some(listed) = &seen.skills {
        let listed: BTreeSet<&str> = listed.iter().map(String::as_str).collect();
        for name in excluded {
            if listed.contains(name.as_str()) {
                problems.push(format!("skill {name} is exposed"));
            }
        }
        for name in kept {
            if !listed.contains(name.as_str()) {
                problems.push(format!("skill {name} is missing"));
            }
        }
    } else {
        problems.push("Claude Code did not list its skills".to_owned());
    }
    if seen.plugins.as_ref().is_some_and(|p| !p.is_empty()) {
        problems.push("plugins are loaded".to_owned());
    }
    if seen.mcp_servers.as_ref().is_some_and(|m| !m.is_empty()) {
        problems.push("MCP servers are loaded".to_owned());
    }
    problems
}

/// The `SKILL.md` files a Codex session listed to the model, read from its own session record: the
/// root table (`` - `r1` = `/path` ``) and each skill line's `(file: r1/…/SKILL.md)`.
pub fn codex_listed_files(rollout: &str) -> Option<BTreeSet<PathBuf>> {
    for line in rollout.lines() {
        if !line.contains("(file: r") {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let mut texts = Vec::new();
        collect_texts(&event, &mut texts);
        for text in texts {
            if !text.contains("(file: r") {
                continue;
            }
            let mut roots = std::collections::BTreeMap::new();
            for row in text.lines() {
                let row = row.trim();
                if let Some(rest) = row.strip_prefix("- `r")
                    && let Some((index, path)) = rest.split_once("` = `")
                {
                    roots.insert(format!("r{index}"), path.trim_end_matches('`').to_owned());
                }
            }
            let mut files = BTreeSet::new();
            for row in text.lines() {
                let Some(start) = row.find("(file: r") else {
                    continue;
                };
                let tail = &row[start + "(file: ".len()..];
                let Some(end) = tail.find(')') else {
                    continue;
                };
                let reference = &tail[..end];
                let Some((root, relative)) = reference.split_once('/') else {
                    continue;
                };
                if let Some(base) = roots.get(root) {
                    files.insert(Path::new(base).join(relative));
                }
            }
            if !files.is_empty() {
                return Some(files);
            }
        }
    }
    None
}

fn collect_texts<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|v| collect_texts(v, out)),
        Value::Object(map) => map.values().for_each(|v| collect_texts(v, out)),
        _ => {}
    }
}

/// The configuration a Codex worker received against what Wyss asked for: no switched-off file
/// listed, every kept file listed.
pub fn verify_codex(
    listed: &BTreeSet<PathBuf>,
    excluded: &[String],
    kept: &[PathBuf],
) -> Vec<String> {
    let mut problems = Vec::new();
    for file in excluded {
        if listed.contains(Path::new(file)) {
            problems.push(format!("{file} is exposed"));
        }
    }
    for file in kept {
        if !listed.contains(file) {
            problems.push(format!("{} is missing", file.display()));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(workspace: &Path) -> Launch<'_> {
        Launch {
            model: "gpt-6.1-terra",
            effort: "low",
            workspace,
            task: "--write a file",
        }
    }

    #[test]
    fn codex_runs_confined_never_asks_and_takes_the_task_after_the_options() {
        let ws = Path::new("/w/ws");
        let mcp: BTreeSet<String> = ["context7".to_owned(), "odd\"name".to_owned()].into();
        let plugins: BTreeSet<String> = ["github@openai-curated".to_owned()].into();
        let argv = codex_argv(
            &launch(ws),
            Path::new("/w/last.txt"),
            &["-c".into(), "skills.config=[]".into()],
            &mcp,
            &plugins,
        );
        let joined = argv.join(" ");
        assert_eq!(argv[..3], ["exec", "--json", "--skip-git-repo-check"]);
        assert!(joined.contains("-s workspace-write -C /w/ws"));
        assert!(joined.contains("approval_policy=\"never\""));
        assert!(joined.contains("sandbox_workspace_write.network_access=false"));
        assert!(joined.contains("model_reasoning_effort=\"low\""));
        // Each key is one quoted TOML key, whatever the name holds.
        let odd = argv
            .iter()
            .find(|w| w.starts_with("mcp_servers.") && w.contains("odd"))
            .unwrap();
        let parsed: toml::Table = toml::from_str(odd).unwrap();
        assert_eq!(
            parsed["mcp_servers"]["odd\"name"]["enabled"].as_bool(),
            Some(false)
        );
        assert!(argv.contains(&"plugins.\"github@openai-curated\".enabled=false".to_owned()));
        assert!(
            !argv
                .iter()
                .any(|w| w.contains("--no-daemon") || w.contains("bypass"))
        );
        // The task is one word after `--`, so a task starting with a dash is not an option.
        assert_eq!(argv[argv.len() - 2..], ["--", "--write a file"]);
    }

    #[test]
    fn claude_runs_without_mcp_shell_or_web_unless_the_shell_is_allowed() {
        let argv = claude_argv(
            &launch(Path::new("/w")),
            Path::new("/w/settings.json"),
            false,
        );
        let joined = argv.join(" ");
        assert!(joined.contains("--strict-mcp-config"));
        assert!(joined.contains("--disallowedTools Bash,WebFetch,WebSearch"));
        assert!(joined.contains("--permission-mode acceptEdits"));
        assert!(joined.contains("--settings /w/settings.json"));
        assert!(!joined.contains("bypassPermissions"));
        let shell = claude_argv(&launch(Path::new("/w")), Path::new("/s"), true);
        assert!(
            shell
                .join(" ")
                .contains("--disallowedTools WebFetch,WebSearch")
        );
    }

    #[test]
    fn the_streams_of_both_providers_are_read_into_one_record() {
        let mut codex = Observed::default();
        for line in [
            r#"{"type":"thread.started","thread_id":"t-1"}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
            r#"{"type":"turn.completed","usage":{"output_tokens":5}}"#,
            "not json",
        ] {
            observe("codex", line, &mut codex);
        }
        assert_eq!(codex.thread_id.as_deref(), Some("t-1"));
        assert_eq!(codex.last_message.as_deref(), Some("done"));
        assert!(codex.finished && codex.error.is_none());

        let mut claude = Observed::default();
        for line in [
            r#"{"type":"system","subtype":"init","model":"claude-haiku-5-5","skills":["a","b"],"plugins":[],"mcp_servers":[]}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"OK","total_cost_usd":0.01}"#,
        ] {
            observe("claude_code", line, &mut claude);
        }
        assert_eq!(claude.model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(claude.result.as_deref(), Some("OK"));
        assert!(verify_claude("haiku", &claude, &["c".into()], &["a".into()]).is_empty());
        let problems = verify_claude("opus", &claude, &["a".into()], &["z".into()]);
        assert_eq!(problems.len(), 3, "{problems:?}");
    }

    #[test]
    fn an_alias_matches_only_its_own_family() {
        assert!(claude_model_matches("haiku", "claude-haiku-5-5"));
        assert!(claude_model_matches("claude-opus-5-5", "claude-opus-5-5"));
        assert!(!claude_model_matches("haiku", "claude-opus-5-5"));
        assert!(!claude_model_matches("claude-opus-5-5", "claude-opus-5-6"));
    }

    #[test]
    fn a_codex_session_record_names_the_files_it_listed() {
        let text = "### Skill roots\n- `r0` = `/h/.codex/skills`\n- `r1` = `/h/.agents/skills`\n### Available skills\n- gsap: GSAP (file: r1/gsap-react/SKILL.md)\n- pdf: PDF (file: r0/pdf/SKILL.md)\n";
        let line =
            serde_json::json!({"type":"response_item","payload":{"content":[{"text": text}]}})
                .to_string();
        let files =
            codex_listed_files(&format!("{{\"type\":\"session_meta\"}}\n{line}\n")).unwrap();
        assert!(files.contains(Path::new("/h/.agents/skills/gsap-react/SKILL.md")));
        assert!(files.contains(Path::new("/h/.codex/skills/pdf/SKILL.md")));
        let problems = verify_codex(
            &files,
            &["/h/.codex/skills/pdf/SKILL.md".into()],
            &[PathBuf::from("/h/.agents/skills/gsap-react/SKILL.md")],
        );
        assert_eq!(problems, ["/h/.codex/skills/pdf/SKILL.md is exposed"]);
    }
}
