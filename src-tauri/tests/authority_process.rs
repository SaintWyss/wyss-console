//! The desktop's authority link against the real `wyss-control` process, in isolated
//! temporary XDG directories. No Codex is required or launched.
#![cfg(target_os = "linux")]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant},
};
use wyss_console::{
    authority::{AuthorityService, AuthorityView, Connection, Fault},
    contracts::{
        WireUuid,
        agent_adapter::{AgentAdapterId, AgentRuntimeKind},
        control::*,
    },
    control::client::ControlClient,
};

/// Descriptors and threads are counted for the whole process, so these tests run one at
/// a time: nothing else in this binary opens or closes anything meanwhile.
static SERIAL: Mutex<()> = Mutex::new(());
fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}
fn prepare(client: &mut ControlClient) -> ManagedSessionRecord {
    client
        .prepare(Prepare {
            intent_id: id(),
            adapter_id: AgentAdapterId::parse("codex").unwrap(),
            runtime_kind: AgentRuntimeKind::Codex,
        })
        .unwrap()
}

struct Sandbox {
    root: PathBuf,
}
impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for d in ["run", "state"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(d))
                .unwrap();
        }
        Self { root }
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
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Authority { child }
    }
    /// A second, independent client of the same authority.
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
    fn service(&self) -> AuthorityService {
        AuthorityService::start(Some(self.run()))
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
    fn stop(&mut self, hard: bool) {
        let pid = self.child.id();
        if hard {
            self.child.kill().unwrap();
        } else {
            kill(Pid::from_raw(pid as i32), Signal::SIGTERM).unwrap();
        }
        let end = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(hard || status.success(), "authority exit: {status}");
                break;
            }
            assert!(Instant::now() < end, "shutdown deadline");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn alive(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }
}
impl Drop for Authority {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn until(
    service: &AuthorityService,
    what: &str,
    accept: impl Fn(&AuthorityView) -> bool,
) -> AuthorityView {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut after = None;
    loop {
        let view = service.wait_changed(after, Duration::from_millis(50));
        if accept(&view) {
            return view;
        }
        assert!(Instant::now() < deadline, "{what}: last view {view:?}");
        after = Some(view.revision);
    }
}
fn head(view: &AuthorityView) -> u64 {
    view.authority
        .as_ref()
        .and_then(|a| a.last_sequence)
        .map_or(0, |s| s.get())
}
/// Connected, as restarted `restarts` times, at durable head `sequence`.
fn live(service: &AuthorityService, restarts: u64, sequence: u64) -> AuthorityView {
    until(service, "live", |v| {
        v.connection == Connection::Connected
            && head(v) == sequence
            && v.authority.as_ref().is_some_and(|a| a.restarts == restarts)
    })
}
fn gone(service: &AuthorityService) -> AuthorityView {
    let view = until(service, "gone", |v| v.connection == Connection::Waiting);
    assert_eq!((&view.authority, &view.agent), (&None, &None));
    view
}

fn descriptors() -> usize {
    fs::read_dir("/proc/self/fd").unwrap().count()
}
/// What the process holds once it is at rest. An attempt in flight owns a socket for an
/// instant, so the count is awaited: a leak never comes back to the expected number.
fn holds(descriptors_expected: usize, links: usize, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let now = (descriptors(), link_threads());
        if now == (descriptors_expected, links) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what}: holds {now:?}, expected {:?}",
            (descriptors_expected, links)
        );
        thread::sleep(Duration::from_millis(10));
    }
}
/// Threads of this process that are an authority link, by the name the service gives them.
fn link_threads() -> usize {
    fs::read_dir("/proc/self/task")
        .unwrap()
        .filter(|task| {
            let comm = task.as_ref().unwrap().path().join("comm");
            fs::read_to_string(comm).is_ok_and(|name| name.starts_with("wyss-authority"))
        })
        .count()
}

#[test]
fn a_real_authority_is_followed_from_absent_to_live_events_without_writing_anything() {
    let _serial = serial();
    let env = Sandbox::new();
    let service = env.service();
    let view = until(&service, "absent", |v| v.reason == Some(Fault::Absent));
    assert_eq!(view.connection, Connection::Waiting);
    let mut authority = env.start();
    let view = live(&service, 0, 0);
    let summary = view.authority.unwrap();
    assert_eq!(
        summary.service_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        (
            summary.protocol_version,
            summary.prepared_sessions,
            summary.closed_sessions
        ),
        (1, 0, 0)
    );
    let agent = view.agent.unwrap();
    assert_eq!(agent.adapter.as_str(), "codex");
    assert_eq!(
        (agent.desired, agent.actual, agent.model_count),
        (DesiredState::Stopped, ActualState::Stopped, 0)
    );
    // Another client commits: the durable watch brings it to the view.
    let mut other = env.client();
    let session = prepare(&mut other);
    let view = live(&service, 0, 1);
    assert_eq!(view.authority.unwrap().prepared_sessions, 1);
    other
        .close(Close {
            intent_id: id(),
            managed_session_id: session.managed_session_id,
            expected_revision: 1,
        })
        .unwrap();
    let summary = live(&service, 0, 2).authority.unwrap();
    assert_eq!((summary.prepared_sessions, summary.closed_sessions), (0, 1));
    until(&service, "both events", |_| service.stats().events == 2);
    let stats = service.stats();
    assert_eq!((stats.connections, stats.resyncs), (1, 0));
    // Everything the authority holds was committed by the other client: the link, which
    // read and watched all along, wrote nothing.
    thread::sleep(Duration::from_millis(300));
    let status = other.status().unwrap();
    assert_eq!(status.last_sequence.unwrap().get(), 2);
    assert_eq!((status.prepared_count, status.closed_count), (0, 1));
    assert!(authority.alive());
}

#[test]
fn sigterm_restart_and_sigkill_keep_state_and_every_event_is_received_once() {
    let _serial = serial();
    let env = Sandbox::new();
    let mut a = env.start();
    let mut other = env.client();
    prepare(&mut other);
    prepare(&mut other);
    drop(other);
    let service = env.service();
    assert_eq!(live(&service, 0, 2).authority.unwrap().prepared_sessions, 2);

    a.stop(false);
    assert!(!env.socket().exists());
    until(&service, "absent after SIGTERM", |v| {
        v.reason == Some(Fault::Absent)
    });
    gone(&service);
    let mut b = env.start();
    // A new instance of the same stream: the committed state is all there.
    let view = live(&service, 1, 2);
    assert_eq!(view.authority.unwrap().prepared_sessions, 2);
    assert_eq!(service.stats().events, 0);

    b.stop(true);
    // A killed authority leaves its socket behind. Nobody answers on it.
    assert!(env.socket().exists());
    gone(&service);
    until(&service, "absent after SIGKILL", |v| {
        v.reason == Some(Fault::Absent)
    });
    let mut c = env.start();
    // Committed as soon as the third instance answers, whether or not the link is back.
    let mut other = env.client();
    prepare(&mut other);
    let view = live(&service, 2, 3);
    assert_eq!(view.authority.unwrap().prepared_sessions, 3);
    until(&service, "the missed event", |_| {
        service.stats().events == 1
    });
    thread::sleep(Duration::from_millis(300));
    let stats = service.stats();
    // Replayed or watched, it arrived exactly once and the cursor was never dropped.
    assert_eq!((stats.connections, stats.events, stats.resyncs), (3, 1, 0));
    assert!(c.alive());
}

#[test]
fn dozens_of_real_restarts_leak_no_descriptor_and_no_thread() {
    let _serial = serial();
    let env = Sandbox::new();
    let mut authority = env.start();
    assert_eq!(link_threads(), 0);
    let idle = descriptors();
    let service = env.service();
    live(&service, 0, 0);
    // One socket and one worker: that is all a connected link holds.
    holds(idle + 1, 1, "connected");
    for cycle in 1..=24u64 {
        authority.stop(cycle % 2 == 0);
        gone(&service);
        // Between authorities the link holds no descriptor at all.
        holds(idle, 1, "between authorities");
        authority = env.start();
        live(&service, cycle, 0);
        holds(idle + 1, 1, "reconnected");
    }
    let stats = service.stats();
    assert_eq!((stats.connections, stats.events, stats.resyncs), (25, 0, 0));

    // Shutdown ends the worker and closes the socket, and tells the authority nothing:
    // it keeps running and serves the next client.
    let started = Instant::now();
    service.shutdown();
    assert!(started.elapsed() < Duration::from_secs(2));
    holds(idle, 0, "after shutdown");
    assert_eq!(service.view().connection, Connection::Stopped);
    assert!(authority.alive());
    let mut other = env.client();
    assert_eq!(other.status().unwrap().last_sequence, None);
    thread::sleep(Duration::from_millis(600));
    assert_eq!(service.stats().connections, 25);
}

#[test]
fn a_connected_link_holds_exactly_one_of_the_four_watcher_slots() {
    let _serial = serial();
    let env = Sandbox::new();
    let _authority = env.start();
    let stream = env.client().hello_status().stream_id.clone();
    let service = env.service();
    live(&service, 0, 0);
    let watch = |wait_ms| AuditWatch {
        stream_id: stream.clone(),
        after_sequence: None,
        limit: 1,
        wait_ms,
    };
    // Give the link time to be parked in its own watch, then park three more.
    thread::sleep(Duration::from_millis(300));
    let parked: Vec<_> = (0..3)
        .map(|_| {
            let (mut client, request) = (env.client(), watch(1_500));
            thread::spawn(move || client.audit_watch(request))
        })
        .collect();
    thread::sleep(Duration::from_millis(400));
    // The fifth watcher is refused at once: four are waiting, one of them the link.
    let refused = Instant::now();
    assert!(matches!(
        env.client().audit_watch(watch(1_500)),
        Err(ControlError::CapacityExceeded)
    ));
    assert!(refused.elapsed() < Duration::from_millis(500));
    for handle in parked {
        assert!(matches!(
            handle.join().unwrap(),
            Ok(ResultValue::Audit { events, .. }) if events.is_empty()
        ));
    }
    // The link was not disturbed by its neighbours.
    assert_eq!(service.view().connection, Connection::Connected);
    assert_eq!(service.stats().failures, 0);
}
