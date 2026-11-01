use super::*;
use crate::arsenal::store::{Store, identity};
use rusqlite::Connection;

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
