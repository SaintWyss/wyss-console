//! The desktop's protocol-2 interface: a typed client, the projection the window is told and
//! the service that joins them. Inactive unless Rust code that owns a `Connector` says
//! otherwise; the installed desktop never does.
mod client;
mod service;
mod view;

pub use client::{Answer, CallError, Connector, F8Client, HandleConnector, SocketConnector};
pub use service::{F8Gui, F8Stats, WATCH_WAIT_MS};
pub use view::*;

#[cfg(test)]
pub(crate) mod harness;
#[cfg(test)]
mod tests;
