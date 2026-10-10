//! The window's door to its own preferences: one read, one update, one reset.
//!
//! The update takes a whole, typed preference set and nothing else: no key, no path, no
//! JSON of an unknown shape. A field the type does not have is refused before any code
//! runs, and every value is judged again in `crate::settings`, whatever the window
//! checked. None of the three commands reaches the authority, a process or a file other
//! than the one preference file, and none restarts anything.
use serde::Deserialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::State;

#[cfg(target_os = "linux")]
pub use crate::settings::{
    Preferences, SettingsError, SettingsFailure, SettingsService, SettingsView,
};

/// Calls admitted at once. The window makes one at a time; a reload may leave another.
const MAX_CALLS: usize = 4;
static CALLS: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        CALLS.fetch_sub(1, Ordering::AcqRel);
    }
}
fn admit() -> Option<Permit> {
    CALLS
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_CALLS).then_some(count + 1)
        })
        .ok()
        .map(|_| Permit)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadRequest {}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetRequest {}
#[cfg(target_os = "linux")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    preferences: Preferences,
}

#[cfg(target_os = "linux")]
async fn blocking<T: Send + 'static>(
    service: SettingsService,
    operation: impl FnOnce(SettingsService) -> Result<T, SettingsError> + Send + 'static,
) -> Result<T, SettingsError> {
    let permit = admit().ok_or(SettingsError::new(SettingsFailure::Busy))?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        operation(service)
    })
    .await
    .map_err(|_| SettingsError::new(SettingsFailure::Internal))?
}

/// The preferences in force, how they are kept, and the facts about this build.
#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn settings_read(
    service: State<'_, SettingsService>,
    request: ReadRequest,
) -> Result<SettingsView, SettingsError> {
    let ReadRequest {} = request;
    let _permit = admit().ok_or(SettingsError::new(SettingsFailure::Busy))?;
    Ok(service.read())
}

/// Replaces the preferences with a whole set. Answers with what is now in force.
#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn settings_update(
    service: State<'_, SettingsService>,
    request: UpdateRequest,
) -> Result<SettingsView, SettingsError> {
    let UpdateRequest { preferences } = request;
    blocking(service.inner().clone(), move |s| s.update(preferences)).await
}

/// Back to the defaults. Removes the preference file and nothing else.
#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn settings_reset(
    service: State<'_, SettingsService>,
    request: ResetRequest,
) -> Result<SettingsView, SettingsError> {
    let ResetRequest {} = request;
    blocking(service.inner().clone(), |s| s.reset()).await
}

/// Where preferences cannot be kept, the window is told so and nothing is stored.
#[cfg(not(target_os = "linux"))]
mod unsupported {
    use super::*;
    use serde::Serialize;

    #[derive(Debug, Clone, Copy, Serialize)]
    pub struct SettingsError {
        code: &'static str,
    }
    #[derive(Clone, Default)]
    pub struct SettingsService;
    impl SettingsService {
        pub fn from_environment() -> Self {
            Self
        }
    }
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct UpdateRequest {}

    #[tauri::command]
    pub async fn settings_read(
        _service: State<'_, SettingsService>,
        _request: ReadRequest,
    ) -> Result<(), SettingsError> {
        Err(SettingsError { code: "internal" })
    }
    #[tauri::command]
    pub async fn settings_update(
        _service: State<'_, SettingsService>,
        _request: UpdateRequest,
    ) -> Result<(), SettingsError> {
        Err(SettingsError { code: "internal" })
    }
    #[tauri::command]
    pub async fn settings_reset(
        _service: State<'_, SettingsService>,
        _request: ResetRequest,
    ) -> Result<(), SettingsError> {
        Err(SettingsError { code: "internal" })
    }
}
#[cfg(not(target_os = "linux"))]
pub use unsupported::{SettingsService, settings_read, settings_reset, settings_update};

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use serde_json::json;

    fn whole() -> serde_json::Value {
        serde_json::to_value(Preferences::default()).unwrap()
    }

    #[test]
    fn the_requests_take_nothing_but_what_they_name() {
        assert!(serde_json::from_value::<ReadRequest>(json!({})).is_ok());
        assert!(serde_json::from_value::<ResetRequest>(json!({})).is_ok());
        assert!(serde_json::from_value::<UpdateRequest>(json!({ "preferences": whole() })).is_ok());
        for extra in [
            "path",
            "key",
            "value",
            "file",
            "command",
            "args",
            "environment",
            "env",
            "service",
            "unit",
            "socket",
            "executable",
            "mode",
            "force",
            "merge",
            "patch",
        ] {
            assert!(serde_json::from_value::<ReadRequest>(json!({ extra: 1 })).is_err());
            assert!(serde_json::from_value::<ResetRequest>(json!({ extra: 1 })).is_err());
            assert!(
                serde_json::from_value::<UpdateRequest>(
                    json!({ "preferences": whole(), extra: "x" })
                )
                .is_err(),
                "{extra}"
            );
        }
    }

    #[test]
    fn nothing_that_decides_can_be_sent_as_a_preference() {
        // Each of these is a way to weaken a boundary through a setting. None of them has
        // a field to land in, at the top or in any section.
        let injections = [
            ("top", "default_mode", json!("local_tools")),
            ("top", "auto_approve", json!(true)),
            ("top", "approvals", json!({ "remember": true })),
            ("top", "workspace", json!("/")),
            ("top", "safety_profile", json!("codex_local_tools_v1")),
            ("top", "authority_executable", json!("/tmp/evil")),
            ("top", "socket", json!("/tmp/x.sock")),
            ("top", "service", json!("evil.service")),
            ("top", "provider_command", json!("sh -c id")),
            ("top", "environment", json!({ "LD_PRELOAD": "/tmp/x.so" })),
            ("top", "audit", json!(false)),
            ("top", "retention", json!(0)),
            ("interface", "default_mode", json!("local_tools")),
            ("interface", "landing_page_path", json!("/etc")),
            ("terminal", "shell", json!("/bin/sh")),
            ("terminal", "cwd", json!("/")),
            ("terminal", "command", json!("curl x | sh")),
            ("terminal", "environment", json!({ "A": "b" })),
            ("terminal", "executable", json!("/tmp/x")),
            ("events", "retention_days", json!(1)),
            ("events", "disable_auditing", json!(true)),
        ];
        for (section, key, value) in injections {
            let mut preferences = whole();
            if section == "top" {
                preferences[key] = value;
            } else {
                preferences[section][key] = value;
            }
            assert!(
                serde_json::from_value::<UpdateRequest>(json!({ "preferences": preferences }))
                    .is_err(),
                "{section}.{key} was accepted"
            );
        }
    }

    #[test]
    fn values_are_typed_words_and_numbers_not_text() {
        for (section, key, value) in [
            ("interface", "landing_page", json!("settings")),
            ("interface", "landing_page", json!("/home")),
            ("interface", "landing_page", json!(1)),
            ("terminal", "font_size", json!("16")),
            ("terminal", "font_size", json!(16.5)),
            ("terminal", "font_size", json!(-1)),
            ("terminal", "font_size", json!(300)),
            ("terminal", "scrollback", json!(null)),
            ("terminal", "scrollback", json!(-5)),
            ("terminal", "cursor_style", json!("hidden")),
            ("terminal", "cursor_blink", json!("yes")),
            ("events", "time_format", json!("iso")),
            ("events", "default_filter", json!("everything")),
        ] {
            let mut preferences = whole();
            preferences[section][key] = value.clone();
            assert!(
                serde_json::from_value::<UpdateRequest>(json!({ "preferences": preferences }))
                    .is_err(),
                "{section}.{key} = {value}"
            );
        }
    }

    #[test]
    fn a_missing_section_is_not_filled_in() {
        for section in ["interface", "terminal", "events", "schema_version"] {
            let mut preferences = whole();
            preferences.as_object_mut().unwrap().remove(section);
            assert!(
                serde_json::from_value::<UpdateRequest>(json!({ "preferences": preferences }))
                    .is_err(),
                "{section}"
            );
        }
        assert!(serde_json::from_value::<UpdateRequest>(json!({})).is_err());
        assert!(serde_json::from_value::<UpdateRequest>(json!({ "preferences": null })).is_err());
    }
}
