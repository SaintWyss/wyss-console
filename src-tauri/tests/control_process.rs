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
