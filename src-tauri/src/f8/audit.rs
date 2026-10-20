//! Vocabulary-2 audit validation and deterministic replay. Inactive and read-only.
//!
//! The stream is observation. Nothing here grants, resumes, answers or executes anything, and
//! no event, response field, PID or selected GUI session is read as authority. Historical
//! vocabulary-1 records are validated by the unchanged baseline validators on their original
//! bytes; they are never rewritten or reinterpreted as generation-aware.
use super::protocol::canonical;
use crate::contracts::{
    Sequence, WireUuid,
    agent_adapter::CompatibilityState,
    control::{ControlError as E, ControlResult, RuntimeDiagnostic},
    event_envelope::EventEnvelopeV1,
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Candidate lifetime limits (resources.json), enforced while replaying.
const MAX_RUNTIME_ROWS: usize = 128;
const MAX_LAUNCH_ROWS: usize = 4096;
const MAX_GENERATION: u64 = i64::MAX as u64;
const MAX_EVENT_BYTES: usize = 2048;

fn bad() -> E {
    E::StorageIncompatible
}
fn text<'a>(v: &'a Value, k: &str) -> ControlResult<&'a str> {
    v.get(k).and_then(Value::as_str).ok_or_else(bad)
}
fn uuid_text<'a>(v: &'a Value, k: &str) -> ControlResult<&'a str> {
    let s = text(v, k)?;
    canonical(s).map_err(|_| bad())?;
    Ok(s)
}
fn u63(v: &Value, k: &str) -> ControlResult<u64> {
    v.get(k)
        .and_then(Value::as_u64)
        .filter(|n| *n <= MAX_GENERATION)
        .ok_or_else(bad)
}
fn positive(v: &Value, k: &str) -> ControlResult<u64> {
    u63(v, k).and_then(|n| if n == 0 { Err(bad()) } else { Ok(n) })
}

/// Closed numeric budget keys from the reviewed resource contract.
pub fn budget_key_valid(key: &str) -> bool {
    serde_json::from_str::<Value>(include_str!("../../../docs/contracts/f8/resources.json"))
        .ok()
        .is_some_and(|r| key != "contract_version" && r.get(key).is_some_and(Value::is_u64))
}

/// Which payload contract a validated event was written under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vocabulary {
    Historical1,
    Generation2,
}

const RUNTIME_V2: [&str; 3] = [
    "authority.runtime.desired_state_changed",
    "authority.runtime.instance_ready",
    "authority.runtime.instance_exited",
];
const TURN_KINDS: [&str; 7] = [
    "admitted",
    "running",
    "completed",
    "interrupted",
    "failed",
    "indeterminate",
    "cancel_requested",
];
const APPROVAL_KINDS: [&str; 8] = [
    "requested",
    "accepted",
    "declined",
    "cancelled",
    "expired",
    "invalidated",
    "indeterminate",
    "confirmed",
];

/// Validates one decoded event: envelope, then the payload contract of its declared version.
/// Unknown event types and unknown payload versions are refused (closed vocabulary).
pub fn validate_event(v: &Value) -> ControlResult<Vocabulary> {
    EventEnvelopeV1::from_value(v.clone()).map_err(|_| E::InvalidRequest)?;
    // Every identifier is canonical (case, hyphens) and the durable size bound holds on the wire too.
    super::protocol::canonical_ids(v).map_err(|_| bad())?;
    for key in ["event_id", "correlation_id", "causation_id"] {
        if let Some(id) = v[key].as_str() {
            canonical(id).map_err(|_| bad())?;
        }
    }
    canonical_source(v)?;
    if serde_json::to_vec(v).map_err(|_| bad())?.len() > MAX_EVENT_BYTES {
        return Err(bad());
    }
    let kind = text(v, "event_type")?;
    uuid_text(v, "event_id")?;
    uuid_text(v, "stream_id")?;
    match v["payload_schema_version"].as_u64() {
        Some(1) => {
            super::super::control::store::validate_historical_event(v)?;
            Ok(Vocabulary::Historical1)
        }
        Some(2) => {
            common(v)?;
            if RUNTIME_V2.contains(&kind) {
                runtime_v2(v, kind)?;
            } else if kind
                .strip_prefix("authority.managed_turn.")
                .is_some_and(|k| TURN_KINDS.contains(&k))
                || kind
                    .strip_prefix("authority.managed_approval.")
                    .is_some_and(|k| APPROVAL_KINDS.contains(&k))
            {
                attributed_v2(v)?;
            } else {
                return Err(bad());
            }
            Ok(Vocabulary::Generation2)
        }
        _ => Err(bad()),
    }
}

fn common(v: &Value) -> ControlResult<()> {
    if v["persistence"] != "durable"
        || v["sensitivity"] != "internal"
        || v["redaction"] != "none"
        || v["source"]["component"] != "wyss_control"
        || !v["causation_id"].is_null()
    {
        return Err(bad());
    }
    let session = uuid_text(v, "session_id")?;
    let source = canonical_source(v)?;
    // The authority, session and (below) runtime instance are three distinct identity domains.
    if source == session {
        return Err(bad());
    }
    Ok(())
}
fn canonical_source(v: &Value) -> ControlResult<&str> {
    let s = v["source"]["instance_id"].as_str().ok_or_else(bad)?;
    canonical(s).map_err(|_| bad())?;
    Ok(s)
}
fn exact(p: &Value, keys: &[&str]) -> ControlResult<()> {
    let m = p.as_object().ok_or_else(bad)?;
    if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k)) {
        Err(bad())
    } else {
        Ok(())
    }
}

fn runtime_v2(v: &Value, kind: &str) -> ControlResult<()> {
    let p = &v["payload"];
    let session = text(v, "session_id")?;
    let source = canonical_source(v)?;
    match kind {
        "authority.runtime.desired_state_changed" => {
            exact(
                p,
                &[
                    "adapter_id",
                    "runtime_kind",
                    "desired_state",
                    "revision",
                    "generation",
                    "runtime_instance_id",
                ],
            )?;
            if v["correlation_id"].is_null()
                || !matches!(p["desired_state"].as_str(), Some("running" | "stopped"))
            {
                return Err(bad());
            }
            positive(p, "revision")?;
            let generation = u63(p, "generation")?;
            match &p["runtime_instance_id"] {
                Value::Null => {
                    // A running desired state always owns a reserved launch.
                    if generation != 0 && p["desired_state"] == "running" {
                        return Err(bad());
                    }
                }
                Value::String(i) => {
                    canonical(i).map_err(|_| bad())?;
                    if generation == 0 || i == session || i == source {
                        return Err(bad());
                    }
                }
                _ => return Err(bad()),
            }
            if generation == 0 && p["desired_state"] == "running" {
                return Err(bad());
            }
        }
        "authority.runtime.instance_ready" => {
            exact(
                p,
                &[
                    "adapter_id",
                    "runtime_kind",
                    "runtime_instance_id",
                    "generation",
                    "runtime_version",
                    "compatibility",
                ],
            )?;
            if !v["correlation_id"].is_null() {
                return Err(bad());
            }
            positive(p, "generation")?;
            let i = uuid_text(p, "runtime_instance_id")?;
            if i == session || i == source {
                return Err(bad());
            }
            let version = text(p, "runtime_version")?;
            if version.is_empty()
                || version.len() > 128
                || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                || version.split('.').any(str::is_empty)
            {
                return Err(bad());
            }
            // The baseline writer only ever publishes a compatible launch as ready.
            if serde_json::from_value::<CompatibilityState>(p["compatibility"].clone())
                .map_err(|_| bad())?
                != CompatibilityState::Compatible
            {
                return Err(bad());
            }
        }
        _ => {
            exact(
                p,
                &[
                    "adapter_id",
                    "runtime_kind",
                    "runtime_instance_id",
                    "generation",
                    "exit_kind",
                    "expected",
                    "desired_state",
                    "budget_failure",
                ],
            )?;
            if !v["correlation_id"].is_null() || !p["expected"].is_boolean() {
                return Err(bad());
            }
            positive(p, "generation")?;
            let i = uuid_text(p, "runtime_instance_id")?;
            if i == session || i == source {
                return Err(bad());
            }
            if !matches!(p["desired_state"].as_str(), Some("running" | "stopped")) {
                return Err(bad());
            }
            let exit = text(p, "exit_kind")?;
            let budget = exit == "budget_exceeded";
            if !budget {
                serde_json::from_value::<RuntimeDiagnostic>(p["exit_kind"].clone())
                    .map_err(|_| bad())?;
            }
            // A budget key is present exactly for a budget exit, and is a closed numeric key.
            match &p["budget_failure"] {
                Value::String(k) if budget && budget_key_valid(k) => (),
                Value::Null if !budget => (),
                _ => return Err(bad()),
            }
        }
    }
    if p["adapter_id"] != "codex" || p["runtime_kind"] != "codex" {
        return Err(bad());
    }
    Ok(())
}

/// Turn and approval events keep their exact baseline payload and add exactly `generation`.
fn attributed_v2(v: &Value) -> ControlResult<()> {
    let mut base = v.clone();
    let p = base["payload"].as_object_mut().ok_or_else(bad)?;
    let generation = p.remove("generation").ok_or_else(bad)?;
    if generation
        .as_u64()
        .is_none_or(|g| g == 0 || g > MAX_GENERATION)
    {
        return Err(bad());
    }
    base["payload_schema_version"] = Value::from(1);
    super::super::control::store::validate_historical_event(&base)?;
    let instance = uuid_text(&base["payload"], "runtime_instance_id")?;
    if instance == text(v, "session_id")? || instance == canonical_source(v)? {
        return Err(bad());
    }
    Ok(())
}

/// Stateless page checks: stream identity, bounds, exact contiguity from the cursor, unique
/// event IDs and per-event validity. `next` must name the last returned event, never a gap.
pub fn check_page(
    stream: &WireUuid,
    after: Option<Sequence>,
    limit: u16,
    events: &[EventEnvelopeV1],
    next: Option<Sequence>,
) -> ControlResult<()> {
    if events.len() > usize::from(limit) {
        return Err(E::CorrelationError);
    }
    let mut expected = after.map_or(0, |s| s.get());
    let mut ids = HashSet::new();
    let mut last = None;
    for event in events {
        let v = serde_json::to_value(event).map_err(|_| E::InvalidRequest)?;
        validate_event(&v)?;
        expected = expected.checked_add(1).ok_or(E::CorrelationError)?;
        if v["stream_id"].as_str() != Some(stream.as_str())
            || event.sequence().get() != expected
            || !ids.insert(text(&v, "event_id")?.to_owned())
        {
            return Err(E::CorrelationError);
        }
        last = Some(event.sequence());
    }
    if next.is_some() && next != last {
        return Err(E::CorrelationError);
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Runtime {
    revision: u64,
    desired_running: bool,
    generation: u64,
    instance: Option<String>,
    closed: bool,
}
#[derive(Debug, Clone)]
struct Launch {
    session: String,
    generation: u64,
    authority: String,
    ready: bool,
    exited: bool,
}
#[derive(Debug, Clone)]
struct Turn {
    session: String,
    /// `None`: a vocabulary-1 (unassigned) turn. Its history continues only in vocabulary 1.
    launch: Option<(String, u64)>,
    revision: u64,
    state: String,
}
#[derive(Debug, Clone)]
struct Approval {
    session: String,
    turn: String,
    launch: Option<(String, u64)>,
    revision: u64,
    state: String,
    effect: String,
    fingerprint: String,
}

/// One runtime mutation as the stream recorded it, with the state it replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub session: String,
    pub revision: u64,
    pub generation: u64,
    pub running: bool,
    pub prev_generation: u64,
    /// The launch the mutation found live (desired running), else none.
    pub prev_instance: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Domain {
    Session,
    Instance,
    Authority,
}

/// Work the log leaves unresolved on a launch that is gone: recovery must mark it
/// indeterminate and may never resume, answer or complete it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Unresolved {
    pub turns: Vec<String>,
    pub approvals: Vec<String>,
}

/// Deterministic replay of the whole stream from sequence 1. A refusal leaves nothing to repair.
#[derive(Default)]
pub struct AuditReplay {
    stream: Option<String>,
    head: u64,
    events: HashSet<String>,
    v2_started: bool,
    sessions: HashMap<String, bool>,
    runtimes: HashMap<String, Runtime>,
    launches: HashMap<String, Launch>,
    turns: HashMap<String, Turn>,
    approvals: HashMap<String, Approval>,
    /// Mutation intent (the event's correlation) to the runtime state it produced.
    intents: HashMap<String, Intent>,
    /// Baseline runtime mutation intents (null session), with the revision they produced.
    legacy_intents: HashMap<String, u64>,
    /// Sessions, launch instances and authorities are three disjoint identity domains.
    domains: HashMap<String, Domain>,
    /// Authority instances are a sole-writer sequence: once another one wrote, a predecessor
    /// can never write again, so a forged "successor" is detected by the owner's next event.
    writer: Option<String>,
    retired: HashSet<String>,
    /// A refusal leaves the replay unusable; callers cannot keep folding past it.
    poisoned: bool,
    pub historical: u64,
    pub attributed: u64,
}

impl AuditReplay {
    pub fn head(&self) -> u64 {
        self.head
    }
    /// Folds one page; it must begin exactly at the next unseen sequence.
    pub fn observe_page(&mut self, events: &[EventEnvelopeV1]) -> ControlResult<()> {
        for event in events {
            let v = serde_json::to_value(event).map_err(|_| E::InvalidRequest)?;
            self.observe(&v)?;
        }
        Ok(())
    }
    pub fn observe(&mut self, v: &Value) -> ControlResult<()> {
        if self.poisoned {
            return Err(bad());
        }
        self.poisoned = true;
        self.fold(v)?;
        self.poisoned = false;
        Ok(())
    }
    fn claim(&mut self, id: &str, domain: Domain) -> ControlResult<()> {
        match self.domains.insert(id.to_owned(), domain) {
            Some(old) if old != domain => Err(bad()),
            _ => Ok(()),
        }
    }
    fn fold(&mut self, v: &Value) -> ControlResult<()> {
        let vocabulary = validate_event(v)?;
        let source = canonical_source(v)?.to_owned();
        if self.retired.contains(&source) {
            return Err(bad());
        }
        if self.writer.as_deref() != Some(source.as_str()) {
            if let Some(old) = self.writer.replace(source.clone()) {
                self.retired.insert(old);
            }
            self.claim(&source, Domain::Authority)?;
        }
        let stream = text(v, "stream_id")?;
        let sequence: u64 = text(v, "sequence")?.parse().map_err(|_| bad())?;
        if self.stream.get_or_insert_with(|| stream.to_owned()) != stream
            || sequence != self.head + 1
            || !self.events.insert(text(v, "event_id")?.to_owned())
        {
            return Err(bad());
        }
        match vocabulary {
            Vocabulary::Historical1 => self.historical_event(v)?,
            Vocabulary::Generation2 => {
                self.v2_started = true;
                self.attributed += 1;
                self.attributed_event(v)?
            }
        }
        self.head = sequence;
        Ok(())
    }

    fn historical_event(&mut self, v: &Value) -> ControlResult<()> {
        self.historical += 1;
        let kind = text(v, "event_type")?;
        let p = &v["payload"];
        if self.v2_started
            && [
                "authority.runtime.",
                "authority.managed_turn.",
                "authority.managed_approval.",
            ]
            .iter()
            .any(|prefix| kind.starts_with(prefix))
        {
            // The quiescent migration leaves no unfinished baseline work. After the first
            // generation-aware event, any baseline-shaped runtime, turn or approval event is
            // a forgery: it could only have been written for work no one can resume.
            return Err(bad());
        }
        if kind == "authority.runtime.desired_state_changed" {
            let intent = text(v, "correlation_id")?.to_owned();
            if self
                .legacy_intents
                .insert(intent, positive(p, "revision")?)
                .is_some()
            {
                return Err(bad());
            }
        }
        match kind {
            "authority.managed_session.prepared" => {
                let session = uuid_text(v, "session_id")?.to_owned();
                self.claim(&session, Domain::Session)?;
                if self.sessions.insert(session, false).is_some() {
                    return Err(bad());
                }
            }
            "authority.managed_session.closed" => {
                let s = uuid_text(v, "session_id")?;
                if self.sessions.insert(s.to_owned(), true) != Some(false) {
                    return Err(bad());
                }
                if let Some(r) = self.runtimes.get_mut(s) {
                    r.closed = true;
                }
            }
            _ if kind.starts_with("authority.managed_turn.") => {
                let id = uuid_text(p, "managed_turn_id")?;
                let session = uuid_text(v, "session_id")?;
                let name = kind.rsplit('.').next().unwrap_or_default();
                let revision = positive(p, "revision")?;
                let state = text(p, "state")?.to_owned();
                match self.turns.get_mut(id) {
                    None if name == "cancel_requested" => return Err(bad()),
                    None => {
                        if revision != 1 {
                            return Err(bad());
                        }
                        self.turns.insert(
                            id.to_owned(),
                            Turn {
                                session: session.to_owned(),
                                launch: None,
                                revision,
                                state,
                            },
                        );
                    }
                    Some(t) => {
                        // Baseline history never adopts a generation-aware turn, or the reverse.
                        if t.launch.is_some() || t.session != session {
                            return Err(bad());
                        }
                        if name == "cancel_requested" {
                            if t.revision != revision || t.state != state {
                                return Err(bad());
                            }
                        } else if revision != t.revision + 1 || terminal_turn(&t.state) {
                            return Err(bad());
                        } else {
                            t.revision = revision;
                            t.state = state;
                        }
                    }
                }
            }
            _ if kind.starts_with("authority.managed_approval.") => {
                let id = uuid_text(p, "managed_approval_id")?;
                let turn = uuid_text(p, "managed_turn_id")?;
                let session = uuid_text(v, "session_id")?;
                let next = approval_head(v, session, turn, None)?;
                match self.turns.get(turn) {
                    Some(t) if t.session == session => (),
                    _ => return Err(bad()),
                }
                match self.approvals.get(id) {
                    None if next.revision != 1
                        || kind != "authority.managed_approval.requested" =>
                    {
                        return Err(bad());
                    }
                    Some(old) if old.launch.is_some() || !approval_step(old, &next, kind) => {
                        return Err(bad());
                    }
                    _ => (),
                }
                if self.turns.get(turn).is_some_and(|t| t.launch.is_some()) {
                    return Err(bad());
                }
                self.approvals.insert(id.to_owned(), next);
            }
            _ => (),
        }
        Ok(())
    }

    fn attributed_event(&mut self, v: &Value) -> ControlResult<()> {
        let kind = text(v, "event_type")?;
        let session = text(v, "session_id")?.to_owned();
        if !self.sessions.contains_key(&session) {
            // Attribution to a session the stream never prepared is cross-session forgery.
            return Err(bad());
        }
        if RUNTIME_V2.contains(&kind) {
            return self.runtime_event(v, kind, &session);
        }
        let p = &v["payload"];
        let instance = uuid_text(p, "runtime_instance_id")?.to_owned();
        let generation = positive(p, "generation")?;
        let launch = self.launches.get(&instance).ok_or_else(bad)?.clone();
        if launch.session != session || launch.generation != generation {
            return Err(bad());
        }
        let source = canonical_source(v)?;
        let authorized = self.live_for(&session, &instance, source);
        let name = kind.rsplit('.').next().unwrap_or_default();
        if kind.starts_with("authority.managed_turn.") {
            self.turn_event(v, name, (&session, &instance, generation), authorized)
        } else {
            self.approval_event(v, name, (&session, &instance, generation), authorized)
        }
    }

    /// A launch is live for `writer` only while it is the session's current, attested,
    /// unexited, running generation which that very authority reserved. For any other writer
    /// the launch is history: it can only be observed as lost, never progressed.
    fn live_for(&self, session: &str, instance: &str, writer: &str) -> bool {
        self.runtimes.get(session).is_some_and(|r| {
            r.desired_running
                && !r.closed
                && r.instance.as_deref() == Some(instance)
                && self
                    .launches
                    .get(instance)
                    .is_some_and(|l| l.ready && !l.exited && l.authority == writer)
        })
    }

    fn runtime_event(&mut self, v: &Value, kind: &str, session: &str) -> ControlResult<()> {
        let p = &v["payload"];
        let source = canonical_source(v)?.to_owned();
        if kind.ends_with("desired_state_changed") {
            let revision = positive(p, "revision")?;
            let generation = u63(p, "generation")?;
            let running = p["desired_state"] == "running";
            let instance = p["runtime_instance_id"].as_str().map(str::to_owned);
            let closed = self.sessions[session];
            let previous = self.runtimes.get(session).cloned();
            let (prev_rev, prev_gen, prev_instance) = previous.as_ref().map_or((0, 0, None), |r| {
                (r.revision, r.generation, r.instance.clone())
            });
            let found_live = previous
                .as_ref()
                .filter(|r| r.desired_running)
                .and_then(|r| r.instance.clone());
            if revision != prev_rev + 1 || generation < prev_gen || generation > prev_gen + 1 {
                return Err(bad());
            }
            // A closed session may only be stopped; it can never reserve another launch.
            if closed && (running || generation != prev_gen) {
                return Err(bad());
            }
            if generation == prev_gen + 1 {
                // A new attempt reserves a launch which never existed before.
                let i = instance.clone().ok_or_else(bad)?;
                if !running || self.launches.contains_key(&i) || i == session {
                    return Err(bad());
                }
                self.claim(&i, Domain::Instance)?;
                if self.launches.len() >= MAX_LAUNCH_ROWS {
                    return Err(E::CapacityExceeded);
                }
                if previous.is_none() && self.runtimes.len() >= MAX_RUNTIME_ROWS {
                    return Err(E::CapacityExceeded);
                }
                self.launches.insert(
                    i,
                    Launch {
                        session: session.to_owned(),
                        generation,
                        authority: source,
                        ready: false,
                        exited: false,
                    },
                );
            } else {
                // Same generation: retain the live launch, or stop with no new reservation.
                if running {
                    let i = prev_instance.clone().ok_or_else(bad)?;
                    if instance.as_deref() != Some(i.as_str())
                        || self
                            .launches
                            .get(&i)
                            .is_none_or(|l| l.exited || l.authority != source)
                    {
                        return Err(bad());
                    }
                } else if instance.is_some() && instance != prev_instance {
                    return Err(bad());
                }
                if previous.is_none() && (running || instance.is_some()) {
                    return Err(bad());
                }
                if previous.is_none() && self.runtimes.len() >= MAX_RUNTIME_ROWS {
                    return Err(E::CapacityExceeded);
                }
            }
            let intent = text(v, "correlation_id")?.to_owned();
            canonical(&intent).map_err(|_| bad())?;
            if self
                .intents
                .insert(
                    intent,
                    Intent {
                        session: session.to_owned(),
                        revision,
                        generation,
                        running,
                        prev_generation: prev_gen,
                        prev_instance: found_live,
                    },
                )
                .is_some()
            {
                return Err(bad());
            }
            self.runtimes.insert(
                session.to_owned(),
                Runtime {
                    revision,
                    desired_running: running,
                    generation,
                    instance: if running { instance } else { None },
                    closed,
                },
            );
            return Ok(());
        }
        let instance = uuid_text(p, "runtime_instance_id")?.to_owned();
        let generation = positive(p, "generation")?;
        let launch = self.launches.get(&instance).ok_or_else(bad)?;
        // Readiness and exit are observations by the authority that owns the allocation.
        if launch.session != session
            || launch.generation != generation
            || launch.authority != source
        {
            return Err(bad());
        }
        if kind.ends_with("instance_ready") {
            let current = self.runtimes.get(session);
            if launch.ready
                || launch.exited
                || self.sessions.get(session).copied().unwrap_or(true)
                || !current.is_some_and(|r| {
                    r.desired_running
                        && r.generation == generation
                        && r.instance.as_deref() == Some(instance.as_str())
                })
            {
                return Err(bad());
            }
            if let Some(l) = self.launches.get_mut(&instance) {
                l.ready = true;
            }
        } else {
            if launch.exited {
                return Err(bad());
            }
            let desired_running = self
                .runtimes
                .get(session)
                .is_some_and(|r| r.desired_running);
            if (p["desired_state"] == "running") != desired_running {
                return Err(bad());
            }
            if let Some(l) = self.launches.get_mut(&instance) {
                l.exited = true;
            }
        }
        Ok(())
    }

    fn turn_event(
        &mut self,
        v: &Value,
        name: &str,
        (session, instance, generation): (&str, &str, u64),
        authorized: bool,
    ) -> ControlResult<()> {
        let p = &v["payload"];
        let id = uuid_text(p, "managed_turn_id")?.to_owned();
        let revision = positive(p, "revision")?;
        let state = text(p, "state")?.to_owned();
        let expected_state = match name {
            "admitted" => "starting",
            "cancel_requested" => "",
            other => other,
        };
        if !expected_state.is_empty() && state != expected_state {
            return Err(bad());
        }
        let owner = Some((instance.to_owned(), generation));
        // Only a failure-class observation may come from a dead launch or another authority.
        let recovery = matches!(name, "interrupted" | "failed" | "indeterminate");
        if !authorized && !recovery {
            return Err(bad());
        }
        match self.turns.get_mut(&id) {
            None => {
                if name != "admitted" || revision != 1 || !authorized {
                    return Err(bad());
                }
                let active = self
                    .turns
                    .values()
                    .any(|t| t.session == session && !terminal_turn(&t.state));
                if active {
                    return Err(bad());
                }
                self.turns.insert(
                    id,
                    Turn {
                        session: session.to_owned(),
                        launch: owner,
                        revision,
                        state,
                    },
                );
            }
            Some(t) => {
                if t.session != session || t.launch != owner || terminal_turn(&t.state) {
                    return Err(bad());
                }
                if name == "cancel_requested" {
                    if revision != t.revision || state != t.state {
                        return Err(bad());
                    }
                } else if revision != t.revision + 1
                    || name == "admitted"
                    || !matches!(
                        (t.state.as_str(), state.as_str()),
                        (
                            "starting",
                            "running" | "failed" | "interrupted" | "indeterminate"
                        ) | (
                            "running",
                            "completed" | "failed" | "interrupted" | "indeterminate"
                        )
                    )
                {
                    return Err(bad());
                } else {
                    t.revision = revision;
                    t.state = state;
                }
            }
        }
        Ok(())
    }

    fn approval_event(
        &mut self,
        v: &Value,
        name: &str,
        (session, instance, generation): (&str, &str, u64),
        authorized: bool,
    ) -> ControlResult<()> {
        let p = &v["payload"];
        let id = uuid_text(p, "managed_approval_id")?.to_owned();
        let turn = uuid_text(p, "managed_turn_id")?.to_owned();
        let owner = Some((instance.to_owned(), generation));
        let next = approval_head(v, session, &turn, owner.clone())?;
        // Observations which cannot have been caused by a client decision on a live launch.
        let passive = matches!(name, "expired" | "invalidated" | "indeterminate");
        if !authorized && !passive {
            return Err(bad());
        }
        let bound = self.turns.get(&turn).ok_or_else(bad)?;
        if bound.session != session || bound.launch != owner {
            return Err(bad());
        }
        match self.approvals.get(&id) {
            None => {
                if name != "requested" || next.revision != 1 || terminal_turn(&bound.state) {
                    return Err(bad());
                }
                // One pending approval per session, as the unique index promises.
                if self
                    .approvals
                    .values()
                    .any(|a| a.session == session && a.state == "pending")
                {
                    return Err(bad());
                }
            }
            Some(old) => {
                if old.session != session
                    || old.launch != owner
                    || old.turn != turn
                    || !approval_step(old, &next, &format!("authority.managed_approval.{name}"))
                {
                    return Err(bad());
                }
            }
        }
        self.approvals.insert(id, next);
        Ok(())
    }

    /// Session to (revision, desired running, generation).
    pub fn runtimes(&self) -> BTreeMap<String, (u64, bool, u64)> {
        self.runtimes
            .iter()
            .map(|(s, r)| (s.clone(), (r.revision, r.desired_running, r.generation)))
            .collect()
    }
    /// Instance to (session, generation, reserving authority).
    pub fn launches(&self) -> BTreeMap<String, (String, u64, String)> {
        self.launches
            .iter()
            .map(|(i, l)| {
                (
                    i.clone(),
                    (l.session.clone(), l.generation, l.authority.clone()),
                )
            })
            .collect()
    }
    /// Turn to (session, launch). `None` launch: unassigned baseline history.
    pub fn turn_launches(&self) -> BTreeMap<String, (String, Option<(String, u64)>)> {
        self.turns
            .iter()
            .map(|(i, t)| (i.clone(), (t.session.clone(), t.launch.clone())))
            .collect()
    }
    /// Approval to (session, turn, launch). `None` launch: unassigned baseline history.
    #[allow(clippy::type_complexity)]
    pub fn approval_launches(&self) -> BTreeMap<String, (String, String, Option<(String, u64)>)> {
        self.approvals
            .iter()
            .map(|(i, a)| {
                (
                    i.clone(),
                    (a.session.clone(), a.turn.clone(), a.launch.clone()),
                )
            })
            .collect()
    }
    /// Every generation-aware runtime mutation the stream recorded.
    pub fn intents(&self) -> &HashMap<String, Intent> {
        &self.intents
    }
    /// Baseline runtime mutations (intent to the revision they produced).
    pub fn legacy_intents(&self) -> &HashMap<String, u64> {
        &self.legacy_intents
    }
    /// Unresolved work as seen by the stream's last writer.
    pub fn unresolved(&self) -> Unresolved {
        self.unresolved_for(self.writer.as_deref().unwrap_or_default())
    }
    /// Non-terminal turns and pending approvals whose launch `current` does not own and
    /// cannot progress: a launch of a predecessor authority is never live. Read-only.
    pub fn unresolved_for(&self, current: &str) -> Unresolved {
        let gone = |o: &Option<(String, u64)>, s: &str| {
            o.as_ref()
                .is_some_and(|(i, _)| !self.live_for(s, i, current))
        };
        let mut out = Unresolved::default();
        for (id, t) in &self.turns {
            if !terminal_turn(&t.state) && gone(&t.launch, &t.session) {
                out.turns.push(id.clone());
            }
        }
        for (id, a) in &self.approvals {
            if matches!(a.state.as_str(), "pending") && gone(&a.launch, &a.session) {
                out.approvals.push(id.clone());
            }
        }
        out.turns.sort();
        out.approvals.sort();
        out
    }
}

fn terminal_turn(state: &str) -> bool {
    !matches!(state, "starting" | "running")
}
fn approval_head(
    v: &Value,
    session: &str,
    turn: &str,
    launch: Option<(String, u64)>,
) -> ControlResult<Approval> {
    let p = &v["payload"];
    Ok(Approval {
        session: session.to_owned(),
        turn: turn.to_owned(),
        launch,
        revision: positive(p, "revision")?,
        state: text(p, "state")?.to_owned(),
        effect: text(p, "effect")?.to_owned(),
        fingerprint: text(p, "action_fingerprint")?.to_owned(),
    })
}
/// One approval advances by exactly one revision, keeps its turn and action, and leaves a
/// terminal state only for the confirmation or indeterminacy of an accepted dispatch.
fn approval_step(old: &Approval, next: &Approval, kind: &str) -> bool {
    let name = kind.rsplit('.').next().unwrap_or_default();
    next.revision == old.revision + 1
        && next.turn == old.turn
        && next.fingerprint == old.fingerprint
        && match old.state.as_str() {
            "pending" => matches!(
                name,
                "accepted" | "declined" | "cancelled" | "expired" | "invalidated"
            ),
            "accepted" => {
                old.effect == "dispatched" && matches!(name, "confirmed" | "indeterminate")
            }
            _ => false,
        }
}
