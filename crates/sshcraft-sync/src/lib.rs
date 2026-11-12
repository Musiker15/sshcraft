//! Client side of SSHCraft sync: talks to a sync server and keeps this device in step with it.
//!
//! Everything that leaves the device is sealed with `sshcraft-proto`; everything that comes back
//! is verified before it touches the local store.

pub mod account;
pub mod client;
pub mod devices;
pub mod engine;
pub mod items;
pub mod runtime;

use sshcraft_proto::ProtoError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    /// The server address is not an `http://` or `https://` address with a host.
    #[error("not a server address")]
    BadOrigin,
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
