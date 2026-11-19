//! The closed list of agents the Console can start. The UI names an
//! `agent_id`, never an executable.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::OsString,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, OnceLock, mpsc},
    time::{Duration, SystemTime},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentId {
    ClaudeCode,
    Codex,
    Opencode,
    Hermes,
}

impl AgentId {
    /// Provider IDs added by the extended registry cannot be serialized into the shared registry or
    /// native protocol: those binaries deserialize that enum as a closed set.
    pub fn fits_shared_registry(self) -> bool {
        matches!(self, Self::ClaudeCode | Self::Codex)
    }
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
    discover_name(spec.executable, path_var, home)
}

/// The same search by a bare executable name, for the tools the owner adds as distributors.
pub fn discover_name(name: &str, path_var: Option<OsString>, home: &Path) -> Option<PathBuf> {
    let mut folders: Vec<PathBuf> = path_var
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    folders.extend(extra_folders(home));
    folders
        .into_iter()
        .filter(|folder| folder.is_absolute())
        .map(|folder| folder.join(name))
        .find(|candidate| executable(candidate))
        .and_then(|candidate| candidate.canonicalize().ok())
}

#[derive(Clone)]
struct CachedProbe {
    modified: SystemTime,
    size: u64,
    version: Arc<OnceLock<Option<String>>>,
    help: Arc<OnceLock<super::capabilities::CliHelp>>,
}

static PROBES: std::sync::LazyLock<Mutex<HashMap<PathBuf, CachedProbe>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Cache display versions by executable identity. UI callers use a worker even for hits:
/// resolving identity can touch disk, and another caller may be probing the same executable.
fn cached_probe(path: &Path) -> Option<(PathBuf, CachedProbe)> {
    let path = path.canonicalize().ok()?;
    let metadata = path.metadata().ok()?;
    let modified = metadata.modified().ok()?;
    let size = metadata.len();
    let probe = {
        let mut versions = PROBES.lock().unwrap();
        let entry = versions.entry(path.clone());
        let entry = entry.or_insert_with(|| CachedProbe {
            modified,
            size,
            version: Arc::default(),
            help: Arc::default(),
        });
        if entry.modified != modified || entry.size != size {
            *entry = CachedProbe {
                modified,
                size,
                version: Arc::default(),
                help: Arc::default(),
            };
        }
        entry.clone()
    };
    Some((path, probe))
}

fn cached_version(path: &Path) -> Option<String> {
    let (path, probe) = cached_probe(path)?;
    probe.version.get_or_init(|| run_version(&path)).clone()
}

/// These interactive launches use the top-level command, with no subcommand-specific flags.
pub fn cached_cli_help(path: &Path) -> super::capabilities::CliHelp {
    let Some((path, probe)) = cached_probe(path) else {
        return Default::default();
    };
    probe
        .help
        .get_or_init(|| {
            let help = run_probe(&path, "--help", 128 * 1024, true);
            super::capabilities::CliHelp::parse(
                help.as_deref()
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .unwrap_or(""),
            )
        })
        .clone()
}

/// `--version` in a process of its own, bounded in time and size. The first line, cleaned.
fn run_version(path: &Path) -> Option<String> {
    let bytes = run_probe(path, "--version", 512, false)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
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

fn run_probe(path: &Path, arg: &str, limit: u64, require_success: bool) -> Option<Vec<u8>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    let mut child = Command::new(path)
        .arg(arg)
        .env("LC_ALL", "C")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(limit + 1).read_to_end(&mut bytes);
        let _ = send.send(
            result
                .ok()
                .and_then(|n| (n as u64 <= limit).then_some(bytes)),
        );
    });
    let output = receive
        .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        .ok()
        .flatten();
    if require_success && output.is_some() {
        while std::time::Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(status)) => return if status.success() { output } else { None },
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => break,
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    if require_success { None } else { output }
}

/// Display only. Hermes `--version` performs an update check, so leave it unobserved.
pub fn cached_provider_version(agent: AgentId, path: &Path) -> Option<String> {
    if agent == AgentId::Hermes {
        None
    } else {
        cached_version(path)
    }
}

/// For previews and launches, read the current version even if the file identity has not changed.
pub(super) fn probe_provider_version(agent: AgentId, path: &Path) -> Option<String> {
    if agent == AgentId::Hermes {
        None
    } else {
        run_version(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_probes_share_identity_and_refresh_after_update() {
        let root = std::env::temp_dir().join(format!("help-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let executable = root.join("provider");
        let alias = root.join("alias");
        let counter = root.join("count");
        let script = |flags| {
            format!(
                "#!/bin/sh\nprintf x >> '{}/count'\nprintf '%s\\n' '{flags}'\n",
                root.display()
            )
        };
        std::fs::write(&executable, script("  --sandbox <MODE>")).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&executable, &alias).unwrap();
        assert!(cached_cli_help(&executable).has("--sandbox"));
        assert!(cached_cli_help(&alias).has("--sandbox"));
        assert_eq!(std::fs::read(&counter).unwrap(), b"x");
        std::fs::write(&executable, script("  --ask-for-approval <MODE>")).unwrap();
        assert!(cached_cli_help(&alias).has("--ask-for-approval"));
        assert_eq!(std::fs::read(&counter).unwrap(), b"xx");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_version_queries_share_the_probe_and_invalidate_on_change() {
        let root = std::env::temp_dir().join(format!("version-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let executable = root.join("provider");
        let alias = root.join("alias");
        let counter = root.join("count");
        let script = |version| {
            format!(
                "#!/bin/sh\nprintf x >> '{}/count'\nprintf '%s\\n' '{version}'\n",
                root.display()
            )
        };
        std::fs::write(&executable, script("v1")).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&executable, &alias).unwrap();

        assert_eq!(
            cached_provider_version(AgentId::Opencode, &alias).as_deref(),
            Some("v1")
        );
        assert_eq!(cached_version(&executable).as_deref(), Some("v1"));
        assert_eq!(
            std::fs::read(&counter).unwrap(),
            b"x",
            "same executable was probed twice"
        );
        std::fs::write(&executable, script("v200")).unwrap();
        assert_eq!(cached_version(&alias).as_deref(), Some("v200"));
        assert_eq!(std::fs::read(&counter).unwrap(), b"xx");
        std::fs::remove_dir_all(root).unwrap();
    }
}
