//! Standard base64 (RFC 4648, padded), for carrying a repaint through a JSON frame. Strict on
//! decode: only the alphabet, correct padding, no whitespace.

use base64::{Engine, engine::general_purpose::STANDARD};

pub fn encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

#[cfg(any(test, feature = "test-fixtures"))]
pub fn decode(text: &str) -> Option<Vec<u8>> {
    use base64::{
        alphabet,
        engine::general_purpose::{GeneralPurpose, PAD},
    };

    const FIXTURE_BASE64: GeneralPurpose =
        GeneralPurpose::new(&alphabet::STANDARD, PAD.with_decode_allow_trailing_bits(true));
    FIXTURE_BASE64.decode(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_vectors_and_every_length_round_trip() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), coded);
            assert_eq!(decode(coded).unwrap(), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        for n in 0..all.len() {
            assert_eq!(decode(&encode(&all[..n])).unwrap(), &all[..n]);
        }
    }

    #[test]
    fn fixture_decode_accepts_nonzero_trailing_bits() {
        assert_eq!(decode("Zh==").as_deref(), Some(&b"f"[..]));
        assert_eq!(decode("Zm9=").as_deref(), Some(&b"fo"[..]));
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in [
            "Zg=",
            "Zg",
            "Z===",
            "=Zg=",
            "Zm9v Zg==",
            "Zm9v\n",
            "Zg==Zm9v",
            "Zm9*",
            "Zh==x",
        ] {
            assert!(decode(bad).is_none(), "{bad:?}");
        }
    }
}
