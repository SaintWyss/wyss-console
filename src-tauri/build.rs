/// What `git` prints for `args`, trimmed, when it succeeds.
fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}

/// Asks Cargo to run this again when the commit `HEAD` names changes. `HEAD` itself only
/// changes when another branch is checked out (or a commit is made detached): a commit on
/// the branch moves the branch's ref, loose or packed, so those are watched too. Paths come
/// from Git (`--git-path`), which knows where a worktree keeps them.
fn watch_head() {
    let mut watched = vec![git(&["rev-parse", "--git-path", "HEAD"])];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        // Watched even before it exists as a loose file: its creation is a new commit.
        watched.push(git(&["rev-parse", "--git-path", &branch]));
        watched.push(
            git(&["rev-parse", "--git-path", "packed-refs"])
                .filter(|path| std::path::Path::new(path).exists()),
        );
    }
    for path in watched.into_iter().flatten() {
        println!("cargo:rerun-if-changed={path}");
    }
}

/// A short source revision for the About section: the packaging command names it, and
/// a checkout is asked. A tree without one has none, and says so.
fn revision() -> Option<String> {
    println!("cargo:rerun-if-env-changed=WYSS_BUILD_REVISION");
    let valid = |text: &str| {
        (7..=40).contains(&text.len())
            && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    };
    if let Ok(named) = std::env::var("WYSS_BUILD_REVISION") {
        return valid(&named).then_some(named);
    }
    watch_head();
    let text = git(&["rev-parse", "--short=12", "HEAD"])?;
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
