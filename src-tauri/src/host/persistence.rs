//! What survives what (M4): a truthful account of when a terminal held by the host keeps running
//! and when it does not, from facts read on this machine, and the one reversible setting that
//! changes the answer for logout: systemd lingering.
//!
//! Lingering is the owner's to turn on. Nothing here turns it on by itself; the one function that
//! does is called only by an explicit request with a confirmation word, and `disable` undoes it.
//! No unit file is written, no service restarted. Power-off and reboot end every process and the
//! report says so; nothing claims a command comes back by itself.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Under the user's systemd manager (a unit, or a desktop-launched app scope).
    UserManager,
    /// In a login session's scope.
    SessionScope,
    Other,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// `None`: this system keeps no record that can be read (not systemd, or no permission).
    pub linger: Option<bool>,
    /// logind's `KillUserProcesses` as configured (its default is no).
    pub kill_user_processes: bool,
    pub placement: Placement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Survives,
    Ends,
    /// Depends on something this machine's facts do not settle.
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub id: &'static str,
    pub outcome: Outcome,
    /// Which of the two logout rows describes the setting this machine has now.
    pub current: bool,
    pub basis: &'static str,
    /// How this is known: a test of this code, a reading of the code, or the facts above.
    pub known_by: &'static str,
}

/// Where the facts are read from (a test points them at a directory of its own).
#[derive(Clone, Debug)]
pub struct Sources {
    pub cgroup: PathBuf,
    pub linger_dir: PathBuf,
    pub logind_confs: Vec<PathBuf>,
    pub uid: u32,
    pub user: String,
}

impl Sources {
    pub fn system() -> Option<Sources> {
        let uid = nix::unistd::getuid();
        let user = nix::unistd::User::from_uid(uid).ok().flatten()?.name;
        Some(Sources {
            cgroup: PathBuf::from("/proc/self/cgroup"),
            linger_dir: PathBuf::from("/var/lib/systemd/linger"),
            logind_confs: vec![
                PathBuf::from("/usr/lib/systemd/logind.conf"),
                PathBuf::from("/etc/systemd/logind.conf"),
            ],
            uid: uid.as_raw(),
            user,
        })
    }
}

fn placement(source: &Sources) -> Placement {
    let Ok(text) = std::fs::read_to_string(&source.cgroup) else {
        return Placement::Unknown;
    };
    let line = text.lines().find(|l| l.starts_with("0::")).unwrap_or("");
    if line.contains(&format!("/user@{}.service/", source.uid)) {
        Placement::UserManager
    } else if line.contains("/session-") {
        Placement::SessionScope
    } else {
        Placement::Other
    }
}

/// The last `KillUserProcesses=` of the files in order, and of each `.d` directory beside them.
fn kill_user_processes(source: &Sources) -> bool {
    let mut files: Vec<PathBuf> = Vec::new();
    for conf in &source.logind_confs {
        files.push(conf.clone());
        let dir = PathBuf::from(format!("{}.d", conf.display()));
        if let Ok(read) = std::fs::read_dir(&dir) {
            let mut extra: Vec<PathBuf> = read
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "conf"))
                .collect();
            extra.sort();
            files.extend(extra);
        }
    }
    let mut value = false;
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("KillUserProcesses=") {
                value = matches!(v.trim(), "yes" | "true" | "1" | "on");
            }
        }
    }
    value
}

pub fn read_facts(source: &Sources) -> Facts {
    let linger = if source.linger_dir.is_dir() {
        Some(source.linger_dir.join(&source.user).exists())
    } else {
        None
    };
    Facts {
        linger,
        kill_user_processes: kill_user_processes(source),
        placement: placement(source),
    }
}

/// The account, one row per thing that can happen to the machine or the host.
pub fn report(facts: &Facts) -> Vec<Event> {
    let logout = |lingering: bool| -> Outcome {
        match (facts.placement, lingering, facts.kill_user_processes) {
            // The user's manager is kept (or not) by lingering, and the host lives inside it.
            (Placement::UserManager, true, _) => Outcome::Survives,
            (Placement::UserManager, false, _) => Outcome::Ends,
            // A session scope is ended by logind only when it is told to kill user processes.
            (Placement::SessionScope, _, true) => Outcome::Ends,
            (Placement::SessionScope, _, false) => Outcome::Uncertain,
            _ => Outcome::Uncertain,
        }
    };
    let now = facts.linger == Some(true);
    vec![
        Event {
            id: "window_close",
            outcome: Outcome::Survives,
            current: true,
            basis: "The host is a separate process with a group of its own; closing the window signals nothing in it.",
            known_by: "tested: a terminal outlives the client that made it",
        },
        Event {
            id: "window_detach",
            outcome: Outcome::Survives,
            current: true,
            basis: "A window that leaves detaches its terminals; the host closes none of them.",
            known_by: "tested: a late client sees what was printed",
        },
        Event {
            id: "screen_lock",
            outcome: Outcome::Survives,
            current: true,
            basis: "Locking the screen keeps the session and changes no process.",
            known_by: "reasoned from what locking does; not exercised here",
        },
        Event {
            id: "logout_with_lingering",
            outcome: logout(true),
            current: now,
            basis: "With lingering the user's manager keeps running after the last session ends.",
            known_by: "this machine's facts (where the host runs, logind's setting)",
        },
        Event {
            id: "logout_without_lingering",
            outcome: logout(false),
            current: !now,
            basis: "Without lingering the user's manager stops with the last session, and what runs inside it stops too.",
            known_by: "this machine's facts (where the host runs, logind's setting)",
        },
        Event {
            id: "suspend_resume",
            outcome: Outcome::Survives,
            current: true,
            basis: "Suspend freezes processes and resume continues them; programs that watch a clock or a network see the gap.",
            known_by: "reasoned from what suspend does; not exercised here",
        },
        Event {
            id: "host_crash",
            outcome: Outcome::Ends,
            current: true,
            basis: "The host owns every terminal's PTY: when it stops, they close. Nothing relaunches a command.",
            known_by: "reading of the code: the host is the PTY owner",
        },
        Event {
            id: "reboot_or_power_off",
            outcome: Outcome::Ends,
            current: true,
            basis: "No process survives a reboot or a loss of power, and nothing is started again by itself afterwards.",
            known_by: "what a reboot is",
        },
    ]
}

fn word(o: Outcome) -> &'static str {
    match o {
        Outcome::Survives => "survives",
        Outcome::Ends => "ends",
        Outcome::Uncertain => "uncertain",
    }
}

pub fn to_json(facts: &Facts) -> Value {
    json!({
        "lingering": facts.linger,
        "kill_user_processes": facts.kill_user_processes,
        "placement": match facts.placement {
            Placement::UserManager => "user_manager",
            Placement::SessionScope => "session_scope",
            Placement::Other => "other",
            Placement::Unknown => "unknown",
        },
        "events": report(facts).into_iter().map(|e| json!({
            "id": e.id,
            "outcome": word(e.outcome),
            "current": e.current,
            "basis": e.basis,
            "known_by": e.known_by,
        })).collect::<Vec<_>>(),
    })
}

/// Runs a program by absolute path with the arguments given, no shell, a fixed environment.
pub trait Runner: Send + Sync {
    fn run(&self, program: &Path, args: &[&str]) -> std::io::Result<i32>;
}

pub struct SystemRunner;
impl Runner for SystemRunner {
    fn run(&self, program: &Path, args: &[&str]) -> std::io::Result<i32> {
        let status = std::process::Command::new(program)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .current_dir("/")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        Ok(status.code().unwrap_or(-1))
    }
}

pub const ENABLE_WORD: &str = "enable-lingering";
pub const DISABLE_WORD: &str = "disable-lingering";

#[derive(Debug, PartialEq, Eq)]
pub enum LingerError {
    /// The confirmation word was not the one for this action.
    NotConfirmed,
    Unsupported,
    Failed,
    /// The command succeeded but the setting is not what was asked for.
    NotApplied,
}

impl LingerError {
    pub fn code(&self) -> &'static str {
        match self {
            LingerError::NotConfirmed => "not_confirmed",
            LingerError::Unsupported => "unsupported",
            LingerError::Failed => "command_failed",
            LingerError::NotApplied => "not_applied",
        }
    }
}

/// Turns lingering on or off for this user, only when `confirm` is the word for it, and says what
/// the setting is afterwards (read back, not assumed).
pub fn set_lingering(
    runner: &dyn Runner,
    source: &Sources,
    enable: bool,
    confirm: &str,
) -> Result<Facts, LingerError> {
    if confirm != if enable { ENABLE_WORD } else { DISABLE_WORD } {
        return Err(LingerError::NotConfirmed);
    }
    if read_facts(source).linger.is_none() {
        return Err(LingerError::Unsupported);
    }
    let program = Path::new("/usr/bin/loginctl");
    let verb = if enable {
        "enable-linger"
    } else {
        "disable-linger"
    };
    match runner.run(program, &[verb]) {
        Ok(0) => {}
        _ => return Err(LingerError::Failed),
    }
    let after = read_facts(source);
    if after.linger == Some(enable) {
        Ok(after)
    } else {
        Err(LingerError::NotApplied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn root() -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("wyss-persist-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    fn source(dir: &Path, cgroup: &str, kill: Option<&str>, linger: Option<bool>) -> Sources {
        std::fs::write(dir.join("cgroup"), cgroup).unwrap();
        let conf = dir.join("logind.conf");
        if let Some(k) = kill {
            std::fs::write(&conf, format!("[Login]\nKillUserProcesses={k}\n")).unwrap();
        }
        let linger_dir = dir.join("linger");
        if let Some(on) = linger {
            std::fs::create_dir_all(&linger_dir).unwrap();
            if on {
                std::fs::write(linger_dir.join("tester"), "").unwrap();
            }
        }
        Sources {
            cgroup: dir.join("cgroup"),
            linger_dir,
            logind_confs: vec![conf],
            uid: 1000,
            user: "tester".into(),
        }
    }
    const USER_MANAGER: &str =
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-x.scope\n";
    const SESSION: &str = "0::/user.slice/user-1000.slice/session-3.scope\n";

    fn outcome(events: &[Event], id: &str) -> Outcome {
        events.iter().find(|e| e.id == id).unwrap().outcome
    }

    #[test]
    fn facts_are_read_from_where_the_host_runs_and_how_logind_is_set() {
        let dir = root();
        let s = source(&dir, USER_MANAGER, Some("no"), Some(false));
        assert_eq!(
            read_facts(&s),
            Facts {
                linger: Some(false),
                kill_user_processes: false,
                placement: Placement::UserManager
            }
        );
        let s = source(&dir, SESSION, Some("yes"), Some(true));
        assert_eq!(
            read_facts(&s),
            Facts {
                linger: Some(true),
                kill_user_processes: true,
                placement: Placement::SessionScope
            }
        );
        // No lingering directory is not a "no": the system does not say.
        let other = root();
        let s = source(&other, "0::/system.slice/x.service\n", None, None);
        assert_eq!(
            read_facts(&s),
            Facts {
                linger: None,
                kill_user_processes: false,
                placement: Placement::Other
            }
        );
    }

    #[test]
    fn the_account_never_promises_what_the_facts_do_not_support() {
        let dir = root();
        let manager = report(&read_facts(&source(&dir, USER_MANAGER, None, Some(false))));
        assert_eq!(
            outcome(&manager, "logout_with_lingering"),
            Outcome::Survives
        );
        assert_eq!(outcome(&manager, "logout_without_lingering"), Outcome::Ends);
        assert!(
            manager
                .iter()
                .find(|e| e.id == "logout_without_lingering")
                .unwrap()
                .current
        );
        assert!(
            !manager
                .iter()
                .find(|e| e.id == "logout_with_lingering")
                .unwrap()
                .current
        );
        let scope = report(&read_facts(&source(
            &root(),
            SESSION,
            Some("no"),
            Some(false),
        )));
        assert_eq!(outcome(&scope, "logout_with_lingering"), Outcome::Uncertain);
        let killed = report(&read_facts(&source(
            &root(),
            SESSION,
            Some("yes"),
            Some(true),
        )));
        assert_eq!(outcome(&killed, "logout_with_lingering"), Outcome::Ends);
        // Whatever the facts, a reboot, a power loss and a host crash end terminals.
        for events in [&manager, &scope, &killed] {
            assert_eq!(outcome(events, "reboot_or_power_off"), Outcome::Ends);
            assert_eq!(outcome(events, "host_crash"), Outcome::Ends);
            assert_eq!(outcome(events, "window_close"), Outcome::Survives);
        }
        let text = to_json(&read_facts(&source(&root(), USER_MANAGER, None, None))).to_string();
        assert!(!text.contains("restart") && !text.contains("resume the command"));
    }

    struct Recorder {
        ran: Mutex<Vec<String>>,
        /// What the fake system does when told to enable or disable.
        effect: Box<dyn Fn(&str) + Send + Sync>,
        exit: i32,
    }
    impl Runner for Recorder {
        fn run(&self, program: &Path, args: &[&str]) -> std::io::Result<i32> {
            self.ran
                .lock()
                .unwrap()
                .push(format!("{} {}", program.display(), args.join(" ")));
            (self.effect)(args[0]);
            Ok(self.exit)
        }
    }

    #[test]
    fn lingering_changes_only_on_the_exact_word_and_is_read_back() {
        let dir = root();
        let s = source(&dir, USER_MANAGER, None, Some(false));
        let marker = s.linger_dir.join("tester");
        let (m1, m2) = (marker.clone(), marker.clone());
        let runner = Recorder {
            ran: Mutex::new(vec![]),
            effect: Box::new(move |verb| {
                if verb == "enable-linger" {
                    std::fs::write(&m1, "").unwrap();
                } else {
                    let _ = std::fs::remove_file(&m2);
                }
            }),
            exit: 0,
        };
        // Nothing runs without the word, or with the word for the other direction.
        for wrong in ["", "yes", DISABLE_WORD, "ENABLE-LINGERING"] {
            assert_eq!(
                set_lingering(&runner, &s, true, wrong).unwrap_err(),
                LingerError::NotConfirmed
            );
        }
        assert!(runner.ran.lock().unwrap().is_empty());
        let on = set_lingering(&runner, &s, true, ENABLE_WORD).unwrap();
        assert_eq!(on.linger, Some(true));
        assert_eq!(
            runner.ran.lock().unwrap().as_slice(),
            ["/usr/bin/loginctl enable-linger"]
        );
        // And it is reversible, by the same kind of explicit request.
        let off = set_lingering(&runner, &s, false, DISABLE_WORD).unwrap();
        assert_eq!(off.linger, Some(false));
        assert_eq!(runner.ran.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_command_that_fails_or_does_nothing_is_reported_as_what_it_was() {
        let dir = root();
        let s = source(&dir, USER_MANAGER, None, Some(false));
        let failing = Recorder {
            ran: Mutex::new(vec![]),
            effect: Box::new(|_| {}),
            exit: 1,
        };
        assert_eq!(
            set_lingering(&failing, &s, true, ENABLE_WORD).unwrap_err(),
            LingerError::Failed
        );
        let silent = Recorder {
            ran: Mutex::new(vec![]),
            effect: Box::new(|_| {}),
            exit: 0,
        };
        assert_eq!(
            set_lingering(&silent, &s, true, ENABLE_WORD).unwrap_err(),
            LingerError::NotApplied
        );
        let none = source(&root(), USER_MANAGER, None, None);
        assert_eq!(
            set_lingering(&silent, &none, true, ENABLE_WORD).unwrap_err(),
            LingerError::Unsupported
        );
        assert!(silent.ran.lock().unwrap().is_empty() || silent.ran.lock().unwrap().len() == 1);
    }

    #[test]
    fn the_system_sources_exist_on_a_linux_machine_and_are_read_only_paths() {
        let s = Sources::system().expect("a user");
        assert!(s.cgroup.starts_with("/proc") && s.linger_dir.starts_with("/var/lib/systemd"));
        let _ = read_facts(&s);
    }
}
