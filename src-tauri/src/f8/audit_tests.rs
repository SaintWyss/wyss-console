use super::audit::*;
use crate::contracts::control::ControlError as E;
use serde_json::{Value, json};

const STREAM: &str = "00000000-0000-4000-8000-0000000000f1";
const AUTH: &str = "00000000-0000-4000-8000-0000000000a1";
const AUTH2: &str = "00000000-0000-4000-8000-0000000000a2";
const S1: &str = "00000000-0000-4000-8000-000000000051";
const S2: &str = "00000000-0000-4000-8000-000000000052";
const I1: &str = "00000000-0000-4000-8000-000000000061";
const I2: &str = "00000000-0000-4000-8000-000000000062";
const I3: &str = "00000000-0000-4000-8000-000000000063";
const T1: &str = "00000000-0000-4000-8000-000000000071";
const T2: &str = "00000000-0000-4000-8000-000000000072";
const AP1: &str = "00000000-0000-4000-8000-000000000081";
const AP2: &str = "00000000-0000-4000-8000-000000000082";
const LEASE: &str = "00000000-0000-4000-8000-000000000091";
const CORR: &str = "00000000-0000-4000-8000-0000000000c1";
const FP: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Builds a chain of events with consecutive sequences and unique IDs.
struct Log {
    events: Vec<Value>,
}
impl Log {
    fn new() -> Self {
        Self { events: vec![] }
    }
    fn raw(
        &mut self,
        kind: &str,
        session: Option<&str>,
        corr: Option<&str>,
        version: u64,
        payload: Value,
        source: &str,
    ) -> &mut Self {
        let seq = self.events.len() as u64 + 1;
        self.events.push(json!({"schema_version":1,
            "event_id":format!("00000000-0000-4000-8000-{:012x}", 0x1000 + seq),
            "event_type":kind,"payload_schema_version":version,
            "timestamp":"2026-10-20T10:00:00.000Z","stream_id":STREAM,"sequence":seq.to_string(),
            "session_id":session,"source":{"component":"wyss_control","instance_id":source},
            "correlation_id":corr,"causation_id":null,"payload":payload,
            "persistence":"durable","sensitivity":"internal","redaction":"none"}));
        self
    }
    fn prepared(&mut self, s: &str) -> &mut Self {
        self.raw("authority.managed_session.prepared", Some(s), Some(CORR), 1,
            json!({"adapter_id":"codex","runtime_kind":"codex","control_state":"prepared","revision":1}), AUTH)
    }
    fn closed(&mut self, s: &str) -> &mut Self {
        self.raw("authority.managed_session.closed", Some(s), Some(CORR), 1,
            json!({"adapter_id":"codex","runtime_kind":"codex","control_state":"closed","revision":2}), AUTH)
    }
    fn desired(
        &mut self,
        s: &str,
        running: bool,
        rev: u64,
        generation: u64,
        inst: Option<&str>,
        src: &str,
    ) -> &mut Self {
        let intent = format!(
            "00000000-0000-4000-8000-{:012x}",
            0xc000 + self.events.len()
        );
        self.raw("authority.runtime.desired_state_changed", Some(s), Some(&intent), 2,
            json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":if running {"running"} else {"stopped"},
                "revision":rev,"generation":generation,"runtime_instance_id":inst}), src)
    }
    fn ready(&mut self, s: &str, inst: &str, generation: u64, src: &str) -> &mut Self {
        self.raw("authority.runtime.instance_ready", Some(s), None, 2,
            json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":inst,"generation":generation,
                "runtime_version":"0.159.0","compatibility":"compatible"}), src)
    }
    fn exited(
        &mut self,
        s: &str,
        inst: &str,
        generation: u64,
        (kind, budget, desired): (&str, Value, &str),
        src: &str,
    ) -> &mut Self {
        self.raw("authority.runtime.instance_exited", Some(s), None, 2,
            json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":inst,"generation":generation,
                "exit_kind":kind,"expected":false,"desired_state":desired,"budget_failure":budget}), src)
    }
    fn turn(
        &mut self,
        s: &str,
        t: &str,
        (inst, generation): (&str, u64),
        name: &str,
        rev: u64,
        src: &str,
    ) -> &mut Self {
        let state = match name {
            "admitted" => "starting",
            "cancel_requested" => "running",
            n => n,
        };
        self.raw(&format!("authority.managed_turn.{name}"), Some(s), Some(CORR), 2,
            json!({"managed_turn_id":t,"codex_turn_id":null,"lease_id":LEASE,"lease_revision":1,
                "runtime_instance_id":inst,"state":state,"revision":rev,"reason":null,"generation":generation}), src)
    }
    fn approval(
        &mut self,
        s: &str,
        (a, t): (&str, &str),
        (inst, generation): (&str, u64),
        name: &str,
        rev: u64,
        src: &str,
    ) -> &mut Self {
        let (state, effect, decision, reason, intent) = match name {
            "requested" => ("pending", "none", Value::Null, Value::Null, false),
            "accepted" => (
                "accepted",
                "dispatched",
                json!("accept"),
                json!("client_decision"),
                true,
            ),
            "declined" => (
                "declined",
                "none",
                json!("decline"),
                json!("client_decision"),
                true,
            ),
            "confirmed" => (
                "accepted",
                "confirmed",
                json!("accept"),
                json!("client_decision"),
                false,
            ),
            "indeterminate" => (
                "indeterminate",
                "unknown",
                json!("accept"),
                json!("effect_unknown"),
                false,
            ),
            "expired" => (
                "expired",
                "none",
                Value::Null,
                json!("approval_timeout"),
                false,
            ),
            "invalidated" => (
                "invalidated",
                "none",
                Value::Null,
                json!("runtime_lost"),
                false,
            ),
            other => panic!("{other}"),
        };
        self.raw(&format!("authority.managed_approval.{name}"), Some(s), intent.then_some(CORR), 2,
            json!({"managed_approval_id":a,"managed_turn_id":t,"kind":"command_execution","lease_id":LEASE,
                "lease_revision":1,"runtime_instance_id":inst,"state":state,"revision":rev,"decision":decision,
                "effect":effect,"reason":reason,"action_fingerprint":FP,"generation":generation}), src)
    }
    /// Two sessions, S1 running generation 1 on I1 and ready.
    fn live() -> Self {
        let mut l = Self::new();
        l.prepared(S1)
            .prepared(S2)
            .desired(S1, true, 1, 1, Some(I1), AUTH)
            .ready(S1, I1, 1, AUTH);
        l
    }
    fn replay(&self) -> Result<AuditReplay, E> {
        let mut r = AuditReplay::default();
        for e in &self.events {
            r.observe(e)?;
        }
        Ok(r)
    }
}

#[test]
fn complete_generation_aware_lifecycle_replays_and_leaves_nothing_unresolved() {
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "requested", 1, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "accepted", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "confirmed", 3, AUTH)
        .turn(S1, T1, (I1, 1), "completed", 3, AUTH)
        .desired(S1, false, 2, 1, None, AUTH)
        .exited(S1, I1, 1, ("child_exited", Value::Null, "stopped"), AUTH);
    let r = l.replay().unwrap();
    assert_eq!(
        (r.head(), r.attributed, r.historical),
        (l.events.len() as u64, 10, 2)
    );
    assert_eq!(r.unresolved(), Unresolved::default());
    // Deterministic: a second replay of the same bytes reaches the same conclusion.
    assert_eq!(l.replay().unwrap().head(), r.head());
}

#[test]
fn historical_vocabulary_one_records_validate_in_original_shape_only() {
    let mut l = Log::new();
    l.prepared(S1).raw(
        "authority.runtime.desired_state_changed",
        None,
        Some(CORR),
        1,
        json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":"running","revision":1}),
        AUTH,
    );
    assert_eq!(l.replay().unwrap().historical, 2);
    // The same baseline shape cannot be relabelled version 2 or acquire fields.
    let mut v2 = l.events[1].clone();
    v2["payload_schema_version"] = json!(2);
    assert!(validate_event(&v2).is_err());
    let mut extra = l.events[1].clone();
    extra["payload"]["generation"] = json!(1);
    assert!(validate_event(&extra).is_err());
    // After the first generation-aware event a baseline runtime event is a forgery.
    let mut forged = Log::new();
    forged.prepared(S1).desired(S1, true, 1, 1, Some(I1), AUTH)
        .raw("authority.runtime.instance_ready", None, None, 1,
            json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":I1,"runtime_version":"0.159.0","compatibility":"compatible"}), AUTH);
    assert_eq!(forged.replay().err(), Some(E::StorageIncompatible));
}

#[test]
fn closed_vocabulary_unknown_versions_types_and_fields_refuse() {
    let l = Log::live();
    for (i, mutate) in [
        (3usize, json!({"payload_schema_version":3})),
        (3, json!({"payload_schema_version":2.5})),
        (3, json!({"event_type":"authority.runtime.unknown"})),
        (0, json!({"payload_schema_version":2})),
    ] {
        let mut v = l.events[i].clone();
        for (k, val) in mutate.as_object().unwrap() {
            v[k] = val.clone();
        }
        assert!(validate_event(&v).is_err(), "{mutate}");
    }
    for key in ["adapter_id", "generation", "runtime_version"] {
        let mut v = l.events[3].clone();
        v["payload"].as_object_mut().unwrap().remove(key);
        assert!(validate_event(&v).is_err(), "missing {key}");
    }
    let mut v = l.events[3].clone();
    v["payload"]["pid"] = json!(1234);
    assert!(validate_event(&v).is_err());
    for bad in ["1.", ".1", "v1", "", &"1".repeat(129)] {
        let mut v = l.events[3].clone();
        v["payload"]["runtime_version"] = json!(bad);
        assert!(validate_event(&v).is_err(), "{bad}");
    }
    let mut v = l.events[3].clone();
    v["payload"]["compatibility"] = json!("trusted");
    assert!(validate_event(&v).is_err());
    let mut v = l.events[3].clone();
    v["session_id"] = Value::Null;
    assert!(
        validate_event(&v).is_err(),
        "no new runtime event uses a null session"
    );
}

#[test]
fn identity_domains_must_be_distinct() {
    let l = Log::live();
    let mut v = l.events[2].clone();
    v["payload"]["runtime_instance_id"] = json!(S1);
    assert!(validate_event(&v).is_err());
    let mut v = l.events[2].clone();
    v["payload"]["runtime_instance_id"] = json!(AUTH);
    assert!(validate_event(&v).is_err());
    let mut v = l.events[2].clone();
    v["session_id"] = json!(AUTH);
    assert!(validate_event(&v).is_err());
    let mut v = l.events[2].clone();
    v["payload"]["runtime_instance_id"] = json!("00000000-0000-4000-8000-0000000000AB");
    assert!(validate_event(&v).is_err());
}

#[test]
fn forged_duplicated_reordered_and_cross_stream_events_refuse() {
    let l = Log::live();
    let mut gap = l.events.clone();
    gap.remove(1);
    assert!(
        gap.iter()
            .try_fold(AuditReplay::default(), |mut r, e| r.observe(e).map(|_| r))
            .is_err()
    );
    let mut swapped = l.events.clone();
    swapped.swap(2, 3);
    assert!(
        swapped
            .iter()
            .try_fold(AuditReplay::default(), |mut r, e| r.observe(e).map(|_| r))
            .is_err()
    );
    let mut dup = Log::live();
    let first = dup.events[0].clone();
    let mut again = first.clone();
    again["sequence"] = json!("5");
    dup.events.push(again);
    assert!(dup.replay().is_err(), "a repeated event id");
    let mut other = Log::live();
    other.events[3]["stream_id"] = json!("00000000-0000-4000-8000-0000000000f2");
    assert!(other.replay().is_err(), "a second stream identity");
    let mut conflict = Log::live();
    conflict
        .desired(S1, true, 2, 1, Some(I1), AUTH)
        .desired(S1, true, 2, 1, Some(I1), AUTH);
    assert!(
        conflict.replay().is_err(),
        "conflicting revision for one runtime"
    );
}

#[test]
fn generation_and_instance_lifetimes_are_monotonic_and_never_reused() {
    let mut ok = Log::live();
    ok.exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH)
        .desired(S1, true, 2, 2, Some(I2), AUTH);
    ok.replay().unwrap();
    for (generation, inst, rev) in [(3, I2, 2), (2, I1, 2), (0, I2, 2), (1, I1, 3)] {
        let mut l = Log::live();
        l.exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH);
        l.desired(S1, true, rev, generation, Some(inst), AUTH);
        assert!(l.replay().is_err(), "gen {generation} {inst} rev {rev}");
    }
    // A new attempt while the previous launch is still live is not a reservation.
    let mut l = Log::live();
    l.desired(S1, true, 2, 2, Some(I2), AUTH);
    l.replay().unwrap();
    // Revision gap, and a retained generation that names a different live launch.
    let mut l = Log::live();
    l.desired(S1, true, 3, 1, Some(I1), AUTH);
    assert!(l.replay().is_err());
    let mut l = Log::live();
    l.desired(S1, true, 2, 1, Some(I2), AUTH);
    assert!(l.replay().is_err());
    // Retaining an exited launch as if still running.
    let mut l = Log::live();
    l.exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH)
        .desired(S1, true, 2, 1, Some(I1), AUTH);
    assert!(l.replay().is_err());
    // Generation overflow cannot be represented.
    let mut v = Log::live().events[2].clone();
    v["payload"]["generation"] = json!(9223372036854775808u64);
    assert!(validate_event(&v).is_err());
    v["payload"]["generation"] = json!(9223372036854775807u64);
    validate_event(&v).unwrap();
}

#[test]
fn a_closed_session_cannot_reserve_another_launch() {
    let mut l = Log::live();
    l.closed(S1).desired(S1, true, 2, 2, Some(I2), AUTH);
    assert!(l.replay().is_err());
    let mut l = Log::live();
    l.closed(S1).desired(S1, false, 2, 1, None, AUTH);
    l.replay().unwrap();
}

#[test]
fn attribution_cannot_cross_sessions_or_name_unknown_launches() {
    let mut l = Log::live();
    l.turn(S2, T1, (I1, 1), "admitted", 1, AUTH);
    assert!(l.replay().is_err(), "S2 naming S1's launch");
    let mut l = Log::live();
    l.turn(S1, T1, (I2, 1), "admitted", 1, AUTH);
    assert!(l.replay().is_err(), "launch the log never reserved");
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 2), "admitted", 1, AUTH);
    assert!(l.replay().is_err(), "wrong generation");
    let mut l = Log::new();
    l.prepared(S1).turn(S2, T1, (I1, 1), "admitted", 1, AUTH);
    assert!(l.replay().is_err(), "session never prepared");
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH);
    let mut cross = l.events.clone();
    cross[5]["session_id"] = json!(S2);
    assert!(
        cross
            .iter()
            .try_fold(AuditReplay::default(), |mut r, e| r.observe(e).map(|_| r))
            .is_err(),
        "turn identity is immutable"
    );
    let mut moved = Log::live();
    moved
        .desired(S1, true, 2, 1, Some(I1), AUTH)
        .turn(S1, T1, (I1, 1), "admitted", 1, AUTH);
    let mut m = moved.events.clone();
    m[5]["payload"]["generation"] = json!(2);
    assert!(
        m.iter()
            .try_fold(AuditReplay::default(), |mut r, e| r.observe(e).map(|_| r))
            .is_err()
    );
}

#[test]
fn work_requires_the_current_live_launch_of_the_owning_authority() {
    // Exited launch, stale generation, and a foreign authority cannot start or progress work.
    let mut l = Log::live();
    l.exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH)
        .turn(S1, T1, (I1, 1), "admitted", 1, AUTH);
    assert!(l.replay().is_err());
    let mut l = Log::live();
    l.exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH)
        .desired(S1, true, 2, 2, Some(I2), AUTH)
        .turn(S1, T1, (I1, 1), "admitted", 1, AUTH);
    assert!(l.replay().is_err(), "stale generation");
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH2);
    assert!(l.replay().is_err(), "foreign authority");
    let mut l = Log::live();
    l.desired(S1, false, 2, 1, None, AUTH)
        .turn(S1, T1, (I1, 1), "admitted", 1, AUTH);
    assert!(l.replay().is_err(), "desired stopped");
    // A reserved launch which was never attested ready cannot admit work.
    let mut l = Log::new();
    l.prepared(S1).desired(S1, true, 1, 1, Some(I1), AUTH).turn(
        S1,
        T1,
        (I1, 1),
        "admitted",
        1,
        AUTH,
    );
    assert!(l.replay().is_err());
}

#[test]
fn late_approvals_after_restart_or_exit_are_refused_but_recovery_is_recorded() {
    let mut base = Log::live();
    base.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "requested", 1, AUTH);
    let crash = |l: &mut Log| {
        l.exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH);
    };
    let r = {
        let mut l = Log::new();
        l.events = base.events.clone();
        crash(&mut l);
        l.replay().unwrap()
    };
    assert_eq!(
        r.unresolved(),
        Unresolved {
            turns: vec![T1.into()],
            approvals: vec![AP1.into()]
        }
    );
    // A decision, completion or progress on the dead launch is refused, from any authority.
    for src in [AUTH, AUTH2] {
        for name in ["accepted", "declined"] {
            let mut l = Log::new();
            l.events = base.events.clone();
            crash(&mut l);
            l.approval(S1, (AP1, T1), (I1, 1), name, 2, src);
            assert!(l.replay().is_err(), "{name} from {src}");
        }
        let mut l = Log::new();
        l.events = base.events.clone();
        crash(&mut l);
        l.turn(S1, T1, (I1, 1), "completed", 3, src);
        assert!(l.replay().is_err(), "completed from {src}");
    }
    // A successor authority records the unknown effect; replay then has nothing left to resolve.
    let mut l = Log::new();
    l.events = base.events.clone();
    crash(&mut l);
    l.approval(S1, (AP1, T1), (I1, 1), "expired", 2, AUTH2)
        .turn(S1, T1, (I1, 1), "indeterminate", 3, AUTH2);
    assert_eq!(l.replay().unwrap().unresolved(), Unresolved::default());
    // A restart that reserves a new generation never inherits the predecessor's approval.
    let mut l = Log::new();
    l.events = base.events.clone();
    crash(&mut l);
    l.desired(S1, true, 2, 2, Some(I2), AUTH2).approval(
        S1,
        (AP1, T1),
        (I1, 1),
        "accepted",
        2,
        AUTH2,
    );
    assert!(l.replay().is_err());
}

#[test]
fn turn_and_approval_state_machines_are_enforced() {
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T2, (I1, 1), "admitted", 1, AUTH);
    assert!(l.replay().is_err(), "second active turn in one session");
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "completed", 3, AUTH);
    assert!(l.replay().is_err(), "revision skip");
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .turn(S1, T1, (I1, 1), "completed", 3, AUTH)
        .turn(S1, T1, (I1, 1), "running", 4, AUTH);
    assert!(l.replay().is_err(), "resurrection after terminal");
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "running", 1, AUTH);
    assert!(l.replay().is_err(), "first event must be admission");
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "requested", 1, AUTH)
        .approval(S1, (AP2, T1), (I1, 1), "requested", 1, AUTH);
    assert!(
        l.replay().is_err(),
        "second pending approval in one session"
    );
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "requested", 1, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "declined", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "accepted", 3, AUTH);
    assert!(l.replay().is_err(), "decision after a terminal decision");
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "requested", 1, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "accepted", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "indeterminate", 3, AUTH);
    l.replay().unwrap();
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "requested", 1, AUTH);
    let mut tail = Log::new();
    tail.events = l.events.clone();
    tail.approval(S1, (AP1, T1), (I1, 1), "accepted", 2, AUTH);
    tail.events[7]["payload"]["action_fingerprint"] = json!("f".repeat(64));
    assert!(tail.replay().is_err(), "the approved action cannot change");
}

#[test]
fn exits_are_owner_observations_with_closed_budget_diagnostics() {
    let mut ok = Log::live();
    ok.exited(
        S1,
        I1,
        1,
        ("budget_exceeded", json!("turn_output_bytes"), "running"),
        AUTH,
    );
    ok.replay().unwrap();
    for (kind, budget) in [
        ("budget_exceeded", Value::Null),
        ("budget_exceeded", json!("contract_version")),
        ("budget_exceeded", json!("made_up")),
        ("child_exited", json!("turn_output_bytes")),
        ("oom", Value::Null),
    ] {
        let mut l = Log::live();
        l.exited(S1, I1, 1, (kind, budget.clone(), "running"), AUTH);
        assert!(l.replay().is_err(), "{kind} {budget}");
    }
    let mut l = Log::live();
    l.exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH2);
    assert!(
        l.replay().is_err(),
        "a successor must not fabricate a historical exit"
    );
    let mut l = Log::live();
    l.exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH)
        .exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH);
    assert!(l.replay().is_err(), "double exit");
    let mut l = Log::live();
    l.exited(S1, I1, 1, ("child_exited", Value::Null, "stopped"), AUTH);
    assert!(
        l.replay().is_err(),
        "exit disagreeing with the replayed desired state"
    );
    let mut l = Log::live();
    l.ready(S1, I1, 1, AUTH);
    assert!(l.replay().is_err(), "ready twice");
    let mut l = Log::live();
    l.exited(S1, I1, 1, ("child_exited", Value::Null, "running"), AUTH)
        .ready(S1, I1, 1, AUTH);
    assert!(l.replay().is_err(), "ready after exit");
    // Budget failure in S1 leaves S2's independent runtime and work untouched.
    let mut l = Log::live();
    l.desired(S2, true, 1, 1, Some(I3), AUTH)
        .ready(S2, I3, 1, AUTH)
        .turn(S2, T2, (I3, 1), "admitted", 1, AUTH)
        .exited(
            S1,
            I1,
            1,
            (
                "budget_exceeded",
                json!("owned_root_rss_soft_kib"),
                "running",
            ),
            AUTH,
        );
    assert_eq!(l.replay().unwrap().unresolved(), Unresolved::default());
}

#[test]
fn runtime_row_and_launch_lifetime_limits_are_enforced() {
    let mut l = Log::new();
    for n in 0..129u32 {
        let s = format!("00000000-0000-4000-8000-{:012x}", 0x5000 + n);
        l.prepared(&s);
    }
    let sessions: Vec<String> = (0..129u32)
        .map(|n| format!("00000000-0000-4000-8000-{:012x}", 0x5000 + n))
        .collect();
    for (n, s) in sessions.iter().enumerate().take(128) {
        let i = format!("00000000-0000-4000-8000-{:012x}", 0x6000 + n as u32);
        l.desired(s, true, 1, 1, Some(&i), AUTH);
    }
    l.replay().unwrap();
    let i = format!("00000000-0000-4000-8000-{:012x}", 0x7000);
    l.desired(&sessions[128], true, 1, 1, Some(&i), AUTH);
    assert_eq!(l.replay().err(), Some(E::CapacityExceeded));
}

#[test]
fn page_checks_bind_stream_cursor_limit_and_continuation() {
    use crate::contracts::{Sequence, WireUuid, event_envelope::EventEnvelopeV1};
    let l = Log::live();
    let events: Vec<EventEnvelopeV1> = l
        .events
        .iter()
        .map(|e| EventEnvelopeV1::from_value(e.clone()).unwrap())
        .collect();
    let stream = WireUuid::try_from(STREAM.to_owned()).unwrap();
    let other = WireUuid::try_from(AUTH.to_owned()).unwrap();
    let seq = |n: u64| n.to_string().parse::<Sequence>().unwrap();
    check_page(&stream, None, 10, &events, None).unwrap();
    check_page(&stream, Some(seq(1)), 10, &events[1..], Some(seq(4))).unwrap();
    assert_eq!(
        check_page(&other, None, 10, &events, None),
        Err(E::CorrelationError)
    );
    assert_eq!(
        check_page(&stream, None, 3, &events, None),
        Err(E::CorrelationError)
    );
    assert_eq!(
        check_page(&stream, Some(seq(2)), 10, &events[1..], None),
        Err(E::CorrelationError)
    );
    assert_eq!(
        check_page(&stream, None, 10, &events[1..], None),
        Err(E::CorrelationError)
    );
    assert_eq!(
        check_page(&stream, None, 10, &events, Some(seq(2))),
        Err(E::CorrelationError)
    );
    let dup = vec![events[0].clone(), events[0].clone()];
    assert!(check_page(&stream, None, 10, &dup, None).is_err());
}

#[test]
fn protocol_two_audit_responses_parse_and_correlate_with_their_request() {
    use super::protocol::{Request, Response};
    use crate::contracts::WireUuid;
    let id = |s: &str| WireUuid::try_from(s.to_owned()).unwrap();
    let request = |stream: &str, after: Value| {
        Request::parse(
            &serde_json::to_vec(
                &json!({"schema_version":2,"request_id":CORR,"authority_instance_id":AUTH,
            "method":"audit/read","params":{"stream_id":stream,"after_sequence":after,"limit":10}}),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let response = |events: &[Value], next: Value| {
        serde_json::to_vec(
            &json!({"schema_version":2,"request_id":CORR,"authority_instance_id":AUTH,
            "result":{"kind":"audit","value":{"events":events,"next":next}}}),
        )
        .unwrap()
    };
    let l = Log::live();
    let ok = Response::parse(&response(&l.events, Value::Null)).unwrap();
    ok.correlate(&request(STREAM, Value::Null), &id(AUTH))
        .unwrap();
    // Round trip is byte-stable through the strict codec.
    assert_eq!(
        Response::parse(&ok.encode().unwrap())
            .unwrap()
            .encode()
            .unwrap(),
        ok.encode().unwrap()
    );
    let skipped = Response::parse(&response(&l.events[1..], Value::Null)).unwrap();
    assert_eq!(
        skipped.correlate(&request(STREAM, Value::Null), &id(AUTH)),
        Err(E::CorrelationError)
    );
    assert_eq!(
        ok.correlate(&request(AUTH2, Value::Null), &id(AUTH)),
        Err(E::CorrelationError)
    );
    skipped
        .correlate(&request(STREAM, json!("1")), &id(AUTH))
        .unwrap();
    // A forged vocabulary-2 payload is refused at decode, before any correlation.
    let mut forged = l.events.clone();
    forged[3]["payload"]["pid"] = json!(1);
    assert!(Response::parse(&response(&forged, Value::Null)).is_err());
    let mut unknown = l.events.clone();
    unknown[3]["payload_schema_version"] = json!(9);
    assert!(Response::parse(&response(&unknown, Value::Null)).is_err());
    // A protocol-1 response is never accepted by the protocol-2 codec.
    let mut v1 = serde_json::from_slice::<Value>(&response(&l.events, Value::Null)).unwrap();
    v1["schema_version"] = json!(1);
    assert_eq!(
        Response::parse(&serde_json::to_vec(&v1).unwrap()).err(),
        Some(E::ProtocolVersionUnsupported)
    );
}

#[test]
fn one_mutation_intent_produces_exactly_one_runtime_event() {
    let mut l = Log::live();
    let mut again = l.events[2].clone();
    again["event_id"] = json!("00000000-0000-4000-8000-0000000000e1");
    again["sequence"] = json!((l.events.len() + 1).to_string());
    again["payload"]["revision"] = json!(2);
    l.events.push(again);
    assert!(
        l.replay().is_err(),
        "an intent replayed as a second state change"
    );
}

fn folds(events: &[Value]) -> Result<AuditReplay, E> {
    events.iter().try_fold(AuditReplay::default(), |mut r, e| {
        r.observe(e)?;
        Ok(r)
    })
}
/// A baseline-shaped (payload version 1) turn event, as an old writer would produce.
fn v1_turn(l: &mut Log, name: &str, id: &str, rev: u64) {
    let state = if name == "admitted" { "starting" } else { name };
    l.raw(
        &format!("authority.managed_turn.{name}"),
        Some(S1),
        Some(CORR),
        1,
        json!({"managed_turn_id":id,"codex_turn_id":null,"lease_id":LEASE,"lease_revision":1,
            "runtime_instance_id":I3,"state":state,"revision":rev,"reason":null}),
        AUTH,
    );
}

#[test]
fn review_baseline_work_cannot_start_after_generation_aware_events() {
    // Before the first generation-aware event baseline work is ordinary history.
    let mut ok = Log::new();
    ok.prepared(S1);
    v1_turn(&mut ok, "admitted", T1, 1);
    v1_turn(&mut ok, "running", T1, 2);
    ok.replay().unwrap();
    // Afterwards any baseline runtime, turn or approval event is refused.
    let mut l = Log::live();
    v1_turn(&mut l, "admitted", T2, 1);
    assert!(l.replay().is_err(), "baseline turn after V8 started");
    let mut l = Log::live();
    l.raw(
        "authority.managed_approval.requested",
        Some(S1),
        None,
        1,
        json!({"managed_approval_id":AP1,"managed_turn_id":T1,"kind":"command_execution","lease_id":LEASE,
            "lease_revision":1,"runtime_instance_id":I1,"state":"pending","revision":1,"decision":null,
            "effect":"none","reason":null,"action_fingerprint":FP}),
        AUTH,
    );
    assert!(l.replay().is_err(), "baseline approval after V8 started");
    // A baseline approval must follow a known turn of the same session, starting at requested.
    let mut l = Log::new();
    l.prepared(S1).raw(
        "authority.managed_approval.requested",
        Some(S1),
        None,
        1,
        json!({"managed_approval_id":AP1,"managed_turn_id":T1,"kind":"command_execution","lease_id":LEASE,
            "lease_revision":1,"runtime_instance_id":I1,"state":"pending","revision":1,"decision":null,
            "effect":"none","reason":null,"action_fingerprint":FP}),
        AUTH,
    );
    assert!(l.replay().is_err(), "approval for an unknown turn");
}

#[test]
fn review_a_successor_cannot_progress_a_predecessors_launch_and_recovery_is_visible() {
    // The predecessor crashed with work in flight: no exit was (or could be) written.
    let mut base = Log::live();
    base.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "requested", 1, AUTH);
    let r = base.replay().unwrap();
    // Seen by the successor, the in-flight work is unresolved even though nothing exited.
    assert_eq!(
        r.unresolved_for(AUTH2),
        Unresolved {
            turns: vec![T1.into()],
            approvals: vec![AP1.into()]
        }
    );
    assert_eq!(r.unresolved_for(AUTH), Unresolved::default());
    // The successor cannot progress, answer or retain it ...
    for name in ["running", "completed"] {
        let mut l = Log::new();
        l.events = base.events.clone();
        l.turn(S1, T1, (I1, 1), name, 3, AUTH2);
        assert!(l.replay().is_err(), "{name}");
    }
    let mut l = Log::new();
    l.events = base.events.clone();
    l.approval(S1, (AP1, T1), (I1, 1), "accepted", 2, AUTH2);
    assert!(l.replay().is_err());
    let mut l = Log::new();
    l.events = base.events.clone();
    l.desired(S1, true, 2, 1, Some(I1), AUTH2);
    assert!(l.replay().is_err(), "retaining a predecessor's launch");
    // ... but it can record the loss, after which the predecessor can never write again.
    let mut l = Log::new();
    l.events = base.events.clone();
    l.approval(S1, (AP1, T1), (I1, 1), "invalidated", 2, AUTH2)
        .turn(S1, T1, (I1, 1), "indeterminate", 3, AUTH2);
    assert_eq!(
        l.replay().unwrap().unresolved_for(AUTH2),
        Unresolved::default()
    );
    let mut forged = Log::new();
    forged.events = base.events.clone();
    forged
        .turn(S1, T1, (I1, 1), "failed", 3, AUTH2)
        .turn(S1, T1, (I1, 1), "completed", 4, AUTH);
    assert!(
        forged.replay().is_err(),
        "a retired authority writing again exposes a forged successor"
    );
}

#[test]
fn review_ready_requires_a_compatible_launch_and_an_open_session() {
    for word in ["incompatible", "degraded", "unknown"] {
        let mut v = Log::live().events[3].clone();
        v["payload"]["compatibility"] = json!(word);
        assert!(validate_event(&v).is_err(), "{word}");
    }
    let mut l = Log::new();
    l.prepared(S1)
        .desired(S1, true, 1, 1, Some(I1), AUTH)
        .closed(S1)
        .ready(S1, I1, 1, AUTH);
    assert!(l.replay().is_err(), "ready after the session closed");
}

#[test]
fn review_transition_tables_match_the_sql_triggers() {
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "requested", 1, AUTH)
        .approval(S1, (AP1, T1), (I1, 1), "indeterminate", 2, AUTH);
    assert!(l.replay().is_err(), "pending cannot become indeterminate");
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "completed", 2, AUTH);
    assert!(
        l.replay().is_err(),
        "starting cannot complete without running"
    );
    let mut l = Log::live();
    l.turn(S1, T1, (I1, 1), "admitted", 1, AUTH)
        .turn(S1, T1, (I1, 1), "running", 2, AUTH)
        .turn(S1, T1, (I1, 1), "running", 3, AUTH);
    assert!(l.replay().is_err(), "running twice");
}

#[test]
fn review_identity_domains_are_disjoint_across_the_whole_stream() {
    // An instance equal to another session, and a session equal to an existing instance.
    let mut l = Log::live();
    l.desired(S2, true, 1, 1, Some(S1), AUTH);
    assert!(l.replay().is_err(), "instance reusing a session id");
    let mut l = Log::live();
    l.prepared(I1);
    assert!(l.replay().is_err(), "session reusing an instance id");
    let mut l = Log::new();
    l.prepared(AUTH2).raw(
        "authority.managed_session.prepared",
        Some(S1),
        Some(CORR),
        1,
        json!({"adapter_id":"codex","runtime_kind":"codex","control_state":"prepared","revision":1}),
        AUTH2,
    );
    assert!(
        l.replay().is_err() || folds(&l.events).is_err(),
        "authority sharing a session id"
    );
    let mut l = Log::live();
    l.desired(S2, true, 1, 1, Some(AUTH), AUTH);
    assert!(l.replay().is_err(), "instance reusing the authority id");
}

#[test]
fn review_wire_events_are_canonical_and_within_the_durable_bound() {
    let l = Log::live();
    let mut shouting = l.events[2].clone();
    let corr = shouting["correlation_id"].as_str().unwrap().to_uppercase();
    shouting["correlation_id"] = json!(corr);
    assert!(validate_event(&shouting).is_err(), "uppercase correlation");
    let mut shouting = l.events[3].clone();
    shouting["event_id"] = json!("00000000-0000-4000-8000-00000000AB01");
    assert!(validate_event(&shouting).is_err(), "uppercase event id");
    let mut t = Log::live();
    t.turn(S1, T1, (I1, 1), "admitted", 1, AUTH);
    let mut shouting = t.events[4].clone();
    shouting["payload"]["managed_turn_id"] = json!("00000000-0000-4000-8000-0000000000AB");
    assert!(validate_event(&shouting).is_err(), "uppercase turn id");
    let mut big = l.events[3].clone();
    big["padding"] = json!("x".repeat(2100));
    assert!(validate_event(&big).is_err(), "unknown and oversized");
    let mut padded = l.events[3].clone();
    padded["payload"]["runtime_version"] = json!(format!("1.{}", "1".repeat(2100)));
    assert!(validate_event(&padded).is_err(), "over the durable bound");
    validate_event(&l.events[3]).unwrap();
}

#[test]
fn review_a_refusal_poisons_the_replay() {
    let l = Log::live();
    let mut r = AuditReplay::default();
    for e in &l.events[..2] {
        r.observe(e).unwrap();
    }
    assert!(r.observe(&l.events[3]).is_err());
    // The valid next event no longer folds: nothing continues past a refusal.
    assert!(r.observe(&l.events[2]).is_err());
}

fn started(l: &mut Log, predecessor: Option<&str>, src: &str) {
    l.raw(
        crate::f8::audit::STARTED,
        None,
        None,
        2,
        json!({ "predecessor_instance_id": predecessor }),
        src,
    );
}
fn replay_with_boundary(l: &Log, upto: u64) -> Result<AuditReplay, E> {
    let mut r = AuditReplay::with_boundary(upto);
    for e in &l.events {
        r.observe(e)?;
    }
    Ok(r)
}

#[test]
fn attestation_every_authority_announces_its_predecessor_before_writing() {
    // Two baseline events, then the boundary.
    let base = || {
        let mut l = Log::new();
        l.prepared(S1).prepared(S2);
        l
    };
    let mut ok = base();
    started(&mut ok, Some(AUTH), AUTH2);
    ok.desired(S1, true, 1, 1, Some(I1), AUTH2).ready(S1, I1, 1, AUTH2);
    replay_with_boundary(&ok, 2).unwrap();
    // The first post-boundary event must be an announcement.
    let mut l = base();
    l.desired(S1, true, 1, 1, Some(I1), AUTH2);
    assert!(replay_with_boundary(&l, 2).is_err(), "unannounced writer");
    // It must name the actual previous writer, and only once.
    let mut l = base();
    started(&mut l, Some(AUTH2), AUTH2);
    assert!(replay_with_boundary(&l, 2).is_err(), "self-succession");
    let mut l = base();
    started(&mut l, Some(S1), AUTH2);
    assert!(replay_with_boundary(&l, 2).is_err(), "wrong predecessor");
    let mut l = base();
    started(&mut l, None, AUTH2);
    assert!(replay_with_boundary(&l, 2).is_err(), "null predecessor after history");
    let mut l = base();
    started(&mut l, Some(AUTH), AUTH2);
    started(&mut l, Some(AUTH), AUTH2);
    assert!(replay_with_boundary(&l, 2).is_err(), "announced twice");
    // A chain AUTH -> AUTH2 -> AUTH3 is valid; the retired AUTH2 can then never write again.
    let auth3 = "00000000-0000-4000-8000-0000000000a3";
    let mut chain = base();
    started(&mut chain, Some(AUTH), AUTH2);
    started(&mut chain, Some(AUTH2), auth3);
    replay_with_boundary(&chain, 2).unwrap();
    chain.desired(S1, true, 1, 1, Some(I1), AUTH2);
    assert!(replay_with_boundary(&chain, 2).is_err(), "retired authority wrote again");
}

#[test]
fn attestation_generation_two_cannot_predate_the_boundary_and_baseline_work_cannot_follow_it() {
    // A version-2 event inside the pre-migration range is impossible history.
    let mut l = Log::new();
    l.prepared(S1).desired(S1, true, 1, 1, Some(I1), AUTH);
    assert!(replay_with_boundary(&l, 5).is_err());
    // Baseline turn after the boundary, even by an announced authority, is a forgery.
    let mut l = Log::new();
    l.prepared(S1);
    started(&mut l, Some(AUTH), AUTH2);
    v1_turn(&mut l, "admitted", T1, 1);
    assert!(replay_with_boundary(&l, 1).is_err());
}
