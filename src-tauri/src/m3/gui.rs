//! What the window is told about M3: the four provider profiles and why each can or cannot
//! start, the model-selection and history capabilities and their state, and, only in a build that
//! hosts a fixture runtime, that runtime's conversation, approvals, usage and model facts.
//!
//! The projection is closed and carries no content of its own: the only text in it is what the
//! user is shown to decide on (a held action) and what the user and the model said in a
//! fixture conversation. It reads; it starts nothing. In every packaged build the only
//! thing that exists is `m3_state`, and everything in it says "not available" for Claude.
use super::{
    CAPABILITY_VERSION, gate,
    history::SinkState,
    model::{CAPABILITY, CAPABILITY_VERSION as MODEL_VERSION, ModelFacts},
    profiles::{self, Card, ClaudeInstall, CodexInstall, Host},
    report::ProviderReport,
};
use crate::f8::claude::launch::ClaudeInstall as InstallConfig;
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum InstallView {
    NotConfigured,
    /// The SDK installation is being hashed against its record files (about a second).
    Verifying,
    Refused {
        reason: String,
    },
    Verified {
        sdk_version: String,
        cli_version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeatureView {
    pub capability: &'static str,
    pub version: u32,
    /// Whether the feature may be used. Closed in this source.
    pub available: bool,
    /// Stable blocker codes, in the order the owner would resolve them.
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryView {
    /// `off` (default), `unavailable`, `recording` or `partial`.
    pub state: &'static str,
    pub enabled: bool,
    pub can_enable: bool,
    pub reasons: Vec<String>,
    pub retention_days: i64,
    pub max_records: i64,
    pub records: Option<i64>,
}

/// One held action, complete: the user decides on exactly this.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApprovalView {
    pub item: String,
    pub kind: String,
    pub fingerprint: String,
    /// The `ProviderAction` as the approval machinery holds it (command, cwd, or file diffs).
    pub action: serde_json::Value,
    pub decisions: Vec<&'static str>,
    /// How the decision ended: `pending`, `accepted`, `declined`, `cancelled`, `confirmed`,
    /// `failed` or `indeterminate`.
    pub state: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub role: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnView {
    pub id: String,
    /// `starting`, `running`, `completed`, `failed`, `interrupted`.
    pub state: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FixtureView {
    pub label: &'static str,
    /// `stopped`, `starting`, `ready`, `refused`, `failed`.
    pub phase: &'static str,
    pub profile: Option<&'static str>,
    pub conversation: Vec<Message>,
    pub turn: Option<TurnView>,
    pub approvals: Vec<ApprovalView>,
    pub report: Option<ProviderReport>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct M3State {
    pub capability: &'static str,
    pub version: &'static str,
    pub profiles: Vec<Card>,
    pub claude_install: InstallView,
    pub model_selection: FeatureView,
    pub history: HistoryView,
    pub fixture: Option<FixtureView>,
}

/// Where the fixture state comes from; implemented only by the fixture host.
pub trait FixtureSource: Send + Sync {
    fn view(&self) -> FixtureView;
}

enum Verify {
    NotStarted,
    Running,
    Done(InstallView),
}

struct Inner {
    install: Option<InstallConfig>,
    verify: Mutex<Verify>,
    fixture: Option<Arc<dyn FixtureSource>>,
    sink: Mutex<SinkState>,
}

/// The desktop's M3 service. `inactive()` is what every packaged entry point builds.
#[derive(Clone)]
pub struct M3Service {
    inner: Arc<Inner>,
}

impl M3Service {
    pub fn inactive() -> Self {
        Self::build(None, None)
    }
    /// A service that verifies the named installation (read-only) and, optionally, shows a
    /// fixture runtime.
    pub fn with(install: Option<InstallConfig>, fixture: Option<Arc<dyn FixtureSource>>) -> Self {
        Self::build(install, fixture)
    }
    fn build(install: Option<InstallConfig>, fixture: Option<Arc<dyn FixtureSource>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                install,
                verify: Mutex::new(Verify::NotStarted),
                fixture,
                sink: Mutex::new(SinkState::Off),
            }),
        }
    }

    fn install_view(&self) -> InstallView {
        let Some(install) = self.inner.install.clone() else {
            return InstallView::NotConfigured;
        };
        let mut v = self.inner.verify.lock().unwrap_or_else(|e| e.into_inner());
        match &*v {
            Verify::Done(view) => return view.clone(),
            Verify::Running => return InstallView::Verifying,
            Verify::NotStarted => (),
        }
        *v = Verify::Running;
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let view = match install.verify() {
                Ok(ok) => InstallView::Verified {
                    sdk_version: ok.sdk_version,
                    cli_version: ok.cli_version,
                },
                Err(e) => InstallView::Refused {
                    reason: e.code().to_owned(),
                },
            };
            *inner.verify.lock().unwrap_or_else(|e| e.into_inner()) = Verify::Done(view);
        });
        InstallView::Verifying
    }

    pub fn state(&self, codex_active: bool) -> M3State {
        let install = self.install_view();
        let claude = match &install {
            InstallView::NotConfigured => ClaudeInstall::NotConfigured,
            // Until the check ends, nothing is claimed about the installation.
            InstallView::Verifying => ClaudeInstall::Refused {
                reason: "verifying".into(),
            },
            InstallView::Refused { reason } => ClaudeInstall::Refused {
                reason: reason.clone(),
            },
            InstallView::Verified {
                sdk_version,
                cli_version,
            } => ClaudeInstall::Verified {
                sdk_version: sdk_version.clone(),
                cli_version: cli_version.clone(),
            },
        };
        let host = Host {
            linux_x86_64: std::env::consts::OS == "linux" && std::env::consts::ARCH == "x86_64",
            claude,
            codex: if codex_active {
                CodexInstall::Connected {
                    runtime_version: None,
                }
            } else {
                CodexInstall::NotConnected
            },
        };
        let blockers = gate::model_selection_blockers();
        let history_blockers = gate::metrics_history_blockers();
        let sink = *self.inner.sink.lock().unwrap_or_else(|e| e.into_inner());
        M3State {
            capability: "m3",
            version: CAPABILITY_VERSION,
            profiles: profiles::cards(&host),
            claude_install: install,
            model_selection: FeatureView {
                capability: CAPABILITY,
                version: MODEL_VERSION,
                available: blockers.is_empty(),
                reasons: blockers.iter().map(|b| b.code().to_owned()).collect(),
            },
            history: HistoryView {
                state: match sink {
                    SinkState::Off => "off",
                    SinkState::Recording => "recording",
                    SinkState::Partial(_) => "partial",
                },
                enabled: !matches!(sink, SinkState::Off),
                can_enable: history_blockers.is_empty(),
                reasons: history_blockers
                    .iter()
                    .map(|b| b.code().to_owned())
                    .collect(),
                retention_days: super::history::MAX_RETENTION_SECS / 86_400,
                max_records: super::history::MAX_RECORDS,
                records: None,
            },
            fixture: self.inner.fixture.as_ref().map(|f| f.view()),
        }
    }
}

/// The model facts of the fixture runtime, for the window's four-column view.
pub fn facts_of(report: &ProviderReport) -> Option<&ModelFacts> {
    report.model.as_ref()
}

#[tauri::command]
pub fn m3_state(
    service: tauri::State<'_, M3Service>,
    f8: tauri::State<'_, crate::desktop::F8>,
) -> M3State {
    service.state(f8.is_active())
}

#[cfg(test)]
mod tests;
