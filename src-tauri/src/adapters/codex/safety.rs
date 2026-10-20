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
/// The local-tools generation additionally proves its managed requirements.
pub const LOCAL_TOOLS_METHODS: [&str; 12] = [
    "initialize",
    "initialized",
    "model/list",
    "config/read",
    "configRequirements/read",
    "hooks/list",
    "thread/start",
    "thread/archive",
    "thread/unarchive",
    "thread/resume",
    "turn/start",
    "turn/interrupt",
];
/// The single feature the local-tools profile turns on: the one-shot command tool.
const LOCAL_TOOLS_ENABLED_KEY: &str = "features.shell_tool";
/// Writes of an approved command are confined to the workspace itself: the runtime's
/// default extra writable roots (`/tmp` and `$TMPDIR`) and network access are removed.
const LOCAL_TOOLS_SANDBOX: [(&str, bool); 3] = [
    ("sandbox_workspace_write.exclude_slash_tmp", true),
    ("sandbox_workspace_write.exclude_tmpdir_env_var", true),
    ("sandbox_workspace_write.network_access", false),
];
/// Managed requirements of the isolated generation. `-c features.unified_exec=false` alone
/// is overridden by the runtime; only a managed requirement removes `write_stdin` and
/// persistent processes.
pub const LOCAL_TOOLS_REQUIREMENTS: &str = "[features]\nunified_exec = false\n";
pub const LOCAL_TOOLS_MANAGED_CONFIG: &str = "check_for_update_on_startup = false\n";
const MAX_MCP: usize = 64;
/// Project roots one generation forces untrusted: the workspace and the ancestors that
/// hold a repository marker. A deeper nesting of repositories keeps the profile off.
pub const MAX_TRUST_ROOTS: usize = 8;
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
    catalog(cache, false)
}
/// Same pins, except that the backend's own patch tool type is kept: the local-tools
/// profile offers `apply_patch` next to the one-shot command tool, and nothing else.
pub fn local_tools_catalog(cache: &[u8]) -> Result<Vec<u8>> {
    catalog(cache, true)
}
fn catalog(cache: &[u8], patch_tool: bool) -> Result<Vec<u8>> {
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
        if !patch_tool {
            model.insert("apply_patch_tool_type".into(), Value::Null);
        } else if !matches!(
            model.get("apply_patch_tool_type"),
            Some(Value::Null | Value::String(_)) | None
        ) {
            return Err(invalid());
        }
        presented.push(Value::Object(model));
    }
    serde_json::to_vec(&json!({ "models": presented })).map_err(|_| invalid())
}
/// Discovery has no catalog yet and no thread-capable IO boundary. The final generation
/// always receives the authority catalog.
pub fn process_args(names: &BTreeSet<String>, catalog: Option<&Path>) -> Result<Vec<String>> {
    launch_args(names, catalog, false)
}
/// The inference clamps with the one-shot command tool enabled and the PTY variant of the
/// persistent command tool pinned off. No client, provider or workspace text is interpolated.
pub fn local_tools_args(names: &BTreeSet<String>, catalog: Option<&Path>) -> Result<Vec<String>> {
    launch_args(names, catalog, true)
}
fn launch_args(
    names: &BTreeSet<String>,
    catalog: Option<&Path>,
    local_tools: bool,
) -> Result<Vec<String>> {
    if names.len() > MAX_MCP || names.iter().any(|n| !safe_mcp_name(n)) {
        return fail("mcp_bound");
    }
    let mut args = Vec::new();
    for key in INFERENCE_DISABLED_KEYS {
        let value = local_tools && key == LOCAL_TOOLS_ENABLED_KEY;
        args.extend(["-c".into(), format!("{key}={value}")]);
    }
    if local_tools {
        args.extend(["-c".into(), "features.unified_exec_tty=false".into()]);
        for (key, value) in LOCAL_TOOLS_SANDBOX {
            args.extend(["-c".into(), format!("{key}={value}")]);
        }
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
    reflected(response, catalog, false)
}
/// The same reflection for the local-tools generation: every clamp off, the command tool
/// on, and no MCP server configured at all in its private home.
pub fn verify_local_tools_config(response: &Value, catalog: &Path) -> Result<()> {
    reflected(response, catalog, true)?;
    let config = response.get("config").ok_or_else(invalid)?;
    if config["features"]["unified_exec_tty"] != false
        || config["sandbox_workspace_write"]
            != json!({"writable_roots": [], "network_access": false, "exclude_tmpdir_env_var": true, "exclude_slash_tmp": true})
        || config
            .get("mcp_servers")
            .is_some_and(|m| m.as_object().is_none_or(|m| !m.is_empty()))
    {
        return fail("local_tools_config_unproven");
    }
    Ok(())
}
fn reflected(response: &Value, catalog: &Path, local_tools: bool) -> Result<()> {
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
        if *value != Value::Bool(local_tools && key == LOCAL_TOOLS_ENABLED_KEY) {
            return fail("inference_config_unproven");
        }
    }
    if config["web_search"] != "disabled" {
        return fail("inference_config_unproven");
    }
    Ok(())
}
/// The runtime itself must report the managed requirement that removes the persistent
/// command tool. Anything else leaves the local-tools profile unproven.
pub fn verify_requirements(response: &Value) -> Result<()> {
    if response["requirements"]["featureRequirements"]["unified_exec"] != false {
        return fail("local_tools_requirements_unproven");
    }
    Ok(())
}
pub fn thread_config(names: &BTreeSet<String>) -> Result<Value> {
    process_args(names, None)?;
    let mut config = clamps(names, false);
    config.insert("default_permissions".into(), json!(":read-only"));
    Ok(Value::Object(config))
}
/// Per-thread overrides of the local-tools profile. Every project root is forced untrusted
/// here, as JSON values: a workspace path is never interpolated into TOML or a shell.
pub fn local_tools_thread_config(names: &BTreeSet<String>, roots: &[String]) -> Result<Value> {
    local_tools_args(names, None)?;
    if roots.is_empty()
        || roots.len() > MAX_TRUST_ROOTS
        || roots.iter().any(|r| !Path::new(r).is_absolute())
    {
        return fail("trust_roots");
    }
    let mut config = clamps(names, true);
    config.insert("features.unified_exec_tty".into(), Value::Bool(false));
    for (key, value) in LOCAL_TOOLS_SANDBOX {
        config.insert(key.into(), Value::Bool(value));
    }
    let projects: serde_json::Map<String, Value> = roots
        .iter()
        .map(|r| (r.clone(), json!({"trust_level": "untrusted"})))
        .collect();
    config.insert("projects".into(), Value::Object(projects));
    Ok(Value::Object(config))
}
fn clamps(names: &BTreeSet<String>, local_tools: bool) -> serde_json::Map<String, Value> {
    let mut config = serde_json::Map::new();
    for key in INFERENCE_DISABLED_KEYS {
        config.insert(
            key.into(),
            Value::Bool(local_tools && key == LOCAL_TOOLS_ENABLED_KEY),
        );
    }
    config.insert("web_search".into(), json!("disabled"));
    config.insert("agents.enabled".into(), Value::Bool(false));
    let servers: serde_json::Map<String, Value> = names
        .iter()
        .map(|n| (n.clone(), json!({"enabled": false})))
        .collect();
    config.insert("mcp_servers".into(), Value::Object(servers));
    config
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

/// A local-tools thread is attached only under the mediated posture: every action asks,
/// writes are confined to the workspace, no network and no extra writable root.
pub fn verify_local_tools_thread(response: &Value) -> Result<()> {
    let sandbox = &response["sandbox"];
    if response["approvalPolicy"] != "untrusted"
        || sandbox["type"] != "workspaceWrite"
        || sandbox["networkAccess"] != false
        || sandbox["excludeSlashTmp"] != true
        || sandbox["excludeTmpdirEnvVar"] != true
        || sandbox["writableRoots"]
            .as_array()
            .is_none_or(|roots| !roots.is_empty())
        || response
            .get("activePermissionProfile")
            .is_some_and(|v| !v.is_null())
    {
        return fail("local_tools_attachment_unproven");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_tools_launch_enables_only_the_command_tool_and_never_interpolates_paths() {
        let names = BTreeSet::new();
        let inference = process_args(&names, None).unwrap();
        let local = local_tools_args(&names, None).unwrap();
        assert_eq!(local.len(), inference.len() + 8);
        let differing: Vec<_> = inference
            .iter()
            .filter(|a| !local.contains(a))
            .cloned()
            .collect();
        assert_eq!(differing, ["features.shell_tool=false"]);
        let added: Vec<_> = local
            .iter()
            .filter(|a| !inference.contains(a))
            .cloned()
            .collect();
        assert_eq!(
            added,
            [
                "features.shell_tool=true",
                "features.unified_exec_tty=false",
                "sandbox_workspace_write.exclude_slash_tmp=true",
                "sandbox_workspace_write.exclude_tmpdir_env_var=true",
                "sandbox_workspace_write.network_access=false"
            ]
        );
        for kept in [
            "features.unified_exec=false",
            "features.hooks=false",
            "features.multi_agent=false",
            "features.code_mode_host=false",
            "agents.enabled=false",
            "web_search=\"disabled\"",
        ] {
            assert!(local.iter().any(|a| a == kept), "{kept}");
        }
        assert_eq!(local.last().unwrap(), "app-server");
        let roots = vec![
            "/work/re\"po = {x}".to_owned(),
            "/work/re\"po = {x}/ws".to_owned(),
        ];
        let config = local_tools_thread_config(&names, &roots).unwrap();
        assert_eq!(config["features.shell_tool"], true);
        assert_eq!(config["features.unified_exec"], false);
        assert_eq!(config["features.unified_exec_tty"], false);
        assert_eq!(config["sandbox_workspace_write.exclude_slash_tmp"], true);
        assert_eq!(
            config["sandbox_workspace_write.exclude_tmpdir_env_var"],
            true
        );
        assert_eq!(config["sandbox_workspace_write.network_access"], false);
        assert_eq!(config["agents.enabled"], false);
        assert!(config.get("default_permissions").is_none());
        assert_eq!(
            config["projects"],
            json!({roots[0].clone(): {"trust_level": "untrusted"}, roots[1].clone(): {"trust_level": "untrusted"}})
        );
        for bad in [
            vec![],
            vec!["relative".to_owned()],
            vec!["/a".to_owned(); MAX_TRUST_ROOTS + 1],
        ] {
            assert!(local_tools_thread_config(&names, &bad).is_err());
        }
        assert_eq!(
            LOCAL_TOOLS_REQUIREMENTS,
            "[features]\nunified_exec = false\n"
        );
        assert_eq!(LOCAL_TOOLS_METHODS.len(), PRODUCTION_METHODS.len() + 1);
        assert!(
            PRODUCTION_METHODS
                .iter()
                .all(|m| LOCAL_TOOLS_METHODS.contains(m))
        );
    }
    #[test]
    fn local_tools_attestation_requires_every_reflected_and_managed_fact() {
        let catalog = Path::new("/run/user/1000/wyss-console/codex-local-tools-catalog.json");
        let mut features = serde_json::Map::new();
        let mut config = serde_json::Map::new();
        for key in INFERENCE_DISABLED_KEYS {
            if let Some(("features", name)) = key.split_once('.') {
                features.insert(name.into(), Value::Bool(name == "shell_tool"));
            }
        }
        features.insert("unified_exec_tty".into(), Value::Bool(false));
        config.insert("features".into(), Value::Object(features));
        config.insert("cloud".into(), json!({"skills": {"enabled": false}}));
        config.insert("skills".into(), json!({"include_instructions": false}));
        config.insert("agents".into(), json!({"enabled": false}));
        config.insert("web_search".into(), json!("disabled"));
        config.insert("mcp_servers".into(), json!({}));
        config.insert(
            "sandbox_workspace_write".into(),
            json!({"writable_roots": [], "network_access": false, "exclude_tmpdir_env_var": true, "exclude_slash_tmp": true}),
        );
        config.insert(
            "model_catalog_json".into(),
            json!(catalog.to_str().unwrap()),
        );
        let good = json!({ "config": config });
        verify_local_tools_config(&good, catalog).unwrap();
        // The inference attestation refuses the same runtime, and the reverse.
        assert!(verify_config(&good, catalog).is_err());
        let broken = |change: &dyn Fn(&mut Value)| {
            let mut v = good.clone();
            change(&mut v["config"]);
            verify_local_tools_config(&v, catalog).is_err()
        };
        let changes: [&dyn Fn(&mut Value); 13] = [
            &|c| c["sandbox_workspace_write"]["exclude_slash_tmp"] = json!(false),
            &|c| c["sandbox_workspace_write"]["exclude_tmpdir_env_var"] = json!(false),
            &|c| c["sandbox_workspace_write"]["network_access"] = json!(true),
            &|c| c["sandbox_workspace_write"]["writable_roots"] = json!(["/"]),
            &|c| c["features"]["shell_tool"] = json!(false),
            &|c| c["features"]["unified_exec"] = json!(true),
            &|c| c["features"]["unified_exec_tty"] = json!(true),
            &|c| c["features"]["multi_agent"] = json!(true),
            &|c| c["features"]["code_mode_host"] = json!(true),
            &|c| c["agents"]["enabled"] = json!(true),
            &|c| c["web_search"] = json!("live"),
            &|c| c["mcp_servers"] = json!({"x": {"enabled": false}}),
            &|c| c["model_catalog_json"] = json!("/other/catalog.json"),
        ];
        for (n, change) in changes.iter().enumerate() {
            assert!(broken(*change), "change {n}");
        }
        verify_requirements(&json!({"requirements": {"featureRequirements": {"unified_exec": false, "hooks": true}}})).unwrap();
        for bad in [
            json!({"requirements": {"featureRequirements": {"unified_exec": true}}}),
            json!({"requirements": {"featureRequirements": {}}}),
            json!({"requirements": {"featureRequirements": null}}),
            json!({"requirements": null}),
            json!({}),
        ] {
            assert!(verify_requirements(&bad).is_err(), "{bad}");
        }
        let thread = json!({"approvalPolicy": "untrusted", "sandbox": {"type": "workspaceWrite", "writableRoots": [], "networkAccess": false, "excludeTmpdirEnvVar": true, "excludeSlashTmp": true}, "activePermissionProfile": null});
        verify_local_tools_thread(&thread).unwrap();
        assert!(verify_resume(&thread).is_err());
        let changes: [&dyn Fn(&mut Value); 9] = [
            &|t| t["sandbox"]["excludeSlashTmp"] = json!(false),
            &|t| t["sandbox"]["excludeTmpdirEnvVar"] = json!(false),
            &|t| t["approvalPolicy"] = json!("never"),
            &|t| t["approvalPolicy"] = json!("on-request"),
            &|t| t["sandbox"]["type"] = json!("dangerFullAccess"),
            &|t| t["sandbox"]["networkAccess"] = json!(true),
            &|t| t["sandbox"]["writableRoots"] = json!(["/"]),
            &|t| {
                t["sandbox"]
                    .as_object_mut()
                    .unwrap()
                    .remove("writableRoots");
            },
            &|t| t["activePermissionProfile"] = json!({"id": ":workspace"}),
        ];
        for (n, change) in changes.iter().enumerate() {
            let mut t = thread.clone();
            change(&mut t);
            assert!(verify_local_tools_thread(&t).is_err(), "thread change {n}");
        }
    }
    #[test]
    fn local_tools_catalog_keeps_only_the_patch_tool_selector() {
        let cache = serde_json::to_vec(&json!({"models": [
            {"slug": "a", "tool_mode": "code_mode_only", "experimental_supported_tools": ["x"], "multi_agent_version": "v2", "apply_patch_tool_type": "freeform"},
            {"slug": "b", "tool_mode": "code_mode_only"}
        ]}))
        .unwrap();
        let local: Value = serde_json::from_slice(&local_tools_catalog(&cache).unwrap()).unwrap();
        let inference: Value = serde_json::from_slice(&inference_catalog(&cache).unwrap()).unwrap();
        for presented in [&local, &inference] {
            for model in presented["models"].as_array().unwrap() {
                assert_eq!(model["tool_mode"], "direct");
                assert_eq!(model["experimental_supported_tools"], json!([]));
                assert!(model["multi_agent_version"].is_null());
            }
        }
        assert_eq!(local["models"][0]["apply_patch_tool_type"], "freeform");
        assert!(local["models"][1].get("apply_patch_tool_type").is_none());
        assert!(inference["models"][0]["apply_patch_tool_type"].is_null());
        let odd = serde_json::to_vec(
            &json!({"models": [{"slug": "a", "apply_patch_tool_type": {"x": 1}}]}),
        )
        .unwrap();
        assert!(local_tools_catalog(&odd).is_err());
        assert!(local_tools_catalog(b"{}").is_err());
    }
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
// Deliberately no Debug on any notice type: commands, output and diffs travel in them.
#[derive(Clone)]
pub(crate) enum NoticeKind {
    Started,
    Delta(crate::contracts::turn::StreamKind, String),
    MessageCompleted,
    SummaryPart,
    Terminal(super::TurnOutcome),
    /// Local-tools profile only, from here on. The provider announced a command it wants
    /// to run. Nothing has executed.
    CommandProposed(CommandProposal),
    /// The provider announced the change set it wants to apply. Nothing was written.
    FileProposed(FileProposal),
    /// The provider asks whether the announced action may happen.
    ApprovalRequested(ApprovalRequest),
    /// Output of an action. Legal only after the authority accepted that exact item.
    Output {
        item: super::CodexItemId,
        kind: ActionKind,
        delta: String,
    },
    ItemEnded {
        item: super::CodexItemId,
        kind: ActionKind,
        end: ItemEnd,
        /// Complete output the provider reports with a finished command, if any.
        output: Option<String>,
    },
    /// The provider no longer waits for an answer to this request.
    RequestResolved(super::CodexRequestId),
    /// The provider reports applied file changes in this turn.
    DiffUpdated,
    /// The provider says it rerouted this turn to another model (ADR 0048). It carries no
    /// tool, command or content. The turn controller accepts it only for the running turn.
    Rerouted(Reroute),
}
/// What a provider's `model/rerouted` notice says, after the classifier accepted its shape.
/// The reason is a closed word; the provider's text is never kept.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Reroute {
    pub from: String,
    pub to: String,
    pub reason: crate::contracts::binding::RerouteReason,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionKind {
    Command,
    File,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemEnd {
    Completed,
    Failed,
    Declined,
}
#[derive(Clone)]
pub(crate) struct CommandProposal {
    pub item: super::CodexItemId,
    pub command: String,
    pub cwd: String,
    pub actions: Vec<String>,
}
#[derive(Clone)]
pub(crate) struct ProposedChange {
    pub path: String,
    pub kind: crate::contracts::approval::FileChangeKind,
    pub move_path: Option<String>,
    pub diff: String,
}
#[derive(Clone)]
pub(crate) struct FileProposal {
    pub item: super::CodexItemId,
    pub changes: Vec<ProposedChange>,
}
/// Decisions the provider lets this request be answered with. The authority only ever
/// emits `accept`, `decline` or `cancel`, and only one that is offered.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Offered {
    pub accept: bool,
    pub decline: bool,
    pub cancel: bool,
}
#[derive(Clone)]
pub(crate) struct ApprovalRequest {
    pub request: super::CodexRequestId,
    pub item: super::CodexItemId,
    pub kind: ActionKind,
    /// Command and working directory as the request itself states them, when it does.
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub actions: Vec<String>,
    pub offered: Offered,
}
/// Exact parameter sets of the two approval requests of the pinned 0.159.0 protocol,
/// experimental fields included. Anything else is an unknown decision surface.
const COMMAND_APPROVAL_FIELDS: [&str; 16] = [
    "threadId",
    "turnId",
    "itemId",
    "startedAtMs",
    "approvalId",
    "command",
    "commandActions",
    "cwd",
    "environmentId",
    "kind",
    "networkApprovalContext",
    "proposedExecpolicyAmendment",
    "proposedNetworkPolicyAmendments",
    "reason",
    "additionalPermissions",
    "availableDecisions",
];
const FILE_APPROVAL_FIELDS: [&str; 6] = [
    "threadId",
    "turnId",
    "itemId",
    "startedAtMs",
    "reason",
    "grantRoot",
];
const ACTION_LABEL_BYTES: usize = crate::contracts::approval::MAX_PARSED_ACTION_BYTES;
/// `model/rerouted` (ModelReroutedNotification): `threadId`, `turnId`, `fromModel`, `toModel`
/// and `reason`, and nothing else. Models are provider identifiers within the descriptor's
/// bounds; the reason is mapped to a closed word. Whether it belongs to the running turn is the
/// turn controller's judgement, not this function's.
fn reroute(p: &Value) -> Result<TurnNotice> {
    use crate::contracts::binding::RerouteReason;
    const KEYS: [&str; 5] = ["threadId", "turnId", "fromModel", "toModel", "reason"];
    let o = p.as_object().ok_or_else(invalid)?;
    if o.len() != KEYS.len() || KEYS.iter().any(|k| !o.contains_key(*k)) {
        return fail("invalid_reroute");
    }
    let word = |k: &str, most: usize| -> Result<&str> {
        let t = o[k].as_str().ok_or_else(invalid)?;
        (!t.is_empty() && t.len() <= most && t.chars().all(|c| c.is_ascii_graphic()))
            .then_some(t)
            .ok_or(CodecError {
                code: "invalid_reroute",
            })
    };
    let reason = match word("reason", 64)? {
        "highRiskCyberActivity" => RerouteReason::HighRiskCyberActivity,
        _ => RerouteReason::Unrecognised,
    };
    Ok(TurnNotice {
        thread: super::CodexThreadId::parse(word("threadId", 256)?)?,
        turn: super::CodexTurnId::parse(word("turnId", 256)?)?,
        kind: NoticeKind::Rerouted(Reroute {
            from: word("fromModel", 256)?.to_owned(),
            to: word("toModel", 256)?.to_owned(),
            reason,
        }),
    })
}
fn violation<T>() -> Result<T> {
    fail("inference_isolation_violation")
}
/// Everything malformed on the mediated surface is a violation, never a protocol detail.
fn guard<T>(r: Result<T>) -> Result<T> {
    r.or_else(|_| violation())
}
fn pair(p: &Value) -> Result<(super::CodexThreadId, super::CodexTurnId)> {
    let text = |k: &str| p[k].as_str().ok_or_else(invalid);
    guard((|| {
        Ok((
            super::CodexThreadId::parse(text("threadId")?)?,
            super::CodexTurnId::parse(text("turnId")?)?,
        ))
    })())
}
fn item_id(v: &Value) -> Result<super::CodexItemId> {
    guard(super::CodexItemId::parse(v.as_str().unwrap_or_default()))
}
fn present(p: &Value, key: &str) -> bool {
    p.get(key).is_some_and(|v| !v.is_null())
}
/// Friendly labels of the provider's best-effort parse. Display only: the fingerprint and
/// the decision cover the exact command, never these.
fn action_labels(v: Option<&Value>) -> Result<Vec<String>> {
    let Some(list) = v.filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let mut labels = Vec::new();
    for action in list.as_array().ok_or_else(invalid)? {
        let kind = action["type"].as_str().ok_or_else(invalid)?;
        if kind.is_empty() || kind.len() > 32 || !kind.bytes().all(|c| c.is_ascii_alphanumeric()) {
            return Err(invalid());
        }
        let full = action["command"]
            .as_str()
            .map(|command| format!("{kind}: {command}"))
            .filter(|label| label.len() <= ACTION_LABEL_BYTES && !label.contains('\0'));
        labels.push(full.unwrap_or_else(|| kind.to_owned()));
    }
    Ok(labels)
}
fn proposed_changes(v: &Value) -> Result<Vec<ProposedChange>> {
    use crate::contracts::approval::FileChangeKind;
    let mut changes = Vec::new();
    for c in v.as_array().ok_or_else(invalid)? {
        let text = |v: &Value| v.as_str().map(str::to_owned).ok_or_else(invalid);
        let kind = c["kind"].as_object().ok_or_else(invalid)?;
        if kind.keys().any(|k| k != "type" && k != "move_path") {
            return Err(invalid());
        }
        let move_path = kind
            .get("move_path")
            .filter(|v| !v.is_null())
            .map(text)
            .transpose()?;
        let kind = match (kind.get("type").and_then(Value::as_str), &move_path) {
            (Some("add"), None) => FileChangeKind::Add,
            (Some("delete"), None) => FileChangeKind::Delete,
            (Some("update"), None) => FileChangeKind::Update,
            (Some("update"), Some(_)) => FileChangeKind::Move,
            _ => return Err(invalid()),
        };
        changes.push(ProposedChange {
            path: text(&c["path"])?,
            kind,
            move_path,
            diff: text(&c["diff"])?,
        });
    }
    Ok(changes)
}
/// One of the two approval requests the local-tools profile mediates, reduced to what the
/// authority decides on. Every other request, and every request that asks for more than
/// the one announced action, is a violation: it is never answered.
fn approval_request(m: &super::WireMessage) -> Result<TurnNotice> {
    use super::{ApprovalKind, approval_kind};
    let kind = match m.method().and_then(approval_kind) {
        Some(ApprovalKind::Command) => ActionKind::Command,
        Some(ApprovalKind::FileChange) => ActionKind::File,
        _ => return violation(),
    };
    let p = m.params().unwrap_or(&Value::Null);
    let fields: &[&str] = match kind {
        ActionKind::Command => &COMMAND_APPROVAL_FIELDS,
        ActionKind::File => &FILE_APPROVAL_FIELDS,
    };
    if p.as_object()
        .is_none_or(|o| o.keys().any(|k| !fields.contains(&k.as_str())))
    {
        return violation();
    }
    // Permission escalation, network approvals, callbacks of another terminal and
    // persistent write grants are not this action.
    for forbidden in [
        "additionalPermissions",
        "networkApprovalContext",
        "approvalId",
        "grantRoot",
    ] {
        if present(p, forbidden) {
            return violation();
        }
    }
    // The pinned runtime names its own process environment `local`. Any other environment
    // is somewhere this generation was never isolated for.
    if p.get("proposedNetworkPolicyAmendments")
        .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
        || p.get("kind").is_some_and(|k| k != "command")
        || p.get("environmentId")
            .is_some_and(|e| !e.is_null() && e != "local")
    {
        return violation();
    }
    let (thread, turn) = pair(p)?;
    let offered = match p.get("availableDecisions").filter(|v| !v.is_null()) {
        // The pinned protocol defines the full set when the request does not narrow it.
        None => Offered {
            accept: true,
            decline: true,
            cancel: true,
        },
        Some(list) => {
            let list = guard(list.as_array().ok_or_else(invalid))?;
            let has = |name: &str| list.iter().any(|d| d == name);
            Offered {
                accept: has("accept"),
                decline: has("decline"),
                cancel: has("cancel"),
            }
        }
    };
    let text = |k: &str| {
        p.get(k)
            .filter(|v| !v.is_null())
            .map(|v| v.as_str().map(str::to_owned).ok_or_else(invalid))
            .transpose()
    };
    Ok(TurnNotice {
        thread,
        turn,
        kind: NoticeKind::ApprovalRequested(ApprovalRequest {
            request: m.id().ok_or_else(invalid)?.clone(),
            item: item_id(&p["itemId"])?,
            kind,
            command: guard(text("command"))?,
            cwd: guard(text("cwd"))?,
            actions: guard(action_labels(p.get("commandActions")))?,
            offered,
        }),
    })
}
/// Notifications of the mediated surface. `None` means the frame is not part of it.
fn local_action(method: &str, p: &Value) -> Result<Option<TurnNotice>> {
    let notice = |kind| {
        let (thread, turn) = pair(p)?;
        Ok(Some(TurnNotice { thread, turn, kind }))
    };
    match method {
        "turn/diff/updated" if p["diff"].as_str() != Some("") => notice(NoticeKind::DiffUpdated),
        "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" => {
            notice(NoticeKind::Output {
                item: item_id(&p["itemId"])?,
                kind: if method == "item/fileChange/outputDelta" {
                    ActionKind::File
                } else {
                    ActionKind::Command
                },
                delta: guard(p["delta"].as_str().map(str::to_owned).ok_or_else(invalid))?,
            })
        }
        "item/fileChange/patchUpdated" => notice(NoticeKind::FileProposed(FileProposal {
            item: item_id(&p["itemId"])?,
            changes: guard(proposed_changes(&p["changes"]))?,
        })),
        "item/started" | "item/completed" => {
            let item = &p["item"];
            let kind = match item["type"].as_str() {
                Some("commandExecution") => ActionKind::Command,
                Some("fileChange") => ActionKind::File,
                _ => return Ok(None),
            };
            let id = item_id(&item["id"])?;
            let status = item["status"].as_str();
            if method == "item/completed" {
                let end = match status {
                    Some("completed") => ItemEnd::Completed,
                    Some("failed") => ItemEnd::Failed,
                    Some("declined") => ItemEnd::Declined,
                    _ => return violation(),
                };
                let output = item
                    .get("aggregatedOutput")
                    .and_then(Value::as_str)
                    .filter(|o| kind == ActionKind::Command && !o.is_empty())
                    .map(str::to_owned);
                return notice(NoticeKind::ItemEnded {
                    item: id,
                    kind,
                    end,
                    output,
                });
            }
            // An item that starts in any other state already ran without being asked.
            if status != Some("inProgress") {
                return violation();
            }
            if kind == ActionKind::File {
                return notice(NoticeKind::FileProposed(FileProposal {
                    item: id,
                    changes: guard(proposed_changes(&item["changes"]))?,
                }));
            }
            // A user shell command or input to a running terminal is not a proposal.
            if item
                .get("source")
                .is_some_and(|s| s != "agent" && s != "unifiedExecStartup")
            {
                return violation();
            }
            let text = |k: &str| guard(item[k].as_str().map(str::to_owned).ok_or_else(invalid));
            notice(NoticeKind::CommandProposed(CommandProposal {
                item: id,
                command: text("command")?,
                cwd: text("cwd")?,
                actions: guard(action_labels(item.get("commandActions")))?,
            }))
        }
        _ => Ok(None),
    }
}
/// Fail closed on execution surfaces before ordinary notification processing.
/// No provider body, user-message content, raw reasoning or free-form error escapes.
pub(crate) fn external_action_candidate(method: &str, p: &Value) -> bool {
    action_candidate(method, p, false)
}
fn action_candidate(method: &str, p: &Value, local: bool) -> bool {
    let allowed_item = |i: &Value| {
        matches!(
            i["type"].as_str(),
            Some("userMessage" | "agentMessage" | "reasoning" | "plan")
        ) || (local && matches!(i["type"].as_str(), Some("commandExecution" | "fileChange")))
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
/// Inference profile: no server request and no action surface exists at all.
pub(crate) fn notification(m: &super::WireMessage) -> Result<Option<TurnNotice>> {
    classify(m, false)
}
/// Local-tools profile: the same classifier plus the two mediated approval requests and
/// the command and file-change items they are about. Nothing else is added.
pub(crate) fn local_notification(m: &super::WireMessage) -> Result<Option<TurnNotice>> {
    classify(m, true)
}
fn classify(m: &super::WireMessage, local: bool) -> Result<Option<TurnNotice>> {
    use super::*;
    use crate::contracts::turn::StreamKind;
    if m.kind() == WireKind::ServerRequest {
        return if local {
            approval_request(m).map(Some)
        } else {
            violation()
        };
    }
    if m.kind() != WireKind::Notification {
        return Ok(None);
    }
    let method = m.method().ok_or_else(invalid)?;
    let p = m.params().unwrap_or(&Value::Null);
    if local && let Some(notice) = local_action(method, p)? {
        return Ok(Some(notice));
    }
    // Exactly one new notification is tolerated, and only when it is exactly what the schema
    // says: anything malformed is the isolation violation it has always been.
    if method == "model/rerouted" {
        return guard(reroute(p)).map(Some);
    }
    if action_candidate(method, p, local) {
        return violation();
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
                _ => return violation(),
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
        // The supervisor correlates this against the one request it holds.
        CodexObservation::ApprovalResolved { .. } if local => Ok(None),
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
        _ => violation(),
    }
}
