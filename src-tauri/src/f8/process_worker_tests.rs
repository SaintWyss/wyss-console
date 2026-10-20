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
            w.inbuf.len() <= INBOUND_FRAME_MAX + CHUNK,
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
