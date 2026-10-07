//! Bounded, memory-only streams. No EventEnvelope or durable storage dependency.
use crate::contracts::{Sequence, control::*};
use std::collections::{BTreeMap, VecDeque};
const MAX_EVENTS: usize = 256;
const MAX_BYTES: usize = 256 * 1024;
const MAX_TURNS: usize = 8;
const DELTA_BYTES: usize = 4096;
#[derive(Default)]
struct Ring {
    events: VecDeque<ManagedTurnStreamEventV1>,
    bytes: usize,
    latest: u64,
    dropped: u64,
}
#[derive(Default)]
pub(super) struct Streams {
    rings: BTreeMap<String, Ring>,
    order: VecDeque<String>,
}
impl Streams {
    pub fn push(&mut self, t: &ManagedTurnRecord, kind: StreamKind, text: Option<&str>) {
        let key = t.managed_turn_id.as_str().to_ascii_lowercase();
        if !self.rings.contains_key(&key) {
            while self.order.len() >= MAX_TURNS {
                if let Some(old) = self.order.pop_front() {
                    self.rings.remove(&old);
                }
            }
            self.order.push_back(key.clone());
            self.rings.insert(key.clone(), Ring::default());
        }
        let ring = self.rings.get_mut(&key).unwrap();
        let mut remaining = text;
        loop {
            let chunk = remaining.map(|s| {
                let mut end = s.len().min(DELTA_BYTES);
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                &s[..end]
            });
            ring.latest = ring.latest.checked_add(1).expect("finite stream sequence");
            let e = ManagedTurnStreamEventV1 {
                schema_version: 1,
                managed_turn_id: t.managed_turn_id.clone(),
                managed_session_id: t.managed_session_id.clone(),
                codex_turn_id: t.codex_turn_id.clone(),
                sequence: ring.latest.to_string().parse().unwrap(),
                kind,
                text: chunk.map(str::to_owned),
            };
            let size = serde_json::to_vec(&e)
                .expect("typed stream serialization")
                .len();
            ring.bytes += size;
            ring.events.push_back(e);
            while ring.events.len() > MAX_EVENTS || ring.bytes > MAX_BYTES {
                let old = ring.events.pop_front().unwrap();
                ring.bytes -= serde_json::to_vec(&old).unwrap().len();
                ring.dropped += 1;
            }
            if let Some(s) = remaining {
                let used = chunk.unwrap().len();
                if used < s.len() {
                    remaining = Some(&s[used..]);
                    continue;
                }
            }
            break;
        }
    }
    pub fn page(&self, t: &ManagedTurnRecord, p: &TurnEvents) -> TurnStreamPage {
        let ring = self
            .rings
            .get(&t.managed_turn_id.as_str().to_ascii_lowercase());
        let after = p.after_sequence.map_or(0, Sequence::get);
        let earliest = ring.and_then(|r| r.events.front().map(|e| e.sequence));
        let latest = ring
            .filter(|r| r.latest > 0)
            .map(|r| r.latest.to_string().parse().unwrap());
        let gap = ring.is_none() || earliest.is_some_and(|s| after < s.get().saturating_sub(1));
        let mut events = Vec::new();
        let mut bytes = 0;
        if gap {
            let sequence = earliest.map_or(after.saturating_add(1).max(1), |s| {
                s.get().saturating_sub(1).max(1)
            });
            events.push(ManagedTurnStreamEventV1 {
                schema_version: 1,
                managed_turn_id: t.managed_turn_id.clone(),
                managed_session_id: t.managed_session_id.clone(),
                codex_turn_id: t.codex_turn_id.clone(),
                sequence: sequence.to_string().parse().unwrap(),
                kind: StreamKind::StreamGap,
                text: None,
            });
        }
        if let Some(r) = ring {
            for e in r.events.iter().filter(|e| e.sequence.get() > after) {
                let size = serde_json::to_vec(e).unwrap().len();
                // Reserve 5 KiB for response envelope, identities and pagination metadata.
                if events.len() >= usize::from(p.limit) || bytes + size > 60_000 {
                    break;
                }
                bytes += size;
                events.push(e.clone());
            }
        }
        let next_sequence = events.last().map(|e| e.sequence);
        TurnStreamPage {
            events,
            earliest_sequence: earliest,
            latest_sequence: latest,
            next_sequence,
            dropped_events: ring.map_or(0, |r| r.dropped),
            gap,
            terminal: t.state.terminal(),
            retained: ring.is_some(),
        }
    }
}
