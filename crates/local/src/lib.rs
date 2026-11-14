//! Terminals that are not SSH, for the desktop: a local shell in a pseudo terminal, and a serial
//! port.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod pty;
pub mod serial;
