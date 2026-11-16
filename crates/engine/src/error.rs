//! Errors as they leave the engine: a kind for the transports (MCP, control channel), a stable
//! code the interface translates, and an untranslated technical detail. No user-facing text.

use serde::Serialize;
use serde::ser::SerializeStruct;
use sshcraft_model::model::ResolveError;

/// Every failure code of a command. Each one needs `errors.<code>` in the translation files of
/// the interface (R4).
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
    KeyNotOnDevice,
    Network,
    Session,
    SyncBadOrigin,
    SyncOriginMismatch,
    SyncNetwork,
    SyncBadApiKey,
    SyncEmailTaken,
    SyncAccountNotFound,
    SyncRevoked,
    SyncBlocked,
    SyncBadAccountKey,
    SyncChecksum,
    SyncProtocol,
    SyncServer,
    SyncNotAllowed,
    ShareContactUnconfirmed,
    ShareReadOnly,
    ShareNotOwner,
    ShareAlreadyShared,
    ShareEndFirst,
    GroupCycle,
    ImportUnreadable,
    ImportTooLarge,
    ShareNestedGroup,
    GroupTooDeep,
    JumpLoop,
    TooManyJumps,
    JumpMissing,
    JumpFailed,
    ProxyFailed,
    ProxyAuthFailed,
    /// A proxy command that is not allowed on this device, and the user said no.
    ProxyCommandNotAllowed,
    /// A password reference that is not allowed for this address, and the user said no.
    SecretRefNotAllowed,
    /// The password manager could not give the password (not installed, locked, no such item).
    SecretRefFailed,
    /// A program of a step before or after connecting did not start, failed or ran too long.
    ConnectStepFailed,
    /// The stored TOTP secret is not Base32.
    TotpInvalid,
    PasswordRejected,
    NotSupported,
    LocalShellFailed,
    SerialFailed,
    JumpIncomplete,
    AgentForwardingUnavailable,
    /// X11 forwarding was asked for, but this computer has no display.
    X11Unavailable,
    /// X11 forwarding came through sync, a share or an import and was not allowed on this
    /// device: the session runs without it.
    X11NotAllowed,
    /// The tmux or screen session name is not one SSHCraft puts into a command.
    SessionNameInvalid,
    CertificateMismatch,
    InvalidCertificate,
    /// The certificate had expired: signed in with the key alone.
    CertificateExpired,
    UnsupportedKeyType,
    SecurityKeyCancelled,
    SecurityKeyFailed,
    /// The server did not run the `authorized_keys` script (no `sh`).
    InstallKeyUnsupported,
    InstallKeyFailed,
    /// A key bound to this device cannot go to a sync server.
    DeviceBound,
    /// The user cancelled a sign-in question (password, code, passphrase).
    SignInCancelled,
    InvalidTemplate,
    TooManyTemplates,
    /// Only `http` and `https` links are opened.
    InvalidLink,
    /// A port forwarding rule is not valid (ports, addresses, more than 50).
    InvalidForward,
    /// The local port of a forwarding rule is taken.
    PortInUse,
    /// The local address of a forwarding rule could not be bound.
    BindFailed,
    /// The server does not allow the remote forwarding.
    ForwardRefused,
    /// A snippet is not valid (name, content, more than 1000 snippets).
    InvalidSnippet,
    /// The command history needs the unlocked vault.
    HistoryUnavailable,
    /// A bookmark or comment is not valid (too long, past the end, more than 500).
    InvalidLogMark,
    /// A part of a session log is missing or does not open.
    LogDamaged,
    /// The session is not being recorded.
    NotRecording,
    /// It took longer than allowed.
    Timeout,
}

impl From<&ResolveError> for ErrorCode {
    fn from(e: &ResolveError) -> Self {
        match e {
            ResolveError::WrongGroup => Self::HostWrongGroup,
            ResolveError::MissingAddress => Self::HostMissingAddress,
            ResolveError::MissingUsername => Self::HostMissingUsername,
            ResolveError::NotFound => Self::NotFound,
            ResolveError::JumpLoop => Self::JumpLoop,
            ResolveError::TooManyJumps => Self::TooManyJumps,
            ResolveError::JumpMissing => Self::JumpMissing,
            ResolveError::Jump { .. } => Self::JumpIncomplete,
        }
    }
}

/// What a transport makes of an error (MCP, control channel, CLI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// No such host, group, key or session (or one hidden from agents).
    NotFound,
    /// Not allowed, or declined in SSHCraft.
    Denied,
    /// The request does not fit (arguments, name, size).
    Invalid,
    /// Allowed, but it did not work (connection, timeout, store).
    Failed,
    /// SSHCraft is not running, or its data is in use elsewhere.
    NotRunning,
}

/// Why a command did not run or did not succeed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command {0}")]
    UnknownCommand(String),
    #[error("{id} is not available: {reason}")]
    Disabled { id: String, reason: String },
    #[error("{id}: {message}")]
    BadParams { id: String, message: String },
    #[error("{0}")]
    Denied(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{code:?}: {detail:?}")]
    Failed { code: ErrorCode, detail: Option<String> },
    /// The command panicked; the engine caught it and goes on.
    #[error("{id} failed unexpectedly")]
    Internal { id: String },
    #[error("the data folder is in use by another SSHCraft")]
    DataInUse,
    #[error("an older SSHCraft (Tauri version) is running; close it first")]
    LegacyRunning,
    #[error("settings: {0}")]
    Settings(String),
}

impl EngineError {
    pub fn failed(code: ErrorCode, detail: impl ToString) -> Self {
        Self::Failed { code, detail: Some(detail.to_string()) }
    }

    /// The same as [`EngineError::failed`], under the name the moved commands use.
    pub fn new(code: ErrorCode, detail: impl ToString) -> Self {
        Self::failed(code, detail)
    }

    pub fn code(code: ErrorCode) -> Self {
        Self::Failed { code, detail: None }
    }

    pub fn bad_params(id: &str, message: impl ToString) -> Self {
        Self::BadParams { id: id.to_string(), message: message.to_string() }
    }

    /// The failure code; [`ErrorCode::Internal`] for errors that are not a failure.
    pub fn failure(&self) -> ErrorCode {
        match self {
            Self::Failed { code, .. } => *code,
            _ => ErrorCode::Internal,
        }
    }

    /// The technical detail, if there is one.
    pub fn detail_text(&self) -> Option<String> {
        self.detail()
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::UnknownCommand(_) | Self::Disabled { .. } | Self::BadParams { .. } | Self::Settings(_) => ErrorKind::Invalid,
            Self::Denied(_) => ErrorKind::Denied,
            Self::NotFound(_) | Self::Failed { code: ErrorCode::NotFound, .. } => ErrorKind::NotFound,
            Self::Failed { .. } | Self::Internal { .. } => ErrorKind::Failed,
            Self::DataInUse | Self::LegacyRunning => ErrorKind::NotRunning,
        }
    }

    /// The stable code of the error: the failure code, or the name of the variant.
    pub fn code_name(&self) -> &'static str {
        match self {
            Self::UnknownCommand(_) => "unknown_command",
            Self::Disabled { .. } => "disabled",
            Self::BadParams { .. } => "bad_params",
            Self::Denied(_) => "denied",
            Self::NotFound(_) => "not_found",
            Self::Failed { code, .. } => code.into(),
            Self::Internal { .. } => "internal",
            Self::DataInUse => "data_in_use",
            Self::LegacyRunning => "legacy_running",
            Self::Settings(_) => "settings",
        }
    }

    fn detail(&self) -> Option<String> {
        match self {
            Self::Failed { detail, .. } => detail.clone(),
            Self::DataInUse | Self::LegacyRunning => None,
            other => Some(other.to_string()),
        }
    }
}

impl Serialize for EngineError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("EngineError", 3)?;
        s.serialize_field("kind", &self.kind())?;
        s.serialize_field("code", self.code_name())?;
        s.serialize_field("detail", &self.detail())?;
        s.end()
    }
}

impl From<ResolveError> for EngineError {
    fn from(e: ResolveError) -> Self {
        Self::code(ErrorCode::from(&e))
    }
}

impl From<sshcraft_store::store::StoreError> for EngineError {
    fn from(e: sshcraft_store::store::StoreError) -> Self {
        use sshcraft_store::store::StoreError as E;
        match e {
            E::GroupCycle => Self::code(ErrorCode::GroupCycle),
            E::TooDeep => Self::code(ErrorCode::GroupTooDeep),
            E::DeviceBound => Self::code(ErrorCode::DeviceBound),
            E::InvalidForward(e) => Self::failed(ErrorCode::InvalidForward, e),
            e => Self::failed(ErrorCode::Store, e),
        }
    }
}

impl From<sshcraft_store::snippets::SnippetStoreError> for EngineError {
    fn from(e: sshcraft_store::snippets::SnippetStoreError) -> Self {
        use sshcraft_store::snippets::SnippetStoreError as E;
        match e {
            E::Invalid(e) => Self::failed(ErrorCode::InvalidSnippet, e),
            E::Vault(e) => e.into(),
            E::Store(e) => e.into(),
        }
    }
}

impl From<sshcraft_store::session_log::LogError> for EngineError {
    fn from(e: sshcraft_store::session_log::LogError) -> Self {
        use sshcraft_store::session_log::LogError as E;
        match e {
            E::Invalid(what) => Self::failed(ErrorCode::InvalidLogMark, what),
            E::NotFound => Self::code(ErrorCode::NotFound),
            E::Damaged => Self::code(ErrorCode::LogDamaged),
            E::Vault(e) => e.into(),
            E::Store(e) => e.into(),
        }
    }
}

impl From<sshcraft_store::vault::VaultError> for EngineError {
    fn from(e: sshcraft_store::vault::VaultError) -> Self {
        Self::failed(vault_code(&e), e)
    }
}

/// The failure code of a vault error.
pub fn vault_code(e: &sshcraft_store::vault::VaultError) -> ErrorCode {
    use sshcraft_store::vault::VaultError as V;
    match e {
        V::MasterKeyMissing => ErrorCode::MasterKeyMissing,
        V::InvalidKey(_) => ErrorCode::InvalidKey,
        V::WrongPassphrase | V::PassphraseRequired => ErrorCode::WrongPassphrase,
        V::NotFound => ErrorCode::NotFound,
        V::UnsupportedKeyType => ErrorCode::UnsupportedKeyType,
        V::CertificateMismatch => ErrorCode::CertificateMismatch,
        V::InvalidCertificate(_) => ErrorCode::InvalidCertificate,
        _ => ErrorCode::Vault,
    }
}

impl From<sshcraft_fido::FidoError> for EngineError {
    fn from(e: sshcraft_fido::FidoError) -> Self {
        use sshcraft_fido::FidoError as F;
        let code = match e {
            F::NotSupported => ErrorCode::NotSupported,
            F::Cancelled => ErrorCode::SecurityKeyCancelled,
            F::Failed(_) | F::BadResponse(_) => ErrorCode::SecurityKeyFailed,
        };
        Self::failed(code, e)
    }
}

#[cfg(test)]
mod tests {
    use super::{EngineError, ErrorCode};

    #[test]
    fn command_error_serializes_as_code_and_detail() {
        let value = serde_json::to_value(EngineError::failed(ErrorCode::AgentRefused, "x")).unwrap();
        assert_eq!(value, serde_json::json!({ "kind": "failed", "code": "agent_refused", "detail": "x" }));
        let value = serde_json::to_value(EngineError::code(ErrorCode::NotFound)).unwrap();
        assert_eq!(value, serde_json::json!({ "kind": "not_found", "code": "not_found", "detail": null }));
        let value = serde_json::to_value(EngineError::LegacyRunning).unwrap();
        assert_eq!(value, serde_json::json!({ "kind": "not_running", "code": "legacy_running", "detail": null }));
    }
}
