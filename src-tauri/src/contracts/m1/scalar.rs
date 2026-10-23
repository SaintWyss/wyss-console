//! Scalars and identities of the M1 store. Every constructor validates, and every wire form
//! is canonical: a value that does not already have its canonical text is refused, never
//! coerced, so a decoded store re-encodes to the same bytes.
use super::{
    M1Error, M1Result, MAX_EPOCH_BYTES, MAX_PATH_BYTES, MAX_TEXT_SCALARS, ensure, invalid,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

fn lower_hex(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b'a'..=b'f')
}

/// Canonical lowercase hyphenated UUID text. Unlike the older identity types, which keep the
/// case they were given, the case here is part of the identity.
fn canonical_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, &byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                lower_hex(byte)
            }
        })
}

macro_rules! uuid_identity {
    ($($(#[$doc:meta])* $name:ident),+ $(,)?) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> M1Result<Self> {
                Self::try_from(value.to_owned())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = M1Error;

            fn try_from(value: String) -> M1Result<Self> {
                ensure(canonical_uuid(&value), "uuid")?;
                Ok(Self(value))
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    )+};
}

uuid_identity!(
    /// A random installation identity, never a machine name, boot ID, path or PID.
    HostId,
    StoreId,
    ProjectId,
    BindingId,
    WorkspaceId,
    LeafId,
    AssociationId,
    OperationId,
    MigrationId,
    /// The new target of a migration mapping row: whichever record the source became.
    MappedId,
    /// A terminal of the host, named as its owner names it.
    TerminalId,
    /// A Managed session, named as its authority names it.
    ManagedSessionId,
    /// The authority's store stream, which keeps Protocol 1 and 2 IDs apart.
    StoreStreamId,
);

/// A Native session or profile ID as the Native registry issues it: exactly 32 lowercase hex
/// characters. It is never a UUID and is preserved byte for byte.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NativeId(String);

impl NativeId {
    pub fn parse(value: &str) -> M1Result<Self> {
        Self::try_from(value.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NativeId {
    type Error = M1Error;

    fn try_from(value: String) -> M1Result<Self> {
        ensure(
            value.len() == 32 && value.bytes().all(lower_hex),
            "native_id",
        )?;
        Ok(Self(value))
    }
}

impl From<NativeId> for String {
    fn from(value: NativeId) -> Self {
        value.0
    }
}

/// A positive u64, always a canonical decimal string on the wire so that JavaScript never
/// rounds it. Zero, leading zeros, signs, spaces and numbers that are not strings are refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Counter(u64);

impl Counter {
    pub fn new(value: u64) -> M1Result<Self> {
        ensure(value >= 1, "counter")?;
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    /// The following value, or none at the end of the range: a revision there cannot advance.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

impl TryFrom<String> for Counter {
    type Error = M1Error;

    fn try_from(value: String) -> M1Result<Self> {
        let bytes = value.as_bytes();
        ensure(
            !bytes.is_empty()
                && bytes.len() <= 20
                && matches!(bytes[0], b'1'..=b'9')
                && bytes.iter().all(u8::is_ascii_digit),
            "counter",
        )?;
        let parsed = value.parse::<u64>().map_err(|_| invalid("counter"))?;
        Self::new(parsed)
    }
}

impl Serialize for Counter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Counter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A Native profile version: a `Counter` that also fits the owner's `u32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProfileVersion(u32);

impl ProfileVersion {
    pub fn new(value: u32) -> M1Result<Self> {
        ensure(value >= 1, "profile_version")?;
        Ok(Self(value))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Serialize for ProfileVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for ProfileVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let counter = Counter::try_from(String::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)?;
        u32::try_from(counter.get())
            .map_err(|_| serde::de::Error::custom("profile_version"))
            .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
    }
}

fn plain(value: &str) -> bool {
    !value.chars().any(char::is_control)
}

macro_rules! text_scalar {
    ($(#[$doc:meta])* $name:ident, $field:literal, $valid:expr) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> M1Result<Self> {
                Self::try_from(value.to_owned())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = M1Error;

            fn try_from(value: String) -> M1Result<Self> {
                let valid: fn(&str) -> bool = $valid;
                ensure(valid(&value), $field)?;
                Ok(Self(value))
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

text_scalar!(
    /// A label or title: 1..=64 Unicode scalars and no control characters.
    Text,
    "text",
    |v| (1..=MAX_TEXT_SCALARS).contains(&v.chars().count()) && plain(v)
);

text_scalar!(
    /// An opaque compatibility observation (host epoch, operation claim). Bounded, never a
    /// PID taken as authority.
    EpochText,
    "epoch",
    |v| (1..=MAX_EPOCH_BYTES).contains(&v.len()) && plain(v)
);

text_scalar!(
    /// A private local root: absolute, at most 4096 bytes, no control characters and no empty,
    /// `.` or `..` segment. Whether it exists or is eligible is for the owner to decide later.
    AbsolutePath,
    "path",
    |v| {
        v.len() <= MAX_PATH_BYTES
            && plain(v)
            && v.starts_with('/')
            && (v == "/" || v[1..].split('/').all(|s| !matches!(s, "" | "." | "..")))
    }
);

impl fmt::Debug for Text {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("Text").field(&self.0).finish()
    }
}

impl fmt::Debug for EpochText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("EpochText").field(&self.0).finish()
    }
}

/// A path is private to this machine, so `{:?}` never prints it.
impl fmt::Debug for AbsolutePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AbsolutePath(<private>)")
    }
}
