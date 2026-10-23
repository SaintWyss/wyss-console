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
            "managed_model_catalogue",
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
            "configuration_author_hook",
            "configuration_edit_hook",
            "configuration_author_mcp",
            "configuration_edit_mcp",
            "configuration_edit_document",
            "configuration_document_export",
            "configuration_document_import",
            "configuration_create_document",
            "configuration_project",
            "configuration_select_project",
            "configuration_template_export",
            "native_launcher",
            "native_preview",
            "native_start",
            "native_sessions",
            "native_stop",
            "native_restart",
            "native_forget",
            "native_choose_project",
            "native_project_default",
            "native_profiles",
            "native_profile_create",
            "native_profile_save",
            "native_profile_delete",
            "native_profile_duplicate",
            "native_profile_restore",
            "native_profile_export",
            "native_profile_import",
            "host_terminals",
            "host_info",
            "terminal_redraw",
            "layout_read",
            "layout_write",
            "credentials_list",
            "credential_add",
            "credential_replace",
            "credential_rename",
            "credential_delete",
            "credential_verify",
            "native_handoff_prepare",
            "native_handoff_save",
            // The project-centered workspace (M1): granted by the `projects` capability, whose
            // Managed side can prepare a session and nothing more.
            "projects_overview",
            "projects_list",
            "projects_project",
            "projects_workspace",
            "projects_sessions",
            "projects_choose_folder",
            "projects_forget_picks",
            "projects_change",
            "projects_initialize_empty",
            "projects_migration_preview",
            "projects_migration_commit",
            "projects_input_claim",
            "projects_input_release",
            "projects_term_read",
            "projects_term_write",
            "projects_term_resize",
            "projects_rollback_report",
            "projects_compact_receipts",
            "projects_set_enabled",
            "projects_open_window",
            // Protocol 2 (F8): granted by the `f8` capability, which the installed
            // configuration does not enable; only the state read is in `main`.
            "f8_state",
            "f8_reconnect",
            "f8_sessions",
            "f8_session",
            "f8_session_prepare",
            "f8_session_close",
            "f8_runtime_start",
            "f8_runtime_stop",
            "f8_models",
            "f8_lease_grant",
            "f8_lease_renew",
            "f8_lease_revoke",
            "f8_binding_create",
            "f8_binding_resume",
            "f8_binding_abandon",
            "f8_turn_start",
            "f8_turn_cancel",
            "f8_turn_read",
            "f8_approval_read",
            "f8_approval_accept",
            "f8_approval_decline",
            "f8_approval_cancel",
            "f8_audit",
            "f8_audit_wait",
        ]),
    ))
    .expect("failed to build Wyss Console metadata");
}
