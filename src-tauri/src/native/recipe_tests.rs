//! OQ-07: for each shell family, the arguments arrive as the words they were, and the agent is
//! the process that runs. A fake agent prints what it received; each recipe is run for real
//! through the shells installed here.
use super::recipe::{Family, family, recipe};
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
