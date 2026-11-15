//! The local link to a running SSHCraft, for AI agents (`sshcraft-cli`, MCP) and for the
//! developer's control channel: JSON lines over TCP on loopback, nothing else.
//!
//! - Every connection signs in by challenge first ([`auth`]): the server proves it holds the
//!   token before the client sends anything derived from it, and the token itself never crosses
//!   the wire. Nothing is handed to the [`Handler`] before that.
//! - Lines are bounded ([`MAX_REQUEST`], [`MAX_REPLY`]), and so are connections
//!   ([`MAX_CONNECTIONS`]) and the requests of one connection running at once.
//! - The token lives in a file only the owner reads ([`Token::load_or_create`]); a running app
//!   publishes its port in a [`ConnectionFile`] beside it, which holds no secret.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod auth;
mod client;
mod files;
mod server;
mod wire;

pub use auth::Token;
pub use client::Client;
pub use files::{CONNECTION_FILE, ConnectionFile, TOKEN_FILE};
pub use server::{ConnId, Handler, Server, ServerOptions};
pub use wire::{Request, Response};

use std::time::Duration;

/// The longest request line a server reads; a longer one closes the connection.
pub const MAX_REQUEST: usize = 1 << 20;
/// The longest answer line; a longer answer is sent as an error instead.
pub const MAX_REPLY: usize = 8 << 20;
/// Connections one server serves at once.
pub const MAX_CONNECTIONS: usize = 16;
/// Requests of one connection running at once.
pub const MAX_RUNNING: usize = 64;
/// How long a new connection has to sign in.
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

/// Why a call over the link did not succeed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// Nothing answers there: the app is not running.
    #[error("SSHCraft is not running")]
    NotRunning,
    /// The other end is not the app, or refused the sign-in.
    #[error("{0}")]
    Refused(String),
    /// The app answered with an error.
    #[error("{0}")]
    Remote(String),
    /// The connection ended before the answer came.
    #[error("the connection to SSHCraft ended; the request may have run")]
    Closed,
    #[error("{0}")]
    Io(String),
}
