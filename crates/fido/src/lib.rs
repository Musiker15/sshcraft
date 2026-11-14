//! FIDO2 security keys and Windows Hello as OpenSSH `sk-` keys.
//!
//! The key file holds no secret: the public key, the application (`ssh:`), flags and the
//! authenticator's key handle. Signing asks the authenticator, which checks presence (a touch)
//! and, if the key wants it, the user (PIN, fingerprint, face). [`Authenticator`] is that
//! authenticator: WebAuthn on Windows (`webauthn`), a software one in tests (`soft`).

use std::sync::Arc;

use russh::keys::ssh_key::private::{self, KeypairData};
use russh::keys::ssh_key::public::{self, EcdsaPublicKey, Ed25519PublicKey};
use russh::keys::{Algorithm, PrivateKey};
use serde::{Deserialize, Serialize};
use sshcraft_ssh::{KeySigner, SshError};

pub mod cose;
pub mod signature;
#[cfg(feature = "testing")]
pub mod soft;
#[cfg(windows)]
pub mod webauthn;

/// The application (WebAuthn relying party) of every key, as with OpenSSH.
pub const SSH_APPLICATION: &str = "ssh:";
/// The user was present (touched the key).
pub const FLAG_USER_PRESENCE: u8 = 0x01;
/// The user was verified (PIN, fingerprint, face).
pub const FLAG_USER_VERIFICATION: u8 = 0x04;
/// Authenticator data carries extensions.
pub(crate) const FLAG_EXTENSIONS: u8 = 0x80;
/// Authenticator data carries attested credential data.
pub(crate) const FLAG_ATTESTED: u8 = 0x40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkAlgorithm {
    EcdsaP256,
    Ed25519,
}

/// A token you plug in, or the authenticator built into this device (Windows Hello).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attachment {
    CrossPlatform,
    Platform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOptions {
    pub algorithm: SkAlgorithm,
    pub attachment: Attachment,
    /// Ask for PIN or biometrics on every use, not only a touch.
    pub verify_user: bool,
    pub user_name: String,
}

/// What a new credential came back with.
#[derive(Debug, Clone)]
pub struct Attestation {
    pub authenticator_data: Vec<u8>,
    pub credential_id: Vec<u8>,
}

/// A signature from the authenticator.
#[derive(Debug, Clone)]
pub struct Assertion {
    pub authenticator_data: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FidoError {
    #[error("security keys are not supported here")]
    NotSupported,
    #[error("cancelled")]
    Cancelled,
    #[error("the security key failed: {0}")]
    Failed(String),
    #[error("the security key answered something unexpected: {0}")]
    BadResponse(String),
}

#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
    /// Creates a credential for `application`. `client_data` is hashed (SHA-256) by the
    /// authenticator side, as WebAuthn does.
    async fn make_credential(&self, application: &str, options: &CreateOptions, client_data: &[u8]) -> Result<Attestation, FidoError>;

    /// Signs `authenticator data || SHA-256(client_data)` with the credential.
    async fn get_assertion(
        &self,
        application: &str,
        credential_id: &[u8],
        client_data: &[u8],
        verify_user: bool,
        attachment: Attachment,
    ) -> Result<Assertion, FidoError>;
}

/// Creates a new `sk-` key on the authenticator.
pub async fn create_key(auth: &dyn Authenticator, options: &CreateOptions, comment: &str) -> Result<PrivateKey, FidoError> {
    let challenge = random_challenge();
    let attestation = auth.make_credential(SSH_APPLICATION, options, &challenge).await?;
    let public = cose::attested_public_key(&attestation.authenticator_data, SSH_APPLICATION, options.algorithm)?;
    let flags = FLAG_USER_PRESENCE | if options.verify_user { FLAG_USER_VERIFICATION } else { 0 };
    let bad = |e: russh::keys::ssh_key::Error| FidoError::BadResponse(e.to_string());
    let handle = attestation.credential_id;
    let data = match public {
        cose::CosePublic::EcdsaP256 { sec1 } => {
            let EcdsaPublicKey::NistP256(point) = EcdsaPublicKey::from_sec1_bytes(&sec1).map_err(bad)? else {
                return Err(FidoError::BadResponse("not a P-256 point".into()));
            };
            let public = public::SkEcdsaSha2NistP256::new(point, SSH_APPLICATION);
            KeypairData::SkEcdsaSha2NistP256(private::SkEcdsaSha2NistP256::new(public, flags, handle).map_err(bad)?)
        }
        cose::CosePublic::Ed25519 { public } => {
            let public = public::SkEd25519::new(Ed25519PublicKey(public), SSH_APPLICATION);
            KeypairData::SkEd25519(private::SkEd25519::new(public, flags, handle).map_err(bad)?)
        }
    };
    PrivateKey::new(data, comment).map_err(bad)
}

/// Whether `key` is a FIDO2 / Windows Hello key (signs through an authenticator).
pub fn is_security_key(key: &PrivateKey) -> bool {
    matches!(key.algorithm(), Algorithm::SkEcdsaSha2NistP256 | Algorithm::SkEd25519)
}

/// Signs SSH sign-in requests through an authenticator.
pub struct SkSigner {
    auth: Arc<dyn Authenticator>,
    algorithm: SkAlgorithm,
    application: String,
    flags: u8,
    handle: Vec<u8>,
    attachment: Attachment,
}

impl SkSigner {
    pub fn new(auth: Arc<dyn Authenticator>, key: &PrivateKey, attachment: Attachment) -> Result<Self, FidoError> {
        let (algorithm, application, flags, handle) = match key.key_data() {
            KeypairData::SkEcdsaSha2NistP256(sk) => {
                (SkAlgorithm::EcdsaP256, sk.public().application().to_owned(), sk.flags(), sk.key_handle().to_vec())
            }
            KeypairData::SkEd25519(sk) => (SkAlgorithm::Ed25519, sk.public().application().to_owned(), sk.flags(), sk.key_handle().to_vec()),
            _ => return Err(FidoError::NotSupported),
        };
        Ok(Self { auth, algorithm, application, flags, handle, attachment })
    }
}

#[async_trait::async_trait]
impl KeySigner for SkSigner {
    async fn sign(&mut self, data: &[u8]) -> Result<Vec<u8>, SshError> {
        let assertion =
            self.auth.get_assertion(&self.application, &self.handle, data, self.flags & FLAG_USER_VERIFICATION != 0, self.attachment).await.map_err(
                |e| match e {
                    FidoError::Cancelled => SshError::SignerCancelled,
                    other => SshError::SignerFailed(other.to_string()),
                },
            )?;
        signature::ssh_signature(self.algorithm, &self.application, &assertion).map_err(|e| SshError::SignerFailed(e.to_string()))
    }
}

/// A fresh challenge for a new credential; SSH does not check it, WebAuthn wants one.
fn random_challenge() -> [u8; 32] {
    rand::random()
}

#[cfg(test)]
mod tests;
