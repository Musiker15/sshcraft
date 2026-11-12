//! Errors as they reach the UI: a stable code the UI translates, plus an untranslated
//! technical detail. No user-facing text is written here.

use serde::Serialize;
use sshcraft_core::model::ResolveError;

/// Every code the UI can receive. Each one needs `errors.<code>` in both translation files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::EnumIter, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ErrorCode {
    Store,
    Io,
    Internal,
    NotFound,
    MasterKeyMissing,
    InvalidKey,
    WrongPassphrase,
    Vault,
    VaultLocked,
    PassCliMissing,
    PassCliLoggedOut,
    PassCliFailed,
    PassCliTimeout,
    NoPrivateKey,
    Encrypted,
    AgentUnavailable,
    AgentKeyMissing,
    AgentRefused,
    HelloDeclined,
    HostWrongGroup,
    HostMissingAddress,
    HostMissingUsername,
    HostMissingKey,
    Network,
    Session,
}

impl From<&ResolveError> for ErrorCode {
    fn from(e: &ResolveError) -> Self {
        match e {
            ResolveError::WrongGroup => Self::HostWrongGroup,
            ResolveError::MissingAddress => Self::HostMissingAddress,
            ResolveError::MissingUsername => Self::HostMissingUsername,
            ResolveError::MissingKey => Self::HostMissingKey,
        }
    }
}

/// What a failed Tauri command returns: `{ "code": "...", "detail": "..." | null }`.
#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    pub code: ErrorCode,
    pub detail: Option<String>,
}

impl CommandError {
    pub fn new(code: ErrorCode, detail: impl ToString) -> Self {
        Self {
            code,
            detail: Some(detail.to_string()),
        }
    }

    pub fn code(code: ErrorCode) -> Self {
        Self { code, detail: None }
    }
}

impl From<ResolveError> for CommandError {
    fn from(e: ResolveError) -> Self {
        Self::code(ErrorCode::from(&e))
    }
}

impl From<sshcraft_core::store::StoreError> for CommandError {
    fn from(e: sshcraft_core::store::StoreError) -> Self {
        Self::new(ErrorCode::Store, e)
    }
}

impl From<sshcraft_core::vault::VaultError> for CommandError {
    fn from(e: sshcraft_core::vault::VaultError) -> Self {
        use sshcraft_core::vault::VaultError as V;
        let code = match e {
            V::MasterKeyMissing => ErrorCode::MasterKeyMissing,
            V::InvalidKey(_) => ErrorCode::InvalidKey,
            V::WrongPassphrase => ErrorCode::WrongPassphrase,
            V::NotFound => ErrorCode::NotFound,
            _ => ErrorCode::Vault,
        };
        Self::new(code, e)
    }
}

impl From<crate::passcli::PassCliError> for CommandError {
    fn from(e: crate::passcli::PassCliError) -> Self {
        use crate::passcli::PassCliError as P;
        let code = match e {
            P::NotInstalled => ErrorCode::PassCliMissing,
            P::LoggedOut => ErrorCode::PassCliLoggedOut,
            P::NoPrivateKey => ErrorCode::NoPrivateKey,
            P::Encrypted => ErrorCode::Encrypted,
            P::Timeout => ErrorCode::PassCliTimeout,
            _ => ErrorCode::PassCliFailed,
        };
        Self::new(code, e)
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    fn keys(json: &str) -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str(json).expect("valid translation file")
    }

    #[test]
    fn every_error_code_has_a_translation() {
        let en = keys(include_str!("../../src/i18n/en.json"));
        let de = keys(include_str!("../../src/i18n/de.json"));
        for code in ErrorCode::iter() {
            let key = format!("errors.{}", <&'static str>::from(code));
            assert!(en.contains_key(&key), "en.json lacks {key}");
            assert!(de.contains_key(&key), "de.json lacks {key}");
        }
    }

    #[test]
    fn command_error_serializes_as_code_and_detail() {
        let value = serde_json::to_value(CommandError::new(ErrorCode::AgentRefused, "x")).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "code": "agent_refused", "detail": "x" })
        );
        let value = serde_json::to_value(CommandError::code(ErrorCode::NotFound)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "code": "not_found", "detail": null })
        );
    }
}
