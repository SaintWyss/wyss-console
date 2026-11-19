//! The configuration folder one Codex launch runs with (`CODEX_HOME`, with the real `HOME`),
//! built from the owner's own `~/.codex` and the environment an Arsenal preset chose (H4.2).
//! The launch home writes its own `config.toml` (the owner's text, with the hook trust this home
//! was given instead of the owner's) and its own `hooks.json` (only the chosen hooks), and links
//! the instructions, the sessions and the sign-in. The sign-in is never copied and never read:
//! `auth.json` and `auth.lock` are links to the owner's, so every home renews the same file under
//! the same lock. In the owner's folder the only thing ever written is an empty `auth.lock`, and
//! only when there is none.
//!
//! The host builds one only for a launch whose preset changes something
//! ([`CodexHomeSpec::is_default`] is false); every other Codex launch runs with the owner's own
//! folder and no `CODEX_HOME`, as before.
//!
//! A launch that leaves one hook out must differ from the owner's Codex in that hook only, so the
//! home also links what Codex reads from its folder besides the files above ([`LINKED`]): rules,
//! skills, plugins, memories, agents, prompts and the folder of hook scripts. What Codex writes
//! for itself is not linked, so two Codex processes never share it through two paths and the home
//! starts its own: the SQLite files and logs, `cache`, `packages`, `shell_snapshots`,
//! `models_cache.json` and `version.json`.
//!
//! Hooks Codex reads from `config.toml` itself (`[[hooks.<event>]]`) are not filtered:
//! `hooks_off` only names hooks of `hooks.json`. [`options`] lists them apart, so the window
//! shows them as always running and never offers to leave one out.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::launch_home::{self, MAX_NAMES, key_ok, place};
use super::store;
use crate::agentconfig::tomlspan;

const CONFIG: &str = "config.toml";
const HOOKS: &str = "hooks.json";
const INSTRUCTIONS: &str = "AGENTS.md";
const AUTH: &str = "auth.json";
const AUTH_LOCK: &str = "auth.lock";
const SESSIONS: &str = "sessions";
/// Entries of the owner's folder a launch home links as they are, when the owner has them.
pub const LINKED: [&str; 7] = [
    "rules", "skills", "plugins", "memories", "agents", "prompts", "hooks",
];
const MAX_FILE: u64 = 4 * 1024 * 1024;

/// What a preset changes in a Codex launch environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexHomeSpec {
    /// The owner's `~/.codex/AGENTS.md`.
    #[serde(default = "yes")]
    pub instructions: bool,
    /// Hooks of `~/.codex/hooks.json` left out, by [`launch_home::hook_key`].
    #[serde(default)]
    pub hooks_off: Vec<String>,
}

fn yes() -> bool {
    true
}

/// The owner's own configuration, unchanged.
impl Default for CodexHomeSpec {
    fn default() -> Self {
        Self {
            instructions: true,
            hooks_off: Vec::new(),
        }
    }
}

impl CodexHomeSpec {
    /// `Err("home_invalid")` when a key is not a [`launch_home::hook_key`], is repeated, or there
    /// are more than [`MAX_NAMES`].
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.hooks_off.len() > MAX_NAMES
            || !self.hooks_off.iter().all(|k| key_ok(k))
            || self.hooks_off.iter().collect::<BTreeSet<_>>().len() != self.hooks_off.len()
        {
            return Err("home_invalid");
        }
        Ok(())
    }

    /// The preset changes nothing.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

pub struct CodexOwner {
    /// `~/.codex`
    pub dir: PathBuf,
}

impl CodexOwner {
    pub fn from_home(home: &Path) -> Self {
        Self {
            dir: home.join(".codex"),
        }
    }

    /// The folder the owner's own Codex uses when this process starts it: an absolute
    /// `CODEX_HOME` of this process, or `.codex` under `home`. The same rule the Arsenal uses to
    /// find the `hooks.json` it edits.
    pub fn from_process(home: &Path) -> Self {
        std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .map_or_else(|| Self::from_home(home), |dir| Self { dir })
    }
}

fn fail(code: &'static str) -> io::Error {
    io::Error::other(code)
}

fn present(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// A file's text: `None` when there is no such file, an error when there is one that is a link,
/// is not the user's, is too long or is not UTF-8. Never used on the sign-in.
fn read_text(path: &Path, code: &'static str) -> io::Result<Option<String>> {
    if !present(path)? {
        return Ok(None);
    }
    let bytes = store::read_bounded(path, MAX_FILE).ok_or_else(|| fail(code))?;
    String::from_utf8(bytes).map(Some).map_err(|_| fail(code))
}

fn is_state(path: &[String]) -> bool {
    path.len() >= 2 && path[0] == "hooks" && path[1] == "state"
}

/// The byte ranges of every `[hooks.state…]` table of `text`: header to the end of its body.
fn state_spans(text: &str, code: &'static str) -> io::Result<Vec<(usize, usize)>> {
    let document = tomlspan::parse(text).map_err(|_| fail(code))?;
    Ok(document
        .tables
        .iter()
        .filter(|table| is_state(&table.path))
        .map(|table| (table.header.0, table.body_end))
        .collect())
}

/// A parsed configuration as (everything but `hooks.state`, `hooks.state`). A `hooks` table that
/// held nothing else counts as absent, and so does an empty `hooks.state`.
fn split_state(mut table: toml::Table) -> (toml::Table, toml::Table) {
    let mut state = toml::Table::new();
    let mut drop_hooks = false;
    if let Some(toml::Value::Table(hooks)) = table.get_mut("hooks") {
        if let Some(toml::Value::Table(found)) = hooks.get("state") {
            state = found.clone();
            hooks.remove("state");
        }
        drop_hooks = hooks.is_empty();
    }
    if drop_hooks {
        table.remove("hooks");
    }
    (table, state)
}

/// The `config.toml` of a launch home: the owner's text without its `[hooks.state…]` tables,
/// followed by the `[hooks.state…]` tables the home's own `config.toml` already has (the trust
/// approved in that home). Comments, order and quoting of everything kept stay as written.
///
/// Both texts must parse as TOML, and the result is checked against them as parsed values: its
/// `hooks.state` is the home's and the rest is the owner's. A layout where that cannot be done by
/// text (`hooks.state` written as an inline table or as dotted keys under `[hooks]`) is an error,
/// never a guess. The same inputs give the same bytes, and composing the result again as the
/// home's text gives it back.
pub fn config_text(owner: &str, home: &str) -> io::Result<String> {
    let owner_value: toml::Table =
        toml::from_str(owner).map_err(|_| fail("codex_home_owner_config_malformed"))?;
    let home_value: toml::Table =
        toml::from_str(home).map_err(|_| fail("codex_home_config_malformed"))?;
    let mut text = String::with_capacity(owner.len());
    let mut at = 0;
    for (start, end) in state_spans(owner, "codex_home_owner_config_unsupported")? {
        text.push_str(&owner[at..start]);
        at = end;
    }
    text.push_str(&owner[at..]);
    text.truncate(text.trim_end().len());
    if !text.is_empty() {
        text.push('\n');
    }
    for (start, end) in state_spans(home, "codex_home_config_unsupported")? {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(home[start..end].trim_end());
        text.push('\n');
    }
    let result: toml::Table =
        toml::from_str(&text).map_err(|_| fail("codex_home_config_unsupported"))?;
    let (rest, state) = split_state(result);
    if rest != split_state(owner_value).0 || state != split_state(home_value).1 {
        return Err(fail("codex_home_config_unsupported"));
    }
    Ok(text)
}

/// The `hooks.json` of a launch home, from the bytes of the owner's: the same bytes when nothing
/// is left out; otherwise the owner's without the hooks in `hooks_off`
/// ([`launch_home::hook_key`] over event, matcher and command), without the groups and events
/// that end up empty, and with every other key as it was. Bytes that are not JSON are an error,
/// and so is a key in `hooks_off` that names no hook of the file (`codex_home_hook_unknown`).
pub fn hooks_file(owner: &[u8], spec: &CodexHomeSpec) -> io::Result<Vec<u8>> {
    let value: Value =
        serde_json::from_slice(owner).map_err(|_| fail("codex_home_owner_hooks_malformed"))?;
    if spec.hooks_off.is_empty() {
        return Ok(owner.to_vec());
    }
    let filter = launch_home::HomeSpec {
        hooks_off: spec.hooks_off.clone(),
        ..launch_home::HomeSpec::default()
    };
    // Every key must name a hook of the file: one that names none is a hook that was edited or
    // removed since the preset left it out, and the launch cannot say it is still out.
    let listed = launch_home::hook_options(&value);
    if spec
        .hooks_off
        .iter()
        .any(|key| !listed.iter().any(|hook| &hook.key == key))
    {
        return Err(fail("codex_home_hook_unknown"));
    }
    serde_json::to_vec_pretty(&launch_home::settings(&value, &filter)).map_err(io::Error::other)
}

/// The sign-in entries of the home. Unlike [`place`], this never removes a file: a regular
/// `auth.json` in the home may be the only valid sign-in (a renewal that replaced the link), so
/// it stops the build for the owner to look at. A link that points somewhere else is refused too.
/// With `apply` false it only checks.
fn sign_in_link(link: &Path, target: Option<&Path>, apply: bool) -> io::Result<()> {
    match fs::symlink_metadata(link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let found = fs::read_link(link)?;
            match target {
                Some(target) if found == target => Ok(()),
                Some(_) => Err(fail("codex_home_sign_in_unexpected")),
                // A link holds no sign-in: one the owner no longer has a target for goes.
                None if apply => fs::remove_file(link),
                None => Ok(()),
            }
        }
        Ok(_) => Err(fail("codex_home_sign_in_unexpected")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => match target {
            Some(target) if apply => std::os::unix::fs::symlink(target, link),
            _ => Ok(()),
        },
        Err(e) => Err(e),
    }
}

/// Refuses a home path that could lead somewhere else: a `..` part anywhere, or a part that
/// exists and is a link. Parts that do not exist yet are made by `private_dir` as plain folders.
// A `.` part is not refused: `Path::components` drops it from an absolute path, which names the
// same folder without it, so it never reaches the loop (the `CurDir` arm is only a guard).
// TOCTOU: the check and the writes that follow go by path, not by an open folder, so a part
// swapped for a link in between is not seen. Every part from the state folder down is a 0700
// folder of the user, so only a process already running as the user can do that.
fn plain_path(dir: &Path) -> io::Result<()> {
    use std::path::Component;
    let mut walked = PathBuf::new();
    // Past the first part that does not exist nothing is on disk to look at, but the rest of the
    // path is still walked: a later `..` is refused all the same.
    let mut missing = false;
    for part in dir.components() {
        match part {
            Component::RootDir | Component::Prefix(_) => walked.push(part),
            Component::Normal(_) if missing => {}
            Component::Normal(name) => {
                walked.push(name);
                match fs::symlink_metadata(&walked) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        return Err(fail("codex_home_path_link"));
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => missing = true,
                    Err(e) => return Err(e),
                }
            }
            Component::ParentDir | Component::CurDir => return Err(fail("codex_home_path")),
        }
    }
    Ok(())
}

/// Whether the owner's `auth.lock` is there. One that is there must be a regular file of the
/// user that nobody else can write: anything else is refused, and it is never changed or replaced.
/// Codex opens `<home>/auth.lock` without a mode (0.169.0, `storage.rs` `lock_for_write`), so the
/// file it creates is 0644 under the usual umask: reading is fine, group/other writing is not.
fn owner_lock(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    match fs::symlink_metadata(path) {
        Ok(meta)
            if meta.is_file()
                && meta.uid() == nix::unistd::geteuid().as_raw()
                && meta.permissions().mode() & 0o022 == 0 =>
        {
            Ok(true)
        }
        Ok(_) => Err(fail("codex_home_owner_lock_unexpected")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Builds (or brings up to date) the Codex launch home at `dir` and returns it. Safe to run again
/// for the next launch of the same preset: the same inputs leave the same home, and what Codex
/// keeps there (its `[hooks.state…]` trust, anything it wrote beside these entries) stays.
///
/// What the home holds afterwards:
/// - `config.toml` (0600): [`config_text`] of the owner's and the home's own. No owner
///   `config.toml` counts as an empty one.
/// - `hooks.json` (0600): [`hooks_file`] of the owner's; absent when the owner has none.
/// - `AGENTS.md`: a link to the owner's, when `spec.instructions` and the owner has one. An owner
///   with only `CLAUDE.md` gets no instructions here: none is made up.
/// - `auth.json`, `auth.lock`: links to the owner's, when the owner has an `auth.json`. The
///   owner's `auth.lock` is created empty (0600) if it is missing, so every home takes the same
///   lock. The sign-in itself is never opened.
/// - `sessions`: a link to the owner's, when the owner has one.
/// - each of [`LINKED`]: a link to the owner's, when the owner has one. A folder of that name
///   Codex made in the home while the owner had none is the home's own and stays.
///
/// Errors, all before anything is written: an invalid `spec`; a `dir` or owner folder that is not
/// absolute; a home path with `..` or with a link in it; no owner folder, or one that is a link;
/// an owner `auth.json` that is not a regular file, `auth.lock` that is not a private regular
/// file of the user, or `sessions` that is not a folder; an owner or home `config.toml` / `hooks.json` that is a link,
/// unreadable or malformed; a home `auth.json` or `auth.lock` that is not the expected link. A
/// folder where a link or a file goes is an error too, and is never removed: that includes a
/// folder of the home's own under a [`LINKED`] name once the owner has one of that name
/// (`codex_home_own_folder`).
pub fn build(owner: &CodexOwner, dir: &Path, spec: &CodexHomeSpec) -> io::Result<PathBuf> {
    spec.validate().map_err(io::Error::other)?;
    if !dir.is_absolute() || !owner.dir.is_absolute() {
        return Err(fail("codex_home_path"));
    }
    if !fs::symlink_metadata(&owner.dir).is_ok_and(|meta| meta.is_dir()) {
        return Err(fail("codex_home_owner_missing"));
    }
    plain_path(dir)?;

    // Everything is read and checked first, so a refusal leaves the home as it was.
    let owner_config = read_text(
        &owner.dir.join(CONFIG),
        "codex_home_owner_config_unreadable",
    )?
    .unwrap_or_default();
    let home_config =
        read_text(&dir.join(CONFIG), "codex_home_config_unreadable")?.unwrap_or_default();
    let config = config_text(&owner_config, &home_config)?;
    let owner_hooks = owner.dir.join(HOOKS);
    let hooks = if present(&owner_hooks)? {
        let bytes = store::read_bounded(&owner_hooks, MAX_FILE)
            .ok_or_else(|| fail("codex_home_owner_hooks_unreadable"))?;
        Some(hooks_file(&bytes, spec)?)
    } else {
        None
    };
    // The sign-in is only ever looked at from outside (`symlink_metadata`), never opened.
    let owner_auth = owner.dir.join(AUTH);
    let owner_lock_path = owner.dir.join(AUTH_LOCK);
    // The owner's must be a regular file: Codex resolves a link and would replace whatever file
    // it points at, outside `~/.codex`.
    let signed_in = match fs::symlink_metadata(&owner_auth) {
        Ok(meta) if meta.is_file() => true,
        Ok(_) => return Err(fail("codex_home_owner_sign_in_unexpected")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    if signed_in {
        owner_lock(&owner_lock_path)?;
    }
    let auth = signed_in.then_some(owner_auth.as_path());
    let lock = signed_in.then_some(owner_lock_path.as_path());
    sign_in_link(&dir.join(AUTH), auth, false)?;
    sign_in_link(&dir.join(AUTH_LOCK), lock, false)?;
    let owner_sessions = owner.dir.join(SESSIONS);
    let sessions = match fs::symlink_metadata(&owner_sessions) {
        Ok(meta) if meta.is_dir() => Some(owner_sessions),
        Ok(_) => return Err(fail("codex_home_owner_sessions_unexpected")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let own_sessions = fs::symlink_metadata(dir.join(SESSIONS)).is_ok_and(|meta| meta.is_dir());
    if own_sessions && sessions.is_some() {
        return Err(fail("codex_home_sessions_folder"));
    }
    for name in [CONFIG, HOOKS, INSTRUCTIONS] {
        if fs::symlink_metadata(dir.join(name)).is_ok_and(|meta| meta.is_dir()) {
            return Err(fail("codex_home_unexpected_folder"));
        }
    }
    // (name, the owner's entry if there is one, whether the home has a folder of its own there)
    let mut linked = Vec::with_capacity(LINKED.len());
    for name in LINKED {
        let source = owner.dir.join(name);
        let target = present(&source)?.then_some(source);
        let own = fs::symlink_metadata(dir.join(name)).is_ok_and(|meta| meta.is_dir());
        if own && target.is_some() {
            return Err(fail("codex_home_own_folder"));
        }
        linked.push((name, target, own));
    }

    store::private_dir(dir)?;
    store::write_atomic(&dir.join(CONFIG), config.as_bytes())?;
    match hooks {
        Some(bytes) => store::write_atomic(&dir.join(HOOKS), &bytes)?,
        None => place(&dir.join(HOOKS), None)?,
    }
    let instructions = owner.dir.join(INSTRUCTIONS);
    let target = (spec.instructions && present(&instructions)?).then_some(instructions);
    place(&dir.join(INSTRUCTIONS), target.as_deref())?;
    if signed_in {
        // Created once and never replaced: a running Codex holds its lock on this very file.
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&owner_lock_path)
        {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // Checked again here: it may have appeared since the check above.
                owner_lock(&owner_lock_path)?;
            }
            Err(e) => return Err(e),
        }
    }
    sign_in_link(&dir.join(AUTH), auth, true)?;
    sign_in_link(&dir.join(AUTH_LOCK), lock, true)?;
    // A sessions folder Codex made here while the owner had none is the home's own: it stays.
    if !own_sessions {
        place(&dir.join(SESSIONS), sessions.as_deref())?;
    }
    for (name, target, own) in linked {
        if !own {
            place(&dir.join(name), target.as_deref())?;
        }
    }
    Ok(dir.to_path_buf())
}

pub fn write_role(
    owner: &CodexOwner,
    dir: &Path,
    spec: &CodexHomeSpec,
    role: &str,
) -> io::Result<()> {
    let source = owner.dir.join(INSTRUCTIONS);
    let mut bytes = if spec.instructions && fs::metadata(&source).is_ok_and(|m| m.is_file()) {
        let mut bytes = fs::read(source)?;
        bytes.extend_from_slice(b"\n\n");
        bytes
    } else {
        Vec::new()
    };
    bytes.extend_from_slice(role.as_bytes());
    if bytes.len() > 128 * 1024 {
        return Err(fail("arsenal_role_too_long"));
    }
    place(&dir.join(INSTRUCTIONS), None)?;
    store::write_atomic(&dir.join(INSTRUCTIONS), &bytes)
}

/// A hook Codex reads from `config.toml` itself. It runs in every launch: a preset cannot leave
/// it out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigHook {
    pub event: String,
    pub matcher: String,
    pub command: String,
}

/// What a Codex preset can choose from, read from the owner's folder now. Commands are shown so
/// the owner can tell hooks apart; they are the owner's own settings, never a secret store.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CodexOptions {
    /// The hooks of `hooks.json`: each can be left out of a launch by its `key`. Two hooks with
    /// the same event, matcher and command have one key and are listed once.
    pub hooks: Vec<launch_home::HookOption>,
    /// The hooks `config.toml` declares: shown, never offered to leave out.
    pub config_hooks: Vec<ConfigHook>,
    /// Whether the owner has an `AGENTS.md` a preset can leave out.
    pub instructions: bool,
}

/// The hooks a parsed `config.toml` declares under `hooks`, whatever is under `hooks.state`
/// (Codex's record of trust, not hooks) aside. An event whose entries have no readable command is
/// still listed, with an empty one: it is never hidden.
fn config_hooks(config: &toml::Table) -> Vec<ConfigHook> {
    let mut out = Vec::new();
    let Some(toml::Value::Table(hooks)) = config.get("hooks") else {
        return out;
    };
    for (event, groups) in hooks {
        let toml::Value::Array(groups) = groups else {
            continue;
        };
        if event == "state" {
            continue;
        }
        let before = out.len();
        for group in groups {
            let text = |table: &toml::Value, key: &str| {
                table
                    .get(key)
                    .and_then(toml::Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            };
            let matcher = text(group, "matcher");
            for hook in group
                .get("hooks")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
            {
                out.push(ConfigHook {
                    event: event.clone(),
                    matcher: matcher.clone(),
                    command: text(hook, "command"),
                });
            }
        }
        if out.len() == before {
            out.push(ConfigHook {
                event: event.clone(),
                matcher: String::new(),
                command: String::new(),
            });
        }
    }
    out
}

/// What the environment chooser of a Codex preset lists. Never fails: a file that is missing,
/// unreadable or malformed lists nothing (a launch with a home then fails at [`build`], aloud).
pub fn options(owner: &CodexOwner) -> CodexOptions {
    let mut hooks = store::read_bounded(&owner.dir.join(HOOKS), MAX_FILE)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .map(|value| launch_home::hook_options(&value))
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    hooks.retain(|hook| seen.insert(hook.key.clone()));
    let config_hooks = read_text(&owner.dir.join(CONFIG), "unreadable")
        .ok()
        .flatten()
        .and_then(|text| toml::from_str::<toml::Table>(&text).ok())
        .map(|config| config_hooks(&config))
        .unwrap_or_default();
    CodexOptions {
        hooks,
        config_hooks,
        instructions: fs::symlink_metadata(owner.dir.join(INSTRUCTIONS)).is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    const FAKE_AUTH: &str = "FAKE-TOKEN-1";
    const OWNER_CONFIG: &str = "# top comment\nmodel = \"x\"\n\n[hooks.state.\"a:b\"]\ntrusted_hash = \"OWNER\"\n\n# keep\n[projects.\"/x\"]\ntrust_level = \"trusted\"\n";
    const OWNER_HOOKS: &str = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","extra":1,"hooks":[{"type":"command","command":"gate.py","x":2},{"type":"command","command":"rtk","timeout":5}]}],"Stop":[{"hooks":[{"type":"command","command":"bye.sh"}]}]},"other":true}"#;

    struct Fx {
        root: PathBuf,
        owner: CodexOwner,
        home: PathBuf,
    }

    /// A fake owner `.codex` in a fresh temporary folder: never the real one.
    fn fx(name: &str) -> Fx {
        let root =
            std::env::temp_dir().join(format!("launch-home-codex-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let dir = root.join(".codex");
        fs::create_dir_all(dir.join("sessions")).unwrap();
        fs::write(dir.join(CONFIG), OWNER_CONFIG).unwrap();
        fs::write(dir.join(HOOKS), OWNER_HOOKS).unwrap();
        fs::write(dir.join(INSTRUCTIONS), "kernel").unwrap();
        fs::write(dir.join(AUTH), FAKE_AUTH).unwrap();
        let home = root.join("homes/preset");
        Fx {
            root,
            owner: CodexOwner { dir },
            home,
        }
    }

    impl Fx {
        fn build(&self, spec: &CodexHomeSpec) -> io::Result<PathBuf> {
            build(&self.owner, &self.home, spec)
        }
        fn ok(&self) -> PathBuf {
            self.build(&CodexHomeSpec::default()).unwrap()
        }
    }

    fn code(r: io::Result<PathBuf>) -> String {
        r.unwrap_err().to_string()
    }

    #[test]
    fn role_home_preserves_owner_and_returns_to_link() {
        let f = fx("own-role");
        let spec = CodexHomeSpec::default();
        f.build(&spec).unwrap();
        let before = fs::read(f.owner.dir.join(INSTRUCTIONS)).unwrap();
        write_role(&f.owner, &f.home, &spec, "role text").unwrap();
        assert_eq!(
            fs::read(f.home.join(INSTRUCTIONS)).unwrap(),
            b"kernel\n\nrole text"
        );
        assert_eq!(fs::read(f.owner.dir.join(INSTRUCTIONS)).unwrap(), before);
        assert!(
            fs::symlink_metadata(f.owner.dir.join(INSTRUCTIONS))
                .unwrap()
                .is_file()
        );
        f.build(&spec).unwrap();
        assert!(
            fs::symlink_metadata(f.home.join(INSTRUCTIONS))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_dir_all(f.root).unwrap();
    }

    /// Every entry under `dir`: file bytes, `link:<target>` or `dir`.
    fn snapshot(dir: &Path) -> BTreeMap<String, String> {
        fn walk(base: &Path, at: &Path, out: &mut BTreeMap<String, String>) {
            let Ok(entries) = fs::read_dir(at) else {
                return;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let rel = path.strip_prefix(base).unwrap().display().to_string();
                let meta = fs::symlink_metadata(&path).unwrap();
                if meta.file_type().is_symlink() {
                    out.insert(
                        rel,
                        format!("link:{}", fs::read_link(&path).unwrap().display()),
                    );
                } else if meta.is_dir() {
                    out.insert(rel, "dir".to_owned());
                    walk(base, &path, out);
                } else {
                    out.insert(
                        rel,
                        String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn is_link(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    }

    fn gone(path: &Path) -> bool {
        fs::symlink_metadata(path).is_err()
    }

    fn off(spec_keys: &[String]) -> CodexHomeSpec {
        CodexHomeSpec {
            instructions: true,
            hooks_off: spec_keys.to_vec(),
        }
    }

    // 1
    #[test]
    fn builds_from_nothing_with_every_entry_and_private_modes() {
        let f = fx("from-nothing");
        assert_eq!(f.ok(), f.home);
        let o = &f.owner.dir;
        assert_eq!(mode(&f.home), 0o700);
        assert_eq!(mode(&f.home.join(CONFIG)), 0o600);
        assert_eq!(mode(&f.home.join(HOOKS)), 0o600);
        assert!(f.home.join(CONFIG).is_file());
        assert_eq!(fs::read_to_string(f.home.join(HOOKS)).unwrap(), OWNER_HOOKS);
        for (name, target) in [
            (INSTRUCTIONS, o.join(INSTRUCTIONS)),
            (AUTH, o.join(AUTH)),
            (AUTH_LOCK, o.join(AUTH_LOCK)),
            (SESSIONS, o.join(SESSIONS)),
        ] {
            assert!(is_link(&f.home.join(name)), "{name} is a link");
            assert_eq!(fs::read_link(f.home.join(name)).unwrap(), target, "{name}");
        }
    }

    // 2
    #[test]
    fn rebuilding_keeps_bytes_and_links_and_config_does_not_grow() {
        let f = fx("rebuild");
        f.ok();
        let first = snapshot(&f.home);
        let len = fs::metadata(f.home.join(CONFIG)).unwrap().len();
        f.ok();
        f.ok();
        assert_eq!(snapshot(&f.home), first);
        assert_eq!(fs::metadata(f.home.join(CONFIG)).unwrap().len(), len);
    }

    // 3
    #[test]
    fn hooks_off_leaves_out_only_those_hooks() {
        let f = fx("hooks-off");
        let gate = launch_home::hook_key("PreToolUse", "Bash", "gate.py");
        let bye = launch_home::hook_key("Stop", "", "bye.sh");
        f.build(&off(&[gate, bye])).unwrap();
        let v: Value = serde_json::from_slice(&fs::read(f.home.join(HOOKS)).unwrap()).unwrap();
        assert_eq!(v["other"], true, "unknown key of the file kept");
        assert!(v["hooks"].get("Stop").is_none(), "event left empty goes");
        let group = &v["hooks"]["PreToolUse"][0];
        assert_eq!(group["extra"], 1, "unknown key of the group kept");
        assert_eq!(group["matcher"], "Bash");
        let hooks = group["hooks"].as_array().unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0]["command"], "rtk");
        assert_eq!(hooks[0]["timeout"], 5, "unknown key of the hook kept");
    }

    #[test]
    fn a_key_that_names_no_hook_is_refused_by_hooks_file_and_build_and_leaves_the_home_intact() {
        let unknown = "0000000000000000".to_owned();
        let gate = launch_home::hook_key("PreToolUse", "Bash", "gate.py");
        // `hooks_file`: the unknown key alone, and next to a key that does name a hook.
        for keys in [vec![unknown.clone()], vec![gate.clone(), unknown.clone()]] {
            let error = hooks_file(OWNER_HOOKS.as_bytes(), &off(&keys)).unwrap_err();
            assert_eq!(error.to_string(), "codex_home_hook_unknown");
        }
        // Control: the same file with only the key it has is fine.
        assert!(hooks_file(OWNER_HOOKS.as_bytes(), &off(std::slice::from_ref(&gate))).is_ok());

        // `build` over a home that was built before: the refusal leaves every entry as it was.
        let f = fx("hook-unknown");
        f.build(&off(std::slice::from_ref(&gate))).unwrap();
        let before = snapshot(&f.home);
        let error = f.build(&off(&[gate.clone(), unknown.clone()])).unwrap_err();
        assert_eq!(error.to_string(), "codex_home_hook_unknown");
        assert_eq!(snapshot(&f.home), before);
        // And over no home at all: nothing is created.
        let fresh = fx("hook-unknown-fresh");
        assert_eq!(
            code(fresh.build(&off(&[unknown]))),
            "codex_home_hook_unknown"
        );
        assert!(gone(&fresh.home));
    }

    #[test]
    fn hooks_off_drops_a_group_left_without_hooks_but_keeps_the_event() {
        let owner = br#"{"hooks":{"E":[{"matcher":"m","hooks":[{"command":"a"}]},{"matcher":"n","hooks":[{"command":"b"}]}]}}"#;
        let spec = off(&[launch_home::hook_key("E", "m", "a")]);
        let v: Value = serde_json::from_slice(&hooks_file(owner, &spec).unwrap()).unwrap();
        let groups = v["hooks"]["E"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["matcher"], "n");
    }

    #[test]
    fn empty_hooks_off_gives_the_owners_exact_bytes() {
        let f = fx("hooks-same");
        // Odd spacing proves the bytes are copied, not re-serialized.
        let odd = b"{ \"hooks\" :  {}  ,\n\"z\":1 }\n";
        fs::write(f.owner.dir.join(HOOKS), odd).unwrap();
        f.ok();
        assert_eq!(fs::read(f.home.join(HOOKS)).unwrap(), odd);
    }

    // 4
    #[test]
    fn home_hook_trust_survives_and_the_owners_never_enters() {
        let f = fx("state");
        f.ok();
        let first = fs::read_to_string(f.home.join(CONFIG)).unwrap();
        assert!(!first.contains("OWNER") && !first.contains("hooks.state"));
        let with_trust = format!("{first}\n[hooks.state.\"h:1\"]\ntrusted_hash = \"HOME\"\n");
        fs::write(f.home.join(CONFIG), &with_trust).unwrap();
        f.ok();
        let second = fs::read_to_string(f.home.join(CONFIG)).unwrap();
        assert_eq!(second.matches("HOME").count(), 1);
        assert!(!second.contains("OWNER"));
        f.ok();
        assert_eq!(
            fs::read_to_string(f.home.join(CONFIG)).unwrap(),
            second,
            "stable"
        );
    }

    #[test]
    fn owner_comments_and_order_are_kept() {
        let f = fx("comments");
        f.ok();
        let text = fs::read_to_string(f.home.join(CONFIG)).unwrap();
        let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("missing {s}"));
        assert!(
            at("# top comment") < at("model = ")
                && at("model = ") < at("# keep")
                && at("# keep") < at("[projects.")
        );
    }

    // 5
    #[test]
    fn malformed_owner_config_is_refused_and_home_is_untouched() {
        let f = fx("owner-malformed");
        f.ok();
        let before = snapshot(&f.home);
        fs::write(f.owner.dir.join(CONFIG), "this = = broken").unwrap();
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_owner_config_malformed"
        );
        assert_eq!(snapshot(&f.home), before);
    }

    #[test]
    fn malformed_home_config_is_refused_and_left_as_it_was() {
        let f = fx("home-malformed");
        f.ok();
        fs::write(f.home.join(CONFIG), "[oops").unwrap();
        let before = snapshot(&f.home);
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_config_malformed"
        );
        assert_eq!(snapshot(&f.home), before);
    }

    #[test]
    fn owner_config_that_is_a_link_is_refused() {
        let f = fx("owner-link");
        f.ok();
        let before = snapshot(&f.home);
        let real = f.root.join("real.toml");
        fs::write(&real, "a = 1\n").unwrap();
        fs::remove_file(f.owner.dir.join(CONFIG)).unwrap();
        symlink(&real, f.owner.dir.join(CONFIG)).unwrap();
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_owner_config_unreadable"
        );
        assert_eq!(snapshot(&f.home), before);
    }

    #[test]
    fn home_config_that_is_a_link_is_refused() {
        let f = fx("home-link");
        f.ok();
        let real = f.root.join("real.toml");
        fs::write(&real, "a = 1\n").unwrap();
        fs::remove_file(f.home.join(CONFIG)).unwrap();
        symlink(&real, f.home.join(CONFIG)).unwrap();
        let before = snapshot(&f.home);
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_config_unreadable"
        );
        assert_eq!(snapshot(&f.home), before);
        assert_eq!(
            fs::read_to_string(&real).unwrap(),
            "a = 1\n",
            "target untouched"
        );
    }

    #[test]
    fn hooks_state_as_inline_table_or_dotted_keys_is_refused_not_guessed() {
        for (name, owner) in [
            (
                "inline",
                "hooks = { state = { a = { trusted_hash = \"x\" } } }\n",
            ),
            ("dotted", "[hooks]\nstate.a.trusted_hash = \"x\"\n"),
        ] {
            let f = fx(&format!("state-{name}"));
            f.ok();
            let before = snapshot(&f.home);
            fs::write(f.owner.dir.join(CONFIG), owner).unwrap();
            let err = code(f.build(&CodexHomeSpec::default()));
            eprintln!("DOC {name}: {err}");
            assert!(err.starts_with("codex_home_"), "{name}: {err}");
            assert!(
                err.ends_with("unsupported") || err.ends_with("malformed"),
                "{name}: {err}"
            );
            assert_eq!(snapshot(&f.home), before, "{name}: home untouched");
        }
    }

    // 6
    #[test]
    fn sign_in_entries_are_links_and_the_fake_auth_is_never_touched() {
        let f = fx("auth-untouched");
        let auth = f.owner.dir.join(AUTH);
        let past = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        fs::File::options()
            .write(true)
            .open(&auth)
            .unwrap()
            .set_modified(past)
            .unwrap();
        let before = fs::metadata(&auth).unwrap();
        f.ok();
        f.ok();
        let after = fs::metadata(&auth).unwrap();
        assert_eq!(fs::read_to_string(&auth).unwrap(), FAKE_AUTH);
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
        assert_eq!(after.len(), before.len());
        assert_eq!(after.ino(), before.ino());
        assert_eq!(fs::read_link(f.home.join(AUTH)).unwrap(), auth);
        assert_eq!(
            fs::read_link(f.home.join(AUTH_LOCK)).unwrap(),
            f.owner.dir.join(AUTH_LOCK)
        );
        assert!(is_link(&f.home.join(AUTH)));
    }

    #[test]
    fn missing_owner_lock_is_created_empty_0600_once() {
        let f = fx("lock-created");
        let lock = f.owner.dir.join(AUTH_LOCK);
        assert!(gone(&lock));
        f.ok();
        let meta = fs::metadata(&lock).unwrap();
        assert!(meta.is_file() && meta.len() == 0);
        assert_eq!(mode(&lock), 0o600);
        let ino = meta.ino();
        f.ok();
        assert_eq!(fs::metadata(&lock).unwrap().ino(), ino);
    }

    #[test]
    fn existing_owner_lock_keeps_its_inode_and_content() {
        let f = fx("lock-kept");
        let lock = f.owner.dir.join(AUTH_LOCK);
        fs::write(&lock, "held").unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
        let ino = fs::metadata(&lock).unwrap().ino();
        f.ok();
        f.ok();
        assert_eq!(fs::metadata(&lock).unwrap().ino(), ino);
        assert_eq!(fs::read_to_string(&lock).unwrap(), "held");
        assert_eq!(mode(&lock), 0o600);
    }

    // 7
    #[test]
    fn renewal_as_codex_does_it_keeps_the_home_link_and_updates_the_owner_file() {
        let f = fx("renewal");
        f.ok();
        let resolved = fs::canonicalize(f.home.join(AUTH)).unwrap();
        assert_eq!(resolved, fs::canonicalize(f.owner.dir.join(AUTH)).unwrap());
        let tmp = resolved.parent().unwrap().join(".auth.json.tmp-renew");
        fs::write(&tmp, "FAKE-TOKEN-2").unwrap();
        fs::rename(&tmp, &resolved).unwrap();
        assert!(is_link(&f.home.join(AUTH)));
        assert_eq!(
            fs::read_to_string(f.owner.dir.join(AUTH)).unwrap(),
            "FAKE-TOKEN-2"
        );
        assert_eq!(
            fs::read_to_string(f.home.join(AUTH)).unwrap(),
            "FAKE-TOKEN-2"
        );
        f.ok();
        assert!(is_link(&f.home.join(AUTH)));
        assert_eq!(
            fs::read_to_string(f.owner.dir.join(AUTH)).unwrap(),
            "FAKE-TOKEN-2"
        );
    }

    // 8
    #[test]
    fn control_renaming_over_the_home_auth_makes_the_next_build_refuse_without_deleting_it() {
        let f = fx("control");
        f.ok();
        let tmp = f.home.join(".auth.json.tmp");
        fs::write(&tmp, "FAKE-TOKEN-HOME-ONLY").unwrap();
        fs::rename(&tmp, f.home.join(AUTH)).unwrap();
        assert!(!is_link(&f.home.join(AUTH)), "control: now a regular file");
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_sign_in_unexpected"
        );
        assert_eq!(
            fs::read_to_string(f.home.join(AUTH)).unwrap(),
            "FAKE-TOKEN-HOME-ONLY"
        );
        assert_eq!(
            fs::read_to_string(f.owner.dir.join(AUTH)).unwrap(),
            FAKE_AUTH
        );
    }

    #[test]
    fn a_link_to_somewhere_else_is_refused_too() {
        let f = fx("auth-elsewhere");
        f.ok();
        fs::remove_file(f.home.join(AUTH_LOCK)).unwrap();
        let other = f.root.join("other.lock");
        fs::write(&other, "").unwrap();
        symlink(&other, f.home.join(AUTH_LOCK)).unwrap();
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_sign_in_unexpected"
        );
        assert!(is_link(&f.home.join(AUTH_LOCK)));
    }

    // 9
    #[test]
    fn owner_without_auth_gets_no_links_and_no_lock_and_stale_links_go() {
        let f = fx("no-auth");
        f.ok();
        assert!(is_link(&f.home.join(AUTH)));
        fs::remove_file(f.owner.dir.join(AUTH)).unwrap();
        let _ = fs::remove_file(f.owner.dir.join(AUTH_LOCK));
        f.ok();
        assert!(gone(&f.home.join(AUTH)) && gone(&f.home.join(AUTH_LOCK)));
        assert!(gone(&f.owner.dir.join(AUTH_LOCK)), "no lock invented");
    }

    #[test]
    fn owner_without_agents_md_or_with_only_claude_md_gets_no_instructions() {
        let f = fx("no-instructions");
        f.ok();
        assert!(is_link(&f.home.join(INSTRUCTIONS)));
        fs::remove_file(f.owner.dir.join(INSTRUCTIONS)).unwrap();
        f.ok();
        assert!(gone(&f.home.join(INSTRUCTIONS)), "stale link goes");
        fs::write(f.owner.dir.join("CLAUDE.md"), "only claude").unwrap();
        f.ok();
        assert!(
            gone(&f.home.join(INSTRUCTIONS)),
            "not made up from CLAUDE.md"
        );
        assert!(gone(&f.owner.dir.join(INSTRUCTIONS)));
    }

    #[test]
    fn instructions_false_removes_the_link() {
        let f = fx("instructions-off");
        f.ok();
        let spec = CodexHomeSpec {
            instructions: false,
            hooks_off: vec![],
        };
        f.build(&spec).unwrap();
        assert!(gone(&f.home.join(INSTRUCTIONS)));
        assert!(f.owner.dir.join(INSTRUCTIONS).is_file());
    }

    #[test]
    fn owner_without_hooks_json_writes_none_and_removes_a_stale_one() {
        let f = fx("no-hooks");
        f.ok();
        assert!(f.home.join(HOOKS).is_file());
        fs::remove_file(f.owner.dir.join(HOOKS)).unwrap();
        f.ok();
        assert!(gone(&f.home.join(HOOKS)));
    }

    #[test]
    fn owner_without_sessions_gets_no_link_and_the_homes_own_folder_stays() {
        let f = fx("no-sessions");
        f.ok();
        assert!(is_link(&f.home.join(SESSIONS)));
        fs::remove_dir(f.owner.dir.join(SESSIONS)).unwrap();
        f.ok();
        assert!(gone(&f.home.join(SESSIONS)), "stale link goes");
        fs::create_dir(f.home.join(SESSIONS)).unwrap();
        fs::write(f.home.join(SESSIONS).join("rollout"), "mine").unwrap();
        f.ok();
        assert_eq!(
            fs::read_to_string(f.home.join(SESSIONS).join("rollout")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn own_sessions_folder_with_an_owner_sessions_is_refused() {
        let f = fx("sessions-clash");
        f.ok();
        fs::remove_file(f.home.join(SESSIONS)).unwrap();
        fs::create_dir(f.home.join(SESSIONS)).unwrap();
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_sessions_folder"
        );
        assert!(f.home.join(SESSIONS).is_dir());
    }

    #[test]
    fn missing_owner_folder_is_refused_and_nothing_is_created() {
        let f = fx("owner-missing");
        fs::remove_dir_all(&f.owner.dir).unwrap();
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_owner_missing"
        );
        assert!(gone(&f.home));
    }

    #[test]
    fn owner_with_no_config_counts_as_an_empty_one() {
        let f = fx("no-config");
        fs::remove_file(f.owner.dir.join(CONFIG)).unwrap();
        f.ok();
        assert_eq!(fs::read_to_string(f.home.join(CONFIG)).unwrap(), "");
    }

    // 10
    #[test]
    fn invalid_specs_are_refused() {
        let good = launch_home::hook_key("E", "m", "c");
        assert!(off(std::slice::from_ref(&good)).validate().is_ok());
        assert_eq!(off(&["nothex".to_owned()]).validate(), Err("home_invalid"));
        assert_eq!(
            off(&["ABCDEF0123456789".to_owned()]).validate(),
            Err("home_invalid")
        );
        assert_eq!(off(&[good.clone(), good]).validate(), Err("home_invalid"));
        let many = |n: usize| (0..n).map(|i| format!("{i:016x}")).collect::<Vec<_>>();
        assert!(off(&many(MAX_NAMES)).validate().is_ok());
        assert_eq!(off(&many(MAX_NAMES + 1)).validate(), Err("home_invalid"));
        assert!(
            serde_json::from_str::<CodexHomeSpec>(r#"{"instructions":true,"nope":1}"#).is_err()
        );
        assert_eq!(
            serde_json::from_str::<CodexHomeSpec>("{}").unwrap(),
            CodexHomeSpec::default()
        );
    }

    #[test]
    fn build_refuses_an_invalid_spec_before_writing() {
        let f = fx("bad-spec");
        assert_eq!(code(f.build(&off(&["zz".to_owned()]))), "home_invalid");
        assert!(gone(&f.home));
    }

    #[test]
    fn relative_paths_are_refused() {
        let f = fx("relative");
        assert_eq!(
            code(build(
                &f.owner,
                Path::new("rel/home"),
                &CodexHomeSpec::default()
            )),
            "codex_home_path"
        );
    }

    // 11
    #[test]
    fn home_path_that_is_a_link_is_refused_and_its_target_untouched() {
        let f = fx("home-is-link");
        let real = f.root.join("real");
        fs::create_dir_all(&real).unwrap();
        fs::create_dir_all(f.home.parent().unwrap()).unwrap();
        symlink(&real, &f.home).unwrap();
        assert!(f.build(&CodexHomeSpec::default()).is_err());
        assert!(snapshot(&real).is_empty());
        assert!(is_link(&f.home));
    }

    #[test]
    fn a_folder_where_a_file_goes_is_refused_and_never_removed() {
        for name in [CONFIG, HOOKS, INSTRUCTIONS] {
            let f = fx(&format!("folder-{name}"));
            fs::create_dir_all(f.home.join(name)).unwrap();
            fs::write(f.home.join(name).join("keep"), "x").unwrap();
            assert!(f.build(&CodexHomeSpec::default()).is_err(), "{name}");
            assert_eq!(
                fs::read_to_string(f.home.join(name).join("keep")).unwrap(),
                "x",
                "{name}"
            );
        }
    }

    // 12
    #[test]
    fn an_existing_ancestor_that_is_a_link_is_refused_and_nothing_is_written_through_it() {
        let f = fx("ancestor-link");
        let real = f.root.join("real-target");
        fs::create_dir_all(&real).unwrap();
        fs::create_dir_all(f.root.join("launch-homes")).unwrap();
        symlink(&real, f.root.join("launch-homes/codex")).unwrap();
        let home = f.root.join("launch-homes/codex/preset");
        let r = build(&f.owner, &home, &CodexHomeSpec::default());
        assert_eq!(code(r), "codex_home_path_link");
        assert!(snapshot(&real).is_empty(), "nothing written at the target");
        assert!(is_link(&f.root.join("launch-homes/codex")));
        assert!(gone(&real.join("preset")));
        assert!(gone(&f.owner.dir.join(AUTH_LOCK)), "owner lock not made");
    }

    // 13
    #[test]
    fn a_home_path_with_parent_dir_after_existing_parts_is_refused_and_nothing_is_created() {
        let f = fx("dotdot");
        fs::create_dir_all(f.root.join("homes")).unwrap();
        let home = f.root.join("homes/../escaped/preset");
        assert_eq!(
            code(build(&f.owner, &home, &CodexHomeSpec::default())),
            "codex_home_path"
        );
        assert!(gone(&f.root.join("escaped")));
    }

    // 13b
    #[test]
    fn a_home_path_with_parent_dir_after_a_missing_part_is_refused() {
        let f = fx("dotdot-missing");
        let home = f.root.join("missing/../escaped/preset");
        assert_eq!(
            code(build(&f.owner, &home, &CodexHomeSpec::default())),
            "codex_home_path"
        );
        assert!(gone(&f.root.join("escaped")));
    }

    // 14
    // `Path::components` drops a `.` in the middle or at the end of an absolute path, so a
    // `CurDir` part never reaches `plain_path` there: the path is normalized, not refused. This
    // pins what the code does today; the `CurDir` arm of `plain_path` is unreachable from `build`.
    #[test]
    fn a_dot_part_in_an_absolute_home_path_is_normalized_away_by_components() {
        let f = fx("dot");
        let home = f.root.join("homes/./preset");
        assert!(
            home.components()
                .all(|c| !matches!(c, std::path::Component::CurDir))
        );
        assert_eq!(
            build(&f.owner, &home, &CodexHomeSpec::default()).unwrap(),
            home
        );
        assert!(f.root.join("homes/preset").is_dir());
    }

    // 15
    #[test]
    fn owner_auth_that_is_a_link_or_a_folder_is_refused_without_opening_it() {
        for kind in ["link", "dir", "dangling"] {
            let f = fx(&format!("auth-{kind}"));
            let auth = f.owner.dir.join(AUTH);
            fs::remove_file(&auth).unwrap();
            let real = f.root.join("elsewhere-auth");
            match kind {
                "link" => {
                    fs::write(&real, "FAKE-TOKEN-OTHER").unwrap();
                    // Unreadable target: opening through the link would fail, not give this code.
                    fs::set_permissions(&real, fs::Permissions::from_mode(0o000)).unwrap();
                    symlink(&real, &auth).unwrap();
                }
                "dir" => fs::create_dir(&auth).unwrap(),
                _ => symlink(f.root.join("nowhere"), &auth).unwrap(),
            }
            assert_eq!(
                code(f.build(&CodexHomeSpec::default())),
                "codex_home_owner_sign_in_unexpected",
                "{kind}"
            );
            assert!(gone(&f.home), "{kind}: nothing written");
            assert!(gone(&f.owner.dir.join(AUTH_LOCK)), "{kind}: no lock made");
            if kind == "link" {
                assert_eq!(fs::read_link(&auth).unwrap(), real);
                fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
                assert_eq!(fs::read_to_string(&real).unwrap(), "FAKE-TOKEN-OTHER");
            }
        }
    }

    // 16
    #[test]
    fn owner_sessions_that_is_a_link_or_a_file_is_refused_and_nothing_is_written() {
        for kind in ["link", "file"] {
            let f = fx(&format!("sessions-{kind}"));
            let sessions = f.owner.dir.join(SESSIONS);
            fs::remove_dir(&sessions).unwrap();
            if kind == "link" {
                let real = f.root.join("other-sessions");
                fs::create_dir_all(&real).unwrap();
                symlink(&real, &sessions).unwrap();
            } else {
                fs::write(&sessions, "not a folder").unwrap();
            }
            assert_eq!(
                code(f.build(&CodexHomeSpec::default())),
                "codex_home_owner_sessions_unexpected",
                "{kind}"
            );
            assert!(gone(&f.home), "{kind}: home not created");
            assert!(gone(&f.owner.dir.join(AUTH_LOCK)), "{kind}: no lock made");
        }
    }

    // 17
    #[test]
    fn owner_lock_that_group_or_other_can_write_is_refused_and_left_exactly_as_it_was() {
        for lock_mode in [0o620, 0o602, 0o660, 0o606, 0o666] {
            let f = fx(&format!("lock-mode-{lock_mode:o}"));
            let lock = f.owner.dir.join(AUTH_LOCK);
            fs::write(&lock, "held").unwrap();
            fs::set_permissions(&lock, fs::Permissions::from_mode(lock_mode)).unwrap();
            let before = fs::metadata(&lock).unwrap();
            assert_eq!(
                code(f.build(&CodexHomeSpec::default())),
                "codex_home_owner_lock_unexpected",
                "{lock_mode:o}"
            );
            let after = fs::metadata(&lock).unwrap();
            assert_eq!(after.ino(), before.ino(), "{lock_mode:o}: same inode");
            assert_eq!(after.len(), before.len());
            assert_eq!(after.modified().unwrap(), before.modified().unwrap());
            assert_eq!(mode(&lock), lock_mode, "mode not tightened");
            assert_eq!(fs::read_to_string(&lock).unwrap(), "held");
            assert!(gone(&f.home), "{lock_mode:o}: home not created");
        }
    }

    // 18
    #[test]
    fn owner_lock_that_is_a_folder_or_a_link_is_refused_and_not_modified() {
        // folder
        let f = fx("lock-dir");
        let lock = f.owner.dir.join(AUTH_LOCK);
        fs::create_dir(&lock).unwrap();
        fs::write(lock.join("keep"), "x").unwrap();
        let ino = fs::metadata(&lock).unwrap().ino();
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_owner_lock_unexpected"
        );
        assert_eq!(fs::metadata(&lock).unwrap().ino(), ino);
        assert_eq!(fs::read_to_string(lock.join("keep")).unwrap(), "x");
        assert!(gone(&f.home));

        // link to a private file of the user: still a link, so refused
        let f = fx("lock-link");
        let lock = f.owner.dir.join(AUTH_LOCK);
        let real = f.root.join("real.lock");
        fs::write(&real, "held").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
        let ino = fs::metadata(&real).unwrap().ino();
        symlink(&real, &lock).unwrap();
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_owner_lock_unexpected"
        );
        assert_eq!(fs::read_link(&lock).unwrap(), real);
        assert_eq!(fs::metadata(&real).unwrap().ino(), ino);
        assert_eq!(fs::read_to_string(&real).unwrap(), "held");
        assert_eq!(mode(&real), 0o600);
        assert!(gone(&f.home));

        // dangling link: not created through, not replaced
        let f = fx("lock-dangling");
        let lock = f.owner.dir.join(AUTH_LOCK);
        let missing = f.root.join("missing.lock");
        symlink(&missing, &lock).unwrap();
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_owner_lock_unexpected"
        );
        assert_eq!(fs::read_link(&lock).unwrap(), missing);
        assert!(gone(&missing), "nothing created through the link");
    }

    // 19
    #[test]
    fn owner_lock_that_nobody_else_can_write_is_accepted_and_not_modified() {
        // 0644 is what Codex 0.169.0 itself creates (no mode on open, usual umask).
        for lock_mode in [0o600, 0o640, 0o644] {
            let f = fx(&format!("lock-ok-{lock_mode:o}"));
            let lock = f.owner.dir.join(AUTH_LOCK);
            fs::write(&lock, "held").unwrap();
            fs::set_permissions(&lock, fs::Permissions::from_mode(lock_mode)).unwrap();
            let before = fs::metadata(&lock).unwrap();
            f.ok();
            let after = fs::metadata(&lock).unwrap();
            assert_eq!(after.ino(), before.ino());
            assert_eq!(after.modified().unwrap(), before.modified().unwrap());
            assert_eq!(mode(&lock), lock_mode, "mode not changed");
            assert_eq!(fs::read_to_string(&lock).unwrap(), "held");
            assert_eq!(fs::read_link(f.home.join(AUTH_LOCK)).unwrap(), lock);
        }
    }

    /// `chmod 000` only blocks a user that is not root (and has no `CAP_DAC_OVERRIDE`).
    fn mode_000_blocks_opening(path: &Path) -> bool {
        fs::File::open(path).is_err()
    }

    // 20
    #[test]
    fn control_a_0o000_owner_auth_does_not_stop_the_build_because_it_is_never_opened() {
        let f = fx("auth-0o000");
        let auth = f.owner.dir.join(AUTH);
        fs::set_permissions(&auth, fs::Permissions::from_mode(0o000)).unwrap();
        if !mode_000_blocks_opening(&auth) {
            fs::set_permissions(&auth, fs::Permissions::from_mode(0o600)).unwrap();
            eprintln!("SKIPPED: chmod 000 does not block opening here (running as root?)");
            return;
        }
        let built = f.build(&CodexHomeSpec::default());
        let linked = fs::read_link(f.home.join(AUTH)).ok();
        let again = f.build(&CodexHomeSpec::default());
        // Restore before asserting so a failure leaves nothing unreadable behind.
        fs::set_permissions(&auth, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(built.unwrap(), f.home);
        assert_eq!(linked.as_deref(), Some(auth.as_path()));
        again.unwrap();
        assert_eq!(fs::read_to_string(&auth).unwrap(), FAKE_AUTH);
    }

    // 21
    #[test]
    fn control_negative_opening_a_0o000_file_fails_with_permission_denied() {
        let root =
            std::env::temp_dir().join(format!("launch-home-codex-neg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let file = root.join("auth.json");
        fs::write(&file, FAKE_AUTH).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
        let opened = fs::File::open(&file);
        let read = fs::read(&file);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        if opened.is_ok() {
            eprintln!("SKIPPED: chmod 000 does not block opening here (running as root?)");
            return;
        }
        assert_eq!(opened.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(read.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        // The metadata path the build uses still works on the same file.
        assert!(fs::symlink_metadata(&file).is_ok());
    }

    const UNLINKED: [&str; 8] = [
        "state_5.sqlite",
        "state_5.sqlite-wal",
        "logs_2.sqlite",
        "cache",
        "packages",
        "shell_snapshots",
        "models_cache.json",
        "version.json",
    ];

    /// Gives the fake owner every linked entry and everything Codex writes for itself.
    fn fill_owner(f: &Fx) {
        for name in LINKED {
            if name == "hooks" {
                fs::create_dir_all(f.owner.dir.join(name)).unwrap();
                fs::write(f.owner.dir.join(name).join("gate.py"), "x").unwrap();
            } else {
                fs::create_dir_all(f.owner.dir.join(name)).unwrap();
            }
        }
        for name in UNLINKED {
            let path = f.owner.dir.join(name);
            if matches!(name, "cache" | "packages" | "shell_snapshots") {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::write(&path, "x").unwrap();
            }
        }
    }

    #[test]
    fn every_linked_entry_the_owner_has_is_linked_and_what_codex_writes_for_itself_is_not() {
        let f = fx("linked");
        fill_owner(&f);
        let home = f.ok();
        for name in LINKED {
            let meta = fs::symlink_metadata(home.join(name)).unwrap();
            assert!(meta.file_type().is_symlink(), "{name}");
            assert_eq!(
                fs::read_link(home.join(name)).unwrap(),
                f.owner.dir.join(name)
            );
        }
        for name in UNLINKED {
            assert!(fs::symlink_metadata(home.join(name)).is_err(), "{name}");
        }
        // The gate script is reached through the link of `hooks`.
        assert_eq!(fs::read_to_string(home.join("hooks/gate.py")).unwrap(), "x");
        // Control: with an owner that has none of them, none is linked.
        let bare = fx("linked-bare");
        let home = bare.ok();
        for name in LINKED {
            assert!(fs::symlink_metadata(home.join(name)).is_err(), "{name}");
        }
    }

    #[test]
    fn a_folder_of_the_homes_own_under_a_linked_name_refuses_the_build_and_leaves_the_home_intact()
    {
        let f = fx("own-folder");
        f.ok();
        fs::create_dir_all(f.home.join("rules")).unwrap();
        fs::write(f.home.join("rules/mine.md"), "mine").unwrap();
        fs::create_dir_all(f.owner.dir.join("rules")).unwrap();
        let before = snapshot(&f.home);
        assert_eq!(
            code(f.build(&CodexHomeSpec::default())),
            "codex_home_own_folder"
        );
        assert_eq!(snapshot(&f.home), before);
        assert!(f.home.join("rules").is_dir());
        assert!(
            !fs::symlink_metadata(f.home.join("rules"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn the_homes_own_folder_stays_when_the_owner_has_no_such_entry() {
        let f = fx("own-folder-stays");
        f.ok();
        fs::create_dir_all(f.home.join("rules")).unwrap();
        fs::write(f.home.join("rules/mine.md"), "mine").unwrap();
        // The owner has no `rules`: the build succeeds twice and the folder is untouched.
        f.ok();
        f.ok();
        assert_eq!(
            fs::read_to_string(f.home.join("rules/mine.md")).unwrap(),
            "mine"
        );
        assert!(
            !fs::symlink_metadata(f.home.join("rules"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn the_options_use_the_hook_keys_of_claude_and_list_a_repeated_hook_once() {
        let f = fx("options");
        fs::write(
            f.owner.dir.join(HOOKS),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"gate.py"},{"type":"command","command":"gate.py"}]},{"matcher":"Bash","hooks":[{"type":"command","command":"gate.py"}]}],"Stop":[{"hooks":[{"type":"command","command":"bye.sh"}]}]}}"#,
        )
        .unwrap();
        let options = options(&f.owner);
        let keys: Vec<(&str, &str, &str, &str)> = options
            .hooks
            .iter()
            .map(|h| {
                (
                    h.key.as_str(),
                    h.event.as_str(),
                    h.matcher.as_str(),
                    h.command.as_str(),
                )
            })
            .collect();
        let gate = launch_home::hook_key("PreToolUse", "Bash", "gate.py");
        let bye = launch_home::hook_key("Stop", "", "bye.sh");
        assert_eq!(
            keys,
            vec![
                (gate.as_str(), "PreToolUse", "Bash", "gate.py"),
                (bye.as_str(), "Stop", "", "bye.sh"),
            ]
        );
        assert!(options.instructions);
        // Every listed key is one a spec accepts.
        let spec = CodexHomeSpec {
            instructions: true,
            hooks_off: options.hooks.iter().map(|h| h.key.clone()).collect(),
        };
        assert_eq!(spec.validate(), Ok(()));
        fs::remove_file(f.owner.dir.join(INSTRUCTIONS)).unwrap();
        assert!(!super::options(&f.owner).instructions);
    }

    #[test]
    fn the_hooks_config_toml_declares_are_listed_apart_and_never_the_trust_state() {
        let f = fx("config-hooks");
        fs::write(
            f.owner.dir.join(CONFIG),
            "[[hooks.PreToolUse]]
matcher = \"Bash\"
[[hooks.PreToolUse.hooks]]
type = \"command\"
command = \"cfg.sh\"
\n[[hooks.Stop]]
\n[hooks.state.\"a:b\"]
trusted_hash = \"OWNER\"
",
        )
        .unwrap();
        let options = options(&f.owner);
        assert_eq!(
            options.config_hooks,
            vec![
                ConfigHook {
                    event: "PreToolUse".into(),
                    matcher: "Bash".into(),
                    command: "cfg.sh".into()
                },
                // An event with no readable command is still listed, never hidden.
                ConfigHook {
                    event: "Stop".into(),
                    matcher: String::new(),
                    command: String::new()
                },
            ]
        );
        assert!(options.config_hooks.iter().all(|h| h.event != "state"));
        // They are not in the list of hooks a preset can leave out.
        assert!(options.hooks.iter().all(|h| h.command != "cfg.sh"));
        // `state` as an array is no hook either.
        fs::write(
            f.owner.dir.join(CONFIG),
            "[[hooks.state]]\ntrusted_hash = \"x\"\n",
        )
        .unwrap();
        assert!(super::options(&f.owner).config_hooks.is_empty());
    }

    #[test]
    fn broken_owner_files_list_nothing() {
        let f = fx("options-broken");
        fs::write(f.owner.dir.join(HOOKS), "{not json").unwrap();
        fs::write(f.owner.dir.join(CONFIG), "= not toml [").unwrap();
        let options = options(&f.owner);
        assert!(options.hooks.is_empty());
        assert!(options.config_hooks.is_empty());
        // And with no files at all.
        fs::remove_file(f.owner.dir.join(HOOKS)).unwrap();
        fs::remove_file(f.owner.dir.join(CONFIG)).unwrap();
        let options = super::options(&f.owner);
        assert!(options.hooks.is_empty() && options.config_hooks.is_empty());
        // Control: the fixture's own files do list their hooks.
        let ok = fx("options-ok");
        assert_eq!(options_len(&ok), 3);
    }

    fn options_len(f: &Fx) -> usize {
        options(&f.owner).hooks.len()
    }


}
