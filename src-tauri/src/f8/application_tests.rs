use super::{
    engine::{Spawned, Spawner},
    migration,
    protocol::RuntimeRef,
    staging::LaunchStaging,
};
use crate::{
    contracts::{
        WireUuid,
        control::{ControlError as E, ControlResult},
    },
    control::{
        application::{LocalSelection, PreparedApplication, run_selected},
        f8_fixture,
    },
};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};
struct NoSpawn;
impl Spawner for NoSpawn {
    fn spawn(&mut self, _: &RuntimeRef, _: Option<&mut LaunchStaging>) -> ControlResult<Spawned> {
        panic!("read/shutdown application fixture must not spawn")
    }
}
#[test]
fn prepared_application_excludes_a_second_writer_in_rollback_and_wal_modes() {
    let root = std::env::temp_dir().join(format!("wyss-f8-single-writer-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    for (mode, shared) in [
        ("delete", false),
        ("wal", false),
        ("delete", true),
        ("wal", true),
    ] {
        let path = root.join(format!("{mode}-{shared}.sqlite3"));
        let (seed, _handle) = fixture_application();
        seed.service()
            .authority()
            .engine()
            .connection()
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        let flags = rusqlite::OpenFlags::default()
            | if shared {
                rusqlite::OpenFlags::SQLITE_OPEN_SHARED_CACHE
            } else {
                rusqlite::OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
            };
        let first = rusqlite::Connection::open_with_flags(&path, flags).unwrap();
        first.pragma_update(None, "journal_mode", mode).unwrap();
        let second = rusqlite::Connection::open_with_flags(&path, flags).unwrap();
        let id = || WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
        let (running, _handle) = PreparedApplication::new(first, id(), None, NoSpawn).unwrap();
        assert!(
            matches!(
                PreparedApplication::new(second, id(), None, NoSpawn),
                Err(E::StorageUnavailable)
            ),
            "second writer admitted in {mode}, shared={shared}"
        );
        drop(running);
        let recovered = PreparedApplication::new(
            rusqlite::Connection::open(&path).unwrap(),
            id(),
            None,
            NoSpawn,
        )
        .unwrap();
        drop(recovered);
    }
    std::fs::remove_dir_all(root).unwrap();
}
fn fixture_application() -> (
    PreparedApplication<NoSpawn>,
    crate::control::application::ApplicationHandle,
) {
    let mut source = f8_fixture::v7_with_completed_turn();
    let connection = migration::prepare(&mut source).unwrap().into_connection();
    PreparedApplication::new(
        connection,
        WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap(),
        None,
        NoSpawn,
    )
    .unwrap()
}
#[test]
fn prepared_application_rejects_unmigrated_storage_without_fallback() {
    let source = f8_fixture::v7_with_completed_turn();
    assert!(matches!(
        PreparedApplication::new(
            source,
            WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap(),
            None,
            NoSpawn
        ),
        Err(E::StorageIncompatible)
    ));
}

#[test]
fn prepared_application_opens_v10_with_authority_owned_guard_but_keeps_capability_closed() {
    let mut source = f8_fixture::v7_with_sessions(0);
    let v8 = migration::prepare(&mut source).unwrap().into_connection();
    let v9 = crate::f8::v9::migrate(&v8, |_| Ok(())).unwrap();
    let v10 = crate::f8::v10::migrate(&v9, |_| Ok(())).unwrap();
    let authority_id = WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
    let (application, _handle) =
        PreparedApplication::new(v10, authority_id.clone(), None, NoSpawn).unwrap();
    let authority = application.service().authority();
    assert_eq!(authority.engine().authority_id(), &authority_id);
    assert!(
        !authority.workspace_guard_ready().unwrap(),
        "the M6 import fence remains closed until an explicit complete snapshot is accepted"
    );
    assert_eq!(
        crate::f8::provider::version(authority.engine().connection()).unwrap(),
        10
    );
    let epoch_count: i64 = authority
        .engine()
        .connection()
        .query_row(
            "SELECT count(*) FROM authority_epoch_history WHERE authority_epoch=?1",
            [authority_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        epoch_count, 1,
        "the F8 authority instance must be durably unique"
    );
}
#[test]
fn local_application_runner_negotiates_two_and_shutdown_bypasses_full_admission_queue() {
    let (application, handle) = fixture_application();
    let (owned, mut client) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    handle.attach(owned).unwrap();
    let run = std::thread::spawn(move || {
        run_selected(LocalSelection::PreparedProtocol2(Box::new(application)))
    });
    let request = serde_json::to_vec(&serde_json::json!({"schema_version":2,
        "request_id":uuid::Uuid::new_v4().to_string(),"method":"authority/hello",
        "params":{"protocol_version":2,"client_instance_id":uuid::Uuid::new_v4().to_string()}}))
    .unwrap();
    client
        .write_all(&(request.len() as u32).to_be_bytes())
        .unwrap();
    client.write_all(&request).unwrap();
    let mut size = [0; 4];
    client.read_exact(&mut size).unwrap();
    let mut answer = vec![0; u32::from_be_bytes(size) as usize];
    client.read_exact(&mut answer).unwrap();
    let answer: serde_json::Value = serde_json::from_slice(&answer).unwrap();
    assert_eq!(answer["result"]["value"]["protocol_version"], 2);
    handle.shutdown();
    assert_eq!(run.join().unwrap(), Ok(()));
    assert_eq!(
        handle.attach(UnixStream::pair().unwrap().0),
        Err(E::StorageUnavailable)
    );

    let (application, handle) = fixture_application();
    let mut peers = Vec::new();
    for _ in 0..8 {
        let (owned, peer) = UnixStream::pair().unwrap();
        handle.attach(owned).unwrap();
        peers.push(peer);
    }
    assert_eq!(
        handle.attach(UnixStream::pair().unwrap().0),
        Err(E::CapacityExceeded)
    );
    handle.shutdown();
    assert_eq!(
        run_selected(LocalSelection::PreparedProtocol2(Box::new(application))),
        Ok(())
    );
}
#[test]
fn application_idle_measurement_keeps_storage_unchanged() {
    let (mut application, _handle) = fixture_application();
    let before = application
        .service()
        .authority()
        .engine()
        .connection()
        .total_changes();
    let cpu = || {
        let mut usage = std::mem::MaybeUninit::<nix::libc::rusage>::uninit();
        assert_eq!(
            unsafe { nix::libc::getrusage(nix::libc::RUSAGE_THREAD, usage.as_mut_ptr()) },
            0
        );
        let usage = unsafe { usage.assume_init() };
        usage.ru_utime.tv_sec * 1_000_000
            + usage.ru_utime.tv_usec
            + usage.ru_stime.tv_sec * 1_000_000
            + usage.ru_stime.tv_usec
    };
    let started = Instant::now();
    let cpu_start = cpu();
    for _ in 0..20 {
        application.poll(Instant::now()).unwrap();
        std::thread::sleep(Duration::from_millis(25));
    }
    let cpu_us = cpu() - cpu_start;
    let after = application
        .service()
        .authority()
        .engine()
        .connection()
        .total_changes();
    assert_eq!(
        after, before,
        "idle supervision must not append transactions"
    );
    println!(
        "F8_IDLE polls=20 wall_us={} thread_cpu_us={cpu_us} sqlite_changes={}",
        started.elapsed().as_micros(),
        after - before
    );
}
