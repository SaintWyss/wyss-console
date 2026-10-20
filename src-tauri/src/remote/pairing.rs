//! Pairing (SM-PAIR, 16 §5.2.2): the host is the root of trust, an offer is single use and short,
//! and nothing enrols without the host's native confirmation (ND-13).
//!
//! The offer carries a 128-bit secret. A device proves it with a MAC over the handshake transcript
//! (which binds its public keys, the host's, and the channel); the comparison value both screens
//! show is derived from the same transcript. The **short-code path is not built**: it needs a PAKE,
//! and no crate for one is in the build. A short code without one is guessable offline, so none is
//! offered.
use super::{
    Refusal,
    device::clean_label,
    primitives::{Random, field, hex, hmac_sha256, same, sha256},
};
use zeroize::Zeroizing;

pub const OFFER_SECONDS: u64 = 5 * 60;
pub const MAX_ATTEMPTS: u32 = 3;
pub const CONFIRM_SECONDS: u64 = 2 * 60;
pub const MOST_KEY_BYTES: usize = 256;
pub const MOST_TRANSCRIPT_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dead {
    Expired,
    Failed,
    Cancelled,
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Offered,
    Proved {
        at: u64,
        key_fingerprint: String,
        name: String,
        platform: String,
        comparison: String,
    },
    Confirmed,
    Dead(Dead),
}

/// What the owner is shown at the host so that the device can find it: the fingerprint of the
/// host key, a QR payload with the secret, and when it dies. The secret is returned once, here.
pub struct Shown {
    pub offer_id: String,
    pub secret_hex: Zeroizing<String>,
    pub host_fingerprint: String,
    pub expires_at: u64,
}

pub struct Offer {
    id: [u8; 16],
    secret: Zeroizing<[u8; 16]>,
    host_fingerprint: String,
    created_at: u64,
    attempts: u32,
    phase: Phase,
}

/// What a device sends to prove the offer: the fingerprint it saw for the host's key, its own
/// connection key (public bytes), the label it proposes, the handshake transcript and the MAC over it.
pub struct Attempt {
    pub host_fingerprint_seen: String,
    pub device_key: Vec<u8>,
    pub name: String,
    pub platform: String,
    pub transcript: Vec<u8>,
    pub mac: [u8; 32],
}

/// What the native dialog (ND-13) shows, as text and fingerprints only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmRequest {
    pub name: String,
    pub platform: String,
    pub device_fingerprint: String,
    pub host_fingerprint: String,
    pub comparison: String,
    pub expires_at: u64,
}

/// What an accepted pairing yields: the device's key and its proposed (untrusted) label. The grants
/// are the default profile's and are decided by the caller, at the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrolment {
    pub key_fingerprint: String,
    pub name: String,
    pub platform: String,
}

impl Offer {
    pub fn new(host_fingerprint: &str, now: u64, random: &mut impl Random) -> (Self, Shown) {
        let (mut id, mut secret) = ([0_u8; 16], [0_u8; 16]);
        random.fill(&mut id);
        random.fill(&mut secret);
        let shown = Shown {
            offer_id: hex(&id),
            secret_hex: Zeroizing::new(hex(&secret)),
            host_fingerprint: host_fingerprint.to_owned(),
            expires_at: now + OFFER_SECONDS,
        };
        (
            Self {
                id,
                secret: Zeroizing::new(secret),
                host_fingerprint: host_fingerprint.to_owned(),
                created_at: now,
                attempts: 0,
                phase: Phase::Offered,
            },
            shown,
        )
    }

    pub fn id_hex(&self) -> String {
        hex(&self.id)
    }

    /// Whether the offer can still be proved.
    pub fn is_open(&self, now: u64) -> bool {
        self.phase == Phase::Offered && now < self.created_at + OFFER_SECONDS
    }
    pub fn is_dead(&self) -> bool {
        matches!(self.phase, Phase::Dead(_))
    }
    pub fn is_confirmed(&self) -> bool {
        self.phase == Phase::Confirmed
    }
    pub fn cancel(&mut self) {
        if matches!(self.phase, Phase::Offered | Phase::Proved { .. }) {
            self.phase = Phase::Dead(Dead::Cancelled);
        }
    }

    /// The MAC a device makes with the secret over the transcript. The device and the host run the
    /// same function, over the same bytes.
    pub fn mac_of(secret: &[u8; 16], transcript: &[u8]) -> [u8; 32] {
        let mut message = b"wyss-pair-proof-v1".to_vec();
        field(&mut message, transcript);
        hmac_sha256(secret, &message)
    }

    /// The six digits both screens show: derived from the same transcript, so that a device that
    /// was reached through someone else's channel shows a different value.
    pub fn comparison_of(secret: &[u8; 16], transcript: &[u8]) -> String {
        let mut message = b"wyss-pair-compare-v1".to_vec();
        field(&mut message, transcript);
        let digest = hmac_sha256(secret, &message);
        let value = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 1_000_000;
        format!("{value:06}")
    }

    /// A device presents its proof. The offer is consumed at the **first attempt that proves the
    /// secret**, whatever happens next; three wrong ones kill it.
    pub fn present(&mut self, now: u64, attempt: &Attempt) -> Result<String, Refusal> {
        if self.phase == Phase::Offered && now >= self.created_at + OFFER_SECONDS {
            self.phase = Phase::Dead(Dead::Expired);
        }
        match &self.phase {
            Phase::Offered => {}
            Phase::Dead(Dead::Expired) => return Err(Refusal::PairingExpired),
            _ => return Err(Refusal::PairingFailed),
        }
        // A device that saw another host's key is told so, and the attempt still counts.
        let host_matches = same(
            attempt.host_fingerprint_seen.as_bytes(),
            self.host_fingerprint.as_bytes(),
        );
        let expected = Self::mac_of(&self.secret, &attempt.transcript);
        let proved = same(&expected, &attempt.mac);
        if !host_matches || !proved {
            self.attempts += 1;
            if self.attempts >= MAX_ATTEMPTS {
                self.phase = Phase::Dead(Dead::Failed);
            }
            return Err(if host_matches {
                Refusal::PairingFailed
            } else {
                Refusal::HostKeyMismatch
            });
        }
        if attempt.device_key.is_empty()
            || attempt.device_key.len() > MOST_KEY_BYTES
            || attempt.transcript.len() > MOST_TRANSCRIPT_BYTES
        {
            self.phase = Phase::Dead(Dead::Failed);
            return Err(Refusal::PairingFailed);
        }
        let (Some(name), Some(platform)) =
            (clean_label(&attempt.name), clean_label(&attempt.platform))
        else {
            // The proof was right and the label is not text: consumed, and dead.
            self.phase = Phase::Dead(Dead::Failed);
            return Err(Refusal::PairingFailed);
        };
        let comparison = Self::comparison_of(&self.secret, &attempt.transcript);
        self.phase = Phase::Proved {
            at: now,
            key_fingerprint: hex(&sha256(&attempt.device_key)),
            name,
            platform,
            comparison: comparison.clone(),
        };
        Ok(comparison)
    }

    /// What ND-13 is asked, once the secret is proved.
    pub fn confirm_request(&self) -> Option<ConfirmRequest> {
        match &self.phase {
            Phase::Proved {
                at,
                key_fingerprint,
                name,
                platform,
                comparison,
            } => Some(ConfirmRequest {
                name: name.clone(),
                platform: platform.clone(),
                device_fingerprint: key_fingerprint.clone(),
                host_fingerprint: self.host_fingerprint.clone(),
                comparison: comparison.clone(),
                expires_at: at + CONFIRM_SECONDS,
            }),
            _ => None,
        }
    }

    /// The host's answer: `Some(true)` accepted, `Some(false)` refused, `None` not answered (a closed
    /// dialog, a timeout). Only an acceptance inside two minutes enrols; anything else enrols nothing.
    pub fn confirm(&mut self, now: u64, answer: Option<bool>) -> Result<Enrolment, Refusal> {
        let Phase::Proved {
            at,
            key_fingerprint,
            name,
            platform,
            ..
        } = self.phase.clone()
        else {
            return Err(Refusal::PairingFailed);
        };
        if now >= at + CONFIRM_SECONDS || answer != Some(true) {
            self.phase = Phase::Dead(Dead::Refused);
            return Err(Refusal::PairingRefused);
        }
        self.phase = Phase::Confirmed;
        Ok(Enrolment {
            key_fingerprint,
            name,
            platform,
        })
    }
}
