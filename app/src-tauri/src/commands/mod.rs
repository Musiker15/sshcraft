//! Tauri commands. None of them ever returns private key material.

pub mod data;
pub mod keys;
pub mod sessions;
pub mod sshconfig;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl CommandError {
    pub fn new(code: &'static str, message: impl ToString) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }
}

impl From<stollen_core::store::StoreError> for CommandError {
    fn from(e: stollen_core::store::StoreError) -> Self {
        Self::new("store", e)
    }
}

impl From<stollen_core::vault::VaultError> for CommandError {
    fn from(e: stollen_core::vault::VaultError) -> Self {
        use stollen_core::vault::VaultError as V;
        let code = match e {
            V::MasterKeyMissing => "master_key_missing",
            V::InvalidKey(_) => "invalid_key",
            V::WrongPassphrase => "wrong_passphrase",
            V::NotFound => "not_found",
            _ => "vault",
        };
        Self::new(code, e)
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

impl From<crate::passcli::PassCliError> for CommandError {
    fn from(e: crate::passcli::PassCliError) -> Self {
        use crate::passcli::PassCliError as P;
        let code = match e {
            P::NotInstalled => "pass_cli_missing",
            P::LoggedOut => "pass_cli_logged_out",
            P::NoPrivateKey => "no_private_key",
            P::Encrypted => "encrypted",
            _ => "pass_cli",
        };
        Self::new(code, e)
    }
}
