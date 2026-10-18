//! One terminal pane: a PTY with a shell, the `alacritty_terminal` emulator fed from
//! a reader thread, shell integration and process inspection.

pub mod input;
pub mod osc;
pub mod pane;
pub mod procinfo;
pub mod shell;

pub use pane::{Pane, PaneEvent, SpawnOptions};
