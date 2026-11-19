use super::*;
use serde_json::json;

fn places() -> Places {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-launch-{}", uuid::Uuid::new_v4().simple()));
    let home = root.join("home");
    fs::create_dir_all(home.join("work")).unwrap();
    let os_release = root.join("os-release");
    fs::write(
        &os_release,
        "NAME=\"CachyOS Linux\"\nPRETTY_NAME=\"CachyOS\"\nID=cachyos\nID_LIKE=arch\n",
    )
    .unwrap();
    Places {
        folder: home.join(".config/wyss-console/launch"),
        home,
        os_release,
    }
}

fn approvals(places: &Places) -> Approvals {
    Approvals::at(places.home.join(".local/state/wyss-console/launch-approvals.json"))
}

/// Runs a launch's `before` commands as the owner would after approving its file.
fn approved_run(places: &Places, id: &str, timeout: Duration) -> Vec<Outcome> {
    let (launch, print) = get_with_fingerprint(places, id).unwrap();
    let approvals = approvals(places);
    approvals.approve(&print).unwrap();
    run_before(&launch, &print, timeout, &approvals).unwrap()
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
    let Kind::Commands {
        commands,
        variants,
        active,
        template,
    } = &update.kind
    else {
        panic!("{update:?}")
    };
    // This computer is CachyOS: its variant is what runs, and no option starts switched on but AUR.
    assert_eq!(
        commands,
        &vec!["sudo pacman -Syu".to_owned(), "paru -Sua".to_owned()]
    );
    assert_eq!(active.as_deref(), Some("cachyos"));
    assert_eq!(template.as_deref(), Some("system-update"));
    let ids: Vec<&str> = variants.iter().map(|v| v.distro.as_str()).collect();
    assert_eq!(ids, vec!["arch", "cachyos", "debian", "fedora", "opensuse"]);
    assert!(
        variants
            .iter()
            .all(|v| !v.commands.join(" ").contains("noconfirm"))
    );
    assert_eq!(list.system.id, "cachyos");
    assert_eq!(update.directory, places.home.to_string_lossy());
    for (launch, agent) in [
        (&list.launches[1], "claude_code"),
        (&list.launches[2], "codex"),
    ] {
        let Kind::Session {
            agent: a,
            permission,
            before,
            ..
        } = &launch.kind
        else {
            panic!("{launch:?}")
        };
        assert_eq!(a, agent);
        assert_eq!(*permission, None);
        assert!(before.is_empty());
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
            commands: vec!["ls".into()],
            variants: vec![],
            active: None,
            template: None,
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
    let outcomes = approved_run(&places, "s", BEFORE_TIMEOUT);
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
    assert!(approved_run(&places, "p", BEFORE_TIMEOUT).is_empty());
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
    let outcomes = approved_run(&places, "slow", Duration::from_millis(300));
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
    let outcomes = approved_run(&places, "bg", BEFORE_TIMEOUT);
    assert_eq!(outcomes[0].result, "ok");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        !marker.exists(),
        "a background child outlived its before line"
    );
}

/// What the desktop holds in these tests: presets, each for one agent.
struct Fake {
    presets: Vec<(&'static str, &'static str)>,
}

impl Known for Fake {
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
            "permission": "bypass", "preset": "Estudio",
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
            permission: Some("bypass".into()),
            before: vec![],
            preset: Some("Estudio".into()),
        }
    );
    let path = places.folder.join("estudiar-utn.toml");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("directory = \"~/work\""), "{text}");
    // What is written is the permission, never the key profiles had.
    assert!(text.contains("permission = \"bypass\"\n"), "{text}");
    assert!(!text.contains("profile"), "{text}");
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
        (
            json!({"name": "x", "directory": work, "agent": "bash"}),
            "claude_code or codex",
        ),
        (
            json!({"name": "a\nb", "directory": work, "agent": "codex"}),
            "one line",
        ),
        (
            json!({"name": "x".repeat(61), "directory": work, "agent": "codex"}),
            "1–60",
        ),
        (
            json!({"name": "!!!", "directory": work, "agent": "codex"}),
            "letter or digit",
        ),
        (
            json!({"name": "x", "directory": "work", "agent": "codex"}),
            "absolute",
        ),
        (
            json!({"name": "x", "directory": nowhere, "agent": "codex"}),
            "not found",
        ),
        (
            json!({"name": "x", "directory": work, "agent": "codex", "permission": "edits"}),
            "only for claude_code",
        ),
        (
            json!({"name": "x", "directory": work, "agent": "claude_code", "preset": "Codex base"}),
            "no preset named",
        ),
        (
            json!({"name": "x", "directory": work, "agent": "codex", "permission": "Full access (bypass)"}),
            "permission must be",
        ),
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
    assert_eq!(
        slug("Estudio Ñandú — Güemes 2026!"),
        "estudio-nandu-guemes-2026"
    );
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
    // A launch with a manually added before command stays owned by its author.
    write(
        &places,
        "codex.toml",
        "name = \"Codex\"\nbefore = [\"printf fixture\"]\n[session]\nagent = \"codex\"\n",
    );
    let refused = delete(&places, "codex").unwrap_err();
    assert!(refused.contains("edit the others"), "{refused}");
    assert!(places.folder.join("codex.toml").exists());
    write(
        &places,
        "typed.toml",
        "name = \"Typed\"\ncommands = [\"ls\"]\n",
    );
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

fn system(id: &str, like: &[&str]) -> System {
    System {
        id: id.into(),
        like: like.iter().map(|s| (*s).to_owned()).collect(),
        name: id.into(),
    }
}

#[test]
fn a_distribution_without_its_own_variant_takes_the_one_it_is_like() {
    let places = places();
    let ubuntu = system("ubuntu", &["debian"]);
    let launch = parse("u", SYSTEM_UPDATE, &places.home, &ubuntu).unwrap();
    let Kind::Commands {
        commands, active, ..
    } = launch.kind
    else {
        panic!()
    };
    assert_eq!(active.as_deref(), Some("debian"));
    assert_eq!(commands[0], "sudo apt update");
    // Nothing matches: the launch is listed and has nothing to type.
    let other = system("gentoo", &[]);
    let launch = parse("u", SYSTEM_UPDATE, &places.home, &other).unwrap();
    let Kind::Commands {
        commands, active, ..
    } = launch.kind
    else {
        panic!()
    };
    assert!(commands.is_empty() && active.is_none());
    // A file's own commands are the fallback.
    let text = "name = \"x\"\ncommands = [\"echo all\"]\n[distro.fedora]\ncommands = [\"dnf\"]\n";
    let launch = parse("x", text, &places.home, &other).unwrap();
    let Kind::Commands { commands, .. } = launch.kind else {
        panic!()
    };
    assert_eq!(commands, vec!["echo all".to_owned()]);
}

#[test]
fn os_release_is_read_with_quotes_and_like_list() {
    let places = places();
    fs::write(
        &places.os_release,
        "PRETTY_NAME='Linux Mint 22'\nID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
    )
    .unwrap();
    let s = places.system();
    assert_eq!(s.id, "linuxmint");
    assert_eq!(s.like, vec!["ubuntu", "debian"]);
    assert_eq!(s.name, "Linux Mint 22");
    assert_eq!(
        System::read(Path::new("/nonexistent/os-release"))
            .chain()
            .len(),
        0
    );
}

#[test]
fn a_variant_with_a_bad_line_or_name_refuses_the_file() {
    let places = places();
    let s = system("cachyos", &[]);
    for text in [
        "name = \"x\"\n[distro.cachyos]\ncommands = []\n",
        "name = \"x\"\n[distro.Cachy]\ncommands = [\"a\"]\n",
        "name = \"x\"\n[distro.cachyos]\ncommands = [\"a\\nrm -rf ~\"]\n",
        "name = \"x\"\n[distro.cachyos]\ncommands = [\"a\"]\n[session]\nagent = \"codex\"\n",
    ] {
        assert!(parse("x", text, &places.home, &s).is_err(), "{text}");
    }
}

fn variant(distro: &str, commands: &[&str], options: &[&str], manual: bool) -> Variant {
    Variant {
        distro: distro.into(),
        commands: commands.iter().map(|c| (*c).to_owned()).collect(),
        options: options.iter().map(|o| (*o).to_owned()).collect(),
        manual,
    }
}

#[test]
fn variants_are_saved_read_back_and_this_computers_one_stays() {
    let places = places();
    list(&places).unwrap();
    let request = VariantsRequest {
        id: "actualizar-sistema".into(),
        variants: vec![
            variant(
                "cachyos",
                &["sudo pacman -Syu --noconfirm"],
                &["noconfirm"],
                false,
            ),
            variant("void", &["sudo xbps-install -Su"], &[], true),
        ],
    };
    let launch = save_variants(&places, &request).unwrap();
    let Kind::Commands {
        commands,
        variants,
        active,
        template,
    } = launch.kind
    else {
        panic!()
    };
    assert_eq!(commands, vec!["sudo pacman -Syu --noconfirm".to_owned()]);
    assert_eq!(active.as_deref(), Some("cachyos"));
    assert_eq!(template.as_deref(), Some("system-update"));
    assert_eq!(variants, request.variants);
    assert_eq!(
        get(&places, "actualizar-sistema").unwrap().name,
        "Actualizar sistema"
    );
    // Removing this computer's variant is refused and writes nothing.
    let before = fs::read_to_string(places.folder.join("actualizar-sistema.toml")).unwrap();
    let refused = VariantsRequest {
        id: "actualizar-sistema".into(),
        variants: vec![variant("void", &["x"], &[], true)],
    };
    assert!(
        save_variants(&places, &refused)
            .unwrap_err()
            .contains("stays")
    );
    for bad in [
        vec![
            variant("cachyos", &["a"], &[], false),
            variant("cachyos", &["b"], &[], false),
        ],
        vec![variant("cachyos", &["a\u{1b}[2J"], &[], false)],
        vec![variant("cachyos", &[], &[], false)],
        (0..=MAX_VARIANTS)
            .map(|i| variant(&format!("d{i}"), &["a"], &[], true))
            .chain([variant("cachyos", &["a"], &[], false)])
            .collect(),
        vec![variant(
            "cachyos",
            &[&"x".repeat(MAX_COMMAND + 1)],
            &[],
            false,
        )],
        vec![variant("cachyos", &["a"], &["Bad Option"], false)],
        vec![
            variant("cachyos", &["a"], &[], false),
            variant("../x", &["a"], &[], false),
        ],
    ] {
        let r = VariantsRequest {
            id: "actualizar-sistema".into(),
            variants: bad,
        };
        assert!(save_variants(&places, &r).is_err(), "{:?}", r.variants);
    }
    assert_eq!(
        fs::read_to_string(places.folder.join("actualizar-sistema.toml")).unwrap(),
        before
    );
    // A session launch has no distributions.
    let r = VariantsRequest {
        id: "codex".into(),
        variants: vec![],
    };
    assert!(save_variants(&places, &r).is_err());
}

#[test]
fn the_earlier_shipped_update_is_upgraded_only_while_untouched() {
    let places = places();
    write(&places, "actualizar-sistema.toml", SYSTEM_UPDATE_BEFORE);
    let launch = get(&places, "actualizar-sistema").unwrap();
    assert!(matches!(&launch.kind, Kind::Commands { variants, .. } if variants.is_empty()));
    list(&places).unwrap();
    assert_eq!(
        fs::read_to_string(places.folder.join("actualizar-sistema.toml")).unwrap(),
        SYSTEM_UPDATE
    );
    // An edited one is never touched.
    let mine = format!("{SYSTEM_UPDATE_BEFORE}# mine\n");
    write(&places, "actualizar-sistema.toml", &mine);
    list(&places).unwrap();
    assert_eq!(
        fs::read_to_string(places.folder.join("actualizar-sistema.toml")).unwrap(),
        mine
    );
}

fn session_file(session: &str) -> String {
    format!("name = \"S\"\n\n[session]\n{session}\n")
}

fn permission_of(places: &Places, id: &str) -> Option<String> {
    match get(places, id).unwrap().kind {
        Kind::Session { permission, .. } => permission,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_session_names_its_permission_and_edits_is_only_for_claude_code() {
    let places = places();
    fs::create_dir_all(&places.folder).unwrap();
    for (agent, word) in [
        ("claude_code", "ask"),
        ("claude_code", "read_only"),
        ("claude_code", "edits"),
        ("claude_code", "bypass"),
        ("codex", "ask"),
        ("codex", "read_only"),
        ("codex", "bypass"),
    ] {
        write(
            &places,
            "s.toml",
            &session_file(&format!("agent = \"{agent}\"\npermission = \"{word}\"")),
        );
        assert_eq!(
            permission_of(&places, "s").as_deref(),
            Some(word),
            "{agent}"
        );
    }
    // Without the key the agent starts as it is configured.
    write(&places, "s.toml", &session_file("agent = \"codex\""));
    assert_eq!(permission_of(&places, "s"), None);
    for (session, why) in [
        (
            "agent = \"codex\"\npermission = \"edits\"",
            "only for claude_code",
        ),
        (
            "agent = \"claude_code\"\npermission = \"Bypass\"",
            "permission must be",
        ),
        (
            "agent = \"claude_code\"\npermission = \"\"",
            "permission must be",
        ),
        (
            "agent = \"claude_code\"\npermission = \"template-full-access\"",
            "permission must be",
        ),
    ] {
        write(&places, "s.toml", &session_file(session));
        let error = get(&places, "s").unwrap_err();
        assert!(error.contains(why), "{session}: {error}");
    }
}

#[test]
fn a_file_from_before_profiles_were_removed_is_read_by_its_template_name_only() {
    let places = places();
    fs::create_dir_all(&places.folder).unwrap();
    for (name, permission) in [
        ("Full access (bypass)", Some("bypass")),
        ("Review, read only", Some("read_only")),
        ("Strict", Some("ask")),
        ("As configured", None),
    ] {
        for agent in ["claude_code", "codex"] {
            let text = session_file(&format!("agent = \"{agent}\"\nprofile = \"{name}\""));
            write(&places, "old.toml", &text);
            assert_eq!(
                permission_of(&places, "old").as_deref(),
                permission,
                "{name}"
            );
            // Reading never rewrites the owner's file.
            assert_eq!(
                fs::read_to_string(places.folder.join("old.toml")).unwrap(),
                text
            );
        }
    }
    // Both a custom profile and an unknown template ID are refused with guidance.
    for name in [
        "Estudio",
        "Exploration, fewer hooks",
        "full access (bypass)",
        "",
    ] {
        write(
            &places,
            "old.toml",
            &session_file(&format!("agent = \"claude_code\"\nprofile = \"{name}\"")),
        );
        let error = get(&places, "old").unwrap_err();
        assert!(
            error.contains("profiles were removed: set permission = "),
            "{name}: {error}"
        );
    }
    // Both keys: refused, even when they agree.
    write(
        &places,
        "old.toml",
        &session_file(
            "agent = \"claude_code\"\npermission = \"bypass\"\nprofile = \"Full access (bypass)\"",
        ),
    );
    let error = get(&places, "old").unwrap_err();
    assert!(error.contains("not both"), "{error}");
    // The refused file is listed with why, and the others still load.
    write(&places, "ok.toml", "name = \"Ok\"\ncommands = [\"ls\"]\n");
    let listed = list(&places).unwrap();
    assert_eq!(listed.launches.len(), 1);
    assert_eq!(listed.refused.len(), 1);
    assert_eq!(listed.refused[0].id, "old");
}

#[test]
fn a_file_changed_outside_with_before_commands_needs_approval_and_then_runs() {
    let places = places();
    let marker = places.home.join("work/ran");
    write(
        &places,
        "x.toml",
        &format!(
            "name = \"X\"\nbefore = [\"touch {}\"]\n[session]\nagent = \"codex\"\npermission = \"bypass\"\n",
            marker.display()
        ),
    );
    let approvals = approvals(&places);
    let (launch, print) = get_with_fingerprint(&places, "x").unwrap();
    let refused = run_before(&launch, &print, BEFORE_TIMEOUT, &approvals).unwrap_err();
    assert_eq!(refused.code, "needs_approval");
    assert_eq!(refused.before, vec![format!("touch {}", marker.display())]);
    assert!(refused.bypass);
    assert!(!marker.exists(), "nothing ran before the approval");
    approvals.approve(&refused.fingerprint).unwrap();
    run_before(&launch, &print, BEFORE_TIMEOUT, &approvals).unwrap();
    assert!(marker.exists());
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(places.home.join(".local/state/wyss-console/launch-approvals.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    // Edited again by another program: the approval was for the old text.
    let text = fs::read_to_string(places.folder.join("x.toml")).unwrap();
    fs::write(places.folder.join("x.toml"), format!("{text}# changed\n")).unwrap();
    let (launch, print) = get_with_fingerprint(&places, "x").unwrap();
    assert_eq!(
        run_before(&launch, &print, BEFORE_TIMEOUT, &approvals)
            .unwrap_err()
            .code,
        "needs_approval"
    );
}

#[test]
fn a_session_with_neither_before_nor_full_access_needs_no_approval() {
    let places = places();
    write(
        &places,
        "plain.toml",
        "name = \"Plain\"\n[session]\nagent = \"codex\"\npermission = \"ask\"\n",
    );
    let (launch, print) = get_with_fingerprint(&places, "plain").unwrap();
    assert!(
        run_before(&launch, &print, BEFORE_TIMEOUT, &approvals(&places))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_before_command_does_not_see_the_consoles_secrets() {
    let places = places();
    let out = places.home.join("work/env");
    write(
        &places,
        "env.toml",
        &format!(
            "name = \"Env\"\nbefore = [\"env > {}\"]\n[session]\nagent = \"codex\"\n",
            out.display()
        ),
    );
    // SAFETY: only this test reads or writes this name.
    unsafe { std::env::set_var("SECRETO_TEST", "no-debe-verse") };
    let outcomes = approved_run(&places, "env", BEFORE_TIMEOUT);
    unsafe { std::env::remove_var("SECRETO_TEST") };
    assert_eq!(outcomes[0].result, "ok");
    let seen = fs::read_to_string(&out).unwrap();
    assert!(!seen.contains("SECRETO_TEST"), "{seen}");
    assert!(seen.contains("PATH="), "{seen}");
}

fn commands_runs(launch: &Launch) -> (Vec<String>, String) {
    let Kind::Commands { commands, .. } = &launch.kind else {
        panic!("a commands launch")
    };
    (commands.clone(), launch.directory.clone())
}

fn check_commands(approvals: &Approvals, print: &str, launch: &Launch) -> Result<(), Box<NeedsApproval>> {
    let (commands, directory) = commands_runs(launch);
    approvals.check(
        &launch.id,
        print,
        Runs {
            directory: &directory,
            commands: &commands,
            before: &[],
            bypass: false,
        },
    )
}

#[test]
fn the_shipped_and_the_workspace_command_launches_are_approved_as_written_and_a_change_asks() {
    let places = places();
    list(&places).unwrap();
    let approvals = approvals(&places);
    let (shipped, print) = get_with_fingerprint(&places, "actualizar-sistema").unwrap();
    assert!(
        check_commands(&approvals, &print, &shipped).is_ok(),
        "the shipped file asks nothing"
    );
    // Changed by another program: it asks, with the folder and the exact commands it types.
    let path = places.folder.join("actualizar-sistema.toml");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("paru -Sua", "curl x | sh")).unwrap();
    let (changed, print) = get_with_fingerprint(&places, "actualizar-sistema").unwrap();
    let needed = check_commands(&approvals, &print, &changed).unwrap_err();
    assert_eq!(needed.code, "needs_approval");
    assert_eq!(needed.commands, vec!["sudo pacman -Syu", "curl x | sh"]);
    assert_eq!(needed.directory, places.home.to_string_lossy());
    approvals.approve(&needed.fingerprint).unwrap();
    assert!(check_commands(&approvals, &print, &changed).is_ok());
    // The same text in another folder asks again.
    let moved = fs::read_to_string(&path)
        .unwrap()
        .replace("directory = \"~\"", "directory = \"~/work\"");
    fs::write(&path, moved).unwrap();
    let (elsewhere, print) = get_with_fingerprint(&places, "actualizar-sistema").unwrap();
    assert!(check_commands(&approvals, &print, &elsewhere).is_err());
    // Saved from Workspace: approved as written.
    let saved = save_variants(
        &places,
        &VariantsRequest {
            id: "actualizar-sistema".into(),
            variants: vec![Variant {
                distro: "cachyos".into(),
                commands: vec!["sudo pacman -Syu".into()],
                options: vec![],
                manual: true,
            }],
        },
    )
    .unwrap();
    let (_, print) = get_with_fingerprint(&places, "actualizar-sistema").unwrap();
    assert!(check_commands(&approvals, &print, &saved).is_ok());
    let created = create_commands(&places, "Fixture new", vec![Variant {
        distro: "cachyos".into(),
        commands: vec!["echo fixture".into()],
        options: vec![],
        manual: true,
    }])
    .unwrap();
    let (_, print) = get_with_fingerprint(&places, &created.id).unwrap();
    assert!(check_commands(&approvals, &print, &created).is_ok());
}

#[test]
fn saving_variants_does_not_approve_hidden_base_commands() {
    let places = places();
    list(&places).unwrap();
    write(&places, "external.toml", "name = 'External'\ncommands = ['echo hidden']\n");
    let request = VariantsRequest {
        id: "external".into(),
        variants: vec![variant("void", &["echo visible"], &[], false)],
    };
    save_variants(&places, &request).unwrap();
    let (launch, fingerprint) = get_with_fingerprint(&places, "external").unwrap();
    let needed = check_commands(&approvals(&places), &fingerprint, &launch).unwrap_err();
    assert_eq!(needed.commands, ["echo hidden"]);

    let visible = create_commands(&places, "Visible", vec![variant("cachyos", &["echo visible"], &[], false)]).unwrap();
    let (_, print) = get_with_fingerprint(&places, &visible.id).unwrap();
    check_commands(&approvals(&places), &print, &visible).unwrap();
}
