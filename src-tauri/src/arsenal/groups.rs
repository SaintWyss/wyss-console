//! The owner's group corrections and the marker that the preset migration ran, kept in
//! `groups.json` beside the library. A small private file rather than a table, so the database
//! schema stays the one older builds open. A damaged file loads as empty, and loading never writes.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

pub const MAX_GROUPS: usize = 4096;
pub const MAX_FILE_BYTES: u64 = 512 * 1024;
const FILE: &str = "groups.json";
const VERSION: u32 = 2;
/// Most categories the owner can make.
pub const MAX_CUSTOM: usize = 64;
/// The sections a category of the owner's is placed in.
pub const SECTIONS: [&str; 5] = ["how", "before", "build", "after", "env"];

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// What `groups.json` holds: each capability's group by identity, and whether the presets have
/// already been turned into selections.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub schema_version: u32,
    pub presets_migrated: bool,
    pub groups: BTreeMap<String, String>,
    /// Categories the owner made (version 2). A version 1 file has none.
    #[serde(default)]
    pub custom: Vec<Custom>,
}

/// A category the owner made: `u_` and 16 hex digits, a name, and the section it is shown in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Custom {
    pub id: String,
    pub name: String,
    pub section: String,
}

/// An owner category's id: `u_` and 16 lowercase hex digits.
pub fn custom_id_ok(id: &str) -> bool {
    id.len() == 18
        && id.starts_with("u_")
        && id[2..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A category name: 1 to 40 characters once trimmed, no control characters.
pub fn name_ok(name: &str) -> bool {
    let trimmed = name.trim();
    (1..=40).contains(&trimmed.chars().count()) && !trimmed.chars().any(char::is_control)
}

fn custom_ok(custom: &[Custom]) -> bool {
    custom.len() <= MAX_CUSTOM
        && custom.iter().all(|category| {
            custom_id_ok(&category.id)
                && name_ok(&category.name)
                && SECTIONS.contains(&category.section.as_str())
        })
}

/// A group is 1 to 32 characters of lowercase letters, digits and `_`.
pub fn group_ok(group_id: &str) -> bool {
    (1..=32).contains(&group_id.len())
        && group_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// An identity is 1 to 80 characters of lowercase hex digits and `#`, as the library writes them.
pub fn identity_ok(identity: &str) -> bool {
    (1..=80).contains(&identity.len())
        && identity
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'#')
}

/// The file's state, or an empty one when there is no file or the file is not a valid one.
pub fn load(directory: &Path) -> State {
    read(directory).unwrap_or_else(|| State {
        schema_version: VERSION,
        ..State::default()
    })
}

fn read(directory: &Path) -> Option<State> {
    let path = directory.join(FILE);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(&path)
        .ok()?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if u64::try_from(bytes.len()).ok()? > MAX_FILE_BYTES {
        return None;
    }
    let state: State = serde_json::from_slice(&bytes).ok()?;
    let valid = (1..=VERSION).contains(&state.schema_version)
        && custom_ok(&state.custom)
        && state.groups.len() <= MAX_GROUPS
        && state
            .groups
            .iter()
            .all(|(identity, group)| identity_ok(identity) && group_ok(group));
    valid.then_some(state)
}

/// Writes the state to a new temporary file created 0600, then renames it onto `groups.json`. The
/// version written is always this one. On any error the temporary file is removed.
pub fn save(directory: &Path, state: &State) -> io::Result<()> {
    let saved = State {
        schema_version: VERSION,
        ..state.clone()
    };
    let bytes = serde_json::to_vec(&saved).map_err(io::Error::other)?;
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = directory.join(format!(".groups.{}.{number}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let written = file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temp, directory.join(FILE)));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arsenal-groups-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        dir
    }

    fn empty() -> State {
        State {
            schema_version: VERSION,
            ..State::default()
        }
    }

    fn identity(n: u32) -> String {
        format!("{n:064x}")
    }

    #[test]
    fn a_missing_file_loads_as_empty_and_not_migrated() {
        let dir = scratch("missing");
        assert_eq!(load(&dir), empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_then_load_round_trips_the_groups_and_the_marker() {
        let dir = scratch("round");
        let mut state = State {
            schema_version: 7,
            presets_migrated: true,
            groups: BTreeMap::new(),
            custom: vec![Custom {
                id: "u_00000000000000ab".into(),
                name: "Clientes".into(),
                section: "build".into(),
            }],
        };
        state.groups.insert(identity(1), "design".into());
        save(&dir, &state).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.schema_version, VERSION);
        assert!(loaded.presets_migrated);
        assert_eq!(loaded.groups, state.groups);
        assert_eq!(loaded.custom, state.custom);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_saved_file_is_private_and_leaves_no_temporary_file() {
        let dir = scratch("private");
        save(&dir, &empty()).unwrap();
        let mode = fs::metadata(dir.join(FILE)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![FILE.to_owned()]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn damaged_files_load_as_empty_and_are_left_as_they_are() {
        let dir = scratch("damaged");
        let bad_group = format!(
            r#"{{"schema_version":1,"presets_migrated":false,"groups":{{"{}":"Bad Group"}}}}"#,
            identity(1)
        );
        let too_many: BTreeMap<String, String> = (0..=MAX_GROUPS as u32)
            .map(|n| (identity(n), "design".to_owned()))
            .collect();
        let cases: Vec<Vec<u8>> = vec![
            b"{not json".to_vec(),
            br#"{"schema_version":1,"presets_migrated":true,"groups":{},"extra":1}"#.to_vec(),
            br#"{"schema_version":3,"presets_migrated":true,"groups":{}}"#.to_vec(),
            br#"{"schema_version":2,"presets_migrated":true,"groups":{},"custom":[{"id":"u_x","name":"A","section":"build"}]}"#.to_vec(),
            br#"{"schema_version":2,"presets_migrated":true,"groups":{},"custom":[{"id":"u_00000000000000ab","name":"A","section":"nowhere"}]}"#.to_vec(),
            bad_group.into_bytes(),
            serde_json::to_vec(&State {
                schema_version: 1,
                presets_migrated: false,
                groups: too_many,
                custom: Vec::new(),
            })
            .unwrap(),
        ];
        for content in cases {
            fs::write(dir.join(FILE), &content).unwrap();
            assert_eq!(load(&dir), empty());
            assert_eq!(fs::read(dir.join(FILE)).unwrap(), content);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_version_1_file_still_loads_with_no_categories_of_the_owner() {
        let dir = scratch("v1");
        let content = format!(
            r#"{{"schema_version":1,"presets_migrated":true,"groups":{{"{}":"design"}}}}"#,
            identity(1)
        );
        fs::write(dir.join(FILE), &content).unwrap();
        let loaded = load(&dir);
        assert_eq!(
            loaded.groups.get(&identity(1)).map(String::as_str),
            Some("design")
        );
        assert!(loaded.custom.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn category_ids_and_names_are_checked() {
        assert!(custom_id_ok("u_0123456789abcdef"));
        assert!(!custom_id_ok("u_0123456789ABCDEF"));
        assert!(!custom_id_ok("design"));
        assert!(group_ok("u_0123456789abcdef"));
        assert!(name_ok("Clientes"));
        assert!(!name_ok("   "));
        assert!(!name_ok(&"x".repeat(41)));
        assert!(!name_ok("a\nb"));
    }

    #[test]
    fn a_symlink_named_like_the_file_is_not_followed() {
        let dir = scratch("link");
        let target = dir.join("elsewhere.json");
        let content = br#"{"schema_version":1,"presets_migrated":true,"groups":{}}"#;
        fs::write(&target, content).unwrap();
        std::os::unix::fs::symlink(&target, dir.join(FILE)).unwrap();
        assert_eq!(load(&dir), empty());
        assert_eq!(fs::read(&target).unwrap(), content);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn group_and_identity_forms_are_checked() {
        assert!(group_ok("design"));
        assert!(group_ok("a_1"));
        assert!(group_ok(&"g".repeat(32)));
        assert!(!group_ok(""));
        assert!(!group_ok("Design"));
        assert!(!group_ok("Bad Group"));
        assert!(!group_ok(&"g".repeat(33)));
        assert!(identity_ok(&"a".repeat(64)));
        assert!(identity_ok(&format!("{}#2", "a".repeat(64))));
        assert!(!identity_ok(""));
        assert!(!identity_ok("G".repeat(64).as_str()));
        assert!(!identity_ok("a/b"));
        assert!(!identity_ok(&"a".repeat(81)));
    }
}
