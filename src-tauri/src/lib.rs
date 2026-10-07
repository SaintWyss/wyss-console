pub mod adapters;
mod application;
pub mod contracts;
#[cfg(target_os = "linux")]
pub mod control;
mod desktop;
pub mod terminal;

pub fn run() {
    desktop::run();
}
