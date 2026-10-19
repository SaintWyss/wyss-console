//! The service against private temporary homes, with the real watcher: a file edited by
//! something else shows within the time AC-F1-02 asks for.
use super::inventory::{Environment, SourceKey};
use super::model::Bounds;
use super::service::{Changes, InventoryService};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let home = base.join(format!("wyss-svc-{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        Self(home)
    }
    fn put(&self, relative: &str, text: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // As an editor does: write beside it, then rename over.
        let beside = path.with_extension("tmp-write");
        fs::write(&beside, text).unwrap();
        fs::set_permissions(&beside, fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(&beside, &path).unwrap();
    }
    fn env(&self) -> Environment {
        Environment {
            home: self.0.clone(),
            claude_config_dir: None,
            codex_home: None,
            project: None,
            system: self.0.join("etc"),
            declared_writers: Vec::new(),
        }
    }
    fn service(&self) -> InventoryService {
        InventoryService::start(self.env(), Bounds::PROPOSED).unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn until(what: &str, mut condition: impl FnMut() -> bool) -> Duration {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "never happened: {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    started.elapsed()
}

#[test]
fn a_file_edited_outside_shows_within_two_seconds_and_names_its_source() {
    let home = Home::new();
    home.put(".claude/settings.json", r#"{ "model": "opus" }"#);
    let service = home.service();
    let first = service.snapshot();
    assert_eq!(first.revision, 1);

    home.put(".claude/settings.json", r#"{ "model": "sonnet" }"#);
    let took = until("revision moves", || {
        service.snapshot().revision > first.revision
    });
    assert!(took < Duration::from_secs(2), "took {took:?}");
    assert_eq!(
        service.changes_since(first.revision),
        Changes::Changed {
            revision: service.snapshot().revision,
            sources: vec![SourceKey::ClaudeUserSettings]
        }
    );
    service.shutdown();
}

#[test]
fn a_burst_of_writes_is_one_reading_or_two_not_one_per_write() {
    let home = Home::new();
    home.put(".claude/settings.json", r#"{ "model": "m0" }"#);
    let service = home.service();
    for n in 1..=25 {
        home.put(
            ".claude/settings.json",
            &format!(r#"{{ "model": "m{n}" }}"#),
        );
    }
    until("last write seen", || {
        service
            .snapshot()
            .inventory
            .items
            .iter()
            .any(|i| serde_json::to_string(&i.detail).is_ok_and(|text| text.contains("m25")))
    });
    assert!(
        service.snapshot().revision <= 4,
        "revision {}",
        service.snapshot().revision
    );
    service.shutdown();
}

#[test]
fn a_provider_directory_created_later_is_picked_up() {
    let home = Home::new();
    let service = home.service();
    assert!(service.snapshot().inventory.items.is_empty());
    home.put(".codex/config.toml", "model = \"gpt\"\n");
    until("directory seen", || {
        !service.snapshot().inventory.items.is_empty()
    });
    // And watched from then on.
    let seen = service.snapshot().revision;
    home.put(".codex/config.toml", "model = \"other\"\n");
    until("later edit seen", || service.snapshot().revision > seen);
    service.shutdown();
}

#[test]
fn what_nothing_changed_is_the_same_and_what_it_cannot_account_for_is_a_resync() {
    let home = Home::new();
    let service = home.service();
    let now = service.snapshot().revision;
    assert_eq!(service.changes_since(now), Changes::Same { revision: now });
    assert_eq!(
        service.changes_since(now + 50),
        Changes::Resync { revision: now }
    );
    assert_eq!(service.changes_since(0), Changes::Resync { revision: now });
    service.shutdown();
}

#[test]
fn the_log_is_bounded_and_older_asks_are_resynced() {
    let home = Home::new();
    home.put(".claude/settings.json", r#"{ "model": "m0" }"#);
    let service = home.service();
    for n in 1..=70 {
        home.put(
            ".claude/settings.json",
            &format!(r#"{{ "model": "m{n}" }}"#),
        );
        service.rescan();
    }
    let latest = service.snapshot().revision;
    assert!(matches!(service.changes_since(1), Changes::Resync { .. }));
    assert!(matches!(
        service.changes_since(latest - 3),
        Changes::Changed { .. }
    ));
    service.shutdown();
}

#[test]
fn an_unchanged_machine_does_not_move_the_revision() {
    let home = Home::new();
    home.put(".claude/settings.json", r#"{ "model": "opus" }"#);
    let service = home.service();
    let revision = service.snapshot().revision;
    assert!(!service.rescan());
    assert_eq!(service.snapshot().revision, revision);
    service.shutdown();
}

#[test]
fn selecting_a_project_adds_its_configuration_and_deselecting_removes_it() {
    let home = Home::new();
    home.put(
        "work/app/.mcp.json",
        r#"{ "mcpServers": { "local": {"command":"x"} } }"#,
    );
    let service = home.service();
    assert!(service.snapshot().inventory.items.is_empty());
    service.select_project(Some(home.0.join("work/app")));
    assert_eq!(service.snapshot().inventory.items.len(), 1);
    service.select_project(None);
    assert!(service.snapshot().inventory.items.is_empty());
    service.shutdown();
}

#[test]
fn the_watcher_stops_promptly_and_dropping_the_last_clone_stops_it_too() {
    let home = Home::new();
    let service = home.service();
    let clone = service.clone();
    let started = Instant::now();
    service.shutdown();
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(clone);

    let alone = home.service();
    let started = Instant::now();
    drop(alone);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn the_watcher_never_reads_through_a_link_planted_in_a_watched_directory() {
    let home = Home::new();
    let service = home.service();
    home.put("elsewhere/real.json", r#"{ "model": "SEKRET-planted" }"#);
    fs::create_dir_all(home.0.join(".claude")).unwrap();
    std::os::unix::fs::symlink(
        home.0.join("elsewhere/real.json"),
        home.0.join(".claude/settings.json"),
    )
    .unwrap();
    until("the link is looked at", || {
        service.snapshot().inventory.sources.iter().any(|s| {
            s.key == SourceKey::ClaudeUserSettings && !matches!(s.outcome, super::Outcome::Absent)
        })
    });
    let text = serde_json::to_string(&*service.snapshot().inventory).unwrap();
    assert!(!text.contains("SEKRET"));
    service.shutdown();
}

#[allow(dead_code)]
fn _paths(_: &Path) {}
