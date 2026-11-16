//! Data model of SSHCraft: hosts, groups, identities, forwarding rules and agent policy.
//! Pure data, no storage and no network.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod appearance;
pub mod forward;
pub mod keytypes;
pub mod login_script;
pub mod logtext;
pub mod model;
pub mod os_detect;
pub mod placeholders;
pub mod policy;
pub mod proxy_command;
pub mod secret_ref;
pub mod totp;
