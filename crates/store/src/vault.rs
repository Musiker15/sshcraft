//! Encrypts private keys at rest. The master key lives in the OS credential store.
//!
//! Sealed format: `1` (format version) || 24-byte nonce || XChaCha20-Poly1305 ciphertext of the
//! unencrypted OpenSSH private key. The key id is the associated data, so a ciphertext cannot be
//! moved to another key record.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngExt;
use russh::keys::ssh_key::LineEnding;
use russh::keys::ssh_key::certificate::CertType;
use russh::keys::ssh_key::private::{KeypairData, RsaKeypair};
use russh::keys::{Algorithm, Certificate, EcdsaCurve, HashAlg, PrivateKey, PublicKey};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::store::{KeyRecord, Store, StoreError};
use sshcraft_model::keytypes::{CertificateInfo, EcdsaCurveName, KeyKind, KeySpec, RSA_BITS};
use sshcraft_model::model::{KeyId, PasswordId, ServerId};

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
    /// The key's passphrase is not saved: it has to be asked.
    #[error("the key's passphrase is needed")]
    PassphraseRequired,
    #[error("this key type or size is not supported")]
    UnsupportedKeyType,
    #[error("the certificate belongs to another key")]
    CertificateMismatch,
    #[error("not a valid OpenSSH user certificate: {0}")]
    InvalidCertificate(String),
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
    pub kind: KeyKind,
    /// The passphrase is asked on each use instead of being saved.
    pub protected: bool,
    pub certificate: Option<CertificateInfo>,
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
            kind: r.kind,
            protected: r.protected,
            certificate: r.certificate.as_deref().and_then(certificate_info),
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

    /// Seals and stores a password under `id`, at `location`.
    pub fn store_password(
        &self,
        store: &Store,
        id: PasswordId,
        secret: &str,
        location: Option<ServerId>,
    ) -> Result<(), VaultError> {
        let sealed = self.seal_with(&password_aad(id), secret.as_bytes())?;
        store.put_password_record(id, &sealed, location)?;
        Ok(())
    }

    pub fn password(&self, store: &Store, id: PasswordId) -> Result<Zeroizing<String>, VaultError> {
        let (sealed, _) = store.password_record(id)?.ok_or(VaultError::NotFound)?;
        let plain = self.open_with(&password_aad(id), &sealed)?;
        let text = std::str::from_utf8(&plain).map_err(|_| VaultError::Corrupt)?;
        Ok(Zeroizing::new(text.to_owned()))
    }

    /// Shorthand for a new Ed25519 key without passphrase.
    pub fn generate_ed25519(&self, store: &Store, name: &str) -> Result<KeyInfo, VaultError> {
        self.generate(store, name, &KeySpec::Ed25519, None)
    }

    /// Creates a key. With a (non-empty) passphrase it is stored protected: asked on each use.
    pub fn generate(
        &self,
        store: &Store,
        name: &str,
        spec: &KeySpec,
        passphrase: Option<&str>,
    ) -> Result<KeyInfo, VaultError> {
        let key = new_key(spec, name.trim())?;
        match non_empty(passphrase) {
            Some(passphrase) => self.store_protected(store, name, &key, passphrase),
            None => self.seal_and_store(store, name, &key),
        }
    }

    /// Imports a key whose passphrase is not saved: checks the passphrase, then keeps the key
    /// encrypted with it.
    pub fn import_key_protected(
        &self,
        store: &Store,
        name: &str,
        text: &str,
        passphrase: &str,
    ) -> Result<KeyInfo, VaultError> {
        let passphrase = non_empty(Some(passphrase)).ok_or(VaultError::WrongPassphrase)?;
        let key = parse_private_key(text, Some(passphrase))?;
        self.store_protected(store, name, &key, passphrase)
    }

    /// Stores `key` encrypted with `passphrase`; the decrypted key is never written. A security
    /// key (`sk-`) holds only a key handle and is stored as it is: its authenticator checks the
    /// user, a passphrase would only lock it out.
    pub fn store_protected(
        &self,
        store: &Store,
        name: &str,
        key: &PrivateKey,
        passphrase: &str,
    ) -> Result<KeyInfo, VaultError> {
        if is_security_key(key) {
            return self.seal_and_store(store, name, key);
        }
        let encrypted = encrypt(key, passphrase)?;
        self.write_record(
            store,
            KeyId::new(),
            name,
            &encrypted,
            KeyKind::Vault,
            None,
            None,
        )
    }

    /// Stores a key bound to this device (Windows Hello). It stays "only on this device".
    pub fn store_device_key(
        &self,
        store: &Store,
        name: &str,
        key: &PrivateKey,
    ) -> Result<KeyInfo, VaultError> {
        self.write_record(store, KeyId::new(), name, key, KeyKind::Device, None, None)
    }

    /// Stores a key received as OpenSSH text (encrypted or not) under its own id.
    pub fn store_openssh_with_id(
        &self,
        store: &Store,
        id: KeyId,
        name: &str,
        openssh: &str,
        certificate: Option<&str>,
        location: Option<ServerId>,
    ) -> Result<KeyInfo, VaultError> {
        let normalized = Zeroizing::new(openssh.replace("\r\n", "\n"));
        let key = PrivateKey::from_openssh(normalized.trim())
            .map_err(|e| VaultError::InvalidKey(e.to_string()))?;
        // A certificate that does not fit the key is dropped, not stored.
        let certificate =
            certificate.and_then(|text| check_certificate(key.public_key(), text).ok());
        self.write_record(store, id, name, &key, KeyKind::Vault, location, certificate)
    }

    /// Attaches an OpenSSH user certificate; it must be for this key.
    pub fn attach_certificate(
        &self,
        store: &Store,
        id: KeyId,
        text: &str,
    ) -> Result<KeyInfo, VaultError> {
        let mut record = store.key_record(id)?.ok_or(VaultError::NotFound)?;
        let public =
            PublicKey::from_openssh(&record.public_key).map_err(|_| VaultError::Corrupt)?;
        record.certificate = Some(check_certificate(&public, text)?);
        store.put_key_record(&record)?;
        Ok(record.into())
    }

    pub fn detach_certificate(&self, store: &Store, id: KeyId) -> Result<KeyInfo, VaultError> {
        let mut record = store.key_record(id)?.ok_or(VaultError::NotFound)?;
        record.certificate = None;
        store.put_key_record(&record)?;
        Ok(record.into())
    }

    /// The key, ready to sign. A key whose passphrase is not saved gives `PassphraseRequired`.
    pub fn private_key(&self, store: &Store, id: KeyId) -> Result<PrivateKey, VaultError> {
        let key = self.stored_key(store, id)?;
        if key.is_encrypted() {
            return Err(VaultError::PassphraseRequired);
        }
        Ok(key)
    }

    /// The key, decrypted with `passphrase` if its passphrase is not saved.
    pub fn unlock_key(
        &self,
        store: &Store,
        id: KeyId,
        passphrase: &str,
    ) -> Result<PrivateKey, VaultError> {
        let key = self.stored_key(store, id)?;
        if !key.is_encrypted() {
            return Ok(key);
        }
        key.decrypt(passphrase)
            .map_err(|_| VaultError::WrongPassphrase)
    }

    /// Saves the passphrase from now on: the key is kept decrypted (inside the vault).
    pub fn remember_passphrase(
        &self,
        store: &Store,
        id: KeyId,
        passphrase: &str,
    ) -> Result<KeyInfo, VaultError> {
        let record = store.key_record(id)?.ok_or(VaultError::NotFound)?;
        let key = self.unlock_key(store, id, passphrase)?;
        self.rewrite(store, &record, &key)
    }

    /// Stops saving the passphrase: the key is kept encrypted with `passphrase`.
    pub fn protect_key(
        &self,
        store: &Store,
        id: KeyId,
        passphrase: &str,
    ) -> Result<KeyInfo, VaultError> {
        let record = store.key_record(id)?.ok_or(VaultError::NotFound)?;
        if record.protected {
            return Ok(record.into());
        }
        let key = self.private_key(store, id)?;
        if record.kind == KeyKind::Device || is_security_key(&key) {
            return Err(VaultError::UnsupportedKeyType);
        }
        let encrypted = encrypt(&key, passphrase)?;
        self.rewrite(store, &record, &encrypted)
    }

    /// The stored OpenSSH text as it is, encrypted with the user's passphrase or not.
    pub fn stored_openssh(
        &self,
        store: &Store,
        id: KeyId,
    ) -> Result<Zeroizing<String>, VaultError> {
        let record = store.key_record(id)?.ok_or(VaultError::NotFound)?;
        let plain = self.open(id, &record.sealed_secret)?;
        let text = std::str::from_utf8(&plain).map_err(|_| VaultError::Corrupt)?;
        Ok(Zeroizing::new(text.to_owned()))
    }

    fn stored_key(&self, store: &Store, id: KeyId) -> Result<PrivateKey, VaultError> {
        let text = self.stored_openssh(store, id)?;
        PrivateKey::from_openssh(text.as_str()).map_err(|_| VaultError::Corrupt)
    }

    /// Replaces the stored key text of `record`, keeping everything else about it.
    fn rewrite(
        &self,
        store: &Store,
        record: &KeyRecord,
        key: &PrivateKey,
    ) -> Result<KeyInfo, VaultError> {
        self.write_record(
            store,
            record.id,
            &record.name,
            key,
            record.kind,
            record.location,
            record.certificate.clone(),
        )
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
        self.write_record(store, id, name, key, KeyKind::Vault, location, None)
    }

    /// Seals and writes `key` (encrypted with a passphrase or not) with all its public facts.
    /// The location of an existing record is kept; see [`Store::put_key_record`].
    #[allow(clippy::too_many_arguments)]
    fn write_record(
        &self,
        store: &Store,
        id: KeyId,
        name: &str,
        key: &PrivateKey,
        kind: KeyKind,
        location: Option<ServerId>,
        certificate: Option<String>,
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
            kind,
            protected: key.is_encrypted(),
            certificate,
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

/// The certificate attached to a key, if any.
pub fn certificate(store: &Store, id: KeyId) -> Result<Option<Certificate>, VaultError> {
    let record = store.key_record(id)?.ok_or(VaultError::NotFound)?;
    record
        .certificate
        .as_deref()
        .map(|text| Certificate::from_openssh(text).map_err(|_| VaultError::Corrupt))
        .transpose()
}

/// Checks that `text` is an OpenSSH user certificate for `public`; returns the trimmed line.
fn check_certificate(public: &PublicKey, text: &str) -> Result<String, VaultError> {
    let line = text.trim();
    let cert = Certificate::from_openssh(line)
        .map_err(|e| VaultError::InvalidCertificate(e.to_string()))?;
    if cert.cert_type() != CertType::User {
        return Err(VaultError::InvalidCertificate(
            "not a user certificate".into(),
        ));
    }
    if cert.public_key() != public.key_data() {
        return Err(VaultError::CertificateMismatch);
    }
    // A damaged or altered certificate would only be refused by the server later.
    cert.verify_signature()
        .map_err(|e| VaultError::InvalidCertificate(e.to_string()))?;
    Ok(line.to_owned())
}

fn certificate_info(text: &str) -> Option<CertificateInfo> {
    let cert = Certificate::from_openssh(text).ok()?;
    Some(CertificateInfo {
        key_id: cert.key_id().to_owned(),
        principals: cert.valid_principals().to_vec(),
        valid_after: cert.valid_after(),
        valid_before: cert.valid_before(),
        ca_fingerprint: cert
            .signature_key()
            .fingerprint(HashAlg::Sha256)
            .to_string(),
    })
}

/// A FIDO2 or Windows Hello key: signs through an authenticator, holds no secret.
fn is_security_key(key: &PrivateKey) -> bool {
    matches!(
        key.algorithm(),
        Algorithm::SkEcdsaSha2NistP256 | Algorithm::SkEd25519
    )
}

/// `None` for a missing or empty passphrase.
fn non_empty(passphrase: Option<&str>) -> Option<&str> {
    passphrase.filter(|p| !p.is_empty())
}

fn encrypt(key: &PrivateKey, passphrase: &str) -> Result<PrivateKey, VaultError> {
    let passphrase = non_empty(Some(passphrase)).ok_or(VaultError::WrongPassphrase)?;
    key.encrypt(&mut rand::rng(), passphrase)
        .map_err(|e| VaultError::InvalidKey(e.to_string()))
}

fn new_key(spec: &KeySpec, comment: &str) -> Result<PrivateKey, VaultError> {
    let mut rng = rand::rng();
    let invalid = |e: russh::keys::ssh_key::Error| VaultError::InvalidKey(e.to_string());
    let mut key = match *spec {
        KeySpec::Ed25519 => PrivateKey::random(&mut rng, Algorithm::Ed25519).map_err(invalid)?,
        KeySpec::Ecdsa { curve } => {
            let curve = match curve {
                EcdsaCurveName::Nistp256 => EcdsaCurve::NistP256,
                EcdsaCurveName::Nistp384 => EcdsaCurve::NistP384,
                EcdsaCurveName::Nistp521 => EcdsaCurve::NistP521,
            };
            PrivateKey::random(&mut rng, Algorithm::Ecdsa { curve }).map_err(invalid)?
        }
        KeySpec::Rsa { bits } => {
            if !RSA_BITS.contains(&bits) {
                return Err(VaultError::UnsupportedKeyType);
            }
            let pair = RsaKeypair::random(&mut rng, bits as usize).map_err(invalid)?;
            PrivateKey::new(KeypairData::from(pair), comment).map_err(invalid)?
        }
    };
    key.set_comment(comment);
    Ok(key)
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

/// Binds a sealed password to its id, so it cannot be passed off as another password, a key or
/// another secret.
fn password_aad(id: PasswordId) -> Vec<u8> {
    format!("password:{}", id.0).into_bytes()
}

/// Associated data for [`Vault::seal_secret`]; the prefix keeps it apart from key ids (16 bytes).
fn secret_aad(label: &str) -> Vec<u8> {
    let mut aad = b"sshcraft/secret/".to_vec();
    aad.extend_from_slice(label.as_bytes());
    aad
}
