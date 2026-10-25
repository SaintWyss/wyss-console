//! Whether a runtime may be used, decided from what the bridge reports.
//!
//! The decision has three independent legs, each of which must pass on its own:
//!
//! 1. `verify_ready`: the build and the pre-turn introspection of the SDK (no tool definitions,
//!    no MCP server, no memory, no managed policy, no apiKeyHelper, the bridge's frozen option
//!    digest).
//! 2. `verify_turn_started`: the CLI's own `system/init` inventory for the turn, before any model
//!    text is forwarded.
//! 3. `verify_tap`: the tool list of every provider request, as seen by the loopback API tap
//!    that the bridge cannot reach around. This is the leg that does not depend on what the SDK
//!    or the CLI say about themselves.
//!
//! None of them proves the absence of an effect the CLI could perform without telling anyone;
//! that residual is stated in ADR0053 and covered by the process boundary, not by this file.
use super::{
    Profile,
    frames::{Attestation, Init, Named},
    pins,
};

/// Why a runtime was refused. The code is stable text for audit and the GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Refusal {
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
}
impl Refusal {
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedBuild => "unsupported_build",
            Self::OptionsDigestMismatch => "options_digest_mismatch",
            Self::ExecutableToolSurface => "executable_tool_surface",
            Self::UnexpectedAuthorityTools => "unexpected_authority_tools",
            Self::McpServerPresent => "mcp_server_present",
            Self::ManagedPolicyPresent => "managed_policy_present",
            Self::PluginPresent => "plugin_present",
            Self::MemoryPresent => "memory_present",
            Self::CredentialHelperPresent => "credential_helper_present",
            Self::AnalyticsNotDisabled => "analytics_not_disabled",
            Self::PermissionModeUnexpected => "permission_mode_unexpected",
            Self::SlashCommandsPresent => "slash_commands_present",
            Self::SkillsPresent => "skills_present",
            Self::UnknownAgentSurface => "unknown_agent_surface",
            Self::ProviderRequestOffersTool => "provider_request_offers_tool",
            Self::ProviderRequestUnexpectedPath => "provider_request_unexpected_path",
            Self::ModelMismatch => "model_mismatch",
            Self::ModelSubstituted => "model_substituted",
            Self::IncompleteAttestation => "incomplete_attestation",
        }
    }
}

/// The authority-owned tool names, as the SDK namespaces them.
pub const AUTHORITY_SERVER: &str = "wyss";
pub const AUTHORITY_TOOLS: [&str; 2] = ["mcp__wyss__run_command", "mcp__wyss__write_file"];
/// A tool entry the CLI adds to API requests whenever an MCP tool exists. Calling it returns
/// "No such tool available" (probe matrix, case g): it has no effect.
pub const INERT_TOOL: &str = "DeferredToolPlaceholder";
/// Agent definitions the CLI ships. Without the `Agent` tool no agent can be started; the
/// names are listed only so that an unknown one (a configured subagent) is noticed.
const BUILTIN_AGENTS: [&str; 5] = [
    "claude",
    "Explore",
    "general-purpose",
    "Plan",
    "statusline-setup",
];
/// Plugins the CLI ships inside its own binary.
fn builtin_plugin(source: &Option<String>) -> bool {
    source.as_deref().is_some_and(|s| s.ends_with("@builtin"))
}
/// How a `system` message of the CLI (or a bridge notice) is treated. The CLI's binary names
/// about fifty subtypes; most belong to features this profile does not offer (tasks, hooks,
/// memory, plugins, local commands, scheduled work). The bridge forwards only the subtype, so
/// the decision is by name, and a name nobody qualified is a refusal, never a shrug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeClass {
    /// Observational: it describes the session, it changes nothing.
    Benign,
    /// The provider or CLI replaced the requested model. Wyss configured no fallback, so this is
    /// a substitution nobody authorized.
    Substitution,
    /// Anything else: a feature this build was not qualified against.
    Unknown,
}

/// Subtypes seen in real SDK runs of this build (`probe/smoke_edge.py`, `capture_fixtures.py`).
pub const OBSERVED_NOTICES: [&str; 3] = ["ui_invalidate", "status", "api_retry"];
/// Subtypes whose names the CLI uses for session or turn bookkeeping. Not yet observed here:
/// qualifying them against the real provider is part of PROVIDER_EVIDENCE_REQUIRED.
pub const EXPECTED_NOTICES: [&str; 9] = [
    "thinking_tokens",
    "turn_duration",
    "turn_starting",
    "session_state_changed",
    "session_title_changed",
    "session_metadata",
    "compact_boundary",
    "control_request_progress",
    "model_refusal_no_fallback",
];
/// Bridge-originated notices.
pub const BRIDGE_NOTICES: [&str; 2] = ["pretooluse_denied", "permission_denied"];
pub const SUBSTITUTION_NOTICES: [&str; 3] = [
    "model_fallback",
    "model_refusal_fallback",
    "model_consent_fallback",
];

/// The API's error classes for a message, as the SDK names them. A class outside this list is not
/// recognised and fails the worker.
pub const PROVIDER_ERRORS: [&str; 7] = [
    "authentication_failed",
    "billing_error",
    "rate_limit",
    "invalid_request",
    "server_error",
    "max_output_tokens",
    "unknown",
];

pub fn classify_notice(subtype: &str) -> NoticeClass {
    if SUBSTITUTION_NOTICES.contains(&subtype) {
        NoticeClass::Substitution
    } else if OBSERVED_NOTICES
        .iter()
        .chain(EXPECTED_NOTICES.iter())
        .chain(BRIDGE_NOTICES.iter())
        .any(|n| *n == subtype)
    {
        NoticeClass::Benign
    } else {
        NoticeClass::Unknown
    }
}

fn expected_digest(profile: Profile) -> &'static str {
    match profile {
        Profile::Inference => pins::INFERENCE_OPTIONS_DIGEST,
        Profile::LocalTools => pins::LOCAL_TOOLS_OPTIONS_DIGEST,
    }
}
fn is_authority_tool(name: &str) -> bool {
    AUTHORITY_TOOLS.contains(&name)
}
fn tools_allowed(profile: Profile, tools: &[String], inert_ok: bool) -> Result<(), Refusal> {
    for t in tools {
        match profile {
            Profile::Inference => return Err(Refusal::ExecutableToolSurface),
            Profile::LocalTools => {
                if t == INERT_TOOL && inert_ok {
                    continue;
                }
                if !is_authority_tool(t) {
                    return Err(Refusal::ExecutableToolSurface);
                }
            }
        }
    }
    Ok(())
}
fn servers_allowed(profile: Profile, servers: &[Named]) -> Result<(), Refusal> {
    for s in servers {
        if s.source.as_deref() == Some("enterprise") {
            return Err(Refusal::ManagedPolicyPresent);
        }
        // The one in-process SDK server of the local-tools profile is the authority's own.
        let ours = profile == Profile::LocalTools
            && s.name.as_deref() == Some(AUTHORITY_SERVER)
            && s.source.as_deref() == Some("sdk");
        if !ours {
            return Err(Refusal::McpServerPresent);
        }
    }
    Ok(())
}

/// Leg 1. `expected_cli` is the bundled CLI's version the authority verified on disk.
pub fn verify_ready(
    profile: Profile,
    bridge_profile: &str,
    a: &Attestation,
) -> Result<(), Refusal> {
    if Profile::from_bridge(bridge_profile) != Some(profile) {
        return Err(Refusal::IncompleteAttestation);
    }
    let (Some(sdk), Some(cli)) = (&a.sdk_version, &a.claude_code_version) else {
        return Err(Refusal::UnsupportedBuild);
    };
    if pins::build_for(sdk, cli).is_none() {
        return Err(Refusal::UnsupportedBuild);
    }
    if a.options_digest.as_deref() != Some(expected_digest(profile)) {
        return Err(Refusal::OptionsDigestMismatch);
    }
    servers_allowed(profile, &a.mcp_servers)?;
    // Before the first turn the SDK server may not have connected yet: its tools are either
    // absent or exactly the authority's.
    tools_allowed(profile, &a.mcp_tools, false)?;
    if a.memory_files_n != 0 || a.mcp_instructions_n != 0 {
        return Err(Refusal::MemoryPresent);
    }
    // `none` is the proof that no apiKeyHelper (a settings-defined command) supplies the key.
    if a.token_source.as_deref() != Some("none") {
        return Err(Refusal::CredentialHelperPresent);
    }
    if a.analytics_disabled != Some(true) {
        return Err(Refusal::AnalyticsNotDisabled);
    }
    if a.permission_mode.as_deref() != Some("dontAsk") {
        return Err(Refusal::PermissionModeUnexpected);
    }
    if a.commands_n != 0 || a.slash_commands_n.is_some_and(|n| n != 0) {
        return Err(Refusal::SlashCommandsPresent);
    }
    if a.skills_n.is_some_and(|n| n != 0) {
        return Err(Refusal::SkillsPresent);
    }
    if a.agents
        .iter()
        .any(|n| !BUILTIN_AGENTS.contains(&n.as_str()))
    {
        return Err(Refusal::UnknownAgentSurface);
    }
    // The context report lists a "System tools"/"Custom agents"/"Memory files"/"Skills"
    // category only when there is something in it.
    const EMPTY_ONLY: [&str; 5] = [
        "System tools",
        "System prompt",
        "Custom agents",
        "Memory files",
        "Skills",
    ];
    if a.context_categories
        .iter()
        .any(|c| EMPTY_ONLY.contains(&c.as_str()))
    {
        return Err(Refusal::ExecutableToolSurface);
    }
    if a.models.is_empty() {
        return Err(Refusal::IncompleteAttestation);
    }
    Ok(())
}

/// Leg 2.
pub fn verify_turn_started(profile: Profile, init: &Init) -> Result<(), Refusal> {
    tools_allowed(profile, &init.tools, true)?;
    if profile == Profile::LocalTools {
        // Exactly the authority's two tools, no fewer: a missing tool means the SDK server did
        // not register what the profile promises.
        let mut have: Vec<&str> = init
            .tools
            .iter()
            .map(String::as_str)
            .filter(|t| *t != INERT_TOOL)
            .collect();
        have.sort_unstable();
        let mut want = AUTHORITY_TOOLS.to_vec();
        want.sort_unstable();
        if have != want {
            return Err(Refusal::UnexpectedAuthorityTools);
        }
    }
    servers_allowed(profile, &init.mcp_servers)?;
    if init.plugins.iter().any(|p| !builtin_plugin(&p.source)) {
        return Err(Refusal::PluginPresent);
    }
    if init.slash_commands_n != 0 {
        return Err(Refusal::SlashCommandsPresent);
    }
    if !init.skills.is_empty() {
        return Err(Refusal::SkillsPresent);
    }
    if init
        .agents
        .iter()
        .any(|n| !BUILTIN_AGENTS.contains(&n.as_str()))
    {
        return Err(Refusal::UnknownAgentSurface);
    }
    if init.permission_mode.as_deref() != Some("dontAsk") {
        return Err(Refusal::PermissionModeUnexpected);
    }
    let Some(cli) = &init.claude_code_version else {
        return Err(Refusal::UnsupportedBuild);
    };
    if !pins::BUILDS.iter().any(|b| b.cli_version == cli) {
        return Err(Refusal::UnsupportedBuild);
    }
    Ok(())
}

/// Leg 3: one provider request, as the tap saw it. `pinned_model` is the model this runtime
/// was started with, if any; absent means the provider default and nothing is pinned.
pub fn verify_tap(
    profile: Profile,
    path: &str,
    tools: &[String],
    model: Option<&str>,
    pinned_model: Option<&str>,
) -> Result<(), Refusal> {
    if !matches!(path, "/v1/messages" | "/v1/messages/count_tokens") {
        return Err(Refusal::ProviderRequestUnexpectedPath);
    }
    tools_allowed(profile, tools, true).map_err(|_| Refusal::ProviderRequestOffersTool)?;
    if let (Some(want), Some(got)) = (pinned_model, model)
        && want != got
        && path == "/v1/messages"
    {
        return Err(Refusal::ModelMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
