//! Local store and key vault of SSHCraft: the SQLite database (schema 14), the encrypted
//! secrets, session logs, history, snippets and the agent audit log.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod agent;
pub mod backup;
pub mod history;
pub mod portable;
pub mod session_log;
pub mod snippets;
pub mod store;
pub mod trash;
pub mod vault;
