#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
//! The egui interface of SSHCraft: the app shell, its pages, and the terminal. Pages read the
//! engine's state and act through [`runner::Runner`]; nothing here blocks a frame on the engine.

pub mod app;
pub mod data;
pub mod i18n;
pub mod runner;
pub mod state;
pub mod terminal;
#[cfg(feature = "testing")]
pub mod testing;
pub mod theme;
pub mod toasts;
pub mod widgets;

pub use app::SshcraftApp;
