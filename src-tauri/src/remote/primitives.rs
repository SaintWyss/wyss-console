//! The few primitives the core needs, from what the build already has (SHA-256) and nothing else.
//! HMAC is written out (RFC 2104) and tested against RFC 4231's vectors, rather than adding a
//! dependency to the manifest for twenty lines.
use sha2::{Digest, Sha256};

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0_u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&sha256(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let (mut inner, mut outer) = ([0x36_u8; BLOCK], [0x5c_u8; BLOCK]);
    for i in 0..BLOCK {
        inner[i] ^= block[i];
        outer[i] ^= block[i];
    }
    let mut first = Sha256::new();
    first.update(inner);
    first.update(message);
    let mut second = Sha256::new();
    second.update(outer);
    second.update(first.finalize());
    second.finalize().into()
}

/// Equality that does not stop at the first difference.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A field with its length in front, so that two fields can never run together into one.
pub fn field(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Where randomness comes from: the operating system in production, a fixed source in a test.
pub trait Random {
    fn fill(&mut self, bytes: &mut [u8]);
}
pub struct OsRandom;
impl Random for OsRandom {
    fn fill(&mut self, bytes: &mut [u8]) {
        // The OS source does not fail on this platform; if it ever did, nothing may proceed on
        // predictable bytes.
        getrandom::fill(bytes).expect("the operating system's random source");
    }
}
