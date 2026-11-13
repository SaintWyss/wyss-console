//! The roles launches are grouped by in Workspace (Orchestrator, Worker, Researcher, Reviewer and
//! the owner's own) and which role each launch has. Kept in `roles.json` beside `environment.json`:
//! a launch file rejects unknown fields (an older Console would list it as refused), and the
//! library's schema stays the one older builds open. Local to this computer. A damaged file loads
//! as the basic roles with no assignment, and loading never writes.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::native::store;

const FILE: &str = "roles.json";
const VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 256 * 1024;
pub const MAX_ROLES: usize = 64;
pub const MAX_ASSIGNED: usize = 4096;
const MAX_NAME: usize = 40;

/// The basic roles: always there, never deleted. A launch with no role, or with a role that no
/// longer exists, is a Worker.
pub const BASIC: [&str; 4] = ["orchestrator", "worker", "researcher", "reviewer"];
pub const DEFAULT_ROLE: &str = "worker";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Custom {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    custom: Vec<Custom>,
    /// Launch id → role id.
    assigned: BTreeMap<String, String>,
}

/// What the window reads: the owner's roles and each launch's role (unknown ones read as Worker).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Roles {
    pub custom: Vec<Custom>,
    pub assigned: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Create {
        name: String,
    },
    /// A basic role cannot be deleted. Its launches go back to Worker.
    Delete {
        id: String,
    },
    Assign {
        launch: String,
        role: String,
    },
    /// A launch saved under a new name keeps its role.
    Rename {
        from: String,
        to: String,
    },
    Forget {
        launch: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Invalid(&'static str),
    Io,
}

fn launch_ok(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn custom_id_ok(id: &str) -> bool {
    id.strip_prefix("u-").is_some_and(|rest| {
        (1..=36).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
    })
}
fn name_ok(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.chars().count() <= MAX_NAME && !n.chars().any(char::is_control)
}

fn known(state: &State, role: &str) -> bool {
    BASIC.contains(&role) || state.custom.iter().any(|c| c.id == role)
}

fn load(directory: &Path) -> State {
    let read = || -> Option<State> {
        let meta = fs::symlink_metadata(directory.join(FILE)).ok()?;
        if !meta.is_file() {
            return None;
        }
        let bytes = store::read_bounded(&directory.join(FILE), MAX_FILE_BYTES)?;
        let state: State = serde_json::from_slice(&bytes).ok()?;
        let valid = state.schema_version == VERSION
            && state.custom.len() <= MAX_ROLES
            && state.assigned.len() <= MAX_ASSIGNED
            && state
                .custom
                .iter()
                .all(|c| custom_id_ok(&c.id) && name_ok(&c.name))
            && state.assigned.iter().all(|(launch, role)| {
                launch_ok(launch) && (BASIC.contains(&role.as_str()) || custom_id_ok(role))
            });
        valid.then_some(state)
    };
    read().unwrap_or(State {
        schema_version: VERSION,
        ..State::default()
    })
}

pub fn read(directory: &Path) -> Roles {
    let state = load(directory);
    let assigned = state
        .assigned
        .iter()
        .map(|(launch, role)| {
            let role = if known(&state, role) {
                role.clone()
            } else {
                DEFAULT_ROLE.to_owned()
            };
            (launch.clone(), role)
        })
        .collect();
    Roles {
        custom: state.custom,
        assigned,
    }
}

pub fn write(directory: &Path, change: Change) -> Result<Roles, Refusal> {
    let mut state = load(directory);
    match change {
        Change::Create { name } => {
            let name = name.trim().to_owned();
            if !name_ok(&name) {
                return Err(Refusal::Invalid("name"));
            }
            if state
                .custom
                .iter()
                .any(|c| c.name.to_lowercase() == name.to_lowercase())
            {
                return Err(Refusal::Invalid("exists"));
            }
            if state.custom.len() >= MAX_ROLES {
                return Err(Refusal::Invalid("too_many"));
            }
            state.custom.push(Custom {
                id: format!("u-{}", uuid::Uuid::new_v4()),
                name,
            });
        }
        Change::Delete { id } => {
            if BASIC.contains(&id.as_str()) {
                return Err(Refusal::Invalid("basic"));
            }
            if !state.custom.iter().any(|c| c.id == id) {
                return Err(Refusal::Invalid("unknown"));
            }
            state.custom.retain(|c| c.id != id);
            // Its launches go back to Worker: no entry is the default.
            state.assigned.retain(|_, role| *role != id);
        }
        Change::Assign { launch, role } => {
            if !launch_ok(&launch) || !known(&state, &role) {
                return Err(Refusal::Invalid("assign"));
            }
            if role == DEFAULT_ROLE {
                state.assigned.remove(&launch);
            } else {
                if state.assigned.len() >= MAX_ASSIGNED && !state.assigned.contains_key(&launch) {
                    return Err(Refusal::Invalid("too_many"));
                }
                state.assigned.insert(launch, role);
            }
        }
        Change::Rename { from, to } => {
            if !launch_ok(&from) || !launch_ok(&to) {
                return Err(Refusal::Invalid("rename"));
            }
            if let Some(role) = state.assigned.remove(&from) {
                state.assigned.insert(to, role);
            }
        }
        Change::Forget { launch } => {
            if !launch_ok(&launch) {
                return Err(Refusal::Invalid("forget"));
            }
            state.assigned.remove(&launch);
        }
    }
    state.schema_version = VERSION;
    let bytes = serde_json::to_vec(&state).map_err(|_| Refusal::Io)?;
    store::private_dir(directory).map_err(|_| Refusal::Io)?;
    store::write_atomic(&directory.join(FILE), &bytes).map_err(|_: io::Error| Refusal::Io)?;
    Ok(read(directory))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arsenal-roles-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_role_of_the_owner_is_made_assigned_and_deleted_and_its_launches_go_back_to_worker() {
        let dir = scratch("cycle");
        let roles = write(
            &dir,
            Change::Create {
                name: " Diseñador ".into(),
            },
        )
        .unwrap();
        let id = roles.custom[0].id.clone();
        assert_eq!(roles.custom[0].name, "Diseñador");
        write(
            &dir,
            Change::Assign {
                launch: "ui".into(),
                role: id.clone(),
            },
        )
        .unwrap();
        write(
            &dir,
            Change::Assign {
                launch: "rev".into(),
                role: "reviewer".into(),
            },
        )
        .unwrap();
        assert_eq!(read(&dir).assigned.get("ui"), Some(&id));
        let after = write(&dir, Change::Delete { id: id.clone() }).unwrap();
        assert!(after.custom.is_empty());
        // No entry: the launch reads as a Worker. Another launch keeps its role.
        assert_eq!(after.assigned.get("ui"), None);
        assert_eq!(
            after.assigned.get("rev").map(String::as_str),
            Some("reviewer")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn basic_roles_cannot_be_deleted_and_names_are_checked() {
        let dir = scratch("rules");
        for basic in BASIC {
            assert_eq!(
                write(&dir, Change::Delete { id: basic.into() }),
                Err(Refusal::Invalid("basic"))
            );
        }
        assert!(write(&dir, Change::Create { name: "".into() }).is_err());
        assert!(
            write(
                &dir,
                Change::Create {
                    name: "a\u{7}".into()
                }
            )
            .is_err()
        );
        assert!(
            write(
                &dir,
                Change::Create {
                    name: "x".repeat(41)
                }
            )
            .is_err()
        );
        write(&dir, Change::Create { name: "QA".into() }).unwrap();
        assert_eq!(
            write(&dir, Change::Create { name: "qa".into() }),
            Err(Refusal::Invalid("exists"))
        );
        assert!(
            write(
                &dir,
                Change::Assign {
                    launch: "../x".into(),
                    role: "worker".into()
                }
            )
            .is_err()
        );
        assert!(
            write(
                &dir,
                Change::Assign {
                    launch: "ok".into(),
                    role: "u-nope".into()
                }
            )
            .is_err()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_renamed_launch_keeps_its_role_and_a_forgotten_one_loses_it() {
        let dir = scratch("rename");
        write(
            &dir,
            Change::Assign {
                launch: "old".into(),
                role: "researcher".into(),
            },
        )
        .unwrap();
        let roles = write(
            &dir,
            Change::Rename {
                from: "old".into(),
                to: "new".into(),
            },
        )
        .unwrap();
        assert_eq!(roles.assigned.get("old"), None);
        assert_eq!(
            roles.assigned.get("new").map(String::as_str),
            Some("researcher")
        );
        let roles = write(
            &dir,
            Change::Forget {
                launch: "new".into(),
            },
        )
        .unwrap();
        assert!(roles.assigned.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_damaged_file_loads_as_the_basic_roles_and_is_written_private() {
        let dir = scratch("damaged");
        fs::write(dir.join(FILE), "{not json").unwrap();
        assert_eq!(
            read(&dir),
            Roles {
                custom: vec![],
                assigned: BTreeMap::new()
            }
        );
        fs::write(
            dir.join(FILE),
            r#"{"schema_version":1,"custom":[],"assigned":{"x":"u-0000abcd"}}"#,
        )
        .unwrap();
        // A role id of the right shape that no longer exists reads as Worker.
        assert_eq!(
            read(&dir).assigned.get("x").map(String::as_str),
            Some("worker")
        );
        write(&dir, Change::Forget { launch: "x".into() }).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.join(FILE)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(dir).unwrap();
    }
}
