//! DIAGNOSTIC ONLY (branch `diag/lease-hang-trace-2026-10-25`, never integrated): a bounded,
//! append-only trace with monotonic timestamps. Active only when `WYSS_LAB_TRACE` names a file.
//! It records event names, short identity prefixes and durations; no prompt, path or secret.
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};

fn sink() -> Option<&'static PathBuf> {
    static SINK: OnceLock<Option<PathBuf>> = OnceLock::new();
    SINK.get_or_init(|| std::env::var_os("WYSS_LAB_TRACE").map(PathBuf::from))
        .as_ref()
}
fn t0() -> Instant {
    static T0: OnceLock<Instant> = OnceLock::new();
    *T0.get_or_init(Instant::now)
}
pub fn start() {
    let _ = t0();
}
pub fn trace(event: &str, detail: &str) {
    let Some(path) = sink() else { return };
    let ms = t0().elapsed().as_secs_f64() * 1000.0;
    let id = format!("{:?}", std::thread::current().id());
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(format!("{ms:>10.1} {id:<10} {event} {detail}\n").as_bytes());
    }
}
/// An action's identity, cut to what correlates and nothing more.
pub fn short(id: &str) -> String {
    id.chars().take(8).collect()
}
pub fn auto_allow() -> Option<Duration> {
    std::env::var("WYSS_LAB_AUTO_ALLOW_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
}
/// What the page reports about itself while a driver runs.
#[tauri::command]
pub fn lab_trace(event: String, detail: String) {
    let detail: String = detail.chars().take(600).collect();
    trace(&format!("js:{event}"), &detail);
}
/// Main-thread liveness: a tick every 250 ms; a gap well past that is a stalled main loop.
#[cfg(target_os = "linux")]
pub fn heartbeat() {
    if sink().is_none() {
        return;
    }
    let last = std::cell::Cell::new(Instant::now());
    gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
        let gap = last.replace(Instant::now()).elapsed();
        if gap > Duration::from_millis(700) {
            trace("main.stall", &format!("gap_ms={}", gap.as_millis()));
        }
        gtk::glib::ControlFlow::Continue
    });
}
