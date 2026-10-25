//! Which Managed provider profile can be started, and the reason when it cannot.
//!
//! One card per profile, all four always listed, each with the exact build and the evidence it
//! rests on. Availability is a computation over facts, never a setting:
//!
//! - `Unsupported`: this host cannot run the profile at all (platform);
//! - `Blocked`: a decision or evidence this source needs is missing (the gates in `gate`);
//! - `Unverified`: nothing is blocked by a decision, but this host's installation has not been
//!   verified against the exact pinned build;
//! - `Available`: the only state in which a start action exists.
//!
//! Fixture evidence never moves a profile toward `Available`: the real-provider evidence is its
//! own gate constant.
use super::{
    gate::{self, Blocker},
    usage::Provider,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Inference,
    LocalTools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileId {
    CodexInference,
    CodexLocalTools,
    ClaudeInference,
    ClaudeLocalTools,
}
impl ProfileId {
    pub const ALL: [Self; 4] = [
        Self::CodexInference,
        Self::CodexLocalTools,
        Self::ClaudeInference,
        Self::ClaudeLocalTools,
    ];
    pub const fn provider(self) -> Provider {
        match self {
            Self::CodexInference | Self::CodexLocalTools => Provider::Codex,
            Self::ClaudeInference | Self::ClaudeLocalTools => Provider::Claude,
        }
    }
    pub const fn mode(self) -> Mode {
        match self {
            Self::CodexInference | Self::ClaudeInference => Mode::Inference,
            Self::CodexLocalTools | Self::ClaudeLocalTools => Mode::LocalTools,
        }
    }
    /// The attachment profile name the runtime presents.
    pub const fn name(self) -> &'static str {
        match self {
            Self::CodexInference => "codex_inference_v1",
            Self::CodexLocalTools => "codex_local_tools_v1",
            Self::ClaudeInference => "claude_inference_v1",
            Self::ClaudeLocalTools => "claude_local_tools_v1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unverified,
    Blocked,
    Unsupported,
}

/// What this host's Claude installation looks like, as `f8::claude::launch` verified it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ClaudeInstall {
    NotConfigured,
    /// Verification refused it, with the launcher's stable reason code.
    Refused {
        reason: String,
    },
    Verified {
        sdk_version: String,
        cli_version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum CodexInstall {
    /// The protocol-2 authority this window is connected to runs it.
    Connected {
        runtime_version: Option<String>,
    },
    NotConnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub linux_x86_64: bool,
    pub claude: ClaudeInstall,
    pub codex: CodexInstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    /// Fixtures, unit and integration tests on this source.
    Passed,
    /// Needs an authorized run against the real provider.
    NotRun,
    /// Does not apply to this profile.
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub fixture: EvidenceState,
    pub real_provider: EvidenceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Card {
    pub id: ProfileId,
    pub provider: Provider,
    pub mode: Mode,
    pub name: &'static str,
    pub availability: Availability,
    /// Why it is not available: gate blockers first, then installation facts. Stable codes.
    pub reasons: Vec<String>,
    pub build: Option<(String, String)>,
    pub evidence: Evidence,
    pub can_start: bool,
}

fn blockers(id: ProfileId) -> Vec<Blocker> {
    match id {
        ProfileId::ClaudeInference => gate::claude_inference_blockers(),
        ProfileId::ClaudeLocalTools => gate::claude_local_tools_blockers(),
        // Codex Managed is the accepted F8 path: it has no M3 blocker of its own.
        ProfileId::CodexInference | ProfileId::CodexLocalTools => Vec::new(),
    }
}

pub fn card(id: ProfileId, host: &Host) -> Card {
    let mut reasons: Vec<String> = blockers(id).iter().map(|b| b.code().to_owned()).collect();
    let mut build = None;
    let mut unverified = false;
    match id.provider() {
        Provider::Claude => match &host.claude {
            ClaudeInstall::NotConfigured => {
                reasons.push("sdk_not_configured".into());
                unverified = true;
            }
            ClaudeInstall::Refused { reason } => {
                reasons.push(reason.clone());
                unverified = true;
            }
            ClaudeInstall::Verified {
                sdk_version,
                cli_version,
            } => build = Some((sdk_version.clone(), cli_version.clone())),
        },
        Provider::Codex => match &host.codex {
            CodexInstall::Connected { runtime_version } => {
                build = runtime_version.clone().map(|v| (String::new(), v));
            }
            CodexInstall::NotConnected => {
                reasons.push("authority_not_connected".into());
                unverified = true;
            }
        },
    }
    let availability = if !host.linux_x86_64 {
        reasons.insert(0, "platform_unsupported".into());
        Availability::Unsupported
    } else if !blockers(id).is_empty() {
        Availability::Blocked
    } else if unverified {
        Availability::Unverified
    } else {
        Availability::Available
    };
    let evidence = match id.provider() {
        // Codex's existing evidence is the accepted F8 record, not part of this milestone.
        Provider::Codex => Evidence {
            fixture: EvidenceState::Passed,
            real_provider: EvidenceState::Passed,
        },
        Provider::Claude => Evidence {
            fixture: EvidenceState::Passed,
            real_provider: EvidenceState::NotRun,
        },
    };
    Card {
        id,
        provider: id.provider(),
        mode: id.mode(),
        name: id.name(),
        availability,
        reasons,
        build,
        evidence,
        can_start: availability == Availability::Available,
    }
}

/// All four profiles, in a fixed order.
pub fn cards(host: &Host) -> Vec<Card> {
    ProfileId::ALL
        .into_iter()
        .map(|id| card(id, host))
        .collect()
}

#[cfg(test)]
mod tests;
