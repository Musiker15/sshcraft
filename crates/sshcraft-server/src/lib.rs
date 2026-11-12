//! Self-hosted sync server for SSHCraft.
//!
//! The server stores accounts, device logs, areas, key envelopes and sealed records and pushes
//! change notices to connected devices. It never sees a private key or a plaintext record.

pub mod admin;
pub mod auth;
pub mod config;
pub mod error;
pub mod hub;
pub mod routes;
pub mod state;
pub mod store;

pub use routes::app;
pub use state::AppState;
