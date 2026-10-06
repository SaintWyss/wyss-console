//! Explicit developer tool; not a Tauri command or packaged desktop entrypoint.
fn main() {
    let report = wyss_console::adapters::codex::probe::run_codex_live_probe();
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("safe report serialization")
    );
    // Absence is a diagnostic outcome, not a deterministic test failure. The
    // developer harness returns nonzero for an observed failed/incompatible probe.
    if report.runtime_found() && !report.successful_probe() {
        std::process::exit(1);
    }
}
