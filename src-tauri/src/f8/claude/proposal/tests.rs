use super::*;
use std::os::unix::fs::{DirBuilderExt, symlink};

fn workspace() -> String {
    let p = std::env::temp_dir().join(format!(
        "wyss-claude-prop-{}",
        crate::control::store::uuid().as_str()
    ));
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    fs::canonicalize(&p).unwrap().to_str().unwrap().to_owned()
}
fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect()
}
fn fingerprint(p: &Prepared) -> String {
    p.action.metadata("claude-act-1").unwrap().1
}

#[test]
fn a_command_becomes_the_exact_command_in_the_workspace() {
    let ws = workspace();
    let p = prepare(
        &ws,
        "run_command",
        &args(&[("command", "ls -la | wc -l")]),
        "claude-act-1",
    )
    .unwrap();
    let Approved::Command(c) = &p.approved else {
        panic!()
    };
    assert_eq!(
        (c.command.as_str(), c.cwd.as_str(), c.workspace.as_str()),
        ("ls -la | wc -l", ws.as_str(), ws.as_str())
    );
    let (kind, _, structure) = p.action.metadata("claude-act-1").unwrap();
    assert_eq!(
        kind,
        crate::contracts::control::ApprovalKind::CommandExecution
    );
    assert_eq!(structure.command_bytes as usize, "ls -la | wc -l".len());
}

#[test]
fn a_different_command_has_a_different_fingerprint() {
    let ws = workspace();
    let a = prepare(
        &ws,
        "run_command",
        &args(&[("command", "ls")]),
        "claude-act-1",
    )
    .unwrap();
    let b = prepare(
        &ws,
        "run_command",
        &args(&[("command", "ls ")]),
        "claude-act-1",
    )
    .unwrap();
    assert_ne!(fingerprint(&a), fingerprint(&b));
    // The same text under another item is another approval, so an answer cannot be replayed.
    assert_ne!(
        a.action.metadata("claude-act-1").unwrap().1,
        a.action.metadata("claude-act-2").unwrap().1
    );
}

#[test]
fn malformed_commands_are_refused_without_a_question() {
    let ws = workspace();
    for (a, why) in [
        (args(&[]), Refuse::BadArguments),
        (args(&[("command", "")]), Refuse::BadArguments),
        (args(&[("command", "   ")]), Refuse::BadArguments),
        (args(&[("command", "ls\0id")]), Refuse::BadArguments),
        (
            args(&[("command", "ls"), ("cwd", "/")]),
            Refuse::BadArguments,
        ),
        (args(&[("cmd", "ls")]), Refuse::BadArguments),
        (
            args(&[("command", &"a".repeat(MAX_COMMAND_BYTES + 1))]),
            Refuse::TooLarge,
        ),
    ] {
        assert_eq!(
            prepare(&ws, "run_command", &a, "claude-act-1").err(),
            Some(why),
            "{a:?}"
        );
    }
    assert_eq!(
        prepare(
            &ws,
            "run_command",
            &args(&[("command", &"a".repeat(MAX_COMMAND_BYTES))]),
            "claude-act-1"
        )
        .err(),
        None
    );
}

#[test]
fn an_unknown_tool_or_a_broken_workspace_is_refused() {
    let ws = workspace();
    assert_eq!(
        prepare(&ws, "Bash", &args(&[("command", "id")]), "claude-act-1").err(),
        Some(Refuse::UnknownTool)
    );
    assert_eq!(
        prepare(
            "/nonexistent-workspace",
            "run_command",
            &args(&[("command", "id")]),
            "claude-act-1"
        )
        .err(),
        Some(Refuse::OutOfScope)
    );
}

#[test]
fn a_new_file_is_shown_as_an_addition_with_its_whole_content() {
    let ws = workspace();
    let p = prepare(
        &ws,
        "write_file",
        &args(&[("path", "notes.txt"), ("content", "one\ntwo\n")]),
        "claude-act-1",
    )
    .unwrap();
    let Approved::Write(w) = &p.approved else {
        panic!()
    };
    assert_eq!(w.path, format!("{ws}/notes.txt"));
    assert_eq!((w.content.as_str(), w.expected_old), ("one\ntwo\n", None));
    let shown = serde_json::to_value(&p.action).unwrap();
    assert_eq!(shown["changes"][0]["kind"], "add");
    let d = shown["changes"][0]["diff"].as_str().unwrap();
    assert!(
        d.contains("+one\n+two\n") && d.starts_with("--- /dev/null"),
        "{d}"
    );
}

#[test]
fn an_existing_file_is_shown_as_a_diff_against_exactly_its_current_bytes() {
    let ws = workspace();
    fs::write(format!("{ws}/a.txt"), "x\ny\nz\n").unwrap();
    let p = prepare(
        &ws,
        "write_file",
        &args(&[("path", "a.txt"), ("content", "x\nY\nz\n")]),
        "claude-act-1",
    )
    .unwrap();
    let Approved::Write(w) = &p.approved else {
        panic!()
    };
    assert_eq!(
        w.expected_old,
        Some(<[u8; 32]>::from(Sha256::digest(b"x\ny\nz\n")))
    );
    let shown = serde_json::to_value(&p.action).unwrap();
    assert_eq!(shown["changes"][0]["kind"], "update");
    assert!(
        shown["changes"][0]["diff"]
            .as_str()
            .unwrap()
            .contains("-y\n+Y\n")
    );
}

#[test]
fn absolute_and_relative_spellings_of_one_file_are_one_action() {
    let ws = workspace();
    let a = prepare(
        &ws,
        "write_file",
        &args(&[("path", "d/./f.txt"), ("content", "c")]),
        "claude-act-1",
    )
    .unwrap();
    let b = prepare(
        &ws,
        "write_file",
        &args(&[("path", &format!("{ws}/d/f.txt")), ("content", "c")]),
        "claude-act-1",
    )
    .unwrap();
    assert_eq!(fingerprint(&a), fingerprint(&b));
}

#[test]
fn paths_that_leave_the_workspace_or_touch_protected_names_are_refused() {
    let ws = workspace();
    let other = workspace();
    fs::create_dir(format!("{ws}/.git")).unwrap();
    symlink(&other, format!("{ws}/out")).unwrap();
    for path in [
        "../escape.txt",
        "d/../../escape.txt",
        "/etc/passwd",
        &format!("{other}/x"),
        ".git/hooks/pre-commit",
        ".git/config",
        ".claude/settings.json",
        ".codex/config.toml",
        "out/x",
        "/",
    ] {
        let r = prepare(
            &ws,
            "write_file",
            &args(&[("path", path), ("content", "x")]),
            "claude-act-1",
        );
        assert_eq!(r.err(), Some(Refuse::OutOfScope), "{path}");
    }
}

#[test]
fn only_plain_utf8_files_within_bounds_can_be_changed() {
    let ws = workspace();
    let other = workspace();
    fs::write(format!("{other}/v"), "keep").unwrap();
    fs::hard_link(format!("{other}/v"), format!("{ws}/hard")).unwrap();
    fs::create_dir(format!("{ws}/dir")).unwrap();
    fs::write(format!("{ws}/bin"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
    fs::write(
        format!("{ws}/big"),
        vec![b'a'; (MAX_EXISTING_BYTES + 1) as usize],
    )
    .unwrap();
    fs::write(format!("{ws}/same"), "same\n").unwrap();
    for (path, content, why) in [
        ("hard", "x", Refuse::OutOfScope),
        ("dir", "x", Refuse::OutOfScope),
        ("bin", "x", Refuse::NotText),
        ("big", "x", Refuse::TooLarge),
        ("same", "same\n", Refuse::NoChange),
    ] {
        assert_eq!(
            prepare(
                &ws,
                "write_file",
                &args(&[("path", path), ("content", content)]),
                "claude-act-1"
            )
            .err(),
            Some(why),
            "{path}"
        );
    }
    assert_eq!(fs::read_to_string(format!("{other}/v")).unwrap(), "keep");
}

#[test]
fn oversized_or_malformed_writes_are_refused_never_truncated() {
    let ws = workspace();
    for (a, why) in [
        (args(&[("path", "f")]), Refuse::BadArguments),
        (args(&[("content", "c")]), Refuse::BadArguments),
        (
            args(&[("path", ""), ("content", "c")]),
            Refuse::BadArguments,
        ),
        (
            args(&[("path", "f"), ("content", "a\0b")]),
            Refuse::BadArguments,
        ),
        (
            args(&[("path", "f"), ("content", "c"), ("mode", "755")]),
            Refuse::BadArguments,
        ),
        (
            args(&[
                ("path", "f"),
                ("content", &"x".repeat(MAX_CONTENT_BYTES + 1)),
            ]),
            Refuse::TooLarge,
        ),
    ] {
        assert_eq!(
            prepare(&ws, "write_file", &a, "claude-act-1").err(),
            Some(why),
            "{a:?}"
        );
    }
    let long_path = format!("{}/f", "d/".repeat(600));
    assert_eq!(
        prepare(
            &ws,
            "write_file",
            &args(&[("path", &long_path), ("content", "c")]),
            "claude-act-1"
        )
        .err(),
        Some(Refuse::OutOfScope)
    );
    assert!(
        prepare(
            &ws,
            "write_file",
            &args(&[
                ("path", "f"),
                ("content", &"x\n".repeat(MAX_CONTENT_BYTES / 2))
            ]),
            "claude-act-1"
        )
        .is_ok()
    );
}

#[test]
fn model_text_never_names_a_path_or_content() {
    for r in [
        Refuse::UnknownTool,
        Refuse::BadArguments,
        Refuse::TooLarge,
        Refuse::OutOfScope,
        Refuse::NotAPlainFile,
        Refuse::NotText,
        Refuse::NoChange,
    ] {
        let t = r.model_text();
        assert!(
            t.starts_with("refused:") && t.contains("nothing was"),
            "{t}"
        );
        assert!(!t.contains('/'), "{t}");
    }
}
