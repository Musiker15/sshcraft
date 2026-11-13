//! AI agents: the gate every request passes, and the connection the `sshcraft` tool uses.

pub mod backend;
pub mod gate;
pub mod server;
#[cfg(desktop)]
pub mod tauri;
