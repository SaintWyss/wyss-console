//! Exact-byte relative paths and symlink targets for B1 admission, export and quarantine.
//!
//! Authorization operates on these bytes, never on display text. Every rule fails closed, and
//! every error names only a field and a stable code, never the rejected bytes.
use super::{
    B1Error, B1ErrorCode, B1Result, MAX_COMPONENT_BYTES, MAX_LINK_TARGET_BYTES, MAX_PATH_BYTES,
    MAX_PATH_DEPTH,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Canonical unpadded URL-safe base64, the wire form of raw Git pathname bytes (doc 52 §3).
/// Decoding accepts only the single canonical spelling of each byte string.
pub(super) mod base64url {
    use super::{B1Error, B1ErrorCode, B1Result, MAX_PATH_BYTES};

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    /// Longest canonical encoding of the largest accepted byte string.
    const MAX_ENCODED_BYTES: usize = MAX_PATH_BYTES.div_ceil(3) * 4;

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let mut block = 0u32;
            for (index, &byte) in chunk.iter().enumerate() {
                block |= u32::from(byte) << (16 - 8 * index);
            }
            for symbol in 0..=chunk.len() {
                let index = ((block >> (18 - 6 * symbol)) & 0x3f) as usize;
                out.push(char::from(ALPHABET[index]));
            }
        }
        out
    }

    pub fn decode(text: &str, field: &'static str) -> B1Result<Vec<u8>> {
        let noncanonical = || B1Error::new(field, B1ErrorCode::PathEncodingNoncanonical);
        let input = text.as_bytes();
        if input.len() > MAX_ENCODED_BYTES {
            return Err(B1Error::new(field, B1ErrorCode::LimitExceeded));
        }
        if input.len() % 4 == 1 {
            return Err(noncanonical());
        }
        let mut out = Vec::with_capacity(input.len() / 4 * 3 + 2);
        let (mut accumulator, mut bits) = (0u32, 0u32);
        for &symbol in input {
            let value = ALPHABET
                .iter()
                .position(|&candidate| candidate == symbol)
                .ok_or_else(noncanonical)?;
            accumulator = (accumulator << 6) | value as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((accumulator >> bits) as u8);
                accumulator &= (1u32 << bits) - 1;
            }
        }
        // Re-encoding catches non-zero trailing bits and any other non-canonical spelling.
        if encode(&out) != text {
            return Err(noncanonical());
        }
        Ok(out)
    }
}

/// `.git` and its Windows-style aliases (trailing dots or spaces, `GIT~1`), ASCII case-insensitive.
fn is_git_control(component: &[u8]) -> bool {
    let trimmed_length = component
        .iter()
        .rposition(|&byte| byte != b' ' && byte != b'.')
        .map_or(0, |index| index + 1);
    let name = &component[..trimmed_length];
    name.eq_ignore_ascii_case(b".git") || name.eq_ignore_ascii_case(b"git~1")
}

/// Backslash is refused because it is a path separator on other platforms, so a name that
/// means different paths on different hosts can never be admitted as one.
fn validate_relative(bytes: &[u8]) -> B1Result {
    let unsafe_path = || B1Error::new("path", B1ErrorCode::PathUnsafe);
    if bytes.len() > MAX_PATH_BYTES {
        return Err(B1Error::new("path", B1ErrorCode::LimitExceeded));
    }
    if bytes.is_empty() || bytes.contains(&0) || bytes.contains(&b'\\') || bytes[0] == b'/' {
        return Err(unsafe_path());
    }
    let components: Vec<&[u8]> = bytes.split(|&byte| byte == b'/').collect();
    if components.len() > MAX_PATH_DEPTH {
        return Err(B1Error::new("path", B1ErrorCode::LimitExceeded));
    }
    for (index, component) in components.iter().enumerate() {
        if component.is_empty() || matches!(*component, b"." | b"..") {
            return Err(unsafe_path());
        }
        if component.len() > MAX_COMPONENT_BYTES {
            return Err(B1Error::new("path", B1ErrorCode::LimitExceeded));
        }
        // A drive-style first component (`C:`) is an alternate-root spelling; reject it.
        if (index == 0 && component.contains(&b':')) || is_git_control(component) {
            return Err(unsafe_path());
        }
    }
    Ok(())
}

/// Repository-relative path bytes. Non-UTF-8 names are preserved exactly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct B1RelPath(Vec<u8>);

impl B1RelPath {
    pub fn from_bytes(bytes: &[u8]) -> B1Result<Self> {
        validate_relative(bytes)?;
        Ok(Self(bytes.to_vec()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &[u8]> + '_ {
        self.0.split(|&byte| byte == b'/')
    }

    /// ASCII case-folded spelling used for destination collision checks.
    fn folded(&self) -> Vec<u8> {
        self.0.to_ascii_lowercase()
    }
}

impl TryFrom<String> for B1RelPath {
    type Error = B1Error;

    fn try_from(text: String) -> B1Result<Self> {
        Self::from_bytes(&base64url::decode(&text, "path")?)
    }
}

impl From<B1RelPath> for String {
    fn from(path: B1RelPath) -> Self {
        base64url::encode(&path.0)
    }
}

/// Symlink target bytes. Stored as inert data: it is never resolved or followed here.
/// Containment is checked against the link's location by [`B1LinkTarget::validate_for`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct B1LinkTarget(Vec<u8>);

impl B1LinkTarget {
    pub fn from_bytes(bytes: &[u8]) -> B1Result<Self> {
        if bytes.len() > MAX_LINK_TARGET_BYTES {
            return Err(B1Error::new("link_target", B1ErrorCode::LimitExceeded));
        }
        Ok(Self(bytes.to_vec()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Relative targets only, lexically contained within the repository root from the link's
    /// directory. `..` may never climb above the root, and `.git` may never be named.
    pub fn validate_for(&self, link: &B1RelPath) -> B1Result {
        let unsafe_target = || B1Error::new("link_target", B1ErrorCode::LinkTargetUnsafe);
        if self.0.is_empty() || self.0.contains(&0) || self.0.contains(&b'\\') || self.0[0] == b'/'
        {
            return Err(unsafe_target());
        }
        // Components always yield at least one item, so the link's own name is never missing.
        let mut depth = link.components().count() - 1;
        for (index, component) in self.0.split(|&byte| byte == b'/').enumerate() {
            match component {
                b"" | b"." => return Err(unsafe_target()),
                b".." => {
                    if depth == 0 {
                        return Err(unsafe_target());
                    }
                    depth -= 1;
                }
                _ => {
                    if component.len() > MAX_COMPONENT_BYTES {
                        return Err(B1Error::new("link_target", B1ErrorCode::LimitExceeded));
                    }
                    // A drive-style first component (`C:foo`) is drive-relative on Windows, not
                    // relative to the link's directory. Refuse it, as repository paths are refused.
                    if (index == 0 && component.contains(&b':')) || is_git_control(component) {
                        return Err(unsafe_target());
                    }
                    depth += 1;
                }
            }
        }
        Ok(())
    }
}

impl TryFrom<String> for B1LinkTarget {
    type Error = B1Error;

    fn try_from(text: String) -> B1Result<Self> {
        Self::from_bytes(&base64url::decode(&text, "link_target")?)
    }
}

impl From<B1LinkTarget> for String {
    fn from(target: B1LinkTarget) -> Self {
        base64url::encode(&target.0)
    }
}

/// Rejects destination collisions and file/directory conflicts across one set of paths.
///
/// Comparison is ASCII case-insensitive. Unicode normalization (NFC/NFD) and non-ASCII case
/// folding are not modeled, so those destination collisions remain a documented limitation.
pub fn validate_destination_set<'a>(paths: impl IntoIterator<Item = &'a B1RelPath>) -> B1Result {
    let folded: Vec<Vec<u8>> = paths.into_iter().map(B1RelPath::folded).collect();
    let mut seen: HashSet<&[u8]> = HashSet::with_capacity(folded.len());
    for path in &folded {
        if !seen.insert(path.as_slice()) {
            return Err(B1Error::new("path", B1ErrorCode::PathCollision));
        }
    }
    for path in &folded {
        let conflicts_with_ancestor = path
            .iter()
            .enumerate()
            .any(|(index, &byte)| byte == b'/' && seen.contains(&path[..index]));
        if conflicts_with_ancestor {
            return Err(B1Error::new("path", B1ErrorCode::PathCollision));
        }
    }
    Ok(())
}
