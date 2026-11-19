mod config;
mod library;
mod model;
mod view;

pub const CSS: &str = include_str!("arsenal.css");
pub use view::build;
