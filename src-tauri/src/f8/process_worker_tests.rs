use super::*;
use std::time::Duration;

fn quick() -> Grace {
    Grace {
        eof: Duration::from_millis(150),
        term: Duration::from_millis(150),
        kill: Duration::from_millis(2000),
    }
}
fn worker(script: &str) -> ProcessWorker {
    let mut c = Command::new("/bin/sh");
    c.arg("-c").arg(script);
    ProcessWorker::spawn(&mut c, quick()).unwrap()
}
fn until<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < end, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn finish(w: &mut ProcessWorker) {
    w.begin_stop(Instant::now());
    until(|| (w.poll_stop(Instant::now()) == StopState::Reaped).then_some(()));
}

#[test]
fn frames_round_trip_through_a_real_child() {
    let mut w = worker("cat");
    w.send(b"{\"id\":1}").unwrap();
    w.send(b"{\"id\":2}").unwrap();
    let mut got = Vec::new();
    until(|| {
        got.extend(w.inbound(8).unwrap());
        (got.len() == 2).then_some(())
    });
    assert_eq!(got, vec![b"{\"id\":1}".to_vec(), b"{\"id\":2}".to_vec()]);
    // Frames are single lines: an embedded newline can never smuggle a second frame.
    assert_eq!(w.send(b"a\nb"), Err(E::FrameTooLarge));
    assert_eq!(
        w.send(&vec![b'x'; OUTBOUND_FRAME_MAX + 1]),
        Err(E::FrameTooLarge)
    );
    finish(&mut w);
}

#[test]
fn a_child_that_never_reads_is_back_pressure_not_a_hang() {
    let mut w = worker("sleep 30");
    let started = Instant::now();
    let frame = vec![b'z'; 60_000];
    let mut refused = None;
    for i in 0..20 {
        match w.send(&frame) {
            Ok(()) => (),
            Err(e) => {
                refused = Some((i, e));
                break;
            }
        }
    }
    let (_, e) = refused.expect("the outbound buffer is bounded");
    assert_eq!(e, E::CapacityExceeded);
    assert!(w.buffered_outbound() <= OUTBOUND_BUFFER_MAX);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "sends never wait"
    );
    finish(&mut w);
}

#[test]
fn inbound_floods_are_read_in_bounded_slices() {
    let mut w = worker("yes aaaa");
    until(|| (!w.inbound(4).unwrap().is_empty()).then_some(()));
    let mut total = 0;
    for _ in 0..200 {
        let started = Instant::now();
        let frames = w.inbound(4).unwrap();
        assert!(frames.len() <= 4);
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(
            w.inbuf.len() <= INBOUND_FRAME_MAX + PROVIDER_SCRATCH,
            "buffered bytes are bounded"
        );
        total += frames.len();
    }
    assert!(total > 0);
    finish(&mut w);
}

#[test]
fn an_oversized_line_fails_the_worker_for_good() {
    let mut w = worker("head -c 400000 /dev/zero | tr '\\0' a; echo; sleep 30");
    let err = until(|| w.inbound(8).err());
    assert_eq!(err, E::FrameTooLarge);
    assert_eq!(w.failure(), Some(E::FrameTooLarge));
    assert_eq!(w.send(b"x"), Err(E::FrameTooLarge));
    assert_eq!(w.inbound(1), Err(E::FrameTooLarge));
    finish(&mut w);
}

#[test]
fn a_chatty_stderr_cannot_wedge_the_child() {
    // 5 MB of stderr would block the child on a full pipe if nobody drained it.
    let mut w = worker("head -c 5000000 /dev/zero >&2; echo done");
    let mut got = Vec::new();
    until(|| {
        got.extend(w.inbound(4).unwrap());
        (!got.is_empty()).then_some(())
    });
    assert_eq!(got, vec![b"done".to_vec()]);
    finish(&mut w);
}

#[test]
fn exit_is_detected_and_stop_closes_stdin_first() {
    let mut w = worker("exit 3");
    until(|| w.has_exited().then_some(()));
    finish(&mut w);
    // A cooperative child ends on EOF alone; afterwards nothing can be sent.
    let mut w = worker("cat >/dev/null");
    w.begin_stop(Instant::now());
    assert_eq!(w.send(b"late"), Err(E::TransportError));
    until(|| (w.poll_stop(Instant::now()) == StopState::Reaped).then_some(()));
}

#[test]
fn an_unresponsive_child_is_killed_through_the_owned_handle_only() {
    let mut other = std::process::Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut w = worker("trap '' TERM; while true; do sleep 0.05; done");
    finish(&mut w);
    assert!(
        other.try_wait().unwrap().is_none(),
        "the unrelated same-path process survives"
    );
    other.kill().unwrap();
    other.wait().unwrap();
}

#[test]
fn critical_approval_write_is_atomic_and_never_queued_after_backpressure() {
    let mut w = worker("cat");
    w.send_critical(b"approval item-1 accept").unwrap();
    let frame = until(|| w.inbound(1).unwrap().into_iter().next());
    assert_eq!(frame, b"approval item-1 accept".to_vec());
    assert_eq!(w.buffered_outbound(), 0);
    assert_eq!(
        w.send_critical(b"approval\nitem-1 accept"),
        Err(E::FrameTooLarge)
    );
    assert_eq!(w.send_critical(&vec![b'x'; 4096]), Err(E::FrameTooLarge));
    finish(&mut w);

    let mut blocked = worker("sleep 30");
    let huge = vec![b'z'; 60_000];
    for _ in 0..20 {
        let _ = blocked.send(&huge);
        if blocked.buffered_outbound() != 0 {
            break;
        }
    }
    assert!(
        blocked.buffered_outbound() > 0,
        "fixture must saturate the child pipe"
    );
    let queued = blocked.buffered_outbound();
    let start = Instant::now();
    assert_eq!(
        blocked.send_critical(b"approval item-1 accept"),
        Err(E::CapacityExceeded)
    );
    assert_eq!(
        blocked.buffered_outbound(),
        queued,
        "no approval was enqueued"
    );
    assert!(start.elapsed() < Duration::from_millis(500));
    finish(&mut blocked);
}

#[test]
fn eof_during_a_frame_is_a_transport_failure() {
    let mut worker = worker("printf incomplete");
    assert_eq!(until(|| worker.inbound(8).err()), E::TransportError);
    assert_eq!(worker.failure(), Some(E::TransportError));
    finish(&mut worker);
}

#[test]
fn deferred_process_worker_returns_before_spawn_and_preserves_round_trip() {
    let mut command = Command::new("/bin/cat");
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let pending = PendingOwnedSpawn::fixture(
        command,
        quick(),
        false,
        Duration::from_secs(5),
        Duration::from_millis(150),
    )
    .unwrap();
    let mut worker =
        DeferredProcessWorker::pending(pending, quick(), ContainmentPolicy::DirectChildOnly);
    assert!(worker.pending.is_some());
    worker.send(b"initialization").unwrap();
    assert_eq!(
        worker.send_critical(b"approval item accept"),
        Err(E::CapacityExceeded)
    );
    let frame = until(|| {
        worker.flush().unwrap();
        worker.inbound(1).unwrap().into_iter().next()
    });
    assert_eq!(frame, b"initialization");
    worker.begin_stop(Instant::now());
    until(|| (worker.poll_stop(Instant::now()) == StopState::Reaped).then_some(()));
}

#[test]
fn deferred_timeout_keeps_engine_slot_and_staging_until_reap() {
    use crate::{
        contracts::WireUuid,
        control::f8_fixture,
        f8::{
            engine::{Engine, Spawned, Spawner},
            migration,
            protocol::{Request, RuntimeRef},
            registry::Registry,
            staging::{LaunchStaging, StagingRoot},
        },
    };
    use serde_json::json;
    use std::os::unix::fs::DirBuilderExt;
    struct Delayed;
    impl Spawner for Delayed {
        fn spawn(
            &mut self,
            _: &RuntimeRef,
            staging: Option<&mut LaunchStaging>,
        ) -> ControlResult<Spawned> {
            staging.unwrap().write_file("catalog.json", b"{}").unwrap();
            let mut command = Command::new("/bin/cat");
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let pending = PendingOwnedSpawn::fixture(
                command,
                quick(),
                false,
                Duration::ZERO,
                Duration::from_millis(250),
            )?;
            Ok(Spawned {
                worker: Box::new(DeferredProcessWorker::pending(
                    pending,
                    quick(),
                    ContainmentPolicy::DirectChildOnly,
                )),
            })
        }
    }
    let root = std::env::temp_dir().join(format!("wyss-f8-deferred-{}", uuid::Uuid::new_v4()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let mut source = f8_fixture::v7_with_sessions(1);
    let conn = migration::prepare(&mut source).unwrap().into_connection();
    let sessions: Vec<String> = conn
        .prepare("SELECT session_id FROM managed_sessions ORDER BY created_sequence")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let auth = WireUuid::try_from("00000000-0000-4000-8000-0000000000a1".to_owned()).unwrap();
    let mut engine = Engine::new(
        conn,
        auth.clone(),
        Registry::new(auth.clone()).unwrap(),
        Some(StagingRoot::open(&root).unwrap()),
    )
    .unwrap();
    let request = |engine: &Engine, session: &str| {
        let (generation, revision): (i64, i64) = engine
            .connection()
            .query_row(
                "SELECT generation,revision FROM managed_runtimes WHERE managed_session_id=?1",
                [session],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap_or((0, 0));
        Request::parse(&serde_json::to_vec(&json!({"schema_version":2,"request_id":uuid::Uuid::new_v4().to_string(),
            "authority_instance_id":auth,"method":"runtime/start","params":{
                "intent_id":uuid::Uuid::new_v4().to_string(),"adapter_id":"codex","runtime_kind":"codex",
                "managed_session_id":session,"expected_generation":generation,"expected_revision":revision
            }})).unwrap()).unwrap()
    };
    let outcome = engine
        .start(&request(&engine, &sessions[0]), &mut Delayed)
        .unwrap();
    let staging = root.join(outcome.runtime_instance_id.unwrap().as_str());
    engine.poll(Instant::now());
    assert!(
        engine.is_live(&sessions[0]),
        "spawn timeout must retain the live cleanup owner"
    );
    assert!(
        staging.join("catalog.json").exists(),
        "staging cannot be cleaned before reap"
    );
    assert!(matches!(
        engine.start(&request(&engine, &sessions[1]), &mut Delayed),
        Err(E::CapacityExceeded)
    ));
    until(|| {
        engine.poll(Instant::now());
        (!engine.is_live(&sessions[0])).then_some(())
    });
    assert!(
        !staging.exists(),
        "only a proved reap permits staging cleanup"
    );
    std::fs::remove_dir(root).unwrap();
}

// ---- ENG-03: the provider scratch ceiling --------------------------------------------------

#[test]
fn the_scratch_buffer_is_the_contracts_number_and_is_classified_by_what_enforces_it() {
    let contract = crate::f8::budget::contract().unwrap();
    assert_eq!(
        contract["provider_scratch_bytes"], PROVIDER_SCRATCH as u64,
        "resources.json and the worker's read buffer name different ceilings"
    );
    // The scratch is far below the frame bound: a frame is assembled, never read in one piece.
    const _: () = assert!(PROVIDER_SCRATCH < INBOUND_FRAME_MAX);
    assert_eq!(contract["provider_frame_bytes"], INBOUND_FRAME_MAX as u64);
    match crate::f8::budget::classify("provider_scratch_bytes") {
        Some(crate::f8::budget::Enforcement::Enforced(by)) => {
            assert!(by.contains("process_worker"), "{by}")
        }
        other => panic!("scratch is not classified as enforced here: {other:?}"),
    }
}

#[test]
fn no_single_stdout_read_exceeds_the_scratch_however_much_the_child_wrote() {
    // 64 KiB lines written as fast as the pipe takes them: every read is offered far more
    // than the scratch holds.
    let mut w = worker("yes \"$(head -c 65000 /dev/zero | tr '\\0' a)\"");
    let mut frames = 0;
    until(|| {
        let got = w.inbound(4).unwrap();
        assert!(got.iter().all(|f| f.len() == 65000), "a frame lost bytes");
        frames += got.len();
        (frames >= 8).then_some(())
    });
    assert!(w.largest_read > 0);
    assert!(
        w.largest_read <= PROVIDER_SCRATCH,
        "one read took {} bytes",
        w.largest_read
    );
    // The pipe was full the whole time, so the bound was actually reached, not merely unmet.
    assert_eq!(w.largest_read, PROVIDER_SCRATCH);
    finish(&mut w);
}

#[test]
fn frames_at_and_around_the_scratch_and_frame_bounds_arrive_whole() {
    let sizes = [1usize, 4095, 4096, 4097, 8192, 8193, INBOUND_FRAME_MAX];
    let script = format!(
        "for n in {}; do head -c $n /dev/zero | tr '\\0' a; echo; done; sleep 30",
        sizes.map(|n| n.to_string()).join(" ")
    );
    let mut w = worker(&script);
    let mut got = Vec::new();
    until(|| {
        got.extend(w.inbound(2).unwrap().into_iter().map(|f| f.len()));
        (got.len() == sizes.len()).then_some(())
    });
    assert_eq!(got, sizes);
    assert!(w.largest_read <= PROVIDER_SCRATCH);
    finish(&mut w);
}

#[test]
fn a_frame_one_byte_over_the_frame_bound_is_refused() {
    let mut w = worker(&format!(
        "head -c {} /dev/zero | tr '\\0' a; echo; sleep 30",
        INBOUND_FRAME_MAX + 1
    ));
    assert_eq!(until(|| w.inbound(8).err()), E::FrameTooLarge);
    finish(&mut w);
}

#[test]
fn a_frame_that_arrives_in_pieces_is_delivered_once_and_only_when_complete() {
    let mut w = worker(
        "printf '{\"a\":'; sleep 0.2; printf '1,\"b\":'; sleep 0.2; printf '2}\\n{\"c\"'; \
         sleep 0.2; printf ':3}\\n'; sleep 30",
    );
    let mut got: Vec<Vec<u8>> = Vec::new();
    let mut polls_with_nothing = 0;
    until(|| {
        let frames = w.inbound(8).unwrap();
        if frames.is_empty() {
            polls_with_nothing += 1;
        }
        got.extend(frames);
        (got.len() == 2).then_some(())
    });
    assert_eq!(
        got,
        vec![b"{\"a\":1,\"b\":2}".to_vec(), b"{\"c\":3}".to_vec()]
    );
    // The pieces were seen before their newline and held back, not handed on as frames.
    assert!(polls_with_nothing > 0);
    finish(&mut w);
}

#[test]
fn a_child_flooding_both_pipes_with_a_slow_reader_still_takes_control_and_stops() {
    let mut w = worker(
        "(while :; do head -c 65536 /dev/zero >&2; done) & \
         yes \"$(head -c 3000 /dev/zero | tr '\\0' b)\"",
    );
    let mut frames = 0;
    for _ in 0..40 {
        // A reader much slower than the child: the pipes stay full between polls.
        std::thread::sleep(Duration::from_millis(10));
        let started = Instant::now();
        let got = w.inbound(4).unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(got.len() <= 4 && got.iter().all(|f| f.len() == 3000));
        assert!(w.inbuf.len() <= INBOUND_FRAME_MAX + PROVIDER_SCRATCH);
        frames += got.len();
        // A control frame is accepted (or refused at once by back pressure), never a wait.
        let asked = Instant::now();
        let _ = w.send(b"{\"control\":true}");
        assert!(asked.elapsed() < Duration::from_millis(500));
    }
    assert!(frames > 0);
    assert!(w.largest_read <= PROVIDER_SCRATCH);
    // Stop is still served while both pipes are saturated.
    finish(&mut w);
}
