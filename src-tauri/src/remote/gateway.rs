//! The gateway core: the registry of devices, one pairing offer at a time, the per-request check in
//! its fixed order, replay and rate rules, and the journal. No transport, no clock of its own (every
//! call is given `now`), no randomness of its own (it is given a source), no storage.
//!
//! Grants are made and widened only by the host-side methods (`give_at_host`, `renew_at_host`…): no
//! request, however it is signed, reaches them (GR1). The only thing a request can do to the
//! registry is revoke another device, under `devices.revoke` with a step-up (16 §5.3).
use super::{
    Refusal,
    audit::{Journal, Outcome, Record, identifier_ok},
    device::{Device, DeviceId, DeviceState},
    grants::{self, Grant, GrantRefused, GrantState, Profile, state_of},
    pairing::{Attempt, ConfirmRequest, Offer, Shown},
    primitives::{Random, hex},
    request::{
        CHALLENGE_SECONDS, Challenge, MOST_CANONICAL, MOST_CHALLENGES, Request, StepUpVerifier,
        step_up_message,
    },
};
use std::collections::BTreeMap;

/// Request ids remembered, and for how long. A full cache with nothing to forget refuses new
/// requests: an old id is never forgotten to make room.
const MOST_SEEN: usize = 65_536;
const SEEN_SECONDS: u64 = 24 * 3600;
const MOST_SOURCES: usize = 256;
const MOST_HOURS: usize = 48;
const BACKOFF_AFTER: usize = 5;
const FAILURE_WINDOW: u64 = 10 * 60;
const BACKOFF_SECONDS: u64 = 60;

/// A request that was not well formed has nothing to be refused *as*: it is not answered with a
/// grant word, and it is not journalled as a device's request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    Refused(Refusal),
    Malformed,
}

/// A request that passed every check. It is journalled before it is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
///
/// Its fields are private to this module: only `handle` makes one, so `apply_revoke` cannot be
/// handed an invented decision (a test-only constructor exists for the tests of the refusals).
pub struct Allowed {
    device: DeviceId,
    grant: Grant,
    operation: String,
    target: Option<String>,
}
impl Allowed {
    pub fn device(&self) -> &DeviceId {
        &self.device
    }
    pub fn grant(&self) -> Grant {
        self.grant
    }
    pub fn operation(&self) -> &str {
        &self.operation
    }
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }
    #[cfg(test)]
    pub(super) fn forged(
        device: DeviceId,
        grant: Grant,
        operation: &str,
        target: Option<&str>,
    ) -> Self {
        Self {
            device,
            grant,
            operation: operation.to_owned(),
            target: target.map(str::to_owned),
        }
    }
}

/// ND-13: the native dialog at the host. `None` is a dialog closed or not answered.
pub trait HostConfirm {
    fn ask(&self, request: &ConfirmRequest) -> Option<bool>;
}

#[derive(Default)]
struct Limiter {
    sources: BTreeMap<String, (Vec<u64>, u64)>,
}
impl Limiter {
    /// After five failures in ten minutes a source gets one attempt a minute.
    fn check(&mut self, source: &str, now: u64) -> Result<(), Refusal> {
        let Some((failures, last)) = self.sources.get_mut(source) else {
            return Ok(());
        };
        failures.retain(|at| now.saturating_sub(*at) < FAILURE_WINDOW);
        if failures.len() >= BACKOFF_AFTER && now.saturating_sub(*last) < BACKOFF_SECONDS {
            return Err(Refusal::RateLimited);
        }
        *last = now;
        Ok(())
    }
    fn failed(&mut self, source: &str, now: u64) {
        if !self.sources.contains_key(source) && self.sources.len() >= MOST_SOURCES {
            // Forget the quietest source rather than refuse to count a new one.
            if let Some(oldest) = self
                .sources
                .iter()
                .min_by_key(|(_, (_, last))| *last)
                .map(|(k, _)| k.clone())
            {
                self.sources.remove(&oldest);
            }
        }
        let entry = self.sources.entry(source.to_owned()).or_default();
        entry.0.push(now);
        entry.1 = now;
    }
}

pub struct Gateway<J: Journal, V: StepUpVerifier, R: Random> {
    host_fingerprint: String,
    pub(super) devices: BTreeMap<DeviceId, Device>,
    /// Fingerprints of revoked devices: refused for ever, even if a new pairing offers them again.
    revoked_keys: Vec<String>,
    offer: Option<Offer>,
    challenges: BTreeMap<String, Challenge>,
    seen: BTreeMap<String, u64>,
    limiter: Limiter,
    /// Unknown-key attempts, counted per hour: a flood cannot fill the journal.
    unknown_per_hour: BTreeMap<u64, u32>,
    journal: J,
    verifier: V,
    random: R,
}

impl<J: Journal, V: StepUpVerifier, R: Random> Gateway<J, V, R> {
    pub fn new(host_fingerprint: &str, journal: J, verifier: V, random: R) -> Self {
        Self {
            host_fingerprint: host_fingerprint.to_owned(),
            devices: BTreeMap::new(),
            revoked_keys: Vec::new(),
            offer: None,
            challenges: BTreeMap::new(),
            seen: BTreeMap::new(),
            limiter: Limiter::default(),
            unknown_per_hour: BTreeMap::new(),
            journal,
            verifier,
            random,
        }
    }

    pub fn devices(&self) -> impl Iterator<Item = &Device> {
        self.devices.values()
    }
    pub fn device(&self, id: &DeviceId) -> Option<&Device> {
        self.devices.get(id)
    }
    pub fn journal(&self) -> &J {
        &self.journal
    }
    pub fn unknown_attempts_in_hour(&self, hour: u64) -> u32 {
        self.unknown_per_hour.get(&hour).copied().unwrap_or(0)
    }
    /// Whether the gateway would listen at all: **not before the first offer** (16 §5.2.2).
    pub fn should_listen(&self) -> bool {
        self.offer.is_some() || !self.devices.is_empty()
    }

    // ---- pairing: only at the host -----------------------------------------------------------

    /// The owner chose Devices → Pair at the host. An earlier offer that was not used dies.
    pub fn begin_pairing(&mut self, now: u64) -> Shown {
        if let Some(old) = self.offer.as_mut() {
            old.cancel();
        }
        let (offer, shown) = Offer::new(&self.host_fingerprint, now, &mut self.random);
        self.offer = Some(offer);
        shown
    }

    /// A device presents its proof. Returns the six digits both screens show.
    pub fn present_pairing(&mut self, now: u64, attempt: &Attempt) -> Result<String, Refusal> {
        let offer = self.offer.as_mut().ok_or(Refusal::PairingFailed)?;
        offer.present(now, attempt)
    }

    /// The host's native confirmation, then enrolment with the default profile (LP1). A key that was
    /// revoked is refused, whatever the offer.
    pub fn confirm_pairing(
        &mut self,
        now: u64,
        host: &impl HostConfirm,
    ) -> Result<DeviceId, Refusal> {
        let offer = self.offer.as_mut().ok_or(Refusal::PairingFailed)?;
        let request = offer.confirm_request().ok_or(Refusal::PairingFailed)?;
        if self.revoked_keys.contains(&request.device_fingerprint) {
            offer.cancel();
            return Err(Refusal::PairingRefused);
        }
        let enrolment = offer.confirm(now, host.ask(&request))?;
        let grants =
            grants::give_profile(Profile::Lp1, now, 30).map_err(|_| Refusal::PairingRefused)?;
        let mut id = [0_u8; 16];
        self.random.fill(&mut id);
        let id = DeviceId::from_bytes(id);
        self.devices.insert(
            id.clone(),
            Device::new(
                id.clone(),
                enrolment.key_fingerprint,
                enrolment.name,
                enrolment.platform,
                grants,
                now,
            ),
        );
        self.offer = None;
        Ok(id)
    }

    // ---- grants and states: only at the host ------------------------------------------------

    /// Grants, made at the host. A profile that holds a kind that cannot be given (LP4) is refused
    /// whole. Existing grants are replaced, never merged.
    pub fn give_at_host(
        &mut self,
        id: &DeviceId,
        profile: Profile,
        days: u32,
        now: u64,
    ) -> Result<(), GrantRefused> {
        let held = grants::give_profile(profile, now, days)?;
        let device = self
            .devices
            .get_mut(id)
            .ok_or(GrantRefused::NotInTheFuture)?;
        if device.state == DeviceState::Revoked {
            return Err(GrantRefused::NeedsDecision);
        }
        device.grants = held;
        Ok(())
    }
    pub fn renew_at_host(&mut self, id: &DeviceId, profile: Profile, days: u32, now: u64) -> bool {
        let Ok(held) = grants::give_profile(profile, now, days) else {
            return false;
        };
        self.devices
            .get_mut(id)
            .is_some_and(|d| d.renewed_at_host(held, now))
    }
    pub fn review_at_host(&mut self, id: &DeviceId, now: u64) -> bool {
        self.devices
            .get_mut(id)
            .is_some_and(|d| d.reviewed_at_host(now))
    }
    pub fn revoke_at_host(&mut self, id: &DeviceId) -> bool {
        self.revoke(id)
    }
    pub fn revoke_every_device_at_host(&mut self) -> usize {
        let ids: Vec<_> = self.devices.keys().cloned().collect();
        ids.iter().filter(|id| self.revoke(id)).count()
    }
    /// The clock's effect on every device (SM-DEVICE).
    pub fn sweep(&mut self, now: u64) {
        for device in self.devices.values_mut() {
            device.sweep(now);
        }
        self.challenges.retain(|_, c| !c.expired(now));
    }

    fn revoke(&mut self, id: &DeviceId) -> bool {
        let Some(device) = self.devices.get_mut(id) else {
            return false;
        };
        let was = device.fingerprint.clone();
        let changed = device.revoke();
        if changed && !self.revoked_keys.contains(&was) {
            self.revoked_keys.push(was);
        }
        // Its pending challenges go with it: nothing it was asked can still be answered.
        self.challenges.retain(|_, c| &c.device != id);
        changed
    }

    // ---- requests -----------------------------------------------------------------------------

    /// A single-use challenge for a device, valid for sixty seconds.
    pub fn issue_challenge(&mut self, device: &DeviceId, now: u64) -> Result<Challenge, Refusal> {
        let known = self
            .devices
            .get(device)
            .ok_or(Refusal::DeviceUnknown)?
            .state;
        match known {
            DeviceState::Active => {}
            DeviceState::Revoked => return Err(Refusal::DeviceRevoked),
            DeviceState::Suspended => return Err(Refusal::DeviceSuspended),
            DeviceState::Suspect => return Err(Refusal::DeviceSuspect),
        }
        self.challenges.retain(|_, c| !c.expired(now));
        if self.challenges.len() >= MOST_CHALLENGES {
            return Err(Refusal::RateLimited);
        }
        let challenge = Challenge::issue(device.clone(), now, &mut self.random);
        self.challenges
            .insert(challenge.id.clone(), challenge.clone());
        Ok(challenge)
    }

    fn record(&mut self, device: &Device, request: &Request, now: u64, outcome: Outcome) -> bool {
        self.journal
            .append(Record {
                at: now,
                device: device.id.clone(),
                fingerprint: device.fingerprint.clone(),
                connection: request.connection.clone(),
                request_id: request.request_id.clone(),
                grant: request.grant,
                operation: request.operation.clone(),
                target: request.target.clone(),
                bytes: request.bytes,
                outcome,
            })
            .is_ok()
    }

    /// The per-request check (GR5: every request, not once per connection), in a fixed order:
    /// a journal with room, the source's rate, the device, its state, a request id not seen, the
    /// grant and its expiry, then the step-up. Only a request that passes all of it is journalled as
    /// allowed, and only after the journal took the record.
    pub fn handle(
        &mut self,
        request: &Request,
        source: &str,
        channel_binding: &[u8],
        now: u64,
    ) -> Result<Allowed, Denied> {
        if !self.journal.has_room() {
            return Err(Denied::Refused(Refusal::AuditUnavailable));
        }
        if !identifier_ok(&request.request_id, 64)
            || !identifier_ok(&request.operation, 64)
            || !request
                .grant
                .operations()
                .contains(&request.operation.as_str())
            || !identifier_ok(&request.connection, 64)
            || request
                .target
                .as_deref()
                .is_some_and(|t| !identifier_ok(t, 128))
            || request.canonical.len() > MOST_CANONICAL
        {
            return Err(Denied::Malformed);
        }
        self.limiter.check(source, now).map_err(Denied::Refused)?;
        let Some(known) = self.devices.get(&request.device) else {
            // An unknown key is refused at the handshake and counted per hour, not journalled.
            self.limiter.failed(source, now);
            let hour = now / 3600;
            *self.unknown_per_hour.entry(hour).or_insert(0) += 1;
            while self.unknown_per_hour.len() > MOST_HOURS {
                self.unknown_per_hour.pop_first();
            }
            return Err(Denied::Refused(Refusal::DeviceUnknown));
        };
        let mut device = known.clone();
        device.sweep(now);
        let refused = |gateway: &mut Self, device: &Device, word: Refusal| {
            // Saved first: what the check learned (a suspension, a failure) outlives the refusal.
            gateway.devices.insert(device.id.clone(), device.clone());
            let _ = gateway.record(device, request, now, Outcome::Refused(word));
            Err(Denied::Refused(word))
        };
        match device.state {
            DeviceState::Active => {}
            DeviceState::Revoked => return refused(self, &device, Refusal::DeviceRevoked),
            DeviceState::Suspended => return refused(self, &device, Refusal::DeviceSuspended),
            DeviceState::Suspect => return refused(self, &device, Refusal::DeviceSuspect),
        }
        if self.seen.contains_key(&request.request_id) {
            // The whole request again: if it also carries an answered challenge, that is the
            // single-use rule broken, and the device is suspect at once.
            if let Some(step_up) = &request.step_up {
                if self
                    .challenges
                    .get(&step_up.challenge_id)
                    .is_some_and(|c| c.used && c.device == device.id)
                {
                    device.challenge_reused();
                }
            }
            return refused(self, &device, Refusal::StaleRequest);
        }

        let Some(held) = device
            .grants
            .iter()
            .find(|h| h.grant == request.grant)
            .copied()
        else {
            self.limiter.failed(source, now);
            return refused(self, &device, Refusal::GrantMissing);
        };
        if state_of(&held, now) == GrantState::Expired {
            return refused(self, &device, Refusal::GrantExpired);
        }
        if request.grant.needs_step_up() {
            let Some(step_up) = &request.step_up else {
                return refused(self, &device, Refusal::StepUpRequired);
            };
            let Some(challenge) = self.challenges.get_mut(&step_up.challenge_id) else {
                device.step_up_failed(now);
                return refused(self, &device, Refusal::StepUpFailed);
            };
            if challenge.device != device.id || challenge.expired(now) {
                self.challenges.remove(&step_up.challenge_id);
                device.step_up_failed(now);
                return refused(self, &device, Refusal::StepUpFailed);
            }
            if challenge.used {
                // A single-use challenge answered twice: the device is suspect at once.
                device.challenge_reused();
                return refused(self, &device, Refusal::StepUpFailed);
            }
            challenge.used = true;
            let message = step_up_message(channel_binding, &challenge.nonce(), request);
            if !self
                .verifier
                .verify(&device.fingerprint, &message, &step_up.signature)
            {
                device.step_up_failed(now);
                return refused(self, &device, Refusal::StepUpFailed);
            }
        }
        self.seen
            .retain(|_, at| now.saturating_sub(*at) < SEEN_SECONDS);
        if self.seen.len() >= MOST_SEEN {
            return refused(self, &device, Refusal::RateLimited);
        }
        if !self.record(&device, request, now, Outcome::Allowed) {
            return refused(self, &device, Refusal::AuditUnavailable);
        }
        self.seen.insert(request.request_id.clone(), now);
        device.seen(now);
        self.devices.insert(device.id.clone(), device);
        Ok(Allowed {
            device: request.device.clone(),
            grant: request.grant,
            operation: request.operation.clone(),
            target: request.target.clone(),
        })
    }

    /// The one change a request may make to the registry: `devices.revoke` of **another** device,
    /// after it passed `handle`. It ends the target's connections and refuses its later requests at
    /// once. A device holding the grant can revoke; it can never enrol or widen.
    pub fn apply_revoke(&mut self, allowed: &Allowed) -> Result<(), Refusal> {
        if allowed.grant != Grant::DevicesRevoke || allowed.operation != "devices.revoke" {
            return Err(Refusal::GrantMissing);
        }
        let target = allowed
            .target
            .as_deref()
            .and_then(DeviceId::parse)
            .ok_or(Refusal::StaleRequest)?;
        if target == allowed.device {
            return Err(Refusal::StaleRequest);
        }
        if self.revoke(&target) {
            Ok(())
        } else {
            Err(Refusal::StaleRequest)
        }
    }

    /// GR4: a device narrows its own grants. It can never widen them: there is nothing to call.
    pub fn narrow(&mut self, id: &DeviceId, keep: &[Grant]) -> bool {
        self.devices
            .get_mut(id)
            .map(|d| d.narrow_to(keep))
            .is_some()
    }

    pub fn challenge_seconds() -> u64 {
        CHALLENGE_SECONDS
    }
}

/// The hex of a key's digest, for a place that wants one (a fingerprint on a screen).
pub fn fingerprint_of(key: &[u8]) -> String {
    hex(&super::primitives::sha256(key))
}
