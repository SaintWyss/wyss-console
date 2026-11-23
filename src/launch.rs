//! Launch configurations: the owner's own files, one per launch, in
//! `$XDG_CONFIG_HOME/wyss-console/launch/*.toml`, read by the desktop and named by the window.
//!
//! A launch either opens a terminal in a folder and types its commands there, as if the owner
//! typed them, or starts a Native session with an agent and the permission it names, after
//! running its `before` commands. To start, the window names a file by its stem, and every
//! command comes from that file. The window writes commands in two places, both one line each
//! with no control character: a commands launch's distribution variants (`save_variants`, never
//! a session's `before`), and the distributors the owner adds (`distributors.rs`). The folder sits in the synchronized home, as
//! the shell's own start-up files do, so both computers get the same launches.
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
    /// What builds the variants' scripts in the window (`system-update`); the desktop only keeps it.
    #[serde(default)]
    template: Option<String>,
    /// One variant per distribution, by its os-release `ID`.
    #[serde(default)]
    distro: std::collections::BTreeMap<String, VariantSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantSpec {
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    manual: bool,
}

/// One distribution's commands for a launch, with the options the window composed them from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub distro: String,
    pub commands: Vec<String>,
    #[serde(default)]
    pub options: Vec<String>,
    /// Written by hand: the options no longer make its commands.
    #[serde(default)]
    pub manual: bool,
}

/// The distribution this computer runs, from os-release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct System {
    pub id: String,
    pub like: Vec<String>,
    pub name: String,
}

impl System {
    pub fn read(path: &Path) -> Self {
        let text = fs::read_to_string(path).unwrap_or_default();
        let mut out = System::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"').trim_matches('\'').to_owned();
            match key.trim() {
                "ID" => out.id = value.to_lowercase(),
                "ID_LIKE" => out.like = value.split_whitespace().map(str::to_lowercase).collect(),
                "PRETTY_NAME" => out.name = value,
                _ => {}
            }
        }
        out
    }

    /// The ids a variant may be saved under for this computer, the most specific first.
    pub fn chain(&self) -> Vec<&str> {
        std::iter::once(self.id.as_str())
            .chain(self.like.iter().map(String::as_str))
            .filter(|id| !id.is_empty())
            .collect()
    }
}

pub const MAX_VARIANTS: usize = 24;
pub const MAX_OPTIONS: usize = 16;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionSpec {
    agent: String,
    /// What the agent may do without asking: one of `PERMISSIONS`. Absent, it starts as it is
    /// configured.
    #[serde(default)]
    permission: Option<String>,
    /// Optional template ID accepted on read and mapped to `permission` on save.
    #[serde(default)]
    profile: Option<String>,
    /// An Arsenal preset's name, for this agent's provider.
    #[serde(default)]
    preset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    /// A new terminal in `directory` with these commands typed, each followed by Enter: the
    /// variant of this computer's distribution when there is one, else the file's own commands.
    Commands {
        commands: Vec<String>,
        variants: Vec<Variant>,
        /// The variant `commands` came from; it cannot be removed from here.
        active: Option<String>,
        template: Option<String>,
    },
    /// A Native session; `before` runs first, without a terminal.
    Session {
        agent: String,
        /// One of `PERMISSIONS`; none when the agent starts as it is configured.
        permission: Option<String>,
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
    pub system: System,
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
    pub os_release: PathBuf,
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
            os_release: PathBuf::from("/etc/os-release"),
        })
    }

    pub fn system(&self) -> System {
        System::read(&self.os_release)
    }
}

/// The launches that ship, written once when the folder does not exist yet. The owner edits or
/// deletes them; a folder that exists is never written again.
pub const DEFAULTS: [(&str, &str); 3] = [
    ("actualizar-sistema.toml", SYSTEM_UPDATE),
    (
        "claude-code.toml",
        r#"# Starts Claude Code as a Native session with the permission named here:
# "ask", "read_only", "edits" or "bypass"; without it, as it is configured.
name = "Claude Code"
directory = "~"

[session]
agent = "claude_code"
"#,
    ),
    (
        "codex.toml",
        r#"# Starts Codex as a Native session with the permission named here:
# "ask", "read_only", "edits" or "bypass"; without it, as it is configured.
name = "Codex"
directory = "~"

[session]
agent = "codex"
"#,
    ),
];

/// The shipped system update: one variant per distribution, edited in Workspace.
pub const SYSTEM_UPDATE: &str = r#"# Opens a new terminal in `directory` and types the commands of this computer's distribution.
# Each [distro.<os-release ID>] is edited in Workspace; `options` are the switches it was built from.
name = "Actualizar sistema"
directory = "~"
template = "system-update"

[distro.cachyos]
options = ["aur"]
commands = ["sudo pacman -Syu", "paru -Sua"]

[distro.arch]
commands = ["sudo pacman -Syu"]

[distro.debian]
commands = ["sudo apt update", "sudo apt full-upgrade"]

[distro.fedora]
commands = ["sudo dnf upgrade --refresh"]

[distro.opensuse]
commands = ["sudo zypper refresh", "sudo zypper dup"]
"#;

pub fn text_ok(text: &str, most: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= most && !text.chars().any(char::is_control)
}

/// A command is one line: no control character (a newline or an escape would type more than
/// the file says).
pub fn command_ok(command: &str) -> bool {
    text_ok(command, MAX_COMMAND)
}

pub fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}

pub fn display(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn directory_path(spec: Option<&str>, home: &Path) -> Result<PathBuf, &'static str> {
    let path = match spec {
        None | Some("~") => home.to_path_buf(),
        Some(text) if text.starts_with("~/") => home.join(&text[2..]),
        Some(text) if Path::new(text).is_absolute() => PathBuf::from(text),
        Some(_) => return Err("directory must be ~, ~/… or an absolute path"),
    };
    Ok(path)
}

fn directory(spec: Option<&str>, home: &Path) -> Result<PathBuf, &'static str> {
    let path = directory_path(spec, home)?;
    let path = fs::canonicalize(&path).map_err(|_| "directory not found")?;
    if !path.is_dir() {
        return Err("directory is not a folder");
    }
    Ok(path)
}

pub fn commands_ok(commands: &[String]) -> Result<(), String> {
    if commands.len() > MAX_COMMANDS || !commands.iter().all(|c| command_ok(c)) {
        return Err(format!(
            "at most {MAX_COMMANDS} commands, each one line of at most {MAX_COMMAND} characters"
        ));
    }
    Ok(())
}

/// What a session may do without asking, as a launch file names it.
pub const PERMISSIONS: [&str; 4] = ["ask", "read_only", "edits", "bypass"];

/// Resolves a session launch's permission from `permission` or a recognized `profile`
/// template ID. Unknown IDs are refused with guidance. This does not rewrite the file.
pub fn session_permission(
    _agent: &str,
    permission: Option<&str>,
    profile: Option<&str>,
) -> Result<Option<String>, String> {
    let permission = match (permission, profile) {
        (Some(_), Some(_)) => {
            return Err(
                "a session names permission or profile, not both: profiles were removed, keep permission"
                    .into(),
            );
        }
        (Some(word), None) => {
            if !PERMISSIONS.contains(&word) {
                return Err(
                    "permission must be \"ask\", \"read_only\", \"edits\" or \"bypass\"".into(),
                );
            }
            Some(word)
        }
        (None, Some("Full access (bypass)")) => Some("bypass"),
        (None, Some("Review, read only")) => Some("read_only"),
        (None, Some("Strict")) => Some("ask"),
        (None, Some("As configured")) | (None, None) => None,
        (None, Some(_)) => {
            return Err(
                "profiles were removed: set permission = \"ask\", \"read_only\", \"edits\" or \"bypass\" in this launch, or remove the profile line to start as configured"
                    .into(),
            );
        }
    };
    Ok(permission.map(str::to_owned))
}

fn variants_of(
    distro: std::collections::BTreeMap<String, VariantSpec>,
) -> Result<Vec<Variant>, String> {
    if distro.len() > MAX_VARIANTS {
        return Err(format!("at most {MAX_VARIANTS} distributions"));
    }
    distro
        .into_iter()
        .map(|(id, v)| {
            if !id_ok(&id) {
                return Err(format!(
                    "distro.{id}: an os-release ID is lowercase letters, digits, - or _"
                ));
            }
            if v.commands.is_empty() {
                return Err(format!("distro.{id} needs at least one command"));
            }
            commands_ok(&v.commands).map_err(|e| format!("distro.{id}: {e}"))?;
            if v.options.len() > MAX_OPTIONS || !v.options.iter().all(|o| id_ok(o)) {
                return Err(format!("distro.{id}: options are short lowercase names"));
            }
            Ok(Variant {
                distro: id,
                commands: v.commands,
                options: v.options,
                manual: v.manual,
            })
        })
        .collect()
}

fn parse(id: &str, text: &str, home: &Path, system: &System) -> Result<Launch, String> {
    parse_for_editor(id, text, home, system, false)
}

fn parse_for_editor(
    id: &str,
    text: &str,
    home: &Path,
    system: &System,
    allow_missing_directory: bool,
) -> Result<Launch, String> {
    let spec: Spec = toml::from_str(text).map_err(|e| {
        let message = e.message().to_owned();
        format!("not a launch file: {message}")
    })?;
    if !text_ok(&spec.name, MAX_NAME) {
        return Err(format!("name must be 1–{MAX_NAME} characters on one line"));
    }
    let folder = match directory(spec.directory.as_deref(), home) {
        Err("directory not found") if allow_missing_directory => {
            directory_path(spec.directory.as_deref(), home)?
        }
        result => result?,
    };
    let runs = !spec.commands.is_empty() || !spec.distro.is_empty();
    if spec.session.is_some() && (!spec.distro.is_empty() || spec.template.is_some()) {
        return Err("distro and template belong to a commands launch".into());
    }
    if spec.template.as_deref().is_some_and(|t| !id_ok(t)) {
        return Err("template must be a short lowercase name".into());
    }
    let kind = match (spec.session, runs) {
        (Some(_), true) => return Err("a launch has commands or a session, not both".into()),
        (None, false) => return Err("a launch needs commands or a session".into()),
        (None, true) => {
            if !spec.before.is_empty() {
                return Err(
                    "before belongs to a session; put the commands in order instead".into(),
                );
            }
            commands_ok(&spec.commands)?;
            let variants = variants_of(spec.distro)?;
            let matched = system
                .chain()
                .into_iter()
                .find_map(|id| variants.iter().find(|v| v.distro == id));
            Kind::Commands {
                commands: matched.map(|v| v.commands.clone()).unwrap_or(spec.commands),
                active: matched.map(|v| v.distro.clone()),
                variants,
                template: spec.template,
            }
        }
        (Some(session), false) => {
            if !matches!(session.agent.as_str(), "claude_code" | "codex") {
                return Err("session agent must be claude_code or codex".into());
            }
            let permission = session_permission(
                &session.agent,
                session.permission.as_deref(),
                session.profile.as_deref(),
            )?;
            if spec.before.len() > MAX_BEFORE || !spec.before.iter().all(|c| command_ok(c)) {
                return Err(format!(
                    "at most {MAX_BEFORE} before commands, each one line of at most {MAX_COMMAND} characters"
                ));
            }
            if session
                .preset
                .as_deref()
                .is_some_and(|p| !text_ok(p, MAX_NAME))
            {
                return Err(format!(
                    "preset must be a preset's name, 1–{MAX_NAME} characters on one line"
                ));
            }
            Kind::Session {
                agent: session.agent,
                permission,
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

/// Approves the shipped launches as they were written in `folder` (their folder is the home).
fn approve_seeded(places: &Places) {
    for (name, text) in DEFAULTS {
        let id = name.trim_end_matches(".toml");
        if let Ok(written) = read_one(&places.folder.join(name))
            && written == text
            && let Ok(launch) = parse(id, text, &places.home, &places.system())
        {
            approve_written(places, &launch, text);
        }
    }
}

pub fn read_one(path: &Path) -> Result<String, String> {
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

/// Exact single-command system update template recognized by `upgrade`.
/// Only a byte-identical file is replaced with the distribution-specific template.
pub const SYSTEM_UPDATE_BEFORE: &str = r#"# Opens a new terminal in `directory` and types each command, followed by Enter.
name = "Actualizar sistema"
directory = "~"
commands = ["sysupdate"]
"#;

fn upgrade(folder: &Path) {
    let target = folder.join("actualizar-sistema.toml");
    if read_one(&target).is_ok_and(|text| text == SYSTEM_UPDATE_BEFORE) {
        let _ = write_private(folder, &target, SYSTEM_UPDATE);
    }
}

/// The editor can open a launch to fix its directory. Execution uses strict validation.
pub fn list_for_editor(places: &Places) -> Result<List, String> {
    list_inner(places, true)
}

fn list_inner(places: &Places, allow_missing_directory: bool) -> Result<List, String> {
    let fresh = fs::symlink_metadata(&places.folder).is_err();
    seed(&places.folder).map_err(|e| format!("launch folder: {e}"))?;
    if fresh {
        approve_seeded(places);
    }
    upgrade(&places.folder);
    let system = places.system();
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
        match read_one(&path).and_then(|text| {
            parse_for_editor(&id, &text, &places.home, &system, allow_missing_directory)
        }) {
            Ok(launch) => launches.push(launch),
            Err(error) => refused.push(Refused { id, error }),
        }
    }
    launches.sort_by_key(|launch| launch.name.to_lowercase());
    Ok(List {
        folder: display(&places.folder, &places.home),
        launches,
        refused,
        system,
    })
}

/// One launch by its id, read again from its file: the window's copy is never trusted.
pub fn get(places: &Places, id: &str) -> Result<Launch, String> {
    get_inner(places, id, false)
}

pub fn get_for_editor(places: &Places, id: &str) -> Result<Launch, String> {
    get_inner(places, id, true)
}

fn get_inner(places: &Places, id: &str, allow_missing_directory: bool) -> Result<Launch, String> {
    if !id_ok(id) {
        return Err("unknown launch".into());
    }
    let text = read_one(&places.folder.join(format!("{id}.toml")))?;
    parse_for_editor(
        id,
        &text,
        &places.home,
        &places.system(),
        allow_missing_directory,
    )
}

/// What the window sends to replace a commands launch's distributions. The name, the folder,
/// the template and the file's own commands stay as the file says.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantsRequest {
    pub id: String,
    pub variants: Vec<Variant>,
}

/// Rewrites a commands launch with these distributions. The variant this computer runs cannot
/// be removed: the launch always keeps what this computer starts. The file is read back.
pub fn save_variants(places: &Places, request: &VariantsRequest) -> Result<Launch, String> {
    write_variants(places, request, None)
}

const WORKSPACE_COMMANDS: &str = "# Command launch created in Workspace (Wyss Console).";

pub fn create_commands(
    places: &Places,
    name: &str,
    variants: Vec<Variant>,
) -> Result<Launch, String> {
    let id = slug(name);
    if !id_ok(&id) || !text_ok(name, MAX_NAME) {
        return Err("invalid launch name".into());
    }
    seed(&places.folder).map_err(|e| e.to_string())?;
    if fs::symlink_metadata(places.folder.join(format!("{id}.toml"))).is_ok() {
        return Err("a launch with that name already exists".into());
    }
    let spec = Spec {
        name: name.trim().into(),
        directory: None,
        commands: vec![],
        before: vec![],
        session: None,
        template: None,
        distro: Default::default(),
    };
    write_variants(places, &VariantsRequest { id, variants }, Some(spec))
}

fn write_variants(
    places: &Places,
    request: &VariantsRequest,
    new: Option<Spec>,
) -> Result<Launch, String> {
    let creating = new.is_some();
    let (spec, created_in_workspace) = if let Some(spec) = new {
        (spec, true)
    } else {
        let launch = get(places, &request.id)?;
        let Kind::Commands { active, .. } = &launch.kind else {
            return Err("only a commands launch has distributions".into());
        };
        let text = read_one(&places.folder.join(format!("{}.toml", request.id)))?;
        if let Some(active) = active
            && !request.variants.iter().any(|v| &v.distro == active)
        {
            return Err(format!(
                "distro.{active} is this computer's distribution and stays"
            ));
        }
        (
            toml::from_str(&text).map_err(|_| "not a launch file".to_owned())?,
            text.lines().next() == Some(WORKSPACE_COMMANDS),
        )
    };
    // Bounded before anything is built from it: the window's request is never trusted.
    if request.variants.len() > MAX_VARIANTS {
        return Err(format!("at most {MAX_VARIANTS} distributions"));
    }
    for v in &request.variants {
        commands_ok(&v.commands)?;
        if v.options.len() > MAX_OPTIONS || !v.options.iter().all(|o| id_ok(o)) {
            return Err("options are short lowercase names".into());
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    if !request
        .variants
        .iter()
        .all(|v| seen.insert(v.distro.as_str()))
    {
        return Err("each distribution once".into());
    }
    if request.variants.is_empty() && spec.commands.is_empty() {
        return Err("a launch needs commands or at least one distribution".into());
    }
    let list = |items: &[String]| {
        format!(
            "[{}]",
            items
                .iter()
                .map(|c| toml_string(c))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let header = if created_in_workspace {
        WORKSPACE_COMMANDS
    } else {
        "# Edited in Workspace (Wyss Console). Each [distro.<os-release ID>] runs on that distribution."
    };
    let mut out = format!(
        "{header}\nname = {}\ndirectory = {}\n",
        toml_string(&spec.name),
        toml_string(spec.directory.as_deref().unwrap_or("~")),
    );
    if let Some(t) = &spec.template {
        out.push_str(&format!("template = {}\n", toml_string(t)));
    }
    if !spec.commands.is_empty() {
        out.push_str(&format!("commands = {}\n", list(&spec.commands)));
    }
    for v in &request.variants {
        if !id_ok(&v.distro) {
            return Err(
                "a distribution is its os-release ID: lowercase letters, digits, - or _".into(),
            );
        }
        out.push_str(&format!("\n[distro.{}]\n", v.distro));
        if !v.options.is_empty() {
            out.push_str(&format!("options = {}\n", list(&v.options)));
        }
        if v.manual {
            out.push_str("manual = true\n");
        }
        out.push_str(&format!("commands = {}\n", list(&v.commands)));
    }
    if out.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("larger than {} KiB", MAX_FILE_BYTES / 1024));
    }
    // Checked as the reader takes it before anything is written.
    let launch = parse(&request.id, &out, &places.home, &places.system())?;
    let target = places.folder.join(format!("{}.toml", request.id));
    write_launch_file(&places.folder, &target, &out, !creating)
        .map_err(|e| format!("the launch could not be written: {e}"))?;
    // The variants editor shows only the variant commands. A base command can become
    // active via the distro fallback, and the approval covers the whole file.
    if spec.commands.is_empty() {
        approve_written(places, &launch, &out);
    }
    Ok(launch)
}

// ---- approval of what a launch runs ---------------------------------------------------------

/// The fingerprint of a launch file: SHA-256 of its exact text, hex.
pub fn fingerprint(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// What an approval is bound to: the exact text of the file and the folder it resolves to, so
/// the same file opening somewhere else asks again. `text_print` is [`fingerprint`] of the text.
pub fn bind(text_print: &str, directory: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(text_print.as_bytes());
    hash.update([0]);
    hash.update(directory.as_bytes());
    format!("{:x}", hash.finalize())
}

/// [`bind`] of a launch's text and its folder.
pub fn approval_print(text: &str, directory: &str) -> String {
    bind(&fingerprint(text), directory)
}

/// Most fingerprints kept; the oldest approval goes first.
const MAX_APPROVALS: usize = 256;

/// The fingerprints of launch files the owner approved, in Wyss's private state
/// (`$XDG_STATE_HOME/wyss-console/launch-approvals.json`, 0600). The launch folder itself is
/// synchronized and any program of the owner may write there; the approval is not.
#[derive(Debug, Clone)]
pub struct Approvals {
    path: PathBuf,
}

/// A launch that types commands, runs `before` commands or starts with full access, from a file
/// whose exact text and folder the owner has not approved: what it would do, shown before
/// anything runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NeedsApproval {
    pub code: &'static str,
    pub id: String,
    pub fingerprint: String,
    /// The resolved folder it runs in.
    pub directory: String,
    /// The commands a commands launch types, each followed by Enter, exactly as sent.
    pub commands: Vec<String>,
    pub before: Vec<String>,
    pub bypass: bool,
}

/// What a launch runs, as the approval check sees it.
#[derive(Debug, Clone, Copy)]
pub struct Runs<'a> {
    pub directory: &'a str,
    pub commands: &'a [String],
    pub before: &'a [String],
    pub bypass: bool,
}

impl Approvals {
    pub fn from_env() -> Option<Self> {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|h| h.is_absolute())
                    .map(|h| h.join(".local/state"))
            })?;
        Some(Self::at(state.join("wyss-console").join("launch-approvals.json")))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    fn read(&self) -> Vec<String> {
        crate::native::store::read_bounded(&self.path, MAX_FILE_BYTES)
            .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn contains(&self, fingerprint: &str) -> bool {
        self.read().iter().any(|f| f == fingerprint)
    }

    pub fn approve(&self, fingerprint: &str) -> std::io::Result<()> {
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(std::io::Error::other("not a fingerprint"));
        }
        let mut all = self.read();
        all.retain(|f| f != fingerprint);
        all.push(fingerprint.to_owned());
        let excess = all.len().saturating_sub(MAX_APPROVALS);
        all.drain(..excess);
        let dir = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("no folder"))?;
        crate::native::store::private_dir(dir)?;
        let text = serde_json::to_vec(&all).map_err(std::io::Error::other)?;
        crate::native::store::write_atomic(&self.path, &text)
    }

    /// `Ok` when the launch may run as it is; else what the owner must see and approve.
    /// `fingerprint` is [`bind`] of the file's text and `runs.directory`. A shipped launch,
    /// byte-identical to how it shipped and in the home folder, needs no approval.
    pub fn check(&self, id: &str, fingerprint: &str, runs: Runs<'_>) -> Result<(), Box<NeedsApproval>> {
        let runs_nothing = runs.commands.is_empty() && runs.before.is_empty() && !runs.bypass;
        if runs_nothing || shipped(fingerprint, runs.directory) || self.contains(fingerprint) {
            return Ok(());
        }
        Err(Box::new(NeedsApproval {
            code: "needs_approval",
            id: id.to_owned(),
            fingerprint: fingerprint.to_owned(),
            directory: runs.directory.to_owned(),
            commands: runs.commands.to_vec(),
            before: runs.before.to_vec(),
            bypass: runs.bypass,
        }))
    }
}

/// Whether `print` is a shipped launch's text, unchanged, in `directory` (the home folder its
/// `directory = "~"` names). Those bytes are this program's own.
fn shipped(print: &str, directory: &str) -> bool {
    std::env::var_os("HOME").is_some_and(|home| Path::new(&home) == Path::new(directory))
        && DEFAULTS
            .iter()
            .any(|(_, text)| approval_print(text, directory) == print)
}

/// Approves what this program just wrote for the owner: a shipped launch or one saved from
/// Workspace. Without a private state folder nothing is approved and the file asks when opened.
fn approve_written(places: &Places, launch: &Launch, text: &str) {
    if let Some(approvals) = approvals_for(places) {
        let _ = approvals.approve(&approval_print(text, &launch.directory));
    }
}

/// Where approvals are kept for these places: Wyss's private state; in this crate's tests, the
/// test's own home, so that tests running together never share one store.
fn approvals_for(places: &Places) -> Option<Approvals> {
    #[cfg(test)]
    return Some(Approvals::at(
        places.home.join(".local/state/wyss-console/launch-approvals.json"),
    ));
    #[cfg(not(test))]
    {
        let _ = places;
        Approvals::from_env()
    }
}

/// [`get`], with the approval fingerprint ([`approval_print`]) of the very text it was parsed
/// from and the folder it resolved to.
pub fn get_with_fingerprint(places: &Places, id: &str) -> Result<(Launch, String), String> {
    if !id_ok(id) {
        return Err("unknown launch".into());
    }
    let text = read_one(&places.folder.join(format!("{id}.toml")))?;
    let launch = parse(id, &text, &places.home, &places.system())?;
    let print = approval_print(&text, &launch.directory);
    Ok((launch, print))
}

/// The variables a `before` command keeps from the Console's environment; every other one
/// (tokens, keys, the session markers) is left out.
const BEFORE_ENV: [&str; 12] = [
    "HOME",
    "PATH",
    "LANG",
    "LC_ALL",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "XDG_RUNTIME_DIR",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// Runs a session launch's `before` commands in order, in its folder, each in `/bin/sh -c`
/// with no input, its output discarded and a minimal environment, bounded in time. Every one
/// runs, whatever the previous one did; what each did is returned. Nothing runs unless the
/// file's `fingerprint` is approved (when it has commands or full access).
pub fn run_before(
    launch: &Launch,
    fingerprint: &str,
    timeout: Duration,
    approvals: &Approvals,
) -> Result<Vec<Outcome>, Box<NeedsApproval>> {
    let Kind::Session {
        before, permission, ..
    } = &launch.kind
    else {
        return Ok(Vec::new());
    };
    approvals.check(
        &launch.id,
        fingerprint,
        Runs {
            directory: &launch.directory,
            commands: &[],
            before,
            bypass: permission.as_deref() == Some("bypass"),
        },
    )?;
    Ok(before
        .iter()
        .map(|command| Outcome {
            command: command.clone(),
            result: run_one(command, Path::new(&launch.directory), timeout),
        })
        .collect())
}

fn run_one(command: &str, folder: &Path, timeout: Duration) -> String {
    let mut process = Command::new("/bin/sh");
    process
        .arg("-c")
        .arg(command)
        .current_dir(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_clear();
    for name in BEFORE_ENV {
        if let Some(value) = std::env::var_os(name) {
            process.env(name, value);
        }
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
// Saving and deleting a launch from Workspace

/// What the desktop holds that a launch file names, by agent: its presets (Arsenal selections,
/// by name, for that agent's provider).
pub trait Known {
    fn presets(&self, agent: &str) -> Result<Vec<String>, String>;
}

/// What the window sends to save a launch. There is no field for a command or a `before` line:
/// the file the window writes is always a session launch, or one that types nothing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    pub name: String,
    pub directory: String,
    pub agent: String,
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub replace: bool,
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
pub(crate) fn toml_string(text: &str) -> String {
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
pub(crate) fn write_private(folder: &Path, target: &Path, text: &str) -> std::io::Result<()> {
    write_launch_file(folder, target, text, true)
}

fn write_launch_file(
    folder: &Path,
    target: &Path,
    text: &str,
    replace: bool,
) -> std::io::Result<()> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temporary = folder.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = write_then_publish(&temporary, target, text, replace);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_then_publish(
    temporary: &Path,
    target: &Path,
    text: &str,
    replace: bool,
) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    if replace {
        fs::rename(temporary, target)
    } else {
        fs::hard_link(temporary, target)?;
        let _ = fs::remove_file(temporary);
        Ok(())
    }
}

/// The owner's two full-access launches in the home folder. Workspace lists and starts them, and
/// never writes or deletes their files.
const FIXED: [&str; 2] = ["claude-global", "codex-global"];
pub const FIXED_REFUSAL: &str = "This launch is fixed and cannot be edited";

pub fn is_fixed(id: &str) -> bool {
    FIXED.contains(&id)
}

pub fn save_session(
    places: &Places,
    request: &SaveRequest,
    known: &dyn Known,
    before: &[String],
) -> Result<Launch, SaveRefusal> {
    let invalid = SaveRefusal::Invalid;
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
    if is_fixed(&id) {
        return Err(invalid(FIXED_REFUSAL.into()));
    }
    let folder =
        directory(Some(&request.directory), &places.home).map_err(|why| invalid(why.to_owned()))?;
    let permission =
        session_permission(agent, request.permission.as_deref(), None).map_err(invalid)?;
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
            return Err(invalid(
                "a launch with that file name is not a plain file".into(),
            ));
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
        "# Saved from Workspace (Wyss Console).\nname = {}\ndirectory = {}\n",
        toml_string(request.name.trim()),
        toml_string(&shown),
    );
    if !before.is_empty() {
        let lines = before
            .iter()
            .map(|line| toml_string(line))
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&format!("before = [{lines}]\n"));
    }
    text.push_str(&format!("\n[session]\nagent = {}\n", toml_string(agent)));
    if let Some(permission) = &permission {
        text.push_str(&format!("permission = {}\n", toml_string(permission)));
    }
    if let Some(preset) = &request.preset {
        text.push_str(&format!("preset = {}\n", toml_string(preset)));
    }
    parse(&id, &text, &places.home, &places.system()).map_err(invalid)?;
    write_launch_file(&places.folder, &target, &text, request.replace).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            SaveRefusal::Exists
        } else {
            invalid(format!("the launch could not be written: {error}"))
        }
    })?;
    get(places, &id).map_err(invalid)
}

/// Deletes a launch the window saved: one with no `before` line and no commands. The owner's
/// own files, and the shipped ones with a `before` line, are edited by hand and never deleted here.
pub fn delete(places: &Places, id: &str) -> Result<(), String> {
    if is_fixed(id) {
        return Err(FIXED_REFUSAL.into());
    }
    let launch = get_for_editor(places, id)?;
    let saved = matches!(&launch.kind, Kind::Session { before, .. } if before.is_empty());
    if !saved {
        return Err(
            "only a launch saved from Workspace can be deleted here; edit the others in their file"
                .into(),
        );
    }
    fs::remove_file(places.folder.join(format!("{id}.toml")))
        .map_err(|_| "the launch could not be deleted".to_owned())
}

pub fn can_delete_commands(places: &Places, id: &str) -> bool {
    let Ok(launch) = get_for_editor(places, id) else {
        return false;
    };
    let Kind::Commands { template, .. } = launch.kind else {
        return false;
    };
    (id == "actualizar-sistema" && template.as_deref() == Some("system-update"))
        || read_one(&places.folder.join(format!("{id}.toml")))
            .is_ok_and(|text| text.lines().next() == Some(WORKSPACE_COMMANDS))
}

pub fn delete_commands(places: &Places, id: &str) -> Result<(), String> {
    if !can_delete_commands(places, id) {
        return Err("Only command launches created in Workspace can be deleted here".into());
    }
    fs::remove_file(places.folder.join(format!("{id}.toml"))).map_err(|error| error.to_string())
}


#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;

#[cfg(test)]
pub fn list(places: &Places) -> Result<List, String> {
    list_inner(places, false)
}

/// Saves a session launch: the agent, the folder, the permission and the preset, written as the
/// file the reader already takes. Everything is checked again here, against what the desktop
/// holds, and the file is read back before the answer goes out.
#[cfg(test)]
pub fn save(
    places: &Places,
    request: &SaveRequest,
    known: &dyn Known,
) -> Result<Launch, SaveRefusal> {
    save_session(places, request, known, &[])
}
