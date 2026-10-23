//! Versioned evidence for Native profile override syntax. This is deliberately an exact-build
//! registry: syntax evidence for one build does not qualify a newer or older build, and never
//! claims account entitlement, provider resolution, session reload, or effective use.
use super::catalogue::AgentId;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    SyntaxVerified,
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub status: Status,
    /// The exact CLI version output this record qualifies, not a version range.
    pub qualified_version: Option<&'static str>,
    pub checked_on: Option<&'static str>,
    /// Operations whose launch/configuration surface is documented or has recorded evidence.
    pub operations: &'static [&'static str],
    pub interface: Option<&'static str>,
    pub format: Option<&'static str>,
    pub scope: Option<&'static str>,
    pub reload_semantics: Option<&'static str>,
    pub source: Option<&'static str>,
    pub limitation: &'static str,
}

const CLAUDE: Evidence = Evidence {
    status: Status::SyntaxVerified,
    qualified_version: Some("2.1.309 (Claude Code)"),
    checked_on: Some("2026-10-23"),
    operations: &[
        "model via --model",
        "reasoning effort via --effort",
        "permission mode via --permission-mode",
        "session JSON overlay via --settings",
        "skillOverrides=off and disableAllHooks behavior tested on Claude Code 2.1.303",
    ],
    interface: Some("--model, --effort, --permission-mode and --settings"),
    format: Some("argv flags plus a JSON settings overlay"),
    scope: Some("one Native child process; Wyss does not write global or project settings"),
    reload_semantics: Some("No Native overlay hot-reload acceptance; effective use is unverified"),
    source: Some("2.1.309 --version/--help; F3 and F1/F2a real overlay checks on 2.1.303"),
    limitation: "Exact 2.1.309 Native provider-resolved/effective values and account entitlement are not observed",
};

const CODEX: Evidence = Evidence {
    status: Status::SyntaxVerified,
    qualified_version: Some("codex-cli 0.159.0"),
    checked_on: Some("2026-10-23"),
    operations: &[
        "model via -m",
        "reasoning effort via -c model_reasoning_effort=...",
        "sandbox via -s",
        "approval mode via -a",
    ],
    interface: Some("-m, -c KEY=VALUE, -s and -a"),
    format: Some("argv flags plus -c KEY=VALUE parsed as a TOML value"),
    scope: Some("one Native child process; ~/.codex/config.toml is not written"),
    reload_semantics: Some("No Native turn or session reload was tested; -c is a launch override"),
    source: Some("0.159.0 --version/--help; F6 read-only config/read (no Native turn)"),
    limitation: "override resolution is not proof that a Native turn used the requested value or was entitled to it",
};

const UNKNOWN: Evidence = Evidence {
    status: Status::Unverified,
    qualified_version: None,
    checked_on: None,
    operations: &[],
    interface: None,
    format: None,
    scope: None,
    reload_semantics: None,
    source: None,
    limitation: "this exact CLI build has no M2 profile-override evidence",
};

/// Returns evidence only for an exact, reviewed CLI version line. In particular, a provider
/// update does not inherit a previous build's result.
pub fn evidence(agent: AgentId, version: Option<&str>) -> Evidence {
    match (agent, version) {
        (AgentId::ClaudeCode, Some(v)) if v == CLAUDE.qualified_version.unwrap() => CLAUDE,
        (AgentId::Codex, Some(v)) if v == CODEX.qualified_version.unwrap() => CODEX,
        _ => UNKNOWN,
    }
}

pub fn permits_profile_overrides(agent: AgentId, version: Option<&str>) -> bool {
    evidence(agent, version).status == Status::SyntaxVerified
}

/// Native can always launch as configured; a profile that contributes words or settings needs
/// syntax evidence for the exact build observed immediately before launch.
pub fn allows_start_with_profile(
    agent: AgentId,
    version: Option<&str>,
    has_effective_overrides: bool,
) -> bool {
    !has_effective_overrides || permits_profile_overrides(agent, version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_observed_builds_receive_their_own_syntax_evidence() {
        let claude = evidence(AgentId::ClaudeCode, Some("2.1.309 (Claude Code)"));
        assert_eq!(claude.status, Status::SyntaxVerified);
        assert!(claude.operations.contains(&"reasoning effort via --effort"));
        assert_eq!(
            claude.format,
            Some("argv flags plus a JSON settings overlay")
        );
        assert!(claude.reload_semantics.unwrap().contains("unverified"));
        assert!(claude.limitation.contains("not observed"));

        let codex = evidence(AgentId::Codex, Some("codex-cli 0.159.0"));
        assert_eq!(codex.status, Status::SyntaxVerified);
        assert_eq!(codex.interface, Some("-m, -c KEY=VALUE, -s and -a"));
        assert_eq!(
            codex.scope,
            Some("one Native child process; ~/.codex/config.toml is not written")
        );
    }

    #[test]
    fn missing_or_changed_versions_do_not_inherit_evidence() {
        for (agent, version) in [
            (AgentId::ClaudeCode, None),
            (AgentId::ClaudeCode, Some("2.1.310 (Claude Code)")),
            (AgentId::Codex, Some("codex-cli 0.160.0")),
            (AgentId::Codex, Some("codex-cli 0.159.0-dev")),
        ] {
            assert_eq!(evidence(agent, version).status, Status::Unverified);
            assert!(!permits_profile_overrides(agent, version));
        }
    }

    #[test]
    fn unverified_builds_keep_as_configured_launch_and_refuse_profile_overrides() {
        assert!(allows_start_with_profile(
            AgentId::ClaudeCode,
            Some("2.1.310 (Claude Code)"),
            false
        ));
        assert!(!allows_start_with_profile(
            AgentId::ClaudeCode,
            Some("2.1.310 (Claude Code)"),
            true
        ));
        assert!(allows_start_with_profile(
            AgentId::ClaudeCode,
            Some("2.1.309 (Claude Code)"),
            true
        ));
    }
}
