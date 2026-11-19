pub(super) mod categories;
mod distros;
#[cfg(test)]
mod markdown;
pub(crate) mod model;
mod view;

pub use view::build;
pub const CSS: &str = include_str!("workspace.css");
