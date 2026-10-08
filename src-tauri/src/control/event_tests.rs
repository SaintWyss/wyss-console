use super::*;
use crate::{
    adapters::codex::{CodexThreadId, CodexTurnId},
    contracts::identities::{AgentRuntimeInstanceId, ManagedApprovalId, ManagedTurnId},
    control::runtime::MaterializationStage,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{Arc, Mutex},
};

const PROMPT: &str = "WYSS_2J_PROMPT_SENTINEL_7c1e";
const FAULT: &str = "CREATE TEMP TRIGGER injected BEFORE INSERT ON control_events BEGIN SELECT RAISE(ABORT,'injected'); END";

struct TestClock(Mutex<OffsetDateTime>);
impl leases::Clock for TestClock {
    fn now(&self) -> OffsetDateTime {
        *self.0.lock().unwrap()
    }
}
fn digest(seed: &str) -> String {
    format!("{:x}", Sha256::digest(seed.as_bytes()))
}
fn seq(n: u64) -> Option<Sequence> {
    (n > 0).then(|| n.to_string().parse().unwrap())
}
/// Every row of every table, in a stable order: the whole canonical and audit state.
fn snapshot(c: &Connection) -> String {
    let tables: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut out = String::new();
    for table in tables {
        let mut q = c
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
            .unwrap();
        let columns = q.column_count();
        let mut rows = q.query([]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            out.push_str(&table);
            for i in 0..columns {
                out.push_str(&format!("|{:?}", r.get_ref(i).unwrap()));
            }
            out.push('\n');
        }
    }
    out
}
fn read(
    s: &mut Store,
    after: u64,
    limit: u16,
) -> ControlResult<(Vec<EventEnvelopeV1>, Option<Sequence>)> {
    let stream_id = s.status()?.stream_id;
    match s.apply(&Operation::AuditRead(AuditRead {
        stream_id,
        after_sequence: seq(after),
        limit,
    }))? {
        ResultValue::Audit { events, next } => Ok((events, next)),
        _ => panic!("audit expected"),
    }
}
/// The whole committed history through the public replay contract, in pages of `limit`.
fn replay_from(s: &mut Store, mut after: u64, limit: u16) -> Vec<EventEnvelopeV1> {
    let mut all = Vec::new();
    loop {
        let (events, next) = read(s, after, limit).unwrap();
        assert!(events.len() <= usize::from(limit));
        for e in &events {
            after += 1;
            assert_eq!(e.sequence().get(), after, "contiguous, no gap, no repeat");
        }
        all.extend(events);
        match next {
            // The continuation cursor is exactly the last delivered sequence.
            Some(n) => assert_eq!(n.get(), after),
            None => return all,
        }
    }
}
fn bytes(events: &[EventEnvelopeV1]) -> Vec<u8> {
    events
        .iter()
        .flat_map(|e| serde_json::to_vec(e).unwrap())
        .collect()
}
fn values(events: &[EventEnvelopeV1]) -> Vec<Value> {
    events
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}
fn head(s: &Store) -> u64 {
    s.head().unwrap()
}
/// An envelope as a newer build or a foreign writer would produce it, numbered next.
fn envelope(c: &Connection, kind: &str, payload: Value, session: Option<&str>) -> (u64, String) {
    let (stream, last): (String, String) = c
        .query_row(
            "SELECT stream_id,last_sequence FROM authority_metadata",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let n = last.parse::<u64>().unwrap() + 1;
    let v = json!({"schema_version":1,"event_id":uuid(),"event_type":kind,"payload_schema_version":3,
        "timestamp":"2026-10-08T00:00:00Z","stream_id":stream,"sequence":n.to_string(),"session_id":session,
        "source":{"component":"wyss_future","instance_id":uuid()},"correlation_id":null,"causation_id":null,
        "payload":payload,"persistence":"durable","sensitivity":"internal","redaction":"none"});
    (n, v.to_string())
}
/// Appends it straight into the log with the head, bypassing this build's append path and
/// its type registry.
fn foreign(c: &Connection, kind: &str, payload: Value, session: Option<&str>) -> u64 {
    let (n, raw) = envelope(c, kind, payload, session);
    c.execute(
        "INSERT INTO control_events SELECT json_extract(?1,'$.event_id'),json_extract(?1,'$.stream_id'),?2,?3,?1",
        params![raw, key(n), session],
    )
    .unwrap();
    c.execute(
        "UPDATE authority_metadata SET last_sequence=?1",
        [n.to_string()],
    )
    .unwrap();
    n
}

struct Life {
    root: PathBuf,
    store: Store,
    clock: Arc<TestClock>,
    runtime: AgentRuntimeInstanceId,
    session: Option<ManagedSessionRecord>,
    lease: Option<ExecutionLease>,
    reservation: Option<bindings::Reservation>,
    binding: Option<SessionBinding>,
    turn: Option<ManagedTurnRecord>,
    approval: Option<ManagedApprovalRecord>,
    other: Option<ManagedSessionRecord>,
    other_lease: Option<ExecutionLease>,
    turns: u32,
}
type Step = (
    &'static str,
    bool,
    Box<dyn Fn(&mut Life) -> ControlResult<()>>,
);
impl Life {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-events-{}", uuid().as_str()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let mut store = Store::open(&root.join("db"), uuid()).unwrap();
        let clock = Arc::new(TestClock(Mutex::new(
            OffsetDateTime::parse("2026-10-08T00:00:00Z", &Rfc3339).unwrap(),
        )));
        store.set_clock(clock.clone());
        Self {
            root,
            store,
            clock,
            runtime: AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap(),
            session: None,
            lease: None,
            reservation: None,
            binding: None,
            turn: None,
            approval: None,
            other: None,
            other_lease: None,
            turns: 0,
        }
    }
    /// A representative full lifecycle: every event type this build writes, once or more.
    fn full() -> Self {
        let mut l = Self::new();
        for (name, _, step) in steps() {
            step(&mut l).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        l
    }
    fn db(&self) -> PathBuf {
        self.root.join("db")
    }
    fn reopen(&mut self) {
        self.store = Store::open(&self.db(), uuid()).unwrap();
        self.store.set_clock(self.clock.clone());
    }
    fn advance(&self, seconds: i64) {
        *self.clock.0.lock().unwrap() += time::Duration::seconds(seconds);
    }
    fn session(&self) -> ManagedSessionRecord {
        self.session.clone().unwrap()
    }
    fn turn_id(&self) -> ManagedTurnId {
        self.turn.as_ref().unwrap().managed_turn_id.clone()
    }
    fn approval_id(&self) -> ManagedApprovalId {
        self.approval.as_ref().unwrap().managed_approval_id.clone()
    }
    fn runtime_mutation(&mut self, start: bool) -> ControlResult<()> {
        let p = RuntimeMutation {
            intent_id: uuid(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
            expected_revision: None,
            safety_profile: None,
            workspace_root: None,
        };
        self.store
            .apply(&if start {
                Operation::RuntimeStart(p)
            } else {
                Operation::RuntimeStop(p)
            })
            .map(|_| ())
    }
    fn prepare(&mut self) -> ControlResult<ManagedSessionRecord> {
        match self.store.apply(&Operation::Prepare(Prepare {
            intent_id: uuid(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
        }))? {
            ResultValue::Session(s) => Ok(s),
            _ => panic!("session expected"),
        }
    }
    fn grant(&mut self, s: &ManagedSessionRecord, ttl: u32) -> ControlResult<ExecutionLease> {
        match self.store.apply(&Operation::LeaseGrant(LeaseGrant {
            intent_id: uuid(),
            session_id: s.managed_session_id.clone(),
            expected_session_revision: 1,
            scopes: vec![
                ExecutionScope::SessionCreate,
                ExecutionScope::SessionResume,
                ExecutionScope::TurnStart,
                ExecutionScope::CommandExecute,
                ExecutionScope::FileChange,
            ],
            ttl_seconds: ttl,
        }))? {
            ResultValue::Lease(l) => Ok(l),
            _ => panic!("lease expected"),
        }
    }
    fn lease(&mut self, renew: bool) -> ControlResult<()> {
        let l = self.lease.clone().unwrap();
        let op = if renew {
            Operation::LeaseRenew(LeaseRenew {
                intent_id: uuid(),
                lease_id: l.lease_id,
                expected_revision: l.revision,
                ttl_seconds: 3600,
            })
        } else {
            Operation::LeaseRevoke(LeaseRevoke {
                intent_id: uuid(),
                lease_id: l.lease_id,
                expected_revision: l.revision,
            })
        };
        match self.store.apply(&op)? {
            ResultValue::Lease(l) => self.lease = Some(l),
            _ => panic!("lease expected"),
        }
        Ok(())
    }
    fn reserve(&mut self, s: &ManagedSessionRecord, create: bool) -> ControlResult<()> {
        let op = if create {
            Operation::BindingCreate(BindingCreate {
                intent_id: uuid(),
                managed_session_id: s.managed_session_id.clone(),
                expected_session_revision: 1,
                workspace_root: self.root.to_str().unwrap().into(),
            })
        } else {
            Operation::BindingResume(BindingResume {
                intent_id: uuid(),
                managed_session_id: s.managed_session_id.clone(),
                expected_session_revision: 1,
            })
        };
        let r = self.store.reserve_binding(&op)?;
        self.store.dispatch_binding(&r)?;
        self.reservation = Some(r);
        Ok(())
    }
    fn thread() -> CodexThreadId {
        CodexThreadId::parse("opaque-durable-thread").unwrap()
    }
    fn commit(&mut self) -> ControlResult<()> {
        let r = self.reservation.clone().unwrap();
        self.binding = Some(
            self.store
                .commit_binding(&r, &Self::thread(), &self.runtime)?,
        );
        Ok(())
    }
    fn admit(&mut self) -> ControlResult<()> {
        let start = TurnStart {
            intent_id: uuid(),
            managed_session_id: self.session().managed_session_id,
            expected_session_revision: 1,
            prompt: format!("{PROMPT} turn"),
        };
        let turn = self.store.reserve_turn(
            &turns::StartFingerprint::compute(&start)?,
            self.binding.as_ref().unwrap(),
            self.lease.as_ref().unwrap(),
            &self.runtime,
        )?;
        self.turns += 1;
        self.turn = Some(turn);
        Ok(())
    }
    fn transition(&mut self, state: TurnState, reason: Option<TurnReason>) -> ControlResult<()> {
        let provider = CodexTurnId::parse(&format!("provider-turn-{}", self.turns)).unwrap();
        self.turn =
            Some(
                self.store
                    .transition_turn(&self.turn_id(), Some(&provider), state, reason)?,
            );
        Ok(())
    }
    fn open(&mut self, item: &str, kind: ApprovalKind) -> ControlResult<()> {
        let structure = match kind {
            ApprovalKind::CommandExecution => ApprovalStructure {
                command_bytes: 12,
                cwd_bytes: 5,
                ..Default::default()
            },
            ApprovalKind::FileChange => ApprovalStructure {
                files: 1,
                diff_bytes: 10,
                ..Default::default()
            },
        };
        self.approval = Some(self.store.open_approval(
            &self.turn_id(),
            &self.runtime,
            kind,
            item,
            &digest(item),
            structure,
        )?);
        Ok(())
    }
    fn decide(&mut self, decision: ApprovalDecision) -> ControlResult<()> {
        let a = self.approval.clone().unwrap();
        match self.store.decide_approval(
            &ApprovalRespond {
                intent_id: uuid(),
                managed_approval_id: a.managed_approval_id,
                expected_revision: a.revision,
                decision,
            },
            &a.action_fingerprint,
        )? {
            approvals::Decided::Fresh(a, _) => self.approval = Some(*a),
            approvals::Decided::Replayed(_) => panic!("fresh decision expected"),
        }
        Ok(())
    }
    fn settle(&mut self, reason: ApprovalReason) -> ControlResult<()> {
        let settled = self.store.settle_approvals(Some(&self.turn_id()), reason)?;
        assert_eq!(settled.len(), 1);
        Ok(())
    }
}
impl Drop for Life {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn step(
    name: &'static str,
    evented: bool,
    f: impl Fn(&mut Life) -> ControlResult<()> + 'static,
) -> Step {
    (name, evented, Box::new(f))
}
fn turn_start(out: &mut Vec<Step>) {
    out.push(step("turn admitted", true, Life::admit));
    out.push(step("turn running", true, |l| {
        l.transition(TurnState::Running, None)
    }));
}
/// The lifecycle as individually retryable steps. A step marked as evented commits exactly
/// one transaction that appends at least one durable event.
fn steps() -> Vec<Step> {
    use ApprovalDecision::{Accept, Cancel, Decline};
    use ApprovalKind::{CommandExecution, FileChange};
    let mut s = vec![
        step("runtime start", true, |l| l.runtime_mutation(true)),
        step("runtime ready", true, |l| {
            l.store.runtime_event("authority.runtime.instance_ready", json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":l.runtime,"runtime_version":"0.159.0","compatibility":"compatible"}))
        }),
        step("prepare", true, |l| {
            l.session = Some(l.prepare()?);
            Ok(())
        }),
        step("lease grant", true, |l| {
            let s = l.session();
            l.lease = Some(l.grant(&s, 3600)?);
            Ok(())
        }),
        step("binding create journal", false, |l| {
            let s = l.session();
            l.reserve(&s, true)?;
            let r = l.reservation.clone().unwrap();
            for stage in [
                MaterializationStage::Started,
                MaterializationStage::Archived,
                MaterializationStage::Unarchived,
            ] {
                l.store.checkpoint_binding(&r, &Life::thread(), stage)?;
            }
            Ok(())
        }),
        step("binding bound", true, Life::commit),
        step("binding resume journal", false, |l| {
            let s = l.session();
            l.reserve(&s, false)
        }),
        step("binding attached", true, Life::commit),
    ];
    turn_start(&mut s);
    s.push(step("command approval requested", true, |l| {
        l.open("item-1", CommandExecution)
    }));
    s.push(step("approval accepted", true, |l| l.decide(Accept)));
    s.push(step("approval confirmed", true, |l| {
        let (id, turn) = (l.approval_id(), l.turn_id());
        l.store
            .confirm_approval(&id, &turn, &l.runtime.clone(), "item-1")
            .map(|_| ())
    }));
    s.push(step("file approval requested", true, |l| {
        l.open("item-2", FileChange)
    }));
    s.push(step("approval declined", true, |l| l.decide(Decline)));
    s.push(step("turn completed", true, |l| {
        l.transition(TurnState::Completed, Some(TurnReason::Completed))
    }));
    turn_start(&mut s);
    s.push(step("approval requested", true, |l| {
        l.open("item-3", CommandExecution)
    }));
    s.push(step("approval cancelled", true, |l| l.decide(Cancel)));
    s.push(step("turn cancel requested", true, |l| {
        let t = l.turn.clone().unwrap();
        l.store
            .cancel_turn_receipt(
                &TurnCancel {
                    intent_id: uuid(),
                    managed_turn_id: t.managed_turn_id,
                    expected_revision: t.revision,
                },
                TurnReason::UserCancel,
            )
            .map(|_| ())
    }));
    s.push(step("turn interrupted", true, |l| {
        l.transition(TurnState::Interrupted, Some(TurnReason::UserCancel))
    }));
    turn_start(&mut s);
    s.push(step("approval requested", true, |l| {
        l.open("item-4", FileChange)
    }));
    s.push(step("approval deadline passes", false, |l| {
        l.advance(APPROVAL_TTL_SECONDS + 1);
        Ok(())
    }));
    s.push(step("approval expired", true, |l| {
        assert_eq!(l.store.expire_approvals()?.len(), 1);
        Ok(())
    }));
    s.push(step("turn failed", true, |l| {
        l.transition(TurnState::Failed, Some(TurnReason::ApprovalExpired))
    }));
    turn_start(&mut s);
    s.push(step("approval requested", true, |l| {
        l.open("item-5", CommandExecution)
    }));
    s.push(step("approval accepted", true, |l| l.decide(Accept)));
    s.push(step("approval indeterminate", true, |l| {
        l.settle(ApprovalReason::RuntimeLost)
    }));
    s.push(step("turn indeterminate", true, |l| {
        l.transition(TurnState::Indeterminate, Some(TurnReason::RuntimeFailed))
    }));
    turn_start(&mut s);
    s.push(step("approval requested", true, |l| {
        l.open("item-6", FileChange)
    }));
    s.push(step("lease renewed", true, |l| l.lease(true)));
    s.push(step("lease revoked", true, |l| l.lease(false)));
    s.push(step("approval invalidated", true, |l| {
        l.settle(ApprovalReason::LeaseRevoked)
    }));
    s.push(step("turn interrupted by revocation", true, |l| {
        l.transition(TurnState::Interrupted, Some(TurnReason::LeaseRevoked))
    }));
    s.push(step("second prepare", true, |l| {
        l.other = Some(l.prepare()?);
        Ok(())
    }));
    s.push(step("second lease grant", true, |l| {
        let s = l.other.clone().unwrap();
        l.other_lease = Some(l.grant(&s, 60)?);
        Ok(())
    }));
    s.push(step("second create journal", false, |l| {
        let s = l.other.clone().unwrap();
        l.reserve(&s, true)
    }));
    s.push(step("operation indeterminate", true, |l| {
        let r = l.reservation.clone().unwrap();
        l.store.fail_binding(&r, true)
    }));
    s.push(step("operation abandoned", true, |l| {
        let p = BindingAbandon {
            intent_id: uuid(),
            managed_session_id: l.other.clone().unwrap().managed_session_id,
            operation_id: l.reservation.clone().unwrap().operation_id,
        };
        l.store.abandon_binding(&p).map(|_| ())
    }));
    s.push(step("second lease deadline passes", false, |l| {
        l.advance(61);
        Ok(())
    }));
    s.push(step("lease expired", true, |l| l.store.expire_due()));
    for first in [true, false] {
        s.push(step("session closed", true, move |l| {
            let s = if first {
                l.session()
            } else {
                l.other.clone().unwrap()
            };
            l.store
                .apply(&Operation::Close(Close {
                    intent_id: uuid(),
                    managed_session_id: s.managed_session_id,
                    expected_revision: 1,
                }))
                .map(|_| ())
        }));
    }
    s.push(step("runtime stop", true, |l| l.runtime_mutation(false)));
    s.push(step("runtime exited", true, |l| {
        l.store.runtime_event("authority.runtime.instance_exited", json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":l.runtime,"exit_kind":"child_exited","expected":true,"desired_state":"stopped"}))
    }));
    s
}
fn kinds(events: &[EventEnvelopeV1]) -> Vec<String> {
    values(events)
        .iter()
        .map(|v| v["event_type"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn lifecycle_writes_exactly_the_registered_event_types_and_reopens() {
    let mut l = Life::full();
    let events = replay_from(&mut l.store, 0, 50);
    let written: BTreeSet<String> = kinds(&events).into_iter().collect();
    let registered: BTreeSet<String> = KNOWN.iter().map(|k| (*k).to_owned()).collect();
    assert_eq!(written, registered);
    // Every one of them is held to its contract again on open, together with coverage.
    l.reopen();
    assert_eq!(bytes(&replay_from(&mut l.store, 0, 50)), bytes(&events));
}
#[test]
fn stream_is_contiguous_unique_single_and_every_event_is_within_the_bound() {
    let mut l = Life::full();
    let stream = l.store.status().unwrap().stream_id;
    let events = replay_from(&mut l.store, 0, 50);
    assert_eq!(events.len() as u64, head(&l.store));
    let mut ids = BTreeSet::new();
    let mut largest = 0;
    for (i, v) in values(&events).iter().enumerate() {
        assert_eq!(v["sequence"], (i + 1).to_string());
        assert!(v["sequence"].is_string(), "never a JSON number");
        assert_eq!(v["stream_id"], stream.as_str());
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["persistence"], "durable");
        assert!(ids.insert(v["event_id"].as_str().unwrap().to_owned()));
        largest = largest.max(serde_json::to_vec(v).unwrap().len());
    }
    // The bound keeps real headroom over everything the lifecycle can write.
    assert!(
        largest <= MAX_EVENT_BYTES * 2 / 3,
        "largest event {largest}"
    );
    let stored: i64 = l
        .store
        .conn
        .query_row(
            "SELECT max(length(CAST(envelope AS BLOB))) FROM control_events",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored as usize, largest);
}
#[test]
fn an_event_over_the_bound_fails_its_mutation_before_commit_without_a_gap() {
    let mut l = Life::new();
    l.prepare().unwrap();
    let before = snapshot(&l.store.conn);
    let h = head(&l.store);
    EVENT_BYTES.with(|b| b.set(400));
    assert_eq!(l.prepare().unwrap_err(), ControlError::CapacityExceeded);
    assert_eq!(
        l.runtime_mutation(true).unwrap_err(),
        ControlError::CapacityExceeded
    );
    EVENT_BYTES.with(|b| b.set(MAX_EVENT_BYTES));
    assert_eq!(snapshot(&l.store.conn), before);
    l.prepare().unwrap();
    assert_eq!(head(&l.store), h + 1);
    l.reopen();
    // The schema refuses what the append path would have refused, independently.
    let over = "x".repeat(MAX_EVENT_BYTES + 1);
    assert!(
        l.store
            .conn
            .execute(
                "INSERT INTO control_events VALUES(?1,?2,?3,NULL,?4)",
                params![uuid().as_str(), "s", key(99), over],
            )
            .is_err()
    );
}
#[test]
fn a_failed_audit_append_rolls_back_every_producer_and_leaves_no_gap() {
    let mut l = Life::new();
    let mut evented = 0;
    for (name, has_event, step) in steps() {
        if !has_event {
            step(&mut l).unwrap();
            continue;
        }
        evented += 1;
        let before = snapshot(&l.store.conn);
        let h = head(&l.store);
        l.store.conn.execute_batch(FAULT).unwrap();
        let refused = step(&mut l);
        l.store.conn.execute_batch("DROP TRIGGER injected").unwrap();
        assert_eq!(refused, Err(ControlError::StorageUnavailable), "{name}");
        assert_eq!(snapshot(&l.store.conn), before, "{name}: nothing committed");
        step(&mut l).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(head(&l.store) > h, "{name}: the retry is audited");
    }
    assert!(evented >= 40);
    let n = head(&l.store);
    assert_eq!(replay_from(&mut l.store, 0, 50).len() as u64, n);
    l.reopen();
    assert_eq!(head(&l.store), n);
}
#[test]
fn replay_pages_of_any_size_concatenate_to_the_same_bytes() {
    let mut l = Life::full();
    for _ in 0..12 {
        l.prepare().unwrap();
    }
    let whole = replay_from(&mut l.store, 0, 50);
    assert!(whole.len() > 50, "more than one maximal page");
    for limit in [1, 2, 3, 7, 49] {
        assert_eq!(
            bytes(&replay_from(&mut l.store, 0, limit)),
            bytes(&whole),
            "limit {limit}"
        );
    }
    // Reading is deterministic: the same cursor and limit give the same page.
    let (a, next_a) = read(&mut l.store, 5, 7).unwrap();
    let (b, next_b) = read(&mut l.store, 5, 7).unwrap();
    assert_eq!(bytes(&a), bytes(&b));
    assert_eq!(next_a, next_b);
    assert_eq!(a[0].sequence().get(), 6, "strictly after the cursor");
}
#[test]
fn cursor_bounds_are_exact_at_the_head_before_it_and_past_it() {
    let mut l = Life::new();
    // An empty stream replays as empty and has no history to point into.
    let (events, next) = read(&mut l.store, 0, 50).unwrap();
    assert_eq!((events.len(), next), (0, None));
    assert_eq!(
        read(&mut l.store, 1, 50).unwrap_err(),
        ControlError::InvalidRequest
    );
    for _ in 0..4 {
        l.prepare().unwrap();
    }
    let (events, next) = read(&mut l.store, 0, 4).unwrap();
    assert_eq!(
        (events.len(), next),
        (4, None),
        "exhausted exactly at the limit"
    );
    let (events, next) = read(&mut l.store, 0, 3).unwrap();
    assert_eq!((events.len(), next), (3, seq(3)));
    let (events, next) = read(&mut l.store, 3, 3).unwrap();
    assert_eq!((events.len(), next), (1, None), "one before the head");
    assert_eq!(events[0].sequence().get(), 4);
    let (events, next) = read(&mut l.store, 4, 50).unwrap();
    assert_eq!((events.len(), next), (0, None), "at the head");
    assert_eq!(
        read(&mut l.store, 5, 50).unwrap_err(),
        ControlError::InvalidRequest
    );
    assert_eq!(
        read(&mut l.store, u64::MAX, 50).unwrap_err(),
        ControlError::InvalidRequest
    );
    for limit in [0, 51] {
        assert_eq!(
            read(&mut l.store, 0, limit).unwrap_err(),
            ControlError::InvalidRequest
        );
    }
    let other = AuditRead {
        stream_id: uuid(),
        after_sequence: None,
        limit: 1,
    };
    assert_eq!(
        l.store.apply(&Operation::AuditRead(other)).unwrap_err(),
        ControlError::InvalidRequest
    );
    // A watch is the same bounded read with the same cursor rules.
    let stream_id = l.store.status().unwrap().stream_id;
    let watch = |after| {
        Operation::AuditWatch(AuditWatch {
            stream_id: stream_id.clone(),
            after_sequence: seq(after),
            limit: 2,
            wait_ms: 0,
        })
    };
    let ResultValue::Audit { events, next } = l.store.apply(&watch(1)).unwrap() else {
        panic!()
    };
    assert_eq!((events.len(), next), (2, seq(3)));
    assert_eq!(
        l.store.apply(&watch(9)).unwrap_err(),
        ControlError::InvalidRequest
    );
}
fn frame(page: (Vec<EventEnvelopeV1>, Option<Sequence>)) -> usize {
    Response {
        request_id: uuid(),
        authority_instance_id: uuid(),
        outcome: Ok(ResultValue::Audit {
            events: page.0,
            next: page.1,
        }),
    }
    .encode()
    .expect("a replay page always fits one frame")
    .len()
}
#[test]
fn a_page_stops_at_the_frame_budget_before_the_count_limit() {
    let mut l = Life::new();
    // The smallest event of this build, alone in a frame.
    l.runtime_mutation(true).unwrap();
    let small = read(&mut l.store, 0, 1).unwrap();
    assert!(frame(small) < 1024);
    // Sixty events at the largest size the schema can hold, ASCII and four-byte UTF-8.
    for i in 0..60 {
        let c = &l.store.conn;
        let (_, base) = envelope(c, "vendor.future.blob", json!({"blob":""}), None);
        let unit = if i % 2 == 0 { "x" } else { "\u{1D11E}" };
        let blob = unit.repeat(MAX_EVENT_BYTES - base.chars().count());
        foreign(c, "vendor.future.blob", json!({"blob":blob}), None);
    }
    let longest: i64 = l
        .store
        .conn
        .query_row(
            "SELECT max(length(envelope)) FROM control_events",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        longest as usize, MAX_EVENT_BYTES,
        "the schema maximum is exercised"
    );
    l.reopen();
    let total = head(&l.store);
    let mut after = 0;
    let mut pages = 0;
    while after < total {
        let (events, next) = read(&mut l.store, after, 50).unwrap();
        assert!(!events.is_empty(), "a page always makes progress");
        assert!(events.len() < 50, "the byte budget stops the page first");
        let encoded: usize = events
            .iter()
            .map(|e| serde_json::to_vec(e).unwrap().len() + 1)
            .sum();
        assert!(events.len() == 1 || encoded <= PAGE_BYTES);
        after += events.len() as u64;
        assert_eq!(next, if after < total { seq(after) } else { None });
        assert!(frame((events, next)) <= MAX_FRAME);
        pages += 1;
    }
    assert!(pages >= 4);
    assert_eq!(after, total);
    // Small pages still deliver the same history as maximal ones.
    assert_eq!(
        bytes(&replay_from(&mut l.store, 0, 3)),
        bytes(&replay_from(&mut l.store, 0, 50))
    );
}
#[test]
fn utf8_runtime_version_is_bounded_in_bytes_not_characters() {
    let mut l = Life::new();
    let ready = |l: &mut Life, version: String| {
        l.store.runtime_event("authority.runtime.instance_ready", json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":l.runtime,"runtime_version":version,"compatibility":"compatible"}))
    };
    ready(&mut l, "\u{e9}".repeat(64)).unwrap();
    assert!(ready(&mut l, "\u{e9}".repeat(65)).is_err());
    assert!(ready(&mut l, "\u{1D11E}".repeat(33)).is_err());
    assert_eq!(head(&l.store), 1);
    let (events, _) = read(&mut l.store, 0, 1).unwrap();
    assert_eq!(
        values(&events)[0]["payload"]["runtime_version"],
        "\u{e9}".repeat(64)
    );
    l.reopen();
}
#[test]
fn replay_is_read_only_and_repeatable() {
    let mut l = Life::full();
    let before = snapshot(&l.store.conn);
    let changes = l.store.conn.total_changes();
    let first = bytes(&replay_from(&mut l.store, 0, 50));
    for limit in [1, 9, 50] {
        assert_eq!(bytes(&replay_from(&mut l.store, 0, limit)), first);
    }
    let stream_id = l.store.status().unwrap().stream_id;
    l.store
        .apply(&Operation::AuditWatch(AuditWatch {
            stream_id,
            after_sequence: None,
            limit: 50,
            wait_ms: 0,
        }))
        .unwrap();
    assert_eq!(l.store.conn.total_changes(), changes, "zero rows written");
    assert_eq!(snapshot(&l.store.conn), before);
}
#[test]
fn restart_keeps_stream_head_and_history_and_continues_the_sequence() {
    let mut l = Life::full();
    let stream = l.store.status().unwrap().stream_id;
    let n = head(&l.store);
    let before = replay_from(&mut l.store, 0, 50);
    let middle = n / 2;
    let suffix = bytes(&replay_from(&mut l.store, middle, 7));
    let first_instance = values(&before)[0]["source"]["instance_id"].clone();
    l.reopen();
    assert_eq!(l.store.status().unwrap().stream_id, stream);
    assert_eq!(head(&l.store), n);
    assert_eq!(bytes(&replay_from(&mut l.store, 0, 50)), bytes(&before));
    // A client that stopped in the middle resumes with its own cursor: the exact suffix.
    assert_eq!(bytes(&replay_from(&mut l.store, middle, 7)), suffix);
    l.prepare().unwrap();
    assert_eq!(head(&l.store), n + 1);
    let (tail, _) = read(&mut l.store, n, 50).unwrap();
    let v = &values(&tail)[0];
    assert_eq!(v["sequence"], (n + 1).to_string());
    assert_eq!(v["stream_id"], stream.as_str());
    assert_ne!(v["source"]["instance_id"], first_instance);
}
#[test]
fn replaying_the_public_stream_reproduces_the_canonical_heads() {
    let mut l = Life::full();
    let mut replay = Replay::default();
    for v in values(&replay_from(&mut l.store, 0, 11)) {
        replay.observe(&v).unwrap();
    }
    assert_eq!((replay.known, replay.unknown), (head(&l.store), 0));
    let rows = |table: &str, id: &str, state: &str| -> HashMap<String, Head> {
        let mut q = l
            .store
            .conn
            .prepare(&format!("SELECT {id},{state},revision FROM {table}"))
            .unwrap();
        q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Head {
                    state: r.get(1)?,
                    revision: r.get::<_, i64>(2)? as u64,
                },
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    };
    assert_eq!(
        replay.sessions,
        rows("managed_sessions", "session_id", "control_state")
    );
    assert_eq!(replay.leases, rows("execution_leases", "lease_id", "state"));
    assert_eq!(
        replay.turns,
        rows("managed_turns", "managed_turn_id", "state")
    );
    assert_eq!(replay.sessions.len(), 2);
    assert_eq!(replay.leases.len(), 2);
    assert_eq!(replay.turns.len(), 5);
    assert_eq!(replay.approvals.len(), 6);
    assert_eq!(replay.bound.len(), 1);
    assert_eq!(replay.uncertain.values().collect::<Vec<_>>(), [&true]);
    assert_eq!(
        replay.runtime,
        Some(Head {
            state: "stopped".into(),
            revision: 2
        })
    );
    // The view is rebuilt from nothing each time and holds no content.
    let all = String::from_utf8(bytes(&replay_from(&mut l.store, 0, 50))).unwrap();
    assert!(!all.contains(PROMPT));
    assert!(!all.contains(l.root.to_str().unwrap()));
}
#[test]
fn an_unknown_future_event_is_replayed_as_stored_and_drives_nothing() {
    let mut l = Life::new();
    let session = l.prepare().unwrap();
    let lease = l.grant(&session, 3600).unwrap();
    let before = snapshot(&l.store.conn);
    // It names a live lease and claims a revocation and a closed session, in the words of
    // known events. None of it is one of them.
    let payload = json!({"lease_id":lease.lease_id,"lease_state":"revoked","revision":2,
        "control_state":"closed","nested":{"list":[1,2.5,null,true,"\u{e9}"],"decision":"accept"}});
    let spoofs = [
        "vendor.future.lease_revoked",
        "authority.execution_lease.revoked_v2",
        "authority.managed_session.closed_by_policy",
        "authority.managed_turn.isolation_violation",
    ];
    for kind in spoofs {
        assert_eq!(classify(kind), Class::Unknown);
        foreign(
            &l.store.conn,
            kind,
            payload.clone(),
            Some(session.managed_session_id.as_str()),
        );
    }
    l.reopen();
    let (events, _) = read(&mut l.store, 2, 50).unwrap();
    assert_eq!(kinds(&events), spoofs);
    for v in values(&events) {
        assert_eq!(v["payload"], payload, "payload preserved exactly");
        assert_eq!(v["payload_schema_version"], 3);
        assert_eq!(v["source"]["component"], "wyss_future");
    }
    // Canonical state is what the tables say: the lease is active, the session prepared.
    let ResultValue::ActiveLease(Some(active)) = l
        .store
        .apply(&Operation::LeaseActive(LeaseActive {
            session_id: session.managed_session_id.clone(),
        }))
        .unwrap()
    else {
        panic!("the lease stays active")
    };
    assert_eq!((active.state, active.revision), (LeaseState::Active, 1));
    assert_eq!(
        l.store
            .gate_session(&session.managed_session_id)
            .unwrap()
            .control_state,
        ControlState::Prepared
    );
    let after = snapshot(&l.store.conn);
    let unchanged = |s: &str| {
        s.lines()
            .filter(|line| {
                !line.starts_with("control_events") && !line.starts_with("authority_metadata")
            })
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(unchanged(&after), unchanged(&before));
    // The authority itself can never append a type it cannot hold to a contract.
    let tx = l.store.conn.transaction().unwrap();
    let refused = append(
        &tx,
        &uuid(),
        &uuid(),
        Draft {
            kind: "vendor.future.lease_revoked",
            session: None,
            correlation: None,
            timestamp: now().unwrap(),
            payload: json!({}),
        },
    );
    assert_eq!(refused.unwrap_err(), ControlError::InternalError);
    drop(tx);
    // And the stream keeps going after it: the next known event takes the next sequence.
    let h = head(&l.store);
    l.prepare().unwrap();
    assert_eq!(head(&l.store), h + 1);
    l.reopen();
}

/// Applies one change behind the schema triggers, as a damaged or forged file would be.
fn tamper(l: &Life, change: impl FnOnce(&Connection)) -> ControlResult<()> {
    let c = Connection::open(l.db()).unwrap();
    c.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER,
        false,
    )
    .unwrap();
    change(&c);
    drop(c);
    Store::open(&l.db(), uuid()).map(|_| ())
}
/// The sequence of the first event of one type.
fn first(c: &Connection, kind: &str) -> String {
    c.query_row(
        "SELECT sequence FROM control_events WHERE json_extract(envelope,'$.event_type')=?1 ORDER BY sequence LIMIT 1",
        [kind],
        |r| r.get(0),
    )
    .unwrap()
}
fn rewrite(c: &Connection, sequence: &str, f: impl FnOnce(String) -> String) {
    let raw: String = c
        .query_row(
            "SELECT envelope FROM control_events WHERE sequence=?1",
            [sequence],
            |r| r.get(0),
        )
        .unwrap();
    let changed = f(raw.clone());
    assert_ne!(changed, raw, "the tamper must change the envelope");
    assert_eq!(
        c.execute(
            "UPDATE control_events SET envelope=?1 WHERE sequence=?2",
            params![changed, sequence],
        )
        .unwrap(),
        1
    );
}
fn set(c: &Connection, sequence: &str, path: &str, value: Value) {
    rewrite(c, sequence, |raw| {
        c.query_row(
            "SELECT json_set(?1,?2,json(?3))",
            params![raw, path, value.to_string()],
            |r| r.get(0),
        )
        .unwrap()
    });
}
type Damage = (&'static str, Box<dyn Fn(&Connection)>);
fn damage(name: &'static str, f: impl Fn(&Connection) + 'static) -> Damage {
    (name, Box::new(f))
}
fn run(c: &Connection, sql: &str) {
    assert!(c.execute(sql, []).unwrap() >= 1, "{sql}");
}
#[test]
fn a_damaged_or_forged_log_is_refused_and_never_repaired() {
    const GRANTED: &str = "authority.execution_lease.granted";
    let other = uuid();
    let cases: Vec<Damage> = vec![
        damage("invalid envelope JSON", |c| {
            rewrite(c, &key(3), |_| "{".into())
        }),
        damage("envelope is not an object", |c| {
            rewrite(c, &key(3), |_| "[]".into())
        }),
        damage("duplicate JSON member", |c| {
            rewrite(c, &key(3), |raw| {
                raw.replacen(
                    "{\"schema_version\":1",
                    "{\"schema_version\":1,\"schema_version\":1",
                    1,
                )
            })
        }),
        damage("duplicate nested member", |c| {
            rewrite(c, &key(3), |raw| {
                raw.replacen("\"payload\":{", "\"payload\":{\"adapter_id\":\"codex\",", 1)
            })
        }),
        damage("unknown envelope field", |c| {
            set(c, &key(3), "$.extra", json!(1))
        }),
        damage("envelope v2", |c| {
            set(c, &key(3), "$.schema_version", json!(2))
        }),
        damage("sequence as a number", |c| {
            set(c, &key(3), "$.sequence", json!(3))
        }),
        damage("sequence with a leading zero", |c| {
            set(c, &key(3), "$.sequence", json!("03"))
        }),
        damage("sequence of another row", |c| {
            set(c, &key(3), "$.sequence", json!("4"))
        }),
        damage("malformed sequence column", |c| {
            run(
                c,
                "UPDATE control_events SET sequence='0000000000000000003x' WHERE sequence='00000000000000000003'",
            )
        }),
        damage("sequence column of the last row", |c| {
            run(
                c,
                "UPDATE control_events SET sequence='99999999999999999999' WHERE sequence=(SELECT max(sequence) FROM control_events)",
            )
        }),
        damage("stream mismatch in the column", move |c| {
            assert_eq!(
                c.execute(
                    "UPDATE control_events SET stream_id=?1 WHERE sequence=?2",
                    params![other.as_str(), key(3)],
                )
                .unwrap(),
                1
            )
        }),
        damage("stream mismatch in the envelope", |c| {
            set(c, &key(3), "$.stream_id", json!(uuid()))
        }),
        damage("sequence gap", |c| {
            run(
                c,
                "DELETE FROM control_events WHERE sequence='00000000000000000003'",
            )
        }),
        damage("truncated tail", |c| {
            run(
                c,
                "DELETE FROM control_events WHERE sequence=(SELECT max(sequence) FROM control_events)",
            )
        }),
        damage("duplicate sequence under another stream", |c| {
            assert_eq!(
                c.execute(
                    "INSERT INTO control_events SELECT ?1,?2,sequence,session_id,envelope FROM control_events WHERE sequence=?3",
                    params![uuid().as_str(), uuid().as_str(), key(3)],
                )
                .unwrap(),
                1
            )
        }),
        damage("event id column mismatch", |c| {
            assert_eq!(
                c.execute(
                    "UPDATE control_events SET event_id=?1 WHERE sequence=?2",
                    params![uuid().as_str(), key(3)],
                )
                .unwrap(),
                1
            )
        }),
        damage("session column mismatch", |c| {
            run(
                c,
                "UPDATE control_events SET session_id=NULL WHERE sequence='00000000000000000003'",
            )
        }),
        damage("known payload with an extra field", |c| {
            set(c, &first(c, GRANTED), "$.payload.extra", json!(true))
        }),
        damage("known payload with a state its type cannot record", |c| {
            set(
                c,
                &first(c, GRANTED),
                "$.payload.lease_state",
                json!("revoked"),
            )
        }),
        damage("known payload with a skipped revision", |c| {
            set(c, &first(c, GRANTED), "$.payload.revision", json!(2))
        }),
        damage("known event made ephemeral", |c| {
            set(c, &key(3), "$.persistence", json!("ephemeral"))
        }),
        damage("durable secret", |c| {
            set(c, &key(3), "$.sensitivity", json!("secret"))
        }),
        damage("head ahead of the log", |c| {
            run(
                c,
                "UPDATE authority_metadata SET last_sequence=CAST(CAST(last_sequence AS INTEGER)+1 AS TEXT)",
            )
        }),
        damage("head behind the log", |c| {
            run(
                c,
                "UPDATE authority_metadata SET last_sequence=CAST(CAST(last_sequence AS INTEGER)-1 AS TEXT)",
            )
        }),
        damage("head reset", |c| {
            run(c, "UPDATE authority_metadata SET last_sequence='0'")
        }),
    ];
    for (name, change) in cases {
        let l = Life::full();
        assert_eq!(tamper(&l, |_| ()), Ok(()), "control: untouched");
        let mut damaged = String::new();
        assert_eq!(
            tamper(&l, |c| {
                change(c);
                damaged = snapshot(c);
            }),
            Err(ControlError::StorageIncompatible),
            "{name}"
        );
        // Refused again and byte for byte as damaged: nothing was deleted, rewritten,
        // renumbered or otherwise repaired by the attempt to open it.
        assert_eq!(
            tamper(&l, |c| assert_eq!(snapshot(c), damaged, "{name}")),
            Err(ControlError::StorageIncompatible),
            "{name}: still refused"
        );
    }
    // The schema itself refuses a second row with an existing event id or sequence.
    let l = Life::full();
    for column in ["event_id", "sequence"] {
        let sql = format!(
            "INSERT INTO control_events SELECT {},stream_id,{},session_id,envelope FROM control_events WHERE sequence=?1",
            if column == "event_id" {
                "event_id"
            } else {
                "'11111111-1111-4111-8111-111111111111'"
            },
            if column == "sequence" {
                "sequence"
            } else {
                "'00000000000000009999'"
            },
        );
        assert!(l.store.conn.execute(&sql, [key(3)]).is_err(), "{column}");
    }
}
#[test]
fn a_canonical_row_the_log_does_not_account_for_is_refused() {
    // Each case hides one audited fact by retyping its event as a future one: every
    // envelope stays valid and every table is untouched, so only coverage can notice.
    for kind in [
        "authority.managed_session.prepared",
        "authority.managed_session.closed",
        "authority.execution_lease.granted",
        "authority.execution_lease.revoked",
        "authority.execution_lease.expired",
        "authority.managed_turn.admitted",
        "authority.managed_turn.interrupted",
        "authority.managed_approval.requested",
        "authority.managed_approval.confirmed",
        "authority.managed_session.runtime_bound",
        "authority.managed_session.runtime_operation_abandoned",
        "authority.runtime.desired_state_changed",
    ] {
        let l = Life::full();
        let hidden = tamper(&l, |c| {
            set(
                c,
                &first(c, kind),
                "$.event_type",
                json!("vendor.future.hidden"),
            )
        });
        assert_eq!(hidden, Err(ControlError::StorageIncompatible), "{kind}");
    }
    // A table changed without its event is refused the same way.
    for sql in [
        "UPDATE managed_sessions SET control_state='prepared',revision=1",
        "UPDATE execution_leases SET revision=revision+1,updated_at=updated_at",
        "UPDATE managed_turns SET state='completed',terminal_reason='completed' WHERE state='failed'",
        "UPDATE managed_runtimes SET desired_state='running'",
        "UPDATE managed_session_bindings SET runtime_session_id='another-thread'",
        "UPDATE managed_session_operations SET state='indeterminate' WHERE state='failed' AND kind='create' AND provider_stage='none'",
        "DELETE FROM execution_leases WHERE state='expired'",
    ] {
        let l = Life::full();
        assert_eq!(
            tamper(&l, |c| {
                c.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
                run(c, sql)
            }),
            Err(ControlError::StorageIncompatible),
            "{sql}"
        );
    }
    // A foreign event that is not durable has no place in the durable log.
    let l = Life::full();
    let n = foreign(&l.store.conn, "vendor.future.note", json!({}), None);
    assert_eq!(tamper(&l, |_| ()), Ok(()));
    assert_eq!(
        tamper(&l, |c| set(c, &key(n), "$.persistence", json!("ephemeral"))),
        Err(ControlError::StorageIncompatible)
    );
}
#[test]
fn an_uncertain_operation_is_audited_once_and_recovered_at_startup() {
    let mut l = Life::new();
    let session = l.prepare().unwrap();
    l.reserve(&session, true).unwrap();
    let operation = l.reservation.clone().unwrap().operation_id;
    let h = head(&l.store);
    // The authority dies with the create dispatched: startup declares it uncertain.
    l.reopen();
    assert_eq!(head(&l.store), h);
    l.store.recover_binding_operations().unwrap();
    assert_eq!(head(&l.store), h + 1);
    let (events, _) = read(&mut l.store, h, 50).unwrap();
    let v = &values(&events)[0];
    assert_eq!(
        v["event_type"],
        "authority.managed_session.runtime_operation_indeterminate"
    );
    assert_eq!(v["payload"], json!({"operation_id":operation}));
    assert_eq!(v["correlation_id"], operation.as_str());
    assert_eq!(v["session_id"], session.managed_session_id.as_str());
    l.store.recover_binding_operations().unwrap();
    assert_eq!(head(&l.store), h + 1, "recovery is idempotent");
    // A reserved operation and a resume are provably without effect: no event.
    let second = l.prepare().unwrap();
    let op = Operation::BindingCreate(BindingCreate {
        intent_id: uuid(),
        managed_session_id: second.managed_session_id.clone(),
        expected_session_revision: 1,
        workspace_root: l.root.to_str().unwrap().into(),
    });
    l.store.reserve_binding(&op).unwrap();
    let h = head(&l.store);
    l.store.recover_binding_operations().unwrap();
    assert_eq!(head(&l.store), h);
    assert!(
        l.store
            .reserve_binding(&Operation::BindingCreate(BindingCreate {
                intent_id: uuid(),
                managed_session_id: second.managed_session_id,
                expected_session_revision: 1,
                workspace_root: l.root.to_str().unwrap().into(),
            }))
            .is_ok()
    );
    l.reopen();
}
#[test]
fn the_page_overhead_reserve_covers_the_real_response_wrapper() {
    let empty = frame((vec![], None));
    let cursor = frame((vec![], seq(u64::MAX)));
    assert!(cursor - empty <= 24, "the cursor is a decimal string");
    assert!(cursor < PAGE_OVERHEAD);
    assert_eq!(PAGE_BYTES + PAGE_OVERHEAD, MAX_FRAME);
}
#[test]
fn damage_that_appears_while_running_is_refused_by_the_read_itself() {
    let behind = |l: &Life, sql: &str| {
        let c = Connection::open(l.db()).unwrap();
        c.set_db_config(
            rusqlite::config::DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER,
            false,
        )
        .unwrap();
        run(&c, sql);
    };
    // A committed event disappears under an open authority: no page skips over the hole.
    let mut l = Life::full();
    behind(
        &l,
        "DELETE FROM control_events WHERE sequence='00000000000000000005'",
    );
    assert_eq!(read(&mut l.store, 0, 3).unwrap().0.len(), 3);
    // The hole is refused as soon as a read can see it, even as the row after the page.
    for (after, limit) in [(0, 4), (0, 5), (0, 50), (4, 1), (3, 2)] {
        assert_eq!(
            read(&mut l.store, after, limit).unwrap_err(),
            ControlError::StorageIncompatible,
            "after {after} limit {limit}"
        );
    }
    // An envelope is replaced by one that is not an envelope.
    let mut l = Life::full();
    behind(
        &l,
        "UPDATE control_events SET envelope='{}' WHERE sequence='00000000000000000002'",
    );
    assert_eq!(
        read(&mut l.store, 0, 50).unwrap_err(),
        ControlError::StorageIncompatible
    );
    // An envelope keeps its shape and takes another row's number.
    let mut l = Life::full();
    behind(
        &l,
        "UPDATE control_events SET envelope=json_set(envelope,'$.sequence','9') WHERE sequence='00000000000000000002'",
    );
    assert_eq!(
        read(&mut l.store, 1, 1).unwrap_err(),
        ControlError::StorageIncompatible
    );
}
#[test]
fn a_binding_row_must_be_the_one_its_event_recorded() {
    // A bound session without a turn: only the binding event vouches for the thread.
    let mut l = Life::new();
    let all = steps();
    for (name, _, step) in &all[..6] {
        step(&mut l).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    assert!(l.binding.is_some() && l.turn.is_none());
    assert_eq!(tamper(&l, |_| ()), Ok(()));
    assert_eq!(
        tamper(&l, |c| run(
            c,
            "UPDATE managed_session_bindings SET runtime_session_id='another-thread'"
        )),
        Err(ControlError::StorageIncompatible)
    );
}
#[test]
fn an_uncommitted_event_is_never_the_announced_head() {
    let mut l = Life::new();
    l.prepare().unwrap();
    assert_eq!(head(&l.store), 1);
    // Inside an open transaction the head is not observable at all, so a watcher can
    // never be told about an event that may still roll back.
    l.store.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    foreign(&l.store.conn, "vendor.future.note", json!({}), None);
    assert_eq!(l.store.head().unwrap_err(), ControlError::InternalError);
    let stream_id = l.store.status().unwrap().stream_id;
    assert!(
        l.store.replay(&stream_id, seq(1), 50).is_err(),
        "nor is a read answered from inside it"
    );
    l.store.conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(head(&l.store), 1);
    assert_eq!(read(&mut l.store, 0, 50).unwrap().0.len(), 1);
}
