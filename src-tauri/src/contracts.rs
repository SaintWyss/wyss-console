use crate::application::ApplicationInfo;
use serde::Serialize;

/// AppInfo v1 is the only serialized application DTO in Sprint 0A.
#[derive(Serialize)]
pub struct AppInfo {
    schema_version: u8,
    name: &'static str,
    version: &'static str,
}

impl From<ApplicationInfo> for AppInfo {
    fn from(info: ApplicationInfo) -> Self {
        Self {
            schema_version: 1,
            name: info.name,
            version: info.version,
        }
    }
}
pub mod agent_adapter;
pub mod event_envelope;
pub mod identities;
mod scalars;

pub use scalars::{ContractError, ErrorCode, Sequence, UtcTimestamp, WireUuid};

#[cfg(test)]
mod tests;

pub mod control;

pub mod execution;

pub mod binding;

pub mod turn;

pub mod approval;

/// Pure B1 contract foundation. Not reachable from control protocol v1 or `METHODS`.
pub mod b1;
