//! The session daemon (§3.3): owns every PTY, applies workspace commands from GUI
//! clients, streams grid deltas, writes the snapshot and per-pane history.

pub mod autostart;
pub mod server;
pub mod transport;

pub use server::{ClientEvent, ClientId, Server, ServerOptions};
