//! Versioned evidence for Native profile override syntax. Builds with a record keep it; any other
//! observed build of an agent that has override syntax uses that agent's newest record. It never
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

/// Every profile flag, including full access, appears in this build's `--help`.
const CLAUDE_316: Evidence = Evidence {
    status: Status::SyntaxVerified,
    qualified_version: Some("2.1.316 (Claude Code)"),
    checked_on: Some("2026-11-01"),
    operations: &[
        "model via --model",
        "reasoning effort via --effort (low, medium, high, xhigh, max)",
        "permission mode via --permission-mode",
        "full access via --dangerously-skip-permissions",
        "session JSON overlay via --settings",
    ],
    interface: Some(
        "--model, --effort, --permission-mode, --dangerously-skip-permissions and --settings",
    ),
    format: Some("argv flags plus a JSON settings overlay"),
    scope: Some("one Native child process; Wyss does not write global or project settings"),
    reload_semantics: Some("No Native overlay hot-reload acceptance; effective use is unverified"),
    source: Some("2.1.316 --version/--help"),
    limitation: "Syntax only: effective values, overlay behavior on this build and account entitlement are not observed",
};

/// This build's `--help` lists the flags and values used by profiles.
const CLAUDE_319: Evidence = Evidence {
    status: Status::SyntaxVerified,
    qualified_version: Some("2.1.319 (Claude Code)"),
    checked_on: Some("2026-11-07"),
    operations: &[
        "model via --model",
        "reasoning effort via --effort (low, medium, high, xhigh, max)",
        "permission mode via --permission-mode",
        "full access via --dangerously-skip-permissions (--help, 2026-11-07)",
        "session JSON overlay via --settings",
    ],
    interface: Some(
        "--model, --effort, --permission-mode, --dangerously-skip-permissions and --settings",
    ),
    format: Some("argv flags plus a JSON settings overlay"),
    scope: Some("one Native child process; Wyss does not write global or project settings"),
    reload_semantics: Some("No Native overlay hot-reload acceptance; effective use is unverified"),
    source: Some("2.1.319 --version/--help"),
    limitation: "Syntax only: effective values, the full-access effect and account entitlement are not observed",
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
        "full access via --dangerously-bypass-approvals-and-sandbox (--help, 2026-11-01)",
    ],
    interface: Some("-m, -c KEY=VALUE, -s, -a and --dangerously-bypass-approvals-and-sandbox"),
    format: Some("argv flags plus -c KEY=VALUE parsed as a TOML value"),
    scope: Some("one Native child process; ~/.codex/config.toml is not written"),
    reload_semantics: Some("No Native turn or session reload was tested; -c is a launch override"),
    source: Some("0.159.0 --version/--help; F6 read-only config/read (no Native turn)"),
    limitation: "override resolution is not proof that a Native turn used the requested value or was entitled to it",
};

const OPENCODE: Evidence = Evidence {
    status: Status::SyntaxVerified,
    qualified_version: Some("1.19.4"),
    checked_on: Some("2026-10-26"),
    operations: &[
        "model via --model",
        "project scope through the current working directory",
        "native TUI by default",
    ],
    interface: Some("opencode [project] --model provider/model"),
    format: Some("OpenCode v1 CLI argument parser"),
    scope: Some("one Native child process; the selected project is its working directory"),
    reload_semantics: Some("launch-time only; effective model/provider resolution is unobserved"),
    source: Some(
        "anomalyco/opencode@148842ef7cb1bc34192b3fff57d0f9721ab9c801, packages/opencode/src/cli/cmd/tui.ts",
    ),
    limitation: "the exact source accepts --model; Wyss does not attest provider resolution or model use",
};

const HERMES_PROFILE_UNVERIFIED: Evidence = Evidence {
    status: Status::Unverified,
    qualified_version: None,
    checked_on: Some("2026-10-26"),
    operations: &[],
    interface: None,
    format: None,
    scope: Some("Native full-account CLI/TUI; no Wyss mediation or confinement"),
    reload_semantics: None,
    source: Some("NousResearch/hermes-agent@bf0203a794a07aac3ff764972ab9087baa4e4f4e"),
    limitation: "the pinned upstream has an interactive CLI/TUI, but `hermes --version` performs an update check; no exact installed build is observed and no profile override is applied",
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

/// Returns the recorded evidence for the exact CLI version line when there is one. Any other
/// observed build of Claude Code, Codex or OpenCode uses the newest record of its agent: the
/// owner asked that a provider update never block a profile. A launch is still bound to the
/// version observed immediately before it (see `NativeService::start`), so a CLI that changes
/// between the preview and the start is refused whatever its evidence. Without an observed
/// version nothing can be bound, so there is no evidence.
pub fn evidence(agent: AgentId, version: Option<&str>) -> Evidence {
    match (agent, version) {
        (AgentId::ClaudeCode, Some(v)) if v == CLAUDE.qualified_version.unwrap() => CLAUDE,
        (AgentId::ClaudeCode, Some(v)) if v == CLAUDE_316.qualified_version.unwrap() => CLAUDE_316,
        (AgentId::ClaudeCode, Some(v)) if v == CLAUDE_319.qualified_version.unwrap() => CLAUDE_319,
        (AgentId::Codex, Some(v)) if v == CODEX.qualified_version.unwrap() => CODEX,
        (AgentId::Opencode, Some(v)) if v == OPENCODE.qualified_version.unwrap() => OPENCODE,
        (AgentId::ClaudeCode, Some(_)) => CLAUDE_319,
        (AgentId::Codex, Some(_)) => CODEX,
        (AgentId::Opencode, Some(_)) => OPENCODE,
        (AgentId::Hermes, _) => HERMES_PROFILE_UNVERIFIED,
        _ => UNKNOWN,
    }
}

pub fn permits_profile_overrides(agent: AgentId, version: Option<&str>) -> bool {
    evidence(agent, version).status == Status::SyntaxVerified
}

#[cfg(any(test, feature = "test-fixtures"))]
/// Native can always launch as configured; a profile that contributes words or settings needs
/// syntax evidence for the exact build observed immediately before launch.
pub fn allows_start_with_profile(
    agent: AgentId,
    version: Option<&str>,
    has_effective_overrides: bool,
) -> bool {
    !has_effective_overrides || permits_profile_overrides(agent, version)
}

/// Evidence that one exact build honours an Arsenal skill exclusion: a `--settings` overlay that
/// holds only `skillOverrides` with `off` values, launched in one Native child. Recorded only from
/// a focused qualification of that build; a passing older build does not qualify a newer one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ArsenalEvidence {
    pub qualified_version: &'static str,
    pub checked_on: &'static str,
    pub operation: &'static str,
    pub source: &'static str,
    pub limitation: &'static str,
}

/// Claude Code builds whose skill exclusion passed a focused qualification. The A0 runs on 2.1.312
/// were judged NO-GO by the independent review and are not listed; any build without a record here
/// is previewed and recorded, never applied.
const ARSENAL_CLAUDE: &[ArsenalEvidence] = &[
    ArsenalEvidence {
        qualified_version: "2.1.314 (Claude Code)",
        checked_on: "2026-10-30",
        operation: "--settings overlay holding only skillOverrides=off for installed user and project skills",
        source: "qualification run 2026-10-30 (9/9 pre-registered cases)",
        limitation: "plugin and built-in skills are not switched off; concurrent sessions and provider use of each skill are not observed",
    },
    ArsenalEvidence {
        qualified_version: "2.1.316 (Claude Code)",
        checked_on: "2026-10-31",
        operation: "--settings overlay holding only skillOverrides=off for installed user and project skills",
        source: "qualification run 2026-10-31 (the same 9 pre-registered cases)",
        limitation: "plugin and built-in skills are not switched off; provider use of each skill is not observed",
    },
    ArsenalEvidence {
        qualified_version: "2.1.319 (Claude Code)",
        checked_on: "2026-11-07",
        operation: "--settings overlay holding only skillOverrides=off for installed user and project skills",
        source: "qualification run 2026-11-07 (the same 9 pre-registered cases)",
        limitation: "plugin and built-in skills are not switched off; provider use of each skill is not observed",
    },
    ArsenalEvidence {
        qualified_version: "2.1.323 (Claude Code)",
        checked_on: "2026-11-12",
        operation: "--settings overlay holding only skillOverrides=off for installed user and project skills",
        source: "qualification run 2026-11-12 (the same 9 pre-registered cases)",
        limitation: "plugin and built-in skills are not switched off; provider use of each skill is not observed",
    },
];

/// Codex builds whose per-session skill switch passed a focused qualification: the session runs with
/// `--no-daemon` (its own embedded server, not the shared one) and one
/// `-c skills.config=[{path = <SKILL.md>, enabled = false}, …]`.
const ARSENAL_CODEX: &[ArsenalEvidence] = &[
    ArsenalEvidence {
        qualified_version: "codex-cli 0.169.0",
        checked_on: "2026-11-15",
        operation: "--no-daemon and -c skills.config=[{path=<SKILL.md>, enabled=false}] for installed user, home and project skills, also from a launch CODEX_HOME",
        source: "qualification run 2026-11-15 (prompt-input probes, real configuration and launch home)",
        limitation: "plugin and system skills are not switched off; a folder path is ignored by Codex, so only SKILL.md paths are used; concurrent interactive sessions and the shared-server separation were not observed again on this build; provider use of each skill is not observed",
    },
    ArsenalEvidence {
        qualified_version: "codex-cli 0.159.0",
        checked_on: "2026-10-31",
        operation: "--no-daemon and -c skills.config=[{path=<SKILL.md>, enabled=false}] for installed user, home and project skills",
        source: "qualification run 2026-10-31 (prompt-input probes and two concurrent interactive sessions)",
        limitation: "plugin and system skills are not switched off; a folder path is ignored by Codex, so only SKILL.md paths are used; the session does not join the shared Codex server; provider use of each skill is not observed",
    },
];

/// A build that exists only in tests, so the enabled path is exercised without qualifying a real one.
#[cfg(test)]
pub const ARSENAL_TEST_VERSION: &str = "0.0.0-arsenal-test (Claude Code)";
#[cfg(test)]
const ARSENAL_TEST: ArsenalEvidence = ArsenalEvidence {
    qualified_version: ARSENAL_TEST_VERSION,
    checked_on: "test",
    operation: "skillOverrides=off through --settings",
    source: "unit test fixture",
    limitation: "not a real build",
};

/// The exclusion evidence for this exact build, if any. OpenCode and Hermes have none.
pub fn arsenal_exclusion(agent: AgentId, version: Option<&str>) -> Option<ArsenalEvidence> {
    let version = version.filter(|v| !v.trim().is_empty())?;
    let records = match agent {
        AgentId::ClaudeCode => ARSENAL_CLAUDE,
        AgentId::Codex => ARSENAL_CODEX,
        _ => return None,
    };
    #[cfg(test)]
    if agent == AgentId::ClaudeCode && version == ARSENAL_TEST_VERSION {
        return Some(ARSENAL_TEST);
    }
    let exact = records
        .iter()
        .find(|record| record.qualified_version == version)
        .copied();
    // Claude Code can use the fallback record. Codex requires an exact build match.
    match agent {
        AgentId::ClaudeCode => exact.or(Some(CLAUDE_ANY_BUILD)),
        _ => exact,
    }
}

/// Fallback for a reported Claude Code build without an exact record.
const CLAUDE_ANY_BUILD: ArsenalEvidence = ArsenalEvidence {
    qualified_version: "any Claude Code build (not checked build by build)",
    checked_on: "2026-11-15",
    operation: "--settings overlay holding only skillOverrides=off for installed user and project skills",
    source: "owner decision 2026-11-15; last focused run 2026-11-12",
    limitation: "this build was not checked; plugin and built-in skills are not switched off; provider use of each skill is not observed",
};

pub fn permits_arsenal_exclusion(agent: AgentId, version: Option<&str>) -> bool {
    arsenal_exclusion(agent, version).is_some()
}

/// A Claude Code launch home requires a nonempty reported version.
/// Codex launch homes are validated separately.
pub fn permits_arsenal_home(agent: AgentId, version: Option<&str>) -> bool {
    agent == AgentId::ClaudeCode && version.is_some_and(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_applies_on_any_reported_build_and_codex_keeps_its_exact_records() {
        for version in [
            "2.1.309 (Claude Code)",
            "2.1.322 (Claude Code)",
            "2.1.325 (Claude Code)",
            "9.9.9 (Claude Code)",
        ] {
            assert!(
                permits_arsenal_exclusion(AgentId::ClaudeCode, Some(version)),
                "{version}"
            );
            assert!(
                permits_arsenal_home(AgentId::ClaudeCode, Some(version)),
                "{version}"
            );
            let evidence = arsenal_exclusion(AgentId::ClaudeCode, Some(version)).unwrap();
            assert!(evidence.limitation.contains("not checked"), "{version}");
        }
        // A checked build keeps its own record.
        let checked =
            arsenal_exclusion(AgentId::ClaudeCode, Some("2.1.323 (Claude Code)")).unwrap();
        assert_eq!(checked.qualified_version, "2.1.323 (Claude Code)");
        // An unknown version is still refused.
        assert!(!permits_arsenal_exclusion(AgentId::ClaudeCode, None));
        assert!(!permits_arsenal_home(AgentId::ClaudeCode, None));
        for blank in ["", "  "] {
            assert!(!permits_arsenal_exclusion(AgentId::ClaudeCode, Some(blank)));
            assert!(!permits_arsenal_home(AgentId::ClaudeCode, Some(blank)));
        }
        // Codex: only the exact qualified builds; it never uses the Claude home.
        assert!(permits_arsenal_exclusion(
            AgentId::Codex,
            Some("codex-cli 0.169.0")
        ));
        assert!(permits_arsenal_exclusion(
            AgentId::Codex,
            Some("codex-cli 0.159.0")
        ));
        for other in [
            "codex-cli 0.158.0",
            "codex-cli 0.160.0",
            "codex-cli 0.170.0",
            "0.159.0",
        ] {
            assert!(
                !permits_arsenal_exclusion(AgentId::Codex, Some(other)),
                "{other}"
            );
        }
        assert!(!permits_arsenal_exclusion(AgentId::Codex, None));
        assert!(!permits_arsenal_home(
            AgentId::Codex,
            Some("codex-cli 0.169.0")
        ));
        assert!(!permits_arsenal_exclusion(AgentId::Opencode, Some("1.0.0")));
        // Profile syntax still does not depend on the build.
        assert!(permits_profile_overrides(
            AgentId::ClaudeCode,
            Some("2.1.314 (Claude Code)")
        ));
    }

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
        assert_eq!(
            codex.interface,
            Some("-m, -c KEY=VALUE, -s, -a and --dangerously-bypass-approvals-and-sandbox")
        );
        assert!(
            codex
                .operations
                .iter()
                .any(|o| o.contains("--dangerously-bypass-approvals-and-sandbox"))
        );

        let installed = evidence(AgentId::ClaudeCode, Some("2.1.316 (Claude Code)"));
        assert_eq!(installed.status, Status::SyntaxVerified);
        assert!(
            installed
                .operations
                .contains(&"full access via --dangerously-skip-permissions")
        );
        assert!(permits_profile_overrides(
            AgentId::ClaudeCode,
            Some("2.1.316 (Claude Code)")
        ));
        let updated = evidence(AgentId::ClaudeCode, Some("2.1.319 (Claude Code)"));
        assert_eq!(updated.status, Status::SyntaxVerified);
        assert!(
            updated
                .operations
                .contains(&"full access via --dangerously-skip-permissions (--help, 2026-11-07)")
        );
        assert!(permits_profile_overrides(
            AgentId::ClaudeCode,
            Some("2.1.319 (Claude Code)")
        ));
        for neighbour in [
            "2.1.315 (Claude Code)",
            "2.1.317 (Claude Code)",
            "2.1.318 (Claude Code)",
            "2.1.320 (Claude Code)",
            "2.1.316",
            "2.1.319",
        ] {
            assert_eq!(
                evidence(AgentId::ClaudeCode, Some(neighbour)).status,
                Status::SyntaxVerified,
                "{neighbour}"
            );
        }
        assert_eq!(
            codex.scope,
            Some("one Native child process; ~/.codex/config.toml is not written")
        );

        let opencode = evidence(AgentId::Opencode, Some("1.19.4"));
        assert_eq!(opencode.status, Status::SyntaxVerified);
        assert!(opencode.operations.contains(&"model via --model"));
        assert_eq!(
            evidence(AgentId::Opencode, Some("1.19.5")).status,
            Status::SyntaxVerified
        );

        let hermes = evidence(AgentId::Hermes, None);
        assert_eq!(hermes.status, Status::Unverified);
        assert!(hermes.limitation.contains("performs an update check"));
        assert!(!permits_profile_overrides(AgentId::Hermes, None));
    }

    #[test]
    fn a_missing_version_has_no_evidence_and_a_changed_one_uses_its_agents_record() {
        assert_eq!(
            evidence(AgentId::ClaudeCode, None).status,
            Status::Unverified
        );
        assert!(!permits_profile_overrides(AgentId::ClaudeCode, None));
        for (agent, version) in [
            (AgentId::ClaudeCode, "2.1.321 (Claude Code)"),
            (AgentId::Codex, "codex-cli 0.160.0"),
            (AgentId::Codex, "codex-cli 0.159.0-dev"),
            (AgentId::Opencode, "9.99.0"),
        ] {
            assert_eq!(
                evidence(agent, Some(version)).status,
                Status::SyntaxVerified
            );
            assert!(permits_profile_overrides(agent, Some(version)));
        }
        // Hermes has no override syntax whatever its version.
        assert!(!permits_profile_overrides(AgentId::Hermes, Some("1.0.0")));
    }

    #[test]
    fn a_build_without_a_record_launches_with_or_without_profile_overrides() {
        assert!(allows_start_with_profile(
            AgentId::ClaudeCode,
            Some("2.1.310 (Claude Code)"),
            false
        ));
        assert!(allows_start_with_profile(
            AgentId::ClaudeCode,
            Some("2.1.310 (Claude Code)"),
            true
        ));
        assert!(!allows_start_with_profile(AgentId::ClaudeCode, None, true));
        assert!(allows_start_with_profile(
            AgentId::ClaudeCode,
            Some("2.1.309 (Claude Code)"),
            true
        ));
    }
}
