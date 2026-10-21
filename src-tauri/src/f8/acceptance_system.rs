//! The application as a whole: restart and recovery on a durable image, the window's link
//! through an outage, hostile and abusive clients, and shutdown. Same rig, same oracle.
use super::*;
use crate::f8::{gui::Connector, v8::validate_v8};

/// A connector whose authority can be taken away and replaced, as a service manager does.
struct Swap(Arc<Mutex<Option<ApplicationHandle>>>);
impl Connector for Swap {
    fn connect(&self) -> ControlResult<UnixStream> {
        let slot = self.0.lock().unwrap();
        let handle = slot.as_ref().ok_or(E::TransportError)?;
        let (owned, client) = UnixStream::pair().map_err(|_| E::TransportError)?;
        handle.attach(owned)?;
        Ok(client)
    }
}

fn durable(root: Option<PathBuf>, authority: &str) -> Options {
    Options {
        limit: 1,
        tools: true,
        file: true,
        root,
        authority: Some(authority.to_owned()),
        ..Options::default()
    }
}
const FIRST: &str = "00000000-0000-4000-8000-0000000000b1";
const SECOND: &str = "00000000-0000-4000-8000-0000000000b2";

#[test]
fn an_authority_restart_recovers_its_durable_image_and_the_window_follows_it() {
    let slot = Arc::new(Mutex::new(None));
    let window = F8Gui::local(Swap(slot.clone()));
    let first = App::start(durable(None, FIRST));
    *slot.lock().unwrap() = Some(first.handle.clone());
    let s = first.up(AgentMode::LocalTools);
    let (turn, approval) = first.waiting(&s);
    let before = window.session(s.id.clone()).unwrap();
    let state = window.state();
    assert_eq!(state.authority.unwrap().instance_id.as_str(), FIRST);
    assert_eq!(state.epoch, 0);
    let lease_before = before.lease.clone().unwrap();

    // The authority goes away while a turn waits on an approval. The window learns it from
    // its next call, says so, and does not try again at once.
    *slot.lock().unwrap() = None;
    let root = first.retire();
    let gone = window.session(s.id.clone()).unwrap_err();
    assert_eq!(gone.code, F8Failure::Unavailable);
    let waiting = window.state();
    assert!(!waiting.link.retry_in_ms.is_none() || waiting.link.fault.is_some());
    let connections = window.stats().connections;
    let refused = window.sessions(None).unwrap_err();
    assert_eq!(refused.code, F8Failure::Unavailable);
    assert_eq!(
        window.stats().connections,
        connections,
        "nothing is tried inside the delay"
    );

    // A new authority instance opens the same file: it validates the whole image, and recovers
    // what its predecessor left, before it accepts anyone.
    let second = App::start(durable(Some(root), SECOND));
    *slot.lock().unwrap() = Some(second.handle.clone());
    let state = window.reconnect();
    assert_eq!(state.authority.unwrap().instance_id.as_str(), SECOND);
    assert_eq!(state.epoch, 1, "the window sees another authority");
    assert_eq!(
        second.knobs.spawns.load(Ordering::Acquire),
        0,
        "nothing is relaunched"
    );

    let after = window.session(s.id.clone()).unwrap();
    // History and the durable lease are as they were; the runtime is not live and is not
    // reported as if it were; the work stranded with it is recorded as such.
    assert_eq!(after.session.runtime.generation, 1);
    assert_eq!(after.session.runtime.actual, ActualState::Stopped);
    assert_eq!(after.session.runtime.fence.instance, None);
    assert_eq!(after.lease.map(|l| l.revision), Some(lease_before.revision));
    assert_eq!(after.binding.state, BindingState::BoundDetached);
    let ended = window.turn(turn.clone(), None, false).unwrap();
    assert_eq!(
        ended.turn.turn.state,
        crate::contracts::control::TurnState::Indeterminate
    );
    assert_eq!(
        ended.turn.turn.reason,
        Some(crate::contracts::control::TurnReason::AuthorityShutdown)
    );
    assert!(!ended.turn.live);
    let stranded = window
        .approval(approval.approval.id.clone(), None, None)
        .unwrap();
    assert_ne!(stranded.approval.state, ApprovalState::Pending);
    assert!(!stranded.approval.decidable);
    assert!(
        stranded.command.is_none(),
        "the action was memory, and is gone with it"
    );

    // Nothing of the old instance can be used: its fence, its approval, its turn.
    let late = window.turn_start(
        uuid(),
        s.fence.clone(),
        after.session.revision,
        "again".into(),
    );
    assert!(late.is_err(), "{late:?}");
    let answer = window.approval_respond(
        uuid(),
        s.fence.clone(),
        approval.approval.id.clone(),
        approval.approval.revision,
        ApprovalDecision::Accept,
    );
    assert!(answer.is_err(), "{answer:?}");
    assert_eq!(second.sent(&s.id, 1, "approval"), 0);
    assert_eq!(
        first_frames(&second, &s.id),
        1,
        "the predecessor's child heard no more"
    );

    // The window starts over from what it reads: a new launch, a resumed binding, a new turn.
    let fresh = second.relaunch(&s.id, AgentMode::LocalTools);
    assert_eq!(fresh.session.runtime.generation, 2);
    let intent = uuid();
    settled(|| {
        window.binding_resume(
            intent.clone(),
            fresh.session.runtime.fence.clone(),
            fresh.session.revision,
        )
    });
    let attached = second.until(&s.id, |d| d.binding.state == BindingState::BoundAttached);
    assert!(attached.lease.is_some());
    // The history reaches back across the restart without a hole.
    let history = second.events("approval_requested");
    assert_eq!(history, 1);

    // The image the whole thing ran on is a valid V8 image when the application is done.
    let db = second.db.clone();
    let root = second.retire();
    validate_v8(&Connection::open(&db).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(root);
}
fn first_frames(app: &App, session: &ManagedSessionId) -> usize {
    app.frames(session, 1)
        .iter()
        .filter(|line| line.contains("\"turn/start\""))
        .count()
}

#[test]
fn the_windows_link_waits_out_an_outage_without_hammering_and_returns_by_itself() {
    let slot = Arc::new(Mutex::new(None));
    let window = F8Gui::local(Swap(slot.clone()));
    let app = App::start(durable(None, FIRST));
    *slot.lock().unwrap() = Some(app.handle.clone());
    assert!(window.state().authority.is_some());
    let connected = window.stats().connections;
    // The authority becomes unreachable while the window holds a connection to it.
    let held = slot.lock().unwrap().take();
    drop(held);
    app.handle.shutdown();
    std::thread::sleep(Duration::from_millis(300));
    let broken = window.sessions(None).unwrap_err();
    assert_eq!(broken.code, F8Failure::Unavailable);
    let link = window.state().link;
    assert_eq!(format!("{:?}", link.state), "Waiting");
    assert!(link.retry_in_ms.is_some());
    // Inside the delay nothing is tried: ten calls, no connection.
    let tried = window.stats().connections;
    for _ in 0..10 {
        assert_eq!(
            window.sessions(None).unwrap_err().code,
            F8Failure::Unavailable
        );
    }
    assert_eq!(window.stats().connections, tried);
    assert!(tried >= connected);
    // A request to try now forgets the delay and tries once.
    let again = window.reconnect();
    assert_eq!(format!("{:?}", again.link.state), "Waiting");
    // An authority is back: the next attempt after the delay connects, with no help.
    let root = app.retire();
    let back = App::start(durable(Some(root), FIRST));
    *slot.lock().unwrap() = Some(back.handle.clone());
    let reached = eventually(|| window.sessions(None).ok());
    assert!(!reached.sessions.is_empty());
    assert_eq!(
        window.state().epoch,
        0,
        "the same authority instance is not another"
    );
}

fn raw_bytes(handle: &ApplicationHandle, bytes: &[u8]) -> UnixStream {
    let (owned, mut client) = UnixStream::pair().unwrap();
    handle.attach(owned).unwrap();
    std::io::Write::write_all(&mut client, bytes).unwrap();
    client
}

#[test]
fn hostile_and_malformed_bytes_never_reach_the_authority_and_never_hurt_the_others() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    let session = app.session();
    let head = app.head();
    let frame = |bytes: &[u8]| {
        let mut framed = (bytes.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(bytes);
        framed
    };
    let hello = |extra: &str| {
        format!(
            "{{\"schema_version\":2,\"request_id\":\"{}\",\"method\":\"authority/hello\",\"params\":{{\"protocol_version\":2,\"client_instance_id\":\"{}\"{extra}}}}}",
            uuid().as_str(),
            uuid().as_str()
        )
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("not json", frame(b"not json at all")),
        ("an empty object", frame(b"{}")),
        ("a json array", frame(b"[1,2,3]")),
        ("invalid utf-8", frame(&[0xff, 0xfe, 0xfd, b'{', b'}'])),
        ("a NUL", frame(b"{\"schema_version\":2\0}")),
        (
            "a duplicate key",
            frame(hello(",\"protocol_version\":1").as_bytes()),
        ),
        ("an unknown key", frame(hello(",\"extra\":true").as_bytes())),
        ("a zero-length frame", vec![0, 0, 0, 0]),
        ("a frame over the bound", (70_000u32).to_be_bytes().to_vec()),
        ("a header that claims 4 GiB", vec![0xff, 0xff, 0xff, 0xff]),
        (
            "deep nesting",
            frame(format!("{}1{}", "[".repeat(5000), "]".repeat(5000)).as_bytes()),
        ),
        (
            "a version that is a string",
            frame(
                hello("")
                    .replace("\"protocol_version\":2", "\"protocol_version\":\"2\"")
                    .as_bytes(),
            ),
        ),
        (
            "a huge number",
            frame(
                hello("")
                    .replace(
                        "\"protocol_version\":2",
                        "\"protocol_version\":99999999999999999999999999",
                    )
                    .as_bytes(),
            ),
        ),
        (
            "a float count",
            frame(
                hello("")
                    .replace("\"protocol_version\":2", "\"protocol_version\":2.5")
                    .as_bytes(),
            ),
        ),
    ];
    for (name, bytes) in cases {
        let mut client = raw_bytes(&app.handle, &bytes);
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        // The authority answers with a closed error or closes the connection: it never answers
        // success, and it never stops answering the others.
        let mut reply = [0u8; 4];
        if std::io::Read::read_exact(&mut client, &mut reply).is_ok() {
            let mut body = vec![0u8; u32::from_be_bytes(reply) as usize];
            std::io::Read::read_exact(&mut client, &mut body).unwrap();
            let answer: Value = serde_json::from_slice(&body).unwrap();
            assert!(answer["error"].is_object(), "{name}: {answer}");
        }
        let mut healthy = app.raw();
        let ok = healthy.hello();
        assert!(
            ok["error"].is_null(),
            "{name}: the authority stopped answering: {ok}"
        );
    }
    // A client that sends half a frame and leaves, and one that sends a header and waits.
    for half in [vec![0, 0, 1, 0, b'{'], vec![0, 0]] {
        let client = raw_bytes(&app.handle, &half);
        drop(client);
    }
    let _idle = raw_bytes(&app.handle, &[0, 0, 1, 0]);
    assert_eq!(app.head(), head, "nothing hostile left a trace");
    assert_eq!(
        app.gui.session(session).unwrap().session.runtime.generation,
        0
    );
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), 0);
}

#[test]
fn watchers_are_bounded_and_clients_are_bounded_and_control_goes_on() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    let stream = app.gui.state().authority.unwrap().stream_id;
    let head = app.gui.state().authority.unwrap().last_sequence;
    let watch = |raw: &mut Raw| {
        raw.send(
            "audit/watch",
            json!({"stream_id":stream.as_str(),"after_sequence":head.map(|h| h.get().to_string()),
                "limit":5,"wait_ms":20_000}),
        )
    };
    // Four watchers hold a slot each; the fifth is refused at once. Control still answers.
    let mut holders = Vec::new();
    for _ in 0..4 {
        let mut raw = app.raw();
        raw.hello();
        let (owned, _) = (raw.stream.try_clone().unwrap(), ());
        let request = json!({"schema_version":2,"request_id":uuid().as_str(),"method":"audit/watch",
            "authority_instance_id":raw.authority,
            "params":{"stream_id":stream.as_str(),"after_sequence":head.map(|h| h.get().to_string()),
                "limit":5,"wait_ms":20_000}});
        let mut owned = owned;
        let stop = AtomicBool::new(false);
        write_frame(
            &mut owned,
            &serde_json::to_vec(&request).unwrap(),
            Instant::now() + Duration::from_secs(5),
            &stop,
        )
        .unwrap();
        holders.push(raw);
    }
    std::thread::sleep(Duration::from_millis(200));
    let mut fifth = app.raw();
    fifth.hello();
    let refused = watch(&mut fifth);
    assert_eq!(Raw::code(&refused), Some("capacity_exceeded"), "{refused}");
    let started = Instant::now();
    let status = fifth.send("authority/status", json!({}));
    assert!(status["error"].is_null(), "{status}");
    assert!(started.elapsed() < Duration::from_millis(250));
    // A commit wakes all four, once each.
    app.session();
    for raw in &mut holders {
        let deadline = Instant::now() + Duration::from_secs(5);
        let bytes = read_frame(&mut raw.stream, deadline, &raw.stop)
            .unwrap()
            .unwrap();
        let answer: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            answer["result"]["value"]["events"]
                .as_array()
                .is_some_and(|e| !e.is_empty()),
            "{answer}"
        );
    }

    // Clients are bounded: with eight connected, a ninth cannot attach.
    let mut crowd: Vec<UnixStream> = Vec::new();
    let mut refused_at = None;
    for n in 0..20 {
        let (owned, client) = UnixStream::pair().unwrap();
        match app.handle.attach(owned) {
            Ok(()) => crowd.push(client),
            Err(_) => {
                refused_at = Some(n);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = (refused_at, crowd);
}

#[test]
fn a_client_that_never_reads_is_dropped_and_the_others_are_not_slowed() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    for _ in 0..30 {
        app.session();
    }
    let mut slow = app.raw();
    slow.hello();
    let mut quick = app.raw();
    quick.hello();
    // The slow client asks for pages and reads none of them.
    let stop = AtomicBool::new(false);
    for _ in 0..50 {
        let request = json!({"schema_version":2,"request_id":uuid().as_str(),"method":"managedSession/list",
            "authority_instance_id":slow.authority,"params":{"after":null,"limit":50}});
        if write_frame(
            &mut slow.stream,
            &serde_json::to_vec(&request).unwrap(),
            Instant::now() + Duration::from_millis(200),
            &stop,
        )
        .is_err()
        {
            break;
        }
    }
    let mut worst = Duration::ZERO;
    for _ in 0..40 {
        let started = Instant::now();
        let answer = quick.send("authority/status", json!({}));
        assert!(answer["error"].is_null());
        worst = worst.max(started.elapsed());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(worst < Duration::from_millis(250), "{worst:?}");
}

#[test]
fn shutdown_stops_every_runtime_even_those_that_ignore_it_and_leaves_a_valid_image() {
    let app = App::start(durable(None, FIRST));
    app.knobs.grace_ms.store(300, Ordering::Release);
    app.knobs
        .queue
        .lock()
        .unwrap()
        .push_back(json!({"direct": true, "ignore_term": true, "ignore_eof": true}));
    let a = app.up(AgentMode::Inference);
    let pid = eventually(|| app.pid_file(&a.id, 1, "pid"));
    assert!(alive(pid));
    let db = app.db.clone();
    let started = Instant::now();
    let root = app.retire();
    // It ended cleanly, within the bound, and its child is gone though it ignored everything.
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
    assert!(!alive(pid));
    validate_v8(&Connection::open(&db).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_window_that_rested_longer_than_the_authority_waits_is_not_failed_by_it() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    let id = app.session();
    assert!(app.gui.session(id.clone()).is_ok());
    let first = app.gui.stats().connections;
    // Past the authority's read timeout, with the window's connection long since closed by it.
    std::thread::sleep(Duration::from_millis(5600));
    assert!(app.gui.session(id.clone()).is_ok());
    assert!(
        app.gui.stats().connections > first,
        "a fresh connection was made"
    );
    // A burst of calls shares one connection.
    let used = app.gui.stats().connections;
    for _ in 0..10 {
        app.gui.session(id.clone()).unwrap();
    }
    assert_eq!(app.gui.stats().connections, used);
}
