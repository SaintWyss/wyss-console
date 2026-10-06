/// Local application facts. This module does not depend on Tauri or own agent authority.
pub struct ApplicationInfo {
    pub name: &'static str,
    pub version: &'static str,
}

pub fn app_info() -> ApplicationInfo {
    ApplicationInfo {
        name: "Wyss Console",
        version: env!("CARGO_PKG_VERSION"),
    }
}
