fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_app_info",
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
