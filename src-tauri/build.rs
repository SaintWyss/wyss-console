fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_app_info",
            "authority_state",
            "managed_sessions_page",
            "managed_session_prepare",
            "managed_session_resume",
            "managed_session_close",
            "managed_session_activate",
            "managed_session_abandon",
            "choose_workspace",
            "managed_turns",
            "managed_turn_read",
            "managed_turn_start",
            "managed_turn_cancel",
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
