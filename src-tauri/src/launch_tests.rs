use super::*;
use serde_json::json;

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
            ..
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

/// What the desktop holds in these tests: profiles and presets, each for one agent.
struct Fake {
    profiles: Vec<(&'static str, &'static str)>,
    presets: Vec<(&'static str, &'static str)>,
}

impl Known for Fake {
    fn profiles(&self, agent: &str) -> Result<Vec<String>, String> {
        Ok(self
            .profiles
            .iter()
            .filter(|(a, _)| *a == agent)
            .map(|(_, n)| n.to_string())
            .collect())
    }
    fn presets(&self, agent: &str) -> Result<Vec<String>, String> {
        Ok(self
            .presets
            .iter()
            .filter(|(a, _)| *a == agent)
            .map(|(_, n)| n.to_string())
            .collect())
    }
}

fn fake() -> Fake {
    Fake {
        profiles: vec![
            ("claude_code", "Full access (bypass)"),
            ("codex", "Estudio"),
        ],
        presets: vec![("claude_code", "Estudio"), ("codex", "Codex base")],
    }
}

fn request(value: serde_json::Value) -> SaveRequest {
    serde_json::from_value(value).unwrap()
}

#[test]
fn a_saved_launch_is_private_names_its_preset_and_reads_back_the_same() {
    let places = places();
    let work = places.home.join("work").display().to_string();
    let saved = save(
        &places,
        &request(json!({
            "name": "Estudiar UTN", "directory": work, "agent": "claude_code",
            "profile": "Full access (bypass)", "preset": "Estudio",
        })),
        &fake(),
    )
    .unwrap();
    assert_eq!(saved.id, "estudiar-utn");
    assert_eq!(saved.name, "Estudiar UTN");
    assert_eq!(
        saved.kind,
        Kind::Session {
            agent: "claude_code".into(),
            profile: Some("Full access (bypass)".into()),
            before: vec![],
            preset: Some("Estudio".into()),
        }
    );
    let path = places.folder.join("estudiar-utn.toml");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("directory = \"~/work\""), "{text}");
    assert!(
        !text.contains("commands") && !text.contains("before"),
        "{text}"
    );
    assert_eq!(get(&places, "estudiar-utn").unwrap(), saved);
    let stray: Vec<String> = fs::read_dir(&places.folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp"))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
    let listed = list(&places).unwrap();
    assert!(listed.refused.is_empty(), "{:?}", listed.refused);
    assert!(listed.launches.iter().any(|l| l.id == "estudiar-utn"));
}

#[test]
fn a_save_is_checked_against_the_desktop_and_writes_nothing_when_refused() {
    let places = places();
    let work = places.home.join("work").display().to_string();
    let nowhere = places.home.join("nowhere").display().to_string();
    let cases = [
        (json!({"name": "x", "directory": work, "agent": "bash"}), "claude_code or codex"),
        (json!({"name": "a\nb", "directory": work, "agent": "codex"}), "one line"),
        (json!({"name": "x".repeat(61), "directory": work, "agent": "codex"}), "1–60"),
        (json!({"name": "!!!", "directory": work, "agent": "codex"}), "letter or digit"),
        (json!({"name": "x", "directory": "work", "agent": "codex"}), "absolute"),
        (json!({"name": "x", "directory": nowhere, "agent": "codex"}), "not found"),
        (json!({"name": "x", "directory": work, "agent": "codex", "profile": "Full access (bypass)"}), "no profile named"),
        (json!({"name": "x", "directory": work, "agent": "claude_code", "preset": "Codex base"}), "no preset named"),
        (json!({"name": "x", "directory": work, "agent": "codex", "profile": ""}), "profile must be"),
    ];
    for (value, why) in cases {
        match save(&places, &request(value.clone()), &fake()) {
            Err(SaveRefusal::Invalid(text)) => assert!(text.contains(why), "{value}: {text}"),
            other => panic!("{value}: {other:?}"),
        }
    }
    assert!(!places.folder.exists(), "a refused save wrote something");
}

#[test]
fn an_existing_launch_is_refused_until_the_window_asks_to_replace_it() {
    let places = places();
    let base = json!({
        "name": "Mismo", "directory": places.home.join("work").display().to_string(),
        "agent": "codex",
    });
    save(&places, &request(base.clone()), &fake()).unwrap();
    assert_eq!(
        save(&places, &request(base.clone()), &fake()).unwrap_err(),
        SaveRefusal::Exists
    );
    let mut again = base;
    again["preset"] = json!("Codex base");
    again["replace"] = json!(true);
    let replaced = save(&places, &request(again), &fake()).unwrap();
    assert!(matches!(
        replaced.kind,
        Kind::Session { preset: Some(ref p), .. } if p == "Codex base"
    ));
}

#[test]
fn a_save_never_writes_through_a_link() {
    let places = places();
    seed(&places.folder).unwrap();
    let victim = places.home.join("victim.txt");
    fs::write(&victim, "keep").unwrap();
    std::os::unix::fs::symlink(&victim, places.folder.join("linked.toml")).unwrap();
    let refusal = save(
        &places,
        &request(json!({
            "name": "Linked", "directory": places.home.display().to_string(),
            "agent": "codex", "replace": true,
        })),
        &fake(),
    );
    assert!(
        matches!(&refusal, Err(SaveRefusal::Invalid(t)) if t.contains("not a plain file")),
        "{refusal:?}"
    );
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
}

#[test]
fn the_window_cannot_send_a_command_or_a_before_line_to_save() {
    let base = json!({"name": "x", "directory": "~", "agent": "codex"});
    for (key, value) in [
        ("commands", json!(["rm -rf ~"])),
        ("before", json!(["id"])),
        ("kind", json!("commands")),
        ("session", json!({})),
    ] {
        let mut tampered = base.clone();
        tampered[key] = value;
        assert!(
            serde_json::from_value::<SaveRequest>(tampered.clone()).is_err(),
            "{tampered}"
        );
    }
}

#[test]
fn a_file_name_is_lowercase_ascii_with_dashes_and_at_most_forty() {
    assert_eq!(slug("Estudio Ñandú — Güemes 2026!"), "estudio-nandu-guemes-2026");
    assert_eq!(slug("  --Codex--  "), "codex");
    let long = slug(&"abc ".repeat(20));
    assert!(long.len() <= 40 && !long.ends_with('-'), "{long}");
    assert_eq!(slug("¿¡?!"), "");
}

#[test]
fn only_a_launch_saved_from_workspace_can_be_deleted_here() {
    let places = places();
    save(
        &places,
        &request(json!({
            "name": "Temporal", "directory": places.home.display().to_string(), "agent": "codex",
        })),
        &fake(),
    )
    .unwrap();
    delete(&places, "temporal").unwrap();
    assert!(!places.folder.join("temporal.toml").exists());
    // The shipped Codex launch names a `before` line: the owner's file, kept.
    let refused = delete(&places, "codex").unwrap_err();
    assert!(refused.contains("edit the others"), "{refused}");
    assert!(places.folder.join("codex.toml").exists());
    write(&places, "typed.toml", "name = \"Typed\"\ncommands = [\"ls\"]\n");
    assert!(delete(&places, "typed").is_err());
    assert!(places.folder.join("typed.toml").exists());
    assert!(delete(&places, "../codex").is_err());
}

#[test]
fn a_session_names_its_preset_and_an_empty_name_is_refused() {
    let places = places();
    write(
        &places,
        "with.toml",
        "name = \"With\"\n[session]\nagent = \"codex\"\npreset = \"Codex base\"\n",
    );
    assert!(matches!(
        get(&places, "with").unwrap().kind,
        Kind::Session { preset: Some(ref p), .. } if p == "Codex base"
    ));
    write(
        &places,
        "empty.toml",
        "name = \"Empty\"\n[session]\nagent = \"codex\"\npreset = \"\"\n",
    );
    let why = get(&places, "empty").unwrap_err();
    assert!(why.contains("preset must be"), "{why}");
}
