pub mod adapters;
mod application;
#[cfg(target_os = "linux")]
pub mod authority;
pub mod contracts;
#[cfg(target_os = "linux")]
pub mod control;
mod desktop;
pub mod terminal;

pub fn run() {
    desktop::run();
}
