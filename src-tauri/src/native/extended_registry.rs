//! M9-only Native session records. Older M1-M6 binaries deserialize `AgentId` as a closed
//! two-variant enum and rewrite an unreadable registry as empty, so new provider IDs must live
//! outside `registry.json`. This sidecar is ignored by those binaries and never stores output.
use super::{
    registry::{Session, State},
    store,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::PathBuf};

const MAX_FILE: u64 = 2 * 1024 * 1024;
const MAX_SESSIONS: usize = 200;
const FILE_NAME: &str = "m9-native-sessions-v1.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u32,
    sessions: Vec<Session>,
    #[serde(default)]
    project_defaults: BTreeMap<String, super::catalogue::AgentId>,
}

pub struct ExtendedRegistry {
    path: PathBuf,
    data: Data,
}

impl ExtendedRegistry {
    pub fn open(dir: PathBuf) -> io::Result<Self> {
        store::private_dir(&dir)?;
        let path = dir.join(FILE_NAME);
        let mut data = if path.exists() {
            let bytes = store::read_bounded(&path, MAX_FILE).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "m9_registry_unreadable")
            })?;
            serde_json::from_slice::<Data>(&bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "m9_registry_invalid"))?
        } else {
            Data::default()
        };
        if data.schema != 0 && data.schema != 1
            || data.sessions.len() > MAX_SESSIONS
            || data
                .sessions
                .iter()
                .any(|session| session.agent_id.is_legacy())
            || data.project_defaults.len() > super::registry::MAX_PROJECTS
            || data
                .project_defaults
                .iter()
                .any(|(path, agent)| path.len() > 4096 || agent.is_legacy())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "m9_registry_schema",
            ));
        }
        data.schema = 1;
        for session in &mut data.sessions {
            if !session.state.finished() {
                session.state = State::Lost;
                session.ended_at = None;
            }
        }
        let registry = Self { path, data };
        registry.save()?;
        Ok(registry)
    }

    pub fn sessions(&self) -> &[Session] {
        &self.data.sessions
    }

    pub fn project_default(&self, path: &str) -> Option<super::catalogue::AgentId> {
        self.data.project_defaults.get(path).copied()
    }

    pub fn set_project_default(
        &mut self,
        path: &str,
        agent: super::catalogue::AgentId,
    ) -> io::Result<()> {
        if agent.is_legacy() || path.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "m9_registry_project_default",
            ));
        }
        if !self.data.project_defaults.contains_key(path)
            && self.data.project_defaults.len() >= super::registry::MAX_PROJECTS
        {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "m9_registry_project_defaults_full",
            ));
        }
        self.data.project_defaults.insert(path.to_owned(), agent);
        self.save()
    }

    pub fn migrate_session(&mut self, session: Session) -> io::Result<()> {
        if session.agent_id.is_legacy() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "m9_registry_agent",
            ));
        }
        if self
            .data
            .sessions
            .iter()
            .any(|existing| existing.id == session.id)
        {
            return Ok(());
        }
        self.add(session)
    }

    pub fn migrate_project_default(
        &mut self,
        path: String,
        agent: super::catalogue::AgentId,
    ) -> io::Result<()> {
        if agent.is_legacy() || path.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "m9_registry_project_default",
            ));
        }
        if !self.data.project_defaults.contains_key(&path)
            && self.data.project_defaults.len() >= super::registry::MAX_PROJECTS
        {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "m9_registry_project_defaults_full",
            ));
        }
        self.data.project_defaults.entry(path).or_insert(agent);
        self.save()
    }

    pub fn session(&self, id: &str) -> Option<&Session> {
        self.data.sessions.iter().find(|session| session.id == id)
    }

    pub fn session_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
    }

    pub fn add(&mut self, session: Session) -> io::Result<()> {
        if session.agent_id.is_legacy() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "m9_registry_agent",
            ));
        }
        if self
            .data
            .sessions
            .iter()
            .any(|existing| existing.id == session.id)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "m9_registry_session_duplicate",
            ));
        }
        self.data.sessions.push(session);
        while self.data.sessions.len() > MAX_SESSIONS {
            let Some(oldest) = self.data.sessions.iter().position(|s| s.state.finished()) else {
                self.data.sessions.pop();
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "m9_registry_full",
                ));
            };
            self.data.sessions.remove(oldest);
        }
        self.save()
    }

    pub fn forget(&mut self, id: &str) -> bool {
        let before = self.data.sessions.len();
        self.data.sessions.retain(|session| session.id != id);
        before != self.data.sessions.len()
    }

    pub fn save(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.data).map_err(io::Error::other)?;
        store::write_atomic(&self.path, &bytes)
    }
}
