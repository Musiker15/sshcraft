//! Encrypts private keys at rest. The master key lives in the OS credential store.
//!
//! Sealed format: `1` (format version) || 24-byte nonce || XChaCha20-Poly1305 ciphertext of the
//! unencrypted OpenSSH private key. The key id is the associated data, so a ciphertext cannot be
//! moved to another key record.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngExt;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, HashAlg, PrivateKey};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::model::{KeyId, ServerId};
use crate::store::{KeyRecord, Store, StoreError};

const FORMAT_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const KEYRING_SERVICE: &str = "de.musiker15.sshcraft";
const KEYRING_ACCOUNT: &str = "vault-master-key";

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error(
        "the master key is missing from the system credential store, existing keys cannot be decrypted"
    )]
    MasterKeyMissing,
    #[error("not a valid OpenSSH private key: {0}")]
    InvalidKey(String),
    #[error("wrong or missing passphrase")]
    WrongPassphrase,
    #[error("stored key data is corrupt")]
    Corrupt,
    #[error("key not found")]
    NotFound,
    #[error("system credential store: {0}")]
    Keyring(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub trait MasterKeyProvider: Send + Sync {
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>, VaultError>;
    fn save(&self, key: &[u8; 32]) -> Result<(), VaultError>;
}

/// Keeps the master key in memory only. For tests.
#[derive(Default)]
pub struct MemoryMasterKey(std::sync::Mutex<Option<[u8; 32]>>);

impl MasterKeyProvider for MemoryMasterKey {
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>, VaultError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| VaultError::Keyring("lock poisoned".into()))?
            .map(Zeroizing::new))
    }

    fn save(&self, key: &[u8; 32]) -> Result<(), VaultError> {
        *self
            .0
            .lock()
            .map_err(|_| VaultError::Keyring("lock poisoned".into()))? = Some(*key);
        Ok(())
    }
}

/// Keeps the master key in the OS credential store (Windows Credential Manager).
pub struct KeyringMasterKey;

impl KeyringMasterKey {
    fn entry() -> Result<keyring::Entry, VaultError> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
            .map_err(|e| VaultError::Keyring(e.to_string()))
    }
}

impl MasterKeyProvider for KeyringMasterKey {
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>, VaultError> {
        match Self::entry()?.get_secret() {
            Ok(bytes) => {
                let bytes = Zeroizing::new(bytes);
                let key: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                    VaultError::Keyring("stored master key has wrong length".into())
                })?;
                Ok(Some(Zeroizing::new(key)))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(VaultError::Keyring(e.to_string())),
        }
    }

    fn save(&self, key: &[u8; 32]) -> Result<(), VaultError> {
        Self::entry()?
            .set_secret(key)
            .map_err(|e| VaultError::Keyring(e.to_string()))
    }
}

/// Public facts about a key. Deliberately has no field that could carry secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyInfo {
    pub id: KeyId,
    pub name: String,
    pub algorithm: String,
    pub fingerprint: String,
    pub public_key: String,
    /// The sync server this key belongs to; missing means "only on this device".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<ServerId>,
}

impl From<KeyRecord> for KeyInfo {
    fn from(r: KeyRecord) -> Self {
        Self {
            id: r.id,
            name: r.name,
            algorithm: r.algorithm,
            fingerprint: r.fingerprint,
            public_key: r.public_key,
            location: r.location,
        }
    }
}

pub struct Vault {
    master: Zeroizing<[u8; 32]>,
}

impl Vault {
    /// Loads the master key, or creates one if no sealed key exists yet. Never replaces a
    /// missing master key while sealed keys exist: that would make them unreadable for good.
    pub fn unlock(
        store: &Store,
        provider: Box<dyn MasterKeyProvider>,
    ) -> Result<Vault, VaultError> {
        if let Some(master) = provider.load()? {
            return Ok(Vault { master });
        }
        if store.has_key_records()? {
            return Err(VaultError::MasterKeyMissing);
        }
        let mut fresh = Zeroizing::new([0u8; 32]);
        rand::rng().fill(&mut fresh[..]);
        provider.save(&fresh)?;
        Ok(Vault { master: fresh })
    }

    pub fn import_key(
        &self,
        store: &Store,
        name: &str,
        openssh_private_key: &str,
        passphrase: Option<&str>,
    ) -> Result<KeyInfo, VaultError> {
        let key = parse_private_key(openssh_private_key, passphrase)?;
        self.store_key(store, name, &key)
    }

    /// Seals and stores an already parsed key. Lets callers do the slow parsing (bcrypt for
    /// passphrase-protected keys) before they lock the store.
    pub fn store_key(
        &self,
        store: &Store,
        name: &str,
        key: &PrivateKey,
    ) -> Result<KeyInfo, VaultError> {
        self.seal_and_store(store, name, key)
    }

    /// Stores a key received from a sync server under its own id and location.
    pub fn store_key_with_id(
        &self,
        store: &Store,
        id: KeyId,
        name: &str,
        key: &PrivateKey,
        location: Option<ServerId>,
    ) -> Result<KeyInfo, VaultError> {
        self.seal_and_store_as(store, id, name, key, location)
    }

    /// Seals any secret (a device's sync keys, for example). `label` names what it is and must
    /// be the same to open it again, so a secret cannot be passed off as another.
    pub fn seal_secret(&self, label: &str, secret: &[u8]) -> Result<Vec<u8>, VaultError> {
        self.seal_with(&secret_aad(label), secret)
    }

    pub fn open_secret(
        &self,
        label: &str,
        sealed: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        self.open_with(&secret_aad(label), sealed)
    }

    pub fn generate_ed25519(&self, store: &Store, name: &str) -> Result<KeyInfo, VaultError> {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .map_err(|e| VaultError::InvalidKey(e.to_string()))?;
        self.seal_and_store(store, name, &key)
    }

    pub fn private_key(&self, store: &Store, id: KeyId) -> Result<PrivateKey, VaultError> {
        let record = store.key_record(id)?.ok_or(VaultError::NotFound)?;
        let plain = self.open(id, &record.sealed_secret)?;
        let text = std::str::from_utf8(&plain).map_err(|_| VaultError::Corrupt)?;
        PrivateKey::from_openssh(text).map_err(|_| VaultError::Corrupt)
    }

    fn seal_and_store(
        &self,
        store: &Store,
        name: &str,
        key: &PrivateKey,
    ) -> Result<KeyInfo, VaultError> {
        self.seal_and_store_as(store, KeyId::new(), name, key, None)
    }

    fn seal_and_store_as(
        &self,
        store: &Store,
        id: KeyId,
        name: &str,
        key: &PrivateKey,
        location: Option<ServerId>,
    ) -> Result<KeyInfo, VaultError> {
        let openssh = key
            .to_openssh(LineEnding::LF)
            .map_err(|e| VaultError::InvalidKey(e.to_string()))?;
        let public_key = key
            .public_key()
            .to_openssh()
            .map_err(|e| VaultError::InvalidKey(e.to_string()))?;
        let record = KeyRecord {
            id,
            name: name.trim().to_owned(),
            algorithm: key.algorithm().as_str().to_owned(),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
            public_key,
            sealed_secret: self.seal(id, openssh.as_bytes())?,
            location,
        };
        store.put_key_record(&record)?;
        Ok(record.into())
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        let key: &[u8; 32] = &self.master;
        XChaCha20Poly1305::new(key.into())
    }

    fn seal(&self, id: KeyId, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
        self.seal_with(id.0.as_bytes(), plaintext)
    }

    fn open(&self, id: KeyId, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        self.open_with(id.0.as_bytes(), sealed)
    }

    fn seal_with(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill(&mut nonce[..]);
        let ciphertext = self
            .cipher()
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| VaultError::Corrupt)?;
        let mut out = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
        out.push(FORMAT_VERSION);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    fn open_with(&self, aad: &[u8], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        let (&version, rest) = sealed.split_first().ok_or(VaultError::Corrupt)?;
        if version != FORMAT_VERSION || rest.len() < NONCE_LEN {
            return Err(VaultError::Corrupt);
        }
        let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| VaultError::Corrupt)?;
        self.cipher()
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| VaultError::Corrupt)
    }
}

/// Parses an OpenSSH private key, decrypting it with `passphrase` if it is protected.
/// Accepts Windows line endings and surrounding whitespace.
pub fn parse_private_key(
    openssh_private_key: &str,
    passphrase: Option<&str>,
) -> Result<PrivateKey, VaultError> {
    let normalized = Zeroizing::new(openssh_private_key.replace("\r\n", "\n"));
    // PEM (PKCS#1, PKCS#8) and PuTTY keys go through russh, which reads both.
    if !normalized
        .trim_start()
        .starts_with("-----BEGIN OPENSSH PRIVATE KEY-----")
    {
        let encrypted = normalized.contains("ENCRYPTED") || normalized.contains("Encryption: aes");
        return match russh::keys::decode_secret_key(normalized.trim(), passphrase) {
            Ok(key) => Ok(key),
            Err(_) if encrypted => Err(VaultError::WrongPassphrase),
            Err(e) => Err(VaultError::InvalidKey(e.to_string())),
        };
    }
    let parsed = PrivateKey::from_openssh(normalized.trim())
        .map_err(|e| VaultError::InvalidKey(e.to_string()))?;
    if parsed.is_encrypted() {
        let passphrase = passphrase.ok_or(VaultError::WrongPassphrase)?;
        parsed
            .decrypt(passphrase)
            .map_err(|_| VaultError::WrongPassphrase)
    } else {
        Ok(parsed)
    }
}

pub fn list_keys(store: &Store) -> Result<Vec<KeyInfo>, VaultError> {
    Ok(store
        .list_key_records()?
        .into_iter()
        .map(KeyInfo::from)
        .collect())
}

pub fn delete_key(store: &Store, id: KeyId) -> Result<(), VaultError> {
    Ok(store.delete_key_record(id)?)
}

#[cfg(test)]
#[path = "vault_tests.rs"]
mod tests;

/// Associated data for [`Vault::seal_secret`]; the prefix keeps it apart from key ids (16 bytes).
fn secret_aad(label: &str) -> Vec<u8> {
    let mut aad = b"sshcraft/secret/".to_vec();
    aad.extend_from_slice(label.as_bytes());
    aad
}
