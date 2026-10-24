use super::*;
use crate::screen::{
    helper,
    sandbox::Applied,
    {Limits, Terminal},
};
use std::{
    os::unix::net::UnixStream,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// A ring stand-in: bytes with a start offset, appended by the test.
struct Bytes {
    inner: Mutex<(u64, Vec<u8>)>,
}
impl Bytes {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new((0, Vec::new())),
        })
    }
    /// Appends, and forgets the oldest bytes beyond 256 KiB, as the host's ring does.
    fn push(&self, data: &[u8]) {
        let mut g = lock(&self.inner);
        g.1.extend_from_slice(data);
        if g.1.len() > 256 * 1024 {
            let drop = g.1.len() - 256 * 1024;
            g.1.drain(..drop);
            g.0 += drop as u64;
        }
    }
    /// Forgets everything before `offset`, as a full ring does.
    fn evict_to(&self, offset: u64) {
        let mut g = lock(&self.inner);
        let drop = (offset - g.0) as usize;
        g.1.drain(..drop);
        g.0 = offset;
    }
}
impl ByteSource for Bytes {
    fn start(&self) -> u64 {
        lock(&self.inner).0
    }
    fn end(&self) -> u64 {
        let g = lock(&self.inner);
        g.0 + g.1.len() as u64
    }
    fn read(&self, from: u64, max: usize) -> Vec<u8> {
        let g = lock(&self.inner);
        let at = (from - g.0) as usize;
        g.1[at..].iter().take(max).copied().collect()
    }
}

#[derive(Clone, Copy, Debug)]
enum Behavior {
    Normal,
    /// The helper's input ends after this many bytes have been read: it exits cleanly.
    EndsAfter(usize),
    /// Reads the hello and then does nothing at all.
    Hangs,
    /// Answers the hello with a frame that is not a frame.
    Garbles,
    /// Applies fewer isolation layers than were required.
    Weak,
}

struct CountingReader {
    inner: UnixStream,
    left: usize,
}
impl Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Ok(0);
        }
        let cap = buf.len().min(self.left);
        let n = self.inner.read(&mut buf[..cap])?;
        self.left -= n;
        Ok(n)
    }
}

struct SocketChild {
    sockets: Vec<UnixStream>,
    killed: Arc<AtomicBool>,
}
impl ChildHandle for SocketChild {
    fn pid(&self) -> Option<u32> {
        Some(4242)
    }
    fn kill(&mut self) {
        self.killed.store(true, Ordering::SeqCst);
        for s in &self.sockets {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
    fn reap(&mut self) {}
}

struct Scripted {
    script: Mutex<VecDeque<Result<Behavior, ()>>>,
    spawned: AtomicUsize,
    killed: Arc<AtomicBool>,
}
impl Scripted {
    fn new(script: Vec<Result<Behavior, ()>>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(script.into()),
            spawned: AtomicUsize::new(0),
            killed: Arc::new(AtomicBool::new(false)),
        })
    }
}
impl Spawner for Scripted {
    fn spawn(&self) -> std::io::Result<Spawned> {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        let next = lock(&self.script)
            .pop_front()
            .unwrap_or(Ok(Behavior::Normal));
        let behavior = next.map_err(|()| std::io::Error::other("no helper"))?;
        let (host, helper_side) = UnixStream::pair()?;
        let host_reader = host.try_clone()?;
        let handle = SocketChild {
            sockets: vec![host.try_clone()?, helper_side.try_clone()?],
            killed: self.killed.clone(),
        };
        thread::spawn(move || {
            let mut writer = helper_side.try_clone().unwrap();
            let layers = if matches!(behavior, Behavior::Weak) {
                layer::NO_NEW_PRIVS
            } else {
                layer::ALL
            };
            let applied = Applied {
                layers,
                landlock_abi: 6,
            };
            match behavior {
                Behavior::Normal | Behavior::Weak => {
                    let mut reader = std::io::BufReader::new(helper_side);
                    helper::serve(&mut reader, &mut writer, applied);
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                }
                Behavior::EndsAfter(n) => {
                    let mut reader = std::io::BufReader::new(CountingReader {
                        inner: helper_side,
                        left: n,
                    });
                    helper::serve(&mut reader, &mut writer, applied);
                    // A process that ends closes its pipes, whoever else holds a copy.
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                }
                Behavior::Hangs => {
                    let mut reader = helper_side;
                    let mut sink = [0u8; 256];
                    while reader.read(&mut sink).unwrap_or(0) > 0 {}
                }
                Behavior::Garbles => {
                    let mut reader = std::io::BufReader::new(helper_side);
                    let _ = ipc::read_frame(&mut reader, MAX_REQUEST);
                    let _ = writer.write_all(&[0, 0, 0, 2, 0x7f, 0x00]);
                    let mut sink = [0u8; 256];
                    while reader.get_mut().read(&mut sink).unwrap_or(0) > 0 {}
                }
            }
        });
        Ok(Spawned {
            input: Box::new(host),
            output: Box::new(host_reader),
            handle: Box::new(handle),
        })
    }
}

fn quick() -> LinkConfig {
    LinkConfig {
        exchange: Duration::from_millis(300),
        max_restarts: 2,
        ..LinkConfig::default()
    }
}

fn text_of(repaint: &[u8], rows: usize, cols: usize) -> Vec<String> {
    let mut t = Terminal::new(rows, cols, Limits::default());
    t.feed(repaint);
    t.text()
}

const SCREEN: &[u8] = b"\x1b[2J\x1b[Hone\r\ntwo \x1b[1;31mred\x1b[0m\r\nthree";

#[test]
fn a_link_feeds_the_ring_and_a_snapshot_redraws_the_screen_exactly() {
    let source = Bytes::new();
    let spawner = Scripted::new(vec![]);
    let link = ScreenLink::start(spawner.clone(), source.clone(), (6, 20), quick());
    source.push(SCREEN);
    link.wake();
    let snap = link.snapshot(100, Duration::from_secs(5)).unwrap();
    assert_eq!(
        text_of(&snap.body.repaint, 6, 20)[..3],
        ["one", "two red", "three"]
    );
    assert_eq!(snap.status.health, Health::Healthy);
    assert_eq!(snap.status.fidelity, Fidelity::Exact);
    assert_eq!(snap.status.fed, SCREEN.len() as u64);
    assert_eq!(snap.status.restarts, 0);
    assert_eq!(snap.status.sandbox, Some((layer::ALL, 6)));
    assert_eq!(spawner.spawned.load(Ordering::SeqCst), 1);
}

#[test]
fn a_helper_that_dies_is_replaced_and_the_history_replayed_into_the_new_one_only() {
    let source = Bytes::new();
    // The first helper's input ends after its hello and part of the stream.
    let spawner = Scripted::new(vec![Ok(Behavior::EndsAfter(60)), Ok(Behavior::Normal)]);
    let link = ScreenLink::start(spawner.clone(), source.clone(), (6, 20), quick());
    source.push(SCREEN);
    source.push(b"\r\nfour");
    let snap = link.snapshot(100, Duration::from_secs(10)).unwrap();
    assert_eq!(
        text_of(&snap.body.repaint, 6, 20)[..4],
        ["one", "two red", "three", "four"]
    );
    assert_eq!(spawner.spawned.load(Ordering::SeqCst), 2);
    assert_eq!(snap.status.restarts, 1);
    assert!(snap.status.last_failure.is_some());
    // The ring started at 0, so nothing is missing from the rebuilt screen.
    assert_eq!(snap.status.fidelity, Fidelity::Exact);
}

#[test]
fn a_ring_that_has_forgotten_its_start_makes_a_rebuilt_screen_say_so() {
    let source = Bytes::new();
    let spawner = Scripted::new(vec![Ok(Behavior::EndsAfter(60)), Ok(Behavior::Normal)]);
    let link = ScreenLink::start(spawner, source.clone(), (6, 20), quick());
    source.push(SCREEN);
    source.push(b"\r\nfour\r\nfive");
    source.evict_to(3);
    let snap = link.snapshot(100, Duration::from_secs(10)).unwrap();
    assert_eq!(snap.status.fidelity, Fidelity::Tail { missing: 3 });
    assert_eq!(snap.status.restarts, 1);
}

#[test]
fn a_helper_that_stops_answering_is_killed_at_the_deadline_and_replaced() {
    let source = Bytes::new();
    let spawner = Scripted::new(vec![Ok(Behavior::Hangs), Ok(Behavior::Normal)]);
    let link = ScreenLink::start(spawner.clone(), source.clone(), (6, 20), quick());
    source.push(SCREEN);
    let started = Instant::now();
    let snap = link.snapshot(100, Duration::from_secs(10)).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(
        spawner.killed.load(Ordering::SeqCst),
        "the stuck helper was ended through its handle"
    );
    assert_eq!(snap.status.last_failure, Some("timeout"));
    assert_eq!(text_of(&snap.body.repaint, 6, 20)[0], "one");
}

#[test]
fn a_helper_that_answers_with_garbage_is_replaced() {
    let source = Bytes::new();
    let spawner = Scripted::new(vec![Ok(Behavior::Garbles), Ok(Behavior::Normal)]);
    let link = ScreenLink::start(spawner, source.clone(), (6, 20), quick());
    source.push(SCREEN);
    let snap = link.snapshot(100, Duration::from_secs(10)).unwrap();
    assert_eq!(snap.status.last_failure, Some("bad_frame_or_exit"));
    assert_eq!(snap.status.health, Health::Healthy);
}

#[test]
fn a_helper_without_the_required_isolation_is_not_used() {
    let source = Bytes::new();
    let spawner = Scripted::new(vec![
        Ok(Behavior::Weak),
        Ok(Behavior::Weak),
        Ok(Behavior::Weak),
        Ok(Behavior::Weak),
    ]);
    let link = ScreenLink::start(spawner, source.clone(), (6, 20), quick());
    source.push(SCREEN);
    // Every helper it can get is too weak: it ends with no screen, never with a degraded one.
    let result = link.snapshot(100, Duration::from_secs(10));
    assert!(
        matches!(result, Err(LinkError::Unavailable(_))),
        "{:?}",
        result.map(|s| s.status)
    );
    let status = link.status();
    assert_ne!(status.health, Health::Healthy);
    assert_eq!(status.fidelity, Fidelity::Unavailable);
    assert_eq!(status.sandbox, None);
    assert_eq!(status.last_failure, Some("refused"));
}

#[test]
fn a_helper_that_keeps_dying_exhausts_the_restart_budget_and_the_link_stays_quiet() {
    let source = Bytes::new();
    let script = (0..10).map(|_| Ok(Behavior::EndsAfter(8))).collect();
    let spawner = Scripted::new(script);
    let link = ScreenLink::start(spawner.clone(), source.clone(), (6, 20), quick());
    source.push(SCREEN);
    for _ in 0..3 {
        let _ = link.snapshot(10, Duration::from_secs(10));
    }
    assert_eq!(link.status().health, Health::Unavailable("restart_budget"));
    let spawned = spawner.spawned.load(Ordering::SeqCst);
    assert!(spawned <= 4, "{spawned} starts for a budget of 2 restarts");
    // Further calls neither start anything nor hang.
    link.wake();
    assert!(link.snapshot(10, Duration::from_secs(2)).is_err());
    assert_eq!(spawner.spawned.load(Ordering::SeqCst), spawned);
}

#[test]
fn a_helper_that_cannot_be_started_is_unavailable_not_retried_in_a_loop() {
    let source = Bytes::new();
    let spawner = Scripted::new(vec![Err(())]);
    let link = ScreenLink::start(spawner.clone(), source, (6, 20), quick());
    assert!(link.snapshot(10, Duration::from_secs(5)).is_err());
    assert_eq!(link.status().health, Health::Unavailable("spawn_failed"));
    assert_eq!(spawner.spawned.load(Ordering::SeqCst), 1);
}

#[test]
fn a_ring_that_moved_past_what_the_helper_had_is_a_skip_with_a_stated_gap_not_a_restart() {
    let source = Bytes::new();
    let spawner = Scripted::new(vec![]);
    let link = ScreenLink::start(spawner.clone(), source.clone(), (6, 20), quick());
    source.push(b"early output\r\n");
    link.wake();
    let first = link.snapshot(10, Duration::from_secs(5)).unwrap();
    assert_eq!(first.status.fidelity, Fidelity::Exact);
    let end = source.end();
    // More output arrives and the ring forgets bytes the helper has not been given.
    source.push(b"gap gap gap\r\nlate output");
    source.evict_to(end + 4);
    let second = link.snapshot(10, Duration::from_secs(10)).unwrap();
    // No restart: the helper goes on from the oldest byte there is, and the gap is stated.
    assert_eq!(second.status.fidelity, Fidelity::Gap { skipped: 4 });
    assert_eq!(second.status.restarts, 0);
    assert_eq!(second.body.skipped, 4);
    assert_eq!(second.status.last_failure, None);
    assert_eq!(spawner.spawned.load(Ordering::SeqCst), 1);
    assert!(
        text_of(&second.body.repaint, 6, 20)
            .join("\n")
            .contains("late output")
    );
}

#[test]
fn a_resize_reaches_the_screen_and_dropping_the_link_ends_the_helper() {
    let source = Bytes::new();
    let spawner = Scripted::new(vec![]);
    let link = ScreenLink::start(spawner.clone(), source.clone(), (6, 20), quick());
    source.push(SCREEN);
    link.resize(8, 30);
    let snap = link.snapshot(10, Duration::from_secs(5)).unwrap();
    assert_eq!((snap.body.rows, snap.body.cols), (8, 30));
    drop(link);
    assert!(
        spawner.killed.load(Ordering::SeqCst),
        "the helper is ended with the link"
    );
}

#[test]
fn a_flood_of_wakes_does_not_block_the_caller() {
    let source = Bytes::new();
    let link = ScreenLink::start(
        Scripted::new(vec![Ok(Behavior::Hangs)]),
        source.clone(),
        (6, 20),
        quick(),
    );
    source.push(SCREEN);
    let started = Instant::now();
    for _ in 0..100_000 {
        link.wake();
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the pump thread was held up"
    );
}

#[test]
fn a_link_that_gave_up_tries_again_after_the_restart_window() {
    let source = Bytes::new();
    // Three helpers that fail to start properly use the whole budget (two restarts); the fourth,
    // after the window, is a good one.
    let mut script: Vec<Result<Behavior, ()>> =
        (0..3).map(|_| Ok(Behavior::EndsAfter(8))).collect();
    script.push(Ok(Behavior::Normal));
    let spawner = Scripted::new(script);
    let config = LinkConfig {
        restart_window: Duration::from_millis(600),
        ..quick()
    };
    let link = ScreenLink::start(spawner, source.clone(), (6, 20), config);
    source.push(SCREEN);
    for _ in 0..3 {
        let _ = link.snapshot(10, Duration::from_secs(10));
    }
    assert_eq!(link.status().health, Health::Unavailable("restart_budget"));
    std::thread::sleep(Duration::from_millis(900));
    // Anything that asks for the screen again finds a helper, and the terminal's history intact.
    let mut tried = None;
    for _ in 0..6 {
        tried = link.snapshot(10, Duration::from_secs(10)).ok();
        if tried.is_some() {
            break;
        }
    }
    let snap = tried.unwrap_or_else(|| panic!("the link recovered: {:?}", link.status()));
    assert_eq!(snap.status.health, Health::Healthy);
    assert_eq!(text_of(&snap.body.repaint, 6, 20)[0], "one");
}

#[test]
fn a_helper_that_ends_while_the_link_is_idle_is_replaced_without_being_asked() {
    let source = Bytes::new();
    // The first helper's input ends exactly after its hello and the first bytes, once answered.
    let hello = 4 + Request::Hello {
        version: PROTOCOL,
        rows: 6,
        cols: 20,
        scrollback_lines: 2000,
        width_table: 0,
        require: layer::ALL,
    }
    .encode()
    .len();
    let feed = 4 + Request::Feed {
        offset: 0,
        bytes: SCREEN.to_vec(),
    }
    .encode()
    .len();
    let spawner = Scripted::new(vec![
        Ok(Behavior::EndsAfter(hello + feed)),
        Ok(Behavior::Normal),
    ]);
    let link = ScreenLink::start(spawner.clone(), source.clone(), (6, 20), quick());
    // Nobody asks for anything after the first bytes.
    source.push(SCREEN);
    link.wake();
    let deadline = Instant::now() + Duration::from_secs(10);
    while (spawner.spawned.load(Ordering::SeqCst) < 2 || link.status().health != Health::Healthy)
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    let status = link.status();
    assert_eq!(spawner.spawned.load(Ordering::SeqCst), 2);
    assert_eq!(
        (status.health, status.fidelity),
        (Health::Healthy, Fidelity::Exact)
    );
    assert_eq!(status.fed, SCREEN.len() as u64);
}

#[test]
fn a_snapshot_is_not_refused_as_busy_while_the_terminal_floods_the_link_with_wakes() {
    let source = Bytes::new();
    let link = Arc::new(ScreenLink::start(
        Scripted::new(vec![]),
        source.clone(),
        (6, 20),
        quick(),
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let flood = {
        let (link, source, stop) = (link.clone(), source.clone(), stop.clone());
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                source.push(b"flooding output line\r\n");
                for _ in 0..50 {
                    link.wake();
                }
            }
        })
    };
    for _ in 0..20 {
        let snap = link.snapshot(10, Duration::from_secs(10));
        assert!(snap.is_ok(), "{:?}", snap.err());
    }
    stop.store(true, Ordering::SeqCst);
    flood.join().unwrap();
}
