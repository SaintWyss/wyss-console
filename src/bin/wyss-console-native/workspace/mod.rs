pub(super) mod categories;
mod distros;
pub(crate) mod model;
mod view;

pub use view::build;
pub const CSS: &str = include_str!("workspace.css");
