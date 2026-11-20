//! Portable mode keeps the vault's master key beside the data instead of in the system's
//! credential store, so a copy of SSHCraft on a USB stick opens on any computer. The key file is
//! sealed with a key made from a passphrase (Argon2id) that is asked before the engine starts;
//! without the passphrase the file, and with it every sealed secret, cannot be read.
//!
//! The file is JSON: the key derivation parameters, the salt, the nonce and the master key sealed
//! with XChaCha20-Poly1305. The parameters are the associated data, so they cannot be lowered
//! unnoticed.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::vault::{MasterKeyProvider, VaultError};

/// The key file's name in the data folder.
pub const KEY_FILE: &str = "master-key";
/// The shortest passphrase taken for a new key file, the same as for a backup.
pub const MIN_PASSPHRASE: usize = crate::backup::MIN_PASSPHRASE;
const MAGIC: &str = "sshcraft-master-key-1";
/// The largest key file read; a real one is a few hundred bytes.
const MAX_FILE: u64 = 4096;
/// Argon2id: 64 MiB, three passes, one lane (as for a backup).
const KDF_MEMORY_KIB: u32 = 64 * 1024;
const KDF_PASSES: u32 = 3;
/// The most a file may ask for, so a crafted file cannot make the computer allocate gigabytes.
const KDF_MAX_MEMORY_KIB: u32 = 256 * 1024;
const KDF_MAX_PASSES: u32 = 8;
/// The least a file may ask for, so a changed file cannot make the passphrase cheap to guess.
const KDF_MIN_MEMORY_KIB: u32 = 19 * 1024;
const KDF_MIN_PASSES: u32 = 2;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;

/// Why the key file could not be opened or written.
#[derive(Debug, thiserror::Error)]
pub enum PortableError {
    #[error("the passphrase needs at least {MIN_PASSPHRASE} characters")]
    WeakPassphrase,
    #[error("the passphrase is wrong")]
    WrongPassphrase,
    #[error("the master key file is damaged")]
    Damaged,
    #[error("{0}")]
    Io(String),
}

#[derive(Serialize, Deserialize)]
struct KeyFile {
    format: String,
    kdf: String,
    memory_kib: u32,
    passes: u32,
    salt: String,
    nonce: String,
    sealed: String,
}

/// Where the key file of `data_dir` lives.
pub fn key_file(data_dir: &Path) -> PathBuf {
    data_dir.join(KEY_FILE)
}

/// Whether `data_dir` has no key file yet, so the first start chooses a passphrase.
pub fn is_new(data_dir: &Path) -> bool {
    !key_file(data_dir).exists()
}

/// The vault's master key, sealed with a passphrase in the data folder.
pub struct PassphraseMasterKey {
    path: PathBuf,
    wrapping: Zeroizing<[u8; 32]>,
    salt: [u8; SALT_LEN],
    memory_kib: u32,
    passes: u32,
    master: Mutex<Option<Zeroizing<[u8; 32]>>>,
}

impl PassphraseMasterKey {
    /// Opens the key file in `data_dir` with `passphrase`. Without a key file, prepares a new one
    /// sealed with `passphrase`; it is written when the vault makes its master key. Slow on
    /// purpose (Argon2id): run it away from the window's thread.
    pub fn open(data_dir: &Path, passphrase: &str) -> Result<Self, PortableError> {
        let path = key_file(data_dir);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.len() > MAX_FILE => Err(PortableError::Damaged),
            Ok(_) => Self::unseal(path, passphrase),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::prepare(path, passphrase),
            Err(e) => Err(PortableError::Io(e.to_string())),
        }
    }

    fn prepare(path: PathBuf, passphrase: &str) -> Result<Self, PortableError> {
        if passphrase.chars().count() < MIN_PASSPHRASE {
            return Err(PortableError::WeakPassphrase);
        }
        let mut salt = [0u8; SALT_LEN];
        rand::rng().fill(&mut salt[..]);
        let wrapping = derive(passphrase, &salt, KDF_MEMORY_KIB, KDF_PASSES)?;
        Ok(Self { path, wrapping, salt, memory_kib: KDF_MEMORY_KIB, passes: KDF_PASSES, master: Mutex::new(None) })
    }

    fn unseal(path: PathBuf, passphrase: &str) -> Result<Self, PortableError> {
        let text = std::fs::read_to_string(&path).map_err(|e| PortableError::Io(e.to_string()))?;
        let file: KeyFile = serde_json::from_str(&text).map_err(|_| PortableError::Damaged)?;
        let in_bounds = |value: u32, min: u32, max: u32| (min..=max).contains(&value);
        if file.format != MAGIC
            || file.kdf != "argon2id"
            || !in_bounds(file.memory_kib, KDF_MIN_MEMORY_KIB, KDF_MAX_MEMORY_KIB)
            || !in_bounds(file.passes, KDF_MIN_PASSES, KDF_MAX_PASSES)
        {
            return Err(PortableError::Damaged);
        }
        let salt: [u8; SALT_LEN] = unb64(&file.salt)?.try_into().map_err(|_| PortableError::Damaged)?;
        let nonce: [u8; NONCE_LEN] = unb64(&file.nonce)?.try_into().map_err(|_| PortableError::Damaged)?;
        let sealed = unb64(&file.sealed)?;
        let wrapping = derive(passphrase, &salt, file.memory_kib, file.passes)?;
        let aad = associated_data(file.memory_kib, file.passes);
        let plain = Zeroizing::new(
            XChaCha20Poly1305::new((&*wrapping).into())
                .decrypt(&XNonce::from(nonce), Payload { msg: &sealed, aad: aad.as_bytes() })
                .map_err(|_| PortableError::WrongPassphrase)?,
        );
        let master: [u8; 32] = plain.as_slice().try_into().map_err(|_| PortableError::Damaged)?;
        Ok(Self { path, wrapping, salt, memory_kib: file.memory_kib, passes: file.passes, master: Mutex::new(Some(Zeroizing::new(master))) })
    }

    fn write(&self, key: &[u8; 32]) -> Result<(), PortableError> {
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill(&mut nonce[..]);
        let aad = associated_data(self.memory_kib, self.passes);
        let sealed = XChaCha20Poly1305::new((&*self.wrapping).into())
            .encrypt(&XNonce::from(nonce), Payload { msg: key, aad: aad.as_bytes() })
            .map_err(|_| PortableError::Io("the master key could not be sealed".into()))?;
        let file = KeyFile {
            format: MAGIC.into(),
            kdf: "argon2id".into(),
            memory_kib: self.memory_kib,
            passes: self.passes,
            salt: b64(&self.salt),
            nonce: b64(&nonce),
            sealed: b64(&sealed),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|e| PortableError::Io(e.to_string()))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| PortableError::Io(e.to_string()))?;
        }
        // Written whole or not at all: a half-written key file would lock the data away.
        let part = self.path.with_extension("part");
        std::fs::write(&part, text).map_err(|e| PortableError::Io(e.to_string()))?;
        std::fs::rename(&part, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            PortableError::Io(e.to_string())
        })
    }
}

impl MasterKeyProvider for PassphraseMasterKey {
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>, VaultError> {
        Ok(self.master.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }

    fn save(&self, key: &[u8; 32]) -> Result<(), VaultError> {
        self.write(key).map_err(|e| VaultError::KeyFile(e.to_string()))?;
        *self.master.lock().unwrap_or_else(PoisonError::into_inner) = Some(Zeroizing::new(*key));
        Ok(())
    }
}

fn associated_data(memory_kib: u32, passes: u32) -> String {
    format!("{MAGIC};argon2id;{memory_kib};{passes}")
}

fn derive(passphrase: &str, salt: &[u8], memory_kib: u32, passes: u32) -> Result<Zeroizing<[u8; 32]>, PortableError> {
    let params = Params::new(memory_kib, passes, 1, Some(32)).map_err(|_| PortableError::Damaged)?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
        .map_err(|_| PortableError::Damaged)?;
    Ok(key)
}

fn b64(bytes: &[u8]) -> String {
    data_encoding::BASE64.encode(bytes)
}

fn unb64(text: &str) -> Result<Vec<u8>, PortableError> {
    data_encoding::BASE64.decode(text.trim().as_bytes()).map_err(|_| PortableError::Damaged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::vault::Vault;

    const PASSPHRASE: &str = "correct horse battery";

    fn file_json(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(key_file(dir)).unwrap()).unwrap()
    }

    /// The first start seals a fresh master key with the passphrase; the next start opens it with
    /// the same passphrase and gets the same key, so sealed secrets stay readable.
    #[test]
    fn the_master_key_comes_back_with_the_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_new(dir.path()));
        let store = Store::open(&dir.path().join("sshcraft.db")).unwrap();
        let first = Vault::unlock(&store, Box::new(PassphraseMasterKey::open(dir.path(), PASSPHRASE).unwrap())).unwrap();
        let password = sshcraft_model::model::PasswordId::new();
        first.store_password(&store, password, "s3cret", None).unwrap();
        assert!(!is_new(dir.path()));
        assert!(!dir.path().join("master-key.part").exists());

        let again = Vault::unlock(&store, Box::new(PassphraseMasterKey::open(dir.path(), PASSPHRASE).unwrap())).unwrap();
        assert_eq!(again.password(&store, password).unwrap().as_str(), "s3cret");
    }

    /// The key file holds no master key in the clear, and nothing opens it without the passphrase.
    #[test]
    fn a_wrong_passphrase_opens_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let provider = PassphraseMasterKey::open(dir.path(), PASSPHRASE).unwrap();
        let key = [7u8; 32];
        provider.save(&key).unwrap();
        let text = std::fs::read_to_string(key_file(dir.path())).unwrap();
        assert!(!text.contains(&b64(&key)));
        assert!(matches!(PassphraseMasterKey::open(dir.path(), "correct horse battery!"), Err(PortableError::WrongPassphrase)));
        assert_eq!(*PassphraseMasterKey::open(dir.path(), PASSPHRASE).unwrap().load().unwrap().unwrap(), key);
    }

    /// A new key file needs a passphrase of backup strength; an existing one takes whatever it was
    /// sealed with.
    #[test]
    fn a_new_key_file_needs_a_long_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(PassphraseMasterKey::open(dir.path(), "short"), Err(PortableError::WeakPassphrase)));
        assert!(is_new(dir.path()));
    }

    /// Lowered key derivation parameters would make the passphrase cheap to guess, raised ones
    /// would make the computer allocate gigabytes: both count as a damaged file, as does a changed
    /// sealed key.
    #[test]
    fn a_changed_key_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        PassphraseMasterKey::open(dir.path(), PASSPHRASE).unwrap().save(&[1u8; 32]).unwrap();
        let original = file_json(dir.path());
        let write = |value: &serde_json::Value| std::fs::write(key_file(dir.path()), value.to_string()).unwrap();

        for (field, value) in [
            ("memory_kib", serde_json::json!(8)),
            ("passes", serde_json::json!(1)),
            ("memory_kib", serde_json::json!(4 << 20)),
            ("kdf", serde_json::json!("pbkdf2")),
        ] {
            let mut changed = original.clone();
            changed[field] = value;
            write(&changed);
            assert!(matches!(PassphraseMasterKey::open(dir.path(), PASSPHRASE), Err(PortableError::Damaged)), "{field}");
        }

        let mut changed = original.clone();
        changed["sealed"] = serde_json::json!(b64(&[0u8; 48]));
        write(&changed);
        assert!(matches!(PassphraseMasterKey::open(dir.path(), PASSPHRASE), Err(PortableError::WrongPassphrase)));

        // A valid file padded past the limit is not read at all.
        std::fs::write(key_file(dir.path()), format!("{original}{}", " ".repeat(5000))).unwrap();
        assert!(matches!(PassphraseMasterKey::open(dir.path(), PASSPHRASE), Err(PortableError::Damaged)));
        std::fs::write(key_file(dir.path()), "not json").unwrap();
        assert!(matches!(PassphraseMasterKey::open(dir.path(), PASSPHRASE), Err(PortableError::Damaged)));
    }

    /// A deleted key file does not get replaced while sealed secrets exist: the vault reports the
    /// key as missing instead of making a new one that could never read them.
    #[test]
    fn a_lost_key_file_is_not_replaced_over_sealed_data() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("sshcraft.db")).unwrap();
        let vault = Vault::unlock(&store, Box::new(PassphraseMasterKey::open(dir.path(), PASSPHRASE).unwrap())).unwrap();
        vault.store_password(&store, sshcraft_model::model::PasswordId::new(), "s3cret", None).unwrap();
        std::fs::remove_file(key_file(dir.path())).unwrap();
        let provider = PassphraseMasterKey::open(dir.path(), PASSPHRASE).unwrap();
        assert!(matches!(Vault::unlock(&store, Box::new(provider)), Err(VaultError::MasterKeyMissing)));
        assert!(is_new(dir.path()));
    }
}
