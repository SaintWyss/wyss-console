//! The skills the account keeps, read for Arsenal. Each skill folder is one tree of files, read
//! through the same approved roots, bounded reads and link rules as the inventory. Nothing is
//! executed, nothing is written, and no link leaves the folder of the skill that holds it.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::inventory::Environment;
use super::model::{Bounds, SourceFailure};
use super::reader::{ConfigReader, EntryKind};
use super::redact;
use super::roots::ApprovedRoots;

/// Most skills one scan reports per source. The rest are counted as truncated.
pub const MAX_SKILLS: usize = 256;
/// Most files one skill may hold. A skill that holds more cannot be imported.
pub const MAX_FILES: usize = 256;
/// Largest single file of a skill. A larger one blocks its skill.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// Largest whole skill. A larger one blocks its skill.
pub const MAX_TREE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 6;
const FRONT_MATTER_BYTES: usize = 8 * 1024;
const DESCRIPTION_CHARS: usize = 500;
const SCRIPT_EXTENSIONS: [&str; 11] = [
    "sh", "bash", "zsh", "py", "js", "mjs", "cjs", "ts", "rb", "pl", "ps1",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Read,
    Absent,
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    pub source: &'static str,
    pub state: SourceState,
}

/// A file or folder of a skill that was not copied, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Excluded {
    pub path: String,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDigest {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// One skill as it stands in its folder right now. `content` is what an import copies; it is
/// never serialized, so the window only ever sees the digests.
#[derive(Debug, Clone, Serialize)]
pub struct Skill {
    pub provider: &'static str,
    pub source: &'static str,
    pub name: String,
    pub description: String,
    /// Where the skill was read, with the account's home shown as `~`. For display only.
    pub origin: String,
    pub files: Vec<FileDigest>,
    pub excluded: Vec<Excluded>,
    /// Copied files whose text mentions a credential word. They do not block the import; the
    /// owner checks them first.
    pub review: Vec<Excluded>,
    pub executes_code: bool,
    /// Why the skill cannot be imported; `None` when it can.
    pub blocked: Option<&'static str>,
    #[serde(skip)]
    pub content: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    pub sources: Vec<SourceReport>,
    pub skills: Vec<Skill>,
    pub truncated: bool,
}

/// The skills of this process's account: `HOME`, `CLAUDE_CONFIG_DIR` and `CODEX_HOME`.
pub fn scan_process() -> Option<Scan> {
    Environment::from_process(None).map(|env| scan(&env))
}

/// The same scan with `home` as the only account folder. Used by tests and fixtures.
pub fn scan_home(home: PathBuf) -> Scan {
    scan(&Environment {
        home,
        claude_config_dir: None,
        codex_home: None,
        project: None,
        system: PathBuf::from("/etc"),
        declared_writers: Vec::new(),
    })
}

/// A skill's name is its folder's name: 1 to 64 letters, digits, `.`, `_` or `-`, not hidden.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn scan(env: &Environment) -> Scan {
    let mut scan = Scan {
        sources: Vec::new(),
        skills: Vec::new(),
        truncated: false,
    };
    let folders = [
        (
            "claude_code",
            "claude_user_skills",
            env.claude_dir().join("skills"),
        ),
        ("codex", "codex_user_skills", env.agents_skills_dir()),
    ];
    for (provider, source, folder) in folders {
        let reader = match ApprovedRoots::new([folder.clone()])
            .ok()
            .and_then(|roots| ConfigReader::new(roots, Bounds::PROPOSED).ok())
        {
            Some(reader) => reader,
            None => {
                scan.sources.push(SourceReport {
                    source,
                    state: SourceState::Unreadable,
                });
                continue;
            }
        };
        let listing = match reader.list_directory(&folder) {
            Ok(listing) => listing,
            Err(SourceFailure::Missing) => {
                scan.sources.push(SourceReport {
                    source,
                    state: SourceState::Absent,
                });
                continue;
            }
            Err(_) => {
                scan.sources.push(SourceReport {
                    source,
                    state: SourceState::Unreadable,
                });
                continue;
            }
        };
        let mut found = 0;
        for entry in listing.entries {
            let Some(name) = entry.name.to_str() else {
                continue;
            };
            if !name_ok(name) {
                continue;
            }
            if found == MAX_SKILLS {
                scan.truncated = true;
                break;
            }
            found += 1;
            let origin = origin(&env.home, &folder, name);
            let skill = match entry.kind {
                EntryKind::Directory => {
                    read_skill(&reader, provider, source, &folder.join(name), name, origin)
                }
                EntryKind::Link => blocked_skill(provider, source, name, origin, "symbolic_link"),
                EntryKind::File | EntryKind::Other => continue,
            };
            scan.skills.push(skill);
        }
        scan.sources.push(SourceReport {
            source,
            state: SourceState::Read,
        });
    }
    scan
}

/// Where a skill was read, with the home folder shown as `~`.
fn origin(home: &Path, folder: &Path, name: &str) -> String {
    let shown = folder.strip_prefix(home).map_or_else(
        |_| folder.display().to_string(),
        |rest| format!("~/{}", rest.display()),
    );
    format!("{shown}/{name}")
}

fn blocked_skill(
    provider: &'static str,
    source: &'static str,
    name: &str,
    origin: String,
    reason: &'static str,
) -> Skill {
    Skill {
        provider,
        source,
        name: name.to_owned(),
        description: String::new(),
        origin,
        files: Vec::new(),
        excluded: Vec::new(),
        review: Vec::new(),
        executes_code: false,
        blocked: Some(reason),
        content: Vec::new(),
    }
}

#[derive(Default)]
struct Tree {
    files: Vec<(String, Vec<u8>)>,
    excluded: Vec<Excluded>,
    review: Vec<Excluded>,
    blocked: Option<&'static str>,
    executes_code: bool,
    bytes: usize,
}

fn read_skill(
    reader: &ConfigReader,
    provider: &'static str,
    source: &'static str,
    folder: &Path,
    name: &str,
    origin: String,
) -> Skill {
    let mut tree = Tree::default();
    let mut pending = vec![(folder.to_path_buf(), String::new(), 0_usize)];
    while let Some((dir, prefix, depth)) = pending.pop() {
        let listing = match reader.list_directory(&dir) {
            Ok(listing) => listing,
            Err(failure) if prefix.is_empty() => {
                tree.blocked.get_or_insert(failure.as_str());
                continue;
            }
            Err(failure) => {
                tree.excluded.push(Excluded {
                    path: prefix,
                    reason: failure.as_str(),
                });
                continue;
            }
        };
        for entry in listing.entries {
            let child = entry.name.to_string_lossy().into_owned();
            let relative = join(&prefix, &child);
            if entry.name.to_str().is_none() {
                tree.excluded.push(Excluded {
                    path: relative,
                    reason: "non_utf8_name",
                });
                continue;
            }
            if child.starts_with('.') {
                tree.excluded.push(Excluded {
                    path: relative,
                    reason: "hidden_entry",
                });
                continue;
            }
            match entry.kind {
                EntryKind::Directory if depth + 1 > MAX_DEPTH => tree.excluded.push(Excluded {
                    path: relative,
                    reason: "too_deep",
                }),
                EntryKind::Directory => {
                    pending.push((dir.join(&child), relative, depth + 1));
                }
                EntryKind::File => take_file(reader, &dir.join(&child), relative, &mut tree),
                EntryKind::Link => tree.excluded.push(Excluded {
                    path: relative,
                    reason: "symbolic_link",
                }),
                EntryKind::Other => tree.excluded.push(Excluded {
                    path: relative,
                    reason: "special_file",
                }),
            }
        }
    }
    if tree.files.len() > MAX_FILES {
        tree.blocked.get_or_insert("too_many_files");
    }
    if tree.bytes > MAX_TREE_BYTES {
        tree.blocked.get_or_insert("tree_too_large");
    }
    if !tree.files.iter().any(|(path, _)| path == "SKILL.md") {
        tree.blocked.get_or_insert("no_skill_file");
    }
    let description = tree
        .files
        .iter()
        .find(|(path, _)| path == "SKILL.md")
        .map(|(_, bytes)| description(bytes))
        .unwrap_or_default();
    let files = tree
        .files
        .iter()
        .map(|(path, bytes)| FileDigest {
            path: path.clone(),
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        })
        .collect();
    Skill {
        provider,
        source,
        name: name.to_owned(),
        description,
        origin,
        files,
        excluded: tree.excluded,
        review: tree.review,
        executes_code: tree.executes_code,
        blocked: tree.blocked,
        content: tree.files,
    }
}

fn take_file(reader: &ConfigReader, path: &Path, relative: String, tree: &mut Tree) {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if credential_name(name) {
        tree.excluded.push(Excluded {
            path: relative,
            reason: "credential_name",
        });
        return;
    }
    let bytes = match reader.read_file(path) {
        Ok(file) => file.bytes().to_vec(),
        Err(failure) => {
            tree.excluded.push(Excluded {
                path: relative,
                reason: failure.as_str(),
            });
            return;
        }
    };
    if bytes.len() > MAX_FILE_BYTES {
        tree.blocked.get_or_insert("file_too_large");
        tree.excluded.push(Excluded {
            path: relative,
            reason: "too_large",
        });
        return;
    }
    let verdict = std::str::from_utf8(&bytes)
        .ok()
        .map(|text| (credential_value(text), redact::text(text).may_hold_secret()));
    match verdict {
        None => tree.excluded.push(Excluded {
            path: relative,
            reason: "binary_file",
        }),
        // A credential value blocks the whole skill: a partial copy would still carry it.
        Some((true, _)) => {
            tree.blocked.get_or_insert("needs_secret");
            tree.excluded.push(Excluded {
                path: relative,
                reason: "may_hold_secret",
            });
        }
        Some((false, mentions)) => {
            if mentions {
                tree.review.push(Excluded {
                    path: relative.clone(),
                    reason: "mentions_credential",
                });
            }
            tree.executes_code |= is_script(&relative);
            tree.bytes += bytes.len();
            tree.files.push((relative, bytes));
        }
    }
}

/// Text that carries a credential by its shape: a provider's token, a password inside a URL, or a
/// private key. Only these block a skill. A credential word in prose, such as `password` or `token`,
/// does not; those files are listed for review instead.
fn credential_value(text: &str) -> bool {
    let private_key = text.contains("-----BEGIN") && text.contains("PRIVATE KEY-----");
    let url_password = redact::text(text)
        .found
        .contains(&redact::SecretClass::UrlCredentials);
    let provider_token = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        .any(|word| {
            word.len() >= 16
                && redact::TOKEN_PREFIXES
                    .iter()
                    .any(|prefix| word.starts_with(prefix))
        });
    private_key || url_password || provider_token
}

/// The `description:` of a skill's YAML front matter, read from its first kilobytes.
fn description(bytes: &[u8]) -> String {
    let head = &bytes[..bytes.len().min(FRONT_MATTER_BYTES)];
    let Ok(text) = std::str::from_utf8(head) else {
        return String::new();
    };
    let Some(rest) = text.strip_prefix("---\n") else {
        return String::new();
    };
    let front = rest.split("\n---").next().unwrap_or_default();
    front
        .lines()
        .find_map(|line| line.strip_prefix("description:"))
        .map(|value| {
            value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .chars()
                .take(DESCRIPTION_CHARS)
                .collect()
        })
        .unwrap_or_default()
}

/// A file that is credentials by its name. Narrow on purpose: design token files are not credentials,
/// and what a file says is checked as text in `take_file`.
fn credential_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("credentials")
        || lower == "auth.json"
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
        || lower.contains("id_rsa")
        || lower.contains("id_ed25519")
}

/// Whether a file of a skill is code a provider would run. Wyss never runs it.
pub fn is_script(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| SCRIPT_EXTENSIONS.contains(&extension))
}

fn join(prefix: &str, child: &str) -> String {
    if prefix.is_empty() {
        child.to_owned()
    } else {
        format!("{prefix}/{child}")
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn home() -> PathBuf {
        let home = std::env::temp_dir().join(format!("skills-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(home.join(".claude/skills")).unwrap();
        home
    }

    #[test]
    fn reads_a_skill_tree_and_reports_what_it_leaves_out() {
        let home = home();
        let skill = home.join(".claude/skills/design");
        fs::create_dir_all(skill.join("refs")).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: design\ndescription: \"Plans screens\"\n---\nbody",
        )
        .unwrap();
        fs::write(skill.join("refs/tokens.md"), "spacing 8px").unwrap();
        fs::write(skill.join("credentials.json"), "placeholder").unwrap();
        fs::write(skill.join(".hidden"), "x").unwrap();
        let scan = scan_home(home.clone());
        let found = scan
            .skills
            .iter()
            .find(|skill| skill.name == "design")
            .unwrap();
        assert_eq!(found.blocked, None);
        assert_eq!(found.description, "Plans screens");
        let paths: Vec<&str> = found.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["SKILL.md", "refs/tokens.md"]);
        assert!(
            found
                .excluded
                .iter()
                .any(|entry| entry.path == "credentials.json" && entry.reason == "credential_name")
        );
        assert!(
            found
                .excluded
                .iter()
                .any(|entry| entry.path == ".hidden" && entry.reason == "hidden_entry")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_provider_token_blocks_the_skill_and_a_missing_file_is_reported() {
        let home = home();
        let leaky = home.join(".claude/skills/leaky");
        fs::create_dir_all(&leaky).unwrap();
        fs::write(
            leaky.join("SKILL.md"),
            "export TOKEN=ghp_0123456789abcdefghijklmnop\n",
        )
        .unwrap();
        let bare = home.join(".claude/skills/bare");
        fs::create_dir_all(&bare).unwrap();
        fs::write(bare.join("notes.md"), "no front matter").unwrap();
        let scan = scan_home(home.clone());
        let blocked = |name: &str| {
            scan.skills
                .iter()
                .find(|skill| skill.name == name)
                .and_then(|skill| skill.blocked)
        };
        assert_eq!(blocked("leaky"), Some("needs_secret"));
        assert_eq!(blocked("bare"), Some("no_skill_file"));
        assert!(
            scan.sources
                .iter()
                .any(|source| source.source == "codex_user_skills"
                    && source.state == SourceState::Absent)
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_credential_word_in_prose_is_reviewed_and_does_not_block() {
        let home = home();
        let prose = home.join(".claude/skills/prose");
        fs::create_dir_all(&prose).unwrap();
        fs::write(
            prose.join("SKILL.md"),
            "---\ndescription: Design tokens\n---\npassword = \"hunter2hunter2\"\n",
        )
        .unwrap();
        let scan = scan_home(home.clone());
        let found = scan
            .skills
            .iter()
            .find(|skill| skill.name == "prose")
            .unwrap();
        assert_eq!(found.blocked, None);
        assert_eq!(found.content.len(), 1);
        assert!(
            found
                .review
                .iter()
                .any(|entry| entry.path == "SKILL.md" && entry.reason == "mentions_credential")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_symbolic_link_skill_is_blocked_and_never_followed() {
        let home = home();
        let outside = home.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("SKILL.md"), "---\ndescription: x\n---").unwrap();
        std::os::unix::fs::symlink(&outside, home.join(".claude/skills/linked")).unwrap();
        let scan = scan_home(home.clone());
        let linked = scan
            .skills
            .iter()
            .find(|skill| skill.name == "linked")
            .unwrap();
        assert_eq!(linked.blocked, Some("symbolic_link"));
        assert!(linked.content.is_empty());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_private_key_blocks_and_a_design_token_file_name_does_not() {
        assert!(credential_value(
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----"
        ));
        assert!(credential_value(
            "curl https://deploy:hunter2@example.com/x"
        ));
        assert!(!credential_value("tokens: spacing 8px, radius 10px"));
        assert!(!credential_name("tokens.md"));
        assert!(credential_name("credentials.json"));
    }

    #[test]
    fn names_outside_the_skill_grammar_are_not_skills() {
        assert!(name_ok("design-system.v2"));
        assert!(!name_ok(".hidden"));
        assert!(!name_ok("with space"));
        assert!(!name_ok(&"a".repeat(65)));
    }
}
