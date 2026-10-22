//! The session server (§3.3): runs on a thread of the app, owns every PTY, applies
//! workspace commands from GUI clients, streams grid deltas, writes the snapshot and
//! per-pane history.

pub mod server;
pub mod transport;

pub use server::{ClientEvent, ClientId, Server, ServerOptions};
