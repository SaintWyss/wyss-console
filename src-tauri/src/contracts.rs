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
