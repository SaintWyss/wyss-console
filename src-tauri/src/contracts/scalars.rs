use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    ExpectedObject,
    MissingField,
    UnexpectedField,
    InvalidSchemaVersion,
    InvalidUuid,
    InvalidTimestamp,
    InvalidSequence,
    InvalidEventType,
    InvalidPositiveInteger,
    InvalidComponent,
    InvalidEnum,
    InvalidJsonValue,
    DurableSecret,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExpectedObject => "expected_object",
            Self::MissingField => "missing_field",
            Self::UnexpectedField => "unexpected_field",
            Self::InvalidSchemaVersion => "invalid_schema_version",
            Self::InvalidUuid => "invalid_uuid",
            Self::InvalidTimestamp => "invalid_timestamp",
            Self::InvalidSequence => "invalid_sequence",
            Self::InvalidEventType => "invalid_event_type",
            Self::InvalidPositiveInteger => "invalid_positive_integer",
            Self::InvalidComponent => "invalid_component",
            Self::InvalidEnum => "invalid_enum",
            Self::InvalidJsonValue => "invalid_json_value",
            Self::DurableSecret => "durable_secret",
        }
    }
}

/// Deterministic diagnostics never include rejected content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError {
    pub field: &'static str,
    pub code: ErrorCode,
}

impl ContractError {
    pub(crate) const fn new(field: &'static str, code: ErrorCode) -> Self {
        Self { field, code }
    }
}

impl fmt::Display for ContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.code.as_str())
    }
}

impl std::error::Error for ContractError {}

/// Hyphenated UUID text; original case is retained across serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WireUuid(String);

impl WireUuid {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for WireUuid {
    type Error = ContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let bytes = value.as_bytes();
        if bytes.len() != 36
            || ![8, 13, 18, 23].iter().all(|&index| bytes[index] == b'-')
            || uuid::Uuid::parse_str(&value).is_err()
        {
            return Err(ContractError::new("uuid", ErrorCode::InvalidUuid));
        }
        Ok(Self(value))
    }
}

impl From<WireUuid> for String {
    fn from(value: WireUuid) -> Self {
        value.0
    }
}

/// A positive u64 which is always serialized as a canonical decimal string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sequence(u64);

impl Sequence {
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl FromStr for Sequence {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 20
            || !matches!(bytes[0], b'1'..=b'9')
            || !bytes.iter().all(u8::is_ascii_digit)
        {
            return Err(ContractError::new("sequence", ErrorCode::InvalidSequence));
        }
        value
            .parse::<u64>()
            .map(Self)
            .map_err(|_| ContractError::new("sequence", ErrorCode::InvalidSequence))
    }
}

impl Serialize for Sequence {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Sequence {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Validated observational time. The wire text is retained, not normalized or sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UtcTimestamp(String);

impl TryFrom<String> for UtcTimestamp {
    type Error = ContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let utc = value.ends_with('Z') || value.ends_with('z') || value.ends_with("+00:00");
        let separator = value
            .as_bytes()
            .get(10)
            .is_some_and(|b| matches!(b, b'T' | b't'));
        if !utc || !separator || OffsetDateTime::parse(&value, &Rfc3339).is_err() {
            return Err(ContractError::new("timestamp", ErrorCode::InvalidTimestamp));
        }
        Ok(Self(value))
    }
}

impl From<UtcTimestamp> for String {
    fn from(value: UtcTimestamp) -> Self {
        value.0
    }
}
