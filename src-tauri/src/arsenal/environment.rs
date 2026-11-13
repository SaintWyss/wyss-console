//! What each preset chose for the launch environment (instructions, hooks, MCP, plugins, memory),
//! kept in `environment.json` beside the library, by selection id. A small private file rather
//! than a table, so the database schema stays the one older builds open. A damaged file loads as
//! empty (every preset then launches as the owner's own configuration), and loading never writes.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::native::launch_home::HomeSpec;
use crate::native::store;

const FILE: &str = "environment.json";
const VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const MAX_PRESETS: usize = 512;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    presets: BTreeMap<String, HomeSpec>,
}

/// `wyss-console/arsenal` under `XDG_DATA_HOME` (or `~/.local/share`): the library's own folder.
pub fn directory() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute())
                .map(|home| home.join(".local/share"))
        })
        .map(|data| data.join("wyss-console").join("arsenal"))
}

fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
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
            && state.presets.len() <= MAX_PRESETS
            && state
                .presets
                .iter()
                .all(|(id, spec)| id_ok(id) && spec.validate().is_ok());
        valid.then_some(state)
    };
    read().unwrap_or(State {
        schema_version: VERSION,
        presets: BTreeMap::new(),
    })
}

/// The environment a preset chose; `None` when it keeps the owner's own.
pub fn get(directory: &Path, selection: &str) -> Option<HomeSpec> {
    load(directory)
        .presets
        .remove(selection)
        .filter(|spec| !spec.is_default())
}

/// Records a preset's environment. A default one is removed, so the file only holds changes.
pub fn set(directory: &Path, selection: &str, spec: &HomeSpec) -> io::Result<()> {
    if !id_ok(selection) || spec.validate().is_err() {
        return Err(io::Error::other("invalid"));
    }
    let mut state = load(directory);
    if spec.is_default() {
        state.presets.remove(selection);
    } else {
        if state.presets.len() >= MAX_PRESETS && !state.presets.contains_key(selection) {
            return Err(io::Error::other("too many"));
        }
        state.presets.insert(selection.to_owned(), spec.clone());
    }
    state.schema_version = VERSION;
    let bytes = serde_json::to_vec(&state).map_err(io::Error::other)?;
    store::write_atomic(&directory.join(FILE), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arsenal-env-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_chosen_environment_round_trips_and_a_default_one_is_not_kept() {
        let dir = scratch("round");
        let spec = HomeSpec {
            instructions: false,
            memory: true,
            mcp: Some(vec!["context7".into()]),
            ..HomeSpec::default()
        };
        assert_eq!(get(&dir, "abc-1"), None);
        set(&dir, "abc-1", &spec).unwrap();
        assert_eq!(get(&dir, "abc-1"), Some(spec));
        let default = HomeSpec {
            instructions: true,
            memory: true,
            ..HomeSpec::default()
        };
        set(&dir, "abc-1", &default).unwrap();
        assert_eq!(get(&dir, "abc-1"), None);
        assert!(set(&dir, "../x", &default).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_damaged_file_loads_as_empty() {
        let dir = scratch("damaged");
        fs::write(dir.join(FILE), "{not json").unwrap();
        assert_eq!(get(&dir, "abc"), None);
        fs::write(
            dir.join(FILE),
            r#"{"schema_version":1,"presets":{"abc":{"rules_off":["../x"]}}}"#,
        )
        .unwrap();
        assert_eq!(get(&dir, "abc"), None);
        fs::remove_dir_all(dir).unwrap();
    }
}
