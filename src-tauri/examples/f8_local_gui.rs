//! The desktop with the protocol-2 interface active, against a local application whose runtime
//! is a fixture child (not Codex). For looking at the window; development only.
//!
//! ```text
//! cargo run --example f8_local_gui --features f8-dev -- <private directory>
//! ```
//! The directory holds the application's durable image and the fixture's workspace.
fn main() {
    let dir = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .expect("name a private directory for the application's image and workspace");
    let mut app = wyss_console::f8::dev::DevApplication::start(&dir)
        .expect("the local protocol-2 application did not start");
    wyss_console::run_with_f8(None, None, app.gui.clone());
    let _ = app.stop();
}
