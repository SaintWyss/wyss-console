//! Distributors the owner adds: a tool the Console does not know, by its name and the commands
//! that open and update it, one file each in `$XDG_CONFIG_HOME/wyss-console/distributors/*.toml`.
//!
//! The Console never runs one on its own: "installed" means its executable's name is found where
//! the agents are looked for, and nothing is started to learn a version. Opening and updating
//! type the file's own commands in a new terminal, where they are seen; the window names the
//! file by its id, and the commands are read again from it. The window writes these files: one
//! line per command, no control character, the file re-parsed before it is written.
use crate::launch::{command_ok, display, id_ok, read_one, text_ok, toml_string, write_private};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

pub const MAX_FILES: usize = 32;
pub const MAX_TEXT: usize = 60;
pub const MAX_NOTE: usize = 280;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    name: String,
    #[serde(default)]
    maker: Option<String>,
    open: String,
    #[serde(default)]
    update: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Distributor {
    pub id: String,
    pub name: String,
    pub maker: Option<String>,
    /// Typed in a new terminal to open the tool.
    pub open: String,
    pub update: Option<String>,
    pub note: Option<String>,
    /// The name looked for: the first word of `open`.
    pub executable: String,
    /// Where it was found, as the owner writes it; `None` when it was not.
    pub found: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refused {
    pub id: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct List {
    pub folder: String,
    pub distributors: Vec<Distributor>,
    pub refused: Vec<Refused>,
    /// Where Open and Update start: the home folder, absolute.
    pub home: String,
}

#[derive(Clone)]
pub struct Places {
    pub folder: PathBuf,
    pub launches: PathBuf,
    pub home: PathBuf,
    pub path_var: Option<std::ffi::OsString>,
}

impl Places {
    pub fn from_env() -> Option<Self> {
        let launch = crate::launch::Places::from_env()?;
        Some(Self {
            folder: launch.folder.parent()?.join("distributors"),
            launches: launch.folder,
            home: launch.home,
            path_var: std::env::var_os("PATH"),
        })
    }
}

/// The executable an open command names: its first word, a bare name (looked for as the agents
/// are) or an absolute path.
fn executable_of(open: &str) -> Result<String, String> {
    let first = open.split_whitespace().next().unwrap_or_default();
    let bare = !first.is_empty()
        && first
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if bare || (first.starts_with('/') && !first.contains("/../")) {
        Ok(first.to_owned())
    } else {
        Err("open must start with the tool's command name (like kimi) or its absolute path".into())
    }
}

fn found(executable: &str, places: &Places) -> Option<String> {
    let path = if executable.starts_with('/') {
        let p = Path::new(executable);
        p.metadata()
            .ok()
            .filter(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.is_file() && m.permissions().mode() & 0o111 != 0
            })
            .map(|_| p.to_path_buf())
    } else {
        crate::native::catalogue::discover_name(executable, places.path_var.clone(), &places.home)
    }?;
    Some(display(&path, &places.home))
}

/// Makes `folder` if it is missing and refuses it when it, or the folder it sits in, is a link:
/// a file is never written through one to somewhere else.
fn private_folder(folder: &Path) -> Result<(), String> {
    for path in [folder.parent(), Some(folder)].into_iter().flatten() {
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!(
                "{} is a link: nothing is written through it",
                path.display()
            ));
        }
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)
        .map_err(|e| format!("folder: {e}"))?;
    if !fs::symlink_metadata(folder).is_ok_and(|m| m.is_dir()) {
        return Err(format!("{} is not a folder", folder.display()));
    }
    Ok(())
}

/// Writes a new file only: it is made under a private name and linked into place, which fails
/// when the name exists, so a file that appeared meanwhile is never replaced.
fn write_new(folder: &Path, target: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temporary = folder.join(format!(".{name}.{}.new", uuid::Uuid::new_v4().simple()));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::hard_link(&temporary, target)
    })();
    let _ = fs::remove_file(&temporary);
    result.map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => "a file with that name exists".to_owned(),
        _ => format!("it could not be written: {e}"),
    })
}

fn optional(text: Option<String>, most: usize, what: &str) -> Result<Option<String>, String> {
    match text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(t) if text_ok(&t, most) => Ok(Some(t)),
        Some(_) => Err(format!(
            "{what} must be at most {most} characters on one line"
        )),
    }
}

fn parse(id: &str, text: &str, places: &Places) -> Result<Distributor, String> {
    let spec: Spec =
        toml::from_str(text).map_err(|e| format!("not a distributor file: {}", e.message()))?;
    if !text_ok(&spec.name, MAX_TEXT) {
        return Err(format!("name must be 1–{MAX_TEXT} characters on one line"));
    }
    let open = spec.open.trim().to_owned();
    if !command_ok(&open) {
        return Err("open must be one command on one line".into());
    }
    let update = spec
        .update
        .map(|u| u.trim().to_owned())
        .filter(|u| !u.is_empty());
    if update.as_deref().is_some_and(|u| !command_ok(u)) {
        return Err("update must be one command on one line".into());
    }
    let executable = executable_of(&open)?;
    Ok(Distributor {
        id: id.to_owned(),
        name: spec.name.trim().to_owned(),
        maker: optional(spec.maker, MAX_TEXT, "maker")?,
        found: found(&executable, places),
        executable,
        open,
        update,
        note: optional(spec.note, MAX_NOTE, "note")?,
    })
}

pub fn list(places: &Places) -> Result<List, String> {
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    if let Ok(dir) = fs::read_dir(&places.folder) {
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".toml") {
                entries.push((stem.to_owned(), entry.path()));
            }
        }
    }
    entries.sort();
    let mut distributors = Vec::new();
    let mut refused = Vec::new();
    for (count, (id, path)) in entries.into_iter().enumerate() {
        let result = if count >= MAX_FILES {
            Err(format!("only the first {MAX_FILES} files are read"))
        } else if !id_ok(&id) {
            Err("file name must be lowercase letters, digits, - or _".into())
        } else {
            read_one(&path).and_then(|text| parse(&id, &text, places))
        };
        match result {
            Ok(d) => distributors.push(d),
            Err(error) => refused.push(Refused { id, error }),
        }
    }
    distributors.sort_by_key(|d| d.name.to_lowercase());
    Ok(List {
        folder: display(&places.folder, &places.home),
        distributors,
        refused,
        home: places.home.to_string_lossy().into_owned(),
    })
}

/// One distributor, read again from its file: the window's copy is never what runs.
pub fn get(places: &Places, id: &str) -> Result<Distributor, String> {
    if !id_ok(id) {
        return Err("unknown distributor".into());
    }
    let text = read_one(&places.folder.join(format!("{id}.toml")))?;
    parse(id, &text, places)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    /// The file it replaces; `None` for a new one, whose file name its name gives.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub maker: Option<String>,
    pub open: String,
    #[serde(default)]
    pub update: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

pub fn save(places: &Places, request: &SaveRequest) -> Result<Distributor, String> {
    let id = match &request.id {
        Some(id) if id_ok(id) => id.clone(),
        Some(_) => return Err("unknown distributor".into()),
        None => {
            let id = crate::launch::slug(&request.name);
            if id.is_empty() {
                return Err("name needs at least one letter or digit".into());
            }
            id
        }
    };
    let mut text = format!(
        "# Added in Personalization › Distributors (Wyss Console).\nname = {}\nopen = {}\n",
        toml_string(request.name.trim()),
        toml_string(request.open.trim()),
    );
    for (key, value) in [
        ("maker", &request.maker),
        ("update", &request.update),
        ("note", &request.note),
    ] {
        if let Some(v) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            text.push_str(&format!("{key} = {}\n", toml_string(v)));
        }
    }
    if text.len() as u64 > crate::launch::MAX_FILE_BYTES {
        return Err("too long".into());
    }
    // Checked as the reader takes it before anything is written.
    parse(&id, &text, places)?;
    private_folder(&places.folder)?;
    let target = places.folder.join(format!("{id}.toml"));
    match request.id {
        // A new one: only while there is room, and never over a file that exists.
        None => {
            let count = fs::read_dir(&places.folder)
                .map(|d| {
                    d.flatten()
                        .filter(|e| e.file_name().to_string_lossy().ends_with(".toml"))
                        .count()
                })
                .unwrap_or(0);
            if count >= MAX_FILES {
                return Err(format!("at most {MAX_FILES} distributors"));
            }
            write_new(&places.folder, &target, &text).map_err(|e| {
                if e.contains("exists") {
                    "a distributor with that name exists".to_owned()
                } else {
                    e
                }
            })?;
        }
        // An edit: only of a plain file that is there.
        Some(_) => {
            if !fs::symlink_metadata(&target).is_ok_and(|m| m.is_file()) {
                return Err("unknown distributor".into());
            }
            let before = get(places, &id)?;
            let launch = own_launch(places, &id, &before);
            write_private(&places.folder, &target, &text)
                .map_err(|e| format!("the distributor could not be written: {e}"))?;
            // Its launch, untouched since it was made, follows the new command.
            if let Some(old) = launch {
                let after = get(places, &id)?;
                let new = places
                    .launches
                    .join(format!("{}.toml", crate::launch::slug(&after.name)));
                if new == old || !new.exists() {
                    write_private(&places.launches, &new, &launch_text(&id, &after))
                        .map_err(|e| format!("its launch could not be written: {e}"))?;
                    if new != old {
                        let _ = fs::remove_file(&old);
                    }
                }
            }
        }
    }
    get(places, &id)
}

pub fn delete(places: &Places, id: &str) -> Result<(), String> {
    let d = get(places, id)?;
    let launch = own_launch(places, id, &d);
    fs::remove_file(places.folder.join(format!("{id}.toml")))
        .map_err(|_| "the distributor could not be deleted".to_owned())?;
    // Its launch goes with it, unless the owner changed it by hand.
    if let Some(path) = launch {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

/// The launch file `make_launch` writes for a distributor, byte for byte.
fn launch_text(id: &str, d: &Distributor) -> String {
    format!(
        "# Made from the distributor {id} (Wyss Console).\nname = {}\ndirectory = \"~\"\ncommands = [{}]\n",
        toml_string(&d.name),
        toml_string(&d.open),
    )
}

/// The launch made from this distributor, while it is still exactly as it was made: one edited
/// by hand is the owner's and is left alone.
fn own_launch(places: &Places, id: &str, d: &Distributor) -> Option<PathBuf> {
    let path = places
        .launches
        .join(format!("{}.toml", crate::launch::slug(&d.name)));
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    (fs::read_to_string(&path).ok()? == launch_text(id, d)).then_some(path)
}

/// Makes a commands launch that opens this distributor, in the home folder, from its own file:
/// the window sends no command. An existing launch of that name is never replaced.
pub fn make_launch(places: &Places, id: &str) -> Result<String, String> {
    let d = get(places, id)?;
    let launch_id = crate::launch::slug(&d.name);
    if launch_id.is_empty() {
        return Err("name needs at least one letter or digit".into());
    }
    let target = places.launches.join(format!("{launch_id}.toml"));
    let text = launch_text(id, &d);
    private_folder(&places.launches)?;
    write_new(&places.launches, &target, &text).map_err(|e| {
        if e.contains("exists") {
            "a launch with that name exists".to_owned()
        } else {
            e
        }
    })?;
    Ok(launch_id)
}

// Window commands


#[cfg(test)]
#[path = "distributors_tests.rs"]
mod tests;
