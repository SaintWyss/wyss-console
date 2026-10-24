use super::*;
use crate::adapters::claude::frames::{Event, Plugin};

const INFERENCE: &str = include_str!("../../../../claude-bridge/fixtures/inference-turn.jsonl");
const TOOLS: &str = include_str!("../../../../claude-bridge/fixtures/local-tools-approved.jsonl");

fn events(body: &str) -> Vec<Event> {
    body.lines()
        .map(|l| Event::parse(l.as_bytes()).unwrap())
        .collect()
}
fn ready(body: &str) -> (String, Attestation) {
    events(body)
        .into_iter()
        .find_map(|e| match e {
            Event::Ready {
                profile,
                attestation,
            } => Some((profile, attestation)),
            _ => None,
        })
        .unwrap()
}
fn started(body: &str) -> Init {
    events(body)
        .into_iter()
        .find_map(|e| match e {
            Event::TurnStarted { init, .. } => Some(init),
            _ => None,
        })
        .unwrap()
}
fn named(name: &str, source: &str) -> Named {
    Named {
        name: Some(name.into()),
        status: Some("connected".into()),
        source: Some(source.into()),
    }
}

#[test]
fn what_the_real_sdk_reported_is_accepted_for_its_profile() {
    let (p, a) = ready(INFERENCE);
    assert_eq!(verify_ready(Profile::Inference, &p, &a), Ok(()));
    assert_eq!(
        verify_turn_started(Profile::Inference, &started(INFERENCE)),
        Ok(())
    );
    let (p, a) = ready(TOOLS);
    assert_eq!(verify_ready(Profile::LocalTools, &p, &a), Ok(()));
    assert_eq!(
        verify_turn_started(Profile::LocalTools, &started(TOOLS)),
        Ok(())
    );
}

#[test]
fn a_profile_is_never_satisfied_by_the_other_profiles_evidence() {
    let (p, a) = ready(INFERENCE);
    assert_eq!(
        verify_ready(Profile::LocalTools, &p, &a),
        Err(Refusal::IncompleteAttestation)
    );
    // The digests differ per profile, so even a lying `profile` field cannot cross over.
    assert_eq!(
        verify_ready(Profile::LocalTools, "local_tools", &a),
        Err(Refusal::OptionsDigestMismatch)
    );
    let (p, a) = ready(TOOLS);
    assert_eq!(
        verify_ready(Profile::Inference, &p, &a),
        Err(Refusal::IncompleteAttestation)
    );
    assert_eq!(
        verify_ready(Profile::Inference, "inference", &a),
        Err(Refusal::OptionsDigestMismatch)
    );
    assert_eq!(
        verify_turn_started(Profile::Inference, &started(TOOLS)),
        Err(Refusal::ExecutableToolSurface)
    );
    assert_eq!(
        verify_turn_started(Profile::LocalTools, &started(INFERENCE)),
        Err(Refusal::UnexpectedAuthorityTools)
    );
}

#[test]
fn unsupported_or_missing_builds_are_refused() {
    let (p, base) = ready(INFERENCE);
    for (sdk, cli) in [
        (Some("0.2.172"), Some("2.1.309")),
        (Some("0.2.173"), Some("2.1.310")),
        (None, Some("2.1.309")),
        (Some("0.2.173"), None),
    ] {
        let mut a = base.clone();
        a.sdk_version = sdk.map(str::to_owned);
        a.claude_code_version = cli.map(str::to_owned);
        assert_eq!(
            verify_ready(Profile::Inference, &p, &a),
            Err(Refusal::UnsupportedBuild),
            "{sdk:?} {cli:?}"
        );
    }
    let mut a = base.clone();
    a.options_digest = None;
    assert_eq!(
        verify_ready(Profile::Inference, &p, &a),
        Err(Refusal::OptionsDigestMismatch)
    );
    a.options_digest = Some("0".repeat(64));
    assert_eq!(
        verify_ready(Profile::Inference, &p, &a),
        Err(Refusal::OptionsDigestMismatch)
    );
}

/// One mutation of the real attestation per hidden-effect class the probe found.
#[test]
fn every_hidden_effect_class_refuses_the_pre_turn_check() {
    let (p, base) = ready(INFERENCE);
    type Mutate = fn(&mut Attestation);
    let cases: [(&str, Mutate, Refusal); 14] = [
        (
            "user MCP server",
            |a| a.mcp_servers.push(named("userglobalmcp", "user")),
            Refusal::McpServerPresent,
        ),
        (
            "project MCP server",
            |a| a.mcp_servers.push(named("projectmcp", "project")),
            Refusal::McpServerPresent,
        ),
        (
            "managed (enterprise) MCP",
            |a| a.mcp_servers.push(named("managedmcp", "enterprise")),
            Refusal::ManagedPolicyPresent,
        ),
        (
            "an MCP tool on inference",
            |a| a.mcp_tools.push("mcp__wyss__run_command".into()),
            Refusal::ExecutableToolSurface,
        ),
        (
            "a built-in tool category",
            |a| a.context_categories.push("System tools".into()),
            Refusal::ExecutableToolSurface,
        ),
        (
            "memory files loaded",
            |a| a.memory_files_n = 1,
            Refusal::MemoryPresent,
        ),
        (
            "MCP instructions",
            |a| a.mcp_instructions_n = 1,
            Refusal::MemoryPresent,
        ),
        (
            "apiKeyHelper supplies the key",
            |a| a.token_source = Some("apiKeyHelper".into()),
            Refusal::CredentialHelperPresent,
        ),
        (
            "token source unknown",
            |a| a.token_source = None,
            Refusal::CredentialHelperPresent,
        ),
        (
            "analytics on",
            |a| a.analytics_disabled = Some(false),
            Refusal::AnalyticsNotDisabled,
        ),
        (
            "permission mode default",
            |a| a.permission_mode = Some("default".into()),
            Refusal::PermissionModeUnexpected,
        ),
        (
            "slash commands",
            |a| a.commands_n = 3,
            Refusal::SlashCommandsPresent,
        ),
        (
            "a configured subagent",
            |a| a.agents.push("useragent".into()),
            Refusal::UnknownAgentSurface,
        ),
        (
            "no model list",
            |a| a.models.clear(),
            Refusal::IncompleteAttestation,
        ),
    ];
    for (name, mutate, expected) in cases {
        let mut a = base.clone();
        mutate(&mut a);
        assert_eq!(
            verify_ready(Profile::Inference, &p, &a),
            Err(expected),
            "{name}"
        );
    }
    let mut a = base.clone();
    a.skills_n = Some(2);
    assert_eq!(
        verify_ready(Profile::Inference, &p, &a),
        Err(Refusal::SkillsPresent)
    );
}

#[test]
fn every_hidden_effect_class_refuses_the_turn_inventory() {
    let base = started(INFERENCE);
    type Mutate = fn(&mut Init);
    let cases: [(&str, Mutate, Refusal); 9] = [
        (
            "a built-in tool",
            |i| i.tools.push("Bash".into()),
            Refusal::ExecutableToolSurface,
        ),
        (
            "an authority tool on inference",
            |i| i.tools.push("mcp__wyss__run_command".into()),
            Refusal::ExecutableToolSurface,
        ),
        (
            "an MCP server",
            |i| i.mcp_servers.push(named("x", "project")),
            Refusal::McpServerPresent,
        ),
        (
            "managed MCP",
            |i| i.mcp_servers.push(named("x", "enterprise")),
            Refusal::ManagedPolicyPresent,
        ),
        (
            "an external plugin",
            |i| {
                i.plugins.push(Plugin {
                    name: Some("p".into()),
                    source: Some("probeplugin@local".into()),
                })
            },
            Refusal::PluginPresent,
        ),
        (
            "slash commands",
            |i| i.slash_commands_n = 1,
            Refusal::SlashCommandsPresent,
        ),
        (
            "a skill",
            |i| i.skills.push("userskill".into()),
            Refusal::SkillsPresent,
        ),
        (
            "a configured agent",
            |i| i.agents.push("projectagent".into()),
            Refusal::UnknownAgentSurface,
        ),
        (
            "bypassPermissions",
            |i| i.permission_mode = Some("bypassPermissions".into()),
            Refusal::PermissionModeUnexpected,
        ),
    ];
    for (name, mutate, expected) in cases {
        let mut i = base.clone();
        mutate(&mut i);
        assert_eq!(
            verify_turn_started(Profile::Inference, &i),
            Err(expected),
            "{name}"
        );
    }
    let mut i = base.clone();
    i.claude_code_version = Some("2.1.1".into());
    assert_eq!(
        verify_turn_started(Profile::Inference, &i),
        Err(Refusal::UnsupportedBuild)
    );
    i.claude_code_version = None;
    assert_eq!(
        verify_turn_started(Profile::Inference, &i),
        Err(Refusal::UnsupportedBuild)
    );
}

#[test]
fn local_tools_needs_exactly_the_authority_tools() {
    let base = started(TOOLS);
    let mut i = base.clone();
    i.tools.retain(|t| t != "mcp__wyss__write_file");
    assert_eq!(
        verify_turn_started(Profile::LocalTools, &i),
        Err(Refusal::UnexpectedAuthorityTools)
    );
    let mut i = base.clone();
    i.tools.push("Write".into());
    assert_eq!(
        verify_turn_started(Profile::LocalTools, &i),
        Err(Refusal::ExecutableToolSurface)
    );
    let mut i = base.clone();
    i.tools.push("mcp__other__run_command".into());
    assert_eq!(
        verify_turn_started(Profile::LocalTools, &i),
        Err(Refusal::ExecutableToolSurface)
    );
    // The SDK server must be the in-process one named `wyss`, nothing that merely looks like it.
    let mut i = base.clone();
    i.mcp_servers = vec![named("wyss", "project")];
    assert_eq!(
        verify_turn_started(Profile::LocalTools, &i),
        Err(Refusal::McpServerPresent)
    );
    let mut i = base.clone();
    i.mcp_servers = vec![named("wyss2", "sdk")];
    assert_eq!(
        verify_turn_started(Profile::LocalTools, &i),
        Err(Refusal::McpServerPresent)
    );
    let mut i = base;
    i.mcp_servers.push(named("extra", "sdk"));
    assert_eq!(
        verify_turn_started(Profile::LocalTools, &i),
        Err(Refusal::McpServerPresent)
    );
}

#[test]
fn the_tap_is_the_check_that_does_not_trust_the_sdk() {
    let none: [String; 0] = [];
    let auth = vec![
        "mcp__wyss__run_command".to_owned(),
        "mcp__wyss__write_file".to_owned(),
        INERT_TOOL.to_owned(),
    ];
    assert_eq!(
        verify_tap(Profile::Inference, "/v1/messages", &none, Some("m"), None),
        Ok(())
    );
    assert_eq!(
        verify_tap(
            Profile::Inference,
            "/v1/messages/count_tokens",
            &none,
            None,
            None
        ),
        Ok(())
    );
    assert_eq!(
        verify_tap(Profile::LocalTools, "/v1/messages", &auth, Some("m"), None),
        Ok(())
    );
    for tools in [
        vec!["Bash".to_owned()],
        vec!["mcp__wyss__run_command".to_owned()],
        vec![INERT_TOOL.to_owned()],
    ] {
        assert_eq!(
            verify_tap(Profile::Inference, "/v1/messages", &tools, None, None),
            Err(Refusal::ProviderRequestOffersTool)
        );
    }
    let mut more = auth.clone();
    more.push("WebFetch".into());
    assert_eq!(
        verify_tap(Profile::LocalTools, "/v1/messages", &more, None, None),
        Err(Refusal::ProviderRequestOffersTool)
    );
    for path in ["/v1/files", "/v1/complete", "/", ""] {
        assert_eq!(
            verify_tap(Profile::Inference, path, &none, None, None),
            Err(Refusal::ProviderRequestUnexpectedPath)
        );
    }
}

#[test]
fn a_pinned_model_is_checked_on_generation_requests_only() {
    let none: [String; 0] = [];
    assert_eq!(
        verify_tap(
            Profile::Inference,
            "/v1/messages",
            &none,
            Some("b"),
            Some("a")
        ),
        Err(Refusal::ModelMismatch)
    );
    assert_eq!(
        verify_tap(
            Profile::Inference,
            "/v1/messages",
            &none,
            Some("a"),
            Some("a")
        ),
        Ok(())
    );
    // Token counting may use a different model name for the estimate; it generates nothing.
    assert_eq!(
        verify_tap(
            Profile::Inference,
            "/v1/messages/count_tokens",
            &none,
            Some("b"),
            Some("a")
        ),
        Ok(())
    );
}

#[test]
fn refusal_codes_are_unique_stable_text() {
    use Refusal::*;
    let all = [
        UnsupportedBuild,
        OptionsDigestMismatch,
        ExecutableToolSurface,
        UnexpectedAuthorityTools,
        McpServerPresent,
        ManagedPolicyPresent,
        PluginPresent,
        MemoryPresent,
        CredentialHelperPresent,
        AnalyticsNotDisabled,
        PermissionModeUnexpected,
        SlashCommandsPresent,
        SkillsPresent,
        UnknownAgentSurface,
        ProviderRequestOffersTool,
        ProviderRequestUnexpectedPath,
        ModelMismatch,
        ModelSubstituted,
        IncompleteAttestation,
    ];
    let codes: std::collections::BTreeSet<_> = all.iter().map(|r| r.code()).collect();
    assert_eq!(codes.len(), all.len());
    assert!(
        codes
            .iter()
            .all(|c| c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
    );
}

#[test]
fn the_pinned_digests_match_the_bridges_own() {
    // `profiles.frozen_digest` is the single definition: recompute it with the embedded
    // bridge source and compare, so profiles.py cannot drift from the pins unnoticed.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/claude-bridge");
    for (profile, pinned) in [
        ("inference", pins::INFERENCE_OPTIONS_DIGEST),
        ("local_tools", pins::LOCAL_TOOLS_OPTIONS_DIGEST),
    ] {
        let out = std::process::Command::new("python3")
            .args(["-I", "-c", &format!("import sys; sys.path.insert(0, {dir:?}); import profiles; print(profiles.frozen_digest({profile:?}))")])
            .output()
            .expect("python3 is required to check the pinned digests");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8(out.stdout).unwrap().trim(),
            pinned,
            "{profile}"
        );
    }
}

#[test]
fn notices_are_classified_by_name_and_only_qualified_names_pass() {
    for n in OBSERVED_NOTICES
        .iter()
        .chain(EXPECTED_NOTICES.iter())
        .chain(BRIDGE_NOTICES.iter())
    {
        assert_eq!(classify_notice(n), NoticeClass::Benign, "{n}");
    }
    for n in SUBSTITUTION_NOTICES {
        assert_eq!(classify_notice(n), NoticeClass::Substitution, "{n}");
    }
    // Names the CLI knows for features this profile does not offer: all refused.
    for n in [
        "local_command",
        "stop_hook_summary",
        "memory_saved",
        "memory_recall",
        "plugin_install",
        "scheduled_task_fire",
        "task_started",
        "task_progress",
        "task_notification",
        "background_tasks_changed",
        "vcs_state_changed",
        "tool_host_result",
        "permission_retry",
        "worker_shutting_down",
        "mirror_error",
        "rewind_stand_in",
        "informational",
        "notification",
        "dev_intent",
        "peer_message_hold",
        "turn_preempted",
        "",
        "UI_INVALIDATE",
        "status ",
        "message:SystemMessage",
    ] {
        assert_eq!(classify_notice(n), NoticeClass::Unknown, "{n:?}");
    }
}

#[test]
fn the_three_notice_lists_are_disjoint() {
    let all: Vec<&str> = OBSERVED_NOTICES
        .iter()
        .chain(&EXPECTED_NOTICES)
        .chain(&BRIDGE_NOTICES)
        .chain(&SUBSTITUTION_NOTICES)
        .copied()
        .collect();
    let set: std::collections::BTreeSet<_> = all.iter().collect();
    assert_eq!(set.len(), all.len());
}
