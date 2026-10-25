//! Sealed credential metadata and native entry commands. Delivery runs outside the GUI.
pub mod commands;
use crate::agentconfig::vault;
use crate::native::store as store_io;
mod core;
pub use core::*;
