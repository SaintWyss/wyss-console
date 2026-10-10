/// A short source revision for the About section: the packaging command names it, and
/// a checkout is asked. A tree without one has none, and says so.
fn revision() -> Option<String> {
    println!("cargo:rerun-if-env-changed=WYSS_BUILD_REVISION");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    let valid = |text: &str| {
        (7..=40).contains(&text.len())
            && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    };
    if let Ok(named) = std::env::var("WYSS_BUILD_REVISION") {
        return valid(&named).then_some(named);
    }
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    valid(&text).then_some(text)
}

fn main() {
    if let Some(revision) = revision() {
        println!("cargo:rustc-env=WYSS_BUILD_REVISION={revision}");
    }
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_app_info",
            "authority_state",
            "authority_lifecycle",
            "authority_retry",
            "managed_sessions_page",
            "managed_session_prepare",
            "managed_session_resume",
            "managed_session_close",
            "managed_session_activate",
            "managed_session_abandon",
            "managed_session_mode",
            "choose_workspace",
            "managed_turns",
            "managed_turn_read",
            "managed_turn_start",
            "managed_turn_cancel",
            "managed_approvals",
            "managed_approval_read",
            "managed_approval_approve",
            "managed_approval_deny",
            "managed_events",
            "managed_events_older",
            "settings_read",
            "settings_update",
            "settings_reset",
            "create_terminal",
            "terminal_write",
            "terminal_read_output",
            "terminal_resize",
            "close_terminal",
            "terminal_status",
        ]),
    ))
    .expect("failed to build Wyss Console metadata");
}
