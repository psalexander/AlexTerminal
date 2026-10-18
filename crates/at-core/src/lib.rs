//! Core model of AlexTerminal. Has no GUI or PTY dependencies so it can be shared
//! by the daemon, the GUI client and the tests.

pub mod config;
pub mod history;
pub mod ids;
pub mod keymap;
pub mod model;
pub mod pane;
pub mod paths;
pub mod snapshot;
pub mod theme;
pub mod time;

pub use ids::{PaneId, TabId, WindowId};
