//! M4: the screen a terminal's output draws, kept apart from the PTY.
//!
//! `terminal` is the byte-driven state machine, `repaint` turns its state back into a byte stream
//! that redraws it (what a window that attaches later receives), `ipc` and `helper` are the
//! typed, bounded protocol and the loop of the separate process that runs the parser, and
//! `sandbox` is what that process does to itself before it reads a byte.
pub mod repaint;
pub mod terminal;
pub mod width;

pub use terminal::{Cell, Color, Limits, Refusals, Row, Style, Terminal, mode};
#[cfg(test)]
mod tests;
