//! What survives what: a truthful account of when a terminal held by the host keeps running
//! and when it does not, from facts read on this machine, and the one reversible setting that
//! changes the answer for logout: systemd lingering.
//!
//! Lingering is the owner's to turn on. Nothing here turns it on by itself; the one function that
//! does is called only by an explicit request with a confirmation word, and `disable` undoes it.
//! No unit file is written, no service restarted. Power-off and reboot end every process and the
//! report says so; nothing claims a command comes back by itself.

use serde_json::{Value, json};
use std::path::PathBuf;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
    fn the_system_sources_exist_on_a_linux_machine_and_are_read_only_paths() {
        let s = Sources::system().expect("a user");
        assert!(s.cgroup.starts_with("/proc") && s.linger_dir.starts_with("/var/lib/systemd"));
        let _ = read_facts(&s);
    }
}
