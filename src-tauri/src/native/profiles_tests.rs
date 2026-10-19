use super::{
    catalogue::AgentId,
    profile::{Body, HooksMode, Permission, ProfileError},
    profiles::{MAX_VERSIONS, ProfileStore},
};
use std::{fs, path::PathBuf};

fn store() -> (ProfileStore, PathBuf) {
    let dir = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-profiles-{}", uuid::Uuid::new_v4().simple()));
    (ProfileStore::open(dir.join("profiles")).unwrap(), dir)
}

fn body(name: &str) -> Body {
    Body {
        name: name.to_owned(),
        applies_to: vec![AgentId::ClaudeCode, AgentId::Codex],
        ..Body::default()
    }
}

#[test]
fn templates_ship_read_only_and_a_copy_is_how_one_becomes_editable() {
    let (store, dir) = store();
    let templates: Vec<_> = store.list().into_iter().filter(|s| s.template).collect();
    assert_eq!(templates.len(), 4);
    let id = &templates[0].id;
    assert_eq!(
        store.save(id, body("x")),
        Err(ProfileError::ReadOnlyTemplate)
    );
    assert_eq!(store.delete(id), Err(ProfileError::ReadOnlyTemplate));
    let copy = store.duplicate("template-review").unwrap();
    let copied = store.list().into_iter().find(|s| s.id == copy).unwrap();
    assert!(!copied.template);
    assert_eq!(copied.body.permission, Some(Permission::ReadOnly));
    assert_eq!(copied.body.name, "Review, read only copy");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn every_save_is_a_new_version_up_to_twenty_and_an_earlier_one_can_be_brought_back() {
    let (store, dir) = store();
    let id = store.create(body("p")).unwrap();
    // The same content again makes no version.
    assert_eq!(store.save(&id, body("p")), Ok(1));
    for n in 0..25 {
        let mut b = body("p");
        b.model = Some(format!("m{n}"));
        store.save(&id, b).unwrap();
    }
    let summary = store.list().into_iter().find(|s| s.id == id).unwrap();
    assert_eq!(summary.versions.len(), MAX_VERSIONS);
    assert_eq!(summary.current, 26);
    assert_eq!(summary.body.model.as_deref(), Some("m24"));
    let earlier = summary.versions[3].n;
    store.restore(&id, earlier).unwrap();
    let back = store.list().into_iter().find(|s| s.id == id).unwrap();
    assert_eq!(back.current, earlier);
    assert_eq!(
        back.versions.len(),
        MAX_VERSIONS,
        "nothing is deleted by going back"
    );
    assert_eq!(store.restore(&id, 999), Err(ProfileError::VersionUnknown));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_child_overrides_scalars_lists_combine_and_the_source_of_each_value_is_shown() {
    let (store, dir) = store();
    let mut parent = body("parent");
    parent.model = Some("opus".to_owned());
    parent.hooks = Some(HooksMode::Off);
    parent.skills_off = vec!["a".to_owned()];
    let parent_id = store.create(parent).unwrap();
    let mut child = body("child");
    child.extends = Some(parent_id.clone());
    child.model = Some("sonnet".to_owned());
    child.skills_off = vec!["b".to_owned()];
    let child_id = store.create(child).unwrap();
    let (resolved, _) = store.resolve(&child_id).unwrap();
    assert_eq!(resolved.body.model.as_deref(), Some("sonnet"));
    assert_eq!(resolved.body.hooks, Some(HooksMode::Off));
    assert_eq!(resolved.body.skills_off, ["a", "b"]);
    assert_eq!(resolved.from["model"], "child");
    assert_eq!(resolved.from["hooks"], "parent");
    // A parent another profile extends is not removed from under it.
    assert_eq!(store.delete(&parent_id), Err(ProfileError::InUse));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_cycle_and_too_deep_a_chain_are_refused_on_save() {
    let (store, dir) = store();
    let a = store.create(body("a")).unwrap();
    let mut b = body("b");
    b.extends = Some(a.clone());
    let b = store.create(b).unwrap();
    let mut a_again = body("a");
    a_again.extends = Some(b);
    assert_eq!(store.save(&a, a_again), Err(ProfileError::ProfileCycle));
    let mut itself = body("a");
    itself.extends = Some(a.clone());
    assert_eq!(store.save(&a, itself), Err(ProfileError::ProfileCycle));
    let mut missing = body("m");
    missing.extends = Some("0".repeat(32));
    assert_eq!(store.create(missing), Err(ProfileError::ExtendsUnknown));
    // Depth: eight is the most.
    let mut last = store.create(body("d0")).unwrap();
    for n in 1..8 {
        let mut next = body(&format!("d{n}"));
        next.extends = Some(last);
        last = store.create(next).unwrap();
    }
    let mut ninth = body("d8");
    ninth.extends = Some(last);
    assert_eq!(store.create(ninth), Err(ProfileError::TooDeep));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_profile_holds_closed_words_and_no_secret() {
    let (store, dir) = store();
    let mut bad = body("p");
    bad.model = Some("--dangerously-skip-permissions".to_owned());
    assert_eq!(store.create(bad), Err(ProfileError::ModelInvalid));
    let mut bad = body("p");
    bad.model = Some("a b; rm".to_owned());
    assert_eq!(store.create(bad), Err(ProfileError::ModelInvalid));
    let mut bad = body("p");
    bad.skills_off = vec!["../x".to_owned()];
    assert_eq!(store.create(bad), Err(ProfileError::SkillInvalid));
    let mut bad = body("p");
    bad.notes = concat!(
        "use sk-ant-",
        "api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG"
    )
    .to_owned();
    assert_eq!(store.create(bad), Err(ProfileError::NotesHoldSecret));
    let mut bad = body("");
    bad.applies_to = vec![];
    assert_eq!(store.create(bad), Err(ProfileError::NameInvalid));
    let mut bad = body("p");
    bad.applies_to = vec![];
    assert_eq!(store.create(bad), Err(ProfileError::AppliesToInvalid));
    // A word the schema does not have is not a field.
    let parsed: Result<Body, _> =
        serde_json::from_str(r#"{"name":"p","applies_to":["codex"],"command":"rm -rf /"}"#);
    assert!(parsed.is_err());
    let parsed: Result<Body, _> = serde_json::from_str(
        r#"{"name":"p","applies_to":["codex"],"permission":"bypass_permissions"}"#,
    );
    assert!(parsed.is_err(), "the dangerous mode is not a word");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn export_and_import_carry_one_profile_grant_nothing_and_never_overwrite() {
    let (store, dir) = store();
    let parent = store.create(body("parent")).unwrap();
    let mut b = body("Shared");
    b.model = Some("sonnet".to_owned());
    b.extends = Some(parent);
    let id = store.create(b).unwrap();
    let bytes = store.export(&id).unwrap();
    assert!(
        !String::from_utf8_lossy(&bytes).contains("parent"),
        "a local id means nothing elsewhere"
    );
    let first = store.import(&bytes).unwrap();
    let second = store.import(&bytes).unwrap();
    let names: Vec<String> = store.list().into_iter().map(|s| s.body.name).collect();
    assert!(names.contains(&"Shared".to_owned()));
    assert!(names.contains(&"Shared (2)".to_owned()));
    assert!(names.contains(&"Shared (3)".to_owned()));
    assert!(first != id && second != first);
    let imported = store.list().into_iter().find(|s| s.id == first).unwrap();
    assert_eq!(imported.body.extends, None);
    // Not a profile file, a file with something extra, one too large, a wrong version.
    for text in [
        "not json".to_owned(),
        r#"{"format":"wyss-session-profile","schema":1,"body":{"name":"x","applies_to":["codex"]},"run":"x"}"#.to_owned(),
        r#"{"format":"other","schema":1,"body":{"name":"x","applies_to":["codex"]}}"#.to_owned(),
        r#"{"format":"wyss-session-profile","schema":2,"body":{"name":"x","applies_to":["codex"]}}"#.to_owned(),
        " ".repeat(20_000),
    ] {
        assert_eq!(store.import(text.as_bytes()), Err(ProfileError::ImportInvalid), "{text:.40}");
    }
    fs::remove_dir_all(dir).unwrap();
}
