use super::*;
use crate::arsenal::store::{Store, identity};
use rusqlite::Connection;

#[test]
fn review_a_record_arriving_after_apply_is_not_acknowledged_until_the_next_run() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    preset(&mut acer, "First", vec![]);
    acer.sync();
    desk.sync();
    preset(&mut acer, "Late", vec![]);
    acer.sync();
    let device = device_of(&acer);
    let late = acer.paths.devices().join(&device).join(record_name(2));
    let held = dir.join("held-until-after-apply");
    fs::rename(&late, &held).unwrap();
    reconcile_at(
        &mut desk.store,
        &desk.paths,
        desk.host,
        MAX_BLOB_BYTES_PER_RUN,
        &mut |point| {
            if point == Point::BeforeAck {
                fs::rename(&held, &late).unwrap();
            }
        },
    )
    .unwrap();
    assert_eq!(desk.preset_names(), vec!["First"]);
    let journal = Dir::open(&desk.paths, &["v1", "devices", &device_of(&desk)], false)
        .unwrap()
        .unwrap();
    assert_eq!(
        read_applied(&journal).unwrap().seen[&device],
        1,
        "transport receipt after apply is not evidence of database application"
    );
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["First", "Late"]);
    assert_eq!(read_applied(&journal).unwrap().seen[&device], 2);
}

// ---------------------------------------------------------------------------------------------
// Acknowledgment: a record is acknowledged only once the library committed it

/// The highest record of `of` that `m` acknowledges as applied.
fn acked(m: &Machine, of: &Machine) -> u64 {
    let journal = Dir::open(&m.paths, &["v1", "devices", &device_of(m)], false)
        .unwrap()
        .unwrap();
    read_applied(&journal)
        .and_then(|applied| applied.seen.get(&device_of(of)).copied())
        .unwrap_or(0)
}

/// A computer whose library is a file, so a second connection can make its writes fail.
fn machine_on_disk(shared: &Path, host: &'static str) -> (Machine, PathBuf) {
    let db = shared.with_extension(format!("{host}.db"));
    let mut m = machine(shared, host);
    m.store = Store::open(Connection::open(&db).unwrap()).unwrap();
    (m, db)
}

fn own_records(m: &Machine) -> usize {
    let journal = m.paths.devices().join(device_of(m));
    fs::read_dir(journal)
        .unwrap()
        .flatten()
        .filter(|f| parse_record_name(&f.file_name().to_string_lossy()).is_some())
        .count()
}

#[test]
fn ack_a_record_arriving_during_the_final_scan_waits_for_the_next_run() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    preset(&mut acer, "First", vec![]);
    acer.sync();
    desk.sync();
    preset(&mut acer, "Late", vec![]);
    acer.sync();
    let late = acer
        .paths
        .devices()
        .join(device_of(&acer))
        .join(record_name(2));
    let held = dir.join("held-during-scan");
    fs::rename(&late, &held).unwrap();
    // The synchronizer finishes writing the file just after this run read the folder: every
    // later read of the run (conflicts, status, acknowledgment) sees it, the apply did not.
    let mut delivered = false;
    reconcile_at(
        &mut desk.store,
        &desk.paths,
        desk.host,
        MAX_BLOB_BYTES_PER_RUN,
        &mut |point| {
            if point == Point::Scanned && !delivered {
                fs::rename(&held, &late).unwrap();
                delivered = true;
            }
        },
    )
    .unwrap();
    assert!(delivered);
    assert_eq!(desk.preset_names(), vec!["First"]);
    assert_eq!(
        acked(&desk, &acer),
        1,
        "only the committed record is acknowledged"
    );
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["First", "Late"]);
    assert_eq!(acked(&desk, &acer), 2);
    let status = acer.sync();
    assert_eq!(status.peers[0].applied_mine, status.peers[0].published_mine);
}

#[test]
fn ack_a_failed_database_transaction_is_never_acknowledged_and_the_retry_applies_it() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let (mut desk, db) = machine_on_disk(&dir, "Santiago-PC");
    preset(&mut acer, "First", vec![]);
    acer.sync();
    desk.sync();
    assert_eq!(acked(&desk, &acer), 1);
    preset(&mut acer, "Second", vec![]);
    acer.skill("design", "one");
    acer.sync();
    let published = own_records(&acer) as u64;
    let applied_before = fs::read(
        desk.paths
            .devices()
            .join(device_of(&desk))
            .join("applied.json"),
    )
    .unwrap();
    let state_before = fs::read(desk.paths.local.join("state.json")).unwrap();
    // SQLite aborts the preset write inside its transaction, after the capability committed.
    let other = Connection::open(&db).unwrap();
    other
        .execute_batch(
            "CREATE TRIGGER injected_failure BEFORE INSERT ON preset \
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    for _ in 0..2 {
        assert!(reconcile(&mut desk.store, &desk.paths, desk.host).is_err());
        assert_eq!(
            desk.preset_names(),
            vec!["First"],
            "the aborted write left nothing"
        );
        assert_eq!(
            fs::read(
                desk.paths
                    .devices()
                    .join(device_of(&desk))
                    .join("applied.json")
            )
            .unwrap(),
            applied_before,
            "a failed run writes no acknowledgment"
        );
        assert_eq!(
            fs::read(desk.paths.local.join("state.json")).unwrap(),
            state_before
        );
    }
    assert_eq!(desk.store.integrity().unwrap(), "ok");
    // The capability committed before the failure; the retry must neither republish nor duplicate it.
    assert_eq!(desk.names(), vec!["design"]);
    other
        .execute_batch("DROP TRIGGER injected_failure;")
        .unwrap();
    // A failure inside a capability import is not acknowledged either.
    acer.skill("design", "two");
    acer.sync();
    let published_two = own_records(&acer) as u64;
    other
        .execute_batch(
            "CREATE TRIGGER injected_revision_failure BEFORE INSERT ON revision \
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    assert!(reconcile(&mut desk.store, &desk.paths, desk.host).is_err());
    assert_eq!(acked(&desk, &acer), 1);
    assert!(published > 1);
    other
        .execute_batch("DROP TRIGGER injected_revision_failure;")
        .unwrap();
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["First", "Second"]);
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert_eq!(desk.cap("design").revisions.len(), 2);
    assert_eq!(acked(&desk, &acer), published_two);
    assert_eq!(
        own_records(&desk),
        0,
        "applying a peer's records publishes nothing"
    );
    assert_eq!(desk.store.integrity().unwrap(), "ok");
}

#[test]
fn ack_a_committed_run_followed_by_retries_changes_nothing() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let design = acer.skill("design", "one");
    preset(
        &mut acer,
        "Uses design",
        vec![Item {
            capability: design,
            revision: None,
        }],
    );
    acer.sync();
    desk.sync();
    let applied = desk
        .paths
        .devices()
        .join(device_of(&desk))
        .join("applied.json");
    let ack = fs::read(&applied).unwrap();
    let names = (desk.names(), desk.preset_names());
    let records = record_files(&dir);
    let generation = desk.sync().generation;
    for _ in 0..3 {
        let status = desk.sync();
        assert_eq!(status.generation, generation, "a retry changes no library");
        assert!(status.conflicts.is_empty() && status.pending.is_empty());
    }
    assert_eq!((desk.names(), desk.preset_names()), names);
    assert_eq!(record_files(&dir), records, "a retry publishes nothing");
    assert_eq!(
        fs::read(&applied).unwrap(),
        ack,
        "the acknowledgment is not rewritten"
    );
    assert_eq!(acked(&desk, &acer), own_records(&acer) as u64);
}

#[test]
fn ack_duplicate_delivery_creates_no_duplicate_objects() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.skill("design", "one");
    preset(&mut acer, "First", vec![]);
    preset(&mut acer, "Second", vec![]);
    acer.sync();
    desk.sync();
    let journal = acer.paths.devices().join(device_of(&acer));
    let last = own_records(&acer) as u64;
    let file = journal.join(record_name(last));
    let bytes = fs::read(&file).unwrap();
    assert_eq!(acked(&desk, &acer), last);
    // The synchronizer delivers the same file again (a new inode with the same bytes), keeps a
    // conflict copy of it, and takes it away and brings it back.
    fs::remove_file(&file).unwrap();
    fs::write(&file, &bytes).unwrap();
    fs::write(
        journal.join(format!(
            "r-{last:010}.sync-conflict-20261101-120000-ABCDEFG.json"
        )),
        &bytes,
    )
    .unwrap();
    desk.sync();
    let held = dir.join("held-duplicate");
    fs::rename(&file, &held).unwrap();
    desk.sync();
    fs::rename(&held, &file).unwrap();
    let status = desk.sync();
    assert_eq!(desk.preset_names(), vec!["First", "Second"]);
    assert_eq!(desk.names(), vec!["design"]);
    assert_eq!(desk.cap("design").revisions.len(), 1);
    assert!(status.conflicts.is_empty(), "{:?}", status.conflicts);
    assert_eq!(own_records(&desk), 0, "nothing is republished");
    assert_eq!(acked(&desk, &acer), last);
    acer.sync();
    assert_eq!(acer.preset_names(), vec!["First", "Second"]);
}

#[test]
fn ack_an_interrupted_reconciliation_resumes_without_duplicates_or_false_acknowledgment() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.skill("design", "one");
    let first = preset(&mut acer, "First", vec![]);
    let gone = preset(&mut acer, "Gone", vec![]);
    acer.sync();
    desk.sync();
    let before = acked(&desk, &acer);
    acer.skill("design", "two");
    acer.store
        .put_preset(
            &first,
            &PresetInput {
                id: None,
                name: "First, edited".into(),
                description: "d".into(),
                items: vec![],
            },
        )
        .unwrap();
    acer.store.delete_preset(&gone).unwrap();
    acer.sync();
    // The process stops after the library committed the round and before its bookkeeping and
    // acknowledgment were written.
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reconcile_at(
            &mut desk.store,
            &desk.paths,
            desk.host,
            MAX_BLOB_BYTES_PER_RUN,
            &mut |point| {
                if point == Point::Applied {
                    panic!("interrupted");
                }
            },
        )
    }));
    assert!(stopped.is_err());
    assert_eq!(
        desk.preset_names(),
        vec!["First, edited"],
        "the library committed"
    );
    assert_eq!(
        acked(&desk, &acer),
        before,
        "nothing is acknowledged by a stopped run"
    );
    // The next runs on both computers converge without a conflict, a copy or a resurrection.
    let status = desk.sync();
    assert!(status.conflicts.is_empty(), "{:?}", status.conflicts);
    assert_eq!(
        own_records(&desk),
        0,
        "what was received is not published back"
    );
    acer.sync();
    let status = desk.sync();
    assert!(status.conflicts.is_empty(), "{:?}", status.conflicts);
    assert_eq!(acer.sync().conflicts, vec![]);
    for m in [&acer, &desk] {
        assert_eq!(m.preset_names(), vec!["First, edited"]);
        assert_eq!(m.names(), vec!["design"]);
        assert_eq!(m.cap("design").revisions.len(), 2);
        assert_eq!(m.store.integrity().unwrap(), "ok");
    }
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert_eq!(acked(&desk, &acer), own_records(&acer) as u64);
}

#[test]
fn ack_an_interrupted_first_application_is_not_republished_after_the_sender_moves_on() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let design = acer.skill("design", "one");
    let id = preset(
        &mut acer,
        "Uses design",
        vec![Item {
            capability: design,
            revision: None,
        }],
    );
    acer.sync();
    // The first application commits both objects and stops before its bookkeeping is saved.
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reconcile_at(
            &mut desk.store,
            &desk.paths,
            desk.host,
            MAX_BLOB_BYTES_PER_RUN,
            &mut |point| {
                if point == Point::Applied {
                    panic!("interrupted");
                }
            },
        )
    }));
    assert!(stopped.is_err());
    assert_eq!(desk.preset_names(), vec!["Uses design"]);
    assert_eq!(acked(&desk, &acer), 0);
    // Before it runs again, the sender edits both.
    acer.skill("design", "two");
    acer.store
        .put_preset(
            &id,
            &PresetInput {
                id: None,
                name: "Uses design, edited".into(),
                description: "d".into(),
                items: vec![],
            },
        )
        .unwrap();
    acer.sync();
    let status = desk.sync();
    assert!(status.conflicts.is_empty(), "{:?}", status.conflicts);
    assert_eq!(
        own_records(&desk),
        0,
        "what was received is not published back as a local edit"
    );
    assert_eq!(desk.preset_names(), vec!["Uses design, edited"]);
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert_eq!(acked(&desk, &acer), own_records(&acer) as u64);
    acer.sync();
    assert_eq!(acer.preset_names(), vec!["Uses design, edited"]);
}

fn rename_preset(m: &mut Machine, id: &str, name: &str) {
    m.store
        .put_preset(
            id,
            &PresetInput {
                id: None,
                name: name.into(),
                description: "d".into(),
                items: vec![],
            },
        )
        .unwrap();
}

/// Stops a run on `m` after the library committed its first round, before the bookkeeping.
fn interrupted_run(m: &mut Machine) {
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reconcile_at(
            &mut m.store,
            &m.paths,
            m.host,
            MAX_BLOB_BYTES_PER_RUN,
            &mut |point| {
                if point == Point::Applied {
                    panic!("interrupted");
                }
            },
        )
    }));
    assert!(stopped.is_err());
}

#[test]
fn ack_an_interrupted_run_past_several_received_versions_is_not_republished() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let id = preset(&mut acer, "First", vec![]);
    acer.skill("design", "one");
    acer.sync();
    desk.sync();
    assert_eq!(own_records(&desk), 0);
    // Two received versions past what the bookkeeping names, for a preset and a capability.
    rename_preset(&mut acer, &id, "E1");
    acer.skill("design", "two");
    acer.sync();
    rename_preset(&mut acer, &id, "E2");
    acer.skill("design", "three");
    acer.sync();
    interrupted_run(&mut desk);
    assert_eq!(desk.preset_names(), vec!["E2"]);
    // While the desk is down, the owner keeps editing on the Acer.
    rename_preset(&mut acer, &id, "E3");
    acer.skill("design", "four");
    acer.sync();
    let status = desk.sync();
    assert!(
        status.conflicts.is_empty(),
        "false conflict: {:?}",
        status.conflicts
    );
    assert_eq!(
        own_records(&desk),
        0,
        "received content republished as a local edit"
    );
    let status = acer.sync();
    assert!(status.conflicts.is_empty(), "{:?}", status.conflicts);
    for m in [&acer, &desk] {
        assert_eq!(m.preset_names(), vec!["E3"]);
        assert_eq!(m.cap("design").revisions.len(), 4);
    }
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert_eq!(acked(&desk, &acer), own_records(&acer) as u64);
}

#[test]
fn ack_an_interrupted_edit_then_deletion_is_not_republished() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let id = preset(&mut acer, "First", vec![]);
    acer.sync();
    desk.sync();
    rename_preset(&mut acer, &id, "E1");
    acer.sync();
    acer.store.delete_preset(&id).unwrap();
    acer.sync();
    interrupted_run(&mut desk);
    assert!(desk.preset_names().is_empty());
    let status = desk.sync();
    assert!(status.conflicts.is_empty(), "{:?}", status.conflicts);
    assert_eq!(
        own_records(&desk),
        0,
        "a received deletion is not published back"
    );
    acer.sync();
    assert!(acer.preset_names().is_empty());
    assert_eq!(acked(&desk, &acer), own_records(&acer) as u64);
}

#[test]
fn ack_a_local_revert_to_an_older_version_stays_an_owner_edit() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let id = preset(&mut acer, "First", vec![]);
    acer.sync();
    desk.sync();
    rename_preset(&mut acer, &id, "Second");
    acer.sync();
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["Second"]);
    // The owner, on the desk, puts the older name back: an edit, not a received version.
    rename_preset(&mut desk, &id, "First");
    desk.sync();
    assert_eq!(own_records(&desk), 1, "the revert is published");
    acer.sync();
    assert_eq!(acer.preset_names(), vec!["First"]);
    assert_eq!(desk.preset_names(), vec!["First"]);
}

#[test]
fn review_the_watcher_never_watches_through_a_planted_link() {
    use std::os::unix::fs::symlink;
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.sync();
    desk.sync();
    let outside = dir.with_extension("watch-outside");
    fs::create_dir_all(outside.join("e".repeat(32))).unwrap();
    let names = |dirs: Vec<(String, Dir)>| -> BTreeSet<String> {
        dirs.into_iter().map(|(name, _)| name).collect()
    };
    let all = names(watch_dirs(&acer.paths, &BTreeSet::new()));
    assert!(all.contains("devices") && all.contains("blobs"));
    assert!(all.contains(&device_of(&acer)) && all.contains(&device_of(&desk)));
    // A peer folder that is a link is not watched.
    symlink(
        outside.join("e".repeat(32)),
        acer.paths.devices().join("f".repeat(32)),
    )
    .unwrap();
    assert!(!names(watch_dirs(&acer.paths, &BTreeSet::new())).contains(&"f".repeat(32)));
    // Nor is anything under a `devices` folder replaced by a link.
    let devices = acer.paths.devices();
    fs::rename(&devices, dir.with_extension("devices-moved")).unwrap();
    symlink(&outside, &devices).unwrap();
    let left = names(watch_dirs(&acer.paths, &BTreeSet::new()));
    assert_eq!(left, BTreeSet::from(["blobs".to_owned()]), "{left:?}");
    // Folders already watched are not returned again.
    fs::remove_file(&devices).unwrap();
    fs::rename(dir.with_extension("devices-moved"), &devices).unwrap();
    assert!(watch_dirs(&acer.paths, &all).is_empty());
}

/// One computer: its own library and bookkeeping, and the folder both computers share.
struct Machine {
    store: Store,
    paths: Paths,
    host: &'static str,
}

fn shared() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("arsenal-sync-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn machine(shared: &Path, host: &'static str) -> Machine {
    let local = shared.with_extension(format!("local-{host}"));
    Machine {
        store: Store::open(Connection::open_in_memory().unwrap()).unwrap(),
        paths: Paths {
            shared: shared.join("arsenal"),
            local,
        },
        host,
    }
}

impl Machine {
    fn sync(&mut self) -> Status {
        reconcile(&mut self.store, &self.paths, self.host).unwrap()
    }
    fn skill(&mut self, name: &str, body: &str) -> String {
        self.store.import(&skill(name, body)).unwrap().capability
    }
    fn tree(&self, name: &str) -> String {
        let c = self.cap(name);
        c.revisions
            .iter()
            .find(|r| r.number == c.current)
            .unwrap()
            .tree_hash
            .clone()
    }
    fn cap(&self, name: &str) -> SyncCapability {
        self.store
            .sync_capabilities()
            .unwrap()
            .into_iter()
            .find(|c| c.name == name)
            .unwrap()
    }
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .store
            .list()
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        names.sort();
        names
    }
    fn preset_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .store
            .presets()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        names.sort();
        names
    }
}

fn skill(name: &str, body: &str) -> NewCapability {
    NewCapability {
        identity: identity("claude_code", "skill", "claude_user_skills", name),
        provider: "claude_code".into(),
        source: "claude_user_skills".into(),
        kind: "skill".into(),
        name: name.into(),
        description: format!("{name} skill"),
        origin: format!("~/.claude/skills/{name}"),
        files: vec![
            ("SKILL.md".into(), body.as_bytes().to_vec()),
            ("refs/notes.md".into(), b"notes".to_vec()),
        ],
    }
}

fn preset(m: &mut Machine, name: &str, items: Vec<Item>) -> String {
    m.store
        .save_preset(&PresetInput {
            id: None,
            name: name.into(),
            description: "d".into(),
            items,
        })
        .unwrap()
        .id
}

fn record_files(shared: &Path) -> usize {
    let devices = shared.join("arsenal/v1/devices");
    fs::read_dir(devices)
        .map(|d| {
            d.flatten()
                .map(|dir| {
                    fs::read_dir(dir.path())
                        .unwrap()
                        .flatten()
                        .filter(|f| f.file_name().to_string_lossy().starts_with("r-"))
                        .count()
                })
                .sum()
        })
        .unwrap_or(0)
}

#[test]
fn the_first_sync_brings_the_whole_library_with_every_revision_and_is_idempotent() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let design = acer.skill("design", "one");
    acer.skill("design", "two");
    let motion = acer.skill("motion", "gsap");
    let pinned = preset(
        &mut acer,
        "Frontend",
        vec![
            Item {
                capability: design.clone(),
                revision: Some(1),
            },
            Item {
                capability: motion.clone(),
                revision: None,
            },
        ],
    );
    acer.store
        .save_selection(&SelectionInput {
            id: None,
            name: "Worker".into(),
            provider: "claude_code".into(),
            items: vec![Item {
                capability: motion,
                revision: None,
            }],
            composites: vec!["gsd@1.0".into()],
        })
        .unwrap();
    acer.sync();
    let status = desk.sync();
    assert_eq!(desk.names(), vec!["design", "motion"]);
    assert_eq!(desk.tree("design"), acer.tree("design"));
    // Every revision travels, and the current one stays current.
    assert_eq!(desk.cap("design").revisions.len(), 2);
    // A preset keeps its id, its name, and its pin to the first revision's content.
    let there = desk
        .store
        .presets()
        .unwrap()
        .into_iter()
        .find(|p| p.id == pinned)
        .unwrap();
    let local_design = desk.cap("design");
    let pinned_rev = there.items[0].revision.unwrap();
    let first_tree = &acer.cap("design").revisions[0].tree_hash;
    assert_eq!(
        local_design
            .revisions
            .iter()
            .find(|r| r.number == pinned_rev)
            .unwrap()
            .tree_hash,
        *first_tree
    );
    assert_eq!(there.items[0].capability, local_design.id);
    let selection = &desk.store.selections().unwrap()[0];
    assert_eq!(
        (selection.name.as_str(), selection.composites.clone()),
        ("Worker", vec!["gsd@1.0".to_owned()])
    );
    assert!(status.conflicts.is_empty() && status.rejected.is_empty() && status.pending.is_empty());

    // Again and again: nothing new is written and nothing changes.
    let files = record_files(&dir);
    let generation = desk.sync().generation;
    for _ in 0..3 {
        acer.sync();
        assert_eq!(desk.sync().generation, generation);
    }
    assert_eq!(record_files(&dir), files);
    assert_eq!(desk.store.integrity().unwrap(), "ok");
    assert_eq!(acer.store.integrity().unwrap(), "ok");
}

#[test]
fn changes_travel_both_ways_and_independent_ones_merge() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.skill("design", "one");
    let id = preset(&mut acer, "Frontend", vec![]);
    acer.sync();
    desk.sync();
    // Acer → desktop: a new revision.
    acer.skill("design", "two");
    acer.sync();
    desk.sync();
    assert_eq!(desk.tree("design"), acer.tree("design"));
    // Desktop → Acer: a preset edit and a new skill.
    desk.store
        .save_preset(&PresetInput {
            id: Some(id.clone()),
            name: "Frontend 2".into(),
            description: "x".into(),
            items: vec![],
        })
        .unwrap();
    desk.skill("tokens", "t");
    desk.sync();
    acer.sync();
    assert_eq!(acer.preset_names(), vec!["Frontend 2"]);
    assert_eq!(acer.names(), vec!["design", "tokens"]);
    // Independent changes on both, neither online: both survive, no conflict.
    acer.skill("motion", "m");
    desk.store
        .save_preset(&PresetInput {
            id: Some(id),
            name: "Frontend 3".into(),
            description: "x".into(),
            items: vec![],
        })
        .unwrap();
    acer.sync();
    let status = desk.sync();
    acer.sync();
    assert_eq!(acer.names(), desk.names());
    assert_eq!(acer.preset_names(), vec!["Frontend 3"]);
    assert!(status.conflicts.is_empty());
}

#[test]
fn a_preset_edited_on_both_computers_keeps_both_versions_and_converges() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let id = preset(&mut acer, "Frontend", vec![]);
    acer.sync();
    desk.sync();
    let edit = |m: &mut Machine, name: &str| {
        m.store
            .save_preset(&PresetInput {
                id: Some(id.clone()),
                name: name.into(),
                description: "".into(),
                items: vec![],
            })
            .unwrap();
    };
    edit(&mut acer, "From acer");
    edit(&mut desk, "From desk");
    acer.sync();
    desk.sync();
    acer.sync();
    desk.sync();
    // Same library on both, and neither edit lost.
    assert_eq!(acer.preset_names(), desk.preset_names());
    let names = acer.preset_names();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("From acer")));
    assert!(names.iter().any(|n| n.starts_with("From desk")));
    assert!(names.iter().any(|n| n.contains("(conflict, ")));
    // Both explain it; and it is over: more runs write nothing.
    let a = acer.sync();
    let d = desk.sync();
    assert!(!a.conflicts.is_empty() || !d.conflicts.is_empty());
    let files = record_files(&dir);
    acer.sync();
    desk.sync();
    assert_eq!(record_files(&dir), files);
}

#[test]
fn a_deletion_travels_but_never_wins_over_an_edit() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let gone = preset(&mut acer, "Gone", vec![]);
    let kept = preset(&mut acer, "Kept", vec![]);
    acer.sync();
    desk.sync();
    acer.store.delete_preset(&gone).unwrap();
    acer.sync();
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["Kept"]);
    // Deleted on one computer while edited on the other, offline: the edit survives everywhere.
    acer.store.delete_preset(&kept).unwrap();
    desk.store
        .save_preset(&PresetInput {
            id: Some(kept),
            name: "Kept and edited".into(),
            description: "".into(),
            items: vec![],
        })
        .unwrap();
    acer.sync();
    desk.sync();
    acer.sync();
    assert_eq!(acer.preset_names(), vec!["Kept and edited"]);
    assert_eq!(desk.preset_names(), vec!["Kept and edited"]);
    let (records, _) = read_records(&desk.paths).unwrap();
    assert!(materialize(&records).values().all(|view| view.complete));
    assert!(desk.sync().pending.is_empty());
    let count = record_files(&dir);
    acer.sync();
    desk.sync();
    assert_eq!(record_files(&dir), count);
}

#[test]
fn a_record_whose_files_have_not_arrived_waits_and_then_applies() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let design = acer.skill("design", "one");
    preset(
        &mut acer,
        "Uses design",
        vec![Item {
            capability: design,
            revision: None,
        }],
    );
    acer.sync();
    // The synchronizer delivered the journal before the blobs.
    let blobs = dir.join("arsenal/v1/blobs");
    let held = dir.join("held-blobs");
    fs::rename(&blobs, &held).unwrap();
    let status = desk.sync();
    assert!(desk.names().is_empty());
    assert!(desk.preset_names().is_empty());
    assert_eq!(status.pending.len(), 2, "{:?}", status.pending);
    fs::remove_dir_all(&blobs).ok();
    fs::rename(&held, &blobs).unwrap();
    let status = desk.sync();
    assert_eq!(desk.names(), vec!["design"]);
    assert_eq!(desk.preset_names(), vec!["Uses design"]);
    assert!(status.pending.is_empty());
}

#[test]
fn an_interrupted_run_resumes_without_rewriting_a_published_record() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    acer.skill("design", "one");
    acer.sync();
    // The bookkeeping is lost after publishing (a crash before it was saved, or a restored backup).
    let state = acer.paths.local.join("state.json");
    let before: Local = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    let journal = acer.paths.devices().join(&before.device);
    let first = fs::read(journal.join(record_name(1))).unwrap();
    let rolled = Local {
        next_seq: 1,
        objects: BTreeMap::new(),
        ..before
    };
    fs::write(&state, serde_json::to_vec(&rolled).unwrap()).unwrap();
    // A half-written staging file was left behind too.
    fs::write(acer.paths.staging().join(".partial.tmp"), b"{").unwrap();
    acer.skill("design", "two");
    acer.sync();
    assert_eq!(
        fs::read(journal.join(record_name(1))).unwrap(),
        first,
        "a record is never rewritten"
    );
    let mut desk = machine(&dir, "Santiago-PC");
    desk.sync();
    assert_eq!(desk.tree("design"), acer.tree("design"));
}

#[test]
fn malformed_unsupported_and_hostile_records_are_refused_and_change_nothing() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.sync();
    let device = "f".repeat(32);
    let journal = acer.paths.devices().join(&device);
    fs::create_dir_all(&journal).unwrap();
    let put_blob = |bytes: &[u8]| {
        let hash = store::digest(bytes);
        fs::write(acer.paths.blobs().join(&hash), bytes).unwrap();
        FileRef {
            path: "SKILL.md".into(),
            sha256: hash,
            size: bytes.len() as u64,
        }
    };
    let capability = |name: &str, files: Vec<FileRef>| {
        let mut entries: Vec<(&str, &str)> = files
            .iter()
            .map(|f| (f.path.as_str(), f.sha256.as_str()))
            .collect();
        let tree = tree_of_digests(&mut entries);
        Body::Capability(CapabilityBody {
            provider: "claude_code".into(),
            source: "claude_user_skills".into(),
            kind: "skill".into(),
            name: name.into(),
            description: "".into(),
            origin: "~".into(),
            tree_hash: tree,
            files,
        })
    };
    let write = |seq: u64, object: String, body: Body| {
        let record = Record {
            schema: SCHEMA.into(),
            device: device.clone(),
            host: "intruder".into(),
            seq,
            at: 1,
            object,
            base: None,
            merged: vec![],
            body,
        };
        fs::write(
            journal.join(record_name(seq)),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    };
    let id = |name: &str| {
        format!(
            "capability/{}",
            identity("claude_code", "skill", "claude_user_skills", name)
        )
    };
    // A secret value inside an otherwise valid skill.
    // Assembled at run time, so the repository scan does not take the fixture for a real token.
    let secret =
        put_blob(format!("export TOKEN={}0123456789abcdefghijklmnop\n", "ghp_").as_bytes());
    write(1, id("leaky"), capability("leaky", vec![secret]));
    // A credential by its name.
    let mut named = put_blob(b"# Skill\n");
    named.path = "credentials.json".into();
    let skill_md = put_blob(b"# Skill\n");
    write(
        2,
        id("named"),
        capability("named", vec![skill_md.clone(), named]),
    );
    // A path that climbs out.
    let mut climbing = skill_md.clone();
    climbing.path = "../../.bashrc".into();
    write(3, id("climb"), capability("climb", vec![climbing]));
    // The identity of one capability with another one's name.
    write(
        4,
        id("someone-else"),
        capability("impostor", vec![skill_md.clone()]),
    );
    // A Claude identity claiming to be a Codex skill.
    let mut codex = capability("cross", vec![skill_md.clone()]);
    if let Body::Capability(c) = &mut codex {
        c.provider = "codex".into();
    }
    write(5, id("cross"), codex);
    // A digest that lies about its content.
    let mut lying = put_blob(b"# real\n");
    lying.sha256 = store::digest(b"# other\n");
    let mut body = capability("lying", vec![lying.clone()]);
    if let Body::Capability(c) = &mut body {
        c.files = vec![lying];
    }
    write(6, id("lying"), body);
    // Not JSON; unknown fields; a newer schema (left alone, not refused).
    fs::write(journal.join(record_name(7)), b"{not json").unwrap();
    fs::write(
        journal.join(record_name(8)),
        br#"{"schema":"wyss.arsenal.sync.v1","device":"x","extra":1}"#,
    )
    .unwrap();
    fs::write(
        journal.join(record_name(9)),
        br#"{"schema":"wyss.arsenal.sync.v9","anything":true}"#,
    )
    .unwrap();
    // A record stored under another device's name.
    let other = acer.paths.devices().join("e".repeat(32));
    fs::create_dir_all(&other).unwrap();
    fs::copy(journal.join(record_name(2)), other.join(record_name(2))).unwrap();

    let status = desk.sync();
    assert!(
        desk.names().is_empty(),
        "nothing hostile is imported: {:?}",
        desk.names()
    );
    assert!(status.rejected.len() >= 6, "{:?}", status.rejected);
    assert!(
        status
            .rejected
            .iter()
            .any(|n| n.detail.contains("may_hold_secret"))
    );
    assert!(
        status
            .rejected
            .iter()
            .any(|n| n.detail.contains("credential_name"))
    );
    assert_eq!(desk.store.integrity().unwrap(), "ok");
    // Refused once, reported once.
    let again = desk.sync();
    assert_eq!(again.rejected.len(), status.rejected.len());
}

#[test]
fn providers_and_pins_are_kept_and_nothing_machine_local_is_shared() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let codex = NewCapability {
        identity: identity("codex", "skill", "codex_user_skills", "plan"),
        provider: "codex".into(),
        source: "codex_user_skills".into(),
        kind: "skill".into(),
        name: "plan".into(),
        description: "".into(),
        origin: "~/.codex/skills/plan".into(),
        files: vec![("SKILL.md".into(), b"# plan".to_vec())],
    };
    acer.store.import(&codex).unwrap();
    acer.skill("plan", "# claude plan");
    acer.sync();
    desk.sync();
    let mut providers: Vec<(String, String)> = desk
        .store
        .list()
        .unwrap()
        .into_iter()
        .map(|s| (s.name, s.provider))
        .collect();
    providers.sort();
    assert_eq!(
        providers,
        vec![
            ("plan".into(), "claude_code".into()),
            ("plan".into(), "codex".into())
        ]
    );
    // The shared folder holds journals, acknowledgments and blobs; never bookkeeping, a database or a lock.
    let mut shared_files = Vec::new();
    let mut stack = vec![dir.join("arsenal")];
    while let Some(next) = stack.pop() {
        for entry in fs::read_dir(next).unwrap().flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                shared_files.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    assert!(
        shared_files
            .iter()
            .all(|name| name.starts_with("r-") || name == "applied.json" || hex64(name))
    );
    assert!(!acer.paths.local.starts_with(dir.join("arsenal")));
}

#[test]
fn the_library_schema_is_unchanged_so_an_older_version_still_opens_it() {
    let dir = shared();
    let path = dir.join("library.sqlite");
    {
        let mut m = Machine {
            store: Store::open(Connection::open(&path).unwrap()).unwrap(),
            paths: Paths {
                shared: dir.join("arsenal"),
                local: dir.join("local"),
            },
            host: "saint-acer",
        };
        m.skill("design", "one");
        m.sync();
    }
    let db = Connection::open(&path).unwrap();
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        tables,
        vec![
            "blob",
            "capability",
            "preset",
            "project_selection",
            "revision",
            "revision_file",
            "selection",
            "terminal_binding"
        ]
    );
}

#[test]
fn one_run_at_a_time_on_one_computer() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    acer.sync();
    let lock = File::create(acer.paths.local.join("lock")).unwrap();
    let held = nix::fcntl::Flock::lock(lock, nix::fcntl::FlockArg::LockExclusiveNonblock).unwrap();
    assert_eq!(
        reconcile(&mut acer.store, &acer.paths, "saint-acer").unwrap_err(),
        "busy"
    );
    drop(held);
    acer.sync();
}

#[test]
fn a_peer_acknowledges_what_it_applied() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.skill("design", "one");
    let published = acer.sync();
    assert_eq!(published.peers.len(), 0);
    desk.sync();
    let status = acer.sync();
    assert_eq!(status.peers.len(), 1);
    let peer = &status.peers[0];
    assert_eq!(peer.host, "Santiago-PC");
    assert_eq!(peer.applied_mine, peer.published_mine);
    assert!(peer.published_mine > 0);
}

fn device_of(m: &Machine) -> String {
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(m.paths.local.join("state.json")).unwrap()).unwrap();
    state["device"].as_str().unwrap().to_owned()
}

fn empty(dir: &Path) -> bool {
    fs::read_dir(dir).unwrap().next().is_none()
}

#[test]
fn a_link_planted_in_the_shared_folder_redirects_no_write_and_no_read() {
    use std::os::unix::fs::symlink;
    let dir = shared();
    let outside = dir.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.sync();
    desk.sync();
    // Another computer replaces this one's journal folder with a link to a folder of its choice.
    let journal = acer.paths.devices().join(device_of(&acer));
    fs::rename(&journal, dir.with_extension("moved")).unwrap();
    symlink(&outside, &journal).unwrap();
    preset(&mut acer, "Redirected", vec![]);
    let error = reconcile(&mut acer.store, &acer.paths, acer.host).unwrap_err();
    assert!(error.contains("not a plain folder"), "{error}");
    assert!(empty(&outside), "nothing is written through the link");
    // The same at the shared files' folder.
    fs::remove_file(&journal).unwrap();
    fs::rename(dir.with_extension("moved"), &journal).unwrap();
    let blobs = acer.paths.blobs();
    fs::rename(&blobs, dir.with_extension("blobs-moved")).unwrap();
    symlink(&outside, &blobs).unwrap();
    acer.skill("redirected", "# Redirected\n");
    assert!(reconcile(&mut acer.store, &acer.paths, acer.host).is_err());
    assert!(empty(&outside));
    fs::remove_file(&blobs).unwrap();
    fs::rename(dir.with_extension("blobs-moved"), &blobs).unwrap();
    acer.sync();
    // A peer folder that is a link, and a record that is a pipe, are refused, not followed or waited on.
    fs::create_dir_all(outside.join("r")).unwrap();
    symlink(outside.join("r"), acer.paths.devices().join("e".repeat(32))).unwrap();
    let peer = desk.paths.devices().join(device_of(&desk));
    nix::unistd::mkfifo(
        &peer.join("r-0000009999.json"),
        nix::sys::stat::Mode::S_IRWXU,
    )
    .unwrap();
    let status = acer.sync();
    let refused: Vec<&str> = status.rejected.iter().map(|n| n.detail.as_str()).collect();
    assert!(refused.contains(&"not a plain folder"), "{refused:?}");
    assert!(refused.contains(&"not a record file"), "{refused:?}");
    assert_eq!(acer.preset_names(), vec!["Redirected"]);
    let _ = fs::remove_file(peer.join("r-0000009999.json"));
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["Redirected"]);
    assert!(desk.names().contains(&"redirected".to_owned()));
}

#[test]
fn a_shared_folder_larger_than_this_version_reads_fails_closed() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    acer.sync();
    let before = acer.names();
    for n in 0..MAX_DEVICE_ENTRIES {
        fs::create_dir_all(acer.paths.devices().join(format!("{n:032x}"))).unwrap();
    }
    let error = reconcile(&mut acer.store, &acer.paths, acer.host).unwrap_err();
    assert!(
        error.contains("more files than this version reads"),
        "{error}"
    );
    assert_eq!(acer.names(), before);
    let status = read_status(&acer.paths, acer.host);
    assert!(status.error.is_some());
}

#[test]
fn what_another_computer_deletes_is_kept_here_and_reported() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let gone = preset(&mut acer, "Deleted elsewhere", vec![]);
    acer.sync();
    desk.sync();
    acer.store.delete_preset(&gone).unwrap();
    acer.sync();
    let status = desk.sync();
    assert!(desk.preset_names().is_empty());
    assert_eq!(status.removed.len(), 1);
    let note = &status.removed[0];
    assert_eq!(note.name, "Deleted elsewhere");
    assert!(
        note.detail.contains("deleted on saint-acer"),
        "{}",
        note.detail
    );
    let kept: Vec<PathBuf> = fs::read_dir(desk.paths.local.join("trash"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(kept.len(), 1);
    let body: Body = serde_json::from_slice(&fs::read(&kept[0]).unwrap()).unwrap();
    assert!(matches!(body, Body::Preset(p) if p.name == "Deleted elsewhere"));
    // The computer that deleted it keeps no copy: it was deleted there by its owner.
    assert!(!acer.paths.local.join("trash").exists());
}

#[test]
fn review_acknowledgment_does_not_skip_a_record_still_in_transport() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    preset(&mut acer, "First", vec![]);
    preset(&mut acer, "Second", vec![]);
    acer.sync();
    let device = device_of(&acer);
    let first = acer.paths.devices().join(&device).join(record_name(1));
    let held = dir.join("held-record");
    fs::rename(&first, &held).unwrap();
    desk.sync();
    let journal = Dir::open(&desk.paths, &["v1", "devices", &device_of(&desk)], false)
        .unwrap()
        .unwrap();
    let applied = read_applied(&journal).unwrap();
    assert_eq!(applied.seen.get(&device).copied().unwrap_or(0), 0);
    fs::rename(held, first).unwrap();
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["First", "Second"]);
    assert_eq!(read_applied(&journal).unwrap().seen[&device], 2);
}

#[test]
fn review_refused_content_is_not_acknowledged_as_applied() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    // The store accepts raw library bytes; incoming synchronization applies the stricter
    // import boundary. A refusal must never become an "applied" acknowledgment.
    let mut secret = skill("credential-fixture", "safe");
    secret.files.push(("auth.json".into(), b"{}".to_vec()));
    acer.store.import(&secret).unwrap();
    acer.sync();
    let status = desk.sync();
    assert!(!status.rejected.is_empty());
    let journal = Dir::open(&desk.paths, &["v1", "devices", &device_of(&desk)], false)
        .unwrap()
        .unwrap();
    assert_eq!(
        read_applied(&journal)
            .unwrap()
            .seen
            .get(&device_of(&acer))
            .copied()
            .unwrap_or(0),
        0
    );
}

#[test]
fn review_late_history_is_imported_even_when_the_current_head_is_unchanged() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let id = acer.skill("design", "old");
    acer.skill("design", "new");
    preset(
        &mut acer,
        "Pinned",
        vec![Item {
            capability: id,
            revision: Some(1),
        }],
    );
    acer.sync();
    let first = acer
        .paths
        .devices()
        .join(device_of(&acer))
        .join(record_name(1));
    let held = dir.join("held-history");
    fs::rename(&first, &held).unwrap();
    let before = record_files(&dir);
    for _ in 0..2 {
        let status = desk.sync();
        assert!(
            status
                .pending
                .iter()
                .any(|n| n.detail.contains("journal history"))
        );
        assert_eq!(desk.cap("design").revisions.len(), 1);
        let journal = Dir::open(&desk.paths, &["v1", "devices", &device_of(&desk)], false)
            .unwrap()
            .unwrap();
        assert_eq!(
            read_applied(&journal).unwrap().seen.get(&device_of(&acer)),
            None
        );
        assert_eq!(
            record_files(&dir),
            before,
            "partial imports are not owner edits"
        );
    }
    fs::rename(held, first).unwrap();
    let status = desk.sync();
    assert_eq!(desk.cap("design").revisions.len(), 2);
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert_eq!(desk.preset_names(), vec!["Pinned"]);
    assert!(status.pending.is_empty());
}

#[test]
fn review_history_larger_than_one_run_eventually_finishes() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.skill("design", "old");
    acer.skill("design", "new");
    acer.sync();
    let (records, _) = read_records(&acer.paths).unwrap();
    let views = materialize(&records);
    let view = views.values().next().unwrap();
    let Body::Capability(body) = &records[view.chosen].body else {
        panic!()
    };
    let allowance = body.files.iter().map(|f| f.size).sum::<u64>();
    let mut budget = allowance;
    assert!(matches!(
        apply_capability(&mut desk.store, &desk.paths, &records, view, &mut budget),
        Err(Applying::Wait(_))
    ));
    let mut budget = allowance;
    assert!(apply_capability(&mut desk.store, &desk.paths, &records, view, &mut budget).is_ok());
    assert_eq!(desk.cap("design").revisions.len(), 2);
    assert_eq!(desk.tree("design"), acer.tree("design"));
}

#[test]
fn review_independent_conflict_merges_do_not_resurrect_a_later_deletion() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let id = preset(&mut acer, "Original", vec![]);
    acer.sync();
    desk.sync();
    let key = format!("preset/{id}");
    // Both offline edits use the same base. Both computers then resolve the same snapshot
    // before either machine receives the other machine's merge.
    for m in [&mut acer, &mut desk] {
        let mut local = load_local(&m.paths).unwrap();
        let journal = Dir::open(&m.paths, &["v1", "devices", &local.device], false)
            .unwrap()
            .unwrap();
        let base = local.objects[&key].record.clone();
        publish(
            &m.paths,
            &journal,
            m.host,
            &mut local,
            &key,
            base,
            vec![],
            Body::Preset(PresetBody {
                name: m.host.into(),
                description: "edit".into(),
                items: vec![],
            }),
        )
        .unwrap();
    }
    let (records, _) = read_records(&acer.paths).unwrap();
    let views = materialize(&records);
    assert!(views[&key].conflict);
    for m in [&mut acer, &mut desk] {
        let mut local = load_local(&m.paths).unwrap();
        let journal = Dir::open(&m.paths, &["v1", "devices", &local.device], false)
            .unwrap()
            .unwrap();
        resolve_conflicts(&m.paths, &journal, m.host, &mut local, &records, &views).unwrap();
    }
    acer.sync();
    desk.sync();
    acer.sync();
    acer.store.delete_preset(&id).unwrap();
    acer.sync();
    desk.sync();
    acer.sync();
    assert!(!acer.store.presets().unwrap().iter().any(|p| p.id == id));
    assert!(!desk.store.presets().unwrap().iter().any(|p| p.id == id));
    assert_eq!(acer.preset_names(), desk.preset_names());
}

#[test]
fn review_directory_limit_counts_non_utf8_names() {
    use std::os::unix::ffi::OsStringExt;
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    acer.sync();
    let folder = Dir::open(&acer.paths, &["v1", "blobs"], false)
        .unwrap()
        .unwrap();
    for n in 0..3 {
        let name = std::ffi::OsString::from_vec(vec![0xff, n + 1]);
        fs::write(acer.paths.blobs().join(name), b"").unwrap();
    }
    assert!(folder.names(2).is_err());
}

#[test]
fn review_excess_devices_fail_instead_of_silently_omitting_a_peer() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    acer.sync();
    for n in 0..MAX_DEVICES {
        fs::create_dir_all(acer.paths.devices().join(format!("{n:032x}"))).unwrap();
    }
    assert!(read_records(&acer.paths).is_err());
}

#[test]
fn review_global_record_limit_counts_rejected_files_before_applying_anything() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    preset(&mut acer, "Must not partially apply", vec![]);
    acer.sync();
    for n in 0..2 {
        let folder = acer.paths.devices().join(format!("{n:032x}"));
        fs::create_dir_all(&folder).unwrap();
        for seq in 1..=(MAX_RECORDS / 2 + 1) {
            fs::write(folder.join(record_name(seq as u64)), b"").unwrap();
        }
    }
    assert!(reconcile(&mut desk.store, &desk.paths, desk.host).is_err());
    assert!(desk.preset_names().is_empty());
}

#[test]
fn review_oversize_tree_is_rejected_before_reading_its_blobs() {
    let mut body = CapabilityBody {
        provider: "claude_code".into(),
        source: "claude_user_skills".into(),
        kind: "skill".into(),
        name: "large".into(),
        description: "".into(),
        origin: "".into(),
        tree_hash: String::new(),
        files: vec![],
    };
    for n in 0..=(MAX_TREE_BYTES / MAX_FILE_BYTES) {
        body.files.push(FileRef {
            path: format!("f-{n}.md"),
            sha256: "a".repeat(64),
            size: MAX_FILE_BYTES as u64,
        });
    }
    let mut entries: Vec<_> = body
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();
    body.tree_hash = tree_of_digests(&mut entries);
    assert!(
        check_capability(
            &identity("claude_code", "skill", "claude_user_skills", "large"),
            &body
        )
        .is_err()
    );
}

#[test]
fn review_budget_limited_reconciliation_checkpoints_partial_history_without_publication() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.skill("design", "old");
    acer.skill("design", "new");
    acer.sync();
    let (records, _) = read_records(&acer.paths).unwrap();
    let Body::Capability(body) = &records.last().unwrap().body else {
        panic!()
    };
    let allowance = body.files.iter().map(|f| f.size).sum();
    let before = record_files(&dir);
    let first = reconcile_with_budget(&mut desk.store, &desk.paths, desk.host, allowance).unwrap();
    assert!(!first.pending.is_empty());
    assert_eq!(desk.cap("design").revisions.len(), 1);
    let checkpoint = load_local(&desk.paths).unwrap();
    assert!(checkpoint.generation > 0);
    let key = records[0].object.clone();
    assert_eq!(
        checkpoint.objects[&key].content,
        format!("tree:{}", desk.tree("design"))
    );
    let second = reconcile_with_budget(&mut desk.store, &desk.paths, desk.host, allowance).unwrap();
    assert!(second.pending.is_empty());
    assert_eq!(desk.cap("design").revisions.len(), 2);
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert_eq!(
        record_files(&dir),
        before,
        "partial import must not publish an owner edit"
    );
    desk.sync();
    assert_eq!(record_files(&dir), before);
}

#[test]
fn review_missing_old_blob_does_not_starve_current_revision() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.skill("design", "old");
    acer.skill("design", "new");
    acer.sync();
    let (records, _) = read_records(&acer.paths).unwrap();
    let Body::Capability(old) = &records[0].body else {
        panic!()
    };
    let blob = acer.paths.blobs().join(&old.files[0].sha256);
    let held = dir.join("held-old-blob");
    fs::rename(&blob, &held).unwrap();
    let Body::Capability(current) = &records[1].body else {
        panic!()
    };
    let allowance = current.files.iter().map(|f| f.size).sum();
    let status = reconcile_with_budget(&mut desk.store, &desk.paths, desk.host, allowance).unwrap();
    assert!(!status.pending.is_empty());
    assert_eq!(desk.tree("design"), acer.tree("design"));
    fs::rename(held, blob).unwrap();
    desk.sync();
    assert_eq!(desk.cap("design").revisions.len(), 2);
    assert_eq!(desk.tree("design"), acer.tree("design"));
}

#[test]
fn review_invalid_history_does_not_poison_subsequent_good_revisions() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let mut invalid = skill("design", "old");
    invalid.files.push(("auth.json".into(), b"{}".to_vec()));
    acer.store.import(&invalid).unwrap();
    acer.skill("design", "new");
    acer.sync();
    let status = desk.sync();
    assert!(!status.rejected.is_empty());
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert_eq!(desk.cap("design").revisions.len(), 1);
    acer.skill("design", "newest");
    acer.sync();
    desk.sync();
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert_eq!(desk.cap("design").revisions.len(), 2);
    let local = load_local(&desk.paths).unwrap();
    assert!(
        local
            .refused
            .contains_key(&format!("{}:1", device_of(&acer)))
    );
}

#[test]
fn review_equal_selection_heads_do_not_resurrect_after_an_edit_or_deletion() {
    for delete in [false, true] {
        let dir = shared();
        let mut acer = machine(&dir, "saint-acer");
        let mut desk = machine(&dir, "Santiago-PC");
        let id = acer
            .store
            .save_selection(&SelectionInput {
                id: None,
                name: "Original".into(),
                provider: "codex".into(),
                items: vec![],
                composites: vec![],
            })
            .unwrap()
            .id;
        acer.sync();
        desk.sync();
        let key = format!("selection/{id}");
        let (records, _) = read_records(&acer.paths).unwrap();
        let original = records.iter().find(|r| r.object == key).unwrap();
        // Independently published equal-content heads, as produced by parallel merges.
        for m in [&mut acer, &mut desk] {
            let mut local = load_local(&m.paths).unwrap();
            let journal = Dir::open(&m.paths, &["v1", "devices", &local.device], false)
                .unwrap()
                .unwrap();
            publish(
                &m.paths,
                &journal,
                m.host,
                &mut local,
                &key,
                Some(original.id()),
                vec![],
                original.body.clone(),
            )
            .unwrap();
        }
        acer.sync();
        desk.sync();
        let (records, _) = read_records(&acer.paths).unwrap();
        let view = &materialize(&records)[&key];
        assert!(!view.conflict);
        assert_eq!(records[view.chosen].body, original.body);
        if delete {
            acer.store.delete_selection(&id).unwrap();
        } else {
            acer.store
                .save_selection(&SelectionInput {
                    id: Some(id.clone()),
                    name: "Further edit".into(),
                    provider: "codex".into(),
                    items: vec![],
                    composites: vec![],
                })
                .unwrap();
        }
        acer.sync();
        desk.sync();
        acer.sync();
        let selections = desk.store.selections().unwrap();
        if delete {
            assert!(!selections.iter().any(|s| s.id == id));
        } else {
            assert_eq!(
                selections.iter().find(|s| s.id == id).unwrap().name,
                "Further edit"
            );
        }
        let (records, _) = read_records(&acer.paths).unwrap();
        assert_eq!(materialize(&records)[&key].heads.len(), 1);
        let count = record_files(&dir);
        desk.sync();
        acer.sync();
        assert_eq!(record_files(&dir), count);
    }
}

#[test]
fn review_held_trees_do_not_require_blobs_to_be_read_again() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.skill("design", "old");
    acer.skill("design", "new");
    acer.sync();
    desk.sync();
    for entry in fs::read_dir(acer.paths.blobs()).unwrap() {
        let entry = entry.unwrap();
        fs::rename(entry.path(), dir.join(entry.file_name())).unwrap();
    }
    let before = record_files(&dir);
    let status = desk.sync();
    assert!(status.pending.is_empty());
    assert_eq!(desk.cap("design").revisions.len(), 2);
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert_eq!(record_files(&dir), before);
}

#[test]
fn review_a_late_duplicate_merge_does_not_resurrect_deleted_objects() {
    for selection in [false, true] {
        let dir = shared();
        let mut acer = machine(&dir, "saint-acer");
        let mut desk = machine(&dir, "Santiago-PC");
        let id = if selection {
            acer.store
                .save_selection(&SelectionInput {
                    id: None,
                    name: "Original".into(),
                    provider: "codex".into(),
                    items: vec![],
                    composites: vec![],
                })
                .unwrap()
                .id
        } else {
            preset(&mut acer, "Original", vec![])
        };
        acer.sync();
        desk.sync();
        let kind = if selection { "selection" } else { "preset" };
        let key = format!("{kind}/{id}");
        for m in [&mut acer, &mut desk] {
            let mut local = load_local(&m.paths).unwrap();
            let journal = Dir::open(&m.paths, &["v1", "devices", &local.device], false)
                .unwrap()
                .unwrap();
            let base = local.objects[&key].record.clone();
            let body = if selection {
                Body::Selection(SelectionBody {
                    name: m.host.into(),
                    provider: "codex".into(),
                    items: vec![],
                    composites: vec![],
                })
            } else {
                Body::Preset(PresetBody {
                    name: m.host.into(),
                    description: "offline edit".into(),
                    items: vec![],
                })
            };
            publish(
                &m.paths,
                &journal,
                m.host,
                &mut local,
                &key,
                base,
                vec![],
                body,
            )
            .unwrap();
        }
        let (snapshot, _) = read_records(&acer.paths).unwrap();
        let views = materialize(&snapshot);
        let desk_before = last_seq(
            &Dir::open(&desk.paths, &["v1", "devices", &device_of(&desk)], false)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        for m in [&mut acer, &mut desk] {
            let mut local = load_local(&m.paths).unwrap();
            let journal = Dir::open(&m.paths, &["v1", "devices", &local.device], false)
                .unwrap()
                .unwrap();
            resolve_conflicts(&m.paths, &journal, m.host, &mut local, &snapshot, &views).unwrap();
        }
        // Syncthing has not yet delivered the other machine's identical merge and copy.
        let journal = desk.paths.devices().join(device_of(&desk));
        let held = dir.join("held-late-merges");
        fs::create_dir_all(&held).unwrap();
        for entry in fs::read_dir(&journal).unwrap().flatten() {
            if parse_record_name(&entry.file_name().to_string_lossy())
                .is_some_and(|seq| seq > desk_before)
            {
                fs::rename(entry.path(), held.join(entry.file_name())).unwrap();
            }
        }
        acer.sync();
        let ids: Vec<_> = if selection {
            acer.store
                .selections()
                .unwrap()
                .into_iter()
                .map(|s| s.id)
                .collect()
        } else {
            acer.store
                .presets()
                .unwrap()
                .into_iter()
                .map(|p| p.id)
                .collect()
        };
        assert_eq!(ids.len(), 2);
        for id in &ids {
            if selection {
                acer.store.delete_selection(id).unwrap();
            } else {
                acer.store.delete_preset(id).unwrap();
            }
            acer.sync();
        }
        assert!(acer.store.presets().unwrap().is_empty());
        assert!(acer.store.selections().unwrap().is_empty());
        for entry in fs::read_dir(&held).unwrap().flatten() {
            fs::rename(entry.path(), journal.join(entry.file_name())).unwrap();
        }
        desk.sync();
        acer.sync();
        desk.sync();
        for m in [&acer, &desk] {
            assert!(
                m.store.presets().unwrap().is_empty(),
                "a delayed equivalent merge is not a new owner edit"
            );
            assert!(
                m.store.selections().unwrap().is_empty(),
                "a delayed equivalent selection is not a new owner edit"
            );
        }
        let count = record_files(&dir);
        acer.sync();
        desk.sync();
        assert_eq!(record_files(&dir), count);
    }
}

fn semantic_record(seq: u64, parents: &[u64], body: Body) -> Record {
    Record {
        schema: SCHEMA.into(),
        device: "semantic-fixture".into(),
        host: "fixture".into(),
        seq,
        at: seq as i64,
        object: "preset/semantic-fixture".into(),
        base: parents.first().map(|id| format!("semantic-fixture:{id}")),
        merged: parents
            .iter()
            .skip(1)
            .map(|id| format!("semantic-fixture:{id}"))
            .collect(),
        body,
    }
}

#[test]
fn review_semantic_duplicates_require_equal_parent_sets() {
    for body in [
        Body::Preset(PresetBody {
            name: "Same".into(),
            description: "".into(),
            items: vec![],
        }),
        Body::Selection(SelectionBody {
            name: "Same".into(),
            provider: "codex".into(),
            items: vec![],
            composites: vec![],
        }),
    ] {
        let mut records = vec![
            semantic_record(1, &[], body.clone()),
            semantic_record(2, &[1], body.clone()),
            semantic_record(3, &[2], Body::Deleted),
        ];
        // A second physical ID for version 2 has exactly the same semantic parents.
        // Even a much newer timestamp cannot defeat its causal successor's tombstone.
        records.push(semantic_record(4, &[1], body.clone()));
        let views = materialize(&records);
        let view = &views["preset/semantic-fixture"];
        assert_eq!(records[view.chosen].body, Body::Deleted);
        assert_eq!(view.heads.len(), 1);
        assert_eq!(view.all.len(), 4);
        // Equal content reached through a different ancestry is a real concurrent edit.
        let different = match &body {
            Body::Preset(preset) => Body::Preset(PresetBody {
                name: "Concurrent ancestor".into(),
                ..preset.clone()
            }),
            Body::Selection(selection) => Body::Selection(SelectionBody {
                name: "Concurrent ancestor".into(),
                ..selection.clone()
            }),
            _ => unreachable!(),
        };
        records.push(semantic_record(5, &[1], different));
        records.push(semantic_record(6, &[5], body.clone()));
        let views = materialize(&records);
        let view = &views["preset/semantic-fixture"];
        assert_eq!(records[view.chosen].id(), records[5].id());
        assert_eq!(records[view.chosen].body, body);
        assert!(view.conflict);
        assert_eq!(view.heads.len(), 2);
    }
}

#[test]
fn review_semantic_parent_sets_ignore_order_and_duplicate_ids() {
    let body = Body::Preset(PresetBody {
        name: "Same".into(),
        description: "".into(),
        items: vec![],
    });
    let records = vec![
        semantic_record(1, &[98], body.clone()),
        semantic_record(2, &[99], body.clone()),
        semantic_record(3, &[1, 2], body.clone()),
        semantic_record(4, &[2, 1, 2], body),
        semantic_record(5, &[3], Body::Deleted),
    ];
    let (versions, complete) = semantic_versions(&records, &[0, 1, 2, 3, 4]);
    assert!(!complete); // The fixture deliberately retains unknown parents 98 and 99.
    assert_eq!(versions[&records[2].id()], versions[&records[3].id()]);
    let views = materialize(&records);
    let view = &views["preset/semantic-fixture"];
    assert_eq!(records[view.chosen].body, Body::Deleted);
    assert_eq!(view.heads.len(), 1);
    let mut reversed = records.clone();
    reversed.reverse();
    let views = materialize(&reversed);
    assert_eq!(
        reversed[views["preset/semantic-fixture"].chosen].id(),
        records[4].id()
    );
}

#[test]
fn review_semantic_normalization_is_iterative_and_cycles_stay_distinct() {
    let body = Body::Preset(PresetBody {
        name: "Chain".into(),
        description: "".into(),
        items: vec![],
    });
    let mut records = Vec::new();
    // Two equivalent chains require transitive normalization at every level. This depth
    // must not recurse, expand ancestor sets, or repeatedly scan the entire graph.
    for level in 0..8192_u64 {
        let left = level * 2 + 1;
        let left_parent = if level == 0 { None } else { Some(left - 2) };
        let right_parent = if level == 0 { None } else { Some(left - 1) };
        records.push(semantic_record(left, left_parent.as_slice(), body.clone()));
        records.push(semantic_record(
            left + 1,
            right_parent.as_slice(),
            body.clone(),
        ));
    }
    records.push(semantic_record(16_385, &[16_383], Body::Deleted));
    records.reverse(); // Dependencies need not appear before children in the input.
    let views = materialize(&records);
    let view = &views["preset/semantic-fixture"];
    assert_eq!(records[view.chosen].body, Body::Deleted);
    assert_eq!(view.heads.len(), 1);
    assert_eq!(view.all.len(), records.len());

    let cyclic = vec![
        semantic_record(1, &[2], body.clone()),
        semantic_record(2, &[1], body.clone()),
        semantic_record(3, &[1], body), // Dependent on the cycle also stays raw.
        semantic_record(4, &[3], Body::Deleted),
    ];
    let (versions, complete) = semantic_versions(&cyclic, &[0, 1, 2, 3]);
    assert!(!complete);
    for record in &cyclic {
        assert_eq!(versions[&record.id()], format!("record:{}", record.id()));
    }
    let views = materialize(&cyclic[..2]);
    let view = &views["preset/semantic-fixture"];
    assert_eq!(view.heads.len(), 1);
    assert_eq!(cyclic[view.chosen].seq, 2); // Existing bounded cycle fallback.
    let views = materialize(&cyclic);
    assert_eq!(
        cyclic[views["preset/semantic-fixture"].chosen].body,
        Body::Deleted
    );
}

#[test]
fn review_tombstone_before_its_parent_merge_does_not_create_a_resurrection_merge() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    let id = preset(&mut acer, "Original", vec![]);
    acer.sync();
    desk.sync();
    let key = format!("preset/{id}");
    for m in [&mut acer, &mut desk] {
        let mut local = load_local(&m.paths).unwrap();
        let journal = Dir::open(&m.paths, &["v1", "devices", &local.device], false)
            .unwrap()
            .unwrap();
        let base = local.objects[&key].record.clone();
        publish(
            &m.paths,
            &journal,
            m.host,
            &mut local,
            &key,
            base,
            vec![],
            Body::Preset(PresetBody {
                name: m.host.into(),
                description: "offline edit".into(),
                items: vec![],
            }),
        )
        .unwrap();
    }
    let (snapshot, _) = read_records(&acer.paths).unwrap();
    let views = materialize(&snapshot);
    let a_journal = acer.paths.devices().join(device_of(&acer));
    let d_journal = desk.paths.devices().join(device_of(&desk));
    let a_start = last_seq(
        &Dir::open(&acer.paths, &["v1", "devices", &device_of(&acer)], false)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let d_start = last_seq(
        &Dir::open(&desk.paths, &["v1", "devices", &device_of(&desk)], false)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    for m in [&mut acer, &mut desk] {
        let mut local = load_local(&m.paths).unwrap();
        let journal = Dir::open(&m.paths, &["v1", "devices", &local.device], false)
            .unwrap()
            .unwrap();
        resolve_conflicts(&m.paths, &journal, m.host, &mut local, &snapshot, &views).unwrap();
    }
    let held_d = dir.join("held-d");
    fs::create_dir_all(&held_d).unwrap();
    for e in fs::read_dir(&d_journal).unwrap().flatten() {
        if parse_record_name(&e.file_name().to_string_lossy()).is_some_and(|s| s > d_start) {
            fs::rename(e.path(), held_d.join(e.file_name())).unwrap();
        }
    }
    acer.sync();
    let a_merged = last_seq(
        &Dir::open(&acer.paths, &["v1", "devices", &device_of(&acer)], false)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    for p in acer.store.presets().unwrap() {
        acer.store.delete_preset(&p.id).unwrap();
        acer.sync();
    }
    let held_a = dir.join("held-a");
    fs::create_dir_all(&held_a).unwrap();
    for e in fs::read_dir(&a_journal).unwrap().flatten() {
        if parse_record_name(&e.file_name().to_string_lossy())
            .is_some_and(|s| s > a_start && s <= a_merged)
        {
            fs::rename(e.path(), held_a.join(e.file_name())).unwrap();
        }
    }
    for e in fs::read_dir(&held_d).unwrap().flatten() {
        fs::rename(e.path(), d_journal.join(e.file_name())).unwrap();
    }
    let before = last_seq(
        &Dir::open(&desk.paths, &["v1", "devices", &device_of(&desk)], false)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    desk.sync();
    let after = last_seq(
        &Dir::open(&desk.paths, &["v1", "devices", &device_of(&desk)], false)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        before, after,
        "unknown causal parents must not produce an automatic conflict merge"
    );
    for e in fs::read_dir(&held_a).unwrap().flatten() {
        fs::rename(e.path(), a_journal.join(e.file_name())).unwrap();
    }
    desk.sync();
    acer.sync();
    desk.sync();
    assert!(acer.preset_names().is_empty());
    assert!(desk.preset_names().is_empty());
}

#[test]
fn review_incomplete_history_defers_automatic_application_but_keeps_owner_edits() {
    for selection in [false, true] {
        let dir = shared();
        let mut acer = machine(&dir, "saint-acer");
        let mut desk = machine(&dir, "Santiago-PC");
        let id = uuid::Uuid::new_v4().to_string();
        let key = format!("{}/{id}", if selection { "selection" } else { "preset" });
        let body = |name: &str| {
            if selection {
                Body::Selection(SelectionBody {
                    name: name.into(),
                    provider: "claude_code".into(),
                    items: vec![],
                    composites: vec![],
                })
            } else {
                Body::Preset(PresetBody {
                    name: name.into(),
                    description: "".into(),
                    items: vec![],
                })
            }
        };
        acer.sync();
        let mut local = load_local(&acer.paths).unwrap();
        let journal = Dir::open(&acer.paths, &["v1", "devices", &local.device], false)
            .unwrap()
            .unwrap();
        let first = publish(
            &acer.paths,
            &journal,
            acer.host,
            &mut local,
            &key,
            None,
            vec![],
            body("Original"),
        )
        .unwrap();
        desk.sync();
        let second = publish(
            &acer.paths,
            &journal,
            acer.host,
            &mut local,
            &key,
            Some(first),
            vec![],
            body("Remote edit"),
        )
        .unwrap();
        publish(
            &acer.paths,
            &journal,
            acer.host,
            &mut local,
            &key,
            Some(second),
            vec![],
            Body::Deleted,
        )
        .unwrap();
        let path = acer
            .paths
            .devices()
            .join(&local.device)
            .join(record_name(2));
        let held = dir.join("held-parent");
        fs::rename(&path, &held).unwrap();
        let before = record_files(&dir);
        for _ in 0..2 {
            let status = desk.sync();
            assert!(
                status
                    .pending
                    .iter()
                    .any(|n| n.detail.contains("journal history"))
            );
            assert!(status.conflicts.is_empty());
            assert!(status.removed.is_empty());
            assert_eq!(record_files(&dir), before);
        }
        if selection {
            assert_eq!(desk.store.selections().unwrap()[0].name, "Original");
            desk.store
                .put_selection(
                    &id,
                    &SelectionInput {
                        id: None,
                        name: "Owner edit".into(),
                        provider: "claude_code".into(),
                        items: vec![],
                        composites: vec![],
                    },
                )
                .unwrap();
        } else {
            assert_eq!(desk.preset_names(), vec!["Original"]);
            desk.store
                .put_preset(
                    &id,
                    &PresetInput {
                        id: None,
                        name: "Owner edit".into(),
                        description: "".into(),
                        items: vec![],
                    },
                )
                .unwrap();
        }
        let status = desk.sync();
        assert!(
            status
                .pending
                .iter()
                .any(|n| n.detail.contains("journal history"))
        );
        assert_eq!(
            record_files(&dir),
            before + 1,
            "publish only the explicit owner edit"
        );
        let owner = load_local(&desk.paths).unwrap();
        let (records, _) = read_records(&desk.paths).unwrap();
        let edit = records.iter().find(|r| r.device == owner.device).unwrap();
        assert_eq!(
            edit.base.as_deref(),
            Some(format!("{}:1", local.device).as_str())
        );
        let own_journal = Dir::open(&desk.paths, &["v1", "devices", &owner.device], false)
            .unwrap()
            .unwrap();
        assert!(read_applied(&own_journal).unwrap().seen.is_empty());
        fs::rename(held, path).unwrap();
        let status = desk.sync();
        assert!(status.pending.is_empty());
        let (records, _) = read_records(&desk.paths).unwrap();
        let views = materialize(&records);
        assert!(views[&key].complete);
        assert_eq!(body_name(&records[views[&key].chosen].body), "Owner edit");
    }
}

#[test]
fn review_cycles_never_apply_or_publish_automatic_resolutions() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.sync();
    let mut local = load_local(&acer.paths).unwrap();
    let journal = Dir::open(&acer.paths, &["v1", "devices", &local.device], false)
        .unwrap()
        .unwrap();
    let key = format!("preset/{}", uuid::Uuid::new_v4());
    let parent = format!("{}:2", local.device);
    let first = publish(
        &acer.paths,
        &journal,
        acer.host,
        &mut local,
        &key,
        Some(parent),
        vec![],
        Body::Deleted,
    )
    .unwrap();
    publish(
        &acer.paths,
        &journal,
        acer.host,
        &mut local,
        &key,
        Some(first),
        vec![],
        Body::Deleted,
    )
    .unwrap();
    publish(
        &acer.paths,
        &journal,
        acer.host,
        &mut local,
        &key,
        None,
        vec![],
        Body::Preset(PresetBody {
            name: "Concurrent".into(),
            description: "".into(),
            items: vec![],
        }),
    )
    .unwrap();
    publish(
        &acer.paths,
        &journal,
        acer.host,
        &mut local,
        &key,
        None,
        vec![],
        Body::Preset(PresetBody {
            name: "Other concurrent edit".into(),
            description: "".into(),
            items: vec![],
        }),
    )
    .unwrap();
    let before = record_files(&dir);
    for _ in 0..2 {
        let status = desk.sync();
        assert!(
            status
                .pending
                .iter()
                .any(|n| n.detail.contains("journal history"))
        );
        assert!(status.conflicts.is_empty());
        assert!(desk.preset_names().is_empty());
        assert_eq!(record_files(&dir), before);
        let local = load_local(&desk.paths).unwrap();
        let journal = Dir::open(&desk.paths, &["v1", "devices", &local.device], false)
            .unwrap()
            .unwrap();
        assert!(read_applied(&journal).unwrap().seen.is_empty());
    }
}

#[test]
fn review_parent_in_another_object_does_not_complete_history() {
    let mut records = vec![
        semantic_record(1, &[], Body::Deleted),
        semantic_record(2, &[1], Body::Deleted),
    ];
    records[0].object = "preset/another-object".into();
    let views = materialize(&records);
    assert!(views["preset/another-object"].complete);
    assert!(!views["preset/semantic-fixture"].complete);
}

#[test]
fn review_causal_pending_keeps_blob_wait_details() {
    let dir = shared();
    let mut acer = machine(&dir, "saint-acer");
    let mut desk = machine(&dir, "Santiago-PC");
    acer.skill("design", "old");
    acer.skill("design", "new");
    acer.sync();
    let (records, _) = read_records(&acer.paths).unwrap();
    let Body::Capability(current) = &records[1].body else {
        panic!()
    };
    let parent = acer
        .paths
        .devices()
        .join(device_of(&acer))
        .join(record_name(1));
    let held_parent = dir.join("held-parent");
    fs::rename(&parent, &held_parent).unwrap();
    let blob = acer.paths.blobs().join(&current.files[0].sha256);
    let held_blob = dir.join("held-blob");
    fs::rename(&blob, &held_blob).unwrap();
    let status = desk.sync();
    let pending = &status.pending[0];
    assert!(pending.detail.contains("journal history"));
    assert!(
        pending.detail.contains("; "),
        "retain the existing blob wait detail"
    );
    fs::rename(held_blob, blob).unwrap();
    let before = record_files(&dir);
    let status = desk.sync();
    assert_eq!(desk.tree("design"), acer.tree("design"));
    assert!(
        status
            .pending
            .iter()
            .any(|n| n.detail.contains("journal history"))
    );
    assert_eq!(record_files(&dir), before);
    fs::rename(held_parent, parent).unwrap();
    assert!(desk.sync().pending.is_empty());
    assert_eq!(desk.cap("design").revisions.len(), 2);
    assert_eq!(record_files(&dir), before + 1); // Only the transported parent returned.
}
