//! Identity-only registry foundation. No provider handles, dispatch, lease grants or spawn API.
//! A successful route is an observation, never an execution/approval attestation.
use super::protocol::{Request, RuntimeFence, RuntimeIdentity, RuntimeRef};
use crate::contracts::{
    WireUuid,
    control::{ControlError as E, ControlResult},
    identities::ManagedSessionId,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Reserved,
    Initializing,
    Ready,
    Stopping,
}
struct Entry {
    reference: RuntimeRef,
    phase: Phase,
    identity: Option<RuntimeIdentity>,
}
pub struct Registry {
    authority: WireUuid,
    /// Active-slot bound. The production constructor always yields the reviewed bound of one;
    /// no API takes a larger, client-selected or configured value.
    limit: usize,
    entries: BTreeMap<String, Entry>,
    // Lifetime anti-reuse history is bounded independently of active slots.
    used: BTreeSet<String>,
    generations: BTreeMap<String, u64>,
}
impl Registry {
    pub fn new(authority: WireUuid) -> ControlResult<Self> {
        super::protocol::canonical(authority.as_str())?;
        Ok(Self {
            authority,
            limit: 1,
            entries: BTreeMap::new(),
            used: BTreeSet::new(),
            generations: BTreeMap::new(),
        })
    }
    /// Restart: prime anti-reuse history from the durable allocator. Nothing becomes live and
    /// no entry is restored: every previous launch is dead to this process.
    pub fn with_history(
        authority: WireUuid,
        used: impl IntoIterator<Item = String>,
        generations: impl IntoIterator<Item = (String, u64)>,
    ) -> ControlResult<Self> {
        let mut registry = Self::new(authority)?;
        for instance in used {
            super::protocol::canonical(&instance)?;
            registry.used.insert(instance);
        }
        for (session, generation) in generations {
            super::protocol::canonical(&session)?;
            registry.generations.insert(session, generation);
        }
        if registry.used.len() > 4096 || registry.generations.len() > 128 {
            return Err(E::CapacityExceeded);
        }
        Ok(registry)
    }
    /// Call only AFTER the corresponding durable allocation commits. Does not allocate a process.
    /// Production capacity remains one; no constructor takes a larger client-selected bound.
    pub fn reserve(&mut self, reference: RuntimeRef) -> ControlResult<()> {
        reference.validate()?;
        if reference.authority_instance_id != self.authority {
            return Err(E::CorrelationError);
        }
        if self.entries.len() >= self.limit
            || self.used.len() >= 4096
            || (!self
                .generations
                .contains_key(reference.managed_session_id.as_str())
                && self.generations.len() >= 128)
        {
            return Err(E::CapacityExceeded);
        }
        let key = reference.managed_session_id.as_str().to_owned();
        // A session's previous launch keeps its slot until it is reaped; it is never replaced.
        if self.entries.contains_key(&key) {
            return Err(E::CapacityExceeded);
        }
        if self.used.contains(reference.runtime_instance_id.as_str())
            || self
                .generations
                .get(&key)
                .is_some_and(|g| reference.generation <= *g)
        {
            return Err(E::StaleRevision);
        }
        self.used
            .insert(reference.runtime_instance_id.as_str().to_owned());
        self.generations.insert(key.clone(), reference.generation);
        self.entries.insert(
            key,
            Entry {
                reference,
                phase: Phase::Reserved,
                identity: None,
            },
        );
        Ok(())
    }
    pub fn initializing(&mut self, reference: &RuntimeRef) -> ControlResult<()> {
        let e = self.entry_mut(reference)?;
        if e.phase != Phase::Reserved {
            return Err(E::InvalidTransition);
        }
        e.phase = Phase::Initializing;
        Ok(())
    }
    /// Check registry readiness before writing the immutable durable ready event.
    /// No transient identity is published by this preflight.
    pub fn check_ready(&self, identity: &RuntimeIdentity) -> ControlResult<()> {
        identity.validate()?;
        if identity.authority_instance_id != self.authority {
            return Err(E::CorrelationError);
        }
        let entry = self
            .entries
            .get(identity.managed_session_id.as_str())
            .filter(|e| e.reference == identity.reference())
            .ok_or(E::StaleRevision)?;
        if entry.phase != Phase::Initializing {
            return Err(E::InvalidTransition);
        }
        Ok(())
    }
    /// Records an independently observed handshake, not a client/provider assertion of execution authority.
    pub fn ready(&mut self, identity: RuntimeIdentity) -> ControlResult<()> {
        self.check_ready(&identity)?;
        let e = self.entry_mut(&identity.reference())?;
        e.phase = Phase::Ready;
        e.identity = Some(identity);
        Ok(())
    }
    pub fn stopping(&mut self, reference: &RuntimeRef) -> ControlResult<()> {
        let e = self.entry_mut(reference)?;
        e.phase = Phase::Stopping;
        e.identity = None;
        Ok(())
    }
    /// Caller must independently prove owned reap; this layer cannot signal or attest OS liveness.
    pub fn remove_after_reap(&mut self, reference: &RuntimeRef) -> ControlResult<()> {
        let e = self.entry_mut(reference)?;
        if e.phase != Phase::Stopping {
            return Err(E::InvalidTransition);
        }
        self.entries.remove(reference.managed_session_id.as_str());
        Ok(())
    }
    pub fn observe(
        &self,
        authority: &WireUuid,
        fence: &RuntimeFence,
    ) -> ControlResult<&RuntimeIdentity> {
        if authority != &self.authority {
            return Err(E::CorrelationError);
        }
        fence.validate(false)?;
        let e = self
            .entries
            .get(fence.managed_session_id.as_str())
            .ok_or(E::StaleRevision)?;
        if e.reference.fence() != *fence {
            return Err(E::StaleRevision);
        }
        e.identity
            .as_ref()
            .filter(|_| e.phase == Phase::Ready)
            .ok_or(E::GateDenied)
    }
    /// Isolation tests only: exercises the multi-runtime logic without changing production
    /// capacity, which stays one until separate evidence supports an owner-approved increase.
    #[cfg(test)]
    pub(crate) fn with_test_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
    pub fn has_free_slot(&self) -> bool {
        self.entries.len() < self.limit
    }
    pub fn active_slots(&self) -> usize {
        self.entries.len()
    }
    pub fn generation(&self, session: &ManagedSessionId) -> Option<u64> {
        self.generations.get(session.as_str()).copied()
    }
    fn entry_mut(&mut self, r: &RuntimeRef) -> ControlResult<&mut Entry> {
        r.validate()?;
        if r.authority_instance_id != self.authority {
            return Err(E::CorrelationError);
        }
        self.entries
            .get_mut(r.managed_session_id.as_str())
            .filter(|e| e.reference == *r)
            .ok_or(E::StaleRevision)
    }
}

/// Bounded in-memory intent fingerprint/receipt cache; not a replacement for the durable journal.
/// Check replay before registry/capacity checks. Receipts never perform or authorize an effect.
pub struct ReplayCache<T> {
    entries: BTreeMap<String, ([u8; 32], T)>,
}
impl<T> Default for ReplayCache<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}
impl<T> ReplayCache<T> {
    pub fn lookup(&self, request: &Request) -> ControlResult<Option<&T>> {
        let (intent, hash) = fingerprint(request)?;
        match self.entries.get(&intent) {
            None => Ok(None),
            Some((old, value)) if *old == hash => Ok(Some(value)),
            _ => Err(E::IntentConflict),
        }
    }
    pub fn record(&mut self, request: &Request, receipt: T) -> ControlResult<()> {
        let (intent, hash) = fingerprint(request)?;
        if let Some((old, _)) = self.entries.get(&intent) {
            return if *old == hash {
                Ok(())
            } else {
                Err(E::IntentConflict)
            };
        }
        if self.entries.len() >= 4096 {
            return Err(E::CapacityExceeded);
        }
        self.entries.insert(intent, (hash, receipt));
        Ok(())
    }
}
fn fingerprint(r: &Request) -> ControlResult<(String, [u8; 32])> {
    let b = r.canonical_intent()?;
    let v: serde_json::Value = serde_json::from_slice(&b).map_err(|_| E::InvalidRequest)?;
    let intent = v["params"]["intent_id"]
        .as_str()
        .ok_or(E::InvalidRequest)?
        .to_owned();
    Ok((intent, Sha256::digest(&b).into()))
}
