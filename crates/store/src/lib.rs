//! Local store and key vault of SSHCraft: the SQLite database (schema 11), the encrypted
//! secrets, session logs, history, snippets and the agent audit log.

pub mod agent;
pub mod history;
pub mod session_log;
pub mod snippets;
pub mod store;
pub mod vault;
