//! A request, the step-up that some of them need, and the rules that make a signature mean what it
//! says (16 §5.2.5). The signature binds the channel, a single-use challenge, the request's identity
//! and canonical bytes, and the object it acts on with its revision, so that none of them can be
//! replaced after the user's key said yes.
use super::{
    device::DeviceId,
    grants::Grant,
    primitives::{Random, field, hex, sha256},
};

pub const CHALLENGE_SECONDS: u64 = 60;
pub const MOST_CANONICAL: usize = 64 * 1024;
pub const MOST_CHALLENGES: usize = 64;

#[derive(Debug, Clone)]
pub struct Request {
    /// A UUID the device made. Duplicates are refused within one gateway instance only.
    pub request_id: String,
    pub device: DeviceId,
    /// The connection it came on, for the journal.
    pub connection: String,
    pub grant: Grant,
    /// A closed operation word of the grant (`approvals.respond`, `sessions.list`…).
    pub operation: String,
    /// The identifier of what it acts on, when it acts on one.
    pub target: Option<String>,
    /// That object's revision as the device saw it: part of what a step-up signs.
    pub object_revision: Option<u64>,
    /// The request's canonical bytes, exactly as they will be acted on.
    pub canonical: Vec<u8>,
    /// For input, how many bytes (never which).
    pub bytes: Option<u64>,
    pub step_up: Option<StepUp>,
}

#[derive(Debug, Clone)]
pub struct StepUp {
    pub challenge_id: String,
    pub signature: Vec<u8>,
}

/// Whether a signature is the one the enrolled device presence key makes over `message`.
pub trait StepUpVerifier {
    fn verify(&self, presence_key: &[u8], message: &[u8], signature: &[u8]) -> bool;
}

#[derive(Debug, Clone)]
pub struct Challenge {
    pub id: String,
    pub(super) nonce: [u8; 16],
    pub(super) issued_at: u64,
    pub(super) device: DeviceId,
    pub(super) used: bool,
}

impl Challenge {
    pub fn issue(device: DeviceId, now: u64, random: &mut impl Random) -> Self {
        let (mut id, mut nonce) = ([0_u8; 16], [0_u8; 16]);
        random.fill(&mut id);
        random.fill(&mut nonce);
        Self {
            id: hex(&id),
            nonce,
            issued_at: now,
            device,
            used: false,
        }
    }
    pub fn nonce(&self) -> [u8; 16] {
        self.nonce
    }
    pub fn expired(&self, now: u64) -> bool {
        now >= self.issued_at.saturating_add(CHALLENGE_SECONDS)
    }
}

/// The exact bytes a device signs for a step-up. Length-prefixed fields, so no two inputs can run
/// together into one message, and a domain word, so that a signature made for something else is
/// never this one.
pub fn step_up_message(channel_binding: &[u8], nonce: &[u8; 16], request: &Request) -> Vec<u8> {
    let mut message = b"wyss-remote-stepup-v1".to_vec();
    field(&mut message, channel_binding);
    field(&mut message, nonce);
    field(&mut message, request.request_id.as_bytes());
    field(&mut message, request.device.as_str().as_bytes());
    field(&mut message, &sha256(&request.canonical));
    field(&mut message, request.operation.as_bytes());
    field(
        &mut message,
        request.target.as_deref().unwrap_or_default().as_bytes(),
    );
    field(
        &mut message,
        &request
            .object_revision
            .map_or(Vec::new(), |r| r.to_be_bytes().to_vec()),
    );
    message
}
