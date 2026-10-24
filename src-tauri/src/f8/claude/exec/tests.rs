use super::*;
use std::os::unix::fs::{DirBuilderExt, symlink};

fn workspace() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "wyss-claude-exec-{}",
        crate::control::store::uuid().as_str()
    ));
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    fs::canonicalize(&p).unwrap()
}
fn cmd(ws: &Path, command: &str) -> Approved {
    Approved::Command(ApprovedCommand {
        command: command.into(),
        cwd: ws.to_str().unwrap().into(),
        workspace: ws.to_str().unwrap().into(),
    })
}
fn finish(e: &mut SandboxExecutor) -> Outcome {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(o) = e.poll(Instant::now()) {
            return o;
        }
        assert!(Instant::now() < end, "executor never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn reap(e: &mut SandboxExecutor) {
    e.begin_stop(Instant::now());
    let end = Instant::now() + Duration::from_secs(15);
    while e.poll_stop(Instant::now()) != StopState::Reaped {
        assert!(Instant::now() < end, "never reaped");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn run(ws: &Path, command: &str) -> Outcome {
    let mut e = SandboxExecutor::new();
    e.dispatch(cmd(ws, command)).unwrap();
    let o = finish(&mut e);
    reap(&mut e);
    o
}
fn text(o: Outcome) -> String {
    match o {
        Outcome::Completed(t) => t,
        other => panic!("{other:?}"),
    }
}
fn sha(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}
fn write(ws: &Path, rel: &str, content: &str, expected: Option<[u8; 32]>) -> Outcome {
    SandboxExecutor::write_file(ApprovedWrite {
        path: ws.join(rel).to_str().unwrap().into(),
        workspace: ws.to_str().unwrap().into(),
        content: content.into(),
        expected_old: expected,
    })
}

#[test]
fn a_command_runs_and_its_output_and_status_are_reported() {
    let ws = workspace();
    let t = text(run(&ws, "echo hello; echo oops 1>&2; exit 3"));
    assert_eq!(t, "exit status 3\nhello\noops\n");
}

#[test]
fn the_workspace_is_writable_and_the_effect_lands_on_the_host() {
    let ws = workspace();
    text(run(&ws, "echo made > f.txt && mkdir d && echo x > d/y"));
    assert_eq!(fs::read_to_string(ws.join("f.txt")).unwrap(), "made\n");
    assert!(ws.join("d/y").exists());
}

#[test]
fn the_environment_is_empty_and_the_home_and_host_network_are_not_there() {
    let ws = workspace();
    let env = text(run(&ws, "env | sort"));
    for forbidden in [
        "ANTHROPIC",
        "SSH",
        "DISPLAY",
        "DBUS",
        "XDG",
        "WAYLAND",
        "TOKEN",
        "KEY",
    ] {
        assert!(!env.contains(forbidden), "{forbidden}: {env}");
    }
    assert!(env.contains("PATH=/usr/bin:/bin"));
    let home = text(run(&ws, "ls /home 2>&1; ls ~/.ssh ~/.claude ~/.codex 2>&1"));
    assert!(!home.contains("saintwyss"), "{home}");
    // A listener on the host loopback is unreachable from the command's own network namespace.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let probe = format!(
        "/usr/bin/python3 -I -c \"import socket; s=socket.socket(); s.settimeout(2); s.connect(('127.0.0.1',{port}))\" 2>&1 | tail -1; echo done"
    );
    let t = text(run(&ws, &probe));
    assert!(
        t.contains("done")
            && (t.contains("refused") || t.contains("unreachable") || t.contains("Error")),
        "{t}"
    );
}

#[test]
fn protected_names_stay_read_only_inside_the_writable_workspace() {
    let ws = workspace();
    fs::create_dir(ws.join(".git")).unwrap();
    fs::write(ws.join(".git/config"), "[core]\n").unwrap();
    let t = text(run(
        &ws,
        "echo hook > .git/pre-commit 2>&1; echo $?; echo ok > normal.txt; echo $?",
    ));
    assert!(
        t.contains("Read-only file system") || t.contains("denied"),
        "{t}"
    );
    assert!(!ws.join(".git/pre-commit").exists());
    assert!(ws.join("normal.txt").exists());
}

#[test]
fn output_is_bounded_and_marked_truncated() {
    let ws = workspace();
    let t = text(run(&ws, "yes aaaaaaaaaaaaaaaaaaaa | head -c 2000000"));
    assert!(
        t.starts_with("exit status 0 (output truncated)\n"),
        "{}",
        &t[..60]
    );
    assert!(t.len() <= MAX_OUTPUT + 64);
}

#[test]
fn a_command_that_outlives_its_deadline_is_killed_and_says_so() {
    let ws = workspace();
    let mut e = SandboxExecutor::with_timeout(Duration::from_secs(1));
    e.dispatch(cmd(&ws, "sleep 30; echo late > late.txt"))
        .unwrap();
    let t = text(finish(&mut e));
    reap(&mut e);
    assert!(t.starts_with("timed out and was killed"), "{t}");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!ws.join("late.txt").exists());
}

#[test]
fn a_cwd_outside_the_workspace_or_through_a_link_is_refused_before_anything_starts() {
    let ws = workspace();
    let other = workspace();
    let mut e = SandboxExecutor::new();
    let bad = Approved::Command(ApprovedCommand {
        command: "true".into(),
        cwd: other.to_str().unwrap().into(),
        workspace: ws.to_str().unwrap().into(),
    });
    assert_eq!(e.dispatch(bad), Err(E::GateDenied));
    symlink(&other, ws.join("link")).unwrap();
    let linked = Approved::Command(ApprovedCommand {
        command: "true".into(),
        cwd: ws.join("link").to_str().unwrap().into(),
        workspace: ws.to_str().unwrap().into(),
    });
    assert_eq!(e.dispatch(linked), Err(E::GateDenied));
    assert!(!e.busy());
}

#[test]
fn only_one_dispatch_at_a_time() {
    let ws = workspace();
    let mut e = SandboxExecutor::new();
    e.dispatch(cmd(&ws, "sleep 5")).unwrap();
    assert!(e.busy());
    assert_eq!(
        e.dispatch(cmd(&ws, "echo second > second.txt")),
        Err(E::ActiveTurnExists)
    );
    reap(&mut e);
    assert!(!ws.join("second.txt").exists());
}

#[test]
fn stopping_kills_a_running_command_and_proves_the_reap() {
    let ws = workspace();
    let mut e = SandboxExecutor::new();
    e.dispatch(cmd(&ws, "sleep 2; echo survived > s.txt"))
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    reap(&mut e);
    assert!(!e.busy());
    std::thread::sleep(Duration::from_millis(2300));
    assert!(!ws.join("s.txt").exists());
}

#[test]
fn a_new_file_is_written_exactly_and_atomically() {
    let ws = workspace();
    let o = write(&ws, "notes.txt", "line one\nline two\n", None);
    assert!(matches!(o, Outcome::Completed(_)), "{o:?}");
    assert_eq!(
        fs::read_to_string(ws.join("notes.txt")).unwrap(),
        "line one\nline two\n"
    );
    assert_eq!(
        fs::metadata(ws.join("notes.txt")).unwrap().mode() & 0o777,
        0o644
    );
    let leftovers: Vec<_> = fs::read_dir(&ws)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
}

#[test]
fn an_update_requires_the_file_to_be_exactly_what_the_preview_was_made_from() {
    let ws = workspace();
    fs::write(ws.join("a.txt"), "old\n").unwrap();
    fs::set_permissions(ws.join("a.txt"), fs::Permissions::from_mode(0o600)).unwrap();
    // Stale: the preview was computed against other bytes.
    let o = write(&ws, "a.txt", "new\n", Some(sha(b"something else\n")));
    assert!(matches!(o, Outcome::Failed(_)), "{o:?}");
    assert_eq!(fs::read_to_string(ws.join("a.txt")).unwrap(), "old\n");
    // Current: written, mode kept.
    let o = write(&ws, "a.txt", "new\n", Some(sha(b"old\n")));
    assert!(matches!(o, Outcome::Completed(_)), "{o:?}");
    assert_eq!(fs::read_to_string(ws.join("a.txt")).unwrap(), "new\n");
    assert_eq!(
        fs::metadata(ws.join("a.txt")).unwrap().mode() & 0o777,
        0o600
    );
}

#[test]
fn creating_over_an_existing_file_and_updating_a_missing_one_both_write_nothing() {
    let ws = workspace();
    fs::write(ws.join("a.txt"), "old\n").unwrap();
    assert!(matches!(write(&ws, "a.txt", "x", None), Outcome::Failed(_)));
    assert_eq!(fs::read_to_string(ws.join("a.txt")).unwrap(), "old\n");
    assert!(matches!(
        write(&ws, "missing.txt", "x", Some(sha(b"old\n"))),
        Outcome::Failed(_)
    ));
    assert!(!ws.join("missing.txt").exists());
}

#[test]
fn writes_outside_the_workspace_through_links_or_into_protected_names_are_refused() {
    let ws = workspace();
    let other = workspace();
    fs::write(other.join("victim.txt"), "keep\n").unwrap();
    symlink(&other, ws.join("out")).unwrap();
    symlink(other.join("victim.txt"), ws.join("victim-link")).unwrap();
    fs::create_dir(ws.join(".git")).unwrap();
    for rel in [
        "out/victim.txt",
        "victim-link",
        ".git/config",
        ".git/hooks/pre-commit",
        ".claude/settings.json",
        ".codex/config.toml",
        ".agents/x",
    ] {
        assert!(
            matches!(write(&ws, rel, "pwned", None), Outcome::Failed(_)),
            "{rel}"
        );
        assert!(
            matches!(
                write(&ws, rel, "pwned", Some(sha(b"keep\n"))),
                Outcome::Failed(_)
            ),
            "{rel}"
        );
    }
    assert_eq!(
        fs::read_to_string(other.join("victim.txt")).unwrap(),
        "keep\n"
    );
    assert!(!ws.join(".git/config").exists());
    // A path that was never inside the workspace.
    let o = SandboxExecutor::write_file(ApprovedWrite {
        path: other.join("new.txt").to_str().unwrap().into(),
        workspace: ws.to_str().unwrap().into(),
        content: "x".into(),
        expected_old: None,
    });
    assert!(matches!(o, Outcome::Failed(_)));
    assert!(!other.join("new.txt").exists());
}

#[test]
fn the_protected_name_check_is_by_component_not_by_substring() {
    assert!(touches_protected("/w", "/w/.claude/settings.json"));
    assert!(touches_protected("/w", "/w/a/.git/hooks/x"));
    assert!(!touches_protected("/w", "/w/.claude-notes.md"));
    assert!(!touches_protected("/w", "/w/a.git/x"));
    // A path outside the workspace is never "not protected".
    assert!(touches_protected("/w", "/other/x"));
}

#[test]
fn a_hard_linked_file_is_not_a_plain_target() {
    let ws = workspace();
    let other = workspace();
    fs::write(other.join("v"), "keep\n").unwrap();
    fs::hard_link(other.join("v"), ws.join("h")).unwrap();
    assert!(matches!(
        write(&ws, "h", "x", Some(sha(b"keep\n"))),
        Outcome::Failed(_)
    ));
    assert_eq!(fs::read_to_string(other.join("v")).unwrap(), "keep\n");
}

#[test]
fn a_write_through_dispatch_is_delivered_by_poll_and_leaves_the_executor_idle() {
    let ws = workspace();
    let mut e = SandboxExecutor::new();
    e.dispatch(Approved::Write(ApprovedWrite {
        path: ws.join("f.txt").to_str().unwrap().into(),
        workspace: ws.to_str().unwrap().into(),
        content: "hi\n".into(),
        expected_old: None,
    }))
    .unwrap();
    assert!(e.busy());
    assert!(matches!(
        e.poll(Instant::now()),
        Some(Outcome::Completed(_))
    ));
    assert!(!e.busy());
    assert_eq!(e.poll_stop(Instant::now()), StopState::Reaped);
}

#[test]
fn the_result_text_is_strict() {
    for bad in [
        json!({}),
        json!({"output":1}),
        json!({"output":"x","truncated":false}),
        json!({"output":"x","truncated":false,"timed_out":false}),
    ] {
        assert_eq!(SandboxExecutor::result_text(&bad), None, "{bad}");
    }
    assert_eq!(
        SandboxExecutor::result_text(
            &json!({"output":"o","truncated":false,"timed_out":false,"exit":null,"signal":9})
        )
        .unwrap(),
        "killed by signal 9\no"
    );
}
