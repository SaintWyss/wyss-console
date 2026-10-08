use super::*;
use std::{sync::Barrier, thread, time::Duration};

fn far() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
/// A connected client socket pair: the server side a watcher polls, and the client's end.
fn client() -> (UnixStream, UnixStream) {
    let (server, client) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();
    (server, client)
}
#[test]
fn a_commit_announced_before_registration_is_seen_without_sleeping() {
    let signal = EventSignal::new();
    signal.publish(7);
    let (server, _client) = client();
    let mut w = signal.register().unwrap();
    let started = Instant::now();
    assert_eq!(w.wait(6, &server, far()), Wake::Advanced);
    assert_eq!(w.wait(0, &server, far()), Wake::Advanced);
    assert!(started.elapsed() < Duration::from_secs(1));
}
#[test]
fn a_commit_announced_after_registration_wakes_the_sleeper() {
    let signal = EventSignal::new();
    signal.publish(7);
    let (server, _client) = client();
    let mut w = signal.register().unwrap();
    let publisher = signal.clone();
    let t = thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        publisher.publish(8);
    });
    let started = Instant::now();
    assert_eq!(w.wait(7, &server, far()), Wake::Advanced);
    assert!(
        started.elapsed() >= Duration::from_millis(60),
        "it really slept"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "and the commit woke it"
    );
    t.join().unwrap();
}
#[test]
fn no_wakeup_is_lost_however_registration_and_commit_interleave() {
    let signal = EventSignal::new();
    let (server, _client) = client();
    for head in 1..=3000u64 {
        let publisher = signal.clone();
        let gate = std::sync::Arc::new(Barrier::new(2));
        let go = gate.clone();
        let t = thread::spawn(move || {
            go.wait();
            // Spread the commit across the register/check/sleep window.
            for _ in 0..(head % 7) * 40 {
                std::hint::spin_loop();
            }
            publisher.publish(head);
        });
        gate.wait();
        if head % 3 == 0 {
            thread::yield_now();
        }
        let mut w = signal.register().unwrap();
        let started = Instant::now();
        assert_eq!(
            w.wait(head - 1, &server, Instant::now() + Duration::from_secs(10)),
            Wake::Advanced,
            "head {head}"
        );
        // A lost wakeup would still report progress, but only once the sleep timed out.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "head {head}: woken by the timeout, not by the commit"
        );
        drop(w);
        t.join().unwrap();
        assert_eq!(signal.waiting(), 0);
    }
}
#[test]
fn a_stale_or_repeated_head_never_reports_progress() {
    let signal = EventSignal::new();
    signal.publish(5);
    let (server, _client) = client();
    let mut w = signal.register().unwrap();
    let publisher = signal.clone();
    let t = thread::spawn(move || {
        // Announcing the same or an older head moves nothing and wakes nobody for it.
        publisher.publish(5);
        publisher.publish(3);
    });
    let started = Instant::now();
    assert_eq!(
        w.wait(5, &server, Instant::now() + Duration::from_millis(150)),
        Wake::Elapsed
    );
    assert!(started.elapsed() >= Duration::from_millis(140));
    t.join().unwrap();
    // A zero wait returns at once.
    assert_eq!(w.wait(5, &server, Instant::now()), Wake::Elapsed);
}
#[test]
fn every_watcher_at_the_same_cursor_wakes_for_one_commit() {
    let signal = EventSignal::new();
    let ready = std::sync::Arc::new(Barrier::new(MAX_WATCHERS + 1));
    let threads: Vec<_> = (0..MAX_WATCHERS)
        .map(|_| {
            let (signal, ready) = (signal.clone(), ready.clone());
            thread::spawn(move || {
                let (server, _client) = client();
                let mut w = signal.register().unwrap();
                ready.wait();
                w.wait(0, &server, far())
            })
        })
        .collect();
    ready.wait();
    assert_eq!(signal.waiting(), MAX_WATCHERS);
    signal.publish(1);
    let started = Instant::now();
    for t in threads {
        assert_eq!(t.join().unwrap(), Wake::Advanced);
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "woken, not timed out"
    );
    assert_eq!(
        signal.waiting(),
        0,
        "nothing is consumed and every slot returns"
    );
}
#[test]
fn watcher_capacity_is_bounded_fails_fast_and_is_returned_on_drop() {
    assert_eq!(MAX_WATCHERS, 4);
    let signal = EventSignal::new();
    let mut held: Vec<Waiter> = (0..MAX_WATCHERS)
        .map(|_| signal.register().unwrap())
        .collect();
    let started = Instant::now();
    assert!(matches!(
        signal.register(),
        Err(ControlError::CapacityExceeded)
    ));
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "no queueing"
    );
    held.pop();
    assert_eq!(signal.waiting(), MAX_WATCHERS - 1);
    held.push(signal.register().unwrap());
    assert!(signal.register().is_err());
}
#[test]
fn shutdown_wakes_every_watcher_at_once_and_admits_no_new_one() {
    let signal = EventSignal::new();
    let ready = std::sync::Arc::new(Barrier::new(MAX_WATCHERS + 1));
    let threads: Vec<_> = (0..MAX_WATCHERS)
        .map(|_| {
            let (signal, ready) = (signal.clone(), ready.clone());
            thread::spawn(move || {
                let (server, _client) = client();
                let mut w = signal.register().unwrap();
                ready.wait();
                w.wait(0, &server, Instant::now() + Duration::from_secs(30))
            })
        })
        .collect();
    ready.wait();
    let started = Instant::now();
    signal.close();
    for t in threads {
        assert_eq!(t.join().unwrap(), Wake::Closed);
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "not the full wait"
    );
    assert!(matches!(
        signal.register(),
        Err(ControlError::TransportError)
    ));
    // A head announced after shutdown changes nothing.
    signal.publish(9);
}
#[test]
fn a_client_that_hangs_up_or_speaks_ends_the_wait_and_frees_the_slot() {
    let signal = EventSignal::new();
    let (server, peer) = client();
    let mut w = signal.register().unwrap();
    let t = thread::spawn(move || {
        thread::sleep(Duration::from_millis(60));
        drop(peer);
    });
    let started = Instant::now();
    assert_eq!(w.wait(0, &server, far()), Wake::ClientGone);
    assert!(started.elapsed() < Duration::from_secs(2));
    t.join().unwrap();
    drop(w);
    assert_eq!(signal.waiting(), 0);

    let (server, mut peer) = client();
    let mut w = signal.register().unwrap();
    peer.write_all(b"next").unwrap();
    assert_eq!(w.wait(0, &server, far()), Wake::ClientSpoke);
    // The bytes were only peeked: the next request is intact for the frame reader.
    let mut next = [0u8; 4];
    (&server).read_exact(&mut next).unwrap();
    assert_eq!(&next, b"next");
}
#[test]
fn a_flood_of_commits_never_blocks_the_publisher_or_grows_without_bound() {
    let signal = EventSignal::new();
    let (server, _client) = client();
    // A registered watcher that is not reading its wake channel at all.
    let mut idle = signal.register().unwrap();
    let started = Instant::now();
    for head in 1..=200_000u64 {
        signal.publish(head);
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a full channel is skipped, never waited on"
    );
    // One wakeup still tells it everything it needs: where the head is now.
    assert_eq!(idle.wait(0, &server, far()), Wake::Advanced);
    assert_eq!(idle.wait(199_999, &server, far()), Wake::Advanced);
    assert_eq!(
        idle.wait(200_000, &server, Instant::now() + Duration::from_millis(50)),
        Wake::Elapsed
    );
}
