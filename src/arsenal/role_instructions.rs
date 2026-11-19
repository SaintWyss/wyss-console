//! Strict per-preset project instruction choices, separate from environment files.
use std::{collections::BTreeMap, fs, io, path::Path};

use serde::{Deserialize, Serialize};

use crate::native::store;

const FILE: &str = "role-instructions.json";
const MAX_FILE: u64 = 1024 * 1024;
const MAX_ROLE: usize = 64 * 1024;
pub const UNREADABLE: &str = "role_unreadable";
pub const TOO_LARGE: &str = "role_too_large";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "project", rename_all = "snake_case", deny_unknown_fields)]
pub enum Choice {
    Load,
    Exclude { role: String },
    Role { role: String },
}

impl Choice {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Load => true,
            Self::Exclude { role } => role.len() <= MAX_ROLE && !role.contains('\0'),
            Self::Role { role } => {
                !role.is_empty() && role.len() <= MAX_ROLE && !role.contains('\0')
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    presets: BTreeMap<String, Choice>,
}

fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn read(directory: &Path) -> io::Result<State> {
    let path = directory.join(FILE);
    let bad = || io::Error::other(UNREADABLE);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(State {
                schema_version: 1,
                presets: BTreeMap::new(),
            });
        }
        Ok(meta) if meta.is_file() => {}
        _ => return Err(bad()),
    }
    let bytes = store::read_bounded(&path, MAX_FILE).ok_or_else(bad)?;
    let state: State = serde_json::from_slice(&bytes).map_err(|_| bad())?;
    if state.schema_version != 1
        || state.presets.len() > super::environment::MAX_PRESETS
        || state
            .presets
            .iter()
            .any(|(id, choice)| !id_ok(id) || !choice.valid() || matches!(choice, Choice::Load))
    {
        return Err(bad());
    }
    Ok(state)
}

pub fn get(directory: &Path, selection: &str) -> io::Result<Choice> {
    Ok(read(directory)?
        .presets
        .remove(selection)
        .unwrap_or(Choice::Load))
}

pub fn set(directory: &Path, selection: &str, choice: &Choice) -> io::Result<()> {
    if !id_ok(selection) || !choice.valid() {
        return Err(io::Error::other("invalid"));
    }
    let mut state = read(directory)?;
    if matches!(choice, Choice::Load) {
        state.presets.remove(selection);
    } else {
        if state.presets.len() >= super::environment::MAX_PRESETS
            && !state.presets.contains_key(selection)
        {
            return Err(io::Error::other("invalid"));
        }
        state.presets.insert(selection.to_owned(), choice.clone());
    }
    let bytes = serde_json::to_vec(&state).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(io::Error::other(TOO_LARGE));
    }
    store::write_atomic(&directory.join(FILE), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn saves_role_and_removes_default() {
        let dir = std::env::temp_dir().join(format!("role-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let role = Choice::Role {
            role: "texto".into(),
        };
        set(&dir, "one", &role).unwrap();
        assert_eq!(get(&dir, "one").unwrap(), role);
        set(&dir, "one", &Choice::Load).unwrap();
        assert_eq!(get(&dir, "one").unwrap(), Choice::Load);
        assert_eq!(
            fs::read_to_string(dir.join(FILE)).unwrap(),
            r#"{"schema_version":1,"presets":{}}"#
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn strict_store_never_overwrites_invalid_data() {
        let dir = std::env::temp_dir().join(format!("role-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join(FILE);
        assert_eq!(get(&dir, "one").unwrap(), Choice::Load);
        assert!(
            set(
                &dir,
                "one",
                &Choice::Role {
                    role: String::new()
                }
            )
            .is_err()
        );
        for bytes in [
            br#"{"schema_version":2,"presets":{}}"#.to_vec(),
            br#"{"schema_version":1,"presets":{},"extra":true}"#.to_vec(),
            br#"{"schema_version":1,"presets":{"one":{"project":"role","role":""}}}"#.to_vec(),
            vec![b'x'; MAX_FILE as usize + 1],
        ] {
            fs::write(&path, &bytes).unwrap();
            assert_eq!(get(&dir, "one").unwrap_err().to_string(), UNREADABLE);
            assert_eq!(
                set(&dir, "one", &Choice::Load).unwrap_err().to_string(),
                UNREADABLE
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_file(&path).unwrap();
        let target = dir.join("target");
        fs::write(&target, b"unchanged").unwrap();
        symlink(&target, &path).unwrap();
        assert_eq!(
            set(&dir, "one", &Choice::Load).unwrap_err().to_string(),
            UNREADABLE
        );
        assert_eq!(fs::read(&target).unwrap(), b"unchanged");
        fs::remove_dir_all(dir).unwrap();
    }
}
