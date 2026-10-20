//! Devices and their states (SM-DEVICE, 16 §5.2.3). A device is a record of what the host has
//! decided about a key: it is not a name, an address or a network position. State changes are
//! functions of the record and the clock, so every transition is a test.
use super::grants::{DAY, GrantState, Held, state_of};
use serde::{Deserialize, Serialize};

pub const SUSPEND_AFTER_UNSEEN: u64 = 30 * DAY;
pub const SUSPECT_FAILURES: usize = 5;
pub const SUSPECT_WINDOW: u64 = 10 * 60;

/// 16 random bytes as 32 lower-case hex digits, made by the gateway, never by a device.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeviceId(String);
impl DeviceId {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }
    pub fn parse(text: &str) -> Option<Self> {
        (text.len() == 32
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| Self(text.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    /// Enrolled; requests are checked against its grants.
    Active,
    /// Unseen for 30 days, or every grant expired: it can only be told to renew at the host.
    Suspended,
    /// Five failed step-ups in ten minutes, or a reused challenge: every grant is held until the
    /// owner reviews it at the host.
    Suspect,
    /// Final. Its keys are refused for ever; the record stays for audit.
    Revoked,
}

/// Text a device proposed about itself: shown with its fingerprint where it matters and **never
/// used to decide anything**. One printable line, with no control or direction-changing character.
pub fn clean_label(text: &str) -> Option<String> {
    let text = text.trim();
    let hostile = |c: char| {
        c.is_control()
            || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
    };
    (!text.is_empty() && text.chars().count() <= 60 && !text.chars().any(hostile))
        .then(|| text.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    /// SHA-256 of the device's connection key, in hex: what the host pinned.
    pub fingerprint: String,
    pub name: String,
    pub platform: String,
    pub state: DeviceState,
    pub grants: Vec<Held>,
    pub enrolled_at: u64,
    pub last_seen: u64,
    /// When each failed step-up of the last ten minutes happened.
    pub step_up_failures: Vec<u64>,
}

impl Device {
    pub fn new(
        id: DeviceId,
        fingerprint: String,
        name: String,
        platform: String,
        grants: Vec<Held>,
        now: u64,
    ) -> Self {
        Self {
            id,
            fingerprint,
            name,
            platform,
            state: DeviceState::Active,
            grants,
            enrolled_at: now,
            last_seen: now,
            step_up_failures: Vec::new(),
        }
    }

    /// The clock's effect (active → suspended): unseen for 30 days, or no grant left alive.
    pub fn sweep(&mut self, now: u64) {
        if self.state != DeviceState::Active {
            return;
        }
        let unseen = now.saturating_sub(self.last_seen) >= SUSPEND_AFTER_UNSEEN;
        let none_alive = self
            .grants
            .iter()
            .all(|g| state_of(g, now) == GrantState::Expired);
        if unseen || none_alive {
            self.state = DeviceState::Suspended;
        }
    }

    /// A request was served: only an active device is ever marked seen.
    pub fn seen(&mut self, now: u64) {
        if self.state == DeviceState::Active {
            self.last_seen = now;
        }
    }

    /// Renewal at the host (suspended → active), with the grants it chose to give.
    pub fn renewed_at_host(&mut self, grants: Vec<Held>, now: u64) -> bool {
        if self.state != DeviceState::Suspended {
            return false;
        }
        self.grants = grants;
        self.last_seen = now;
        self.state = DeviceState::Active;
        true
    }

    /// A step-up signature failed. Five inside ten minutes make the device suspect.
    pub fn step_up_failed(&mut self, now: u64) {
        if self.state != DeviceState::Active {
            return;
        }
        self.step_up_failures
            .retain(|at| now.saturating_sub(*at) < SUSPECT_WINDOW);
        self.step_up_failures.push(now);
        if self.step_up_failures.len() >= SUSPECT_FAILURES {
            self.state = DeviceState::Suspect;
        }
    }

    /// A challenge was used twice: the device is suspect at once.
    pub fn challenge_reused(&mut self) {
        if self.state == DeviceState::Active {
            self.state = DeviceState::Suspect;
        }
    }

    /// The owner reviewed it at the host (suspect → active).
    pub fn reviewed_at_host(&mut self, now: u64) -> bool {
        if self.state != DeviceState::Suspect {
            return false;
        }
        self.step_up_failures.clear();
        self.last_seen = now;
        self.state = DeviceState::Active;
        true
    }

    /// Final, from any state but revoked. What it held ends with it.
    pub fn revoke(&mut self) -> bool {
        if self.state == DeviceState::Revoked {
            return false;
        }
        self.state = DeviceState::Revoked;
        self.grants.clear();
        true
    }

    /// GR4: a device may narrow its own grants and never widen them. Only grants it already holds
    /// can remain; an expiry is never moved later.
    pub fn narrow_to(&mut self, keep: &[super::grants::Grant]) {
        self.grants.retain(|held| keep.contains(&held.grant));
    }
}
