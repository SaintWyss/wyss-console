//! The text of one global instruction file, for Workspace to show. Read only, and only the
//! files a launch can switch: `CLAUDE.md` and `rules/<name>.md` of `~/.claude`, and `AGENTS.md`
//! of the Codex folder. The UI names a file by those closed words, never by a path, and a file
//! whose real path leaves its folder (a link pointing elsewhere) is refused.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest text shown. Past it the text is cut and says so.
pub const MAX_BYTES: u64 = 256 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not one of the names Workspace lists.
    Name,
    /// Missing, unreadable, or its real path is outside its folder.
    Unreadable,
}

/// Which file a name means: its folder and its path inside it.
fn locate(name: &str, claude: &Path, codex: &Path) -> Result<(PathBuf, PathBuf), Refusal> {
    if name == "CLAUDE.md" {
        return Ok((claude.to_owned(), claude.join("CLAUDE.md")));
    }
    if let Some(file @ ("AGENTS.md" | "AGENTS.override.md")) = name.strip_prefix("codex/") {
        return Ok((codex.to_owned(), codex.join(file)));
    }
    let rule = name.strip_prefix("rules/").ok_or(Refusal::Name)?;
    let ok = (4..=200).contains(&rule.len())
        && rule.ends_with(".md")
        && !rule.starts_with('.')
        && !rule.contains(['/', '\\'])
        && !rule.chars().any(char::is_control);
    if !ok {
        return Err(Refusal::Name);
    }
    Ok((claude.to_owned(), claude.join("rules").join(rule)))
}

/// The file's text (lossy UTF-8), cut at [`MAX_BYTES`] with a closing line when longer.
pub fn read(name: &str, claude: &Path, codex: &Path) -> Result<String, Refusal> {
    let (folder, path) = locate(name, claude, codex)?;
    let folder = folder.canonicalize().map_err(|_| Refusal::Unreadable)?;
    let real = path.canonicalize().map_err(|_| Refusal::Unreadable)?;
    if !real.starts_with(&folder) || !real.is_file() {
        return Err(Refusal::Unreadable);
    }
    let file = fs::File::open(&real).map_err(|_| Refusal::Unreadable)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Refusal::Unreadable)?;
    let cut = bytes.len() as u64 > MAX_BYTES;
    bytes.truncate(MAX_BYTES as usize);
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if cut {
        text.push_str("\n\n[…]");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn dirs(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "wyss-instruction-text-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let claude = root.join("claude");
        let codex = root.join("codex");
        fs::create_dir_all(claude.join("rules")).unwrap();
        fs::create_dir_all(&codex).unwrap();
        (root, claude, codex)
    }

    #[test]
    fn reads_the_listed_files_and_follows_a_link_inside_the_folder() {
        let (_root, claude, codex) = dirs("ok");
        fs::write(claude.join("AGENTS.md"), "# Kernel").unwrap();
        symlink("AGENTS.md", claude.join("CLAUDE.md")).unwrap();
        fs::write(claude.join("rules/ux.md"), "# UX").unwrap();
        fs::write(codex.join("AGENTS.md"), "# Codex").unwrap();
        assert_eq!(read("CLAUDE.md", &claude, &codex).unwrap(), "# Kernel");
        assert_eq!(read("rules/ux.md", &claude, &codex).unwrap(), "# UX");
        assert_eq!(read("codex/AGENTS.md", &claude, &codex).unwrap(), "# Codex");
    }

    #[test]
    fn refuses_names_that_are_paths_or_not_listed() {
        let (_root, claude, codex) = dirs("names");
        for name in [
            "",
            "AGENTS.md",
            "../secret.md",
            "rules/../../etc/passwd",
            "rules/a/b.md",
            "rules/.hidden.md",
            "rules/notes.txt",
            "/etc/passwd",
            "settings.json",
        ] {
            assert_eq!(read(name, &claude, &codex), Err(Refusal::Name), "{name}");
        }
    }

    #[test]
    fn refuses_a_link_that_leaves_the_folder_and_a_missing_file() {
        let (root, claude, codex) = dirs("escape");
        fs::write(root.join("outside.md"), "secret").unwrap();
        symlink(root.join("outside.md"), claude.join("rules/out.md")).unwrap();
        assert_eq!(
            read("rules/out.md", &claude, &codex),
            Err(Refusal::Unreadable)
        );
        assert_eq!(
            read("rules/none.md", &claude, &codex),
            Err(Refusal::Unreadable)
        );
    }

    #[test]
    fn cuts_a_long_file_and_says_so() {
        let (_root, claude, codex) = dirs("long");
        fs::write(
            claude.join("rules/big.md"),
            "a".repeat(MAX_BYTES as usize + 10),
        )
        .unwrap();
        let text = read("rules/big.md", &claude, &codex).unwrap();
        assert!(text.ends_with("[…]"));
        assert_eq!(text.len(), MAX_BYTES as usize + "\n\n[…]".len());
    }
}
