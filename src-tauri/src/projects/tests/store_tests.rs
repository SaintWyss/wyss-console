//! M1-04 and the store's recovery rules: a locked compare-and-swap file that is always the
//! complete old store or the complete new one, from threads, from separate processes and across
//! a process killed at every step of a write.
use super::*;
use std::{
    io,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    thread,
};
use wyss_console::contracts::m1::{M1ErrorCode, records::StoreV1};

/// A valid change that is visible in the revision and in the bytes.
fn relabel(store: &mut StoreV1, _tag: &str) -> Result<(), ()> {
    store.workspaces[0].pinned = !store.workspaces[0].pinned;
    Ok(())
}

fn ready(world: &World) -> StoreV1 {
    world.now()
}

#[test]
fn the_store_is_private_and_a_second_creation_is_refused() {
    let world = World::new();
    let dir = world.store.dir();
    assert_eq!(dir.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    let file = dir.join("store.json");
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    let before = std::fs::read(&file).unwrap();
    let again = empty_store(&*world.env).unwrap();
    assert!(matches!(
        world.store.create(again),
        Err(StoreError::StoreState("exists"))
    ));
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "an existing store is never replaced"
    );
}

#[test]
fn a_loser_of_a_race_gets_a_revision_conflict_and_the_winner_is_kept() {
    let world = World::new();
    let first = ready(&world).revision;
    let committed = world
        .store
        .transact(first, |s| relabel(s, "a"))
        .unwrap()
        .unwrap();
    assert_eq!(committed.revision.get(), first.get() + 1);
    match world.store.transact(first, |s| relabel(s, "b")) {
        Err(StoreError::RevisionConflict { current }) => {
            assert_eq!(current, committed.revision.get())
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_eq!(ready(&world), committed);
}

#[test]
fn a_change_that_fails_writes_nothing_and_a_refused_store_is_not_published() {
    let world = World::new();
    let before = std::fs::read(world.store.dir().join("store.json")).unwrap();
    let revision = ready(&world).revision;
    let refused = world
        .store
        .transact(revision, |_| Err::<(), &str>("no"))
        .unwrap();
    assert_eq!(refused.unwrap_err(), "no");
    // A change that breaks the contract (a second Scratch) is refused whole.
    let broken = world.store.transact(revision, |s| {
        let mut second = s.workspaces[0].clone();
        second.id = wyss_console::contracts::m1::scalar::WorkspaceId::parse(
            "00000000-0000-4000-8000-0000000000aa",
        )
        .unwrap();
        s.workspaces.push(second);
        Ok::<(), ()>(())
    });
    assert!(matches!(broken, Err(StoreError::Invalid(_))));
    assert_eq!(
        std::fs::read(world.store.dir().join("store.json")).unwrap(),
        before
    );
}

#[test]
fn eight_threads_at_one_revision_commit_exactly_once() {
    let world = Arc::new(World::new());
    let revision = ready(&world).revision;
    let handles: Vec<_> = (0..8)
        .map(|n| {
            let world = world.clone();
            thread::spawn(move || {
                world
                    .store
                    .transact(revision, |s| relabel(s, &"x".repeat(n + 1)))
                    .map(|r| r.is_ok())
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let won = results.iter().filter(|r| matches!(r, Ok(true))).count();
    let lost = results
        .iter()
        .filter(|r| matches!(r, Err(StoreError::RevisionConflict { .. })))
        .count();
    assert_eq!((won, lost), (1, 7), "{results:?}");
    assert_eq!(ready(&world).revision.get(), revision.get() + 1);
}

// ---- separate processes -------------------------------------------------------------------

/// Run by the parent tests below as a disposable child process. Run on its own it does nothing.
#[test]
fn child_store_worker() {
    let Ok(dir) = std::env::var("WYSS_M1_CHILD_DIR") else {
        return;
    };
    let mut store = Store::open(PathBuf::from(dir)).unwrap();
    if let Ok(point) = std::env::var("WYSS_M1_CRASH_AT") {
        let crash: Faults = Arc::new(move |at: Point| {
            if format!("{at:?}") == point {
                // The process dies here, mid-write, as a killed one would.
                std::process::abort();
            }
            Ok::<(), io::Error>(())
        });
        store = store.with_faults(crash);
    }
    let revision = match store.read().unwrap() {
        Loaded::Ready(s) => s.revision,
        other => panic!("{other:?}"),
    };
    let expected = std::env::var("WYSS_M1_EXPECT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(revision, |v| Counter::new(v).unwrap());
    let tag = std::env::var("WYSS_M1_TAG").unwrap_or_default();
    match store.transact(expected, |s| relabel(s, &tag)) {
        Ok(Ok(_)) => println!("RESULT WON"),
        Err(StoreError::RevisionConflict { .. }) => println!("RESULT LOST"),
        other => println!("RESULT OTHER {other:?}"),
    }
}

fn child(dir: &PathBuf, envs: &[(&str, &str)]) -> std::process::Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "projects::tests::store_tests::child_store_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("WYSS_M1_CHILD_DIR", dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in envs {
        command.env(key, value);
    }
    command.spawn().expect("a disposable child process")
}

fn verdict(child: std::process::Child) -> (bool, String) {
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
    )
}

#[test]
fn six_processes_at_one_revision_commit_exactly_once() {
    let world = World::new();
    let dir = world.store.dir().to_path_buf();
    let revision = ready(&world).revision.get().to_string();
    let children: Vec<_> = (0..6)
        .map(|n| {
            child(
                &dir,
                &[
                    ("WYSS_M1_EXPECT", &revision),
                    ("WYSS_M1_TAG", &"y".repeat(n + 1)),
                ],
            )
        })
        .collect();
    let outputs: Vec<_> = children.into_iter().map(verdict).collect();
    let won = outputs
        .iter()
        .filter(|(_, o)| o.contains("RESULT WON"))
        .count();
    let lost = outputs
        .iter()
        .filter(|(_, o)| o.contains("RESULT LOST"))
        .count();
    assert_eq!((won, lost), (1, 5), "{outputs:?}");
    assert_eq!(
        ready(&world).revision.get(),
        revision.parse::<u64>().unwrap() + 1
    );
}

#[test]
fn a_process_killed_at_every_step_of_a_write_leaves_the_old_or_the_new_store_whole() {
    let before_rename = [
        "AfterLock",
        "BeforeTempWrite",
        "MidTempWrite",
        "BeforeFileFsync",
        "BeforeRename",
    ];
    let after_rename = ["AfterRename", "BeforeDirFsync"];
    for point in before_rename.iter().chain(after_rename.iter()) {
        let world = World::new();
        let dir = world.store.dir().to_path_buf();
        let old = ready(&world);
        let old_bytes = std::fs::read(dir.join("store.json")).unwrap();
        let (success, _) = verdict(child(
            &dir,
            &[("WYSS_M1_CRASH_AT", point), ("WYSS_M1_TAG", "zz")],
        ));
        assert!(!success, "{point}: the child must have died");
        // Whatever the child did, the file is a complete valid store: never empty, never half.
        let now = match world.store.read().unwrap() {
            Loaded::Ready(store) => *store,
            other => panic!("{point}: {other:?}"),
        };
        if before_rename.contains(point) {
            assert_eq!(now, old, "{point}");
            assert_eq!(
                std::fs::read(dir.join("store.json")).unwrap(),
                old_bytes,
                "{point}"
            );
        } else {
            assert_eq!(now.revision.get(), old.revision.get() + 1, "{point}");
        }
        // The killed writer's lock died with it: the next writer is not blocked, and it
        // sweeps the temporary file the dead one left.
        let next = world
            .store
            .transact(now.revision, |s| relabel(s, "again"))
            .unwrap()
            .unwrap();
        assert_eq!(next.revision.get(), now.revision.get() + 1, "{point}");
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("store.json.tmp-")
            })
            .count();
        assert_eq!(leftovers, 0, "{point}");
    }
}

#[test]
fn an_error_before_the_rename_leaves_the_old_store_and_one_after_it_is_unknown() {
    for (point, committed) in [
        (Point::BeforeTempWrite, false),
        (Point::MidTempWrite, false),
        (Point::BeforeFileFsync, false),
        (Point::BeforeRename, false),
        (Point::AfterRename, true),
        (Point::BeforeDirFsync, true),
    ] {
        let fail: Faults = Arc::new(move |at: Point| {
            if at == point {
                Err(io::Error::other("injected"))
            } else {
                Ok(())
            }
        });
        let world = World::new().with_faults(fail);
        let old = ready(&world);
        let result = world.store.transact(old.revision, |s| relabel(s, "q"));
        let now = ready(&world);
        if committed {
            assert!(
                matches!(result, Err(StoreError::PublishUnknown(_))),
                "{point:?}"
            );
            assert_eq!(now.revision.get(), old.revision.get() + 1, "{point:?}");
        } else {
            assert!(matches!(result, Err(StoreError::Io(_))), "{point:?}");
            assert_eq!(now, old, "{point:?}");
        }
        let leftovers = std::fs::read_dir(world.store.dir())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("store.json.tmp-")
            })
            .count();
        assert_eq!(leftovers, 0, "{point:?}");
    }
}

// ---- a store that cannot be used is never replaced ---------------------------------------

fn write_raw(world: &World, bytes: &[u8], mode: u32) {
    let path = world.store.dir().join("store.json");
    std::fs::write(&path, bytes).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn a_corrupt_newer_or_oversized_store_is_read_only_and_keeps_its_exact_bytes() {
    let cases: Vec<(Vec<u8>, M1ErrorCode)> = vec![
        (b"{ not json".to_vec(), M1ErrorCode::InvalidRecord),
        (Vec::new(), M1ErrorCode::InvalidRecord),
        (
            br#"{"schema":2,"anything":[1,2,3]}"#.to_vec(),
            M1ErrorCode::UnsupportedSchema,
        ),
        (
            vec![b' '; 4 * 1024 * 1024 + 1],
            M1ErrorCode::CapacityExceeded,
        ),
    ];
    for (bytes, code) in cases {
        let world = World::new();
        write_raw(&world, &bytes, 0o600);
        let loaded = world.store.read().unwrap();
        let Loaded::Unusable(found) = loaded else {
            panic!("expected an unusable store");
        };
        assert_eq!(found.code, code);
        // Neither a change nor a fresh creation may replace it.
        let unusable = super::unusable(
            world
                .store
                .transact(Counter::new(1).unwrap(), |_| Ok::<(), ()>(()))
                .map(|r| r.unwrap().clone()),
        );
        assert_eq!(unusable.code, code);
        assert!(matches!(
            world.store.create(empty_store(&*world.env).unwrap()),
            Err(StoreError::Unusable(_))
        ));
        assert_eq!(
            std::fs::read(world.store.dir().join("store.json")).unwrap(),
            bytes
        );
    }
}

#[test]
fn a_store_with_loose_permissions_or_a_link_in_its_place_is_not_used() {
    let world = World::new();
    let good = std::fs::read(world.store.dir().join("store.json")).unwrap();
    write_raw(&world, &good, 0o644);
    assert!(matches!(world.store.read().unwrap(), Loaded::Unusable(_)));
    let world = World::new();
    let path = world.store.dir().join("store.json");
    std::fs::rename(&path, world.dir.0.join("elsewhere.json")).unwrap();
    std::os::unix::fs::symlink(world.dir.0.join("elsewhere.json"), &path).unwrap();
    assert!(matches!(world.store.read().unwrap(), Loaded::Unusable(_)));
}

#[test]
fn a_missing_store_is_missing_and_a_change_does_not_invent_one() {
    let dir = TestDir::new();
    let store = Store::open(dir.0.join("v1")).unwrap();
    assert_eq!(store.read().unwrap(), Loaded::Missing);
    let result = store.transact(Counter::new(1).unwrap(), |_| Ok::<(), ()>(()));
    assert!(matches!(result, Err(StoreError::StoreState("missing"))));
    assert_eq!(store.read().unwrap(), Loaded::Missing);
}

#[test]
fn a_link_among_the_users_own_ancestors_is_refused() {
    let dir = TestDir::new();
    std::fs::create_dir(dir.0.join("real")).unwrap();
    std::os::unix::fs::symlink(dir.0.join("real"), dir.0.join("link")).unwrap();
    assert!(Store::open(dir.0.join("link").join("v1")).is_err());
    assert!(Store::open(PathBuf::from("relative/v1")).is_err());
}

#[test]
fn a_leftover_temporary_file_does_not_block_a_writer_and_is_swept() {
    let world = World::new();
    std::fs::write(world.store.dir().join("store.json.tmp-deadbeef"), b"half").unwrap();
    let revision = ready(&world).revision;
    world
        .store
        .transact(revision, |s| relabel(s, "k"))
        .unwrap()
        .unwrap();
    assert!(!world.store.dir().join("store.json.tmp-deadbeef").exists());
}

#[test]
fn a_version_already_decoded_is_shared_until_the_file_is_replaced() {
    use crate::projects::store::Shared;
    let world = World::new();
    let first = match world.store.read_shared().unwrap() {
        Shared::Ready(store) => store,
        other => panic!("{other:?}"),
    };
    let again = match world.store.read_shared().unwrap() {
        Shared::Ready(store) => store,
        other => panic!("{other:?}"),
    };
    assert!(
        Arc::ptr_eq(&first, &again),
        "an unchanged file is decoded once"
    );
    // A commit replaces the file: the next read is the new version, not the cached one.
    let committed = world
        .store
        .transact(first.revision, |s| relabel(s, "n"))
        .unwrap()
        .unwrap();
    let after = match world.store.read_shared().unwrap() {
        Shared::Ready(store) => store,
        other => panic!("{other:?}"),
    };
    assert_eq!(after.revision, committed.revision);
    assert!(!Arc::ptr_eq(&first, &after));
    // And bytes that no longer decode are reported as they are, never served from the cache.
    write_raw(&world, b"{ not json", 0o600);
    assert!(matches!(
        world.store.read_shared().unwrap(),
        Shared::Unusable(_)
    ));
    std::fs::remove_file(world.store.dir().join("store.json")).unwrap();
    assert!(matches!(
        world.store.read_shared().unwrap(),
        Shared::Missing
    ));
}
