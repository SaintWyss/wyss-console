//! The remote gateway's **core** (F10a, local foundations, ADR 0049): grants, device states,
//! pairing, the per-request check, replay protection and the journal's rules, as a library of
//! decisions with no transport.
//!
//! What this is not. It listens on nothing, opens no socket, reads no file and writes none; no
//! window command reaches it and nothing constructs it outside tests. It is not enabled, not
//! reviewed by anyone but its author, and not a security boundary: **DN-05 (how a device reaches the
//! host) is the owner's and is undecided, and the external security review the roadmap requires
//! for F10a has not happened.** It is here so that the remote rules of `docs/REQUIREMENTS.md` §8 exist
//! as tested code before any network code does, and so that a later transport has nothing to decide.
//!
//! Remaining boundaries it cannot be honest about without separate decisions:
//!
//! - **Transport and host identity.** [`crypto::P256StepUpVerifier`] verifies device presence
//!   signatures and pairing rejects invalid P-256 public points. It does not provide an encrypted
//!   channel, host-key persistence/rotation, platform keystore integration or production
//!   authentication.
//! - **Short-code pairing.** It needs a password-authenticated key exchange, which also needs a
//!   crate that is not here. Only the QR path (a 128-bit secret, a MAC over the transcript) exists.
//!
//! It lives in the desktop binary and not in the library the authority links, so the authority
//! binary cannot contain it (C-02, TS-SEC-03).
#![allow(
    dead_code,
    reason = "F10a: decisions with no transport yet; only the tests construct the gateway (ADR 0049)"
)]

pub mod audit;
pub mod crypto;
pub mod device;
pub mod gateway;
pub mod grants;
pub mod pairing;
pub mod primitives;
pub mod protocol;
pub mod request;
#[cfg(test)]
mod tests;

/// The words a device is told when it is refused, closed (IF-REMOTE, 16 §5.2.7). None carries
/// content, a name from a file or a reason that would help a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    PairingExpired,
    PairingFailed,
    PairingRefused,
    HostKeyMismatch,
    DeviceUnknown,
    DeviceRevoked,
    DeviceSuspended,
    DeviceSuspect,
    GrantMissing,
    GrantExpired,
    StepUpRequired,
    StepUpFailed,
    StaleRequest,
    AuditUnavailable,
    RateLimited,
}
