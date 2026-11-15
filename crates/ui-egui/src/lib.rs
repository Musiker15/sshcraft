#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
//! The egui interface of SSHCraft: the app shell, its pages, and the terminal. Pages read the
//! engine's state and act through [`runner::Runner`]; nothing here blocks a frame on the engine.

pub mod app;
pub mod commands;
pub mod completion;
pub mod confirm;
pub mod control;
pub mod data;
pub mod fonts;
pub mod forms;
pub mod history;
pub mod i18n;
pub mod keymap;
pub mod link;
pub mod log_viewer;
pub mod menus;
pub mod pages;
pub mod palette;
pub mod panes;
pub mod prompts;
pub mod quit;
pub mod runner;
pub mod session_tab;
pub mod shell;
pub mod state;
pub mod status;
pub mod terminal;
#[cfg(feature = "testing")]
pub mod testing;
pub mod theme;
pub mod toasts;
pub mod widgets;
pub mod window;

pub use app::SshcraftApp;
