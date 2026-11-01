use super::*;

fn places() -> Places {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-launch-{}", uuid::Uuid::new_v4().simple()));
    let home = root.join("home");
    fs::create_dir_all(home.join("work")).unwrap();
    Places {
        folder: home.join(".config/wyss-console/launch"),
        home,
    }
}

fn write(places: &Places, name: &str, text: &str) {
    fs::create_dir_all(&places.folder).unwrap();
    fs::write(places.folder.join(name), text).unwrap();
}

#[test]
fn a_missing_folder_is_made_once_with_the_shipped_launches() {
    let places = places();
    let list = list(&places).unwrap();
    assert_eq!(list.folder, "~/.config/wyss-console/launch");
    assert!(list.refused.is_empty(), "{:?}", list.refused);
    let names: Vec<&str> = list.launches.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Actualizar sistema", "Claude Code", "Codex"]);
    let update = &list.launches[0];
    assert_eq!(
        update.kind,
        Kind::Commands {
            commands: vec!["sysupdate".into()]
        }
    );
    assert_eq!(update.directory, places.home.to_string_lossy());
    for (launch, agent) in [
        (&list.launches[1], "claude_code"),
        (&list.launches[2], "codex"),
    ] {
        let Kind::Session {
            agent: a,
            profile,
            before,
        } = &launch.kind
        else {
            panic!("{launch:?}")
        };
        assert_eq!(a, agent);
        assert_eq!(profile.as_deref(), Some("Full access (bypass)"));
        assert_eq!(before.len(), 1);
        assert!(before[0].contains("rest/db/scan"));
    }
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(&places.folder).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    // The owner deletes one: it is not written back.
    fs::remove_file(places.folder.join("codex.toml")).unwrap();
    assert_eq!(super::list(&places).unwrap().launches.len(), 2);
    // Nor are edits overwritten.
    write(
        &places,
        "actualizar-sistema.toml",
        "name = \"Mine\"\ncommands = [\"echo hi\"]\n",
    );
    assert_eq!(super::list(&places).unwrap().launches[1].name, "Mine");
}

#[test]
fn a_bad_file_is_listed_with_why_and_the_others_still_load() {
    let places = places();
    fs::create_dir_all(&places.folder).unwrap();
    write(&places, "ok.toml", "name = \"Ok\"\ncommands = [\"ls\"]\n");
    let cases = [
        (
            "unknown-key.toml",
            "name = \"x\"\ncommands = [\"ls\"]\nshell = \"/bin/zsh\"\n",
            "not a launch file",
        ),
        (
            "both.toml",
            "name = \"x\"\ncommands = [\"ls\"]\n[session]\nagent = \"codex\"\n",
            "not both",
        ),
        (
            "neither.toml",
            "name = \"x\"\n",
            "needs commands or a session",
        ),
        (
            "newline.toml",
            "name = \"x\"\ncommands = [\"ls\\nrm -rf ~\"]\n",
            "one line",
        ),
        (
            "escape.toml",
            "name = \"x\"\ncommands = [\"ls\\u001b[A\"]\n",
            "one line",
        ),
        (
            "agent.toml",
            "name = \"x\"\n[session]\nagent = \"bash\"\n",
            "claude_code or codex",
        ),
        (
            "before-plain.toml",
            "name = \"x\"\ncommands = [\"ls\"]\nbefore = [\"id\"]\n",
            "before belongs to a session",
        ),
        (
            "relative.toml",
            "name = \"x\"\ndirectory = \"work\"\ncommands = [\"ls\"]\n",
            "absolute",
        ),
        (
            "gone.toml",
            "name = \"x\"\ndirectory = \"~/nowhere\"\ncommands = [\"ls\"]\n",
            "not found",
        ),
        (
            "long-name.toml",
            &format!("name = \"{}\"\ncommands = [\"ls\"]\n", "n".repeat(61)),
            "name",
        ),
        (
            "Upper.toml",
            "name = \"x\"\ncommands = [\"ls\"]\n",
            "lowercase",
        ),
    ];
    for (file, text, _) in &cases {
        write(&places, file, text);
    }
    let many: Vec<String> = (0..=MAX_COMMANDS).map(|i| format!("\"c{i}\"")).collect();
    write(
        &places,
        "too-many.toml",
        &format!("name = \"x\"\ncommands = [{}]\n", many.join(",")),
    );
    fs::write(
        places.folder.join("big.toml"),
        vec![b'#'; MAX_FILE_BYTES as usize + 1],
    )
    .unwrap();
    std::os::unix::fs::symlink("/etc/hostname", places.folder.join("link.toml")).unwrap();
    let list = list(&places).unwrap();
    assert_eq!(list.launches.len(), 1);
    assert_eq!(list.launches[0].id, "ok");
    for (file, _, why) in cases {
        let id = file.trim_end_matches(".toml");
        let refused = list
            .refused
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("{id} not refused"));
        assert!(refused.error.contains(why), "{id}: {}", refused.error);
    }
    for (id, why) in [
        ("too-many", "at most"),
        ("big", "larger than"),
        ("link", "not a plain file"),
    ] {
        let refused = list.refused.iter().find(|r| r.id == id).unwrap();
        assert!(refused.error.contains(why), "{id}: {}", refused.error);
    }
}

#[test]
fn a_launch_is_read_again_by_its_id_and_a_path_never_names_one() {
    let places = places();
    write(
        &places,
        "here.toml",
        "name = \"Here\"\ndirectory = \"~/work\"\ncommands = [\"pwd\"]\n",
    );
    let launch = get(&places, "here").unwrap();
    assert_eq!(launch.directory, places.home.join("work").to_string_lossy());
    for id in ["../here", "here.toml", "", "a/b", "HERE"] {
        assert!(get(&places, id).is_err(), "{id}");
    }
    // What the window holds is never what runs: a change on disk is what the next open uses.
    write(
        &places,
        "here.toml",
        "name = \"Here\"\ndirectory = \"~/work\"\ncommands = [\"ls\"]\n",
    );
    assert_eq!(
        get(&places, "here").unwrap().kind,
        Kind::Commands {
            commands: vec!["ls".into()]
        }
    );
}

#[test]
fn before_commands_run_in_order_in_the_folder_and_report_what_each_did() {
    let places = places();
    let marker = places.home.join("work/marker");
    write(
        &places,
        "s.toml",
        &format!(
            "name = \"S\"\ndirectory = \"~/work\"\nbefore = [\"pwd > {m}\", \"exit 3\", \"echo after >> {m}\"]\n[session]\nagent = \"codex\"\n",
            m = marker.display()
        ),
    );
    let launch = get(&places, "s").unwrap();
    let outcomes = run_before(&launch, BEFORE_TIMEOUT);
    let results: Vec<&str> = outcomes.iter().map(|o| o.result.as_str()).collect();
    assert_eq!(
        results,
        vec!["ok", "exit 3", "ok"],
        "a failure does not stop the rest"
    );
    let written = fs::read_to_string(&marker).unwrap();
    assert_eq!(
        written,
        format!("{}\nafter\n", places.home.join("work").display())
    );
    // A plain launch has nothing to run before.
    write(
        &places,
        "p.toml",
        "name = \"P\"\ncommands = [\"touch nope\"]\n",
    );
    assert!(run_before(&get(&places, "p").unwrap(), BEFORE_TIMEOUT).is_empty());
    assert!(!places.home.join("nope").exists());
}

#[test]
fn a_before_command_that_hangs_is_stopped_with_what_it_started() {
    let places = places();
    let marker = places.home.join("work/child-alive");
    write(
        &places,
        "slow.toml",
        &format!(
            "name = \"Slow\"\nbefore = [\"(sleep 2; touch {}) & sleep 30\"]\n[session]\nagent = \"claude_code\"\n",
            marker.display()
        ),
    );
    let started = Instant::now();
    let outcomes = run_before(&get(&places, "slow").unwrap(), Duration::from_millis(300));
    assert_eq!(outcomes[0].result, "timeout");
    assert!(started.elapsed() < Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(2500));
    assert!(!marker.exists(), "the background child was stopped too");
}

#[test]
fn what_a_before_command_leaves_running_is_stopped_when_it_ends() {
    let places = places();
    let marker = places.home.join("work/left-behind");
    write(
        &places,
        "bg.toml",
        &format!(
            "name = \"Bg\"\nbefore = [\"(sleep 1; touch {}) &\"]\n[session]\nagent = \"codex\"\n",
            marker.display()
        ),
    );
    let outcomes = run_before(&get(&places, "bg").unwrap(), BEFORE_TIMEOUT);
    assert_eq!(outcomes[0].result, "ok");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        !marker.exists(),
        "a background child outlived its before line"
    );
}
