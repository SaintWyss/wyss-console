//! Local session/terminal IDs cannot be interchanged with each other or managed IDs.
//!
//! ```compile_fail
//! use wyss_console::contracts::identities::{ConsoleSessionId, TerminalId};
//! let terminal = TerminalId::parse("11111111-1111-4111-8111-111111111111").unwrap();
//! let session: ConsoleSessionId = terminal;
//! ```
//!
//! ```compile_fail
//! use wyss_console::contracts::identities::{ConsoleSessionId, ManagedSessionId};
//! let local = ConsoleSessionId::parse("11111111-1111-4111-8111-111111111111").unwrap();
//! let managed: ManagedSessionId = local;
//! ```
use super::WireUuid;
use serde::{Deserialize, Serialize};

macro_rules! identity {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(WireUuid);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, super::ContractError> {
                WireUuid::try_from(value.to_owned()).map(Self)
            }

            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }
    };
}

identity!(ManagedSessionId);
identity!(ConsoleSessionId);
identity!(TerminalId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_ids_validate_uuid_text_and_round_trip() {
        let text = "ABCDEFAB-1234-4567-89AB-ABCDEFABCDEF";
        let session = ConsoleSessionId::parse(text).unwrap();
        let terminal = TerminalId::parse(text).unwrap();
        assert_eq!(session.as_str(), text);
        assert_eq!(terminal.as_str(), text);
        for json in [
            serde_json::to_string(&session).unwrap(),
            serde_json::to_string(&terminal).unwrap(),
        ] {
            assert_eq!(
                serde_json::from_str::<ConsoleSessionId>(&json)
                    .unwrap()
                    .as_str(),
                text
            );
            assert_eq!(
                serde_json::from_str::<TerminalId>(&json).unwrap().as_str(),
                text
            );
        }
        assert!(ConsoleSessionId::parse("not-a-uuid").is_err());
        assert!(TerminalId::parse("not-a-uuid").is_err());
    }
}
