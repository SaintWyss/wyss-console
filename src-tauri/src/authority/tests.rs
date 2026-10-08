//! The service against a scripted authority that speaks the real wire protocol.
//!
//! The link sends one request at a time, so an action attached to "the n-th request of a
//! method" happens at one exact point of its sequence, every run. That is what makes the
//! snapshot/event/watch interleavings below deterministic.
use super::*;
use crate::contracts::{
    WireUuid,
    agent_adapter::{AgentAdapterId, AgentRuntimeKind, CompatibilityState},
    control::*,
    event_envelope::EventEnvelopeV1,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

fn uuid() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}

/// How the scripted authority departs from the protocol, if it does.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Behavior {
    Canonical,
    /// Refuses the handshake with this error.
    RejectHello(ControlError),
    /// Answers the handshake in an envelope of another schema version.
    EnvelopeVersion,
    /// A well-formed handshake that declares another protocol version.
    HelloProtocol,
    /// A well-formed handshake that declares another storage schema.
    HelloSchema,
    /// A frame that is not JSON.
    Garbage,
    /// A length prefix past the frame bound.
    Oversize,
    /// An answer to a request nobody made.
    WrongRequestId,
    /// The handshake answered with a result of another kind.
    WrongKind,
    /// Accepts the connection and never answers.
    Silent,
    /// After the handshake: half a frame, then the connection closes.
    Truncated,
    /// After the handshake: answers as another authority instance.
    WrongInstance,
    /// After the handshake: the status names another stream.
    WrongStream,
    /// After the handshake: a watch page that skips one sequence.
    SkipEvent,
    /// After the handshake: every watcher slot is taken.
    Crowded,
}

type Hook = Box<dyn FnMut(&mut Model) + Send>;
struct Model {
    instance: WireUuid,
    stream: WireUuid,
    /// Durable envelopes. The sequence of each is its position plus one.
    events: Vec<Value>,
    prepared: u64,
    actual: ActualState,
    behavior: Behavior,
    /// Actions run right before the n-th request of a method is answered.
    hooks: Vec<(&'static str, usize, Hook)>,
    seen: HashMap<String, usize>,
    log: Vec<String>,
    /// Connections of an older epoch are closed.
    epoch: u64,
    serving: bool,
    /// An empty watch answers after this long at most, whatever wait was requested.
    watch_cap: Duration,
    /// The cursor of a watch that found nothing after it, until the next request.
    watching: Option<u64>,
}
impl Model {
    fn head(&self) -> u64 {
        self.events.len() as u64
    }
    /// One durable event and the canonical change it records.
    fn commit(&mut self, actual: ActualState) {
        let sequence = self.head() + 1;
        self.events.push(json!({
            "schema_version": 1, "event_id": uuid(),
            "event_type": "authority.runtime.instance_ready", "payload_schema_version": 1,
            "timestamp": "2026-10-08T00:00:00Z", "stream_id": self.stream,
            "sequence": sequence.to_string(), "session_id": null,
            "source": {"component": "wyss-control", "instance_id": self.instance},
            "correlation_id": null, "causation_id": null, "payload": {},
            "persistence": "durable", "sensitivity": "internal", "redaction": "none"
        }));
        self.actual = actual;
        self.prepared += 1;
    }
    fn status(&self) -> AuthorityStatus {
        AuthorityStatus {
            protocol_version: 1,
            authority_instance_id: self.instance.clone(),
            service_version: "9.9.9-test".into(),
            storage_schema_version: 7,
            stream_id: self.stream.clone(),
            last_sequence: self.head().to_string().parse().ok(),
            prepared_count: self.prepared,
            closed_count: 0,
        }
    }
    fn runtime(&self) -> RuntimeStatus {
        let ready = self.actual == ActualState::Ready;
        RuntimeStatus {
            adapter_id: AgentAdapterId::parse("codex").unwrap(),
            runtime_kind: AgentRuntimeKind::Codex,
            desired_state: if self.actual == ActualState::Stopped {
                DesiredState::Stopped
            } else {
                DesiredState::Running
            },
            revision: 1,
            actual_state: self.actual,
            runtime_instance_id: None,
            runtime_version: ready.then(|| "0.159.0".to_owned()),
            compatibility: CompatibilityState::Unknown,
            model_count: if ready { 7 } else { 0 },
            diagnostic: Some(RuntimeDiagnostic::SpawnFailed),
            outbound_methods: RuntimeMethods::default(),
            runtime_safety_profile: None,
            workspace_root: None,
        }
    }
    fn page(&self, after: Option<crate::contracts::Sequence>, limit: u16) -> ResultValue {
        let from = after.map_or(0, |s| s.get()) as usize;
        let events: Vec<EventEnvelopeV1> = self
            .events
            .iter()
            .skip(from)
            .take(limit.into())
            .map(|e| EventEnvelopeV1::from_value(e.clone()).unwrap())
            .collect();
        let more = from + events.len() < self.events.len();
        ResultValue::Audit {
            next: more.then(|| events.last().unwrap().sequence()),
            events,
        }
    }
}

type Shared = Arc<(Mutex<Model>, Condvar)>;
/// A private runtime directory with a scripted authority that can stop and start again.
struct Fake {
    root: PathBuf,
    shared: Shared,
    threads: Arc<Mutex<Vec<thread::JoinHandle<()>>>>,
    accept: Option<thread::JoinHandle<()>>,
}
impl Fake {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join("wyss-console"))
            .unwrap();
        let model = Model {
            instance: uuid(),
            stream: uuid(),
            events: Vec::new(),
            prepared: 0,
            actual: ActualState::Stopped,
            behavior: Behavior::Canonical,
            hooks: Vec::new(),
            seen: HashMap::new(),
            log: Vec::new(),
            epoch: 0,
            serving: false,
            watch_cap: Duration::from_millis(60),
            watching: None,
        };
        Self {
            root,
            shared: Arc::new((Mutex::new(model), Condvar::new())),
            threads: Arc::default(),
            accept: None,
        }
    }
    fn started() -> Self {
        let mut fake = Self::new();
        fake.start();
        fake
    }
    fn socket(&self) -> PathBuf {
        self.root.join("wyss-console/control.sock")
    }
    fn with<R>(&self, f: impl FnOnce(&mut Model) -> R) -> R {
        let mut model = self.shared.0.lock().unwrap();
        let result = f(&mut model);
        drop(model);
        self.shared.1.notify_all();
        result
    }
    fn commit(&self, actual: ActualState) {
        self.with(|m| m.commit(actual));
    }
    fn hook(&self, method: &'static str, nth: usize, f: impl FnMut(&mut Model) + Send + 'static) {
        self.with(|m| m.hooks.push((method, nth, Box::new(f))));
    }
    fn count(&self, method: &str) -> usize {
        self.with(|m| m.seen.get(method).copied().unwrap_or(0))
    }
    /// A new authority instance over the same durable state.
    fn start(&mut self) {
        let listener = UnixListener::bind(self.socket()).unwrap();
        fs::set_permissions(self.socket(), fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        self.with(|m| {
            m.instance = uuid();
            m.serving = true;
        });
        let (shared, threads) = (self.shared.clone(), self.threads.clone());
        self.accept = Some(thread::spawn(move || {
            loop {
                let epoch = {
                    let model = shared.0.lock().unwrap();
                    if !model.serving {
                        return;
                    }
                    model.epoch
                };
                match listener.accept() {
                    Ok((stream, _)) => {
                        let shared = shared.clone();
                        threads
                            .lock()
                            .unwrap()
                            .push(thread::spawn(move || serve(stream, &shared, epoch)));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(2)),
                }
            }
        }));
    }
    /// Closes every connection. The listener stays: the same instance keeps answering.
    fn drop_connections(&self) {
        self.with(|m| m.epoch += 1);
        for handle in self.threads.lock().unwrap().drain(..) {
            handle.join().unwrap();
        }
    }
    /// The authority exits. `clean` removes its socket as a normal shutdown does; a
    /// killed authority leaves it behind.
    fn stop(&mut self, clean: bool) {
        self.with(|m| {
            m.serving = false;
            m.epoch += 1;
        });
        if let Some(accept) = self.accept.take() {
            accept.join().unwrap();
        }
        for handle in self.threads.lock().unwrap().drain(..) {
            handle.join().unwrap();
        }
        if clean {
            fs::remove_file(self.socket()).unwrap();
        }
    }
    /// Starts again, counting requests from this start.
    fn restart(&mut self) {
        let _ = fs::remove_file(self.socket());
        self.with(|m| m.seen.clear());
        self.start();
    }
    fn service(&self) -> AuthorityService {
        AuthorityService::start(Some(self.root.clone()))
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        if self.accept.is_some() {
            self.stop(false);
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn send(stream: &mut UnixStream, bytes: &[u8]) -> bool {
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|()| stream.write_all(bytes))
        .is_ok()
}
/// One frame, or `None` once the peer is gone or this connection's epoch has passed.
fn receive(stream: &mut UnixStream, shared: &Shared, epoch: u64) -> Option<Vec<u8>> {
    stream
        .set_read_timeout(Some(Duration::from_millis(10)))
        .unwrap();
    let mut read = |out: &mut [u8]| {
        let mut offset = 0;
        while offset < out.len() {
            if shared.0.lock().unwrap().epoch != epoch {
                return false;
            }
            match stream.read(&mut out[offset..]) {
                Ok(0) => return false,
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return false,
            }
        }
        true
    };
    let mut header = [0; 4];
    if !read(&mut header) {
        return None;
    }
    let mut body = vec![0; u32::from_be_bytes(header) as usize];
    read(&mut body).then_some(body)
}

fn serve(mut stream: UnixStream, shared: &Shared, epoch: u64) {
    stream.set_nonblocking(false).unwrap();
    let mut greeted = false;
    while let Some(frame) = receive(&mut stream, shared, epoch) {
        let request = Request::parse(&frame).unwrap();
        let method = serde_json::to_value(&request.operation).unwrap()["method"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut model = shared.0.lock().unwrap();
        let nth = {
            let seen = model.seen.entry(method.clone()).or_insert(0);
            *seen += 1;
            *seen
        };
        model.log.push(method.clone());
        model.watching = None;
        let mut hooks = std::mem::take(&mut model.hooks);
        for (_, _, hook) in hooks
            .iter_mut()
            .filter(|(m, n, _)| *m == method && *n == nth)
        {
            hook(&mut model);
        }
        hooks.append(&mut model.hooks);
        model.hooks = hooks;
        let behavior = model.behavior;
        let hello = matches!(request.operation, Operation::Hello(_));
        if !greeted && !hello {
            panic!("the link sent {method} before its handshake");
        }
        // Departures that never produce a canonical frame.
        match (behavior, hello) {
            (Behavior::Silent, true) => {
                drop(model);
                while shared.0.lock().unwrap().epoch == epoch {
                    thread::sleep(Duration::from_millis(5));
                }
                return;
            }
            (Behavior::Garbage, true) => {
                send(&mut stream, b"not json at all");
                return;
            }
            (Behavior::Oversize, true) => {
                let _ = stream.write_all(&((MAX_FRAME as u32) + 1).to_be_bytes());
                return;
            }
            (Behavior::Truncated, false) => {
                let _ = stream.write_all(&[0, 0, 0, 90, b'{', b'"']);
                return;
            }
            _ => (),
        }
        let outcome = match &request.operation {
            Operation::Hello(_) => match behavior {
                Behavior::RejectHello(error) => Err(error),
                Behavior::WrongKind => Ok(ResultValue::Status(model.status())),
                _ => Ok(ResultValue::Hello(model.status())),
            },
            Operation::Status(_) => Ok(ResultValue::Status(model.status())),
            Operation::RuntimeStatus(_) => Ok(ResultValue::RuntimeStatus(model.runtime())),
            Operation::AuditRead(p) => Ok(model.page(p.after_sequence, p.limit)),
            Operation::AuditWatch(_) if behavior == Behavior::Crowded => {
                Err(ControlError::CapacityExceeded)
            }
            Operation::AuditWatch(p) => {
                let cursor = p.after_sequence.map_or(0, |s| s.get());
                let wait = Duration::from_millis(p.wait_ms.into()).min(model.watch_cap);
                let deadline = Instant::now() + wait;
                if model.head() <= cursor {
                    model.watching = Some(cursor);
                }
                // The same contract as the authority: at once when history exists,
                // otherwise at the next commit or after the requested wait.
                while model.head() <= cursor && model.epoch == epoch {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    model = shared
                        .1
                        .wait_timeout(model, left.min(Duration::from_millis(10)))
                        .unwrap()
                        .0;
                }
                if model.epoch != epoch {
                    return;
                }
                Ok(model.page(p.after_sequence, p.limit))
            }
            _ => Err(ControlError::Unsupported),
        };
        let response = Response {
            request_id: if behavior == Behavior::WrongRequestId {
                uuid()
            } else {
                request.request_id.clone()
            },
            authority_instance_id: if behavior == Behavior::WrongInstance && !hello {
                uuid()
            } else {
                model.instance.clone()
            },
            outcome,
        };
        let mut value: Value = serde_json::from_slice(&response.encode().unwrap()).unwrap();
        match (behavior, &request.operation) {
            (Behavior::EnvelopeVersion, Operation::Hello(_)) => value["schema_version"] = json!(2),
            (Behavior::HelloProtocol, Operation::Hello(_)) => {
                value["result"]["value"]["protocol_version"] = json!(2)
            }
            (Behavior::HelloSchema, Operation::Hello(_)) => {
                value["result"]["value"]["storage_schema_version"] = json!(8)
            }
            (Behavior::WrongStream, Operation::Status(_)) => {
                value["result"]["value"]["stream_id"] = json!(uuid())
            }
            (Behavior::SkipEvent, Operation::AuditWatch(_)) => {
                if let Some(first) = value["result"]["value"]["events"].get_mut(0) {
                    let skipped = first["sequence"].as_str().unwrap().parse::<u64>().unwrap() + 1;
                    first["sequence"] = json!(skipped.to_string());
                }
            }
            _ => (),
        }
        drop(model);
        if !send(&mut stream, &serde_json::to_vec(&value).unwrap()) {
            return;
        }
        greeted = true;
    }
}

const SOON: Duration = Duration::from_secs(10);
/// The first view that satisfies `accept`, waiting on the service's own change signal.
fn until(
    service: &AuthorityService,
    what: &str,
    accept: impl Fn(&AuthorityView) -> bool,
) -> AuthorityView {
    let deadline = Instant::now() + SOON;
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
fn connected(service: &AuthorityService) -> AuthorityView {
    until(service, "connected", |v| {
        v.connection == Connection::Connected
    })
}
fn waiting(service: &AuthorityService, fault: Fault) -> AuthorityView {
    until(service, "waiting", |v| {
        v.connection == Connection::Waiting && v.reason == Some(fault)
    })
}
fn head(view: &AuthorityView) -> u64 {
    view.authority
        .as_ref()
        .and_then(|a| a.last_sequence)
        .map_or(0, |s| s.get())
}
/// Waits until the link is watching at the durable head: no event is in flight.
fn quiet(fake: &Fake, service: &AuthorityService) {
    until(service, "quiet", |_| {
        fake.with(|m| m.watching == Some(m.head()))
    });
}
fn at(service: &AuthorityService, sequence: u64, actual: ActualState) -> AuthorityView {
    until(service, "canonical view", |v| {
        v.connection == Connection::Connected
            && head(v) == sequence
            && v.agent.as_ref().is_some_and(|a| a.actual == actual)
    })
}

#[test]
fn absent_authority_is_reported_and_retried_without_ever_connecting() {
    let fake = Fake::new();
    let service = fake.service();
    let view = waiting(&service, Fault::Absent);
    assert_eq!((&view.authority, &view.agent), (&None, &None));
    // Two more attempts: the delay doubled, and each one ended the same way.
    until(&service, "retries", |_| service.stats().failures >= 3);
    assert_eq!(service.stats().connections, 0);
    assert_eq!(service.view().reason, Some(Fault::Absent));
}

#[test]
fn an_authority_that_starts_later_is_found_and_read_canonically() {
    let mut fake = Fake::new();
    let service = fake.service();
    waiting(&service, Fault::Absent);
    fake.with(|m| {
        m.commit(ActualState::Starting);
        m.commit(ActualState::Ready);
    });
    fake.start();
    let view = at(&service, 2, ActualState::Ready);
    assert_eq!(view.reason, None);
    let authority = view.authority.unwrap();
    assert_eq!(authority.service_version.as_deref(), Some("9.9.9-test"));
    assert_eq!(
        (
            authority.protocol_version,
            authority.restarts,
            authority.prepared_sessions
        ),
        (1, 0, 2)
    );
    let agent = view.agent.unwrap();
    assert_eq!(agent.adapter.as_str(), "codex");
    assert_eq!(agent.desired, DesiredState::Running);
    assert_eq!(
        (agent.runtime_version.as_deref(), agent.model_count),
        (Some("0.159.0"), 7)
    );
    // History before the first snapshot is in the snapshot: none of it is replayed.
    assert_eq!(service.stats().events, 0);
    assert_eq!(fake.count("audit/read"), 0);
}

/// The serialized view is the frontend's whole contract: the same fixture is parsed by
/// the TypeScript validator. The revision counts changes, so it is not compared.
fn assert_fixture(view: &AuthorityView, name: &str) {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/authority-view-v1.json"
    ))
    .unwrap();
    let mut expected = fixtures[name].clone();
    expected["revision"] = json!(view.revision);
    assert_eq!(serde_json::to_value(view).unwrap(), expected, "{name}");
}

#[test]
fn the_projection_is_the_exact_v1_contract_without_identity_diagnostic_or_workspace() {
    let mut fake = Fake::started();
    fake.commit(ActualState::Failed);
    let service = fake.service();
    let view = at(&service, 1, ActualState::Failed);
    let text = serde_json::to_string(&view).unwrap();
    let (instance, stream) = fake.with(|m| (m.instance.clone(), m.stream.clone()));
    for hidden in [
        instance.as_str(),
        stream.as_str(),
        "spawn_failed",
        "diagnostic",
        "workspace",
    ] {
        assert!(!text.contains(hidden), "{hidden} in {text}");
    }
    assert_fixture(&view, "connected");
    fake.stop(true);
    assert_fixture(&waiting(&service, Fault::Absent), "waiting_absent");
    service.shutdown();
    assert_fixture(&service.view(), "stopped");
}

#[test]
fn live_events_refresh_the_view_through_the_watch() {
    let fake = Fake::started();
    let service = fake.service();
    at(&service, 0, ActualState::Stopped);
    for (n, actual) in [
        ActualState::Starting,
        ActualState::Initializing,
        ActualState::Ready,
    ]
    .into_iter()
    .enumerate()
    {
        fake.commit(actual);
        at(&service, n as u64 + 1, actual);
    }
    quiet(&fake, &service);
    let stats = service.stats();
    assert_eq!(
        (
            stats.connections,
            stats.failures,
            stats.events,
            stats.resyncs
        ),
        (1, 0, 3, 0)
    );
    assert_eq!(fake.count("audit/read"), 0);
}

#[test]
fn a_change_that_writes_no_event_is_read_when_a_watch_ends_empty() {
    let fake = Fake::started();
    let service = fake.service();
    at(&service, 0, ActualState::Stopped);
    quiet(&fake, &service);
    // The runtime settles and the session count moves, and no event says so.
    fake.with(|m| {
        m.actual = ActualState::Ready;
        m.prepared = 5;
    });
    let view = at(&service, 0, ActualState::Ready);
    assert_eq!(view.authority.unwrap().prepared_sessions, 5);
    assert_eq!(
        (service.stats().events, service.stats().connections),
        (0, 1)
    );
}

#[test]
fn a_restarted_authority_is_a_new_instance_and_missed_events_are_replayed_once() {
    let mut fake = Fake::started();
    fake.with(|m| (0..3).for_each(|_| m.commit(ActualState::Ready)));
    let service = fake.service();
    at(&service, 3, ActualState::Ready);
    fake.stop(true);
    let view = until(&service, "authority gone", |v| {
        v.connection == Connection::Waiting
    });
    assert_eq!((&view.authority, &view.agent), (&None, &None));
    // Two pages of history commit while nobody is connected.
    fake.with(|m| (0..60).for_each(|_| m.commit(ActualState::Stopped)));
    fake.restart();
    let view = at(&service, 63, ActualState::Stopped);
    assert_eq!(view.authority.unwrap().restarts, 1);
    quiet(&fake, &service);
    let stats = service.stats();
    assert_eq!((stats.connections, stats.events, stats.resyncs), (2, 60, 0));
    assert_eq!(fake.count("audit/read"), 2);
}

#[test]
fn a_killed_authority_leaves_a_dead_socket_that_reads_as_absent() {
    let mut fake = Fake::started();
    let service = fake.service();
    connected(&service);
    fake.stop(false);
    assert!(fake.socket().exists());
    waiting(&service, Fault::Absent);
    fake.restart();
    assert_eq!(connected(&service).authority.unwrap().restarts, 1);
}

#[test]
fn a_lost_socket_reconnects_to_the_same_instance_without_counting_a_restart() {
    let fake = Fake::started();
    let service = fake.service();
    connected(&service);
    fake.drop_connections();
    until(&service, "second connection", |_| {
        service.stats().connections == 2
    });
    fake.commit(ActualState::Ready);
    let view = at(&service, 1, ActualState::Ready);
    assert_eq!(view.authority.unwrap().restarts, 0);
    assert_eq!(service.stats().failures, 1);
}

#[test]
fn another_stream_or_a_long_gap_moves_the_cursor_to_the_head_instead_of_replaying() {
    let mut fake = Fake::started();
    fake.with(|m| (0..5).for_each(|_| m.commit(ActualState::Ready)));
    let service = fake.service();
    at(&service, 5, ActualState::Ready);
    // Another store: a shorter history under another stream identity.
    fake.stop(true);
    fake.with(|m| {
        m.stream = uuid();
        m.events.clear();
        m.commit(ActualState::Stopped);
    });
    fake.restart();
    at(&service, 1, ActualState::Stopped);
    quiet(&fake, &service);
    assert_eq!((service.stats().events, service.stats().resyncs), (0, 0));
    // The same stream, further ahead than is worth replaying.
    fake.stop(true);
    fake.with(|m| (0..2_001).for_each(|_| m.commit(ActualState::Ready)));
    fake.restart();
    at(&service, 2_002, ActualState::Ready);
    quiet(&fake, &service);
    assert_eq!((service.stats().events, service.stats().resyncs), (0, 1));
    assert_eq!(fake.count("audit/read"), 0);
    // And a head behind the cursor is a position that names no history.
    fake.stop(true);
    fake.with(|m| m.events.truncate(4));
    fake.restart();
    at(&service, 4, ActualState::Ready);
    quiet(&fake, &service);
    assert_eq!((service.stats().events, service.stats().resyncs), (0, 2));
}

/// As the authority does, an empty watch waits for as long as it was asked to: within
/// these tests nothing is ever read again unless an event says so. A change the link lost
/// stays lost and the test fails, instead of being repaired by the periodic re-read.
const PATIENT: Duration = Duration::from_secs(30);
/// What makes the link read a second time: a commit that lands in its first watch.
const SECOND_READ: (&str, usize) = ("audit/watch", 1);

#[test]
fn a_commit_at_any_point_of_the_first_synchronization_is_seen_exactly_once() {
    // (where the commit lands, whether a second read is provoked, events streamed)
    for (point, second_read, streamed) in [
        // Before the head was read, the commit is part of the snapshot.
        (("authority/hello", 1), false, 0),
        (("authority/status", 1), false, 0),
        // After it, the commit is past the cursor and the stream delivers it.
        (("runtime/status", 1), false, 1),
        (("audit/watch", 1), false, 1),
        (("authority/status", 2), true, 2),
        (("runtime/status", 2), true, 2),
        (("audit/watch", 2), true, 2),
    ] {
        let fake = Fake::started();
        fake.with(|m| {
            m.watch_cap = PATIENT;
            (0..2).for_each(|_| m.commit(ActualState::Stopped));
        });
        if second_read {
            fake.hook(SECOND_READ.0, SECOND_READ.1, |m| {
                m.commit(ActualState::Starting)
            });
        }
        fake.hook(point.0, point.1, |m| m.commit(ActualState::Ready));
        let service = fake.service();
        at(&service, 3 + u64::from(second_read), ActualState::Ready);
        quiet(&fake, &service);
        let stats = service.stats();
        assert_eq!(
            (
                stats.connections,
                stats.failures,
                stats.events,
                stats.resyncs
            ),
            (1, 0, streamed, 0),
            "{point:?}"
        );
        assert_eq!(fake.count("audit/read"), 0, "{point:?}");
    }
}

#[test]
fn a_commit_at_any_point_of_a_reconnection_replay_is_seen_exactly_once() {
    // Counted from the reconnection: the handshake, the snapshot, two replay pages, the
    // first watch and, when a second read is provoked, what follows it.
    for (point, second_read) in [
        (("authority/hello", 1), false),
        (("authority/status", 1), false),
        (("runtime/status", 1), false),
        (("audit/read", 1), false),
        (("audit/read", 2), false),
        (("audit/watch", 1), false),
        (("authority/status", 2), true),
        (("runtime/status", 2), true),
        (("audit/watch", 2), true),
    ] {
        let mut fake = Fake::started();
        fake.with(|m| {
            m.watch_cap = PATIENT;
            (0..4).for_each(|_| m.commit(ActualState::Stopped));
        });
        let service = fake.service();
        at(&service, 4, ActualState::Stopped);
        quiet(&fake, &service);
        fake.stop(true);
        until(&service, "authority gone", |v| {
            v.connection == Connection::Waiting
        });
        fake.with(|m| (0..60).for_each(|_| m.commit(ActualState::Starting)));
        if second_read {
            fake.hook(SECOND_READ.0, SECOND_READ.1, |m| {
                m.commit(ActualState::Stopping)
            });
        }
        fake.hook(point.0, point.1, |m| m.commit(ActualState::Ready));
        fake.restart();
        let extra = 1 + u64::from(second_read);
        at(&service, 64 + extra, ActualState::Ready);
        quiet(&fake, &service);
        let stats = service.stats();
        // Every event after the old cursor arrived through the stream, once.
        assert_eq!(
            (stats.connections, stats.events, stats.resyncs),
            (2, 60 + extra, 0),
            "{point:?}"
        );
        assert_eq!(fake.count("audit/read"), 2, "{point:?}");
    }
}

#[test]
fn an_incompatible_authority_is_named_and_a_compatible_one_recovers() {
    for behavior in [
        Behavior::RejectHello(ControlError::ProtocolVersionUnsupported),
        Behavior::EnvelopeVersion,
        Behavior::HelloProtocol,
        Behavior::HelloSchema,
    ] {
        let fake = Fake::started();
        fake.with(|m| m.behavior = behavior);
        let service = fake.service();
        let view = waiting(&service, Fault::Incompatible);
        assert_eq!(
            (&view.authority, &view.agent),
            (&None, &None),
            "{behavior:?}"
        );
        assert_eq!(service.stats().connections, 0, "{behavior:?}");
        assert_eq!(fake.count("authority/status"), 0, "{behavior:?}");
        fake.with(|m| m.behavior = Behavior::Canonical);
        assert_eq!(connected(&service).reason, None, "{behavior:?}");
    }
}

#[test]
fn a_malformed_handshake_is_rejected_before_anything_is_read() {
    for behavior in [
        Behavior::Garbage,
        Behavior::Oversize,
        Behavior::WrongRequestId,
        Behavior::WrongKind,
        Behavior::RejectHello(ControlError::InternalError),
    ] {
        let fake = Fake::started();
        fake.with(|m| m.behavior = behavior);
        let service = fake.service();
        waiting(&service, Fault::Malformed);
        assert_eq!(service.stats().connections, 0, "{behavior:?}");
        assert_eq!(fake.count("authority/status"), 0, "{behavior:?}");
        fake.with(|m| m.behavior = Behavior::Canonical);
        connected(&service);
    }
}

#[test]
fn a_peer_that_breaks_the_protocol_after_the_handshake_is_dropped_and_shows_nothing() {
    for (behavior, fault) in [
        (Behavior::WrongInstance, Fault::Malformed),
        (Behavior::WrongStream, Fault::Malformed),
        (Behavior::Truncated, Fault::Lost),
    ] {
        let fake = Fake::started();
        fake.with(|m| m.behavior = behavior);
        let service = fake.service();
        let view = waiting(&service, fault);
        assert_eq!(
            (&view.authority, &view.agent),
            (&None, &None),
            "{behavior:?}"
        );
        assert!(service.stats().connections >= 1, "{behavior:?}");
        fake.with(|m| m.behavior = Behavior::Canonical);
        connected(&service);
    }
}

#[test]
fn a_skipped_sequence_ends_the_connection_and_the_replay_restores_the_stream() {
    let fake = Fake::started();
    let service = fake.service();
    at(&service, 0, ActualState::Stopped);
    fake.with(|m| {
        m.behavior = Behavior::SkipEvent;
        m.commit(ActualState::Ready);
    });
    waiting(&service, Fault::Malformed);
    // The event that was misnumbered was never taken: the cursor did not move.
    assert_eq!(service.stats().events, 0);
    fake.with(|m| m.behavior = Behavior::Canonical);
    at(&service, 1, ActualState::Ready);
    assert_eq!((service.stats().events, service.stats().resyncs), (1, 0));
    assert_eq!(fake.count("audit/read"), 1);
}

#[test]
fn an_unsafe_runtime_directory_or_socket_is_refused_without_a_handshake() {
    let fake = Fake::started();
    fs::set_permissions(fake.socket(), fs::Permissions::from_mode(0o666)).unwrap();
    let service = fake.service();
    waiting(&service, Fault::UnsafePath);
    fs::set_permissions(fake.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(
        fake.root.join("wyss-console"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    until(&service, "unsafe directory", |_| {
        service.stats().failures >= 3
    });
    assert_eq!(service.view().reason, Some(Fault::UnsafePath));
    assert_eq!(
        (service.stats().connections, fake.count("authority/hello")),
        (0, 0)
    );
    fs::set_permissions(
        fake.root.join("wyss-console"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    connected(&service);
    // No runtime directory at all is the same refusal.
    let nowhere = AuthorityService::start(None);
    waiting(&nowhere, Fault::UnsafePath);
}

#[test]
fn full_watcher_slots_degrade_to_bounded_reads_on_the_same_connection() {
    let fake = Fake::started();
    fake.with(|m| m.behavior = Behavior::Crowded);
    let service = fake.service();
    at(&service, 0, ActualState::Stopped);
    fake.commit(ActualState::Ready);
    at(&service, 1, ActualState::Ready);
    until(&service, "event read", |_| service.stats().events == 1);
    let stats = service.stats();
    assert_eq!(
        (stats.connections, stats.failures, stats.resyncs),
        (1, 0, 0)
    );
    assert!(fake.count("audit/read") >= 1);
}

#[test]
fn shutdown_cancels_a_parked_watch_and_closes_the_connection() {
    let fake = Fake::started();
    fake.with(|m| m.watch_cap = PATIENT);
    let service = fake.service();
    connected(&service);
    quiet(&fake, &service);
    let waiter = {
        let service = service.clone();
        let revision = service.view().revision;
        thread::spawn(move || service.wait_changed(Some(revision), Duration::from_secs(30)))
    };
    let started = Instant::now();
    service.shutdown();
    // The watch would have waited twenty seconds.
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    let last = waiter.join().unwrap();
    assert_eq!(last.connection, Connection::Stopped);
    assert_eq!(
        (&last.reason, &last.authority, &last.agent),
        (&None, &None, &None)
    );
    // The authority sees an end of file and nothing else: no further request arrives.
    fake.drop_connections();
    let seen = fake.with(|m| m.log.len());
    thread::sleep(Duration::from_millis(400));
    assert_eq!(fake.with(|m| m.log.len()), seen);
    assert_eq!(service.stats().connections, 1);
    // Stopped is terminal and answers at once, and shutdown is idempotent.
    service.shutdown();
    let again = Instant::now();
    let view = service.wait_changed(Some(last.revision), Duration::from_secs(30));
    assert_eq!(view, last);
    assert!(again.elapsed() < Duration::from_secs(1));
}

#[test]
fn shutdown_cancels_a_handshake_that_nobody_answers() {
    let fake = Fake::started();
    fake.with(|m| m.behavior = Behavior::Silent);
    let service = fake.service();
    until(&service, "handshake sent", |_| {
        fake.count("authority/hello") == 1
    });
    let started = Instant::now();
    service.shutdown();
    // The handshake deadline is five seconds.
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(service.view().connection, Connection::Stopped);
}

#[test]
fn a_silent_authority_times_out_and_is_retried() {
    let fake = Fake::started();
    fake.with(|m| m.behavior = Behavior::Silent);
    let service = fake.service();
    waiting(&service, Fault::Absent);
    fake.with(|m| m.behavior = Behavior::Canonical);
    connected(&service);
}

#[test]
fn waiting_for_a_change_returns_at_once_when_stale_and_blocks_when_current() {
    let fake = Fake::started();
    let service = fake.service();
    connected(&service);
    quiet(&fake, &service);
    let view = service.view();
    let instant = Instant::now();
    assert_eq!(service.wait_changed(None, Duration::from_secs(30)), view);
    assert_eq!(
        service.wait_changed(Some(view.revision + 7), Duration::from_secs(30)),
        view
    );
    assert!(instant.elapsed() < Duration::from_secs(1));
    let blocked = Instant::now();
    assert_eq!(
        service.wait_changed(Some(view.revision), Duration::from_millis(150)),
        view
    );
    assert!(blocked.elapsed() >= Duration::from_millis(150));
    let waiter = {
        let (service, revision) = (service.clone(), view.revision);
        thread::spawn(move || service.wait_changed(Some(revision), Duration::from_secs(30)))
    };
    fake.commit(ActualState::Ready);
    let changed = waiter.join().unwrap();
    assert!(changed.revision > view.revision);
}

#[test]
fn dozens_of_reconnections_keep_one_connection_and_an_exact_cursor() {
    let fake = Fake::started();
    let service = fake.service();
    connected(&service);
    for cycle in 1..=24u64 {
        fake.drop_connections();
        fake.commit(ActualState::Ready);
        at(&service, cycle, ActualState::Ready);
        until(&service, "reconnected", |_| {
            service.stats().connections == cycle + 1
        });
    }
    let stats = service.stats();
    assert_eq!(
        (stats.connections, stats.events, stats.resyncs),
        (25, 24, 0)
    );
    // One connection is open: the scripted authority holds exactly one live handler.
    quiet(&fake, &service);
    let live = fake
        .threads
        .lock()
        .unwrap()
        .iter()
        .filter(|h| !h.is_finished())
        .count();
    assert_eq!(live, 1);
}
