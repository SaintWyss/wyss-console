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
) -> Result<Vec<String>, String> {
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
        "-c".to_owned(),
        "features.multi_agent=false".to_owned(),
    ]);
    for name in mcp_servers {
        argv.extend([
            "-c".to_owned(),
            format!("mcp_servers.{}.enabled=false", config_key(name)?),
        ]);
    }
    for key in plugins {
        argv.extend([
            "-c".to_owned(),
            format!("plugins.{}.enabled=false", config_key(key)?),
        ]);
    }
    argv.extend(skill_words.iter().cloned());
    argv.extend(["-o".to_owned(), last_message.display().to_string()]);
    argv.push("--".to_owned());
    argv.push(launch.task.to_owned());
    Ok(argv)
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
    let mut disallowed = Vec::new();
    if !allow_bash {
        disallowed.push("Bash");
    }
    disallowed.extend(["WebFetch", "WebSearch", "Agent", "Task"]);
    argv.push(disallowed.join(","));
    argv.push("--".into());
    argv.push(launch.task.to_owned());
    argv
}

/// A TOML key that is a quoted string, so no name can add a path segment.
/// A name `codex -c` can address. Codex splits the key path on every dot and keeps quotes as
/// written (`mcp_servers."context7"` names a new server and fails to load), so a name with a dot,
/// `=`, a quote or white space cannot be turned off from the command line: the delegation is
/// refused rather than run with it on.
fn config_key(name: &str) -> Result<&str, String> {
    if name.is_empty() || name.contains(['.', '=', '"', '\'']) || name.contains(char::is_whitespace)
    {
        Err(name.to_owned())
    } else {
        Ok(name)
    }
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
    match &seen.plugins {
        Some(plugins) if !plugins.is_empty() => problems.push("plugins are loaded".to_owned()),
        None => problems.push("Claude Code did not list its plugins".to_owned()),
        _ => {}
    }
    match &seen.mcp_servers {
        Some(servers) if !servers.is_empty() => problems.push("MCP servers are loaded".to_owned()),
        None => problems.push("Claude Code did not list its MCP servers".to_owned()),
        _ => {}
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRuntime {
    pub model: String,
    pub effort: String,
}

/// Runtime settings from a Codex `turn_context` record bound to the worker's own session.
/// A later incomplete context revokes the earlier observation.
pub fn codex_runtime(rollout: &str, expected_session: &str) -> Option<CodexRuntime> {
    let mut session: Option<String> = None;
    let mut observed = None;
    for line in rollout.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let payload = event
            .get("payload")
            .filter(|payload| payload.is_object())
            .unwrap_or(&Value::Null);
        session = event
            .get("sessionId")
            .and_then(Value::as_str)
            .or_else(|| event.get("session_id").and_then(Value::as_str))
            .or_else(|| payload.get("session_id").and_then(Value::as_str))
            .map(str::to_owned)
            .or(session);
        match event["type"].as_str() {
            Some("session_meta") => {
                session = payload
                    .get("id")
                    .and_then(Value::as_str)
                    .or_else(|| payload.get("session_id").and_then(Value::as_str))
                    .map(str::to_owned)
                    .or(session);
                observed = None;
            }
            Some("turn_context") => {
                observed = None;
                if session.as_deref() == Some(expected_session) {
                    let model = payload.get("model").and_then(Value::as_str);
                    let effort = payload.get("effort").and_then(Value::as_str);
                    if let (Some(model), Some(effort)) = (model, effort)
                        && runtime_token(model)
                        && runtime_token(effort)
                    {
                        observed = Some(CodexRuntime {
                            model: model.to_owned(),
                            effort: effort.to_owned(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    observed
}

fn runtime_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(byte))
}

/// Compares only the worker runtime event, never prompt text or configured model defaults.
pub fn verify_codex_runtime(
    requested_model: &str,
    requested_effort: &str,
    observed: Option<&CodexRuntime>,
) -> Vec<String> {
    let Some(observed) = observed else {
        return vec!["Codex did not report its runtime model and effort".into()];
    };
    let mut problems = Vec::new();
    if observed.model != requested_model {
        problems.push(format!("model {} is not {requested_model}", observed.model));
    }
    if observed.effort != requested_effort {
        problems.push(format!(
            "effort {} is not {requested_effort}",
            observed.effort
        ));
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
        let mcp: BTreeSet<String> = ["context7".to_owned(), "node_repl".to_owned()].into();
        let plugins: BTreeSet<String> = ["github@openai-curated".to_owned()].into();
        let argv = codex_argv(
            &launch(ws),
            Path::new("/w/last.txt"),
            &["-c".into(), "skills.config=[]".into()],
            &mcp,
            &plugins,
        )
        .unwrap();
        let joined = argv.join(" ");
        assert_eq!(argv[..3], ["exec", "--json", "--skip-git-repo-check"]);
        assert!(joined.contains("-s workspace-write -C /w/ws"));
        assert!(joined.contains("approval_policy=\"never\""));
        assert!(joined.contains("sandbox_workspace_write.network_access=false"));
        assert!(joined.contains("features.multi_agent=false"));
        assert!(joined.contains("model_reasoning_effort=\"low\""));
        // Bare keys: Codex keeps quotes as part of the name.
        assert!(argv.contains(&"mcp_servers.context7.enabled=false".to_owned()));
        assert!(argv.contains(&"mcp_servers.node_repl.enabled=false".to_owned()));
        assert!(argv.contains(&"plugins.github@openai-curated.enabled=false".to_owned()));
        assert!(
            !argv
                .iter()
                .any(|w| w.contains("--no-daemon") || w.contains("bypass"))
        );
        // The task is one word after `--`, so a task starting with a dash is not an option.
        assert_eq!(argv[argv.len() - 2..], ["--", "--write a file"]);
    }

    #[test]
    fn a_server_or_plugin_codex_cannot_address_refuses_the_worker_instead_of_leaving_it_on() {
        for odd in ["a.b", "a=b", "\"q\"", "two words", ""] {
            let named: BTreeSet<String> = [odd.to_owned()].into();
            let none = BTreeSet::new();
            let ws = Path::new("/w");
            assert_eq!(
                codex_argv(&launch(ws), Path::new("/l"), &[], &named, &none),
                Err(odd.to_owned())
            );
            assert_eq!(
                codex_argv(&launch(ws), Path::new("/l"), &[], &none, &named),
                Err(odd.to_owned())
            );
        }
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
        assert!(joined.contains("--disallowedTools Bash,WebFetch,WebSearch,Agent,Task"));
        assert!(joined.contains("--permission-mode acceptEdits"));
        assert!(joined.contains("--settings /w/settings.json"));
        assert!(!joined.contains("bypassPermissions"));
        let shell = claude_argv(&launch(Path::new("/w")), Path::new("/s"), true);
        assert!(
            shell
                .join(" ")
                .contains("--disallowedTools WebFetch,WebSearch,Agent,Task")
        );
    }

    #[test]
    fn claude_init_requires_explicit_skills_plugins_and_mcp_arrays() {
        let mut complete = Observed::default();
        observe(
            "claude_code",
            r#"{"type":"system","subtype":"init","model":"claude-haiku-5-5","skills":[],"plugins":[],"mcp_servers":[]}"#,
            &mut complete,
        );
        assert!(verify_claude("haiku", &complete, &[], &[]).is_empty());

        let mut missing_plugins = Observed::default();
        observe(
            "claude_code",
            r#"{"type":"system","subtype":"init","model":"claude-haiku-5-5","skills":[],"mcp_servers":[]}"#,
            &mut missing_plugins,
        );
        let mut missing_mcp = Observed::default();
        observe(
            "claude_code",
            r#"{"type":"system","subtype":"init","model":"claude-haiku-5-5","skills":[],"plugins":[]}"#,
            &mut missing_mcp,
        );
        for (seen, omitted) in [
            (&missing_plugins, "Claude Code did not list its plugins"),
            (&missing_mcp, "Claude Code did not list its MCP servers"),
        ] {
            let problems = verify_claude("haiku", seen, &[], &[]);
            assert!(problems.contains(&omitted.to_owned()), "{problems:?}");
            assert!(!problems.is_empty(), "incomplete init must not verify");
        }
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

    #[test]
    fn codex_runtime_model_and_effort_are_session_bound_and_mismatch_is_detected() {
        let transcript = [
            serde_json::json!({"type":"session_meta","payload":{"id":"t-1"}}),
            serde_json::json!({"type":"turn_context","payload":{"model":"gpt-6-luna","effort":"high","turn_id":"turn-1"}}),
        ]
        .into_iter()
        .map(|event| event.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let runtime = codex_runtime(&transcript, "t-1").unwrap();
        assert_eq!(runtime.model, "gpt-6-luna");
        assert_eq!(runtime.effort, "high");
        assert!(verify_codex_runtime("gpt-6-luna", "high", Some(&runtime)).is_empty());
        assert_eq!(
            verify_codex_runtime("gpt-6.1-sol", "low", Some(&runtime)),
            [
                "model gpt-6-luna is not gpt-6.1-sol",
                "effort high is not low"
            ]
        );
        assert!(codex_runtime(&transcript, "t-other").is_none());
    }

    #[test]
    fn a_later_incomplete_codex_runtime_event_revokes_the_earlier_one() {
        let transcript = [
            serde_json::json!({"type":"session_meta","payload":{"id":"t-1"}}),
            serde_json::json!({"type":"turn_context","payload":{"model":"gpt-6-luna","effort":"high"}}),
            serde_json::json!({"type":"turn_context","payload":{"model":"gpt-6-luna"}}),
        ]
        .into_iter()
        .map(|event| event.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        assert!(codex_runtime(&transcript, "t-1").is_none());
        assert_eq!(
            verify_codex_runtime("gpt-6-luna", "high", None),
            ["Codex did not report its runtime model and effort"]
        );
    }
}
