//! Tauri commands. None of them ever returns private key material.

pub mod data;
pub mod forwards;
pub mod identities;
pub mod import;
pub mod keys;
pub mod local;
pub mod logs;
pub mod passwords;
pub mod sessions;
pub mod shares;
pub mod snippets;
pub mod sshconfig;
pub mod sync;
pub mod workspaces;

pub use crate::errors::{CommandError, CommandResult, ErrorCode};
