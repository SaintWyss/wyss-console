//! Real file/WAL fixtures. No installed database, provider or service is opened.
use super::{
    allocator::{Admission, Allocator},
    migration,
    protocol::Request,
    v8::validate_v8,
};
use crate::{
    contracts::{WireUuid, control::ControlError},
    control::f8_fixture,
};
use rusqlite::Connection;
use serde_json::json;
use std::{os::unix::fs::DirBuilderExt, path::PathBuf, time::Duration};

const AUTH: &str = "00000000-0000-4000-8000-0000000000a1";

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("wyss-f8-storage-{}", uuid::Uuid::new_v4()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn authority() -> WireUuid {
    WireUuid::try_from(AUTH.to_owned()).unwrap()
}
fn start(session: &str) -> Request {
    Request::parse(
        &serde_json::to_vec(&json!({"schema_version":2,
        "request_id":uuid::Uuid::new_v4().to_string(), "authority_instance_id":AUTH,
        "method":"runtime/start","params":{"intent_id":uuid::Uuid::new_v4().to_string(),
        "adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,
        "expected_revision":0,"expected_generation":0}}))
        .unwrap(),
    )
    .unwrap()
}
fn snapshot(c: &Connection, path: &std::path::Path) {
    c.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}
fn fixture(files: &Files) -> (Connection, String) {
    let mut source = f8_fixture::v7_with_completed_turn();
    let image = migration::prepare(&mut source).unwrap().into_connection();
    let session = image
        .query_row("SELECT session_id FROM managed_sessions", [], |r| r.get(0))
        .unwrap();
    let path = files.0.join("fixture.sqlite3");
    snapshot(&image, &path);
    let c = Connection::open(path).unwrap();
    c.pragma_update(None, "journal_mode", "WAL").unwrap();
    c.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    c.pragma_update(None, "synchronous", "FULL").unwrap();
    c.busy_timeout(Duration::ZERO).unwrap();
    (c, session)
}
fn events(c: &Connection) -> Vec<String> {
    c.prepare("SELECT envelope FROM control_events ORDER BY sequence")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn wal_snapshot_restore_and_checkpoint_preserve_exact_mixed_history() {
    let files = Files::new();
    let (mut c, session) = fixture(&files);
    let before = events(&c);
    let request = start(&session);
    Allocator::new(&mut c, authority())
        .unwrap()
        .start(
            &request,
            &Admission {
                live: None,
                slot_available: true,
            },
        )
        .unwrap();
    let summary = validate_v8(&c).unwrap();
    assert_eq!(summary.launches, 1);
    assert_eq!(summary.legacy_turns, 1);
    let accepted = events(&c);
    assert_eq!(&accepted[..before.len()], before);
    let wal = files.0.join("fixture.sqlite3-wal");
    assert!(std::fs::metadata(&wal).unwrap().len() > 32);
    let backup = files.0.join("snapshot.sqlite3");
    snapshot(&c, &backup);
    let restored = Connection::open(&backup).unwrap();
    assert_eq!(validate_v8(&restored).unwrap(), summary);
    assert_eq!(events(&restored), accepted);
    let checkpoint: (i64, i64, i64) = c
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(checkpoint.0, 0);
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0);
    drop(c);
    let mut reopened = Connection::open(files.0.join("fixture.sqlite3")).unwrap();
    assert_eq!(validate_v8(&reopened).unwrap(), summary);
    let replay = Allocator::new(&mut reopened, authority())
        .unwrap()
        .start(
            &request,
            &Admission {
                live: None,
                slot_available: false,
            },
        )
        .unwrap();
    assert!(replay.replayed && !replay.reserved);
    assert_eq!(events(&reopened), accepted);
}

#[test]
fn busy_writer_refuses_without_a_partial_intent_and_retry_commits_once() {
    let files = Files::new();
    let (mut c, session) = fixture(&files);
    let before = events(&c);
    let lock = Connection::open(files.0.join("fixture.sqlite3")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let request = start(&session);
    let admission = Admission {
        live: None,
        slot_available: true,
    };
    let error = Allocator::new(&mut c, authority())
        .unwrap()
        .start(&request, &admission)
        .unwrap_err();
    assert_eq!(error, ControlError::StorageUnavailable);
    assert_eq!(events(&c), before);
    lock.execute_batch("ROLLBACK").unwrap();
    let outcome = Allocator::new(&mut c, authority())
        .unwrap()
        .start(&request, &admission)
        .unwrap();
    assert!(outcome.reserved && !outcome.replayed);
    validate_v8(&c).unwrap();
    let after = events(&c);
    assert!(
        Allocator::new(&mut c, authority())
            .unwrap()
            .start(&request, &admission)
            .unwrap()
            .replayed
    );
    assert_eq!(events(&c), after);
}
