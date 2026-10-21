//! Protocol-2 audit reads and cooperative audit watches through the real `F8Transport` over
//! `UnixStream::pair`, on a V8 fixture with real audit events. Time is injected: no test sleeps,
//! and no test launches a provider. Ordinary in-memory SQLite only.
use super::{
    authority::Authority,
    engine::{Engine, Spawned, Spawner},
    migration,
    protocol::RuntimeRef,
    registry::Registry,
    service::Service,
    staging::LaunchStaging,
};
use crate::{
    contracts::{WireUuid, control::ControlResult},
    control::{F8Transport, f8_fixture},
};
use serde_json::{Value, json};
use std::{
    io::{ErrorKind, Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

const AUTH: &str = "00000000-0000-4000-8000-0000000000a1";

fn uuid() -> String {
    crate::control::store::uuid().as_str().to_owned()
}

/// Watches and reads never launch a runtime; a spawn here is a test failure, not a launch.
struct NoSpawn;
impl Spawner for NoSpawn {
    fn spawn(&mut self, _: &RuntimeRef, _: Option<&mut LaunchStaging>) -> ControlResult<Spawned> {
        panic!("audit reads and watches must never launch a runtime")
    }
}

fn server() -> F8Transport<NoSpawn> {
    let mut source = f8_fixture::v7_with_sessions(1);
    let conn = migration::prepare(&mut source).unwrap().into_connection();
    let auth = WireUuid::try_from(AUTH.to_owned()).unwrap();
    let engine = Engine::new(conn, auth.clone(), Registry::new(auth).unwrap(), None).unwrap();
    F8Transport::new(Service::new(Authority::new(engine), NoSpawn))
}

/// The durable stream identity and head, read from the authority's own store.
fn stream(t: &F8Transport<NoSpawn>) -> (String, u64) {
    let c = t.service().authority().engine().connection();
    c.query_row(
        "SELECT stream_id,last_sequence FROM authority_metadata WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get::<_, String>(1)?.parse().unwrap())),
    )
    .unwrap()
}

fn changes(t: &F8Transport<NoSpawn>) -> u64 {
    t.service()
        .authority()
        .engine()
        .connection()
        .total_changes()
}

/// One client connection: the server's end is attached to the transport, this end is ours.
struct Peer {
    sock: UnixStream,
    buf: Vec<u8>,
    closed: bool,
}
impl Peer {
    fn connect(t: &mut F8Transport<NoSpawn>, now: Instant) -> Self {
        let (owned, sock) = UnixStream::pair().unwrap();
        sock.set_nonblocking(true).unwrap();
        t.attach(owned, now).unwrap();
        Self {
            sock,
            buf: Vec::new(),
            closed: false,
        }
    }
    fn send(&mut self, request: &Value) {
        let body = serde_json::to_vec(request).unwrap();
        self.sock
            .write_all(&(body.len() as u32).to_be_bytes())
            .unwrap();
        self.sock.write_all(&body).unwrap();
    }
    /// Every complete response that has arrived so far, in order. Never blocks.
    fn frames(&mut self) -> Vec<Value> {
        let mut chunk = [0u8; 4096];
        loop {
            match self.sock.read(&mut chunk) {
                Ok(0) => {
                    self.closed = true;
                    break;
                }
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.closed = true;
                    break;
                }
            }
        }
        let mut out = Vec::new();
        while self.buf.len() >= 4 {
            let n = u32::from_be_bytes(self.buf[..4].try_into().unwrap()) as usize;
            if self.buf.len() < 4 + n {
                break;
            }
            out.push(serde_json::from_slice(&self.buf[4..4 + n]).unwrap());
            self.buf.drain(..4 + n);
        }
        out
    }
}

/// One request needs two reads (header, then body), and each quantum takes at most one per client.
fn tick(t: &mut F8Transport<NoSpawn>, now: Instant) {
    for _ in 0..3 {
        t.poll(now).unwrap();
    }
}

fn request(method: &str, params: Value) -> Value {
    json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":AUTH,
        "method":method,"params":params})
}

/// Connects, greets, and returns the peer; the greeting's own response is consumed.
fn greeted(t: &mut F8Transport<NoSpawn>, now: Instant) -> Peer {
    let mut peer = Peer::connect(t, now);
    peer.send(
        &json!({"schema_version":2,"request_id":uuid(),"method":"authority/hello",
        "params":{"protocol_version":2,"client_instance_id":uuid()}}),
    );
    tick(t, now);
    let hello = peer.frames();
    assert_eq!(hello.len(), 1, "{hello:?}");
    assert_eq!(hello[0]["result"]["kind"], "hello", "{hello:?}");
    peer
}

fn audit_page(stream_id: &str, after: Value, limit: u16, wait_ms: Option<u32>) -> Value {
    let mut params = json!({"stream_id":stream_id,"after_sequence":after,"limit":limit});
    if let Some(wait) = wait_ms {
        params["wait_ms"] = json!(wait);
        return request("audit/watch", params);
    }
    request("audit/read", params)
}

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<ok>")
}

fn sequences(page: &Value) -> Vec<u64> {
    page["result"]["value"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["sequence"].as_str().unwrap().parse().unwrap())
        .collect()
}

#[test]
fn audit_read_pages_cover_the_durable_stream_exactly_once() {
    let mut t = server();
    let now = Instant::now();
    let mut p = greeted(&mut t, now);
    let (id, head) = stream(&t);
    assert!(
        head >= 3,
        "fixture must hold real audit events, head {head}"
    );
    let before = changes(&t);

    for limit in [1u16, 2, 5, 50] {
        let mut seen = Vec::new();
        let mut after = Value::Null;
        for _ in 0..=head {
            p.send(&audit_page(&id, after.clone(), limit, None));
            tick(&mut t, now);
            let page = p.frames().pop().expect("one response per request");
            assert_eq!(page["result"]["kind"], "audit", "limit {limit}: {page}");
            let got = sequences(&page);
            assert!(got.len() <= usize::from(limit), "limit {limit}: {page}");
            seen.extend(got);
            let next = page["result"]["value"]["next"].clone();
            if next.is_null() {
                break;
            }
            // A continuation names the last event returned, so the next page starts after it.
            assert_eq!(
                next,
                json!(seen.last().unwrap().to_string()),
                "limit {limit}"
            );
            after = next;
        }
        let expected: Vec<u64> = (1..=head).collect();
        assert_eq!(
            seen, expected,
            "limit {limit} skipped or duplicated an event"
        );
    }

    // Refusals: an unknown stream, a cursor past the head and a zero limit name no events.
    let refusals = [
        audit_page(&uuid(), Value::Null, 2, None),
        audit_page(&id, json!((head + 1).to_string()), 2, None),
        audit_page(&id, Value::Null, 0, None),
    ];
    for refused in refusals {
        p.send(&refused);
        tick(&mut t, now);
        let answer = p.frames().pop().expect("refusal is still answered");
        assert_ne!(code(&answer), "<ok>", "{answer}");
    }
    assert_eq!(changes(&t), before, "audit reads must not write the stream");
    assert_eq!(stream(&t).1, head);
}

#[test]
fn watch_with_zero_wait_or_existing_events_answers_at_once() {
    let mut t = server();
    let now = Instant::now();
    let mut p = greeted(&mut t, now);
    let (id, head) = stream(&t);

    p.send(&audit_page(&id, json!(head.to_string()), 5, Some(0)));
    tick(&mut t, now);
    let empty = p.frames();
    assert_eq!(empty.len(), 1, "wait_ms=0 answers in the same quantum");
    assert!(sequences(&empty[0]).is_empty(), "{empty:?}");
    assert!(empty[0]["result"]["value"]["next"].is_null(), "{empty:?}");

    p.send(&audit_page(
        &id,
        json!((head - 2).to_string()),
        5,
        Some(5_000),
    ));
    tick(&mut t, now);
    let events = p.frames();
    assert_eq!(events.len(), 1, "existing events never wait: {events:?}");
    assert_eq!(sequences(&events[0]), vec![head - 1, head]);
}

#[test]
fn an_empty_watch_wakes_on_a_new_durable_commit_once() {
    let mut t = server();
    let t0 = Instant::now();
    let mut watcher = greeted(&mut t, t0);
    let mut writer = greeted(&mut t, t0);
    let (id, head) = stream(&t);

    watcher.send(&audit_page(&id, json!(head.to_string()), 5, Some(5_000)));
    tick(&mut t, t0);
    assert!(
        watcher.frames().is_empty(),
        "the watch is pending, not answered"
    );

    // An ordinary authority operation from another client commits one durable event.
    writer.send(&request(
        "managedSession/prepare",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex"}),
    ));
    tick(&mut t, t0 + Duration::from_millis(10));
    let prepared = writer.frames();
    assert_eq!(prepared.len(), 1, "{prepared:?}");
    assert_eq!(prepared[0]["result"]["kind"], "session", "{prepared:?}");

    tick(&mut t, t0 + Duration::from_millis(20));
    let woken = watcher.frames();
    assert_eq!(woken.len(), 1, "exactly one response: {woken:?}");
    let seqs = sequences(&woken[0]);
    assert!(
        !seqs.is_empty() && seqs.iter().all(|s| *s > head),
        "{woken:?}"
    );
    assert_eq!(seqs[0], head + 1);

    // The answer is final: later polls never answer the same request again.
    tick(&mut t, t0 + Duration::from_millis(30));
    assert!(watcher.frames().is_empty());
}

#[test]
fn an_empty_watch_times_out_into_a_valid_empty_page() {
    let mut t = server();
    let t0 = Instant::now();
    let mut p = greeted(&mut t, t0);
    let (id, head) = stream(&t);

    p.send(&audit_page(&id, json!(head.to_string()), 5, Some(2_000)));
    tick(&mut t, t0);
    tick(&mut t, t0 + Duration::from_millis(1_999));
    assert!(p.frames().is_empty(), "the deadline has not passed");

    tick(&mut t, t0 + Duration::from_millis(2_001));
    let timed_out = p.frames();
    assert_eq!(timed_out.len(), 1, "{timed_out:?}");
    assert_eq!(timed_out[0]["result"]["kind"], "audit");
    assert!(sequences(&timed_out[0]).is_empty());
    assert!(timed_out[0]["result"]["value"]["next"].is_null());

    // The connection survives, and the idle reader deadline resumes after the answer.
    p.send(&request("authority/status", json!({})));
    tick(&mut t, t0 + Duration::from_millis(2_002));
    assert_eq!(p.frames()[0]["result"]["kind"], "status");
    assert_eq!(t.client_count(), 1);
}

#[test]
fn a_disconnect_removes_a_pending_watch_and_never_answers() {
    let mut t = server();
    let t0 = Instant::now();
    let watcher = greeted(&mut t, t0);
    let mut writer = greeted(&mut t, t0);
    let (id, head) = stream(&t);
    let mut watcher = watcher;
    watcher.send(&audit_page(&id, json!(head.to_string()), 5, Some(5_000)));
    tick(&mut t, t0);
    assert_eq!(t.client_count(), 2);

    drop(watcher.sock);
    tick(&mut t, t0 + Duration::from_millis(5));
    assert_eq!(t.client_count(), 1, "the pending watch's client is gone");

    writer.send(&request(
        "managedSession/prepare",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex"}),
    ));
    tick(&mut t, t0 + Duration::from_millis(10));
    assert_eq!(writer.frames().len(), 1);
    tick(&mut t, t0 + Duration::from_millis(1_000));
    assert_eq!(t.client_count(), 1, "nothing revives the vanished watcher");
}

#[test]
fn two_simultaneous_watchers_each_get_one_answer_and_both_stay_bounded() {
    let mut t = server();
    let t0 = Instant::now();
    let mut a = greeted(&mut t, t0);
    let mut b = greeted(&mut t, t0);
    let mut writer = greeted(&mut t, t0);
    let (id, head) = stream(&t);

    a.send(&audit_page(&id, json!(head.to_string()), 5, Some(5_000)));
    b.send(&audit_page(&id, json!(head.to_string()), 5, Some(5_000)));
    tick(&mut t, t0);
    assert!(a.frames().is_empty() && b.frames().is_empty());

    writer.send(&request(
        "managedSession/prepare",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex"}),
    ));
    tick(&mut t, t0 + Duration::from_millis(10));
    assert_eq!(writer.frames().len(), 1);
    tick(&mut t, t0 + Duration::from_millis(20));
    for (name, peer) in [("a", &mut a), ("b", &mut b)] {
        let answers = peer.frames();
        assert_eq!(
            answers.len(),
            1,
            "watcher {name} answered once: {answers:?}"
        );
        assert_eq!(sequences(&answers[0])[0], head + 1);
    }
    assert_eq!(t.client_count(), 3);
}

#[test]
fn a_pending_watch_never_delays_another_clients_ordinary_operation() {
    let mut t = server();
    let t0 = Instant::now();
    let mut watcher = greeted(&mut t, t0);
    let mut other = greeted(&mut t, t0);
    let (id, head) = stream(&t);

    watcher.send(&audit_page(&id, json!(head.to_string()), 5, Some(30_000)));
    tick(&mut t, t0);
    assert!(watcher.frames().is_empty());

    // The second client is answered in the very next quantum while the watch still waits.
    other.send(&request("authority/status", json!({})));
    tick(&mut t, t0 + Duration::from_millis(25));
    let status = other.frames();
    assert_eq!(status.len(), 1, "{status:?}");
    assert_eq!(status[0]["result"]["kind"], "status");
    assert!(watcher.frames().is_empty(), "the watch is still waiting");
}
