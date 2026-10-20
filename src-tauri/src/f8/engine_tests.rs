//! F8-01 .. F8-18 against the real engine, allocator, registry, scheduler, meters, owned
//! children and staging. Ordinary in-memory SQLite and unit fixtures only: synthetic success here
//! is NOT evidence of provider concurrency, real containment or installed-application acceptance.
use super::*;
use crate::{
    contracts::control::ControlError as E,
    control::f8_fixture,
    f8::{
        migration,
        ownership::{ContainmentPolicy, Grace},
        scheduler::Class,
        v8::validate_v8,
    },
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    os::unix::fs::DirBuilderExt,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

const AUTH: &str = "00000000-0000-4000-8000-0000000000a1";
type Frames = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

fn uuid() -> String {
    crate::control::store::uuid().as_str().to_owned()
}
fn wire(s: &str) -> WireUuid {
    WireUuid::try_from(s.to_owned()).unwrap()
}

struct FakeWorker {
    session: String,
    frames: Frames,
}
impl Worker for FakeWorker {
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.frames
            .lock()
            .unwrap()
            .push((self.session.clone(), frame.to_vec()));
        Ok(())
    }
}
struct Spawn {
    frames: Frames,
    spawned: usize,
    script: Option<String>,
    fail: bool,
    catalogue: bool,
    grace: Grace,
}
impl Spawn {
    fn new(frames: &Frames) -> Self {
        Self {
            frames: frames.clone(),
            spawned: 0,
            script: None,
            fail: false,
            catalogue: false,
            grace: Grace {
                eof: Duration::from_millis(100),
                term: Duration::from_millis(100),
                kill: Duration::from_millis(2000),
            },
        }
    }
}
impl Spawner for Spawn {
    fn spawn(
        &mut self,
        launch: &RuntimeRef,
        staging: Option<&mut LaunchStaging>,
    ) -> ControlResult<Spawned> {
        self.spawned += 1;
        if let (true, Some(s)) = (self.catalogue, staging) {
            s.write_file(
                "catalogue.json",
                launch.runtime_instance_id.as_str().as_bytes(),
            )?;
        }
        if self.fail {
            return Err(E::ProviderRejected);
        }
        let child = match &self.script {
            Some(script) => {
                let mut c = Command::new("/bin/sh");
                c.arg("-c")
                    .arg(script.as_str())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                Some(OwnedChild::spawn(&mut c, ContainmentPolicy::DirectChildOnly, self.grace)?.0)
            }
            None => None,
        };
        Ok(Spawned {
            worker: Box::new(FakeWorker {
                session: launch.managed_session_id.as_str().to_owned(),
                frames: self.frames.clone(),
            }),
            child,
        })
    }
}

#[derive(Default)]
struct Leases(Mutex<HashSet<(String, String)>>);
impl Leases {
    fn grant(&self, session: &str) -> LeaseClaim {
        let lease_id = uuid();
        self.0
            .lock()
            .unwrap()
            .insert((session.to_owned(), lease_id.clone()));
        LeaseClaim {
            lease_id,
            session: session.to_owned(),
        }
    }
    fn revoke(&self, claim: &LeaseClaim) {
        self.0
            .lock()
            .unwrap()
            .remove(&(claim.session.clone(), claim.lease_id.clone()));
    }
}
impl LeaseView for Leases {
    fn active(&self, session: &str, lease: &str, _now_ms: u64) -> bool {
        self.0
            .lock()
            .unwrap()
            .contains(&(session.to_owned(), lease.to_owned()))
    }
}

struct Rig {
    engine: Engine,
    frames: Frames,
    sessions: Vec<String>,
    leases: Leases,
}
fn attach() -> Attachment {
    Attachment {
        workspace: "/work/space".into(),
        profile: "codex_local_tools_v1".into(),
        revision: 1,
    }
}
impl Rig {
    fn new(limit: usize, extra: usize) -> Self {
        let mut source = f8_fixture::v7_with_sessions(extra);
        let conn = migration::prepare(&mut source).unwrap().into_connection();
        let sessions: Vec<String> = conn
            .prepare("SELECT session_id FROM managed_sessions ORDER BY created_sequence")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let registry = Registry::new(wire(AUTH)).unwrap().with_test_limit(limit);
        Self {
            engine: Engine::new(conn, wire(AUTH), registry, None).unwrap(),
            frames: Arc::default(),
            sessions,
            leases: Leases::default(),
        }
    }
    fn expect(&self, session: &str) -> (u64, u64) {
        self.engine
            .connection()
            .query_row(
                "SELECT generation,revision FROM managed_runtimes WHERE managed_session_id=?1",
                [session],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
            )
            .unwrap_or((0, 0))
    }
    fn start_request(&self, session: &str, intent: &str) -> Request {
        let (generation, revision) = self.expect(session);
        request(
            "runtime/start",
            json!({"intent_id":intent,"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":session,"expected_generation":generation,"expected_revision":revision}),
        )
    }
    fn stop_request(&self, session: &str, instance: Option<&str>) -> Request {
        let (generation, revision) = self.expect(session);
        request(
            "runtime/stop",
            json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":session,"expected_generation":generation,"expected_revision":revision,
            "expected_runtime_instance_id":instance}),
        )
    }
    fn start(&mut self, session: &str, spawn: &mut Spawn) -> ControlResult<RuntimeRef> {
        let r = self.start_request(session, &uuid());
        let o = self.engine.start(&r, spawn)?;
        Ok(reference(&o))
    }
    /// Starts and completes the handshake.
    fn ready(&mut self, session: &str, spawn: &mut Spawn) -> RuntimeRef {
        let r = self.start(session, spawn).unwrap();
        self.engine
            .complete_handshake(&r, "0.159.0", attach())
            .unwrap();
        r
    }
    fn count_rows(&self) -> (i64, i64, i64, i64) {
        let n = |t: &str| {
            self.engine
                .connection()
                .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        };
        (
            n("control_events"),
            n("processed_intents"),
            n("managed_runtimes"),
            n("managed_runtime_launches"),
        )
    }
    fn frames_for(&self, session: &str) -> usize {
        self.frames
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| s == session)
            .count()
    }
    fn drain(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self
            .engine
            .live
            .values()
            .any(|l| l.phase == Phase::Stopping)
        {
            self.engine.poll(Instant::now());
            assert!(Instant::now() < deadline, "stop never completed");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
fn request(method: &str, params: Value) -> Request {
    Request::parse(
        &serde_json::to_vec(&json!({"schema_version":2,"request_id":uuid(),
            "authority_instance_id":AUTH,"method":method,"params":params}))
        .unwrap(),
    )
    .unwrap()
}
fn reference(o: &Outcome) -> RuntimeRef {
    RuntimeRef {
        schema: 2,
        authority_instance_id: wire(AUTH),
        managed_session_id: o.managed_session_id.clone(),
        runtime_instance_id: o.runtime_instance_id.clone().unwrap(),
        generation: o.generation,
    }
}
fn turn(rig: &mut Rig, r: &RuntimeRef, now: u64) -> (LeaseClaim, TurnTicket) {
    let claim = rig.leases.grant(r.managed_session_id.as_str());
    let ticket = rig
        .engine
        .admit_turn(&r.fence(), &claim, &attach(), &uuid(), &rig.leases, now)
        .unwrap();
    (claim, ticket)
}

#[test]
fn f8_01_a_lease_cannot_drive_another_sessions_runtime() {
    let mut rig = Rig::new(2, 1);
    let mut sp = Spawn::new(&rig.frames);
    let (a, b) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    let (ra, rb) = (rig.ready(&a, &mut sp), rig.ready(&b, &mut sp));
    let lease_a = rig.leases.grant(&a);
    let before = rig.count_rows();
    // A's lease presented with B's fence, and B's own session id claimed in the lease.
    assert_eq!(
        rig.engine
            .admit_turn(&rb.fence(), &lease_a, &attach(), &uuid(), &rig.leases, 0),
        Err(E::GateDenied)
    );
    let forged = LeaseClaim {
        session: b.clone(),
        ..lease_a.clone()
    };
    assert_eq!(
        rig.engine
            .admit_turn(&rb.fence(), &forged, &attach(), &uuid(), &rig.leases, 0),
        Err(E::GateDenied)
    );
    assert_eq!(
        (
            rig.engine.active_turn(&b),
            rig.frames_for(&b),
            rig.count_rows()
        ),
        (None, 0, before)
    );
    // A's own admission is unaffected, and a frame ticket for A cannot be replayed into B.
    let ticket = rig
        .engine
        .admit_turn(&ra.fence(), &lease_a, &attach(), &uuid(), &rig.leases, 0)
        .unwrap();
    let mut crossed = ticket.clone();
    crossed.reference = rb.clone();
    assert_eq!(
        rig.engine.write_frame(&crossed, b"x", &rig.leases, 0),
        Err(E::StaleRevision)
    );
    assert_eq!(rig.frames_for(&b), 0);
}

#[test]
fn f8_02_identical_provider_request_ids_never_cross_sessions() {
    let mut rig = Rig::new(2, 1);
    let mut sp = Spawn::new(&rig.frames);
    let (a, b) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    let (ra, rb) = (rig.ready(&a, &mut sp), rig.ready(&b, &mut sp));
    turn(&mut rig, &ra, 0);
    turn(&mut rig, &rb, 0);
    rig.engine.approval_requested(&ra, "req-1", "fp-a").unwrap();
    rig.engine.approval_requested(&rb, "req-1", "fp-b").unwrap();
    // The same id twice in one runtime is a conflict; across runtimes it is simply distinct.
    assert_eq!(
        rig.engine.approval_requested(&ra, "req-1", "fp-a"),
        Err(E::IntentConflict)
    );
    // A's answer can only carry A's fingerprint and only reaches A's worker.
    assert_eq!(
        rig.engine
            .answer_approval(&ra.fence(), "req-1", "fp-b", b"no"),
        Err(E::GateDenied)
    );
    rig.engine
        .answer_approval(&ra.fence(), "req-1", "fp-a", b"yes-a")
        .unwrap();
    assert_eq!((rig.frames_for(&a), rig.frames_for(&b)), (1, 0));
    assert_eq!(rig.engine.pending_approvals(&b), 1);
    assert_eq!(
        rig.engine
            .answer_approval(&ra.fence(), "req-1", "fp-a", b"again"),
        Err(E::ApprovalNotFound)
    );
    rig.engine
        .answer_approval(&rb.fence(), "req-1", "fp-b", b"yes-b")
        .unwrap();
    assert_eq!(rig.frames_for(&b), 1);
}

#[test]
fn f8_03_stale_generation_traffic_is_refused_after_a_relaunch() {
    let mut rig = Rig::new(1, 0);
    let mut sp = Spawn::new(&rig.frames);
    let a = rig.sessions[0].clone();
    let old = rig.ready(&a, &mut sp);
    turn(&mut rig, &old, 0);
    rig.engine.approval_requested(&old, "req-9", "fp").unwrap();
    let stop = rig.stop_request(&a, Some(old.runtime_instance_id.as_str()));
    rig.engine.stop(&stop, Instant::now()).unwrap();
    rig.drain();
    let new = rig.ready(&a, &mut sp);
    assert_eq!(new.generation, old.generation + 1);
    assert_ne!(new.runtime_instance_id, old.runtime_instance_id);
    // A late catalogue cursor / binding reply / approval answer from generation n.
    assert_eq!(
        rig.engine.provider_event(&old, 10, 0),
        Err(E::StaleRevision)
    );
    assert_eq!(
        rig.engine
            .answer_approval(&old.fence(), "req-9", "fp", b"late"),
        Err(E::StaleRevision)
    );
    assert_eq!(
        rig.engine.approval_requested(&old, "req-9", "fp"),
        Err(E::StaleRevision)
    );
    let claim = rig.leases.grant(&a);
    assert_eq!(
        rig.engine
            .admit_turn(&old.fence(), &claim, &attach(), &uuid(), &rig.leases, 0),
        Err(E::StaleRevision)
    );
    assert_eq!(rig.frames_for(&a), 0);
    assert_eq!(rig.engine.pending_approvals(&a), 0);
}

#[test]
fn f8_04_an_unknown_outcome_is_never_retried_through_a_new_generation() {
    let mut rig = Rig::new(1, 0);
    let mut sp = Spawn::new(&rig.frames);
    let a = rig.sessions[0].clone();
    let old = rig.ready(&a, &mut sp);
    turn(&mut rig, &old, 0);
    rig.engine.approval_requested(&old, "req-1", "fp").unwrap();
    // Crash with an accepted action in flight: the launch ends, its approval is invalidated.
    rig.engine.begin_stop_internal(&a, "child_exited", None);
    rig.drain();
    let new = rig.ready(&a, &mut sp);
    assert_eq!(
        (
            rig.engine.pending_approvals(&a),
            rig.engine.active_turn(&a),
            rig.frames_for(&a)
        ),
        (0, None, 0)
    );
    assert_eq!(
        rig.engine
            .answer_approval(&new.fence(), "req-1", "fp", b"retry"),
        Err(E::ApprovalNotFound)
    );
    assert_eq!(rig.frames_for(&a), 0);
    validate_v8(rig.engine.connection()).unwrap();
}

#[test]
fn f8_05_a_lease_revoked_after_admission_writes_nothing_while_b_continues() {
    let mut rig = Rig::new(2, 1);
    let mut sp = Spawn::new(&rig.frames);
    let (a, b) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    let (ra, rb) = (rig.ready(&a, &mut sp), rig.ready(&b, &mut sp));
    let (lease_a, ta) = turn(&mut rig, &ra, 0);
    let (_lb, tb) = turn(&mut rig, &rb, 0);
    rig.engine
        .write_frame(&ta, b"first", &rig.leases, 1)
        .unwrap();
    rig.leases.revoke(&lease_a);
    assert_eq!(
        rig.engine.write_frame(&ta, b"second", &rig.leases, 2),
        Err(E::GateDenied)
    );
    rig.engine
        .write_frame(&tb, b"b-still-admitted", &rig.leases, 2)
        .unwrap();
    assert_eq!((rig.frames_for(&a), rig.frames_for(&b)), (1, 1));
}

#[test]
fn f8_06_replaying_old_intents_after_runtime_loss_revives_nothing() {
    let mut rig = Rig::new(1, 0);
    let mut sp = Spawn::new(&rig.frames);
    let a = rig.sessions[0].clone();
    let intent = uuid();
    let r = rig.start_request(&a, &intent);
    let first = rig.engine.start(&r, &mut sp).unwrap();
    let old = reference(&first);
    rig.engine
        .complete_handshake(&old, "0.159.0", attach())
        .unwrap();
    let (claim, _) = turn(&mut rig, &old, 0);
    rig.engine.begin_stop_internal(&a, "child_exited", None);
    rig.drain();
    let before = (rig.count_rows(), sp.spawned);
    // The identical start intent returns its original receipt: no launch, no admission.
    let again = rig.engine.start(&r, &mut sp).unwrap();
    assert!(again.replayed && !again.reserved && again.generation == first.generation);
    assert_eq!((rig.count_rows(), sp.spawned), before);
    assert!(!rig.engine.is_live(&a));
    rig.leases.revoke(&claim);
    assert_eq!(
        rig.engine
            .admit_turn(&old.fence(), &claim, &attach(), &uuid(), &rig.leases, 0),
        Err(E::StaleRevision)
    );
}

#[test]
fn f8_07_capacity_is_a_fixed_immediate_refusal_including_stopping_slots() {
    let mut rig = Rig::new(1, 7);
    let engine = std::mem::replace(
        &mut rig.engine,
        Engine::new(
            Connection::open_in_memory().unwrap(),
            wire(AUTH),
            Registry::new(wire(AUTH)).unwrap(),
            None,
        )
        .unwrap(),
    );
    let engine = Arc::new(Mutex::new(engine));
    let frames = rig.frames.clone();
    let spawned = Arc::new(Mutex::new(0usize));
    let handles: Vec<_> = rig
        .sessions
        .clone()
        .into_iter()
        .map(|session| {
            let (engine, frames, spawned) = (engine.clone(), frames.clone(), spawned.clone());
            std::thread::spawn(move || {
                let mut sp = Spawn::new(&frames);
                let mut e = engine.lock().unwrap();
                let (generation, revision) = e
                    .connection()
                    .query_row(
                        "SELECT generation,revision FROM managed_runtimes WHERE managed_session_id=?1",
                        [&session],
                        |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
                    )
                    .unwrap_or((0, 0));
                let r = request(
                    "runtime/start",
                    json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
                    "managed_session_id":session,"expected_generation":generation,"expected_revision":revision}),
                );
                let result = e.start(&r, &mut sp);
                *spawned.lock().unwrap() += sp.spawned;
                result.err()
            })
        })
        .collect();
    let results: Vec<Option<E>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_none()).count(), 1);
    assert!(results.iter().flatten().all(|e| *e == E::CapacityExceeded));
    assert_eq!(*spawned.lock().unwrap(), 1);
    let mut engine = Arc::try_unwrap(engine).ok().unwrap().into_inner().unwrap();
    let launches: i64 = engine
        .connection()
        .query_row("SELECT count(*) FROM managed_runtime_launches", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(launches, 1, "a refused start reserves nothing");
    // A stopping launch still consumes the slot until it is reaped.
    let live = engine.live.keys().next().unwrap().clone();
    let other = rig.sessions.iter().find(|s| **s != live).unwrap().clone();
    engine.begin_stop_internal(&live, "stopped", None);
    let mut sp = Spawn::new(&frames);
    let request_other = request(
        "runtime/start",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":other,
        "expected_generation":0,"expected_revision":0}),
    );
    assert_eq!(
        engine.start(&request_other, &mut sp).err(),
        Some(E::CapacityExceeded)
    );
    assert_eq!(sp.spawned, 0);
    engine.poll(Instant::now());
    engine.start(&request_other, &mut sp).unwrap();
}

#[test]
fn f8_08_staging_collisions_leave_the_other_runtimes_catalogue_untouched() {
    let dir = std::env::temp_dir().join(format!("wyss-f8-eng-{}", uuid()));
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    let mut rig = Rig::new(2, 1);
    rig.engine.staging = Some(StagingRoot::open(&dir).unwrap());
    let (a, b) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    let mut sp = Spawn::new(&rig.frames);
    sp.catalogue = true;
    let rb = rig.ready(&b, &mut sp);
    let b_file = dir
        .join(rb.runtime_instance_id.as_str())
        .join("catalogue.json");
    let bytes = std::fs::read(&b_file).unwrap();
    let ino = std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&b_file).unwrap());
    // A's spawn fails after staging: its directory is removed, B's catalogue is unchanged.
    let mut failing = Spawn::new(&rig.frames);
    failing.catalogue = true;
    failing.fail = true;
    assert_eq!(rig.start(&a, &mut failing).err(), Some(E::ProviderRejected));
    assert_eq!(std::fs::read(&b_file).unwrap(), bytes);
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&b_file).unwrap()),
        ino
    );
    let entries = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(entries, 1, "only B's private launch directory remains");
    assert!(!rig.engine.registry.has_free_slot() || rig.engine.registry.active_slots() == 1);
    // The failed attempt consumed its generation; the retry gets the next one and a new directory.
    let ra = rig.ready(&a, &mut sp);
    assert_eq!(ra.generation, 2);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    assert_eq!(std::fs::read(&b_file).unwrap(), bytes);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn f8_09_a_flood_and_a_blocked_shutdown_cannot_delay_bs_control() {
    let mut rig = Rig::new(2, 1);
    let mut sp = Spawn::new(&rig.frames);
    let (a, b) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    sp.script = Some("trap '' TERM; while true; do sleep 0.05; done".into());
    sp.grace = Grace {
        eof: Duration::from_millis(400),
        term: Duration::from_millis(400),
        kill: Duration::from_secs(3),
    };
    let ra = rig.ready(&a, &mut sp);
    sp.script = None;
    let _rb = rig.ready(&b, &mut sp);
    // A's shutdown cannot complete soon (it ignores EOF and TERM); A also floods its lane.
    rig.engine.begin_stop_internal(&a, "stopped", None);
    let _ = ra;
    let mut worst_wait = 0;
    for tick in 0..20u64 {
        while rig
            .engine
            .scheduler_mut()
            .push(&a, Class::Data, 100, vec![0], tick * 10)
            .is_ok()
        {}
        let started = Instant::now();
        rig.engine.poll(Instant::now());
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "cleanup must not block the writer"
        );
        rig.engine
            .scheduler_mut()
            .push(&b, Class::Control, 64, b"revoke".to_vec(), tick * 10)
            .unwrap();
        let out = rig.engine.scheduler_mut().tick(tick * 10);
        let revoke = out
            .iter()
            .find(|d| d.runtime == b && d.class == Class::Control)
            .expect("B's control every quantum");
        worst_wait = worst_wait.max(revoke.waited_ms);
        rig.engine.scheduler_mut().ack(&b);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(worst_wait, 0);
    // Memory is bounded by the reserved lanes however long the flood lasted.
    let ceiling = rig.engine.scheduler_mut().memory_ceiling(2);
    assert!(ceiling <= 2 * (524_288 + 32_768));
    rig.drain();
    assert!(!rig.engine.is_live(&a));
}

#[test]
fn f8_10_a_budget_violation_stops_only_a_and_preserves_bs_approval() {
    let mut rig = Rig::new(2, 1);
    let mut sp = Spawn::new(&rig.frames);
    let (a, b) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    let (ra, rb) = (rig.ready(&a, &mut sp), rig.ready(&b, &mut sp));
    let (_la, ta) = turn(&mut rig, &ra, 0);
    let (_lb, tb) = turn(&mut rig, &rb, 0);
    rig.engine.approval_requested(&rb, "wait", "fp-b").unwrap();
    // A sends a provider frame over the contract's frame bound.
    assert_eq!(
        rig.engine.provider_event(&ra, 262_145, 5),
        Err(E::CapacityExceeded)
    );
    rig.drain();
    assert!(!rig.engine.is_live(&a));
    assert!(rig.engine.is_live(&b));
    assert_eq!(
        (rig.engine.pending_approvals(&b), rig.engine.active_turn(&b)),
        (1, Some(tb.turn_id.as_str()))
    );
    assert_eq!(
        rig.engine.write_frame(&ta, b"x", &rig.leases, 6),
        Err(E::StaleRevision)
    );
    rig.engine
        .answer_approval(&rb.fence(), "wait", "fp-b", b"accept")
        .unwrap();
    rig.engine.provider_event(&rb, 100, 7).unwrap();
    // The durable record names the exact budget key; the stream stays valid.
    let exit: String = rig.engine.connection().query_row(
        "SELECT json_extract(envelope,'$.payload.budget_failure') FROM control_events WHERE json_extract(envelope,'$.event_type')='authority.runtime.instance_exited' ORDER BY sequence DESC LIMIT 1",
        [], |r| r.get(0)).unwrap();
    assert_eq!(exit, "provider_frame_bytes");
    validate_v8(rig.engine.connection()).unwrap();
}

#[test]
fn f8_11_one_turn_and_one_approval_per_session_but_distinct_sessions_coexist() {
    let mut rig = Rig::new(2, 1);
    let mut sp = Spawn::new(&rig.frames);
    let (a, b) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    let (ra, rb) = (rig.ready(&a, &mut sp), rig.ready(&b, &mut sp));
    turn(&mut rig, &ra, 0);
    turn(&mut rig, &rb, 0);
    let second = rig.leases.grant(&a);
    assert_eq!(
        rig.engine
            .admit_turn(&ra.fence(), &second, &attach(), &uuid(), &rig.leases, 0),
        Err(E::ActiveTurnExists)
    );
    rig.engine.approval_requested(&ra, "r1", "fp1").unwrap();
    assert_eq!(
        rig.engine.approval_requested(&ra, "r2", "fp2"),
        Err(E::CapacityExceeded)
    );
    rig.engine.approval_requested(&rb, "r1", "fp1").unwrap();
    assert_eq!(
        (
            rig.engine.pending_approvals(&a),
            rig.engine.pending_approvals(&b)
        ),
        (1, 1)
    );
}

#[test]
fn f8_12_protocol_version_confusion_never_selects_an_implicit_session() {
    let mut rig = Rig::new(2, 1);
    let mut sp = Spawn::new(&rig.frames);
    let (a, b) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    let now = Instant::now();
    let before = rig.count_rows();
    let start = |v: Value| serde_json::to_vec(&v).unwrap();
    let body = |version: u64, authority: &str, params: Value| {
        start(
            json!({"schema_version":version,"request_id":uuid(),"authority_instance_id":authority,
            "method":"runtime/start","params":params}),
        )
    };
    let params = |session: Option<&str>| {
        let mut p = json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
            "expected_generation":0,"expected_revision":0});
        if let Some(s) = session {
            p["managed_session_id"] = json!(s);
        }
        p
    };
    for (bytes, expected) in [
        (
            body(1, AUTH, params(Some(&a))),
            E::ProtocolVersionUnsupported,
        ),
        (
            body(3, AUTH, params(Some(&a))),
            E::ProtocolVersionUnsupported,
        ),
        (body(2, AUTH, params(None)), E::InvalidRequest),
        (
            body(2, "00000000-0000-4000-8000-0000000000ff", params(Some(&a))),
            E::CorrelationError,
        ),
    ] {
        assert_eq!(
            rig.engine.handle(&bytes, &mut sp, now).err(),
            Some(expected)
        );
    }
    assert_eq!((rig.count_rows(), sp.spawned), (before, 0));
    // The explicit session id is the only selector: B starts while A stays untouched.
    let ok = rig
        .engine
        .handle(&body(2, AUTH, params(Some(&b))), &mut sp, now)
        .unwrap();
    assert_eq!(ok.managed_session_id.as_str(), b);
    assert!(rig.engine.is_live(&b) && !rig.engine.is_live(&a));
    // Fences cannot be swapped between sessions either.
    let rb = reference(&ok);
    let crossed = RuntimeFence {
        managed_session_id: ManagedSessionId::parse(&a).unwrap(),
        ..rb.fence()
    };
    let claim = rig.leases.grant(&a);
    assert!(
        rig.engine
            .admit_turn(&crossed, &claim, &attach(), &uuid(), &rig.leases, 0)
            .is_err()
    );
}

#[test]
fn f8_13_a_crash_at_every_launch_stage_never_reuses_a_generation_or_fabricates_ready() {
    for stage in [
        LaunchStage::AfterReservation,
        LaunchStage::AfterSpawn,
        LaunchStage::AfterHandshake,
    ] {
        let mut rig = Rig::new(1, 0);
        rig.engine = std::mem::replace(
            &mut rig.engine,
            Engine::new(
                Connection::open_in_memory().unwrap(),
                wire(AUTH),
                Registry::new(wire(AUTH)).unwrap(),
                None,
            )
            .unwrap(),
        )
        .with_stage_hook(move |s| {
            if s == stage {
                Err(E::InternalError)
            } else {
                Ok(())
            }
        });
        let a = rig.sessions[0].clone();
        let mut sp = Spawn::new(&rig.frames);
        sp.script = Some("trap '' TERM; while true; do sleep 0.05; done".into());
        let first = rig.start(&a, &mut sp);
        let first = match stage {
            LaunchStage::AfterHandshake => {
                let r = first.unwrap();
                assert!(
                    rig.engine
                        .complete_handshake(&r, "0.159.0", attach())
                        .is_err()
                );
                r
            }
            _ => {
                assert_eq!(first.err(), Some(E::InternalError));
                // Recover the reserved identity from the durable row, not from the failed call.
                let (instance, generation): (String, i64) = rig
                    .engine
                    .connection()
                    .query_row(
                        "SELECT runtime_instance_id,generation FROM managed_runtime_launches",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                RuntimeRef {
                    schema: 2,
                    authority_instance_id: wire(AUTH),
                    managed_session_id: ManagedSessionId::parse(&a).unwrap(),
                    runtime_instance_id: AgentRuntimeInstanceId::parse(&instance).unwrap(),
                    generation: generation as u64,
                }
            }
        };
        // Never ready, no implicit restart, and an owned child was stopped, not abandoned.
        assert!(
            rig.engine
                .registry
                .observe(&wire(AUTH), &first.fence())
                .is_err(),
            "{stage:?}"
        );
        let spawned_before = sp.spawned;
        rig.drain();
        assert_eq!(sp.spawned, spawned_before, "no implicit restart");
        assert!(
            !rig.engine.is_live(&a) && rig.engine.registry.active_slots() == 0,
            "{stage:?}"
        );
        validate_v8(rig.engine.connection()).unwrap();
        // The next explicit start consumes the next generation with a brand-new identity.
        rig.engine.hook = None;
        let mut again = Spawn::new(&rig.frames);
        let second = rig.start(&a, &mut again).unwrap();
        assert_eq!(second.generation, first.generation + 1, "{stage:?}");
        assert_ne!(second.runtime_instance_id, first.runtime_instance_id);
        rig.engine
            .complete_handshake(&second, "0.159.0", attach())
            .unwrap();
        validate_v8(rig.engine.connection()).unwrap();
    }
}

#[test]
fn f8_14_generation_overflow_refuses_without_spawning_and_replay_is_unchanged() {
    let mut rig = Rig::new(1, 0);
    let mut sp = Spawn::new(&rig.frames);
    let a = rig.sessions[0].clone();
    let intent = uuid();
    let r = rig.start_request(&a, &intent);
    let first = rig.engine.start(&r, &mut sp).unwrap();
    rig.engine.begin_stop_internal(&a, "child_exited", None);
    rig.drain();
    let c = rig.engine.connection();
    let triggers: Vec<String> = c.prepare("SELECT sql FROM sqlite_master WHERE type='trigger' AND name='managed_runtimes_transition'").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    c.execute_batch("DROP TRIGGER managed_runtimes_transition")
        .unwrap();
    c.execute(
        "UPDATE managed_runtimes SET generation=9223372036854775807",
        [],
    )
    .unwrap();
    c.execute_batch(&triggers[0]).unwrap();
    let before = (rig.count_rows(), sp.spawned);
    let (generation, revision) = rig.expect(&a);
    let overflow = request(
        "runtime/start",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":a,"expected_generation":generation,"expected_revision":revision}),
    );
    assert_eq!(
        rig.engine.start(&overflow, &mut sp).err(),
        Some(E::CapacityExceeded)
    );
    assert_eq!((rig.count_rows(), sp.spawned), before);
    // Prior receipts replay unchanged.
    let again = rig.engine.start(&r, &mut sp).unwrap();
    assert!(again.replayed && again.generation == first.generation);
    assert_eq!((rig.count_rows(), sp.spawned), before);
}

#[test]
fn f8_15_a_exiting_with_a_stuck_stop_never_blocks_b_or_the_writer() {
    let mut rig = Rig::new(2, 1);
    let mut sp = Spawn::new(&rig.frames);
    let (a, b) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    sp.script = Some("trap '' TERM; while true; do sleep 0.05; done".into());
    sp.grace = Grace {
        eof: Duration::from_millis(500),
        term: Duration::from_millis(500),
        kill: Duration::from_secs(3),
    };
    let _ra = rig.ready(&a, &mut sp);
    sp.script = None;
    let rb = rig.ready(&b, &mut sp);
    let (_lb, tb) = turn(&mut rig, &rb, 0);
    rig.engine.begin_stop_internal(&a, "stopped", None);
    for _ in 0..5 {
        let started = Instant::now();
        rig.engine.poll(Instant::now());
        assert!(started.elapsed() < Duration::from_millis(100));
        // B keeps its state, its turn and its writes while A's stop is still escalating.
        rig.engine
            .write_frame(&tb, b"still-working", &rig.leases, 1)
            .unwrap();
        assert!(rig.engine.is_live(&a), "A's grace has not completed");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(rig.frames_for(&b), 5);
    rig.drain();
    assert!(!rig.engine.is_live(&a) && rig.engine.is_live(&b));
}

/// Simulates audit storage exhaustion: every durable event append fails like SQLITE_FULL.
/// (A real full filesystem is not exercised; a partially filled page cannot be forced to fail.)
fn fill(c: &Connection) {
    c.execute_batch(
        "CREATE TEMP TRIGGER f8_storage_full BEFORE INSERT ON main.control_events BEGIN SELECT RAISE(ABORT,'database or disk is full'); END",
    )
    .unwrap();
}
fn unfill(c: &Connection) {
    c.execute_batch("DROP TRIGGER temp.f8_storage_full")
        .unwrap();
}

#[test]
fn f8_16_audit_storage_full_fails_closed_and_the_unknown_stays_visible() {
    // At admission: nothing is reserved, spawned or held.
    let mut rig = Rig::new(1, 0);
    let mut sp = Spawn::new(&rig.frames);
    let a = rig.sessions[0].clone();
    fill(rig.engine.connection());
    let r = rig.start_request(&a, &uuid());
    let before = rig.count_rows();
    assert_eq!(
        rig.engine.start(&r, &mut sp).err(),
        Some(E::StorageUnavailable)
    );
    assert_eq!(
        (
            rig.count_rows(),
            sp.spawned,
            rig.engine.registry.active_slots()
        ),
        (before, 0, 0)
    );
    unfill(rig.engine.connection());

    // At readiness: the runtime is never reported ready and its cleanup still completes.
    let started = rig.engine.start(&r, &mut sp).unwrap();
    let reference = reference(&started);
    fill(rig.engine.connection());
    assert!(
        rig.engine
            .complete_handshake(&reference, "0.159.0", attach())
            .is_err()
    );
    assert!(
        rig.engine
            .registry
            .observe(&wire(AUTH), &reference.fence())
            .is_err()
    );
    rig.drain();
    assert!(!rig.engine.is_live(&a));
    assert_eq!(
        rig.engine.unrecorded_exits(),
        1,
        "the exit is retried, not lost"
    );
    // When storage returns, the retained exit is recorded exactly once and the image validates.
    unfill(rig.engine.connection());
    rig.engine.poll(Instant::now());
    assert_eq!(rig.engine.unrecorded_exits(), 0);
    validate_v8(rig.engine.connection()).unwrap();
}

#[test]
fn f8_17_only_the_owned_child_is_signalled_and_descendants_are_reported_not_killed() {
    let mut rig = Rig::new(1, 0);
    let mut sp = Spawn::new(&rig.frames);
    let a = rig.sessions[0].clone();
    let mut bystander = Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    // The descendant announces its own PID to a file this test created: no process is ever
    // found by name or command line, neither by the code under test nor by the test.
    let marker = std::env::temp_dir().join(format!("wyss-f8-17-{}", uuid()));
    sp.script = Some(format!(
        "sleep 30 & echo $! > {}; trap '' TERM; while true; do sleep 0.05; done",
        marker.display()
    ));
    let r = rig.ready(&a, &mut sp);
    let child_pid = rig.engine.live[&a]
        .child
        .as_ref()
        .unwrap()
        .pid_for_diagnostics();
    assert!((0..200).any(|_| {
        std::thread::sleep(Duration::from_millis(10));
        rig.engine.live[&a]
            .child
            .as_ref()
            .unwrap()
            .descendant_count()
            >= 1
            && marker.exists()
            && !std::fs::read_to_string(&marker)
                .unwrap_or_default()
                .trim()
                .is_empty()
    }));
    let grandchild: i32 = std::fs::read_to_string(&marker)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // Pin the descendant now, by pidfd, so the test can later clean up exactly that process.
    // SAFETY: integer-only syscall; the descriptor is owned immediately below.
    let pinned = unsafe { nix::libc::syscall(nix::libc::SYS_pidfd_open, grandchild, 0) };
    assert!(pinned >= 0);
    let stop = rig.stop_request(&a, Some(r.runtime_instance_id.as_str()));
    rig.engine.stop(&stop, Instant::now()).unwrap();
    rig.drain();
    assert!(!std::path::Path::new(&format!("/proc/{child_pid}/stat")).exists());
    // The unrelated process sharing the executable is untouched, and so is the descendant: the
    // containment gap is reported (descendant_count above), never "fixed" by widening cleanup.
    assert!(bystander.try_wait().unwrap().is_none());
    assert!(std::path::Path::new(&format!("/proc/{grandchild}/stat")).exists());
    // SAFETY: signals only the descendant this test pinned through its pidfd, then closes it.
    unsafe {
        nix::libc::syscall(
            nix::libc::SYS_pidfd_send_signal,
            pinned as i32,
            nix::libc::SIGKILL,
            std::ptr::null::<nix::libc::siginfo_t>(),
            0,
        );
        nix::libc::close(pinned as i32);
    }
    bystander.kill().unwrap();
    bystander.wait().unwrap();
    std::fs::remove_file(&marker).ok();
}

#[test]
fn f8_18_a_workspace_or_profile_replaced_during_dispatch_writes_nothing() {
    let mut rig = Rig::new(1, 0);
    let mut sp = Spawn::new(&rig.frames);
    let a = rig.sessions[0].clone();
    let r = rig.ready(&a, &mut sp);
    let (_claim, ticket) = turn(&mut rig, &r, 0);
    rig.engine
        .write_frame(&ticket, b"ok", &rig.leases, 1)
        .unwrap();
    for next in [
        Attachment {
            workspace: "/other".into(),
            ..attach()
        },
        Attachment {
            profile: "codex_inference_v1".into(),
            ..attach()
        },
        Attachment {
            revision: 2,
            ..attach()
        },
    ] {
        rig.engine.replace_attachment(&r.fence(), next).unwrap();
        assert_eq!(
            rig.engine.write_frame(&ticket, b"blocked", &rig.leases, 2),
            Err(E::GateDenied)
        );
        // Restoring the exact accepted context makes the same ticket valid again.
        rig.engine.replace_attachment(&r.fence(), attach()).unwrap();
    }
    assert_eq!(rig.frames_for(&a), 1);
    // A new admission under a changed context is refused outright.
    rig.engine
        .replace_attachment(
            &r.fence(),
            Attachment {
                workspace: "/other".into(),
                ..attach()
            },
        )
        .unwrap();
    let claim = rig.leases.grant(&a);
    rig.engine.begin_stop_internal(&a, "stopped", None);
    rig.drain();
    assert!(
        rig.engine
            .admit_turn(&r.fence(), &claim, &attach(), &uuid(), &rig.leases, 0)
            .is_err()
    );
}

use crate::contracts::identities::{AgentRuntimeInstanceId, ManagedSessionId};
