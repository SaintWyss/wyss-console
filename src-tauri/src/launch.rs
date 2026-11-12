//! Launch configurations (ADR 0059): the owner's own files, one per launch, in
//! `$XDG_CONFIG_HOME/wyss-console/launch/*.toml`, read by the desktop and named by the window.
//!
//! A launch either opens a terminal in a folder and types its commands there, as if the owner
//! typed them, or starts a Native session with an agent and a profile named by its name, after
//! running its `before` commands. The window never sends a command: it names a file by its
//! stem, and every command comes from that file. The folder sits in the synchronized home, as
//! the shell's own start-up files do, so both computers get the same launches.
use crate::arsenal::Arsenal;
use crate::arsenal::store::Store;
use crate::host::client::HostLink;
use crate::host::protocol::HostError;
use crate::native::commands::call_extended_or_legacy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::State;

pub const MAX_FILES: usize = 64;
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_NAME: usize = 60;
pub const MAX_COMMANDS: usize = 16;
pub const MAX_BEFORE: usize = 8;
pub const MAX_COMMAND: usize = 4096;
/// How long one `before` command may run; past it, it is stopped and the start goes on.
pub const BEFORE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    name: String,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    before: Vec<String>,
    #[serde(default)]
    session: Option<SessionSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionSpec {
    agent: String,
    #[serde(default)]
    profile: Option<String>,
    /// An Arsenal preset's name, for this agent's provider (ADR 0059 / arsenal-UI-SPEC).
    #[serde(default)]
    preset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    /// A new terminal in `directory` with these commands typed, each followed by Enter.
    Commands { commands: Vec<String> },
    /// A Native session; `before` runs first, without a terminal.
    Session {
        agent: String,
        profile: Option<String>,
        before: Vec<String>,
        preset: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Launch {
    /// The file's stem: what the window names.
    pub id: String,
    pub name: String,
    /// Absolute; the home folder when the file names none.
    pub directory: String,
    #[serde(flatten)]
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refused {
    pub id: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct List {
    /// Where the files are, as the owner writes it (`~/…`).
    pub folder: String,
    pub launches: Vec<Launch>,
    pub refused: Vec<Refused>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub command: String,
    /// `ok`, `exit <code>`, `signal`, `timeout` or `not_started`.
    pub result: String,
}

pub struct Places {
    pub folder: PathBuf,
    pub home: PathBuf,
}

impl Places {
    pub fn from_env() -> Option<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|h| h.is_absolute())?;
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        Some(Self {
            folder: config.join("wyss-console/launch"),
            home,
        })
    }
}

/// The launches that ship, written once when the folder does not exist yet. The owner edits or
/// deletes them; a folder that exists is never written again.
pub const DEFAULTS: [(&str, &str); 3] = [
    (
        "actualizar-sistema.toml",
        r#"# Opens a new terminal in `directory` and types each command, followed by Enter.
name = "Actualizar sistema"
directory = "~"
commands = ["sysupdate"]
"#,
    ),
    (
        "claude-code.toml",
        r#"# Starts Claude Code as a Native session with the profile named here (by its name).
# `before` runs first in /bin/sh, without a terminal; a failure is reported and the start goes on.
name = "Claude Code"
directory = "~"
before = [
  "STKEY=$(grep -oP '(?<=<apikey>)[^<]+' ~/.local/state/syncthing/config.xml) && curl -s -X POST -H \"X-API-Key: $STKEY\" 'http://127.0.0.1:8384/rest/db/scan?folder=home-saintwyss' && curl -s -X POST -H \"X-API-Key: $STKEY\" 'http://127.0.0.1:8384/rest/db/scan?folder=mnt-hdd-data'",
]

[session]
agent = "claude_code"
profile = "Full access (bypass)"
"#,
    ),
    (
        "codex.toml",
        r#"# Starts Codex as a Native session with the profile named here (by its name).
# `before` runs first in /bin/sh, without a terminal; a failure is reported and the start goes on.
name = "Codex"
directory = "~"
before = [
  "STKEY=$(grep -oP '(?<=<apikey>)[^<]+' ~/.local/state/syncthing/config.xml) && curl -s -X POST -H \"X-API-Key: $STKEY\" 'http://127.0.0.1:8384/rest/db/scan?folder=home-saintwyss' && curl -s -X POST -H \"X-API-Key: $STKEY\" 'http://127.0.0.1:8384/rest/db/scan?folder=mnt-hdd-data'",
]

[session]
agent = "codex"
profile = "Full access (bypass)"
"#,
    ),
];

fn text_ok(text: &str, most: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= most && !text.chars().any(char::is_control)
}

/// A command is one line: no control character (a newline or an escape would type more than
/// the file says).
fn command_ok(command: &str) -> bool {
    text_ok(command, MAX_COMMAND)
}

fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}

fn display(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn directory(spec: Option<&str>, home: &Path) -> Result<PathBuf, &'static str> {
    let path = match spec {
        None | Some("~") => home.to_path_buf(),
        Some(text) if text.starts_with("~/") => home.join(&text[2..]),
        Some(text) if Path::new(text).is_absolute() => PathBuf::from(text),
        Some(_) => return Err("directory must be ~, ~/… or an absolute path"),
    };
    let path = fs::canonicalize(&path).map_err(|_| "directory not found")?;
    if !path.is_dir() {
        return Err("directory is not a folder");
    }
    Ok(path)
}

fn parse(id: &str, text: &str, home: &Path) -> Result<Launch, String> {
    let spec: Spec = toml::from_str(text).map_err(|e| {
        let message = e.message().to_owned();
        format!("not a launch file: {message}")
    })?;
    if !text_ok(&spec.name, MAX_NAME) {
        return Err(format!("name must be 1–{MAX_NAME} characters on one line"));
    }
    let folder = directory(spec.directory.as_deref(), home)?;
    let kind = match (spec.session, spec.commands.is_empty()) {
        (Some(_), false) => return Err("a launch has commands or a session, not both".into()),
        (None, true) => return Err("a launch needs commands or a session".into()),
        (None, false) => {
            if !spec.before.is_empty() {
                return Err(
                    "before belongs to a session; put the commands in order instead".into(),
                );
            }
            if spec.commands.len() > MAX_COMMANDS || !spec.commands.iter().all(|c| command_ok(c)) {
                return Err(format!(
                    "at most {MAX_COMMANDS} commands, each one line of at most {MAX_COMMAND} characters"
                ));
            }
            Kind::Commands {
                commands: spec.commands,
            }
        }
        (Some(session), true) => {
            if !matches!(session.agent.as_str(), "claude_code" | "codex") {
                return Err("session agent must be claude_code or codex".into());
            }
            if session
                .profile
                .as_deref()
                .is_some_and(|p| !text_ok(p, crate::native::profile::MAX_NAME))
            {
                return Err("profile must be a profile's name".into());
            }
            if spec.before.len() > MAX_BEFORE || !spec.before.iter().all(|c| command_ok(c)) {
                return Err(format!(
                    "at most {MAX_BEFORE} before commands, each one line of at most {MAX_COMMAND} characters"
                ));
            }
            if session.preset.as_deref().is_some_and(|p| !text_ok(p, MAX_NAME)) {
                return Err(format!(
                    "preset must be a preset's name, 1–{MAX_NAME} characters on one line"
                ));
            }
            Kind::Session {
                agent: session.agent,
                profile: session.profile,
                before: spec.before,
                preset: session.preset,
            }
        }
    };
    Ok(Launch {
        id: id.to_owned(),
        name: spec.name.trim().to_owned(),
        directory: folder.to_string_lossy().into_owned(),
        kind,
    })
}

/// Writes the shipped launches when the folder does not exist yet.
fn seed(folder: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(folder).is_ok() {
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)?;
    for (name, text) in DEFAULTS {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(folder.join(name))?;
        file.write_all(text.as_bytes())?;
    }
    Ok(())
}

fn read_one(path: &Path) -> Result<String, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "unreadable".to_owned())?;
    if !meta.is_file() {
        return Err("not a plain file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| "unreadable".to_owned())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("larger than {} KiB", MAX_FILE_BYTES / 1024));
    }
    String::from_utf8(bytes).map_err(|_| "not UTF-8".to_owned())
}

pub fn list(places: &Places) -> Result<List, String> {
    seed(&places.folder).map_err(|e| format!("launch folder: {e}"))?;
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&places.folder).map_err(|e| format!("launch folder: {e}"))? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".toml") {
            entries.push((stem.to_owned(), entry.path()));
        }
    }
    entries.sort();
    let mut launches = Vec::new();
    let mut refused = Vec::new();
    for (count, (id, path)) in entries.into_iter().enumerate() {
        if count >= MAX_FILES {
            refused.push(Refused {
                id,
                error: format!("only the first {MAX_FILES} files are read"),
            });
            continue;
        }
        if !id_ok(&id) {
            refused.push(Refused {
                id,
                error: "file name must be lowercase letters, digits, - or _".into(),
            });
            continue;
        }
        match read_one(&path).and_then(|text| parse(&id, &text, &places.home)) {
            Ok(launch) => launches.push(launch),
            Err(error) => refused.push(Refused { id, error }),
        }
    }
    launches.sort_by_key(|launch| launch.name.to_lowercase());
    Ok(List {
        folder: display(&places.folder, &places.home),
        launches,
        refused,
    })
}

/// One launch by its id, read again from its file: the window's copy is never trusted.
pub fn get(places: &Places, id: &str) -> Result<Launch, String> {
    if !id_ok(id) {
        return Err("unknown launch".into());
    }
    let text = read_one(&places.folder.join(format!("{id}.toml")))?;
    parse(id, &text, &places.home)
}

/// Runs a session launch's `before` commands in order, in its folder, each in `/bin/sh -c`
/// with no input and its output discarded, bounded in time. Every one runs, whatever the
/// previous one did; what each did is returned.
pub fn run_before(launch: &Launch, timeout: Duration) -> Vec<Outcome> {
    let Kind::Session { before, .. } = &launch.kind else {
        return Vec::new();
    };
    before
        .iter()
        .map(|command| Outcome {
            command: command.clone(),
            result: run_one(command, Path::new(&launch.directory), timeout),
        })
        .collect()
}

fn run_one(command: &str, folder: &Path, timeout: Duration) -> String {
    let mut process = Command::new("/bin/sh");
    process
        .arg("-c")
        .arg(command)
        .current_dir(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in crate::native::service::SESSION_MARKERS {
        process.env_remove(name);
    }
    // Its own process group, so that a timeout stops what it started too.
    std::os::unix::process::CommandExt::process_group(&mut process, 0);
    let Ok(mut child) = process.spawn() else {
        return "not_started".into();
    };
    let started = Instant::now();
    let group = nix::unistd::Pid::from_raw(-(child.id() as i32));
    // Whatever the command left running in its group is stopped with it: a `before` line never
    // outlives its own run, whether it ends or times out.
    let stop = || {
        let _ = nix::sys::signal::kill(group, nix::sys::signal::Signal::SIGKILL);
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                stop();
                return match status.code() {
                    Some(0) => "ok".into(),
                    Some(code) => format!("exit {code}"),
                    None => "signal".into(),
                };
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                stop();
                let _ = child.wait();
                return "timeout".into();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Saving and deleting a launch from Workspace (arsenal-UI-SPEC, phase D)

/// What the desktop holds that a launch file names, by agent: its profiles (by name) and its
/// presets (Arsenal selections, by name, for that agent's provider).
pub trait Known {
    fn profiles(&self, agent: &str) -> Result<Vec<String>, String>;
    fn presets(&self, agent: &str) -> Result<Vec<String>, String>;
}

/// What the window sends to save a launch. There is no field for a command or a `before` line:
/// the file the window writes is always a session launch, or one that types nothing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    name: String,
    directory: String,
    agent: String,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    replace: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SaveRefusal {
    /// A launch with that name is already saved, and the window did not ask to replace it.
    Exists,
    Invalid(String),
}

/// The file name a launch's name gives: lowercase ASCII letters, digits and single dashes, at
/// most 40 characters, Latin accents folded. Empty when the name has none of those.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        let mapped = match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c if c.is_ascii_lowercase() || c.is_ascii_digit() => c,
            _ => '-',
        };
        if mapped == '-' {
            if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(mapped);
        }
    }
    out.truncate(40);
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// A TOML basic string: quoted, with the characters that must be escaped written escaped.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Writes `text` to `target` through a private temporary file and a rename, so a reader sees
/// the old file or the new one, never a part of either. The temporary name does not end in
/// `.toml`, so `list` never reads it.
fn write_private(folder: &Path, target: &Path, text: &str) -> std::io::Result<()> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temporary = folder.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = write_then_rename(&temporary, target, text);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_then_rename(temporary: &Path, target: &Path, text: &str) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(temporary, target)
}

/// Saves a session launch: the agent, the folder, the profile and the preset, written as the
/// file the reader already takes. Everything is checked again here, against what the desktop
/// holds, and the file is read back before the answer goes out.
pub fn save(
    places: &Places,
    request: &SaveRequest,
    known: &dyn Known,
) -> Result<Launch, SaveRefusal> {
    let invalid = |why: String| SaveRefusal::Invalid(why);
    if !text_ok(&request.name, MAX_NAME) {
        return Err(invalid(format!(
            "name must be 1–{MAX_NAME} characters on one line"
        )));
    }
    let agent = request.agent.as_str();
    if !matches!(agent, "claude_code" | "codex") {
        return Err(invalid("agent must be claude_code or codex".into()));
    }
    let id = slug(&request.name);
    if id.is_empty() {
        return Err(invalid("name needs at least one letter or digit".into()));
    }
    let folder = directory(Some(&request.directory), &places.home)
        .map_err(|why| invalid(why.to_owned()))?;
    if let Some(profile) = request.profile.as_deref() {
        if !text_ok(profile, crate::native::profile::MAX_NAME) {
            return Err(invalid("profile must be a profile's name".into()));
        }
        let names = known.profiles(agent).map_err(invalid)?;
        if !names.iter().any(|n| n == profile) {
            return Err(invalid(format!(
                "no profile named “{profile}” for this agent"
            )));
        }
    }
    if let Some(preset) = request.preset.as_deref() {
        if !text_ok(preset, MAX_NAME) {
            return Err(invalid("preset must be a preset's name".into()));
        }
        let names = known.presets(agent).map_err(invalid)?;
        if !names.iter().any(|n| n == preset) {
            return Err(invalid(format!(
                "no preset named “{preset}” for this agent"
            )));
        }
    }
    seed(&places.folder).map_err(|e| invalid(format!("launch folder: {e}")))?;
    let target = places.folder.join(format!("{id}.toml"));
    if let Ok(meta) = fs::symlink_metadata(&target) {
        if !meta.is_file() {
            return Err(invalid("a launch with that file name is not a plain file".into()));
        }
        if !request.replace {
            return Err(SaveRefusal::Exists);
        }
    }
    let shown = if folder == places.home {
        "~".to_owned()
    } else {
        display(&folder, &places.home)
    };
    let mut text = format!(
        "# Saved from Workspace (Wyss Console).\nname = {}\ndirectory = {}\n\n[session]\nagent = {}\n",
        toml_string(request.name.trim()),
        toml_string(&shown),
        toml_string(agent),
    );
    if let Some(profile) = &request.profile {
        text.push_str(&format!("profile = {}\n", toml_string(profile)));
    }
    if let Some(preset) = &request.preset {
        text.push_str(&format!("preset = {}\n", toml_string(preset)));
    }
    parse(&id, &text, &places.home).map_err(invalid)?;
    write_private(&places.folder, &target, &text)
        .map_err(|e| invalid(format!("the launch could not be written: {e}")))?;
    get(places, &id).map_err(invalid)
}

/// Deletes a launch the window saved: one with no `before` line and no commands. The owner's
/// own files, and the shipped ones with a `before` line, are edited by hand and never deleted here.
pub fn delete(places: &Places, id: &str) -> Result<(), String> {
    let launch = get(places, id)?;
    let saved = matches!(&launch.kind, Kind::Session { before, .. } if before.is_empty());
    if !saved {
        return Err("only a launch saved from Workspace can be deleted here; edit the others in their file".into());
    }
    fs::remove_file(places.folder.join(format!("{id}.toml")))
        .map_err(|_| "the launch could not be deleted".to_owned())
}

// Window commands

fn places() -> Result<Places, HostError> {
    Places::from_env().ok_or_else(|| HostError::new("launch_unavailable"))
}

fn refused(error: String) -> HostError {
    let mut e = HostError::new("launch_invalid");
    e.reasons.push(error);
    e
}

#[tauri::command]
pub async fn launch_list() -> Result<List, HostError> {
    tauri::async_runtime::spawn_blocking(|| list(&places()?).map_err(refused))
        .await
        .map_err(|_| HostError::new("internal"))?
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchRequest {
    id: String,
}

/// The launch as its file says now, for the window to open.
#[tauri::command]
pub async fn launch_get(request: LaunchRequest) -> Result<Launch, HostError> {
    tauri::async_runtime::spawn_blocking(move || get(&places()?, &request.id).map_err(refused))
        .await
        .map_err(|_| HostError::new("internal"))?
}

#[tauri::command]
pub async fn launch_before(request: LaunchRequest) -> Result<Vec<Outcome>, HostError> {
    tauri::async_runtime::spawn_blocking(move || {
        let launch = get(&places()?, &request.id).map_err(refused)?;
        Ok(run_before(&launch, BEFORE_TIMEOUT))
    })
    .await
    .map_err(|_| HostError::new("internal"))?
}

/// What the desktop holds: the profiles from the host, the presets from the Arsenal library.
struct Desktop {
    link: HostLink,
    library: Option<Arc<Mutex<Store>>>,
}

impl Known for Desktop {
    fn profiles(&self, agent: &str) -> Result<Vec<String>, String> {
        let value = call_extended_or_legacy(&self.link, "profiles_extended", "profiles", Value::Null)
            .map_err(|e| format!("the profiles could not be read ({})", e.code))?;
        Ok(value
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| {
                p["body"]["applies_to"]
                    .as_array()
                    .is_some_and(|agents| agents.iter().any(|a| a.as_str() == Some(agent)))
            })
            .filter_map(|p| p["body"]["name"].as_str().map(str::to_owned))
            .collect())
    }

    fn presets(&self, agent: &str) -> Result<Vec<String>, String> {
        let library = self.library.as_ref().ok_or("the Arsenal library is not open")?;
        let store = library
            .lock()
            .map_err(|_| "the Arsenal library is busy".to_owned())?;
        let selections = store
            .selections()
            .map_err(|_| "the Arsenal presets could not be read".to_owned())?;
        Ok(selections
            .into_iter()
            .filter(|s| s.provider == agent)
            .map(|s| s.name)
            .collect())
    }
}

fn saving(refusal: SaveRefusal) -> HostError {
    match refusal {
        SaveRefusal::Exists => HostError::new("exists"),
        SaveRefusal::Invalid(why) => refused(why),
    }
}

/// Saves the launch the window describes. The file is written by the desktop, which checks the
/// profile and the preset itself; a launch that already exists is replaced only when asked.
#[tauri::command]
pub async fn launch_save(
    request: SaveRequest,
    arsenal: State<'_, Arsenal>,
    link: State<'_, HostLink>,
) -> Result<Launch, HostError> {
    let known = Desktop {
        link: link.inner().clone(),
        library: arsenal.launch_handle(),
    };
    tauri::async_runtime::spawn_blocking(move || {
        save(&places()?, &request, &known).map_err(saving)
    })
    .await
    .map_err(|_| HostError::new("internal"))?
}

#[tauri::command]
pub async fn launch_delete(request: LaunchRequest) -> Result<(), HostError> {
    tauri::async_runtime::spawn_blocking(move || delete(&places()?, &request.id).map_err(refused))
        .await
        .map_err(|_| HostError::new("internal"))?
}

const COMMANDS: &[&str] = &[
    "launch_list",
    "launch_get",
    "launch_before",
    "launch_save",
    "launch_delete",
];

pub fn extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: COMMANDS,
        handler: Box::new(tauri::generate_handler![
            launch_list,
            launch_get,
            launch_before,
            launch_save,
            launch_delete
        ]),
        install: Box::new(|builder| builder),
        on_exit: None,
    }
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
