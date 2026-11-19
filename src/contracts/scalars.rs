use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidUuid,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidUuid => "invalid_uuid",
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
