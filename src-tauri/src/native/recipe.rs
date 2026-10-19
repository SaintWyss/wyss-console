//! How an agent is started through the user's shell (AD-09, DN-02, OQ-07).
//!
//! The chosen way: start the user's interactive shell with a command that *replaces* it by the
//! agent (`exec`). The shell reads its start-up files, so the agent gets the environment the
//! user gets when typing the command; and the process the terminal owns becomes the agent, so
//! it is observed as what it is. The arguments travel as separate words after the command text
//! and are never parsed: no word is ever put inside text a shell reads.
//!
//! A shell without a recipe falls back to starting the agent directly, and the session says
//! that the environment is then the Console's own.
use std::path::{Path, PathBuf};

/// The shells with a recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `sh`, `dash`, `ash`: POSIX.
    Posix,
    Bash,
    Zsh,
    Fish,
}

pub fn family(shell: &Path) -> Option<Family> {
    match shell.file_name()?.to_str()? {
        "sh" | "dash" | "ash" => Some(Family::Posix),
        "bash" => Some(Family::Bash),
        "zsh" => Some(Family::Zsh),
        "fish" => Some(Family::Fish),
        _ => None,
    }
}

/// What the terminal runs: the program, its arguments, and whether the user's shell sits
/// between (so the environment is the user's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub through_shell: bool,
}

/// `words` is the agent's executable name followed by its arguments. `direct` is the agent's
/// absolute path, used only when the shell has no recipe.
pub fn recipe(shell: &Path, words: &[String], direct: &Path) -> Recipe {
    let strings = |parts: &[&str]| parts.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>();
    let mut args = match family(shell) {
        // `$0` is the label after the text; the words are `$@`.
        Some(Family::Posix | Family::Bash | Family::Zsh) => {
            strings(&["-i", "-c", "exec \"$@\"", "wyss-launch"])
        }
        // fish hands the words after the command to it as `$argv`. `--` keeps a word that
        // looks like an option (`--model`) from being read as fish's own.
        Some(Family::Fish) => strings(&["-i", "-c", "exec $argv", "--"]),
        None => {
            return Recipe {
                program: direct.to_owned(),
                args: words.iter().skip(1).cloned().collect(),
                through_shell: false,
            };
        }
    };
    args.extend(words.iter().cloned());
    Recipe {
        program: shell.to_owned(),
        args,
        through_shell: true,
    }
}
