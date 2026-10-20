//! Per-runtime resource budgets from the reviewed contract (docs/contracts/f8/resources.json).
//!
//! Every ceiling is classified honestly: *enforced* here or by a named component, a *soft
//! observation* which never terminates anything, or *not proven* (no enforcement exists yet).
//! Parent-process RSS is never presented as whole-process-tree containment. A violation belongs
//! to exactly one runtime's meter: it cannot terminate, fail or invalidate another runtime.
use crate::contracts::control::{ControlError as E, ControlResult};
use serde_json::Value;
use std::collections::BTreeMap;

const RESOURCES: &str = include_str!("../../../docs/contracts/f8/resources.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    /// A component in this crate refuses or stops at the ceiling.
    Enforced(&'static str),
    /// An observation which may be reported but never terminates a runtime.
    SoftObservation,
    /// No enforcement exists yet; releasing without it is blocked by the contract.
    NotProven,
}

/// Classification of every numeric key of the contract.
pub fn classify(key: &str) -> Option<Enforcement> {
    use Enforcement::{Enforced, NotProven, SoftObservation};
    Some(match key {
        "max_active_runtimes" => Enforced("f8::registry"),
        "max_runtime_rows" | "max_launch_rows" => Enforced("f8::allocator + SQL triggers"),
        "client_frame_bytes" => Enforced("f8::protocol (MAX_FRAME)"),
        "provider_frame_bytes"
        | "turn_output_bytes"
        | "turn_events"
        | "turn_events_per_second"
        | "turn_event_burst"
        | "turn_wall_seconds"
        | "pending_approvals_per_runtime" => Enforced("f8::budget::Meter"),
        "data_queue_events_per_runtime"
        | "data_queue_bytes_per_runtime"
        | "control_queue_events_per_runtime"
        | "control_event_bytes"
        | "control_queue_bytes_per_runtime"
        | "control_service_quantum_ms"
        | "max_data_events_per_quantum"
        | "control_dispatch_deadline_ms" => Enforced("f8::scheduler"),
        "eof_grace_ms" | "term_grace_ms" => Enforced("f8::ownership"),
        "hello_timeout_ms" | "provider_init_timeout_ms" => Enforced("f8::engine"),
        "owned_root_rss_soft_kib" | "aggregate_owned_roots_rss_soft_kib" => SoftObservation,
        "max_clients"
        | "provider_scratch_bytes"
        | "catalogue_bytes_per_runtime"
        | "catalogue_models_per_runtime"
        | "stream_turns_per_runtime"
        | "stream_events_per_turn"
        | "stream_bytes_per_turn"
        | "max_attachments" => NotProven,
        _ => return None,
    })
}

pub fn contract() -> ControlResult<BTreeMap<String, u64>> {
    let v: Value = serde_json::from_str(RESOURCES).map_err(|_| E::InternalError)?;
    v.as_object()
        .ok_or(E::InternalError)?
        .iter()
        .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n)))
        .filter(|(k, _)| k != "contract_version")
        .map(Ok)
        .collect()
}

/// The exact budget key which stopped a runtime. Always a numeric key of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Violation {
    pub key: &'static str,
}

/// One runtime's counters. Time is a caller-supplied monotonic millisecond count; a clock which
/// goes backwards is treated as no time passing, never as extra budget.
#[derive(Debug, Clone)]
pub struct Meter {
    limits: Limits,
    started_ms: u64,
    last_ms: u64,
    output_bytes: u64,
    events: u64,
    /// Token bucket in milli-tokens: capacity burst*1000, refill rate*1000 per second.
    milli_tokens: u64,
    pending_approvals: u64,
    latched: Option<Violation>,
}
#[derive(Debug, Clone, Copy)]
struct Limits {
    provider_frame: u64,
    output: u64,
    events: u64,
    rate: u64,
    burst: u64,
    wall_ms: u64,
    approvals: u64,
}

impl Meter {
    pub fn new(now_ms: u64) -> ControlResult<Self> {
        let c = contract()?;
        let get = |k: &str| c.get(k).copied().ok_or(E::InternalError);
        let limits = Limits {
            provider_frame: get("provider_frame_bytes")?,
            output: get("turn_output_bytes")?,
            events: get("turn_events")?,
            rate: get("turn_events_per_second")?,
            burst: get("turn_event_burst")?,
            wall_ms: get("turn_wall_seconds")?.saturating_mul(1000),
            approvals: get("pending_approvals_per_runtime")?,
        };
        Ok(Self {
            limits,
            started_ms: now_ms,
            last_ms: now_ms,
            output_bytes: 0,
            events: 0,
            milli_tokens: limits.burst.saturating_mul(1000),
            pending_approvals: 0,
            latched: None,
        })
    }
    pub fn violation(&self) -> Option<Violation> {
        self.latched
    }
    fn fail(&mut self, key: &'static str) -> Result<(), Violation> {
        let v = *self.latched.get_or_insert(Violation { key });
        Err(v)
    }
    fn advance(&mut self, now_ms: u64) {
        let now = now_ms.max(self.last_ms);
        let elapsed = now - self.last_ms;
        self.last_ms = now;
        let cap = self.limits.burst.saturating_mul(1000);
        self.milli_tokens = self
            .milli_tokens
            .saturating_add(elapsed.saturating_mul(self.limits.rate))
            .min(cap);
    }
    /// Wall-clock budget of the turn.
    pub fn check_wall(&mut self, now_ms: u64) -> Result<(), Violation> {
        self.advance(now_ms);
        if let Some(v) = self.latched {
            return Err(v);
        }
        if self.last_ms - self.started_ms > self.limits.wall_ms {
            return self.fail("turn_wall_seconds");
        }
        Ok(())
    }
    /// One provider event of `bytes` bytes: frame size, rate, event count and output volume.
    pub fn charge_event(&mut self, now_ms: u64, bytes: u64) -> Result<(), Violation> {
        self.check_wall(now_ms)?;
        if bytes > self.limits.provider_frame {
            return self.fail("provider_frame_bytes");
        }
        if self.milli_tokens < 1000 {
            return self.fail("turn_events_per_second");
        }
        self.milli_tokens -= 1000;
        if self.events >= self.limits.events {
            return self.fail("turn_events");
        }
        self.events += 1;
        match self.output_bytes.checked_add(bytes) {
            Some(total) if total <= self.limits.output => self.output_bytes = total,
            _ => return self.fail("turn_output_bytes"),
        }
        Ok(())
    }
    pub fn approval_opened(&mut self) -> Result<(), Violation> {
        if let Some(v) = self.latched {
            return Err(v);
        }
        if self.pending_approvals >= self.limits.approvals {
            return self.fail("pending_approvals_per_runtime");
        }
        self.pending_approvals += 1;
        Ok(())
    }
    pub fn approval_closed(&mut self) {
        self.pending_approvals = self.pending_approvals.saturating_sub(1);
    }
}

/// A soft RSS observation of the owned root only. It is data, not a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RssObservation {
    pub owned_root_kib: u64,
    pub over_soft_limit: bool,
}
pub fn observe_root_rss(owned_root_kib: u64) -> ControlResult<RssObservation> {
    let soft = contract()?
        .get("owned_root_rss_soft_kib")
        .copied()
        .ok_or(E::InternalError)?;
    Ok(RssObservation {
        owned_root_kib,
        over_soft_limit: owned_root_kib > soft,
    })
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
