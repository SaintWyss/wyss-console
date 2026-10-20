//! Fair, bounded, deterministic scheduling of per-runtime work. Synchronous and clock-injected:
//! no thread, async task, unbounded queue or timer lives here.
//!
//! Each runtime owns two lanes with fixed capacity from the reviewed contract: a *data* lane
//! (provider output) and a *control* lane (revocation, expiry, cancellation, approval answers)
//! whose capacity is reserved, so a data flood can never evict or delay a security/control
//! message. A full data lane is back pressure the producer must honour (stop reading that
//! runtime); a full control lane faults only that runtime, because a control message may never
//! be silently dropped. A worker which does not acknowledge a dispatch by the contract deadline
//! is faulted alone. Nothing here executes, grants or answers anything.
use super::budget::contract;
use crate::contracts::control::{ControlError as E, ControlResult};
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Data,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The data lane is full: the producer must stop reading this runtime until it drains.
    Backpressure,
    /// The reserved control lane overflowed: that runtime is faulted, never silently dropped.
    ControlOverflow,
    /// One item larger than its lane's per-event bound.
    TooLarge,
    UnknownRuntime,
    /// The runtime was faulted; nothing more is accepted for it.
    Faulted,
    /// A runtime with this key already has lanes.
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultReason {
    ControlOverflow,
    /// A dispatched control item was not acknowledged within the contract deadline.
    WorkerStalled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub runtime: String,
    pub reason: FaultReason,
    /// Queued items which were discarded rather than delivered. Their effects are unknown.
    pub discarded: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Dispatch<T> {
    pub runtime: String,
    pub class: Class,
    pub item: T,
    /// Milliseconds the item waited in its lane before dispatch.
    pub waited_ms: u64,
    /// A control item dispatched after the contract deadline (reported, still delivered).
    pub late: bool,
}

#[derive(Debug, Clone, Copy)]
struct Limits {
    data_events: usize,
    data_bytes: usize,
    control_events: usize,
    control_bytes: usize,
    control_event_bytes: usize,
    data_per_quantum: usize,
    control_deadline_ms: u64,
}
impl Limits {
    fn load() -> ControlResult<Self> {
        let c = contract()?;
        let get = |k: &str| c.get(k).copied().ok_or(E::InternalError);
        let n = |k: &str| get(k).and_then(|v| usize::try_from(v).map_err(|_| E::InternalError));
        Ok(Self {
            data_events: n("data_queue_events_per_runtime")?,
            data_bytes: n("data_queue_bytes_per_runtime")?,
            control_events: n("control_queue_events_per_runtime")?,
            control_bytes: n("control_queue_bytes_per_runtime")?,
            control_event_bytes: n("control_event_bytes")?,
            data_per_quantum: n("max_data_events_per_quantum")?,
            control_deadline_ms: get("control_dispatch_deadline_ms")?,
        })
    }
}

struct Lane<T> {
    data: VecDeque<(T, usize, u64)>,
    data_bytes: usize,
    control: VecDeque<(T, usize, u64)>,
    control_bytes: usize,
    /// A dispatched control item awaiting acknowledgement, with its dispatch time.
    unacked_since: Option<u64>,
    /// Control items dispatched and not yet acknowledged.
    unacked: usize,
    faulted: bool,
}
impl<T> Lane<T> {
    fn new() -> Self {
        Self {
            data: VecDeque::new(),
            data_bytes: 0,
            control: VecDeque::new(),
            control_bytes: 0,
            unacked_since: None,
            unacked: 0,
            faulted: false,
        }
    }
    fn clear(&mut self) -> usize {
        let n = self.data.len() + self.control.len();
        self.data.clear();
        self.control.clear();
        self.data_bytes = 0;
        self.control_bytes = 0;
        n
    }
}

/// The real size of a queued item. A caller can never under-declare it: the declared byte count
/// must be at least the item's own weight, so the memory ceiling cannot be dodged with zero.
pub trait Weigh {
    fn weight(&self) -> usize;
}
impl Weigh for Vec<u8> {
    fn weight(&self) -> usize {
        self.len()
    }
}
impl Weigh for u64 {
    fn weight(&self) -> usize {
        0
    }
}

pub struct Scheduler<T> {
    limits: Limits,
    lanes: BTreeMap<String, Lane<T>>,
    /// Rotating start index: round-robin fairness across runtimes.
    cursor: usize,
}

impl<T: Weigh> Scheduler<T> {
    pub fn new() -> ControlResult<Self> {
        Ok(Self {
            limits: Limits::load()?,
            lanes: BTreeMap::new(),
            cursor: 0,
        })
    }
    pub fn add_runtime(&mut self, key: &str) -> Result<(), Refusal> {
        if self.lanes.contains_key(key) {
            return Err(Refusal::Duplicate);
        }
        self.lanes.insert(key.to_owned(), Lane::new());
        Ok(())
    }
    /// Removes a runtime; the number of queued items discarded is returned (effects unknown).
    pub fn remove_runtime(&mut self, key: &str) -> usize {
        self.lanes.remove(key).map_or(0, |mut l| l.clear())
    }
    pub fn runtimes(&self) -> usize {
        self.lanes.len()
    }
    /// Bytes currently queued for one runtime: (data, control).
    pub fn queued_bytes(&self, key: &str) -> Option<(usize, usize)> {
        self.lanes.get(key).map(|l| (l.data_bytes, l.control_bytes))
    }
    /// Hard upper bound of memory the scheduler can ever hold for `runtimes` runtimes.
    pub fn memory_ceiling(&self, runtimes: usize) -> usize {
        runtimes * (self.limits.data_bytes + self.limits.control_bytes)
    }

    pub fn push(
        &mut self,
        key: &str,
        class: Class,
        bytes: usize,
        item: T,
        now_ms: u64,
    ) -> Result<(), Refusal> {
        let limits = self.limits;
        let lane = self.lanes.get_mut(key).ok_or(Refusal::UnknownRuntime)?;
        if lane.faulted {
            return Err(Refusal::Faulted);
        }
        if bytes < item.weight() {
            return Err(Refusal::TooLarge);
        }
        match class {
            Class::Data => {
                if bytes > limits.data_bytes {
                    return Err(Refusal::TooLarge);
                }
                if lane.data.len() >= limits.data_events
                    || lane.data_bytes.saturating_add(bytes) > limits.data_bytes
                {
                    return Err(Refusal::Backpressure);
                }
                lane.data_bytes += bytes;
                lane.data.push_back((item, bytes, now_ms));
            }
            Class::Control => {
                if bytes > limits.control_event_bytes {
                    return Err(Refusal::TooLarge);
                }
                if lane.control.len() >= limits.control_events
                    || lane.control_bytes.saturating_add(bytes) > limits.control_bytes
                {
                    lane.faulted = true;
                    lane.clear();
                    return Err(Refusal::ControlOverflow);
                }
                lane.control_bytes += bytes;
                lane.control.push_back((item, bytes, now_ms));
            }
        }
        Ok(())
    }

    /// One service quantum. Control items of every runtime are served first (strict priority,
    /// bounded by the reserved lane size); then each runtime receives at most
    /// `max_data_events_per_quantum` data items, rotating the starting runtime every call so no
    /// runtime is permanently first or last.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Dispatch<T>> {
        let limits = self.limits;
        let keys: Vec<String> = self.lanes.keys().cloned().collect();
        let mut out = Vec::new();
        if keys.is_empty() {
            return out;
        }
        let start = self.cursor % keys.len();
        self.cursor = self.cursor.wrapping_add(1);
        let order = (0..keys.len()).map(|i| &keys[(start + i) % keys.len()]);
        let order: Vec<&String> = order.collect();
        for key in &order {
            let Some(lane) = self.lanes.get_mut(key.as_str()) else {
                continue;
            };
            if lane.faulted {
                continue;
            }
            while let Some((item, bytes, queued)) = lane.control.pop_front() {
                lane.control_bytes -= bytes;
                let waited = now_ms.saturating_sub(queued);
                lane.unacked_since.get_or_insert(now_ms);
                lane.unacked += 1;
                out.push(Dispatch {
                    runtime: (*key).clone(),
                    class: Class::Control,
                    item,
                    waited_ms: waited,
                    late: waited > limits.control_deadline_ms,
                });
            }
        }
        for key in &order {
            let Some(lane) = self.lanes.get_mut(key.as_str()) else {
                continue;
            };
            if lane.faulted {
                continue;
            }
            for _ in 0..limits.data_per_quantum {
                let Some((item, bytes, queued)) = lane.data.pop_front() else {
                    break;
                };
                lane.data_bytes -= bytes;
                out.push(Dispatch {
                    runtime: (*key).clone(),
                    class: Class::Data,
                    item,
                    waited_ms: now_ms.saturating_sub(queued),
                    late: false,
                });
            }
        }
        out
    }

    /// The worker acknowledged `count` control dispatches. The stall clock only stops when every
    /// dispatched item is acknowledged, so an early ack cannot hide a later unacknowledged one;
    /// after a partial ack the deadline restarts for the remainder.
    pub fn ack(&mut self, key: &str, count: usize, now_ms: u64) {
        if let Some(l) = self.lanes.get_mut(key) {
            l.unacked = l.unacked.saturating_sub(count);
            l.unacked_since = (l.unacked > 0).then_some(now_ms);
        }
    }
    /// Faults every runtime whose dispatched control item is unacknowledged past the contract
    /// deadline. Only that runtime is affected; others keep their queues and progress.
    pub fn check_stalls(&mut self, now_ms: u64) -> Vec<Fault> {
        let deadline = self.limits.control_deadline_ms;
        let mut faults = Vec::new();
        for (key, lane) in &mut self.lanes {
            if lane.faulted {
                continue;
            }
            if lane
                .unacked_since
                .is_some_and(|since| now_ms.saturating_sub(since) > deadline)
            {
                lane.faulted = true;
                lane.unacked_since = None;
                lane.unacked = 0;
                faults.push(Fault {
                    runtime: key.clone(),
                    reason: FaultReason::WorkerStalled,
                    discarded: lane.clear(),
                });
            }
        }
        faults
    }
    /// Discards pending data for an explicit cancellation; reserved control is kept.
    pub fn cancel_data(&mut self, key: &str) -> usize {
        self.lanes.get_mut(key).map_or(0, |l| {
            let n = l.data.len();
            l.data.clear();
            l.data_bytes = 0;
            n
        })
    }
    /// How many more data items of up to `item_max` bytes this runtime's lane can certainly take
    /// right now (0 when full, faulted, or too few bytes remain for one maximal item).
    pub fn data_room(&self, key: &str, item_max: usize) -> usize {
        self.lanes.get(key).map_or(0, |l| {
            if l.faulted {
                0
            } else {
                let events = self.limits.data_events.saturating_sub(l.data.len());
                let bytes = self.limits.data_bytes.saturating_sub(l.data_bytes);
                events.min(bytes.checked_div(item_max).unwrap_or(events))
            }
        })
    }
    pub fn is_faulted(&self, key: &str) -> bool {
        self.lanes.get(key).is_some_and(|l| l.faulted)
    }
}

#[cfg(test)]
#[path = "scheduler_tests.rs"]
mod tests;
