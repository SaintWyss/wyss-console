//! The closed list of agents the Console can start (05 §4.1). Compiled in: the page names an
//! `agent_id`, never an executable.
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentId {
    ClaudeCode,
    Codex,
    Opencode,
    Hermes,
}

pub struct AgentSpec {
    pub id: AgentId,
    pub name: &'static str,
    pub executable: &'static str,
}

pub const AGENTS: [AgentSpec; 4] = [
    AgentSpec {
        id: AgentId::ClaudeCode,
        name: "Claude Code",
        executable: "claude",
    },
    AgentSpec {
        id: AgentId::Codex,
        name: "Codex",
        executable: "codex",
    },
    AgentSpec {
        id: AgentId::Opencode,
        name: "OpenCode",
        executable: "opencode",
    },
    AgentSpec {
        id: AgentId::Hermes,
        name: "Hermes Agent (Native)",
        executable: "hermes",
    },
];

/// The folders searched besides `PATH`: where the agents install themselves for one user, so
/// a window started from a menu (which has not read the shell's start-up files) still finds
/// what the user's shell will find.
fn extra_folders(home: &Path) -> Vec<PathBuf> {
    [".local/bin", ".bun/bin", ".npm-global/bin", ".cargo/bin"]
        .iter()
        .map(|part| home.join(part))
        .chain(["/usr/local/bin", "/usr/bin"].iter().map(PathBuf::from))
        .collect()
}

fn executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The absolute path of the agent's executable: the first regular executable file of that
/// name in `PATH`, then in the usual per-user folders. Links are resolved.
pub fn discover(spec: &AgentSpec, path_var: Option<OsString>, home: &Path) -> Option<PathBuf> {
    let mut folders: Vec<PathBuf> = path_var
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    folders.extend(extra_folders(home));
    folders
        .into_iter()
        .filter(|folder| folder.is_absolute())
        .map(|folder| folder.join(spec.executable))
        .find(|candidate| executable(candidate))
        .and_then(|candidate| candidate.canonicalize().ok())
}

/// `--version` in a process of its own, bounded in time and size. The first line, cleaned.
pub fn probe_version(path: &Path) -> Option<String> {
    let mut child = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; 512];
        let n = stdout.read(&mut buffer).unwrap_or(0);
        buffer.truncate(n);
        let _ = send.send(buffer);
    });
    let bytes = receive.recv_timeout(Duration::from_secs(4)).ok();
    let _ = child.kill();
    let _ = child.wait();
    let text = String::from_utf8_lossy(&bytes?).into_owned();
    let line: String = text
        .lines()
        .next()?
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect();
    let line = line.trim().to_owned();
    (!line.is_empty()).then_some(line)
}

/// Some provider version commands are not read-only. Hermes `--version` performs a passive
/// release/update check as part of its normal output. Do not run it during launcher discovery:
/// Native can still start as configured, while profile overrides stay unavailable until a
/// future non-networking identity surface is qualified.
pub fn probe_provider_version(agent: AgentId, path: &Path) -> Option<String> {
    if agent == AgentId::Hermes {
        None
    } else {
        probe_version(path)
    }
}
