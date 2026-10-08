#![cfg(target_os = "linux")]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt},
        net::UnixStream,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use wyss_console::{
    contracts::{
        WireUuid,
        agent_adapter::{AgentAdapterId, AgentRuntimeKind},
        control::*,
    },
    control::client::ControlClient,
};
fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}
fn prep(intent: WireUuid) -> Prepare {
    Prepare {
        intent_id: intent,
        adapter_id: AgentAdapterId::parse("codex").unwrap(),
        runtime_kind: AgentRuntimeKind::Codex,
    }
}
struct Sandbox {
    root: PathBuf,
}
impl Sandbox {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("wyss-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        for d in ["run", "state"] {
            fs::DirBuilder::new().mode(0o700).create(p.join(d)).unwrap();
        }
        Self { root: p }
    }
    fn run(&self) -> PathBuf {
        self.root.join("run")
    }
    fn socket(&self) -> PathBuf {
        self.run().join("wyss-console/control.sock")
    }
    fn start(&self) -> Authority {
        let child = Command::new(env!("CARGO_BIN_EXE_wyss-control"))
            .env("XDG_RUNTIME_DIR", self.run())
            .env("XDG_STATE_HOME", self.root.join("state"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Authority { child }
    }
    fn client(&self) -> ControlClient {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            match ControlClient::connect_in_runtime(&self.run()) {
                Ok(c) => return c,
                Err(e) => {
                    assert!(Instant::now() < end, "client startup: {e}");
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct Authority {
    child: Child,
}
impl Authority {
    fn pid(&self) -> u32 {
        self.child.id()
    }
    fn stop(&mut self, hard: bool) {
        if hard {
            self.child.kill().unwrap();
        } else {
            kill(Pid::from_raw(self.pid() as i32), Signal::SIGTERM).unwrap();
        }
        let end = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                if !hard {
                    assert!(status.success(), "authority exit: {status}");
                }
                break;
            }
            assert!(Instant::now() < end, "shutdown deadline");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!PathBuf::from(format!("/proc/{}", self.pid())).exists());
    }
}
impl Drop for Authority {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn raw_send(stream: &mut UnixStream, bytes: &[u8]) {
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(bytes).unwrap();
}
fn raw_read(stream: &mut UnixStream) -> Response {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut n = [0; 4];
    stream.read_exact(&mut n).unwrap();
    let n = u32::from_be_bytes(n) as usize;
    assert!(n <= MAX_FRAME);
    let mut b = vec![0; n];
    stream.read_exact(&mut b).unwrap();
    Response::parse(&b).unwrap()
}
#[test]
fn live_disconnect_idempotency_close_and_restart_acceptance() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut client_a = env.client();
    let first_instance = client_a.hello_status().authority_instance_id.clone();
    let original = client_a.status().unwrap();
    assert_eq!(original.last_sequence, None);
    let intent = id();
    let prepared = client_a.prepare(prep(intent.clone())).unwrap();
    assert_eq!(prepared.revision, 1);
    assert_eq!(
        client_a.get(prepared.managed_session_id.clone()).unwrap(),
        prepared
    );
    let head = client_a.status().unwrap();
    assert_eq!(head.last_sequence.unwrap().get(), 1);
    let ResultValue::Audit { events, .. } = client_a
        .audit(AuditRead {
            stream_id: head.stream_id.clone(),
            after_sequence: None,
            limit: 50,
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(events.len(), 1);
    drop(client_a);
    assert!(a.child.try_wait().unwrap().is_none());
    let mut b = env.client();
    let after = b.status().unwrap();
    assert_eq!(after.last_sequence, head.last_sequence);
    assert_eq!(
        b.get(prepared.managed_session_id.clone()).unwrap(),
        prepared
    );
    assert_eq!(b.prepare(prep(intent)).unwrap(), prepared);
    assert_eq!(b.status().unwrap().last_sequence, head.last_sequence);
    let close = Close {
        intent_id: id(),
        managed_session_id: prepared.managed_session_id.clone(),
        expected_revision: 1,
    };
    let closed = b.close(close.clone()).unwrap();
    assert_eq!(closed.control_state, ControlState::Closed);
    assert_eq!(closed.revision, 2);
    assert_eq!(b.close(close).unwrap(), closed);
    assert_eq!(b.status().unwrap().last_sequence.unwrap().get(), 2);
    let pid_a = a.pid();
    drop(b);
    a.stop(false);
    assert!(!env.socket().exists());
    let mut authority_b = env.start();
    let mut c = env.client();
    assert_ne!(c.hello_status().authority_instance_id, first_instance);
    assert_eq!(c.hello_status().stream_id, head.stream_id);
    assert_eq!(c.get(closed.managed_session_id.clone()).unwrap(), closed);
    let ResultValue::Audit { events, .. } = c
        .audit(AuditRead {
            stream_id: head.stream_id.clone(),
            after_sequence: None,
            limit: 50,
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(events.len(), 2);
    let next = c.prepare(prep(id())).unwrap();
    assert_eq!(next.created_sequence.get(), 3);
    let pid_b = authority_b.pid();
    drop(c);
    authority_b.stop(false);
    println!(
        "{}",
        serde_json::json!({"authority_a":pid_a,"authority_b":pid_b,"managed_session_id":closed.managed_session_id,"revision":closed.revision,"stream_id":head.stream_id,"sequences":[1,2,3],"disconnect_state_unchanged":true,"prepare_retry_same_result":true,"restart_recovered":true,"both_reaped":true})
    );
}
#[test]
fn hard_kill_recovers_committed_state_and_stale_socket() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut c = env.client();
    let p = prep(id());
    let s = c.prepare(p.clone()).unwrap();
    let stream = c.hello_status().stream_id.clone();
    drop(c);
    a.stop(true);
    assert!(env.socket().exists());
    let mut b = env.start();
    let mut c = env.client();
    assert_eq!(c.get(s.managed_session_id.clone()).unwrap(), s);
    assert_eq!(c.prepare(p).unwrap(), s);
    assert_eq!(c.hello_status().stream_id, stream);
    drop(c);
    b.stop(false);
    assert!(!env.socket().exists());
}
#[test]
fn response_loss_retry_does_not_allocate_twice() {
    let env = Sandbox::new();
    let mut a = env.start();
    drop(env.client());
    let mut raw = UnixStream::connect(env.socket()).unwrap();
    let hello = Request {
        request_id: id(),
        operation: Operation::Hello(Hello {
            protocol_version: 1,
            client_instance_id: id(),
        }),
    };
    raw_send(&mut raw, &hello.encode().unwrap());
    raw_read(&mut raw).outcome.unwrap();
    let p = prep(id());
    let r = Request {
        request_id: id(),
        operation: Operation::Prepare(p.clone()),
    };
    raw_send(&mut raw, &r.encode().unwrap());
    drop(raw);
    let mut c = env.client();
    let s = c.prepare(p.clone()).unwrap();
    assert_eq!(c.prepare(p).unwrap(), s);
    let status = c.status().unwrap();
    assert_eq!(status.prepared_count, 1);
    assert_eq!(status.last_sequence.unwrap().get(), 1);
    drop(c);
    a.stop(false);
}
#[test]
fn concurrent_clients_have_unique_ids_and_sequences() {
    let env = Sandbox::new();
    let mut a = env.start();
    drop(env.client());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let mut jobs = vec![];
    for _ in 0..4 {
        let root = env.run();
        let b = barrier.clone();
        jobs.push(thread::spawn(move || {
            let mut c = ControlClient::connect_in_runtime(&root).unwrap();
            b.wait();
            c.prepare(prep(id())).unwrap()
        }));
    }
    let mut ids = vec![];
    let mut seq = vec![];
    for j in jobs {
        let s = j.join().unwrap();
        ids.push(s.managed_session_id.as_str().to_owned());
        seq.push(s.created_sequence.get());
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 4);
    seq.sort();
    assert_eq!(seq, [1, 2, 3, 4]);
    a.stop(false);
}
#[test]
fn hello_required_and_exact_response_id() {
    let env = Sandbox::new();
    let mut a = env.start();
    drop(env.client());
    let mut stream = UnixStream::connect(env.socket()).unwrap();
    let r = Request {
        request_id: id(),
        operation: Operation::Prepare(prep(id())),
    };
    raw_send(&mut stream, &r.encode().unwrap());
    let reply = raw_read(&mut stream);
    assert_eq!(reply.request_id, r.request_id);
    assert!(matches!(
        reply.outcome,
        Err(ControlError::HandshakeRequired)
    ));
    assert_eq!(env.client().status().unwrap().prepared_count, 0);
    drop(stream);
    a.stop(false);
}
#[test]
fn bad_clients_do_not_kill_authority() {
    let env = Sandbox::new();
    let mut a = env.start();
    drop(env.client());
    for bytes in [
        b"{bad}".as_slice(),
        b"\xff",
        b"{\"request_id\":1,\"request_id\":2}",
    ] {
        let mut s = UnixStream::connect(env.socket()).unwrap();
        raw_send(&mut s, bytes);
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        assert_eq!(s.read(&mut [0]).unwrap(), 0);
        assert_eq!(env.client().status().unwrap().prepared_count, 0);
    }
    for header in [0u32, MAX_FRAME as u32 + 1] {
        let mut s = UnixStream::connect(env.socket()).unwrap();
        s.write_all(&header.to_be_bytes()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        assert_eq!(s.read(&mut [0]).unwrap(), 0);
    }
    let mut c = env.client();
    assert_eq!(c.status().unwrap().last_sequence, None);
    drop(c);
    a.stop(false);
}
#[test]
fn slow_partial_hello_times_out() {
    let env = Sandbox::new();
    let mut a = env.start();
    drop(env.client());
    let mut s = UnixStream::connect(env.socket()).unwrap();
    s.write_all(&[0, 0, 0, 50, b'{']).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let started = Instant::now();
    assert_eq!(s.read(&mut [0]).unwrap(), 0);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(a.child.try_wait().unwrap().is_none());
    a.stop(false);
}
#[test]
fn client_capacity_is_bounded_and_recovers() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut clients = vec![env.client()];
    for _ in 1..8 {
        clients.push(ControlClient::connect_in_runtime(&env.run()).unwrap());
    }
    assert!(ControlClient::connect_in_runtime(&env.run()).is_err());
    drop(clients);
    let mut c = env.client();
    assert_eq!(c.status().unwrap().prepared_count, 0);
    drop(c);
    a.stop(false);
}
#[test]
fn second_authority_fails_without_replacing_socket() {
    let env = Sandbox::new();
    let mut a = env.start();
    drop(env.client());
    let inode = fs::metadata(env.socket()).unwrap().ino();
    let mut second = env.start();
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = second.child.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(5));
    }
    let mut error = String::new();
    second
        .child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut error)
        .unwrap();
    assert!(error.contains("already_running"));
    assert_eq!(fs::metadata(env.socket()).unwrap().ino(), inode);
    assert!(a.child.try_wait().unwrap().is_none());
    a.stop(false);
}
#[test]
fn shutdown_with_partial_clients_is_bounded() {
    let env = Sandbox::new();
    let mut a = env.start();
    drop(env.client());
    let mut clients = vec![];
    for _ in 0..4 {
        let mut s = UnixStream::connect(env.socket()).unwrap();
        s.write_all(&[0, 0, 0, 50, b'{']).unwrap();
        clients.push(s);
    }
    a.stop(false);
    assert!(!env.socket().exists());
}
#[test]
fn authority_has_no_runtime_spawn_or_tauri_coupling() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for name in [
        "mod.rs",
        "client.rs",
        "server.rs",
        "store.rs",
        "paths.rs",
        "framing.rs",
    ] {
        let source = fs::read_to_string(root.join("src/control").join(name)).unwrap();
        for forbidden in [
            "Command::new",
            "std::process",
            "adapters::codex",
            "tauri::",
            "TcpListener",
            "TcpStream",
        ] {
            assert!(!source.contains(forbidden), "{name}: {forbidden}");
        }
    }
    let desktop = fs::read_to_string(root.join("src/desktop.rs")).unwrap();
    assert!(!desktop.contains("control::"));
}

// Durable event replay and live watch, against the real authority process.

fn seq(n: u64) -> Option<wyss_console::contracts::Sequence> {
    (n > 0).then(|| n.to_string().parse().unwrap())
}
fn events(r: ResultValue) -> (Vec<serde_json::Value>, Option<u64>) {
    let ResultValue::Audit { events, next } = r else {
        panic!("audit expected")
    };
    (
        events
            .iter()
            .map(|e| serde_json::to_value(e).unwrap())
            .collect(),
        next.map(|n| n.get()),
    )
}
fn sequences(events: &[serde_json::Value]) -> Vec<u64> {
    events
        .iter()
        .map(|e| e["sequence"].as_str().unwrap().parse().unwrap())
        .collect()
}
fn page(c: &mut ControlClient, after: u64, limit: u16) -> (Vec<serde_json::Value>, Option<u64>) {
    let stream_id = c.hello_status().stream_id.clone();
    events(
        c.audit(AuditRead {
            stream_id,
            after_sequence: seq(after),
            limit,
        })
        .unwrap(),
    )
}
fn watch(
    c: &mut ControlClient,
    after: u64,
    wait_ms: u32,
) -> ControlResult<(Vec<serde_json::Value>, Option<u64>)> {
    let stream_id = c.hello_status().stream_id.clone();
    c.audit_watch(AuditWatch {
        stream_id,
        after_sequence: seq(after),
        limit: 50,
        wait_ms,
    })
    .map(events)
}
/// The committed history after `after`, through the public replay contract only.
fn history(c: &mut ControlClient, mut after: u64, limit: u16) -> Vec<serde_json::Value> {
    let mut all = Vec::new();
    loop {
        let (events, next) = page(c, after, limit);
        for n in sequences(&events) {
            after += 1;
            assert_eq!(n, after, "contiguous replay");
        }
        all.extend(events);
        match next {
            Some(n) => assert_eq!(n, after, "exact continuation cursor"),
            None => return all,
        }
    }
}
fn head(c: &mut ControlClient) -> u64 {
    c.status().unwrap().last_sequence.map_or(0, |s| s.get())
}
fn raw_hello(env: &Sandbox) -> UnixStream {
    let mut raw = UnixStream::connect(env.socket()).unwrap();
    let hello = Request {
        request_id: id(),
        operation: Operation::Hello(Hello {
            protocol_version: 1,
            client_instance_id: id(),
        }),
    };
    raw_send(&mut raw, &hello.encode().unwrap());
    raw_read(&mut raw).outcome.unwrap();
    raw
}
fn raw_watch(env: &Sandbox, stream_id: &WireUuid, after: u64, wait_ms: u32) -> UnixStream {
    let mut raw = raw_hello(env);
    let request = Request {
        request_id: id(),
        operation: Operation::AuditWatch(AuditWatch {
            stream_id: stream_id.clone(),
            after_sequence: seq(after),
            limit: 50,
            wait_ms,
        }),
    };
    raw_send(&mut raw, &request.encode().unwrap());
    raw
}
type Seen = (Vec<serde_json::Value>, Option<u64>);
/// A watcher on its own connection. A typed capacity refusal is retried, as a client
/// would; the time reported is that of the attempt that was finally admitted.
fn park(
    env: &Sandbox,
    after: u64,
    wait_ms: u32,
) -> thread::JoinHandle<(ControlResult<Seen>, Duration)> {
    let mut w = env.client();
    thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let started = Instant::now();
            match watch(&mut w, after, wait_ms) {
                Err(ControlError::CapacityExceeded) if Instant::now() < end => {
                    thread::sleep(Duration::from_millis(2))
                }
                outcome => return (outcome, started.elapsed()),
            }
        }
    })
}
/// Blocks until every watcher slot is held by someone else, observed only through the
/// public contract: a further waiting watch is refused exactly then. The probe holds a
/// slot for a millisecond at a time, which is why real watchers retry a refusal.
fn until_parked(env: &Sandbox, after: u64) {
    let mut probe = env.client();
    let end = Instant::now() + Duration::from_secs(5);
    while watch(&mut probe, after, 1) != Err(ControlError::CapacityExceeded) {
        assert!(Instant::now() < end, "watchers never parked");
    }
}
/// Four raw connections parked in a watch, to be dropped without ceremony. A parked
/// watch is silent, so the only frame that can be waiting on one is a refusal caused by
/// the probe, and that one is sent again.
fn raw_parked(env: &Sandbox, stream_id: &WireUuid, after: u64) -> Vec<UnixStream> {
    let mut raws: Vec<UnixStream> = (0..4)
        .map(|_| raw_watch(env, stream_id, after, 30_000))
        .collect();
    let mut probe = env.client();
    let end = Instant::now() + Duration::from_secs(5);
    while watch(&mut probe, after, 1) != Err(ControlError::CapacityExceeded) {
        assert!(Instant::now() < end, "raw watchers never parked");
        for raw in &mut raws {
            raw.set_read_timeout(Some(Duration::from_millis(1)))
                .unwrap();
            let mut header = [0u8; 4];
            let Ok(read) = raw.read(&mut header) else {
                continue;
            };
            raw.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            raw.read_exact(&mut header[read..]).unwrap();
            let mut body = vec![0; u32::from_be_bytes(header) as usize];
            raw.read_exact(&mut body).unwrap();
            assert_eq!(
                Response::parse(&body).unwrap().outcome.map(events),
                Err(ControlError::CapacityExceeded)
            );
            let again = Request {
                request_id: id(),
                operation: Operation::AuditWatch(AuditWatch {
                    stream_id: stream_id.clone(),
                    after_sequence: seq(after),
                    limit: 50,
                    wait_ms: 30_000,
                }),
            };
            raw_send(raw, &again.encode().unwrap());
        }
    }
    raws
}
#[test]
fn watch_returns_history_at_once_and_wakes_every_watcher_for_one_commit() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut c = env.client();
    for _ in 0..3 {
        c.prepare(prep(id())).unwrap();
    }
    // History after the cursor is returned immediately, in order, however long the wait.
    let started = Instant::now();
    let (behind, next) = watch(&mut c, 1, 30_000).unwrap();
    assert_eq!((sequences(&behind), next), (vec![2, 3], None));
    // Caught up with no wait: an empty answer, immediately.
    assert_eq!(watch(&mut c, 3, 0).unwrap(), (vec![], None));
    assert!(started.elapsed() < Duration::from_secs(2));
    // Caught up with a wait and no commit: empty, after the wait.
    let started = Instant::now();
    assert_eq!(watch(&mut c, 3, 300).unwrap(), (vec![], None));
    assert!(started.elapsed() >= Duration::from_millis(280));
    // Four watchers at the same cursor; one commit; each sees it on its own.
    let watchers: Vec<_> = (0..4).map(|_| park(&env, 3, 20_000)).collect();
    until_parked(&env, 3);
    let committed = c.prepare(prep(id())).unwrap();
    assert_eq!(committed.created_sequence.get(), 4);
    for w in watchers {
        let (outcome, waited) = w.join().unwrap();
        let (seen, next) = outcome.unwrap();
        assert_eq!((sequences(&seen), next), (vec![4], None));
        assert_eq!(seen[0]["session_id"], committed.managed_session_id.as_str());
        assert!(waited < Duration::from_secs(5), "woken by the commit");
    }
    // Nothing was consumed: the same cursor still yields the same event, to anyone.
    let again = watch(&mut env.client(), 3, 0).unwrap();
    assert_eq!(sequences(&again.0), vec![4]);
    assert_eq!(history(&mut c, 0, 1).len(), 4);
    // The watch obeys the cursor rules of the read it wraps.
    assert_eq!(watch(&mut c, 9, 1_000), Err(ControlError::InvalidRequest));
    let foreign = c.audit_watch(AuditWatch {
        stream_id: id(),
        after_sequence: None,
        limit: 1,
        wait_ms: 0,
    });
    assert_eq!(foreign.map(events), Err(ControlError::InvalidRequest));
    for (limit, wait_ms) in [(0, 0), (51, 0), (1, 30_001)] {
        let stream_id = c.hello_status().stream_id.clone();
        let refused = c.audit_watch(AuditWatch {
            stream_id,
            after_sequence: None,
            limit,
            wait_ms,
        });
        assert_eq!(refused.map(events), Err(ControlError::InvalidRequest));
        c = env.client();
    }
    drop(c);
    a.stop(false);
}
#[test]
fn racing_watch_registration_against_commits_never_loses_an_event() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut writer = env.client();
    let stream_id = writer.hello_status().stream_id.clone();
    let mut cursors = [0u64; 2];
    let mut watchers = [env.client(), env.client()];
    for round in 0..150u64 {
        let committed = round + 1;
        let handles: Vec<_> = watchers
            .into_iter()
            .zip(cursors)
            .map(|(mut w, cursor)| {
                let stream_id = stream_id.clone();
                thread::spawn(move || {
                    let started = Instant::now();
                    let seen = events(
                        w.audit_watch(AuditWatch {
                            stream_id,
                            after_sequence: seq(cursor),
                            limit: 50,
                            wait_ms: 20_000,
                        })
                        .unwrap(),
                    );
                    (w, seen, started.elapsed())
                })
            })
            .collect();
        // The commit lands before, during or after registration, round by round.
        if round % 4 != 0 {
            thread::sleep(Duration::from_micros(round % 7 * 150));
        }
        assert_eq!(
            writer.prepare(prep(id())).unwrap().created_sequence.get(),
            committed
        );
        let mut back = Vec::new();
        for (i, h) in handles.into_iter().enumerate() {
            let (w, (seen, _), waited) = h.join().unwrap();
            assert!(
                waited < Duration::from_secs(10),
                "round {round}: a lost wakeup would wait the whole timeout"
            );
            let got = sequences(&seen);
            assert_eq!(got.first(), Some(&(cursors[i] + 1)), "round {round}");
            assert_eq!(got.last(), Some(&committed), "round {round}");
            cursors[i] = committed;
            back.push(w);
        }
        watchers = [back.remove(0), back.remove(0)];
    }
    assert_eq!(head(&mut writer), 150);
    assert_eq!(history(&mut writer, 0, 50).len(), 150);
    drop(watchers);
    drop(writer);
    a.stop(false);
}
#[test]
fn watcher_capacity_is_bounded_typed_and_never_starves_control_operations() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut c = env.client();
    let session = c.prepare(prep(id())).unwrap();
    let parked: Vec<_> = (0..4).map(|_| park(&env, 1, 25_000)).collect();
    until_parked(&env, 1);
    // A fifth waiting watch is refused at once with a typed error, and not queued.
    let started = Instant::now();
    assert_eq!(
        watch(&mut env.client(), 1, 25_000),
        Err(ControlError::CapacityExceeded)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    // Reads and non-waiting watches need no slot.
    assert_eq!(watch(&mut env.client(), 1, 0).unwrap(), (vec![], None));
    assert_eq!(page(&mut env.client(), 0, 50).0.len(), 1);
    // Ordinary control operations keep their own capacity while every slot is waiting.
    let started = Instant::now();
    let lease = c
        .lease_grant(LeaseGrant {
            intent_id: id(),
            session_id: session.managed_session_id.clone(),
            expected_session_revision: 1,
            scopes: vec![ExecutionScope::TurnStart],
            ttl_seconds: 600,
        })
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1), "lease grant");
    for w in parked {
        let (seen, _) = w.join().unwrap().0.unwrap();
        assert_eq!(sequences(&seen), vec![2], "the grant woke every watcher");
    }
    // With the slots free again, four new watchers park and the rest still works.
    let parked: Vec<_> = (0..4).map(|_| park(&env, 2, 25_000)).collect();
    until_parked(&env, 2);
    let mut others: Vec<ControlClient> = (0..3).map(|_| env.client()).collect();
    let started = Instant::now();
    others[0].status().unwrap();
    let revoked = others[1]
        .lease_revoke(LeaseRevoke {
            intent_id: id(),
            lease_id: lease.lease_id.clone(),
            expected_revision: 1,
        })
        .unwrap();
    assert_eq!(revoked.state, LeaseState::Revoked);
    let closed = others[2]
        .close(Close {
            intent_id: id(),
            managed_session_id: session.managed_session_id.clone(),
            expected_revision: 1,
        })
        .unwrap();
    assert_eq!(closed.control_state, ControlState::Closed);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "revoke and close"
    );
    for w in parked {
        let (seen, _) = w.join().unwrap().0.unwrap();
        assert_eq!(sequences(&seen).first(), Some(&3));
    }
    assert_eq!(head(&mut c), 4);
    drop((c, others));
    a.stop(false);
}
#[test]
fn slow_and_silent_clients_never_delay_a_security_critical_mutation() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut c = env.client();
    let stream_id = c.hello_status().stream_id.clone();
    for _ in 0..60 {
        c.prepare(prep(id())).unwrap();
    }
    // Three watchers that will be woken and then never read what they are sent.
    let silent: Vec<UnixStream> = (0..3)
        .map(|_| raw_watch(&env, &stream_id, 60, 25_000))
        .collect();
    // One client that asks for maximal pages over and over and reads none of them, until
    // the authority's socket buffer is full and its writer for that client stalls.
    let mut flood = raw_hello(&env);
    flood.set_nonblocking(true).unwrap();
    let request = Request {
        request_id: id(),
        operation: Operation::AuditRead(AuditRead {
            stream_id: stream_id.clone(),
            after_sequence: None,
            limit: 50,
        }),
    }
    .encode()
    .unwrap();
    let mut frame = (request.len() as u32).to_be_bytes().to_vec();
    frame.extend(&request);
    for _ in 0..64 {
        if flood.write_all(&frame).is_err() {
            break;
        }
    }
    thread::sleep(Duration::from_millis(200));
    let session = c.prepare(prep(id())).unwrap();
    let mut slowest = Duration::ZERO;
    let mut lease = None;
    for round in 0..40u64 {
        let started = Instant::now();
        match round % 4 {
            0 => {
                lease = Some(
                    c.lease_grant(LeaseGrant {
                        intent_id: id(),
                        session_id: session.managed_session_id.clone(),
                        expected_session_revision: 1,
                        scopes: vec![ExecutionScope::TurnStart, ExecutionScope::CommandExecute],
                        ttl_seconds: 600,
                    })
                    .unwrap(),
                )
            }
            1 => {
                let l = lease.take().unwrap();
                lease = Some(
                    c.lease_renew(LeaseRenew {
                        intent_id: id(),
                        lease_id: l.lease_id,
                        expected_revision: l.revision,
                        ttl_seconds: 600,
                    })
                    .unwrap(),
                )
            }
            2 => {
                let l = lease.take().unwrap();
                c.lease_revoke(LeaseRevoke {
                    intent_id: id(),
                    lease_id: l.lease_id,
                    expected_revision: l.revision,
                })
                .unwrap();
            }
            _ => {
                c.prepare(prep(id())).unwrap();
            }
        }
        slowest = slowest.max(started.elapsed());
    }
    assert!(
        slowest < Duration::from_millis(750),
        "slowest mutation {slowest:?}"
    );
    // Nothing was lost, repeated or reordered while they stalled.
    let total = head(&mut c);
    assert_eq!(total, 101);
    assert_eq!(history(&mut c, 0, 50).len() as u64, total);
    assert_eq!(history(&mut c, 0, 7).len() as u64, total);
    drop((silent, flood));
    assert!(a.child.try_wait().unwrap().is_none());
    drop(c);
    a.stop(false);
}
#[test]
fn a_watcher_that_disconnects_changes_nothing_and_returns_its_slot() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut c = env.client();
    c.prepare(prep(id())).unwrap();
    let stream_id = c.hello_status().stream_id.clone();
    let status = serde_json::to_value(c.status().unwrap()).unwrap();
    let before = history(&mut c, 0, 50);
    let gone = raw_parked(&env, &stream_id, 1);
    drop(gone);
    // The slots come back at once, not when the thirty second wait would have ended:
    // four new watchers are all admitted within their five second retry window.
    let parked: Vec<_> = (0..4).map(|_| park(&env, 1, 20_000)).collect();
    until_parked(&env, 1);
    // No event, no state change, no cursor moved on the authority's side.
    assert_eq!(serde_json::to_value(c.status().unwrap()).unwrap(), status);
    assert_eq!(history(&mut c, 0, 50), before);
    c.prepare(prep(id())).unwrap();
    for w in parked {
        assert_eq!(sequences(&w.join().unwrap().0.unwrap().0), vec![2]);
    }
    drop(c);
    a.stop(false);
}
#[test]
fn restart_and_hard_kill_resume_from_the_clients_own_cursor() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut c = env.client();
    let stream_id = c.hello_status().stream_id.clone();
    for _ in 0..9 {
        c.prepare(prep(id())).unwrap();
    }
    let cursor = 4;
    let missed = history(&mut c, cursor, 2);
    assert_eq!(sequences(&missed), vec![5, 6, 7, 8, 9]);
    let whole = history(&mut c, 0, 50);
    // Parked watchers do not hold shutdown back for the rest of their wait.
    let parked: Vec<_> = (0..4).map(|_| park(&env, 9, 30_000)).collect();
    until_parked(&env, 9);
    drop(c);
    let started = Instant::now();
    a.stop(false);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "bounded shutdown"
    );
    for w in parked {
        let (outcome, waited) = w.join().unwrap();
        assert!(outcome.is_err(), "the watcher is cancelled, not answered");
        assert!(waited < Duration::from_secs(8));
    }
    // Graceful restart: same stream, same head, the exact missed suffix from the cursor.
    let mut b = env.start();
    let mut c = env.client();
    assert_eq!(c.hello_status().stream_id, stream_id);
    assert_eq!(head(&mut c), 9);
    assert_eq!(history(&mut c, cursor, 3), missed);
    assert_eq!(history(&mut c, 0, 50), whole);
    assert_eq!(c.prepare(prep(id())).unwrap().created_sequence.get(), 10);
    let whole = history(&mut c, 0, 50);
    // Hard kill with a watcher parked and no cleanup at all.
    let orphan = raw_watch(&env, &stream_id, 10, 30_000);
    drop(c);
    b.stop(true);
    drop(orphan);
    let mut d = env.start();
    let mut c = env.client();
    assert_eq!(c.hello_status().stream_id, stream_id);
    assert_eq!(head(&mut c), 10);
    assert_eq!(history(&mut c, 0, 4), whole);
    assert_eq!(history(&mut c, cursor, 50), whole[cursor as usize..]);
    let next = c.prepare(prep(id())).unwrap();
    assert_eq!(next.created_sequence.get(), 11);
    let tail = history(&mut c, 10, 50);
    assert_eq!(sequences(&tail), vec![11]);
    // The process identity changes across restarts; the stream does not.
    assert_ne!(
        tail[0]["source"]["instance_id"],
        whole[0]["source"]["instance_id"]
    );
    assert_eq!(tail[0]["stream_id"], whole[0]["stream_id"]);
    // A watch from the old cursor after two restarts catches up with everything missed.
    let (seen, _) = watch(&mut c, cursor, 30_000).unwrap();
    assert_eq!(sequences(&seen), vec![5, 6, 7, 8, 9, 10, 11]);
    drop(c);
    d.stop(false);
}
#[test]
fn a_failed_audit_append_in_the_running_authority_commits_nothing_and_wakes_nobody() {
    let env = Sandbox::new();
    let mut a = env.start();
    let mut c = env.client();
    let first = c.prepare(prep(id())).unwrap();
    let before = history(&mut c, 0, 50);
    // An external fault on exactly the audit insert of the live database.
    let db =
        rusqlite::Connection::open(env.root.join("state/wyss-console/control.sqlite3")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.execute_batch("CREATE TRIGGER injected BEFORE INSERT ON control_events BEGIN SELECT RAISE(ABORT,'injected'); END").unwrap();
    let watchers: Vec<_> = (0..4).map(|_| park(&env, 1, 1_500)).collect();
    until_parked(&env, 1);
    let refused = prep(id());
    assert_eq!(
        c.prepare(refused.clone()),
        Err(ControlError::StorageUnavailable)
    );
    assert_eq!(
        c.close(Close {
            intent_id: id(),
            managed_session_id: first.managed_session_id.clone(),
            expected_revision: 1,
        }),
        Err(ControlError::StorageUnavailable)
    );
    // Canonical mutation 0, new event 0, head unchanged.
    let status = c.status().unwrap();
    assert_eq!((status.prepared_count, status.closed_count), (1, 0));
    assert_eq!(status.last_sequence.unwrap().get(), 1);
    assert_eq!(history(&mut c, 0, 50), before);
    assert_eq!(c.get(first.managed_session_id.clone()).unwrap(), first);
    // A rolled back mutation is never announced: every watcher sleeps out its whole wait.
    for w in watchers {
        let (outcome, waited) = w.join().unwrap();
        assert!(outcome.unwrap().0.is_empty());
        assert!(waited >= Duration::from_millis(1_400), "{waited:?}");
    }
    db.execute_batch("DROP TRIGGER injected").unwrap();
    drop(db);
    // The refused intent left no receipt: retried, it commits now, at the very next
    // sequence. No gap was burned by the failures.
    let retried = c.prepare(refused).unwrap();
    assert_eq!(retried.created_sequence.get(), 2);
    assert_eq!(sequences(&history(&mut c, 0, 50)), vec![1, 2]);
    drop(c);
    a.stop(false);
    let mut b = env.start();
    let mut c = env.client();
    assert_eq!(head(&mut c), 2);
    drop(c);
    b.stop(false);
}
fn resident_kib(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap()
}
#[test]
fn sustained_mutation_with_every_watcher_slot_following_loses_and_reorders_nothing() {
    const TOTAL: u64 = 1_200;
    let env = Sandbox::new();
    let mut a = env.start();
    let mut c = env.client();
    let stream_id = c.hello_status().stream_id.clone();
    // Three followers take watcher slots for the whole run, each with its own cursor; a
    // fourth slot goes to a client that is woken once and never reads again.
    let followers: Vec<_> = (0..3)
        .map(|_| {
            let mut w = env.client();
            thread::spawn(move || {
                let (mut cursor, mut pages, mut refused) = (0u64, 0u64, 0u64);
                let end = Instant::now() + Duration::from_secs(60);
                while cursor < TOTAL {
                    assert!(Instant::now() < end, "follower stalled at {cursor}");
                    match watch(&mut w, cursor, 2_000) {
                        Ok((seen, _)) => {
                            for n in sequences(&seen) {
                                // No loss, no duplicate, no inversion: exactly the next one.
                                assert_eq!(n, cursor + 1);
                                cursor = n;
                            }
                            pages += 1;
                        }
                        Err(ControlError::CapacityExceeded) => {
                            refused += 1;
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(e) => panic!("follower: {e}"),
                    }
                }
                (pages, refused)
            })
        })
        .collect();
    let silent = raw_watch(&env, &stream_id, 0, 30_000);
    let session = c.prepare(prep(id())).unwrap();
    let mut lease: Option<ExecutionLease> = None;
    let mut slowest = Duration::ZERO;
    let mut warm = 0;
    while head(&mut c) < TOTAL {
        let started = Instant::now();
        match lease.take() {
            None => {
                lease = Some(
                    c.lease_grant(LeaseGrant {
                        intent_id: id(),
                        session_id: session.managed_session_id.clone(),
                        expected_session_revision: 1,
                        scopes: vec![ExecutionScope::TurnStart, ExecutionScope::FileChange],
                        ttl_seconds: 600,
                    })
                    .unwrap(),
                )
            }
            Some(l) if l.revision < 3 => {
                lease = Some(
                    c.lease_renew(LeaseRenew {
                        intent_id: id(),
                        lease_id: l.lease_id,
                        expected_revision: l.revision,
                        ttl_seconds: 600,
                    })
                    .unwrap(),
                )
            }
            Some(l) => {
                c.lease_revoke(LeaseRevoke {
                    intent_id: id(),
                    lease_id: l.lease_id,
                    expected_revision: l.revision,
                })
                .unwrap();
            }
        }
        slowest = slowest.max(started.elapsed());
        if warm == 0 && head(&mut c) >= 200 {
            warm = resident_kib(a.pid());
        }
    }
    let mut pages = 0;
    for f in followers {
        let (seen_pages, _) = f.join().unwrap();
        pages += seen_pages;
    }
    assert!(pages >= 3, "every follower read the stream to its end");
    assert!(
        slowest < Duration::from_millis(750),
        "slowest mutation {slowest:?}"
    );
    // A thousand more commits with every slot taken did not grow the authority: nothing
    // is queued per watcher and nothing is retained per event.
    let grown = resident_kib(a.pid()).saturating_sub(warm);
    assert!(
        warm > 0 && grown < 16 * 1024,
        "resident set grew {grown} KiB"
    );
    let all = history(&mut c, 0, 50);
    assert_eq!(all.len() as u64, head(&mut c));
    let mut ids: Vec<&str> = all
        .iter()
        .map(|e| e["event_id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), all.len(), "event ids are unique");
    assert!(all.iter().all(|e| e["stream_id"] == stream_id.as_str()));
    assert_eq!(history(&mut c, 0, 13), all);
    drop(silent);
    drop(c);
    a.stop(false);
}
