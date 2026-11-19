//! The registry of native sessions: what the Console knows about the agents it
//! started, kept in one private file so that it survives the window. A process is not kept:
//! after a restart every session that was running is `lost`, because the Console did not see
//! it end and its terminal died with the window.
use super::{catalogue::AgentId, store};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_SESSIONS: usize = 200;
pub const MAX_PROJECTS: usize = 100;
const MAX_FILE: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// A start was accepted and is being prepared (shown as "restarting" for a restart).
    Requested,
    Running,
    /// The process ended by itself; the code is known.
    Exited,
    /// The user stopped it.
    Stopped,
    FailedStart,
    /// The Console restarted and cannot say what became of it.
    Lost,
}

impl State {
    pub fn finished(self) -> bool {
        !matches!(self, Self::Requested | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileRef {
    pub id: String,
    pub name: String,
    /// The version it started with: a running session keeps it.
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub agent_id: AgentId,
    pub agent_name: String,
    pub version: Option<String>,
    pub project: String,
    pub profile: Option<ProfileRef>,
    /// Whether the user's shell started it (so the environment is the user's) or the
    /// Console did, directly.
    pub through_shell: bool,
    pub state: State,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub exit_code: Option<u32>,
    pub exit_signal: Option<String>,
    pub error: Option<String>,
    pub restart_of: Option<String>,
    pub restarts: u32,
    /// The Arsenal exclusion it was launched with; a restart launches with the same one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arsenal: Option<super::arsenal::ArsenalLaunch>,
    /// Started as an orchestrator: it was given the Wyss delegation tool.
    #[serde(default)]
    pub delegation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub path: String,
    #[serde(default)]
    pub default_agent: Option<AgentId>,
    #[serde(default)]
    pub default_profile: Option<String>,
    pub last_used: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    pub schema: u32,
    pub sessions: Vec<Session>,
    pub projects: Vec<Project>,
}

pub struct Registry {
    path: PathBuf,
    pub data: Data,
}

pub struct M9Open {
    pub registry: Registry,
    pub lost: Vec<String>,
    pub extended_sessions: Vec<Session>,
    pub extended_defaults: Vec<(String, AgentId)>,
}

impl Registry {
    /// Reads shared registry files that also contain extended providers. The returned shared
    /// registry contains only IDs from the shared provider list. The caller durably saves the
    /// extracted extended sessions and defaults before saving the shared registry.
    pub fn open_for_m9(dir: PathBuf) -> std::io::Result<M9Open> {
        store::private_dir(&dir)?;
        let path = dir.join("registry.json");
        let mut data: Data = store::read_bounded(&path, MAX_FILE)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        data.schema = 1;
        let mut lost = Vec::new();
        for session in &mut data.sessions {
            if !session.state.finished() {
                session.state = State::Lost;
                session.ended_at = None;
                lost.push(session.id.clone());
            }
        }
        let mut extended_sessions = Vec::new();
        data.sessions.retain(|session| {
            if session.agent_id.fits_shared_registry() {
                true
            } else {
                extended_sessions.push(session.clone());
                false
            }
        });
        let mut extended_defaults = Vec::new();
        for project in &mut data.projects {
            if let Some(agent) = project.default_agent
                && !agent.fits_shared_registry()
            {
                extended_defaults.push((project.path.clone(), agent));
                project.default_agent = None;
            }
        }
        Ok(M9Open {
            registry: Self { path, data },
            lost,
            extended_sessions,
            extended_defaults,
        })
    }

    pub fn save(&self) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.data).map_err(std::io::Error::other)?;
        store::write_atomic(&self.path, &bytes)
    }

    pub fn session(&self, id: &str) -> Option<&Session> {
        self.data.sessions.iter().find(|s| s.id == id)
    }

    pub fn session_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.data.sessions.iter_mut().find(|s| s.id == id)
    }

    /// Adds a session; past the bound the oldest finished ones are forgotten.
    pub fn add(&mut self, session: Session) {
        self.data.sessions.push(session);
        while self.data.sessions.len() > MAX_SESSIONS {
            match self.data.sessions.iter().position(|s| s.state.finished()) {
                Some(at) => {
                    self.data.sessions.remove(at);
                }
                None => break,
            }
        }
    }

    pub fn forget(&mut self, id: &str) -> bool {
        let before = self.data.sessions.len();
        self.data.sessions.retain(|s| s.id != id);
        before != self.data.sessions.len()
    }

    pub fn project_mut(&mut self, path: &str) -> &mut Project {
        if let Some(at) = self.data.projects.iter().position(|p| p.path == path) {
            return &mut self.data.projects[at];
        }
        if self.data.projects.len() >= MAX_PROJECTS
            && let Some(oldest) = self
                .data
                .projects
                .iter()
                .enumerate()
                .min_by_key(|(_, p)| p.last_used)
                .map(|(i, _)| i)
        {
            self.data.projects.remove(oldest);
        }
        self.data.projects.push(Project {
            path: path.to_owned(),
            default_agent: None,
            default_profile: None,
            last_used: 0,
        });
        self.data.projects.last_mut().expect("just pushed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Schema 1 fixture with built-in and custom session profiles and a project default profile.
    const WRITTEN_WITH_PROFILES: &str = r#"{
  "schema": 1,
  "sessions": [
    {
      "id": "11111111111111111111111111111111",
      "agent_id": "claude_code",
      "agent_name": "Claude Code",
      "version": "2.1.319 (Claude Code)",
      "project": "/home/owner/work",
      "profile": { "id": "template-full-access", "name": "Full access (bypass)", "version": 0 },
      "through_shell": true,
      "state": "exited",
      "started_at": 1762000000,
      "ended_at": 1762000100,
      "exit_code": 0,
      "exit_signal": null,
      "error": null,
      "restart_of": null,
      "restarts": 0,
      "delegation": false
    },
    {
      "id": "22222222222222222222222222222222",
      "agent_id": "codex",
      "agent_name": "Codex",
      "version": null,
      "project": "/home/owner/work",
      "profile": { "id": "0123456789abcdef0123456789abcdef", "name": "Estudio", "version": 3 },
      "through_shell": true,
      "state": "running",
      "started_at": 1762000200,
      "ended_at": null,
      "exit_code": null,
      "exit_signal": null,
      "error": null,
      "restart_of": "11111111111111111111111111111111",
      "restarts": 1,
      "delegation": true
    },
    {
      "id": "33333333333333333333333333333333",
      "agent_id": "claude_code",
      "agent_name": "Claude Code",
      "version": null,
      "project": "/home/owner",
      "profile": { "id": "template-exploration", "name": "Exploration, fewer hooks", "version": 0 },
      "through_shell": false,
      "state": "stopped",
      "started_at": 1762000300,
      "ended_at": 1762000400,
      "exit_code": null,
      "exit_signal": "SIGTERM",
      "error": null,
      "restart_of": null,
      "restarts": 0,
      "delegation": false
    },
    {
      "id": "44444444444444444444444444444444",
      "agent_id": "codex",
      "agent_name": "Codex",
      "version": null,
      "project": "/home/owner",
      "profile": null,
      "through_shell": true,
      "state": "lost",
      "started_at": 1762000500,
      "ended_at": null,
      "exit_code": null,
      "exit_signal": null,
      "error": null,
      "restart_of": null,
      "restarts": 0,
      "delegation": false
    }
  ],
  "projects": [
    {
      "path": "/home/owner/work",
      "default_agent": "claude_code",
      "default_profile": "0123456789abcdef0123456789abcdef",
      "last_used": 1762000200
    },
    {
      "path": "/home/owner",
      "default_agent": null,
      "default_profile": "template-full-access",
      "last_used": 1762000500
    },
    { "path": "/home/owner/other", "default_agent": null, "default_profile": null, "last_used": 0 }
  ]
}"#;

    #[test]
    fn schema_one_registry_preserves_saved_session_and_project_profiles() {
        let dir = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("wyss-registry-{}", uuid::Uuid::new_v4().simple()));
        store::private_dir(&dir).unwrap();
        std::fs::write(dir.join("registry.json"), WRITTEN_WITH_PROFILES).unwrap();
        // A file that did not parse would open as an empty registry: every record is counted.
        let opened = Registry::open_for_m9(dir.clone()).unwrap();
        let registry = opened.registry;
        assert_eq!(registry.data.sessions.len(), 4);
        assert_eq!(registry.data.projects.len(), 3);
        // The profile a session started with is kept as it was recorded, so a restart names it.
        let profile = |id: &str| registry.session(id).unwrap().profile.clone();
        assert_eq!(
            profile(&"1".repeat(32)),
            Some(ProfileRef {
                id: "template-full-access".into(),
                name: "Full access (bypass)".into(),
                version: 0,
            })
        );
        assert_eq!(
            profile(&"2".repeat(32)),
            Some(ProfileRef {
                id: "0123456789abcdef0123456789abcdef".into(),
                name: "Estudio".into(),
                version: 3,
            })
        );
        assert_eq!(
            profile(&"3".repeat(32)).map(|p| p.id),
            Some("template-exploration".to_owned())
        );
        assert_eq!(profile(&"4".repeat(32)), None);
        // The one that was running when the file was written is lost, as any reopening says.
        assert_eq!(opened.lost, vec!["2".repeat(32)]);
        // The registry preserves a saved project's default_profile field verbatim.
        let defaults: Vec<Option<&str>> = registry
            .data
            .projects
            .iter()
            .map(|p| p.default_profile.as_deref())
            .collect();
        assert_eq!(
            defaults,
            [
                Some("0123456789abcdef0123456789abcdef"),
                Some("template-full-access"),
                None
            ]
        );
        registry.save().unwrap();
        let again = Registry::open_for_m9(dir.clone()).unwrap().registry;
        assert_eq!(again.data.sessions.len(), 4);
        assert_eq!(
            again.data.projects[0].default_profile.as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            again.data.projects[0].default_agent,
            Some(AgentId::ClaudeCode)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
