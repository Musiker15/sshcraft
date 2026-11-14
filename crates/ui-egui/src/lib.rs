#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
//! The egui interface of SSHCraft. In R3 it holds the terminal: layout and painting of a
//! [`sshcraft_term::Screen`], the widget that takes keyboard, IME and mouse, and the pump that
//! feeds a session's output into the terminal with flow control.

pub mod terminal;
