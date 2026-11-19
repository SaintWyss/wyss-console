//! For each shell family, the arguments arrive as the words they were, and the agent is
//! the process that runs. A fake agent prints what it received; each recipe is run for real
//! through the shells installed here.
use super::recipe::{Family, family, recipe, recipe_pinned, recipe_with_version_guard};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn fake_agent() -> (PathBuf, PathBuf) {
    let dir = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-recipe-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&dir).unwrap();
    let agent = dir.join("fake-agent");
    fs::write(
        &agent,
        "#!/bin/sh\nprintf 'argc=%s\\n' \"$#\"\nfor a in \"$@\"; do printf '[%s]\\n' \"$a\"; done\nprintf 'pid=%s\\nENV=%s\\n' \"$$\" \"$WYSS_FIXTURE\"\n",
    )
    .unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
    (dir, agent)
}

fn run(shell: &str, dir: &Path, words: &[&str]) -> Option<String> {
    let shell = PathBuf::from(shell);
    if !shell.exists() {
        return None;
    }
    let words: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
    let made = recipe(&shell, &words, &dir.join(&words[0]));
    assert!(made.through_shell);
    let out = Command::new(&made.program)
        .args(&made.args)
        .current_dir(dir)
        // The fixture's directory is on PATH only for the child, as a user's start-up file
        // would have put it there.
        .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
        .env("WYSS_FIXTURE", "from-the-environment")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn every_shell_family_passes_words_as_words_and_runs_the_agent() {
    let (dir, _agent) = fake_agent();
    // Words that would be damaged by being parsed as text: spaces, quotes, `$`, a glob, an
    // option-looking word, a semicolon, an empty word.
    let words = [
        "fake-agent",
        "--model",
        "sonnet 4",
        "it's \"quoted\"",
        "$HOME;touch wyss-injected",
        "*",
        "",
        "-n",
    ];
    let mut tried = 0;
    for shell in [
        "/usr/bin/sh",
        "/usr/bin/bash",
        "/usr/bin/zsh",
        "/usr/bin/fish",
    ] {
        let Some(out) = run(shell, &dir, &words) else {
            continue;
        };
        tried += 1;
        let expected = String::from(
            "argc=7\n[--model]\n[sonnet 4]\n[it's \"quoted\"]\n[$HOME;touch wyss-injected]\n[*]\n[]\n[-n]\n",
        );
        assert!(out.starts_with(&expected), "{shell}: {out:?}");
        assert!(out.contains("ENV=from-the-environment"), "{shell}: {out:?}");
        assert!(!dir.join("wyss-injected").exists(), "{shell}");
    }
    assert!(tried >= 2, "at least two shells are installed to try");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_agent_replaces_the_shell_so_the_terminals_process_is_the_agent() {
    // `exec` leaves one process: the pid the agent prints is the pid the shell had.
    let dir = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-recipe-pid-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&dir).unwrap();
    let agent = dir.join("who");
    fs::write(&agent, "#!/bin/sh\necho $PPID\n").unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
    for shell in ["/usr/bin/bash", "/usr/bin/zsh", "/usr/bin/fish"] {
        if !Path::new(shell).exists() {
            continue;
        }
        let made = recipe(Path::new(shell), &["who".to_owned()], &agent);
        let child = Command::new(&made.program)
            .args(&made.args)
            .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let started = child.id();
        let out = child.wait_with_output().unwrap();
        let parent: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
        // The agent's parent is the test, not a leftover shell: the shell was replaced.
        assert_eq!(parent, std::process::id(), "{shell} (started {started})");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_shell_without_a_recipe_starts_the_agent_directly_and_says_so() {
    assert_eq!(family(Path::new("/usr/bin/nu")), None);
    let made = recipe(
        Path::new("/usr/bin/nu"),
        &["claude".to_owned(), "--model".to_owned(), "x".to_owned()],
        Path::new("/home/u/.local/bin/claude"),
    );
    assert!(!made.through_shell);
    assert_eq!(made.program, Path::new("/home/u/.local/bin/claude"));
    assert_eq!(made.args, ["--model", "x"]);
    assert_eq!(family(Path::new("/bin/dash")), Some(Family::Posix));
}

fn bash_version_guard_fixture(tag: &str, shell_version: &str, on_start: &str) -> (PathBuf, std::process::Output) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-{tag}-{}", uuid::Uuid::new_v4().simple()));
    let (home, known, shell_path) = (root.join("home"), root.join("known"), root.join("shell"));
    for dir in [&home, &known, &shell_path] {
        fs::create_dir_all(dir).unwrap();
    }
    let shell = Path::new("/usr/bin/bash");
    assert!(shell.exists(), "the test requires the Bash recipe");
    fs::write(home.join(".bashrc"), "export PATH=\"$WYSS_SHELL_PATH:/usr/bin:/bin\"\n").unwrap();
    if shell_version != "2.1.309 (Claude Code)" {
        fs::write(home.join(".env"), "PATH=\"$WYSS_SHELL_PATH:/usr/bin:/bin\"; export PATH\n").unwrap();
    }
    for (dir, version) in [(&known, "2.1.309 (Claude Code)"), (&shell_path, shell_version)] {
        let path = dir.join("fake-cli");
        fs::write(
            &path,
            format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '{version}'; exit 0; fi\n{on_start}\n"),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let words = ["fake-cli", "--model", "sonnet"].map(str::to_owned);
    let made = recipe_with_version_guard(shell, &words, &known.join("fake-cli"), Some("2.1.309 (Claude Code)"));
    let output = Command::new(made.program)
        .args(made.args)
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", known.display()))
        .env("WYSS_SHELL_PATH", &shell_path)
        .env("WYSS_MARKER", root.join("provider-started"))
        .env("WYSS_NATIVE_VERSION_GUARD", root.join("version-verified"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (root, output)
}

#[test]
fn a_shell_startup_path_change_cannot_run_an_unverified_provider_build() {
    let (root, output) = bash_version_guard_fixture(
        "version-guard",
        "2.1.310 (Claude Code)",
        r#"printf started > "$WYSS_MARKER""#,
    );
    assert_eq!(output.status.code(), Some(125));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("provider build changed after preview"),
        "{:?}",
        output
    );
    assert!(
        !root.join("provider-started").exists(),
        "the mismatched CLI must not receive profile flags"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_matching_shell_resolved_cli_is_marked_before_that_same_binary_starts() {
    let (root, output) = bash_version_guard_fixture(
        "version-ok",
        "2.1.309 (Claude Code)",
        r#"printf '%s\n' "$@" > "$WYSS_MARKER""#,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(root.join("version-verified")).unwrap(), "ok\n");
    assert_eq!(fs::read_to_string(root.join("provider-started")).unwrap(), "--model\nsonnet\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_version_guard_runs_for_each_supported_shell_family() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "wyss-version-shells-{}",
            uuid::Uuid::new_v4().simple()
        ));
    let (home, known, shell_path, config) = (
        root.join("home"),
        root.join("known"),
        root.join("shell"),
        root.join("config"),
    );
    for dir in [&home, &known, &shell_path, &config.join("fish")] {
        fs::create_dir_all(dir).unwrap();
    }
    let started = root.join("provider-started");
    let verified = root.join("version-verified");
    for (dir, label) in [(&known, "known"), (&shell_path, "shell-path")] {
        let path = dir.join("fake-cli");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '2.1.309 (Claude Code)'; exit 0; fi\nprintf '%s\\n' '{label}' > \"$WYSS_MARKER\"\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        home.join(".bashrc"),
        "export PATH=\"$WYSS_SHELL_PATH:/usr/bin:/bin\"\n",
    )
    .unwrap();
    fs::write(
        home.join(".zshrc"),
        "export PATH=\"$WYSS_SHELL_PATH:/usr/bin:/bin\"\n",
    )
    .unwrap();
    fs::write(
        config.join("fish/config.fish"),
        "set -gx PATH \"$WYSS_SHELL_PATH\" /usr/bin /bin\n",
    )
    .unwrap();

    let mut tried = 0;
    for shell_name in ["sh", "bash", "zsh", "fish"] {
        let shell = PathBuf::from("/usr/bin").join(shell_name);
        if !shell.exists() {
            continue;
        }
        let made = recipe_with_version_guard(
            &shell,
            &[
                "fake-cli".to_owned(),
                "--model".to_owned(),
                "sonnet".to_owned(),
            ],
            &known.join("fake-cli"),
            Some("2.1.309 (Claude Code)"),
        );
        // POSIX shells differ in whether `-i -c` consults ENV; their inherited PATH is
        // still checked. The interactive rc mutation itself is covered by the Bash fixture.
        let path = if shell_name == "sh" {
            format!("{}:/usr/bin:/bin", shell_path.display())
        } else {
            format!("{}:/usr/bin:/bin", known.display())
        };
        let output = Command::new(made.program)
            .args(made.args)
            .env("HOME", &home)
            .env("ENV", home.join(".env"))
            .env("XDG_CONFIG_HOME", &config)
            .env("PATH", path)
            .env("WYSS_SHELL_PATH", &shell_path)
            .env("WYSS_MARKER", &started)
            .env("WYSS_NATIVE_VERSION_GUARD", &verified)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell_name}: {output:?}");
        assert_eq!(
            fs::read_to_string(&verified).unwrap(),
            "ok\n",
            "{shell_name}"
        );
        assert_eq!(
            fs::read_to_string(&started).unwrap(),
            "shell-path\n",
            "{shell_name} must execute the checked PATH entry"
        );
        fs::remove_file(&verified).unwrap();
        fs::remove_file(&started).unwrap();
        tried += 1;
    }
    assert!(tried >= 2, "at least two shell families must be installed");
    fs::remove_dir_all(root).unwrap();
}

fn words(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_owned()).collect()
}

#[test]
fn a_pinned_recipe_puts_the_variable_in_argv_and_ends_in_exec_env() {
    let w = words(&["claude", "--model", "x"]);
    let direct = Path::new("/bin/claude");
    for shell in [
        "/usr/bin/bash",
        "/usr/bin/zsh",
        "/usr/bin/sh",
        "/usr/bin/fish",
    ] {
        let shell = Path::new(shell);
        let made = recipe_pinned(
            shell,
            &w,
            direct,
            Some("1.0"),
            Some(("CODEX_HOME", "/launch/home")),
        );
        assert!(made.through_shell, "{shell:?}");
        // Order of the words after the script label: command, version, pin, the rest.
        let n = made.args.len();
        assert_eq!(
            &made.args[n - 5..],
            ["claude", "1.0", "CODEX_HOME=/launch/home", "--model", "x"],
            "{shell:?}"
        );
        let script = &made.args[2];
        assert!(script.contains("exec /usr/bin/env"), "{shell:?}: {script}");
        // The variable is one argv word, never text inside the script.
        assert!(!script.contains("/launch/home"), "{shell:?}");
    }
}

#[test]
fn an_unpinned_recipe_is_the_same_text_as_the_version_guard() {
    for shell in [
        "/usr/bin/bash",
        "/usr/bin/zsh",
        "/usr/bin/sh",
        "/usr/bin/fish",
    ] {
        let shell = Path::new(shell);
        let w = words(&["claude", "--model", "x"]);
        let direct = Path::new("/bin/claude");
        assert_eq!(
            recipe_pinned(shell, &w, direct, Some("1.0"), None),
            recipe_with_version_guard(shell, &w, direct, Some("1.0")),
            "{shell:?}"
        );
        // The script does not exec env when nothing is pinned.
        let made = recipe_pinned(shell, &w, direct, Some("1.0"), None);
        assert!(!made.args[2].contains("/usr/bin/env"), "{shell:?}");
        // And without a version either: the plain recipe.
        assert_eq!(
            recipe_pinned(shell, &w, direct, None, None),
            recipe(shell, &w, direct),
            "{shell:?}"
        );
    }
}

#[test]
fn without_a_version_the_pinned_words_start_with_env_and_the_variable() {
    let w = words(&["codex", "--flag"]);
    let direct = Path::new("/bin/codex");
    let shell = Path::new("/usr/bin/bash");
    let made = recipe_pinned(shell, &w, direct, None, Some(("CODEX_HOME", "/h")));
    let plain = recipe(
        shell,
        &words(&["/usr/bin/env", "CODEX_HOME=/h", "codex", "--flag"]),
        direct,
    );
    assert_eq!(made, plain);
    assert_eq!(
        &made.args[made.args.len() - 4..],
        ["/usr/bin/env", "CODEX_HOME=/h", "codex", "--flag"]
    );
}

#[test]
fn a_shell_without_a_recipe_ignores_the_pin() {
    let w = words(&["claude", "--model", "x"]);
    let direct = Path::new("/home/u/.local/bin/claude");
    let nu = Path::new("/usr/bin/nu");
    let plain = recipe(nu, &w, direct);
    assert_eq!(
        recipe_pinned(nu, &w, direct, Some("1.0"), Some(("CODEX_HOME", "/h"))),
        plain
    );
    assert_eq!(
        recipe_pinned(nu, &w, direct, None, Some(("CODEX_HOME", "/h"))),
        plain
    );
    assert!(!plain.through_shell);
}

/// What a fake agent prints as `CODEX_HOME` after a start-up file exported the owner's, with or
/// without a pin. `None` when the shell is not installed (said on stderr, the test skips it).
fn codex_home_seen(shell_name: &str, pin: Option<&str>, versioned: bool) -> Option<String> {
    let shell = PathBuf::from("/usr/bin").join(shell_name);
    if !shell.exists() {
        eprintln!("skipped: {shell_name} is not installed");
        return None;
    }
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-pin-{}", uuid::Uuid::new_v4().simple()));
    let (home, bin, config) = (root.join("home"), root.join("bin"), root.join("config"));
    for dir in [&home, &bin, &config.join("fish")] {
        fs::create_dir_all(dir).unwrap();
    }
    let agent = bin.join("fake-agent");
    fs::write(
        &agent,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 1.0; exit 0; fi\nprintf 'CODEX_HOME=[%s]\\n' \"$CODEX_HOME\"\n",
    )
    .unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
    // The owner's start-up file exports the owner's folder, as a real one could.
    let export = "export CODEX_HOME=/owner\n";
    fs::write(home.join(".bashrc"), export).unwrap();
    fs::write(home.join(".zshrc"), export).unwrap();
    fs::write(home.join(".env"), export).unwrap();
    fs::write(
        config.join("fish/config.fish"),
        "set -gx CODEX_HOME /owner\n",
    )
    .unwrap();
    let made = recipe_pinned(
        &shell,
        &words(&["fake-agent"]),
        &agent,
        versioned.then_some("1.0"),
        pin.map(|dir| ("CODEX_HOME", dir)),
    );
    let output = Command::new(&made.program)
        .args(&made.args)
        .env("HOME", &home)
        .env("ENV", home.join(".env"))
        .env("XDG_CONFIG_HOME", &config)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("WYSS_NATIVE_VERSION_GUARD", root.join("verified"))
        .env_remove("CODEX_HOME")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(output.status.success(), "{shell_name}: {output:?}");
    fs::remove_dir_all(root).unwrap();
    Some(out)
}

#[test]
fn a_startup_file_that_exports_the_owners_folder_does_not_beat_the_pin() {
    let mut tried = 0;
    for shell in ["bash", "zsh", "fish"] {
        for versioned in [true, false] {
            let Some(pinned) = codex_home_seen(shell, Some("/launch/home"), versioned) else {
                continue;
            };
            assert_eq!(pinned, "CODEX_HOME=[/launch/home]\n", "{shell} {versioned}");
            // Negative control: the same start-up file without the pin wins, so the pin is
            // what made the difference.
            let owner = codex_home_seen(shell, None, versioned).unwrap();
            assert_eq!(owner, "CODEX_HOME=[/owner]\n", "{shell} {versioned}");
        }
        tried += 1;
    }
    assert!(tried >= 1, "no shell family installed to try");
}

/// A checked launch with the sign-in token through its file, run for real through `shell`.
/// Returns what the agent printed, the version probe's view of the token, the exit status, and
/// whether the file is still there. `None` when that shell is not installed here.
fn secret_launch(shell: &str, lock_folder: bool) -> Option<(String, String, Option<i32>, bool)> {
    let shell = PathBuf::from(shell);
    if !shell.exists() {
        return None;
    }
    let (dir, _) = fake_agent();
    let agent = dir.join("token-agent");
    let probe = dir.join("probe");
    fs::write(
        &agent,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'probe=[%s]' \"$CLAUDE_CODE_OAUTH_TOKEN\" > '{}'; echo 9.9; exit 0; fi\nprintf 'TOKEN=[%s]\\n' \"$CLAUDE_CODE_OAUTH_TOKEN\"\n",
            probe.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
    let held = dir.join("held");
    fs::create_dir(&held).unwrap();
    let token = held.join("oauth-token");
    fs::write(&token, "fixture-token-1\n").unwrap();
    if lock_folder {
        // Readable, but nothing in it can be removed.
        fs::set_permissions(&held, fs::Permissions::from_mode(0o500)).unwrap();
    }
    let words = vec!["token-agent".to_owned()];
    let made = super::recipe::recipe_pinned_secret(&shell, &words, &agent, Some("9.9"), None, true);
    let out = Command::new(&made.program)
        .args(&made.args)
        .current_dir(&dir)
        .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
        .env("HOME", &dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("WYSS_NATIVE_TOKEN_FILE", &token)
        .env("WYSS_NATIVE_VERSION_GUARD", dir.join("marker"))
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let still = token.exists();
    let _ = fs::set_permissions(&held, fs::Permissions::from_mode(0o700));
    let seen = fs::read_to_string(&probe).unwrap_or_default();
    let printed = String::from_utf8_lossy(&out.stdout).into_owned();
    let _ = fs::remove_dir_all(&dir);
    Some((printed, seen, out.status.code(), still))
}

#[test]
fn the_token_file_is_read_and_removed_before_the_version_check_and_reaches_only_the_agent() {
    for shell in ["/bin/sh", "/usr/bin/bash", "/usr/bin/zsh", "/usr/bin/fish"] {
        let Some((printed, probe, code, still)) = secret_launch(shell, false) else {
            eprintln!("{shell} is not installed here; skipped");
            continue;
        };
        assert!(printed.contains("TOKEN=[fixture-token-1]"), "{shell}: {printed:?}");
        assert_eq!(probe, "probe=[]", "{shell}: the version check saw the token");
        assert_eq!(code, Some(0), "{shell}");
        assert!(!still, "{shell}: the token file is left");
    }
}

#[test]
fn a_token_file_that_cannot_be_removed_starts_no_agent() {
    for shell in ["/bin/sh", "/usr/bin/bash", "/usr/bin/fish"] {
        let Some((printed, probe, code, _)) = secret_launch(shell, true) else {
            eprintln!("{shell} is not installed here; skipped");
            continue;
        };
        assert_eq!(code, Some(125), "{shell}: {printed:?}");
        assert!(!printed.contains("TOKEN="), "{shell}: the agent ran");
        assert!(probe.is_empty(), "{shell}: the version check ran first");
    }
}
