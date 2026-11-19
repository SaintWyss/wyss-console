//! Typed terminal identity in the host wire protocol.
use super::{ContractError, WireUuid};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TerminalId(WireUuid);

impl TerminalId {
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        WireUuid::try_from(value.to_owned()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
