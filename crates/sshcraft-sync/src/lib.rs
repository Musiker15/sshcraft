//! Client side of SSHCraft sync: talks to a sync server and keeps this device in step with it.
//!
//! Everything that leaves the device is sealed with `sshcraft-proto`; everything that comes back
//! is verified before it touches the local store.

pub mod account;
pub mod areas;
pub mod client;
pub mod contacts;
pub mod devices;
pub mod engine;
pub mod items;
pub mod runtime;
pub mod shares;

use sshcraft_proto::ProtoError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    /// The server address is not an `http://` or `https://` address with a host.
    #[error("not a server address")]
    BadOrigin,
    /// The server was set up for another address than the one entered; logins would fail.
    #[error("the server expects the address {server}")]
    OriginMismatch { server: String },
    /// The server could not be reached or did not answer in time.
    #[error("network: {0}")]
    Network(String),
    /// The server answered with an error code (see docs/server-api.md).
    #[error("server answered {status} {code}")]
    Server {
        status: u16,
        code: String,
        current: Option<u64>,
    },
    /// Something the server sent did not verify, or local data is malformed.
    #[error("protocol: {0}")]
    Proto(ProtoError),
    #[error("local store: {0}")]
    Store(String),
    #[error("vault: {0}")]
    Vault(String),
    /// The server's operator blocked this device; that can be lifted, so it is not stored.
    #[error("this device is blocked by the server's operator")]
    Blocked,
    /// This device was revoked on the server.
    #[error("this device was revoked")]
    Revoked,
    /// No account with this account key exists on the server.
    #[error("no such account on this server")]
    AccountNotFound,
    /// The server is not set up on this device.
    #[error("not set up")]
    NotSetUp,
    /// The action is not allowed (revoking this device or the recovery device, for example).
    #[error("not allowed")]
    NotAllowed,
    /// Sharing needs a contact whose safety number the user confirmed.
    #[error("the recipient's safety number is not confirmed")]
    ContactUnconfirmed,
    /// The group is already shared with all its hosts; add recipients to that share instead.
    #[error("already shared")]
    AlreadyShared,
}

impl SyncError {
    /// The server's error code, if the server answered with one.
    pub fn server_code(&self) -> Option<&str> {
        match self {
            Self::Server { code, .. } => Some(code),
            _ => None,
        }
    }
}

impl From<ProtoError> for SyncError {
    fn from(err: ProtoError) -> Self {
        Self::Proto(err)
    }
}

impl From<sshcraft_core::store::StoreError> for SyncError {
    fn from(err: sshcraft_core::store::StoreError) -> Self {
        Self::Store(err.to_string())
    }
}

impl From<sshcraft_core::vault::VaultError> for SyncError {
    fn from(err: sshcraft_core::vault::VaultError) -> Self {
        Self::Vault(err.to_string())
    }
}
