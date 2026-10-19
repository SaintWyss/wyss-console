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
/// the branch moves the branch's ref, loose or packed, so those are watched too, each only
/// where it exists. Paths come from Git (`--git-path`), which knows where a worktree keeps
/// them.
fn watch_head() {
    let mut watched = vec![git(&["rev-parse", "--git-path", "HEAD"])];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watched.push(
            git(&["rev-parse", "--git-path", &branch]).map(|loose| loose_ref_watch(&branch, loose)),
        );
        watched.push(
            git(&["rev-parse", "--git-path", "packed-refs"])
                .filter(|path| std::path::Path::new(path).exists()),
        );
    }
    for path in watched.into_iter().flatten() {
        println!("cargo:rerun-if-changed={path}");
    }
}

/// What to watch for the loose file of `branch` (`refs/heads/...`), found at `loose`. A
/// branch that is only in `packed-refs` has no such file until its next commit creates one,
/// and Cargo takes a watched path that does not exist as changed on every build. So, while
/// it is missing, the closest directory above it within `refs/` is watched instead (Cargo
/// watches a directory's whole contents), and the file itself once it exists. Where that
/// directory is not one (a repository that keeps its refs in another format), the missing
/// file is watched as before: a build that always runs this again is never a stale one.
fn loose_ref_watch(branch: &str, loose: String) -> String {
    let path = std::path::Path::new(&loose);
    if path.exists() {
        return loose;
    }
    let within_refs = std::path::Path::new(branch)
        .components()
        .count()
        .saturating_sub(1);
    match path
        .ancestors()
        .skip(1)
        .take(within_refs)
        .find(|a| a.exists())
    {
        Some(directory) if directory.is_dir() => directory.display().to_string(),
        _ => loose,
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
            "managed_turns_older",
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
            "configuration_inventory",
            "configuration_changes",
            "configuration_plan",
            "configuration_apply",
            "configuration_history",
            "configuration_undo",
            "configuration_records",
            "configuration_record_delete",
            "configuration_export",
            "configuration_import",
        ]),
    ))
    .expect("failed to build Wyss Console metadata");
}
