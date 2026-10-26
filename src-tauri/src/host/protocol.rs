//! The host's local protocol: one request and one answer per connection, each a 4-byte
//! big-endian length and a JSON document (at most 1 MiB). The socket is private to the user
//! (directory 0700, socket 0600, the peer's uid is checked), so the protocol carries no
//! credentials; a remote client goes through a gateway that does (F10).
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Write};

/// Changed whenever a request or an answer changes shape.
pub const PROTOCOL: u32 = 1;
/// Additions to protocol 1 that a window may need to ask about before it relies on them.
/// A feature is never removed; a host without the name does not have it.
pub const FEATURES: [&str; 4] = [
    "profile_effort",
    "native_profile_plan_token",
    "screen_snapshot",
    "native_extended_catalogue_v1",
];
pub const MAX_FRAME: usize = 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub op: String,
    #[serde(default)]
    pub params: Value,
}

/// What a failed call carries: a closed word, the reasons a profile gave, and for terminal
/// writes how much was written. It is the shape the window already parses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostError {
    pub code: String,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub bytes_written: usize,
}

impl HostError {
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            reasons: Vec::new(),
            bytes_written: 0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Ok(Value),
    Err(HostError),
}

pub fn write_frame(stream: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::other("frame too large"));
    }
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()
}

pub fn read_frame<T: for<'de> Deserialize<'de>>(stream: &mut impl Read) -> io::Result<T> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(io::Error::other("frame too large"));
    }
    let mut bytes = vec![0u8; length];
    stream.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_round_trips_and_an_oversized_one_is_refused_both_ways() {
        let mut wire = Vec::new();
        write_frame(
            &mut wire,
            &Request {
                op: "x".into(),
                params: serde_json::json!({"a": 1}),
            },
        )
        .unwrap();
        let back: Request = read_frame(&mut wire.as_slice()).unwrap();
        assert_eq!(back.op, "x");
        let huge = Request {
            op: "x".into(),
            params: Value::String("a".repeat(MAX_FRAME)),
        };
        assert!(write_frame(&mut Vec::new(), &huge).is_err());
        let mut lying = ((MAX_FRAME + 1) as u32).to_be_bytes().to_vec();
        lying.extend_from_slice(b"{}");
        assert!(read_frame::<Request>(&mut lying.as_slice()).is_err());
    }

    #[test]
    fn a_request_with_an_unknown_field_is_not_one() {
        let bytes = br#"{"op":"x","params":{},"shell":"rm"}"#;
        let mut wire = (bytes.len() as u32).to_be_bytes().to_vec();
        wire.extend_from_slice(bytes);
        assert!(read_frame::<Request>(&mut wire.as_slice()).is_err());
    }
}
