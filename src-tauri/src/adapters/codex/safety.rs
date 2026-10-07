//! Authority-owned, pinned 0.159.0 inference configuration. Never client configuration.
use super::{CodecError, Result, fail};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

pub const INFERENCE_DISABLED_KEYS: [&str; 28] = [
    "features.apps",
    "features.code_mode",
    "features.code_mode_host",
    "features.code_mode_only",
    "features.context_management",
    "features.current_time_reminder",
    "features.deferred_executor",
    "features.enable_fanout",
    "features.goals",
    "features.hooks",
    "features.image_generation",
    "features.memories",
    "features.multi_agent",
    "features.multi_agent_v2",
    "features.plugins",
    "features.request_permissions_tool",
    "features.shell_snapshot",
    "features.shell_tool",
    "features.sleep_tool",
    "features.standalone_web_search",
    "features.token_budget",
    "features.tool_suggest",
    "features.unified_exec",
    "features.view_image",
    "cloud.skills.enabled",
    "skills.include_instructions",
    "tools.experimental_request_user_input.enabled",
    "tools.update_plan.enabled",
];
pub const PRODUCTION_METHODS: [&str; 11] = [
    "initialize",
    "initialized",
    "model/list",
    "config/read",
    "hooks/list",
    "thread/start",
    "thread/archive",
    "thread/unarchive",
    "thread/resume",
    "turn/start",
    "turn/interrupt",
];
const MAX_MCP: usize = 64;
pub const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_CATALOG_MODELS: usize = 64;
fn invalid() -> CodecError {
    CodecError {
        code: "inference_safety_unproven",
    }
}

/// Bare TOML path segments only. Reject unsupported names; never interpolate shell syntax.
pub fn safe_mcp_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
pub fn mcp_names(response: &Value, disabled: bool) -> Result<BTreeSet<String>> {
    let config = response
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    let Some(servers) = config.get("mcp_servers") else {
        return Ok(BTreeSet::new());
    };
    let servers = servers.as_object().ok_or_else(invalid)?;
    if servers.len() > MAX_MCP {
        return fail("mcp_bound");
    }
    let mut names = BTreeSet::new();
    for (name, server) in servers {
        if !safe_mcp_name(name)
            || !server.is_object()
            || (disabled && server.get("enabled") != Some(&Value::Bool(false)))
        {
            return fail("mcp_clamp_unproven");
        }
        names.insert(name.clone());
    }
    Ok(names)
}
/// Authority-written path only. The alphabet needs no TOML escaping; never interpolate
/// a client or provider path.
pub fn safe_catalog_path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|s| {
            s.len() <= 1024
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'/' | b'_' | b'-' | b'.'))
                && !s.split('/').any(|part| part == "..")
        })
}
/// Backend model metadata registers tools independently of feature flags: code mode,
/// multi-agent collaboration, asynchronous user input, clock and patch tools. The
/// authority presents every model with those selectors pinned off, so the inference
/// profile advertises no model-controllable tool at all.
pub fn inference_catalog(cache: &[u8]) -> Result<Vec<u8>> {
    if cache.len() > MAX_CATALOG_BYTES {
        return fail("catalog_bound");
    }
    let value: Value = serde_json::from_slice(cache).map_err(|_| invalid())?;
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if models.is_empty() || models.len() > MAX_CATALOG_MODELS {
        return fail("catalog_bound");
    }
    let mut presented = Vec::with_capacity(models.len());
    for model in models {
        let mut model = model.as_object().ok_or_else(invalid)?.clone();
        if !model
            .get("slug")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty() && s.len() <= 128)
        {
            return Err(invalid());
        }
        model.insert("tool_mode".into(), json!("direct"));
        model.insert("experimental_supported_tools".into(), json!([]));
        model.insert("multi_agent_version".into(), Value::Null);
        model.insert("apply_patch_tool_type".into(), Value::Null);
        presented.push(Value::Object(model));
    }
    serde_json::to_vec(&json!({ "models": presented })).map_err(|_| invalid())
}
/// Discovery has no catalog yet and no thread-capable IO boundary. The final generation
/// always receives the authority catalog.
pub fn process_args(names: &BTreeSet<String>, catalog: Option<&Path>) -> Result<Vec<String>> {
    if names.len() > MAX_MCP || names.iter().any(|n| !safe_mcp_name(n)) {
        return fail("mcp_bound");
    }
    let mut args = Vec::new();
    for key in INFERENCE_DISABLED_KEYS {
        args.extend(["-c".into(), format!("{key}=false")]);
    }
    args.extend(["-c".into(), "web_search=\"disabled\"".into()]);
    args.extend(["-c".into(), "agents.enabled=false".into()]);
    if let Some(catalog) = catalog {
        if !safe_catalog_path(catalog) {
            return fail("catalog_path");
        }
        args.extend([
            "-c".into(),
            format!("model_catalog_json=\"{}\"", catalog.display()),
        ]);
    }
    for name in names {
        args.extend(["-c".into(), format!("mcp_servers.{name}.enabled=false")]);
    }
    args.push("app-server".into());
    Ok(args)
}
/// Require reflected stable settings. Managed policy may keep the hooks engine enabled;
/// executable hook inventory is checked independently. The two tools.* overrides are
/// not reflected by 0.159.0 config/read and remain fixed at both encoder boundaries.
pub fn verify_config(response: &Value, catalog: &Path) -> Result<()> {
    let config = response.get("config").ok_or_else(invalid)?;
    if config["agents"]["enabled"] != false
        || config["model_catalog_json"].as_str() != catalog.to_str()
    {
        return fail("inference_config_unproven");
    }
    for key in INFERENCE_DISABLED_KEYS {
        if key == "features.hooks" || key.starts_with("tools.") {
            continue;
        }
        let mut value = config;
        for segment in key.split('.') {
            value = value.get(segment).ok_or_else(invalid)?;
        }
        if value != false {
            return fail("inference_config_unproven");
        }
    }
    if config["web_search"] != "disabled" {
        return fail("inference_config_unproven");
    }
    Ok(())
}
pub fn thread_config(names: &BTreeSet<String>) -> Result<Value> {
    process_args(names, None)?;
    let mut config = serde_json::Map::new();
    for key in INFERENCE_DISABLED_KEYS {
        config.insert(key.into(), Value::Bool(false));
    }
    config.insert("web_search".into(), json!("disabled"));
    config.insert("agents.enabled".into(), Value::Bool(false));
    config.insert("default_permissions".into(), json!(":read-only"));
    let servers: serde_json::Map<String, Value> = names
        .iter()
        .map(|n| (n.clone(), json!({"enabled": false})))
        .collect();
    config.insert("mcp_servers".into(), Value::Object(servers));
    Ok(Value::Object(config))
}
/// Unknown enablement is a rejection. No hook commands or source paths escape this boundary.
pub fn zero_external_hooks(response: &Value) -> Result<()> {
    let data = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if data.len() > 64 {
        return fail("hook_bound");
    }
    let mut total = 0;
    for entry in data {
        let hooks = entry
            .get("hooks")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        total += hooks.len();
        if total > 256
            || hooks
                .iter()
                .any(|h| h.get("enabled") != Some(&Value::Bool(false)))
        {
            return fail("external_hooks_enabled");
        }
    }
    Ok(())
}
pub fn verify_resume(response: &Value) -> Result<()> {
    if response["approvalPolicy"] != "never"
        || response["sandbox"]["type"] != "readOnly"
        || response["sandbox"]
            .get("networkAccess")
            .is_some_and(|v| v != false)
        || response
            .get("activePermissionProfile")
            .filter(|v| !v.is_null())
            .is_some_and(|v| v["id"] != ":read-only")
    {
        return fail("inference_attachment_unproven");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn immutable_launch_is_canonical_and_contains_hook_clamp() {
        let names = BTreeSet::from(["server_b".into(), "server-a".into()]);
        let args = process_args(&names, None).unwrap();
        assert_eq!(args.last().unwrap(), "app-server");
        assert_eq!(args.len(), 2 * (28 + 1 + 1 + 2) + 1);
        assert_eq!(&args[58..60], ["-c", "agents.enabled=false"]);
        let catalog = Path::new("/run/user/1000/wyss-console/codex-model-catalog.json");
        let pinned = process_args(&names, Some(catalog)).unwrap();
        assert_eq!(pinned.len(), args.len() + 2);
        assert_eq!(
            &pinned[60..62],
            [
                "-c",
                "model_catalog_json=\"/run/user/1000/wyss-console/codex-model-catalog.json\""
            ]
        );
        for unsafe_path in [
            "relative/catalog.json",
            "/tmp/a\"b.json",
            "/tmp/a b.json",
            "/tmp/../etc/catalog.json",
            "/tmp/$(x).json",
        ] {
            assert!(process_args(&names, Some(Path::new(unsafe_path))).is_err());
        }
        assert_eq!(
            args.iter().filter(|a| *a == "features.hooks=false").count(),
            1
        );
        assert_eq!(
            INFERENCE_DISABLED_KEYS
                .into_iter()
                .collect::<BTreeSet<_>>()
                .len(),
            28
        );
        for key in ["features.code_mode_host", "features.sleep_tool"] {
            assert!(INFERENCE_DISABLED_KEYS.contains(&key));
        }
        assert_eq!(
            &args[60..64],
            [
                "-c",
                "mcp_servers.server-a.enabled=false",
                "-c",
                "mcp_servers.server_b.enabled=false"
            ]
        );
        assert_eq!(
            PRODUCTION_METHODS,
            [
                "initialize",
                "initialized",
                "model/list",
                "config/read",
                "hooks/list",
                "thread/start",
                "thread/archive",
                "thread/unarchive",
                "thread/resume",
                "turn/start",
                "turn/interrupt"
            ]
        );
        assert!(!PRODUCTION_METHODS.contains(&"command/exec"));
    }
    #[test]
    fn unsafe_dynamic_paths_and_unknown_hook_state_fail_closed() {
        for name in ["", "a.b", "a\"", "a=b", "$(x)", "a\\b", "a b"] {
            assert!(!safe_mcp_name(name));
        }
        assert!(zero_external_hooks(&json!({"data":[{"hooks":[{"enabled":false}]}]})).is_ok());
        for h in [json!({}), json!({"enabled":true}), json!({"enabled":null})] {
            assert!(zero_external_hooks(&json!({"data":[{"hooks":[h]}]})).is_err());
        }
        assert!(
            mcp_names(
                &json!({"config":{"mcp_servers":{"a":{"enabled":false}}}}),
                true
            )
            .is_ok()
        );
        assert!(mcp_names(&json!({"config":{"mcp_servers":{"a":{}}}}), true).is_err());
    }
    #[test]
    fn authority_catalog_pins_every_metadata_tool_selector_and_fails_closed() {
        let cache = json!({"fetched_at":"t","etag":"e","models":[
            {"slug":"gpt-6.1-sol","tool_mode":"code_mode_only","shell_type":"unified_exec",
             "apply_patch_tool_type":"freeform","multi_agent_version":"v2",
             "experimental_supported_tools":["send_user_message_async","clock"],"priority":1},
            {"slug":"gpt-5.5","shell_type":"unified_exec","apply_patch_tool_type":"freeform"}
        ]});
        let out: Value =
            serde_json::from_slice(&inference_catalog(cache.to_string().as_bytes()).unwrap())
                .unwrap();
        assert_eq!(out.as_object().unwrap().len(), 1);
        let models = out["models"].as_array().unwrap();
        assert_eq!(models.len(), 2);
        for model in models {
            assert_eq!(model["tool_mode"], "direct");
            assert_eq!(model["experimental_supported_tools"], json!([]));
            assert!(model["multi_agent_version"].is_null());
            assert!(model["apply_patch_tool_type"].is_null());
        }
        // Unrelated provider metadata is carried unchanged.
        assert_eq!(models[0]["slug"], "gpt-6.1-sol");
        assert_eq!(models[0]["priority"], 1);
        assert_eq!(models[0]["shell_type"], "unified_exec");
        for bad in [
            "".to_owned(),
            "not json".to_owned(),
            json!({}).to_string(),
            json!({"models":[]}).to_string(),
            json!({"models":["gpt"]}).to_string(),
            json!({"models":[{"tool_mode":"direct"}]}).to_string(),
            json!({"models":[{"slug":""}]}).to_string(),
            json!({"models": vec![json!({"slug":"m"}); 65]}).to_string(),
        ] {
            assert!(inference_catalog(bad.as_bytes()).is_err(), "{bad}");
        }
        assert!(inference_catalog(&vec![b' '; MAX_CATALOG_BYTES + 1]).is_err());
        let catalog = Path::new("/run/user/1000/wyss-console/codex-model-catalog.json");
        let mut features = serde_json::Map::new();
        for key in INFERENCE_DISABLED_KEYS {
            if let Some(name) = key.strip_prefix("features.") {
                features.insert(name.into(), Value::Bool(false));
            }
        }
        let config = json!({"config":{"features":features,"cloud":{"skills":{"enabled":false}},
            "skills":{"include_instructions":false},"web_search":"disabled",
            "agents":{"enabled":false},"model_catalog_json":catalog.to_str().unwrap()}});
        assert!(verify_config(&config, catalog).is_ok());
        for (pointer, value) in [
            ("/config/agents/enabled", json!(true)),
            ("/config/agents", Value::Null),
            ("/config/model_catalog_json", Value::Null),
            ("/config/model_catalog_json", json!("/tmp/other.json")),
            ("/config/features/code_mode_host", json!(true)),
            ("/config/features/sleep_tool", json!(true)),
        ] {
            let mut bad = config.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(verify_config(&bad, catalog).is_err(), "{pointer}");
        }
    }
}

#[derive(Clone)]
pub(crate) struct TurnNotice {
    pub thread: super::CodexThreadId,
    pub turn: super::CodexTurnId,
    pub kind: NoticeKind,
}
#[derive(Clone)]
pub(crate) enum NoticeKind {
    Started,
    Delta(crate::contracts::turn::StreamKind, String),
    MessageCompleted,
    SummaryPart,
    Terminal(super::TurnOutcome),
}
/// Fail closed on execution surfaces before ordinary notification processing.
/// No provider body, user-message content, raw reasoning or free-form error escapes.
pub(crate) fn external_action_candidate(method: &str, p: &Value) -> bool {
    let allowed_item = |i: &Value| {
        matches!(
            i["type"].as_str(),
            Some("userMessage" | "agentMessage" | "reasoning" | "plan")
        )
    };
    method.starts_with("hook/")
        || method.starts_with("mcpServer/")
        || matches!(method, "item/started" | "item/completed") && !allowed_item(&p["item"])
        || method == "turn/diff/updated" && p["diff"].as_str() != Some("")
        || matches!(method, "turn/started" | "turn/completed")
            && p["turn"]["items"]
                .as_array()
                .is_some_and(|a| a.iter().any(|i| !allowed_item(i)))
}
pub(crate) fn notification(m: &super::WireMessage) -> Result<Option<TurnNotice>> {
    use super::*;
    use crate::contracts::turn::StreamKind;
    if m.kind() == WireKind::ServerRequest {
        return fail("inference_isolation_violation");
    }
    if m.kind() != WireKind::Notification {
        return Ok(None);
    }
    let method = m.method().ok_or_else(invalid)?;
    let p = m.params().unwrap_or(&Value::Null);
    if external_action_candidate(method, p) {
        return fail("inference_isolation_violation");
    }
    if method == "item/reasoning/textDelta" {
        return Ok(None);
    }
    if method == "item/reasoning/summaryPartAdded" {
        let thread = CodexThreadId::parse(p["threadId"].as_str().ok_or_else(invalid)?)?;
        let turn = CodexTurnId::parse(p["turnId"].as_str().ok_or_else(invalid)?)?;
        CodexItemId::parse(p["itemId"].as_str().ok_or_else(invalid)?)?;
        if p["summaryIndex"].as_i64().is_none_or(|i| i < 0) {
            return fail("invalid_summary_index");
        }
        return Ok(Some(TurnNotice {
            thread,
            turn,
            kind: NoticeKind::SummaryPart,
        }));
    }
    match normalize_inbound(m, None)? {
        CodexObservation::Turn {
            thread_id,
            turn_id,
            outcome,
            ..
        } => Ok(Some(TurnNotice {
            thread: thread_id,
            turn: turn_id,
            kind: if outcome == TurnOutcome::InProgress {
                NoticeKind::Started
            } else {
                NoticeKind::Terminal(outcome)
            },
        })),
        CodexObservation::Content {
            thread_id,
            turn_id,
            kind,
            text,
            ..
        } => {
            let kind = match kind {
                ContentKind::AgentMessage => StreamKind::AssistantMessageDelta,
                ContentKind::ReasoningSummary => StreamKind::ReasoningSummaryDelta,
                ContentKind::Plan => return Ok(None),
                _ => return fail("inference_isolation_violation"),
            };
            Ok(Some(TurnNotice {
                thread: thread_id,
                turn: turn_id,
                kind: NoticeKind::Delta(kind, text),
            }))
        }
        CodexObservation::Item(i)
            if i.kind == ItemKind::AgentMessage && i.stage == ItemStage::Completed =>
        {
            Ok(Some(TurnNotice {
                thread: i.thread_id,
                turn: i.turn_id,
                kind: NoticeKind::MessageCompleted,
            }))
        }
        CodexObservation::Item(i)
            if matches!(
                i.kind,
                ItemKind::UserMessage
                    | ItemKind::AgentMessage
                    | ItemKind::ReasoningSummary
                    | ItemKind::Plan
            ) =>
        {
            Ok(None)
        }
        CodexObservation::Session { .. } => Ok(None),
        CodexObservation::SuppressedPrivateReasoning => Ok(None),
        CodexObservation::UnknownNotification { .. }
            if matches!(
                method,
                "remoteControl/status/changed"
                    | "account/updated"
                    | "account/rateLimits/updated"
                    | "thread/status/changed"
                    | "thread/tokenUsage/updated"
                    | "thread/archived"
                    | "thread/unarchived"
                    | "turn/diff/updated"
                    | "turn/plan/updated"
                    | "item/plan/delta"
            ) =>
        {
            Ok(None)
        }
        _ => fail("inference_isolation_violation"),
    }
}
