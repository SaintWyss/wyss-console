use super::*;
use crate::arsenal::store::{Store, identity};
use rusqlite::Connection;

#[test]
fn review_a_record_arriving_after_apply_is_not_acknowledged_until_the_next_run() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let (mut desk, db) = machine_on_disk(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    ACCEPT_ALL.with(|accept| accept.set(true));
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
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
    let mut desk = machine(&dir, "fixture-node-b");
    desk.sync();
    assert_eq!(desk.tree("design"), acer.tree("design"));
}

#[test]
fn final_review_sync_tree_digest_wire_format_matches_fixed_vector() {
    // Fixed SHA-256 of the sorted path, NUL, hex digest and LF records.
    let files = [
        ("notes/usage.md".into(), b"Examples\n".to_vec()),
        ("SKILL.md".into(), b"# Fixture\n".to_vec()),
    ];
    assert_eq!(
        store::tree_digest(&files),
        "2348adabf26ce14cf559145bb5b77afd4cc2fd8cf22f60bc5eb1f1d6bb44e370"
    );
}

#[test]
fn malformed_unsupported_and_hostile_records_are_refused_and_change_nothing() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
        let tree = store::tree_of_digests(&mut entries);
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
fn the_library_schema_is_version_4_with_only_the_received_tables_added() {
    let dir = shared();
    let path = dir.join("library.sqlite");
    {
        let mut m = Machine {
            store: Store::open(Connection::open(&path).unwrap()).unwrap(),
            paths: Paths {
                shared: dir.join("arsenal"),
                local: dir.join("local"),
            },
            host: "fixture-node-a",
        };
        m.skill("design", "one");
        m.sync();
    }
    let db = Connection::open(&path).unwrap();
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);
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
            "sync_decision",
            "sync_incoming",
            "terminal_binding"
        ]
    );
}

#[test]
fn one_run_at_a_time_on_one_computer() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    acer.sync();
    let lock = File::create(acer.paths.local.join("lock")).unwrap();
    let held = nix::fcntl::Flock::lock(lock, nix::fcntl::FlockArg::LockExclusiveNonblock).unwrap();
    assert_eq!(
        reconcile(&mut acer.store, &acer.paths, "fixture-node-a").unwrap_err(),
        "busy"
    );
    drop(held);
    acer.sync();
}

#[test]
fn a_peer_acknowledges_what_it_applied() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    acer.skill("design", "one");
    let published = acer.sync();
    assert_eq!(published.peers.len(), 0);
    desk.sync();
    let status = acer.sync();
    assert_eq!(status.peers.len(), 1);
    let peer = &status.peers[0];
    assert_eq!(peer.host, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
        note.detail.contains("deleted on fixture-node-a"),
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    assert!(matches!(
        apply_capability(&mut desk.store, &desk.paths, &records, view, allowance),
        Err(Applying::Wait(_))
    ));
    assert!(apply_capability(&mut desk.store, &desk.paths, &records, view, allowance).is_ok());
    assert_eq!(desk.cap("design").revisions.len(), 2);
    assert_eq!(desk.tree("design"), acer.tree("design"));
}

fn offline_conflict(selection: bool, description: &str) -> (PathBuf, Machine, Machine, String) {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    // Both offline edits use the same base.
    for m in [&acer, &desk] {
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
                description: description.into(),
                items: vec![],
            })
        };
        publish(&m.paths, &journal, m.host, &mut local, &key, base, vec![], body).unwrap();
    }
    (dir, acer, desk, id)
}

fn resolve_same_snapshot(peers: [&Machine; 2], records: &[Record], views: &BTreeMap<String, View>) {
    for m in peers {
        let mut local = load_local(&m.paths).unwrap();
        let journal = Dir::open(&m.paths, &["v1", "devices", &local.device], false)
            .unwrap()
            .unwrap();
        resolve_conflicts(
            &m.store, &m.paths, &journal, m.host, &mut local, records, views, &BTreeSet::new(),
        )
        .unwrap();
    }
}

#[test]
fn review_independent_conflict_merges_do_not_resurrect_a_later_deletion() {
    let (_, mut acer, mut desk, id) = offline_conflict(false, "edit");
    let key = format!("preset/{id}");
    let (records, _) = read_records(&acer.paths).unwrap();
    let views = materialize(&records);
    assert!(views[&key].conflict);
    resolve_same_snapshot([&acer, &desk], &records, &views);
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
    let mut acer = machine(&dir, "fixture-node-a");
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
    let mut acer = machine(&dir, "fixture-node-a");
    acer.sync();
    for n in 0..MAX_DEVICES {
        fs::create_dir_all(acer.paths.devices().join(format!("{n:032x}"))).unwrap();
    }
    assert!(read_records(&acer.paths).is_err());
}

#[test]
fn review_global_record_limit_counts_rejected_files_before_applying_anything() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    body.tree_hash = store::tree_of_digests(&mut entries);
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
        let mut acer = machine(&dir, "fixture-node-a");
        let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
        let (dir, mut acer, mut desk, _) = offline_conflict(selection, "offline edit");
        let (snapshot, _) = read_records(&acer.paths).unwrap();
        let views = materialize(&snapshot);
        let desk_before = last_seq(
            &Dir::open(&desk.paths, &["v1", "devices", &device_of(&desk)], false)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        resolve_same_snapshot([&acer, &desk], &snapshot, &views);
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
    let (dir, mut acer, mut desk, _) = offline_conflict(false, "offline edit");
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
    resolve_same_snapshot([&acer, &desk], &snapshot, &views);
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
        let mut acer = machine(&dir, "fixture-node-a");
        let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
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

// ---------------------------------------------------------------------------------------------
// Review fixes: poisoned blobs, folder ownership, read limits, bookkeeping file

#[test]
fn review_a_poisoned_blob_is_quarantined_and_the_good_one_is_applied_later() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    acer.skill("design", "one");
    acer.sync();
    let hash = store::digest(b"one");
    let blob = acer.paths.blobs().join(&hash);
    fs::write(&blob, b"xyz").unwrap();
    let status = desk.sync();
    assert!(desk.names().is_empty());
    assert!(status.rejected.is_empty(), "{:?}", status.rejected);
    assert!(
        !blob.exists(),
        "the bad copy is moved out of the shared folder"
    );
    let kept = fs::read_dir(desk.paths.local.join("quarantine"))
        .unwrap()
        .count();
    assert_eq!(kept, 1);
    // The synchronizer delivers the right copy.
    fs::write(&blob, b"one").unwrap();
    desk.sync();
    assert_eq!(desk.names(), vec!["design"]);
}

#[test]
fn review_a_shared_folder_others_can_write_to_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    acer.sync();
    let devices = acer.paths.devices();
    fs::set_permissions(&devices, fs::Permissions::from_mode(0o777)).unwrap();
    let error = reconcile(&mut acer.store, &acer.paths, acer.host).unwrap_err();
    assert!(
        error.contains("devices can be written by other users"),
        "{error}"
    );
    fs::set_permissions(&devices, fs::Permissions::from_mode(0o700)).unwrap();
    acer.sync();
}

#[test]
fn review_record_bytes_past_the_limit_fail_closed() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    preset(&mut acer, "Must not partially apply", vec![]);
    acer.sync();
    let folder = acer.paths.devices().join("e".repeat(32));
    fs::create_dir_all(&folder).unwrap();
    let filler = vec![b' '; 200 * 1024];
    // 5.2 MB of records exceeds the journal read budget.
    for seq in 1..=26 {
        fs::write(folder.join(record_name(seq)), &filler).unwrap();
    }
    let error = reconcile(&mut desk.store, &desk.paths, desk.host).unwrap_err();
    assert!(error.contains("more records"), "{error}");
    assert!(desk.preset_names().is_empty());
}

#[test]
fn review_unchanged_records_are_parsed_once_per_run() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    preset(&mut acer, "One", vec![]);
    acer.sync();
    let mut cache = Cache::new();
    let (first, _) = read_records_cached(&acer.paths, &mut cache).unwrap();
    assert_eq!(cache.len(), 1);
    let (again, _) = read_records_cached(&acer.paths, &mut cache).unwrap();
    assert_eq!(first, again);
    // A changed file is read again.
    let path = acer
        .paths
        .devices()
        .join(device_of(&acer))
        .join(record_name(1));
    fs::write(&path, b"{not json").unwrap();
    let (records, refused) = read_records_cached(&acer.paths, &mut cache).unwrap();
    assert!(records.is_empty());
    assert_eq!(refused.len(), 1);
}

#[test]
fn review_the_bookkeeping_file_is_never_written_through_a_link() {
    use std::os::unix::fs::symlink;
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    acer.sync();
    let outside = dir.with_extension("outside");
    fs::write(&outside, b"untouched").unwrap();
    symlink(&outside, acer.paths.local.join(".state.json.tmp")).unwrap();
    preset(&mut acer, "One", vec![]);
    acer.sync();
    assert_eq!(fs::read(&outside).unwrap(), b"untouched");
    // A linked state file is not read as this computer's bookkeeping, and is not replaced by a
    // new device either.
    let device = device_of(&acer);
    let state = acer.paths.local.join("state.json");
    let real = fs::read(&state).unwrap();
    fs::remove_file(&state).unwrap();
    let elsewhere = dir.with_extension("state-elsewhere");
    fs::write(&elsewhere, &real).unwrap();
    symlink(&elsewhere, &state).unwrap();
    assert!(load_local_existing(&acer.paths).is_none());
    let error = reconcile(&mut acer.store, &acer.paths, acer.host).unwrap_err();
    assert!(error.contains("state.json"), "{error}");
    assert_eq!(fs::read(&elsewhere).unwrap(), real);
    fs::remove_file(&state).unwrap();
    fs::write(&state, &real).unwrap();
    acer.sync();
    assert_eq!(device_of(&acer), device);
}

#[test]
fn confirm_a_decision_on_a_replaced_version_decides_nothing() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    acer.skill("design", "one");
    acer.sync();
    desk.sync();
    let seen = held(&desk)[0].clone();
    acer.skill("design", "two");
    acer.sync();
    desk.sync();
    assert_ne!(held(&desk)[0].record, seen.record);
    assert!(decide(&mut desk.store, &desk.paths, desk.host, &seen, true).is_err());
    assert!(desk.names().is_empty());
    assert_eq!(held(&desk).len(), 1);
}

#[test]
fn confirm_a_script_in_an_older_received_revision_is_reported() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    let mut scripted = skill("design", "one");
    scripted
        .files
        .push(("hooks/run.py".into(), b"print(1)\n".to_vec()));
    acer.store.import(&scripted).unwrap();
    acer.skill("design", "two");
    acer.sync();
    desk.sync();
    let one = &held(&desk)[0];
    assert_eq!(one.records.len(), 2);
    assert!(one.files.iter().all(|f| !f.script));
    assert!(one.scripts, "the older revision it brings has a script");
}

// ---------------------------------------------------------------------------------------------
// Received changes wait for the owner

thread_local! {
    /// The tests above predate confirmation and accept whatever arrives; `confirming` turns
    /// that off for the tests below, which check the behavior every user gets.
    pub(super) static ACCEPT_ALL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn confirming() {
    ACCEPT_ALL.with(|accept| accept.set(false));
}

fn decide_on(m: &mut Machine, object: &str, accept: bool) -> Status {
    let offer = held(m).into_iter().find(|h| h.object == object).unwrap();
    decide(&mut m.store, &m.paths, m.host, &offer, accept)
        .unwrap()
        .unwrap()
}

fn held(m: &Machine) -> Vec<Incoming> {
    incoming(&m.store).unwrap()
}

#[test]
fn confirm_a_capability_from_another_computer_waits_and_is_not_usable() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    let mut scripted = skill("design", "one");
    scripted
        .files
        .push(("scripts/run.sh".into(), b"echo fixture\n".to_vec()));
    acer.store.import(&scripted).unwrap();
    acer.sync();
    desk.sync();
    assert!(desk.names().is_empty(), "nothing received is usable yet");
    assert!(desk.store.sync_capabilities().unwrap().is_empty());
    let waiting = held(&desk);
    assert_eq!(waiting.len(), 1);
    let one = &waiting[0];
    assert_eq!(
        (one.kind.as_str(), one.name.as_str(), one.change.as_str()),
        ("capability", "design", "new")
    );
    assert_eq!(one.host, "fixture-node-a");
    assert_eq!(one.device, device_of(&acer));
    assert!(one.scripts);
    let scripts: Vec<_> = one
        .files
        .iter()
        .filter(|f| f.script)
        .map(|f| f.path.as_str())
        .collect();
    assert_eq!(scripts, vec!["scripts/run.sh"]);
    assert_eq!(one.files.len(), 3);
    // Held, so not acknowledged to the sender either.
    assert_eq!(acked(&desk, &acer), 0);
    // Running again offers the same thing once.
    desk.sync();
    assert_eq!(held(&desk), waiting);

    decide_on(&mut desk, &one.object, true);
    assert_eq!(desk.names(), vec!["design"]);
    assert!(held(&desk).is_empty());
    assert_eq!(acked(&desk, &acer), 1);
}

#[test]
fn confirm_a_refused_change_is_not_offered_again_and_is_acknowledged() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    acer.skill("design", "one");
    acer.sync();
    desk.sync();
    let object = held(&desk)[0].object.clone();
    decide_on(&mut desk, &object, false);
    desk.sync();
    desk.sync();
    assert!(
        held(&desk).is_empty(),
        "a refused version is not offered again"
    );
    assert!(desk.names().is_empty());
    assert_eq!(acked(&desk, &acer), 1, "refusing is a decision, not a wait");
    // A newer version is a new question.
    acer.skill("design", "two");
    acer.sync();
    desk.sync();
    let again = held(&desk);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].change, "new");
    assert!(desk.names().is_empty());
}

#[test]
fn confirm_a_change_made_here_is_never_held() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    desk.skill("local", "mine");
    preset(&mut desk, "Mine", vec![]);
    desk.sync();
    desk.skill("local", "edited");
    desk.sync();
    assert!(held(&desk).is_empty());
    assert_eq!(desk.names(), vec!["local"]);
    assert_eq!(desk.preset_names(), vec!["Mine"]);
    acer.sync();
    assert_eq!(held(&acer).len(), 2, "the other computer is asked");
}

#[test]
fn confirm_a_received_revision_keeps_the_current_one_until_accepted() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    acer.skill("design", "one");
    acer.sync();
    desk.sync();
    let object = held(&desk)[0].object.clone();
    decide_on(&mut desk, &object, true);
    let before = desk.tree("design");
    acer.skill("design", "two");
    acer.sync();
    desk.sync();
    assert_eq!(desk.tree("design"), before);
    assert_eq!(desk.cap("design").revisions.len(), 1);
    assert_eq!(held(&desk)[0].change, "revision");
    decide_on(&mut desk, &object, true);
    assert_eq!(desk.tree("design"), acer.tree("design"));
}

#[test]
fn confirm_a_preset_deleted_on_another_computer_stays_until_accepted() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    let id = preset(&mut acer, "Shared", vec![]);
    acer.sync();
    desk.sync();
    assert!(desk.preset_names().is_empty());
    let object = format!("preset/{id}");
    assert_eq!(held(&desk)[0].change, "new");
    decide_on(&mut desk, &object, true);
    assert_eq!(desk.preset_names(), vec!["Shared"]);
    acer.store.delete_preset(&id).unwrap();
    acer.sync();
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["Shared"]);
    let waiting = held(&desk);
    assert_eq!(
        (waiting[0].change.as_str(), waiting[0].name.as_str()),
        ("deleted", "Shared")
    );
    decide_on(&mut desk, &object, true);
    assert!(desk.preset_names().is_empty());
}

#[test]
fn confirm_a_conflict_does_not_republish_what_the_owner_has_not_accepted() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    acer.skill("design", "one");
    acer.sync();
    desk.sync();
    let object = held(&desk)[0].object.clone();
    decide_on(&mut desk, &object, true);
    acer.sync();
    acer.skill("design", "two");
    desk.skill("design", "three");
    acer.sync();
    let theirs = acer.tree("design");
    let mine = desk.tree("design");
    for _ in 0..3 {
        desk.sync();
    }
    assert_eq!(desk.tree("design"), mine);
    let cap = desk.cap("design");
    assert_eq!(desk.store.tree_revision(&cap.id, &theirs).unwrap(), None);
    assert_eq!(held(&desk)[0].change, "revision");
    // Nothing this computer published carries the other computer's version.
    let (records, _) = read_records(&desk.paths).unwrap();
    assert!(
        records.iter().all(
            |r| r.device != device_of(&desk) || content_of(&r.body) != format!("tree:{theirs}")
        )
    );
}

// ---------------------------------------------------------------------------------------------
// Round 2: a record's origin is this computer's diary, not the name in the shared folder

/// Writes `record` into `device`'s journal as another computer with write access to the shared
/// folder could.
fn plant(m: &Machine, record: &Record) {
    let dir = m.paths.devices().join(&record.device);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(record_name(record.seq)),
        serde_json::to_vec_pretty(record).unwrap(),
    )
    .unwrap();
}

fn planted_preset(device: &str, seq: u64, name: &str) -> Record {
    Record {
        schema: SCHEMA.into(),
        device: device.into(),
        host: "fixture-node-b".into(),
        seq,
        at: now(),
        object: format!("preset/{}", uuid::Uuid::new_v4()),
        base: None,
        merged: vec![],
        body: Body::Preset(PresetBody {
            name: name.into(),
            description: "d".into(),
            items: vec![],
        }),
    }
}

#[test]
fn origin_a_record_planted_under_this_computers_id_waits_for_the_owner() {
    let dir = shared();
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    desk.sync();
    let me = device_of(&desk);
    plant(&desk, &planted_preset(&me, 1, "Planted"));
    desk.sync();
    assert!(
        desk.preset_names().is_empty(),
        "a planted record is not this computer's"
    );
    let waiting = held(&desk);
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        (waiting[0].device.as_str(), waiting[0].name.as_str()),
        (me.as_str(), "Planted")
    );
    let status = desk.sync();
    assert_eq!(
        status.published, 0,
        "a planted record is not counted as published here"
    );
}

#[test]
fn origin_this_computers_own_records_still_need_no_decision() {
    let dir = shared();
    let mut desk = machine(&dir, "fixture-node-b");
    let mut acer = machine(&dir, "fixture-node-a");
    confirming();
    preset(&mut desk, "Mine", vec![]);
    desk.skill("design", "one");
    desk.sync();
    desk.sync();
    assert!(held(&desk).is_empty());
    assert_eq!(desk.preset_names(), vec!["Mine"]);
    assert_eq!(desk.sync().published, own_records(&desk) as u64);
    // Another computer still sees them as received.
    acer.sync();
    assert_eq!(held(&acer).len(), 2);
}

#[test]
fn origin_an_own_record_altered_in_the_shared_folder_is_not_applied() {
    let dir = shared();
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    preset(&mut desk, "Mine", vec![]);
    desk.sync();
    let me = device_of(&desk);
    // The same place, another body: a new object under this computer's first record.
    plant(&desk, &planted_preset(&me, 1, "Altered"));
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["Mine"]);
    let waiting = held(&desk);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].name, "Altered");
}

#[test]
fn origin_bookkeeping_without_a_diary_trusts_only_what_it_had_published() {
    let dir = shared();
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    preset(&mut desk, "Mine", vec![]);
    desk.sync();
    let mut local = load_local(&desk.paths).unwrap();
    local.published = None;
    save_local(&desk.paths, &local).unwrap();
    let me = device_of(&desk);
    plant(&desk, &planted_preset(&me, local.next_seq, "Planted"));
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["Mine"]);
    assert_eq!(held(&desk).len(), 1);
    assert_eq!(held(&desk)[0].name, "Planted");
}

#[test]
fn decision_a_refused_version_whose_body_changes_is_offered_again() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    let id = preset(&mut acer, "One", vec![]);
    acer.sync();
    desk.sync();
    let object = format!("preset/{id}");
    decide_on(&mut desk, &object, false);
    assert!(held(&desk).is_empty());
    // Same device:seq, another body.
    let (records, _) = read_records(&desk.paths).unwrap();
    let mut changed = records.into_iter().find(|r| r.object == object).unwrap();
    changed.body = Body::Preset(PresetBody {
        name: "Two".into(),
        description: "d".into(),
        items: vec![],
    });
    plant(&desk, &changed);
    desk.sync();
    let waiting = held(&desk);
    assert_eq!(
        waiting.len(),
        1,
        "a decision covers the body that was shown"
    );
    assert_eq!(waiting[0].name, "Two");
    assert!(desk.preset_names().is_empty());
}

#[test]
fn conflict_a_losing_remote_version_is_offered_when_the_local_one_wins() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    let id = preset(&mut acer, "Shared", vec![]);
    acer.sync();
    desk.sync();
    let object = format!("preset/{id}");
    decide_on(&mut desk, &object, true);
    acer.sync();
    rename_preset(&mut acer, &id, "From acer");
    rename_preset(&mut desk, &id, "From desk");
    acer.sync();
    desk.sync();
    acer.sync();
    let (records, _) = read_records(&acer.paths).unwrap();
    let views = materialize(&records);
    let view = &views[&object];
    assert!(view.conflict);
    let winner = records[view.chosen].device.clone();
    let w = if winner == device_of(&acer) {
        &mut acer
    } else {
        &mut desk
    };
    w.sync();
    w.sync();
    // The winner shows its own version; the other computer's waits for the owner, visibly.
    let waiting = held(w);
    assert_eq!(
        waiting.len(),
        1,
        "the losing version is offered, not waited on silently"
    );
    assert_eq!(waiting[0].object, object);
    decide_on(w, &object, true);
    assert!(held(w).is_empty());
    let (records, _) = read_records(&w.paths).unwrap();
    assert!(
        !materialize(&records)[&object].conflict,
        "the conflict ended"
    );
    assert_eq!(
        w.preset_names().len(),
        2,
        "the losing version is kept as a copy"
    );
}

#[test]
fn quarantine_a_blob_of_the_wrong_size_is_set_aside() {
    let dir = shared();
    let mut acer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    acer.skill("design", "one");
    acer.sync();
    let hash = store::digest(b"one");
    let blob = acer.paths.blobs().join(&hash);
    fs::write(&blob, b"one plus more").unwrap();
    desk.sync();
    assert!(desk.names().is_empty());
    assert!(!blob.exists(), "the wrong-size blob left the shared folder");
    assert!(desk.paths.local.join("quarantine").join(&hash).exists());
    // The right copy arrives and is applied.
    fs::write(&blob, b"one").unwrap();
    desk.sync();
    assert_eq!(desk.names(), vec!["design"]);
}

#[test]
fn quarantine_is_bounded() {
    let dir = shared();
    let acer = machine(&dir, "fixture-node-a");
    let blobs = Dir::open(&acer.paths, &["v1", "blobs"], true)
        .unwrap()
        .unwrap();
    let mut names = Vec::new();
    for i in 0..(MAX_QUARANTINE + 5) {
        let hash = store::digest(format!("real {i}").as_bytes());
        fs::write(acer.paths.blobs().join(&hash), b"planted").unwrap();
        quarantine(&acer.paths, &blobs, &hash);
        assert!(!acer.paths.blobs().join(&hash).exists());
        names.push(hash);
    }
    let kept = fs::read_dir(acer.paths.local.join("quarantine"))
        .unwrap()
        .count();
    assert_eq!(kept, MAX_QUARANTINE);
}

#[test]
fn origin_bookkeeping_without_a_diary_does_not_trust_an_older_sequence_number() {
    let dir = shared();
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    let id = preset(&mut desk, "A", vec![]);
    desk.sync();
    rename_preset(&mut desk, &id, "B");
    desk.sync();
    let mut local = load_local(&desk.paths).unwrap();
    assert!(local.next_seq > 2);
    local.published = None;
    save_local(&desk.paths, &local).unwrap();
    // Record 1 is below the next sequence number but no longer named by the bookkeeping.
    let me = device_of(&desk);
    plant(&desk, &planted_preset(&me, 1, "Planted"));
    desk.sync();
    assert_eq!(desk.preset_names(), vec!["B"]);
    let waiting = held(&desk);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].name, "Planted");
}

#[test]
fn diary_migration_requires_the_local_tree() {
    let dir = shared();
    let mut own = machine(&dir, "fixture-node-a");
    confirming();
    own.skill("design", "original");
    own.skill("verified", "unchanged");
    own.sync();

    let (records, _) = read_records(&own.paths).unwrap();
    let me = device_of(&own);
    let local_design = records.iter().find(|r| r.device == me && object_name(r) == "design").unwrap();
    let mut replaced = local_design.clone();
    let Body::Capability(body) = &mut replaced.body else { panic!("expected a capability") };
    let fixture = skill("design", "substitute");
    body.tree_hash = store::tree_digest(&fixture.files);
    let blobs = Dir::open(&own.paths, &["v1", "blobs"], false).unwrap().unwrap();
    body.files = fixture.files.iter().map(|(path, bytes)| {
        let sha256 = store::digest(bytes);
        write_blob(&own.paths, &blobs, &sha256, bytes).unwrap();
        FileRef { path: path.clone(), sha256, size: bytes.len() as u64 }
    }).collect();
    let unchanged = records.iter().find(|r| r.device == me && object_name(r) == "verified").unwrap();
    plant(&own, &replaced);
    let mut local = load_local(&own.paths).unwrap();
    local.published = None;
    save_local(&own.paths, &local).unwrap();
    own.sync();

    let local = load_local(&own.paths).unwrap();
    assert!(local.is_mine(unchanged));
    assert!(!local.is_mine(&replaced), "the changed record must not enter the diary");
    assert!(held(&own).iter().any(|card| card.device == me && card.records.contains(&replaced.decision_key())));
    assert_eq!(own.tree("design"), local_tree(local_design));
}

fn local_tree(record: &Record) -> String {
    let Body::Capability(body) = &record.body else { panic!("expected a capability") };
    body.tree_hash.clone()
}

#[test]
fn wrong_declared_size_leaves_a_valid_blob_in_place() {
    let dir = shared();
    let mut peer = machine(&dir, "fixture-node-a");
    peer.skill("design", "one");
    peer.sync();
    let (records, _) = read_records(&peer.paths).unwrap();
    let Body::Capability(mut body) = records[0].body.clone() else { panic!("expected a capability") };
    let file = body.files.iter_mut().find(|file| file.sha256 == store::digest(b"one")).unwrap();
    file.size += 1;
    let hash = file.sha256.clone();
    let blobs = Dir::open(&peer.paths, &["v1", "blobs"], false).unwrap().unwrap();

    let mut blob_reads = BlobReads::new(MAX_BLOB_BYTES_PER_RUN);
    assert!(matches!(
        capability_ready(&peer.paths, &blobs, &body, &mut blob_reads),
        Err(Applying::Wait(_))
    ));
    assert!(
        read_blob(&peer.paths, &blobs, &hash, 4, &mut blob_reads)
            .unwrap()
            .is_none()
    );
    assert_eq!(blob_reads.remaining, MAX_BLOB_BYTES_PER_RUN - 3);
    assert_eq!(
        read_blob(&peer.paths, &blobs, &hash, 3, &mut blob_reads)
            .unwrap()
            .as_deref(),
        Some(b"one".as_slice())
    );
    assert!(!peer.paths.local.join("quarantine").join(&hash).exists());
}

#[test]
fn losing_preset_cards_are_decided_individually() {
    let dir = shared();
    let mut own = machine(&dir, "fixture-node-a");
    confirming();
    let id = preset(&mut own, "Original", vec![]);
    own.sync();
    rename_preset(&mut own, &id, "Local winner");
    own.sync();
    let object = format!("preset/{id}");
    let local = load_local(&own.paths).unwrap();
    let (mut records, _) = read_records(&own.paths).unwrap();
    let mut winner = records.pop().unwrap();
    // Keep the winner's digest authentic while assigning the other heads older times.
    assert_eq!(winner.id(), local.objects[&object].record.clone().unwrap());
    let mut loser_a = planted_preset(&"a".repeat(32), 1, "Loser A");
    loser_a.object = object.clone();
    loser_a.host = "node A".into();
    loser_a.base = winner.base.take();
    loser_a.at = winner.at - 2;
    let mut loser_b = planted_preset(&"b".repeat(32), 1, "Loser B");
    loser_b.object = object.clone();
    loser_b.host = "node B".into();
    loser_b.base = loser_a.base.clone();
    loser_b.at = winner.at - 1;
    plant(&own, &loser_a);
    plant(&own, &loser_b);
    own.sync();

    let mut cards = held(&own);
    cards.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(cards.iter().map(|card| card.name.as_str()).collect::<Vec<_>>(), ["Loser A", "Loser B"]);
    assert_eq!(cards[0].records, [loser_a.decision_key()]);
    assert_eq!(cards[1].records, [loser_b.decision_key()]);
    decide(&mut own.store, &own.paths, own.host, &cards[0], true)
        .unwrap()
        .unwrap();
    assert_eq!(
        held(&own)
            .iter()
            .map(|card| card.record.clone())
            .collect::<Vec<_>>(),
        [loser_b.decision_key()]
    );
    assert_eq!(own.store.decision(&loser_b.decision_key()).unwrap(), None);
    assert!(own.preset_names().iter().any(|name| name.starts_with("Loser A")));
    assert!(!own.preset_names().iter().any(|name| name.starts_with("Loser B")));
}

#[test]
fn a_conflict_card_cannot_decide_the_later_grouped_history() {
    let dir = shared();
    let mut safe_peer = machine(&dir, "fixture-node-a");
    let mut scripted_peer = machine(&dir, "fixture-node-b");
    let mut desk = machine(&dir, "fixture-node-c");
    confirming();
    safe_peer.skill("design", "safe");
    safe_peer.sync();
    let (records, _) = read_records(&desk.paths).unwrap();
    let mut safe = records[0].clone();

    let mut scripted = skill("design", "scripted");
    scripted
        .files
        .push(("hooks/run.py".into(), b"print(1)\n".to_vec()));
    scripted_peer.store.import(&scripted).unwrap();
    scripted_peer.sync();
    let (records, _) = read_records(&desk.paths).unwrap();
    let mut script = records
        .iter()
        .find(|r| r.device == device_of(&scripted_peer))
        .unwrap()
        .clone();
    script.at = 1;
    safe.at = 3;
    plant(&desk, &script);
    plant(&desk, &safe);
    desk.sync();
    let displayed = held(&desk)
        .into_iter()
        .find(|card| card.record == safe.decision_key())
        .unwrap();
    assert_eq!(displayed.records, [safe.decision_key()]);
    assert!(!displayed.scripts);

    let successor = Record {
        device: script.device.clone(),
        seq: script.seq + 1,
        at: 2,
        base: Some(script.id()),
        ..safe.clone()
    };
    plant(&desk, &successor);
    desk.sync();
    let mut cards = held(&desk);
    assert_eq!(cards.len(), 1);
    let refreshed = cards.pop().unwrap();
    assert_eq!(refreshed.record, displayed.record);
    assert_eq!(refreshed.records.len(), 3);
    assert!(refreshed.records.contains(&script.decision_key()));
    assert!(refreshed.scripts);

    for accept in [true, false] {
        assert!(
            decide(&mut desk.store, &desk.paths, desk.host, &displayed, accept).is_err(),
            "the stale card must decide nothing (accept={accept})"
        );
        for key in &refreshed.records {
            assert_eq!(desk.store.decision(key).unwrap(), None);
        }
        assert_eq!(held(&desk).as_slice(), std::slice::from_ref(&refreshed));
        assert!(desk.names().is_empty());
    }
    let mut refreshed = refreshed;
    refreshed.records.reverse();
    decide(&mut desk.store, &desk.paths, desk.host, &refreshed, true)
        .unwrap()
        .unwrap();
    assert!(held(&desk).is_empty());
    assert_eq!(
        desk.store.decision(&script.decision_key()).unwrap(),
        Some(true)
    );
}

thread_local! {
    pub(super) static SIZE_MISMATCH_HASHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[test]
fn false_sizes_hash_a_shared_blob_once_per_run_and_use_the_read_budget() {
    let dir = shared();
    let mut peer = machine(&dir, "fixture-node-a");
    let mut desk = machine(&dir, "fixture-node-b");
    confirming();
    let bytes = "a".repeat(MAX_FILE_BYTES / 2);
    let mut original = skill("design", &bytes);
    original.files.truncate(1);
    peer.store.import(&original).unwrap();
    peer.sync();
    let (records, _) = read_records(&desk.paths).unwrap();
    let original = &records[0];

    for seq in 1..=4 {
        let mut revision = Record {
            seq,
            ..original.clone()
        };
        if seq > 1 {
            revision.base = Some(format!("{}:{}", revision.device, seq - 1));
        }
        let Body::Capability(body) = &mut revision.body else {
            panic!("expected a capability")
        };
        body.files[0].size = seq;
        plant(&desk, &revision);
    }
    desk.sync();
    let offer = held(&desk).pop().unwrap();
    assert_eq!(offer.records.len(), 4);
    desk.store.decide(&offer, true).unwrap();

    // First there is too little allowance, then enough for the one verified blob.
    let size = bytes.len() as u64;
    for (budget, expected_hashes) in [(size - 1, 0), (size, 1), (size, 1)] {
        SIZE_MISMATCH_HASHES.set(0);
        let status =
            reconcile_with_budget(&mut desk.store, &desk.paths, desk.host, budget).unwrap();
        assert_eq!(
            SIZE_MISMATCH_HASHES.get(),
            expected_hashes,
            "budget {budget}"
        );
        assert!(!status.pending.is_empty());
        assert!(desk.names().is_empty());
        assert!(
            desk.paths
                .blobs()
                .join(store::digest(bytes.as_bytes()))
                .exists()
        );
    }
}
