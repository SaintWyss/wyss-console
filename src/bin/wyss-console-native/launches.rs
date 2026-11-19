//! The owner's launch files: `$XDG_CONFIG_HOME/wyss-console/launch/*.toml`, one per
//! launch, read by this window the way `src/launch.rs` reads them for the Console. The
//! Console's own host has no operation for them (they are a desktop feature), so the window reads
//! the folder itself, read-only: it never seeds the shipped defaults and never writes a file.
//!
//! A launch either opens a terminal in a folder and types its commands (`Kind::Commands`), or
//! starts an agent as a Native session with the permission it names (`Kind::Session`). The TOML
//! subset the files use is parsed here (strings, arrays of strings, booleans, `[table]` headers) so
//! the window needs no TOML crate; anything outside that subset makes the file "not a launch file".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wyss_console::launch::{MAX_BEFORE, MAX_COMMAND, MAX_FILES, MAX_NAME, command_ok, commands_ok, id_ok, read_one, text_ok};

pub use wyss_console::launch::System;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
    Codex,
}

impl Agent {
    pub fn id(&self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude_code",
            Agent::Codex => "codex",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Agent::ClaudeCode => "Claude Code",
            Agent::Codex => "Codex",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Ask,
    ReadOnly,
    Edits,
    Bypass,
}

impl Permission {
    fn from_word(w: &str) -> Option<Permission> {
        Some(match w {
            "ask" => Permission::Ask,
            "read_only" => Permission::ReadOnly,
            "edits" => Permission::Edits,
            "bypass" => Permission::Bypass,
            _ => return None,
        })
    }

    /// The host profile sent in preview and start requests for this permission.
    pub fn profile_id(self) -> &'static str {
        match self {
            Permission::Ask => "template-strict",
            Permission::ReadOnly => "template-review",
            Permission::Edits => "template-edits",
            Permission::Bypass => "template-full-access",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A new terminal in the folder with these commands typed, each followed by Enter.
    Commands { commands: Vec<String> },
    Session { agent: Agent, permission: Option<Permission>, before: Vec<String>, preset: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    /// The file's stem.
    pub id: String,
    pub name: String,
    /// As the file writes it: `~`, `~/…` or absolute.
    pub directory: String,
    pub kind: Kind,
}

impl Launch {
    /// The type shown next to the name: `Terminal` or the agent.
    pub fn type_name(&self) -> &'static str {
        match &self.kind {
            Kind::Commands { .. } => "Terminal",
            Kind::Session { agent, .. } => agent.name(),
        }
    }

    /// Whether a session requests bypass permission.
    pub fn full_access(&self) -> bool {
        matches!(&self.kind, Kind::Session { permission: Some(Permission::Bypass), .. })
    }
}

// ---- the TOML subset ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Bool(bool),
    Arr(Vec<Value>),
}

type Table = BTreeMap<String, Value>;

/// `root` is the keys before any header; `tables` maps a header (`session`, `distro.cachyos`) to its keys.
#[derive(Debug, Default)]
pub struct Doc {
    pub root: Table,
    pub tables: BTreeMap<String, Table>,
}

struct Cursor<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    /// Spaces and tabs only.
    fn blanks(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i += 1;
        }
    }

    /// Blanks, newlines and comments (inside an array).
    fn space(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.i += 1,
                Some(b'#') => self.skip_line(),
                _ => return,
            }
        }
    }

    fn skip_line(&mut self) {
        while !matches!(self.peek(), None | Some(b'\n')) {
            self.i += 1;
        }
    }

    fn string(&mut self) -> Result<String, String> {
        let quote = self.peek().ok_or("a string was expected")?;
        self.i += 1;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.i..];
            let c = rest.chars().next().ok_or("a string is not closed")?;
            self.i += c.len_utf8();
            if c == quote as char {
                return Ok(out);
            }
            if c == '\n' {
                return Err("a string is not closed on its line".into());
            }
            if c == '\\' && quote == b'"' {
                let e = self.text[self.i..].chars().next().ok_or("a string is not closed")?;
                self.i += e.len_utf8();
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    'u' | 'U' => {
                        let n = if e == 'u' { 4 } else { 8 };
                        let hex = self.text.get(self.i..self.i + n).ok_or("a bad \\u escape")?;
                        out.push(u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).ok_or("a bad \\u escape")?);
                        self.i += n;
                    }
                    _ => return Err("an escape this reader does not know".into()),
                }
            } else {
                out.push(c);
            }
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some(b'"') | Some(b'\'') => {
                if self.text[self.i..].starts_with("\"\"\"") || self.text[self.i..].starts_with("'''") {
                    return Err("multi-line strings are not used in launch files".into());
                }
                Ok(Value::Str(self.string()?))
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.space();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                        return Ok(Value::Arr(items));
                    }
                    items.push(self.value()?);
                    self.space();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {}
                        _ => return Err("an array is not closed".into()),
                    }
                }
            }
            _ => {
                let rest = &self.text[self.i..];
                let word: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                self.i += word.len();
                match word.as_str() {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    _ => Err("a value this reader does not know".into()),
                }
            }
        }
    }

    fn key(&mut self) -> Result<String, String> {
        if matches!(self.peek(), Some(b'"') | Some(b'\'')) {
            return self.string();
        }
        let rest = &self.text[self.i..];
        let word: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.').collect();
        if word.is_empty() {
            return Err("a key was expected".into());
        }
        self.i += word.len();
        Ok(word)
    }
}

pub fn parse_toml(text: &str) -> Result<Doc, String> {
    let mut c = Cursor { s: text.as_bytes(), text, i: 0 };
    let mut doc = Doc::default();
    let mut current: Option<String> = None;
    loop {
        c.space();
        match c.peek() {
            None => return Ok(doc),
            Some(b'[') => {
                if c.s.get(c.i + 1) == Some(&b'[') {
                    return Err("arrays of tables are not used in launch files".into());
                }
                c.i += 1;
                c.blanks();
                let name = c.key()?;
                c.blanks();
                if c.peek() != Some(b']') {
                    return Err("a table header is not closed".into());
                }
                c.i += 1;
                if doc.tables.contains_key(&name) {
                    return Err(format!("table [{name}] appears twice"));
                }
                doc.tables.insert(name.clone(), Table::new());
                current = Some(name);
            }
            Some(_) => {
                let key = c.key()?;
                c.blanks();
                if c.peek() != Some(b'=') {
                    return Err(format!("`{key}` has no value"));
                }
                c.i += 1;
                c.blanks();
                let value = c.value()?;
                c.blanks();
                match c.peek() {
                    None | Some(b'\n') | Some(b'\r') | Some(b'#') => {}
                    _ => return Err(format!("something follows the value of `{key}`")),
                }
                let table = match &current {
                    Some(t) => doc.tables.get_mut(t).expect("inserted with its header"),
                    None => &mut doc.root,
                };
                if table.insert(key.clone(), value).is_some() {
                    return Err(format!("`{key}` appears twice"));
                }
            }
        }
    }
}

// ---- a launch file -----------------------------------------------------------------------

fn strings(t: &Table, key: &str) -> Result<Vec<String>, String> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Arr(items)) => items.iter().map(|v| if let Value::Str(s) = v { Ok(s.clone()) } else { Err(format!("`{key}` is a list of strings")) }).collect(),
        Some(_) => Err(format!("`{key}` is a list of strings")),
    }
}

fn string(t: &Table, key: &str) -> Result<Option<String>, String> {
    match t.get(key) {
        None => Ok(None),
        Some(Value::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("`{key}` is a string")),
    }
}

fn only(t: &Table, allowed: &[&str], what: &str) -> Result<(), String> {
    match t.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("not a launch file: unknown field `{k}` in {what}")),
        None => Ok(()),
    }
}

/// `src/launch.rs::parse`, for the subset of its rules this window needs.
pub fn parse(id: &str, text: &str, system: &System) -> Result<Launch, String> {
    let doc = parse_toml(text).map_err(|e| format!("not a launch file: {e}"))?;
    only(&doc.root, &["name", "directory", "commands", "before", "template"], "the file")?;
    for table in doc.tables.keys() {
        if table != "session" && !table.starts_with("distro.") {
            return Err(format!("not a launch file: unknown table [{table}]"));
        }
    }
    let name = string(&doc.root, "name")?.ok_or("not a launch file: missing field `name`")?;
    if !text_ok(&name, MAX_NAME) {
        return Err(format!("name must be 1–{MAX_NAME} characters on one line"));
    }
    let directory = string(&doc.root, "directory")?.unwrap_or_else(|| "~".into());
    if !(directory == "~" || directory.starts_with("~/") || directory.starts_with('/')) {
        return Err("directory must be ~, ~/… or an absolute path".into());
    }
    let own = strings(&doc.root, "commands")?;
    let before = strings(&doc.root, "before")?;
    let template = string(&doc.root, "template")?;
    let distros: Vec<(&str, &Table)> = doc.tables.iter().filter_map(|(k, v)| k.strip_prefix("distro.").map(|d| (d, v))).collect();
    let session = doc.tables.get("session");
    let runs = !own.is_empty() || !distros.is_empty();
    if session.is_some() && (!distros.is_empty() || template.is_some()) {
        return Err("distro and template belong to a commands launch".into());
    }
    if template.as_deref().is_some_and(|t| !id_ok(t)) {
        return Err("template must be a short lowercase name".into());
    }
    let kind = match (session, runs) {
        (Some(_), true) => return Err("a launch has commands or a session, not both".into()),
        (None, false) => return Err("a launch needs commands or a session".into()),
        (None, true) => {
            if !before.is_empty() {
                return Err("before belongs to a session; put the commands in order instead".into());
            }
            commands_ok(&own)?;
            let mut variants: Vec<(&str, Vec<String>)> = Vec::new();
            for (d, t) in &distros {
                only(t, &["commands", "options", "manual"], &format!("[distro.{d}]"))?;
                if !id_ok(d) {
                    return Err(format!("distro.{d}: an os-release ID is lowercase letters, digits, - or _"));
                }
                let commands = strings(t, "commands")?;
                if commands.is_empty() {
                    return Err(format!("distro.{d} needs at least one command"));
                }
                commands_ok(&commands).map_err(|e| format!("distro.{d}: {e}"))?;
                variants.push((d, commands));
            }
            let matched = system.chain().into_iter().find_map(|id| variants.iter().find(|(d, _)| *d == id));
            Kind::Commands { commands: matched.map(|(_, c)| c.clone()).unwrap_or(own) }
        }
        (Some(session), false) => {
            only(session, &["agent", "permission", "profile", "preset"], "[session]")?;
            let agent = match string(session, "agent")?.as_deref() {
                Some("claude_code") => Agent::ClaudeCode,
                Some("codex") => Agent::Codex,
                _ => return Err("session agent must be claude_code or codex".into()),
            };
            let permission = wyss_console::launch::session_permission(
                agent.id(), string(session, "permission")?.as_deref(), string(session, "profile")?.as_deref(),
            )?.as_deref().and_then(Permission::from_word);
            if before.len() > MAX_BEFORE || !before.iter().all(|c| command_ok(c)) {
                return Err(format!("at most {MAX_BEFORE} before commands, each one line of at most {MAX_COMMAND} characters"));
            }
            let preset = string(session, "preset")?;
            if preset.as_deref().is_some_and(|p| !text_ok(p, MAX_NAME)) {
                return Err(format!("preset must be a preset's name, 1–{MAX_NAME} characters on one line"));
            }
            Kind::Session { agent, permission, before, preset }
        }
    };
    Ok(Launch { id: id.to_owned(), name: name.trim().to_owned(), directory, kind })
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub launches: Vec<Launch>,
    /// `(file stem, why)` for each file that is not a launch.
    pub refused: Vec<(String, String)>,
}

/// Every `*.toml` of the folder, as the Console lists them: by name (case-insensitive), at most
/// `MAX_FILES` files, bounded plain files. Read-only; a folder that does not
/// exist is an empty list (this window never creates it).
pub fn read_folder(folder: &Path, system: &System) -> Listing {
    let mut files: Vec<(String, PathBuf)> = std::fs::read_dir(folder)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".toml").map(|s| (s.to_owned(), e.path()))).collect())
        .unwrap_or_default();
    files.sort();
    let mut out = Listing::default();
    for (n, (id, path)) in files.into_iter().enumerate() {
        if n >= MAX_FILES {
            out.refused.push((id, format!("only the first {MAX_FILES} files are read")));
        } else if !id_ok(&id) {
            out.refused.push((id, "file name must be lowercase letters, digits, - or _".into()));
        } else {
            match read_one(&path).and_then(|t| parse(&id, &t, system)) {
                Ok(l) => out.launches.push(l),
                Err(e) => out.refused.push((id, e)),
            }
        }
    }
    out.launches.sort_by_key(|l| l.name.to_lowercase());
    out
}

/// One launch by its id, read again from its file.
#[cfg(test)]
pub fn get(folder: &Path, id: &str, system: &System) -> Result<Launch, String> {
    if !id_ok(id) {
        return Err("unknown launch".into());
    }
    parse(id, &read_one(&folder.join(format!("{id}.toml")))?, system)
}

/// [`get`], with the fingerprint of the very text it was parsed from (`launch::fingerprint`).
pub fn get_with_fingerprint(folder: &Path, id: &str, system: &System) -> Result<(Launch, String), String> {
    if !id_ok(id) {
        return Err("unknown launch".into());
    }
    let text = read_one(&folder.join(format!("{id}.toml")))?;
    Ok((parse(id, &text, system)?, wyss_console::launch::fingerprint(&text)))
}

/// `$XDG_CONFIG_HOME/wyss-console/launch`, or under `~/.config` when it is unset.
pub fn folder() -> Option<PathBuf> {
    crate::prefs::path().map(|p| p.with_file_name("launch"))
}

/// `~/x` for the footer of the menu (`display` of `launch.rs`).
pub fn display(path: &Path, home: &str) -> String {
    wyss_console::launch::display(path, Path::new(home))
}

/// The folder a launch opens in: `~` is the owner's home as the host lists it.
pub fn resolve_directory(spec: &str, home: &str) -> Result<String, &'static str> {
    let path = match spec {
        "~" => home.to_owned(),
        s if s.starts_with("~/") => format!("{}/{}", home.trim_end_matches('/'), &s[2..]),
        s if s.starts_with('/') => s.to_owned(),
        _ => return Err("directory must be ~, ~/… or an absolute path"),
    };
    if Path::new(&path).is_dir() {
        Ok(path)
    } else {
        Err("directory not found")
    }
}

/// What typing a commands launch sends: each command followed by Enter.
pub fn typed(commands: &[String]) -> Vec<u8> {
    commands.iter().flat_map(|c| format!("{c}\r").into_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = r#"# Starts Claude Code as a Native session with the permission named here:
name = "Claude Code"
directory = "~"
before = [
  "printf \"%s\\n\" 'fixture'",
]

[session]
agent = "claude_code"
permission = "bypass"
"#;

    const UPDATE: &str = r#"# Opens a new terminal in `directory` and types the commands of this computer's distribution.
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
"#;

    fn sys(id: &str, like: &[&str]) -> System {
        System { id: id.into(), like: like.iter().map(|s| s.to_string()).collect(), ..System::default() }
    }

    #[test]
    fn a_session_launch_preserves_quoted_before_commands() {
        let l = parse("claude-code", CLAUDE, &sys("cachyos", &["arch"])).unwrap();
        assert_eq!((l.name.as_str(), l.directory.as_str(), l.type_name(), l.full_access()), ("Claude Code", "~", "Claude Code", true));
        let Kind::Session { agent, permission, before, preset } = &l.kind else { panic!("{l:?}") };
        assert_eq!((agent, *permission, preset), (&Agent::ClaudeCode, Some(Permission::Bypass), &None));
        assert_eq!(before, &[r#"printf "%s\n" 'fixture'"#]);
    }

    #[test]
    fn the_system_update_picks_the_variant_of_this_computer() {
        let on = |s: System| match parse("actualizar-sistema", UPDATE, &s).unwrap().kind {
            Kind::Commands { commands } => commands,
            k => panic!("{k:?}"),
        };
        assert_eq!(on(sys("cachyos", &["arch"])), vec!["sudo pacman -Syu", "paru -Sua"]);
        assert_eq!(on(sys("endeavouros", &["arch"])), vec!["sudo pacman -Syu"], "ID_LIKE decides when the ID has no variant");
        assert_eq!(on(sys("debian", &[])), vec!["sudo apt update", "sudo apt full-upgrade"]);
        // No variant for this computer and no commands of the file's own: nothing to type.
        assert_eq!(on(sys("nixos", &[])), Vec::<String>::new());
        let l = parse("actualizar-sistema", UPDATE, &sys("cachyos", &[])).unwrap();
        assert_eq!((l.type_name(), l.full_access()), ("Terminal", false));
    }

    #[test]
    fn what_the_console_refuses_this_window_refuses() {
        let bad = |text: &str| parse("x", text, &System::default()).unwrap_err();
        assert!(bad("").contains("name"));
        assert!(bad("name = \"a\"").contains("needs commands or a session"));
        assert!(
            bad("name = \"a\"\ncommands = [\"x\"]\n[session]\nagent = \"codex\"")
                .contains("not both")
        );
        assert!(
            bad("name = \"a\"\n[session]\nagent = \"gemini\"").contains("claude_code or codex")
        );
        assert!(
            bad("name = \"a\"\n[session]\nagent = \"codex\"\npermission = \"root\"")
                .contains("permission must be")
        );
        assert!(bad("name = \"a\"\n[session]\nagent = \"codex\"\npermission = \"ask\"\nprofile = \"Strict\"").contains("not both"));
        assert!(bad("name = \"a\"\nbefore = [\"x\"]\ncommands = [\"y\"]").contains("before belongs to a session"));
        assert!(bad("name = \"a\"\ncommands = [\"a\\u0007b\"]").contains("one line"), "a control character would type more than the file says");
        assert!(bad("name = \"a\"\ncommands = [\"x\"]\nsurprise = 1").contains("not a launch file"));
        assert!(bad("name = \"a\"\ndirectory = \"projects\"\ncommands = [\"x\"]").contains("directory must be"));
        assert!(bad("name = \"a\"\nname = \"b\"").contains("twice"));
        assert!(bad("name = \"a\"\nnotes = \"\"\"x\"\"\"").contains("multi-line"));
    }

    #[test]
    fn permissions_map_to_the_templates_the_console_sends() {
        let p = |w: &str| parse("x", &format!("name = \"a\"\n[session]\nagent = \"claude_code\"\npermission = \"{w}\""), &System::default()).unwrap();
        let id = |l: Launch| match l.kind {
            Kind::Session { permission, .. } => permission.map(Permission::profile_id),
            k => panic!("{k:?}"),
        };
        assert_eq!(id(p("ask")), Some("template-strict"));
        assert_eq!(id(p("read_only")), Some("template-review"));
        assert_eq!(id(p("edits")), Some("template-edits"));
        assert_eq!(id(p("bypass")), Some("template-full-access"));
        let none = parse("x", "name = \"a\"\n[session]\nagent = \"codex\"", &System::default()).unwrap();
        assert_eq!(id(none), None, "no permission: as configured");
    }

    #[test]
    fn the_toml_subset_reads_strings_arrays_comments_and_tables() {
        let d = parse_toml("a = 'lit\\eral'  # note\nb = [\"x\", # c\n  \"y\",\n]\nc = true\n[t.u]\nk = \"\\u00e9\\t\"\n").unwrap();
        assert_eq!(d.root["a"], Value::Str("lit\\eral".into()), "a literal string keeps its backslash");
        assert_eq!(d.root["b"], Value::Arr(vec![Value::Str("x".into()), Value::Str("y".into())]));
        assert_eq!(d.root["c"], Value::Bool(true));
        assert_eq!(d.tables["t.u"]["k"], Value::Str("é\t".into()));
        assert!(parse_toml("a = ").is_err());
        assert!(parse_toml("a = \"open").is_err());
        assert!(parse_toml("a = [\"x\"").is_err());
        assert!(parse_toml("a = 1").is_err(), "numbers are not used in launch files");
        assert!(parse_toml("[[x]]").is_err());
        assert!(parse_toml("a = \"x\" b").is_err());
    }

    #[test]
    fn a_folder_is_listed_by_name_with_the_refused_files_apart() {
        let dir = std::env::temp_dir().join(format!("wyss-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("zeta.toml"), "name = \"Zeta\"\ncommands = [\"echo preset-ok\"]\n").unwrap();
        std::fs::write(dir.join("claude-code.toml"), CLAUDE).unwrap();
        std::fs::write(dir.join("roto.toml"), "name = ").unwrap();
        std::fs::write(dir.join("Mayus.toml"), "name = \"x\"\ncommands = [\"y\"]\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a launch").unwrap();
        let l = read_folder(&dir, &System::default());
        assert_eq!(l.launches.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), vec!["Claude Code", "Zeta"]);
        let refused: Vec<&str> = l.refused.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(refused, vec!["Mayus", "roto"], "a bad name and a bad file; notes.txt is not a launch");
        assert_eq!(read_folder(&dir.join("nope"), &System::default()), Listing::default(), "a missing folder is an empty list, never created");
        assert!(!dir.join("nope").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn one_launch_is_read_again_from_its_file_by_a_safe_id() {
        let dir = std::env::temp_dir().join(format!("wyss-launch-get-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), "name = \"A\"\ncommands = [\"echo preset-ok\"]\n").unwrap();
        assert_eq!(get(&dir, "a", &System::default()).unwrap().name, "A");
        std::fs::write(dir.join("a.toml"), "name = \"Changed\"\ncommands = [\"x\"]\n").unwrap();
        assert_eq!(get(&dir, "a", &System::default()).unwrap().name, "Changed", "what opens is what the file says now");
        assert_eq!(get(&dir, "../a", &System::default()), Err("unknown launch".into()), "an id is never a path");
        assert!(get(&dir, "missing", &System::default()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn directories_resolve_against_the_hosts_home_and_must_exist() {
        let tmp = std::env::temp_dir();
        let home = tmp.to_string_lossy().into_owned();
        assert_eq!(resolve_directory("~", &home).unwrap(), home);
        assert_eq!(resolve_directory("/", "/nowhere").unwrap(), "/");
        assert_eq!(resolve_directory("~/definitely-not-here-wyss-console", &home), Err("directory not found"));
        assert_eq!(resolve_directory("projects", &home), Err("directory must be ~, ~/… or an absolute path"));
        assert_eq!(display(Path::new("/h/a/.config/wyss-console/launch"), "/h/a"), "~/.config/wyss-console/launch");
        assert_eq!(display(Path::new("/tmp/x/launch"), "/h/a"), "/tmp/x/launch");
    }

    #[test]
    fn typing_a_launch_sends_each_command_with_enter() {
        assert_eq!(typed(&["echo preset-ok".into(), "ls".into()]), b"echo preset-ok\rls\r");
        assert!(typed(&[]).is_empty());
    }
}
