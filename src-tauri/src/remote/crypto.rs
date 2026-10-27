//! Standard-library-backed P-256 identity primitives for the transport-neutral remote core.
//!
//! This module verifies device presence signatures and validates public points. It is not a
//! channel protocol, key store, pairing authenticator or production remote boundary. Transport
//! selection, host-key persistence and independent review remain separate gates.
use ring::{
    agreement::{self, EphemeralPrivateKey, UnparsedPublicKey, agree_ephemeral},
    rand::SystemRandom,
    signature::{self, UnparsedPublicKey as UnparsedSignatureKey},
};

use super::request::StepUpVerifier;

pub const P256_PUBLIC_KEY_BYTES: usize = 65;

/// Reject malformed and off-curve SEC1 public points before pairing can be confirmed.
pub fn valid_p256_public_key(public_key: &[u8]) -> bool {
    if public_key.len() != P256_PUBLIC_KEY_BYTES || public_key.first() != Some(&0x04) {
        return false;
    }
    let rng = SystemRandom::new();
    let Ok(private_key) = EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng) else {
        return false;
    };
    let peer = UnparsedPublicKey::new(&agreement::ECDH_P256, public_key);
    agree_ephemeral(private_key, &peer, |_| Ok::<(), ()>(())).is_ok()
}

/// Verifies P-256 ECDSA/SHA-256 signatures in ASN.1 DER form against an enrolled SEC1 key.
/// Android's `SHA256withECDSA` produces this signature encoding. No private key enters Rust.
#[derive(Debug, Default, Clone, Copy)]
pub struct P256StepUpVerifier;

impl StepUpVerifier for P256StepUpVerifier {
    fn verify(&self, presence_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        if !valid_p256_public_key(presence_key) {
            return false;
        }
        UnparsedSignatureKey::new(&signature::ECDSA_P256_SHA256_ASN1, presence_key)
            .verify(message, signature)
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use ring::{
        rand::SystemRandom,
        signature::{EcdsaKeyPair, KeyPair},
    };

    use super::*;

    #[test]
    fn ring_verifies_real_p256_presence_signatures_and_rejects_mutations() {
        let rng = SystemRandom::new();
        let pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            pkcs8.as_ref(),
            &rng,
        )
        .unwrap();
        let message = b"wyss remote presence test";
        let signed = pair.sign(&rng, message).unwrap();
        let verifier = P256StepUpVerifier;
        let public_key = pair.public_key().as_ref();

        assert!(valid_p256_public_key(public_key));
        assert!(verifier.verify(public_key, message, signed.as_ref()));
        assert!(!verifier.verify(public_key, b"different request", signed.as_ref()));
        assert!(!verifier.verify(public_key, message, b"bad DER"));
        assert!(!verifier.verify(&[0x04; P256_PUBLIC_KEY_BYTES], message, signed.as_ref()));
    }

    #[test]
    fn public_key_validation_rejects_non_sec1_points_and_wrong_lengths() {
        assert!(!valid_p256_public_key(&[]));
        assert!(!valid_p256_public_key(&[0x04; P256_PUBLIC_KEY_BYTES]));
        assert!(!valid_p256_public_key(&[0x02; P256_PUBLIC_KEY_BYTES]));
    }
}
