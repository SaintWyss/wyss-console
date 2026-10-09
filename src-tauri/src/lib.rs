pub mod adapters;
mod application;
#[cfg(target_os = "linux")]
pub mod authority;
pub mod contracts;
#[cfg(target_os = "linux")]
pub mod control;
mod desktop;
#[cfg(target_os = "linux")]
pub mod events;
#[cfg(target_os = "linux")]
pub mod sessions;
pub mod terminal;
#[cfg(target_os = "linux")]
pub mod turns;

pub fn run() {
    desktop::run();
}
