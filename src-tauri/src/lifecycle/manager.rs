//! The user's service manager, asked four fixed things about one fixed unit.
//!
//! There is no shell and no string built from input: one executable found at a fixed
//! place, fixed arguments, an environment of two variables. There is no stop and no
//! restart here at all: this product never asks for either.
use super::UNIT_NAME;
use std::{
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Loaded,
    NotFound,
    /// Masked, in error, or a word this build does not know.
    Other,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveState {
    Inactive,
    /// Starting, or waiting to be restarted by its own policy.
    Activating,
    Active,
    Deactivating,
    Failed,
    Other,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitResult {
    Success,
    /// It was restarted as often as its limit allows, and is not started again by itself.
    StartLimitHit,
    Other,
}
/// What the service manager says of the unit. The process it names is the manager's own
/// record, read at one moment: it is compared with who answers the socket, never used to
/// signal anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitStatus {
    pub load: LoadState,
    pub active: ActiveState,
    pub result: UnitResult,
    pub main_pid: Option<u32>,
    /// The file the manager loaded the unit from.
    pub fragment: Option<PathBuf>,
}
impl UnitStatus {
    /// The manager runs it, or is about to by its own policy: it is not started again.
    pub fn in_hand(&self) -> bool {
        matches!(
            self.active,
            ActiveState::Active | ActiveState::Activating | ActiveState::Deactivating
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerError {
    /// No service manager answers for this user.
    Unavailable,
    /// It answered, and did not do what was asked.
    Refused,
}

pub trait ServiceManager: Send + Sync + 'static {
    fn status(&self) -> Result<UnitStatus, ManagerError>;
    /// Has the manager read unit files again.
    fn reload(&self) -> Result<(), ManagerError>;
    fn start(&self) -> Result<(), ManagerError>;
    /// Forgets a failure so that a start is accepted again. Only a user's Retry asks.
    fn reset_failed(&self) -> Result<(), ManagerError>;
}

/// Where `systemctl` is on the systems this product supports. It is never looked up in
/// `PATH`.
const SYSTEMCTL: [&str; 2] = ["/usr/bin/systemctl", "/bin/systemctl"];
const QUERY_LIMIT: Duration = Duration::from_secs(5);
const ACTION_LIMIT: Duration = Duration::from_secs(15);
/// Output read from one call. `show` with five properties is a few hundred bytes.
const MAX_OUTPUT: u64 = 16 * 1024;

/// `systemd --user`, through its own `systemctl`.
pub struct Systemd {
    systemctl: Option<PathBuf>,
    runtime: PathBuf,
}
impl Systemd {
    /// The manager of the user session that owns `runtime` (`$XDG_RUNTIME_DIR`).
    pub fn new(runtime: &Path) -> Self {
        // A system file: root's, written by root alone, reached through root's links.
        let systemctl = SYSTEMCTL.iter().find_map(|candidate| {
            let path = fs::canonicalize(candidate).ok()?;
            let meta = fs::metadata(&path).ok()?;
            (meta.is_file() && meta.uid() == 0 && meta.mode() & 0o022 == 0).then_some(path)
        });
        Self {
            systemctl,
            runtime: runtime.to_owned(),
        }
    }
    /// Runs `systemctl --user` with these arguments and answers what it printed.
    fn call(&self, arguments: &[&str], limit: Duration) -> Result<String, ManagerError> {
        let systemctl = self.systemctl.as_ref().ok_or(ManagerError::Unavailable)?;
        let mut child = Command::new(systemctl)
            // Never a prompt: there is nobody to answer one, and nothing to ask.
            .args(["--user", "--no-ask-password"])
            .args(arguments)
            // Nothing of the desktop's environment: where the manager is, and plain output.
            .env_clear()
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ManagerError::Unavailable)?;
        let deadline = Instant::now() + limit;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                // It does not answer: that is a manager that is not there.
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ManagerError::Unavailable);
                }
            }
        };
        // What it printed is in the pipe already. It is read without waiting, so a
        // process that outlived it with the pipe open cannot hold this call.
        let mut output = String::new();
        if let Some(stdout) = child.stdout.take() {
            use nix::fcntl::{FcntlArg, OFlag, fcntl};
            use std::os::fd::AsRawFd;
            if fcntl(stdout.as_raw_fd(), FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).is_ok() {
                let mut bytes = Vec::new();
                let _ = stdout.take(MAX_OUTPUT).read_to_end(&mut bytes);
                output = String::from_utf8_lossy(&bytes).into_owned();
            }
        }
        match status.code() {
            Some(0) => Ok(output),
            // systemctl's own word for "could not reach the manager" is exit status 1
            // with nothing printed; a job that failed prints nothing either. They are
            // told apart by asking the manager something it always answers.
            _ => Err(ManagerError::Refused),
        }
    }
    /// Whether a manager answers at all.
    fn reachable(&self) -> bool {
        self.call(&["show", "--property=Version"], QUERY_LIMIT)
            .is_ok_and(|output| output.starts_with("Version="))
    }
    fn act(&self, arguments: &[&str]) -> Result<(), ManagerError> {
        match self.call(arguments, ACTION_LIMIT) {
            Ok(_) => Ok(()),
            Err(ManagerError::Refused) if !self.reachable() => Err(ManagerError::Unavailable),
            Err(error) => Err(error),
        }
    }
}

/// Reads `show`'s `Key=Value` lines. A key that is missing or a word that is not known
/// becomes `Other` or `None`: nothing is assumed from what was not said.
pub(super) fn parse_status(output: &str) -> UnitStatus {
    let value = |key: &str| {
        output
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
    };
    UnitStatus {
        load: match value("LoadState") {
            Some("loaded") => LoadState::Loaded,
            Some("not-found") => LoadState::NotFound,
            _ => LoadState::Other,
        },
        active: match value("ActiveState") {
            Some("inactive") => ActiveState::Inactive,
            Some("activating") => ActiveState::Activating,
            Some("active") => ActiveState::Active,
            Some("deactivating") => ActiveState::Deactivating,
            Some("failed") => ActiveState::Failed,
            _ => ActiveState::Other,
        },
        result: match value("Result") {
            Some("success") => UnitResult::Success,
            Some("start-limit-hit") => UnitResult::StartLimitHit,
            _ => UnitResult::Other,
        },
        main_pid: value("MainPID")
            .and_then(|pid| pid.parse().ok())
            .filter(|pid| *pid != 0),
        fragment: value("FragmentPath")
            .filter(|path| path.starts_with('/'))
            .map(PathBuf::from),
    }
}

impl ServiceManager for Systemd {
    fn status(&self) -> Result<UnitStatus, ManagerError> {
        let output = self
            .call(
                &[
                    "show",
                    UNIT_NAME,
                    "--property=LoadState,ActiveState,Result,MainPID,FragmentPath",
                ],
                QUERY_LIMIT,
            )
            .map_err(|_| ManagerError::Unavailable)?;
        // A manager that answers always says how the unit is loaded.
        if !output.contains("LoadState=") {
            return Err(ManagerError::Unavailable);
        }
        Ok(parse_status(&output))
    }
    fn reload(&self) -> Result<(), ManagerError> {
        self.act(&["daemon-reload"])
    }
    fn start(&self) -> Result<(), ManagerError> {
        self.act(&["start", UNIT_NAME])
    }
    fn reset_failed(&self) -> Result<(), ManagerError> {
        self.act(&["reset-failed", UNIT_NAME])
    }
}
