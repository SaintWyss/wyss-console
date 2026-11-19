//! How an agent is started through the user's shell.
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

/// Where the pinned variable is set from: an absolute path, so a start-up file that changes
/// `PATH`, or defines a function or an alias named `env`, does not choose what runs.
const ENV: &str = "/usr/bin/env";

const POSIX_GUARD: &str = "resolved=$(command -v \"$1\") || { printf '%s\\n' 'Wyss: provider command could not be resolved; no agent was started.' >&2; exit 127; }; actual=$(\"$resolved\" --version 2>/dev/null) || { printf '%s\\n' 'Wyss: provider version could not be checked; no agent was started.' >&2; exit 125; }; if [ \"$actual\" != \"$2\" ]; then printf '%s\\n' 'Wyss: provider build changed after preview; no profile was applied and no agent was started.' >&2; exit 125; fi; if [ -z \"${WYSS_NATIVE_VERSION_GUARD:-}\" ] || ! printf 'ok\\n' > \"$WYSS_NATIVE_VERSION_GUARD\"; then printf '%s\\n' 'Wyss: provider start could not be verified; no agent was started.' >&2; exit 125; fi; unset WYSS_NATIVE_VERSION_GUARD; ";
const POSIX_MARKER: &str = "if [ -z \"${WYSS_NATIVE_VERSION_GUARD:-}\" ] || ! printf 'ok\\n' > \"$WYSS_NATIVE_VERSION_GUARD\"; then exit 125; fi; unset WYSS_NATIVE_VERSION_GUARD; ";
const POSIX_EXEC: &str = "shift 2; exec \"$resolved\" \"$@\"";
const POSIX_EXEC_PINNED: &str = "pin=$3; shift 3; exec /usr/bin/env \"$pin\" \"$resolved\" \"$@\"";
const FISH_RESOLVE: &str = "set -l resolved (command -s -- \"$argv[1]\"); if test -z \"$resolved\"; printf '%s\\n' 'Wyss: provider command could not be resolved; no agent was started.' >&2; exit 127; end; ";
const FISH_PROBE: &str = "set -l actual (\"$resolved\" --version 2>/dev/null); ";
const FISH_SECRET_PROBE: &str = "set -l actual (begin; set -lu CLAUDE_CODE_OAUTH_TOKEN; \"$resolved\" --version 2>/dev/null; end); ";
const FISH_VERSION: &str = "if test (count $actual) -ne 1; printf '%s\\n' 'Wyss: provider version could not be checked; no agent was started.' >&2; exit 125; end; if test \"$actual\" != \"$argv[2]\"; printf '%s\\n' 'Wyss: provider build changed after preview; no profile was applied and no agent was started.' >&2; exit 125; end; ";
const FISH_MARKER: &str = "if not set -q WYSS_NATIVE_VERSION_GUARD; or test -z \"$WYSS_NATIVE_VERSION_GUARD\"; printf '%s\\n' 'Wyss: provider start could not be verified; no agent was started.' >&2; exit 125; end; printf 'ok\\n' > \"$WYSS_NATIVE_VERSION_GUARD\"; or begin; printf '%s\\n' 'Wyss: provider start could not be verified; no agent was started.' >&2; exit 125; end; set -e WYSS_NATIVE_VERSION_GUARD; ";
/// Reads the stored sign-in token from the private file `WYSS_NATIVE_TOKEN_FILE` names, first
/// thing in the command (after the start-up files, before the version check), and removes the
/// file; if it cannot be removed, no agent starts. The token is held in a shell variable that
/// is not exported (unset first, so a start-up file's export does not carry over), so the
/// version check never sees it; `POSIX_SECRET_EXPORT` exports it right
/// before the `exec`, for the agent alone: the shell's own environment never held it.
const POSIX_SECRET_READ: &str = "unset wyss_has_token; if [ -n \"${WYSS_NATIVE_TOKEN_FILE:-}\" ]; then unset CLAUDE_CODE_OAUTH_TOKEN; IFS= read -r CLAUDE_CODE_OAUTH_TOKEN < \"$WYSS_NATIVE_TOKEN_FILE\" || { rm -f -- \"$WYSS_NATIVE_TOKEN_FILE\"; printf '%s\\n' 'Wyss: the sign-in token could not be read; no agent was started.' >&2; exit 125; }; rm -f -- \"$WYSS_NATIVE_TOKEN_FILE\" || { printf '%s\\n' 'Wyss: the sign-in token file could not be removed; no agent was started.' >&2; exit 125; }; unset WYSS_NATIVE_TOKEN_FILE; wyss_has_token=1; fi; ";
const POSIX_SECRET_EXPORT: &str = "if [ -n \"${wyss_has_token:-}\" ]; then export CLAUDE_CODE_OAUTH_TOKEN; unset wyss_has_token; fi; ";
/// Read and export without expanding the token as a traced command argument. An unexported
/// local shadows the token while external cleanup and version checks run.
const FISH_SECRET_READ: &str = "if set -q WYSS_NATIVE_TOKEN_FILE; and test -n \"$WYSS_NATIVE_TOKEN_FILE\"; read -gx CLAUDE_CODE_OAUTH_TOKEN < \"$WYSS_NATIVE_TOKEN_FILE\"; or begin; set -lu CLAUDE_CODE_OAUTH_TOKEN; command rm -f -- \"$WYSS_NATIVE_TOKEN_FILE\"; printf '%s\\n' 'Wyss: the sign-in token could not be read; no agent was started.' >&2; exit 125; end; begin; set -lu CLAUDE_CODE_OAUTH_TOKEN; command rm -f -- \"$WYSS_NATIVE_TOKEN_FILE\"; end; or begin; printf '%s\\n' 'Wyss: the sign-in token file could not be removed; no agent was started.' >&2; exit 125; end; set -e WYSS_NATIVE_TOKEN_FILE; end; ";
const FISH_EXEC: &str = "set -e argv[1]; set -e argv[1]; exec \"$resolved\" $argv";
const FISH_EXEC_PINNED: &str = "set -l pin $argv[3]; set -e argv[1]; set -e argv[1]; set -e argv[1]; exec /usr/bin/env $pin \"$resolved\" $argv";

/// A profile's provider flags are checked against the build that the window discovered. A
/// shell may change PATH in its startup files, so a profile launch checks the shell-resolved
/// binary with its supported `--version` interface and execs that same resolved path. Values
/// remain argv words; none of the provider or profile data becomes shell command text.
#[cfg(test)]
pub fn recipe_with_version_guard(
    shell: &Path,
    words: &[String],
    direct: &Path,
    expected_version: Option<&str>,
) -> Recipe {
    recipe_pinned(shell, words, direct, expected_version, None)
}

/// [`recipe_with_version_guard`], with one variable the agent must start with whatever the
/// shell's start-up files say: the configuration folder of a launch home (`CODEX_HOME`,
/// `CLAUDE_CONFIG_DIR`). A start-up file that exports the same name would otherwise replace it,
/// and the agent would run with the owner's own folder: with the hooks the launch left out.
///
/// The variable is set after the start-up files ran, in the very `exec` that becomes the agent
/// (`exec /usr/bin/env NAME=value agent …`). `NAME=value` travels as one argv word and is never
/// shell text. Started directly (a shell without a recipe) there is no start-up file between:
/// the caller's process environment is the agent's, so `pin` adds nothing there.
///
/// `pin` is never a secret: argv is readable by other processes of the user.
#[cfg(test)]
pub fn recipe_pinned(
    shell: &Path,
    words: &[String],
    direct: &Path,
    expected_version: Option<&str>,
    pin: Option<(&str, &str)>,
) -> Recipe {
    recipe_pinned_secret(shell, words, direct, expected_version, pin, false)
}

/// [`recipe_pinned`]; with `secret`, a checked launch (a recipe and `expected_version`) also
/// reads the sign-in token from `WYSS_NATIVE_TOKEN_FILE` before anything else and exports it
/// for the agent, hidden from the version check and cleanup commands. Without a checked launch `secret` adds nothing: the caller hands the token by
/// environment then, and there is no shell between when there is no recipe.
pub fn recipe_pinned_secret(
    shell: &Path,
    words: &[String],
    direct: &Path,
    expected_version: Option<&str>,
    pin: Option<(&str, &str)>,
    secret: bool,
) -> Recipe {
    let pin = pin.map(|(name, value)| format!("{name}={value}"));
    let Some(family) = family(shell) else {
        // The service checks the known absolute path immediately before this direct launch.
        return recipe(shell, words, direct);
    };
    let Some(expected_version) = expected_version else {
        if secret {
            let (command, label) = match family {
                Family::Fish => (format!("{FISH_SECRET_READ}{FISH_MARKER}exec $argv"), "--"),
                _ => (
                    format!("{POSIX_SECRET_READ}{POSIX_MARKER}{POSIX_SECRET_EXPORT}exec \"$@\""),
                    "wyss-launch",
                ),
            };
            let mut args = vec!["-i".into(), "-c".into(), command, label.into()];
            if let Some(pin) = pin {
                args.extend([ENV.to_owned(), pin]);
            }
            args.extend_from_slice(words);
            return Recipe {
                program: shell.to_owned(),
                args,
                through_shell: true,
            };
        }
        let Some(pin) = pin else {
            return recipe(shell, words, direct);
        };
        let mut pinned = vec![ENV.to_owned(), pin];
        pinned.extend(words.iter().cloned());
        return recipe(shell, &pinned, direct);
    };
    let (guard, exec, label) = match (family, pin.is_some()) {
        (Family::Posix | Family::Bash | Family::Zsh, false) => {
            (POSIX_GUARD, POSIX_EXEC, "wyss-checked-launch")
        }
        (Family::Posix | Family::Bash | Family::Zsh, true) => {
            (POSIX_GUARD, POSIX_EXEC_PINNED, "wyss-checked-launch")
        }
        (Family::Fish, false) => (FISH_MARKER, FISH_EXEC, "--"),
        (Family::Fish, true) => (FISH_MARKER, FISH_EXEC_PINNED, "--"),
    };
    let mut args = vec![
        "-i".to_owned(),
        "-c".to_owned(),
        {
            match (secret, family) {
                (true, Family::Fish) => format!("{FISH_SECRET_READ}{FISH_RESOLVE}{FISH_SECRET_PROBE}{FISH_VERSION}{guard}{exec}"),
                (false, Family::Fish) => format!("{FISH_RESOLVE}{FISH_PROBE}{FISH_VERSION}{guard}{exec}"),
                (true, _) => format!("{POSIX_SECRET_READ}{guard}{POSIX_SECRET_EXPORT}{exec}"),
                (false, _) => format!("{guard}{exec}"),
            }
        },
        label.to_owned(),
    ];
    args.push(words[0].clone());
    args.push(expected_version.to_owned());
    args.extend(pin);
    args.extend(words.iter().skip(1).cloned());
    Recipe {
        program: shell.to_owned(),
        args,
        through_shell: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn fish_trace_does_not_print_the_oauth_token() {
        let Some(fish) = ["/usr/bin/fish", "/bin/fish"].into_iter().map(Path::new).find(|p| p.is_file()) else {
            eprintln!("SKIPPED: fish is unavailable");
            return;
        };
        let root = std::env::temp_dir().join(format!("wyss-fish-trace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let file = root.join("oauth-token");
        let marker = root.join("verified");
        let provider = root.join("fixture-provider");
        const TOKEN: &str = "synthetic-token-r3-do-not-display";
        super::super::store::write_atomic(&file, format!("{TOKEN}\n").as_bytes()).unwrap();
        std::fs::write(&provider, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then\n  [ -z \"${{CLAUDE_CODE_OAUTH_TOKEN:-}}\" ] || exit 1\n  echo fixture-1\nelse\n  [ \"${{CLAUDE_CODE_OAUTH_TOKEN:-}}\" = '{TOKEN}' ] && echo agent-got-token\nfi\n")).unwrap();
        std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o700)).unwrap();
        let recipe = recipe_pinned_secret(fish, &[provider.to_string_lossy().into_owned()], &provider, Some("fixture-1"), None, true);
        let output = std::process::Command::new(recipe.program)
            .arg("--no-config")
            .args(recipe.args)
            .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
            .env("fish_trace", "1")
            .env("TERM", "dumb")
            .env("WYSS_NATIVE_TOKEN_FILE", &file)
            .env("WYSS_NATIVE_VERSION_GUARD", &marker)
            .output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("agent-got-token"));
        let trace = String::from_utf8_lossy(&output.stderr);
        assert!(trace.contains("command -s"), "fish trace was not active");
        assert!(!trace.contains(TOKEN) && !String::from_utf8_lossy(&output.stdout).contains(TOKEN), "fish trace printed the synthetic token");
        assert!(!file.exists());
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "ok\n");
    }
}
