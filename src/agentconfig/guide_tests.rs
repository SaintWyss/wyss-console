use super::*;
use std::fs;
use std::os::unix::fs::symlink;

fn home() -> PathBuf {
    let home = std::env::temp_dir().join(format!("guide-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    fs::create_dir_all(home.join(".agents/skills")).unwrap();
    home
}

fn env(home: &Path) -> Environment {
    Environment {
        home: home.to_path_buf(),
        claude_config_dir: None,
        codex_home: None,
        project: None,
        system: PathBuf::from("/etc"),
        declared_writers: Vec::new(),
    }
}

fn skill(folder: &Path, front: &str, body: &str) {
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        format!("---\n{front}\n---\n{body}"),
    )
    .unwrap();
}

fn find<'a>(guide: &'a Guide, tool: &str, name: &str) -> &'a GuideEntry {
    guide
        .entries
        .iter()
        .find(|entry| entry.tool == tool && entry.name == name)
        .unwrap_or_else(|| panic!("{tool} {name} not listed"))
}

#[test]
fn classifies_each_skill_by_where_it_resolves() {
    let home = home();
    let skills = home.join(".claude/skills");
    // The owner's own: a plain folder, with a folded description.
    skill(
        &skills.join("loops"),
        "name: loops\ndescription: >\n  Designs loops.\n  Use when a task repeats.",
        "# Loops\n",
    );
    // A link to the GSD Core payload.
    let gsd = home.join(".local/share/wyss/gsd-core/1.15.0/skills/gsd-help");
    skill(&gsd, "name: gsd-help\ndescription: Shows GSD commands.", "");
    symlink(&gsd, skills.join("gsd-help")).unwrap();
    // A link to a folder of the owner's that lives inside a checkout one level up: still the owner's.
    let source = home.join(".claude/skills-src/humanizer");
    fs::create_dir_all(source.join(".git")).unwrap();
    skill(
        &source.join("skills/humanizer"),
        "description: |\n  Rewrites text.",
        "",
    );
    symlink(
        "../skills-src/humanizer/skills/humanizer",
        skills.join("humanizer"),
    )
    .unwrap();
    // A Git checkout.
    skill(
        &skills.join("taste"),
        "description: Reads a site's taste.",
        "",
    );
    fs::create_dir_all(skills.join("taste/.git")).unwrap();
    // The Claude account's synced skills.
    skill(
        &skills.join("synced/acct/pdf"),
        "description: Reads PDFs.",
        "",
    );
    // A link out of the roots is listed, never read.
    let outside = home.join("elsewhere/evil");
    skill(&outside, "description: Should not be read.", "");
    symlink(&outside, skills.join("evil")).unwrap();

    let (guide, places) = scan(&env(&home));

    let loops = find(&guide, "claude_code", "loops");
    assert_eq!(loops.set, "mine");
    assert_eq!(loops.description, "Designs loops. Use when a task repeats.");
    assert_eq!(loops.scope, Some("claude"));
    let help = find(&guide, "claude_code", "gsd-help");
    assert_eq!(help.set, "gsd:claude_code");
    assert_eq!(help.description, "Shows GSD commands.");
    assert!(
        help.linked_to
            .as_deref()
            .unwrap()
            .contains("gsd-core/1.15.0")
    );
    let humanizer = find(&guide, "claude_code", "humanizer");
    assert_eq!(humanizer.set, "mine");
    assert_eq!(humanizer.description, "Rewrites text.");
    assert_eq!(
        find(&guide, "claude_code", "taste").set,
        "repos:claude_code"
    );
    let pdf = find(&guide, "claude_code", "pdf");
    assert_eq!(pdf.set, "account:claude_code");
    assert!(guide.entries.iter().all(|entry| entry.name != "synced"));
    let evil = find(&guide, "claude_code", "evil");
    assert_eq!(evil.unreadable, Some("outside_roots"));
    assert_eq!(evil.description, "");
    assert!(body(&env(&home), &places, &evil.id).is_none());
    let gsd_set = guide
        .sets
        .iter()
        .find(|set| set.key == "gsd:claude_code")
        .unwrap();
    assert_eq!(
        (gsd_set.name.as_str(), gsd_set.version.as_str()),
        ("GSD Core", "1.15.0")
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn tells_which_tools_see_a_skill_and_whether_the_copies_differ() {
    let home = home();
    skill(
        &home.join(".claude/skills/same"),
        "description: Same.",
        "body",
    );
    skill(
        &home.join(".agents/skills/same"),
        "description: Same.",
        "body",
    );
    skill(
        &home.join(".claude/skills/differs"),
        "description: One.",
        "claude",
    );
    skill(
        &home.join(".agents/skills/differs"),
        "description: One.",
        "codex",
    );
    skill(
        &home.join(".agents/skills/codex-only"),
        "description: C.",
        "",
    );
    skill(
        &home.join(".agents/skills/gstack-qa"),
        "description: QA.",
        "",
    );
    skill(
        &home.join(".agents/skills/website-to-hyperframes"),
        "description: H.",
        "",
    );

    let (guide, _) = scan(&env(&home));

    let same = find(&guide, "claude_code", "same");
    assert_eq!((same.scope, same.codex_differs), (Some("both"), false));
    let differs = find(&guide, "claude_code", "differs");
    assert_eq!((differs.scope, differs.codex_differs), (Some("both"), true));
    assert!(find(&guide, "codex", "differs").codex_differs);
    assert_eq!(find(&guide, "codex", "codex-only").scope, Some("codex"));
    assert_eq!(find(&guide, "codex", "codex-only").set, "mine");
    assert_eq!(
        find(&guide, "codex", "gstack-qa").set,
        "bundle:codex:gstack"
    );
    assert_eq!(
        find(&guide, "codex", "website-to-hyperframes").set,
        "bundle:codex:hyperframes"
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn reads_plugins_agents_and_the_body_by_id_only() {
    let home = home();
    let plugin = home.join(".claude/plugins/cache/market/caveman/1.0");
    fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
    fs::write(
        plugin.join(".claude-plugin/plugin.json"),
        r#"{"name":"caveman","version":"1.0","description":"Talks short."}"#,
    )
    .unwrap();
    skill(
        &plugin.join("skills/caveman-help"),
        "description: Help.",
        "",
    );
    fs::write(
        home.join(".claude/plugins/installed_plugins.json"),
        format!(
            r#"{{"plugins":{{"caveman@market":[{{"installPath":"{}"}}]}}}}"#,
            plugin.display()
        ),
    )
    .unwrap();
    let synced = home.join(".claude/plugins/synced/acct/uuid-1");
    fs::create_dir_all(synced.join(".claude-plugin")).unwrap();
    fs::write(
        synced.join(".claude-plugin/plugin.json"),
        r#"{"name":"design","version":"1.2.0"}"#,
    )
    .unwrap();
    skill(
        &synced.join("skills/critique"),
        "description: Critiques.",
        "",
    );
    fs::create_dir_all(home.join(".claude/agents")).unwrap();
    fs::write(
        home.join(".claude/agents/planner.md"),
        "---\ndescription: Plans.\n---\nYou plan.",
    )
    .unwrap();
    skill(
        &home.join(".claude/skills/hostile"),
        "description: Hostile.",
        "# Title\n<script>alert(1)</script>\n<img src=x onerror=alert(1)>\n",
    );
    fs::write(home.join(".claude/skills/hostile/notes.txt"), "n").unwrap();

    let (guide, places) = scan(&env(&home));

    assert_eq!(
        find(&guide, "claude_code", "caveman-help").set,
        "plugin:claude_code:caveman"
    );
    assert_eq!(
        find(&guide, "claude_code", "critique").set,
        "plugin:claude_code:design"
    );
    let agent = find(&guide, "claude_code", "planner");
    assert_eq!(
        (agent.kind, agent.description.as_str()),
        ("agent", "Plans.")
    );
    let caveman = guide.sets.iter().find(|set| set.name == "caveman").unwrap();
    assert_eq!(caveman.description, "Talks short.");

    let hostile = find(&guide, "claude_code", "hostile");
    let read = body(&env(&home), &places, &hostile.id).unwrap();
    // The text is returned as it is written, without its front matter; the window shows it as text.
    assert!(read.text.starts_with("# Title\n<script>"));
    assert_eq!(
        read.files,
        vec!["SKILL.md".to_owned(), "notes.txt".to_owned()]
    );
    assert!(!read.cut);
    let agent_body = body(&env(&home), &places, &agent.id).unwrap();
    assert_eq!(agent_body.text, "You plan.");
    assert!(body(&env(&home), &places, "claude_code/skill/mine/../../etc").is_none());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_folder_linked_out_of_the_roots_is_not_read_and_says_the_lists_are_incomplete() {
    let home = home();
    let elsewhere = home.join("dotfiles/skills");
    skill(&elsewhere.join("loops"), "description: Loops.", "");
    fs::remove_dir(home.join(".claude/skills")).unwrap();
    symlink(&elsewhere, home.join(".claude/skills")).unwrap();
    let (guide, _) = scan(&env(&home));
    // The approved roots are the providers' folders and GSD Core, never where a link leads.
    assert!(guide.entries.iter().all(|entry| entry.name != "loops"));
    assert!(guide.truncated);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn reads_plugins_through_a_link_that_stays_inside_the_roots() {
    let home = home();
    // `~/.claude/plugins` is a link to another folder inside `~/.claude`.
    let plugins = home.join(".claude/plugins-real");
    let plugin = plugins.join("cache/m/p/1.0");
    fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
    fs::write(plugin.join(".claude-plugin/plugin.json"), r#"{"name":"p"}"#).unwrap();
    skill(&plugin.join("skills/ps"), "description: Plugin skill.", "");
    fs::write(
        plugins.join("installed_plugins.json"),
        format!(
            r#"{{"plugins":{{"p@m":[{{"installPath":"{}"}}]}}}}"#,
            plugin.display()
        ),
    )
    .unwrap();
    symlink(&plugins, home.join(".claude/plugins")).unwrap();
    let (guide, places) = scan(&env(&home));
    let ps = find(&guide, "claude_code", "ps");
    assert_eq!(ps.set, "plugin:claude_code:p");
    assert!(body(&env(&home), &places, &ps.id).is_some());
    assert!(!guide.truncated);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_link_of_mine_into_a_plugin_belongs_to_that_plugin() {
    let home = home();
    let plugin = home.join(".claude/plugins/cache/m/p/1.0");
    fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
    fs::write(plugin.join(".claude-plugin/plugin.json"), r#"{"name":"p"}"#).unwrap();
    skill(&plugin.join("skills/ps"), "description: Plugin skill.", "");
    fs::write(
        home.join(".claude/plugins/installed_plugins.json"),
        format!(
            r#"{{"plugins":{{"p@m":[{{"installPath":"{}"}}]}}}}"#,
            plugin.display()
        ),
    )
    .unwrap();
    symlink(
        plugin.join("skills/ps"),
        home.join(".claude/skills/ps-linked"),
    )
    .unwrap();
    let (guide, _) = scan(&env(&home));
    assert_eq!(
        find(&guide, "claude_code", "ps-linked").set,
        "plugin:claude_code:p"
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_folder_past_the_listing_bound_is_reported_incomplete() {
    let home = home();
    let skills = home.join(".agents/skills");
    for n in 0..2100 {
        fs::create_dir(skills.join(format!("s{n:04}"))).unwrap();
    }
    let (guide, _) = scan(&env(&home));
    assert!(guide.truncated);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn keeps_a_long_description_whole_and_says_when_it_is_cut() {
    let home = home();
    let long = format!("{}. Use when it is long.", "A".repeat(5000));
    skill(
        &home.join(".claude/skills/long"),
        &format!("description: {long}"),
        "",
    );
    let huge = "B".repeat(DESCRIPTION_CHARS + 10);
    skill(
        &home.join(".claude/skills/huge"),
        &format!("description: {huge}"),
        "",
    );
    let (guide, _) = scan(&env(&home));
    let entry = find(&guide, "claude_code", "long");
    assert!(entry.description.ends_with("Use when it is long."));
    assert!(!entry.description_cut);
    assert!(find(&guide, "claude_code", "huge").description_cut);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_skill_with_too_many_files_is_counted_within_a_budget() {
    let home = home();
    let folder = home.join(".claude/skills/many");
    skill(&folder, "description: Many.", "");
    for d in 0..50 {
        let sub = folder.join(format!("d{d:02}"));
        fs::create_dir(&sub).unwrap();
        for f in 0..100 {
            fs::write(sub.join(format!("f{f:03}")), "x").unwrap();
        }
    }
    skill(&home.join(".agents/skills/many"), "description: Many.", "");
    let (guide, places) = scan(&env(&home));
    let many = find(&guide, "claude_code", "many");
    // The comparison with Codex's copy gave up instead of walking it all.
    assert!(!many.codex_differs);
    let read = body(&env(&home), &places, &many.id).unwrap();
    assert!(read.files_cut);
    assert!(read.files.len() <= 256);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn cuts_a_long_body_and_says_so() {
    let home = home();
    skill(
        &home.join(".claude/skills/long"),
        "description: Long.",
        &"á".repeat(BODY_BYTES),
    );
    let (guide, places) = scan(&env(&home));
    let read = body(
        &env(&home),
        &places,
        &find(&guide, "claude_code", "long").id,
    )
    .unwrap();
    assert!(read.cut);
    assert!(read.text.len() <= BODY_BYTES);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_translation_beside_a_skill_is_shown_and_changes_nothing_else() {
    let home = home();
    let claude = home.join(".claude/skills/tr");
    skill(
        &claude,
        "name: tr\ndescription: Does a thing.",
        "# tr\n\nEnglish.",
    );
    skill(
        &home.join(".agents/skills/tr"),
        "name: tr\ndescription: Does a thing.",
        "# tr\n\nEnglish.",
    );
    fs::write(
        claude.join(TRANSLATION),
        "---\nname: tr\ndescription: Hace algo.\n---\n# tr\n\nEspañol.",
    )
    .unwrap();
    let (guide, places) = scan(&env(&home));
    let entry = find(&guide, "claude_code", "tr");
    assert_eq!(entry.description, "Does a thing.");
    assert_eq!(entry.description_es.as_deref(), Some("Hace algo."));
    // The translation is not part of the skill: the two copies are still the same.
    assert!(!entry.codex_differs);
    let read = body(&env(&home), &places, &entry.id).unwrap();
    assert!(read.text.contains("English."));
    assert!(read.text_es.as_deref().unwrap().contains("Español."));
    // Codex's copy has none.
    let codex = find(&guide, "codex", "tr");
    assert_eq!(codex.description_es, None);
    assert_eq!(body(&env(&home), &places, &codex.id).unwrap().text_es, None);
    // A translation that is a link out of the roots is not read.
    let outside = std::env::temp_dir().join(format!("tr-out-{}", uuid::Uuid::new_v4().simple()));
    fs::write(&outside, "---\ndescription: Afuera.\n---\nAfuera.").unwrap();
    let linked = home.join(".claude/skills/lk");
    skill(&linked, "name: lk\ndescription: Link.", "Body.");
    symlink(&outside, linked.join(TRANSLATION)).unwrap();
    let (guide, places) = scan(&env(&home));
    let entry = find(&guide, "claude_code", "lk");
    assert_eq!(entry.description_es, None);
    assert_eq!(body(&env(&home), &places, &entry.id).unwrap().text_es, None);
}
