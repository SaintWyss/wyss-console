//! OQ-07: for each shell family, the arguments arrive as the words they were, and the agent is
//! the process that runs. A fake agent prints what it received; each recipe is run for real
//! through the shells installed here.
use super::recipe::{Family, family, recipe, recipe_with_version_guard};
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
        "$HOME;touch /tmp/wyss-injected",
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
            "argc=7\n[--model]\n[sonnet 4]\n[it's \"quoted\"]\n[$HOME;touch /tmp/wyss-injected]\n[*]\n[]\n[-n]\n",
        );
        assert!(out.starts_with(&expected), "{shell}: {out:?}");
        assert!(out.contains("ENV=from-the-environment"), "{shell}: {out:?}");
        assert!(!Path::new("/tmp/wyss-injected").exists(), "{shell}");
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

#[test]
fn a_shell_startup_path_change_cannot_run_an_unverified_provider_build() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "wyss-version-guard-{}",
            uuid::Uuid::new_v4().simple()
        ));
    let (home, known, shell_path) = (root.join("home"), root.join("known"), root.join("shell"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&known).unwrap();
    fs::create_dir_all(&shell_path).unwrap();
    let shell = Path::new("/usr/bin/bash");
    assert!(shell.exists(), "the test requires the Bash recipe");
    fs::write(
        home.join(".bashrc"),
        "export PATH=\"$WYSS_SHELL_PATH:/usr/bin:/bin\"\n",
    )
    .unwrap();
    fs::write(
        home.join(".env"),
        "PATH=\"$WYSS_SHELL_PATH:/usr/bin:/bin\"; export PATH\n",
    )
    .unwrap();
    let marker = root.join("provider-was-started");
    let verified = root.join("version-verified");
    for (path, version) in [
        (known.join("fake-cli"), "2.1.309 (Claude Code)"),
        (shell_path.join("fake-cli"), "2.1.310 (Claude Code)"),
    ] {
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '{version}'; exit 0; fi\nprintf started > \"$WYSS_MARKER\"\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let words = vec![
        "fake-cli".to_owned(),
        "--model".to_owned(),
        "sonnet".to_owned(),
    ];
    let made = recipe_with_version_guard(
        shell,
        &words,
        &known.join("fake-cli"),
        Some("2.1.309 (Claude Code)"),
    );
    let output = Command::new(made.program)
        .args(made.args)
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", known.display()))
        .env("WYSS_SHELL_PATH", &shell_path)
        .env("WYSS_MARKER", &marker)
        .env("WYSS_NATIVE_VERSION_GUARD", &verified)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(125));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("provider build changed after preview"),
        "{:?}",
        output
    );
    assert!(
        !marker.exists(),
        "the mismatched CLI must not receive profile flags"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_matching_shell_resolved_cli_is_marked_before_that_same_binary_starts() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-version-ok-{}", uuid::Uuid::new_v4().simple()));
    let (known, shell_path) = (root.join("known"), root.join("shell"));
    fs::create_dir_all(&known).unwrap();
    fs::create_dir_all(&shell_path).unwrap();
    let shell = Path::new("/usr/bin/bash");
    assert!(shell.exists(), "the test requires the Bash recipe");
    let marker = root.join("provider-started");
    let verified = root.join("version-verified");
    for dir in [&known, &shell_path] {
        let path = dir.join("fake-cli");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '2.1.309 (Claude Code)'; exit 0; fi\nprintf '%s\\n' \"$@\" > \"$WYSS_MARKER\"\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join(".bashrc"),
        "export PATH=\"$WYSS_SHELL_PATH:/usr/bin:/bin\"\n",
    )
    .unwrap();
    let words = vec![
        "fake-cli".to_owned(),
        "--model".to_owned(),
        "sonnet".to_owned(),
    ];
    let made = recipe_with_version_guard(
        shell,
        &words,
        &known.join("fake-cli"),
        Some("2.1.309 (Claude Code)"),
    );
    let output = Command::new(made.program)
        .args(made.args)
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", known.display()))
        .env("WYSS_SHELL_PATH", &shell_path)
        .env("WYSS_MARKER", &marker)
        .env("WYSS_NATIVE_VERSION_GUARD", &verified)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(verified).unwrap(), "ok\n");
    assert_eq!(fs::read_to_string(marker).unwrap(), "--model\nsonnet\n");
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
