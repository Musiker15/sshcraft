//! Sync protocol and end-to-end cryptography for SSHCraft.
//!
//! Pure library: no I/O, no clock, no database. The client and the server share these types;
//! the server only ever sees headers (ids, versions, public keys) and ciphertext.

pub mod area;
pub mod devicelog;
pub mod encoding;
pub mod identity;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtoError {
    #[error("not a SSHCraft account key")]
    BadAccountKeyFormat,
    #[error("the account key checksum does not match, check for a typo")]
    BadChecksum,
    #[error("invalid signature")]
    BadSignature,
    #[error("unknown device")]
    UnknownDevice,
    #[error("the device is revoked")]
    DeviceRevoked,
    #[error("the device log is broken: {0}")]
    LogBroken(String),
    #[error("older state than already seen")]
    Rollback,
    #[error("two different states with the same version")]
    Fork,
    #[error("not allowed")]
    NotAllowed,
    #[error("decryption failed")]
    Decrypt,
    #[error("malformed data: {0}")]
    Malformed(String),
}
