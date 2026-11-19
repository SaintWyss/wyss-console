//! What each preset chose for the launch environment (instructions, hooks, MCP, plugins, memory),
//! kept in `environment.json` beside the library, by selection id. A small private file rather
//! than a table, so the database schema stays the one older builds open. Loading never writes.
//!
//! What a Codex preset chose (instructions, hooks left out) is kept the same way in a file of its
//! own, `environment-codex.json` ([`codex`]): `environment.json` keeps the exact shape an older
//! build reads, which refuses a key it does not know.
//!
//! Both files are read strictly: a file that is not there is an empty one, but one that is
//! there and cannot be used (unreadable, not a regular file, too long, not this schema, an
//! invalid entry) is an error, [`UNREADABLE`]. A preset whose choice cannot be read must not
//! launch as if it had chosen nothing: the hooks it left out would run. Such a file is never
//! written over.
//! Neither file is written when its text would be longer than the reader accepts.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::native::launch_home::HomeSpec;
use crate::native::launch_home_codex::CodexHomeSpec;
use crate::native::store;

const FILE: &str = "environment.json";
const CODEX_FILE: &str = "environment-codex.json";
const VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const MAX_PRESETS: usize = 512;

/// One provider's launch environment, as its file keeps it.
trait Spec: Serialize + DeserializeOwned + Clone {
    const FILE: &'static str;
    /// Whether a file that is there but cannot be used is an error (never an empty one).
    fn valid(&self) -> bool;
    fn unchanged(&self) -> bool;
}

impl Spec for HomeSpec {
    const FILE: &'static str = FILE;
    fn valid(&self) -> bool {
        self.validate().is_ok()
    }
    fn unchanged(&self) -> bool {
        self.is_default()
    }
}

impl Spec for CodexHomeSpec {
    const FILE: &'static str = CODEX_FILE;
    fn valid(&self) -> bool {
        self.validate().is_ok()
    }
    fn unchanged(&self) -> bool {
        self.is_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State<S> {
    schema_version: u32,
    presets: BTreeMap<String, S>,
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

/// The error of a file that is there and cannot be used.
pub const UNREADABLE: &str = "environment_unreadable";
/// The error of a file that would be longer than the reader accepts: nothing is written.
pub const TOO_LARGE: &str = "environment_too_large";

fn empty<S>() -> State<S> {
    State {
        schema_version: VERSION,
        presets: BTreeMap::new(),
    }
}

/// The file as it is: empty when there is none, an error when there is one that cannot be used.
fn read<S: Spec>(directory: &Path) -> io::Result<State<S>> {
    let unreadable = || io::Error::other(UNREADABLE);
    let path = directory.join(S::FILE);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(empty()),
        Ok(meta) if meta.is_file() => {}
        _ => return Err(unreadable()),
    }
    let bytes = store::read_bounded(&path, MAX_FILE_BYTES).ok_or_else(unreadable)?;
    let state: State<S> = serde_json::from_slice(&bytes).map_err(|_| unreadable())?;
    let valid = state.schema_version == VERSION
        && state.presets.len() <= MAX_PRESETS
        && state
            .presets
            .iter()
            .all(|(id, spec)| id_ok(id) && spec.valid());
    if valid { Ok(state) } else { Err(unreadable()) }
}

fn write<S: Spec>(directory: &Path, state: &mut State<S>) -> io::Result<()> {
    state.schema_version = VERSION;
    let bytes = serde_json::to_vec(state).map_err(io::Error::other)?;
    // Checked before writing: a file the reader would refuse is never left on disk.
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::other(TOO_LARGE));
    }
    store::write_atomic(&directory.join(S::FILE), &bytes)
}

fn get_spec<S: Spec>(directory: &Path, selection: &str) -> io::Result<Option<S>> {
    Ok(read::<S>(directory)?
        .presets
        .remove(selection)
        .filter(|spec| !spec.unchanged()))
}

fn set_spec<S: Spec>(directory: &Path, selection: &str, spec: &S) -> io::Result<()> {
    if !id_ok(selection) || !spec.valid() {
        return Err(io::Error::other("invalid"));
    }
    let mut state = read::<S>(directory)?;
    if spec.unchanged() {
        state.presets.remove(selection);
    } else {
        if state.presets.len() >= MAX_PRESETS && !state.presets.contains_key(selection) {
            return Err(io::Error::other("too many"));
        }
        state.presets.insert(selection.to_owned(), spec.clone());
    }
    write(directory, &mut state)
}

/// The environment a preset chose; `None` when it keeps the owner's own. An error
/// ([`UNREADABLE`]) when the file is there and cannot be used: the caller must not go on as if the
/// preset had chosen nothing.
pub fn get(directory: &Path, selection: &str) -> io::Result<Option<HomeSpec>> {
    get_spec(directory, selection)
}

/// Records a preset's environment. A default one is removed, so the file only holds changes. A
/// file that is there and cannot be used is left as it is ([`UNREADABLE`]), and so is one the new
/// text would make too long ([`TOO_LARGE`]).
pub fn set(directory: &Path, selection: &str, spec: &HomeSpec) -> io::Result<()> {
    set_spec(directory, selection, spec)
}

/// The same three for a Codex preset, over `environment-codex.json`. A hook of Claude Code and one
/// of Codex with the same event, matcher and command have the same key: each file follows only
/// the edits of its own provider's hooks.
pub mod codex {
    use super::*;

    /// The environment a Codex preset chose; `None` when it keeps the owner's own. An error
    /// ([`UNREADABLE`]) when the file is there and cannot be used: the caller must not go on as
    /// if the preset had chosen nothing.
    pub fn get(directory: &Path, selection: &str) -> io::Result<Option<CodexHomeSpec>> {
        get_spec(directory, selection)
    }

    /// Records a Codex preset's environment. A default one is removed. A file that is there and
    /// cannot be used is left as it is ([`UNREADABLE`]), and so is one the new text would make
    /// too long ([`TOO_LARGE`]).
    pub fn set(directory: &Path, selection: &str, spec: &CodexHomeSpec) -> io::Result<()> {
        set_spec(directory, selection, spec)
    }

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
        assert_eq!(get(&dir, "abc-1").unwrap(), None);
        set(&dir, "abc-1", &spec).unwrap();
        assert_eq!(get(&dir, "abc-1").unwrap(), Some(spec));
        let default = HomeSpec {
            instructions: true,
            memory: true,
            ..HomeSpec::default()
        };
        set(&dir, "abc-1", &default).unwrap();
        assert_eq!(get(&dir, "abc-1").unwrap(), None);
        assert!(set(&dir, "../x", &default).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    fn codex_spec(keys: &[&str], instructions: bool) -> CodexHomeSpec {
        CodexHomeSpec {
            instructions,
            hooks_off: keys.iter().map(|k| (*k).to_owned()).collect(),
        }
    }

    #[test]
    fn a_codex_environment_round_trips_and_a_default_one_is_removed() {
        let dir = scratch("codex-round");
        let spec = codex_spec(&["0123456789abcdef"], false);
        assert_eq!(codex::get(&dir, "abc-1").unwrap(), None);
        codex::set(&dir, "abc-1", &spec).unwrap();
        assert_eq!(codex::get(&dir, "abc-1").unwrap(), Some(spec));
        assert!(dir.join(CODEX_FILE).is_file());
        codex::set(&dir, "abc-1", &CodexHomeSpec::default()).unwrap();
        assert_eq!(codex::get(&dir, "abc-1").unwrap(), None);
        assert!(codex::set(&dir, "../x", &codex_spec(&[], false)).is_err());
        assert!(codex::set(&dir, "abc-1", &codex_spec(&["not-a-key"], true)).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

}
