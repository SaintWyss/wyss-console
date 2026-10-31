//! The registry of native sessions (F3b, 05 §5): what the Console knows about the agents it
//! started, kept in one private file so that it survives the window. A process is not kept:
//! after a restart every session that was running is `lost`, because the Console did not see
//! it end and its terminal died with the window (until F9).
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
    /// The version it started with: a running session keeps it (09 §4.4).
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
    pub arsenal: Option<super::arsenal::ArsenalRef>,
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
    /// Opens the shared registry without committing it so M9 can first move records written by
    /// an earlier M9 build into its sidecar. The returned shared registry contains only IDs an
    /// M1-M6 binary can deserialize; the caller must durably save the returned M9 records and
    /// defaults before saving this projection.
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
            if session.agent_id.is_legacy() {
                true
            } else {
                extended_sessions.push(session.clone());
                false
            }
        });
        let mut extended_defaults = Vec::new();
        for project in &mut data.projects {
            if let Some(agent) = project.default_agent
                && !agent.is_legacy()
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
