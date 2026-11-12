use super::*;
use crate::store::Store;
use russh::keys::{Algorithm, HashAlg, PrivateKey, ssh_key::LineEnding};

fn test_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

fn openssh(key: &PrivateKey) -> String {
    key.to_openssh(LineEnding::LF).unwrap().to_string()
}

fn seed(key: &PrivateKey) -> Vec<u8> {
    key.key_data()
        .ed25519()
        .unwrap()
        .private
        .to_bytes()
        .to_vec()
}

fn vault(store: &Store) -> Vault {
    Vault::unlock(store, Box::new(MemoryMasterKey::default())).unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn import_then_load_returns_same_key() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let info = v.import_key(&store, "k", &openssh(&key), None).unwrap();
    let loaded = v.private_key(&store, info.id).unwrap();
    assert_eq!(
        loaded.fingerprint(HashAlg::Sha256).to_string(),
        info.fingerprint
    );
    assert_eq!(seed(&loaded), seed(&key));
}

#[test]
fn generate_ed25519_has_sha256_fingerprint() {
    let store = Store::open_in_memory().unwrap();
    let info = vault(&store).generate_ed25519(&store, "new").unwrap();
    assert!(info.fingerprint.starts_with("SHA256:"));
    assert_eq!(info.algorithm, "ssh-ed25519");
    assert!(info.public_key.starts_with("ssh-ed25519 "));
}

#[test]
fn import_accepts_crlf_and_trailing_whitespace() {
    let store = Store::open_in_memory().unwrap();
    let text = openssh(&test_key()).replace('\n', "\r\n") + "\r\n\n  ";
    assert!(vault(&store).import_key(&store, "k", &text, None).is_ok());
}

#[test]
fn import_with_passphrase() {
    let store = Store::open_in_memory().unwrap();
    let key = test_key();
    let encrypted = key.encrypt(&mut rand::rng(), "secret").unwrap();
    let info = vault(&store)
        .import_key(&store, "k", &openssh(&encrypted), Some("secret"))
        .unwrap();
    assert_eq!(
        info.fingerprint,
        key.fingerprint(HashAlg::Sha256).to_string()
    );
}

#[test]
fn import_wrong_passphrase_is_error() {
    let store = Store::open_in_memory().unwrap();
    let encrypted = test_key().encrypt(&mut rand::rng(), "secret").unwrap();
    let result = vault(&store).import_key(&store, "k", &openssh(&encrypted), Some("wrong"));
    assert!(matches!(result, Err(VaultError::WrongPassphrase)));
}

#[test]
fn import_garbage_is_error() {
    let store = Store::open_in_memory().unwrap();
    let result = vault(&store).import_key(&store, "k", "hello", None);
    assert!(matches!(result, Err(VaultError::InvalidKey(_))));
}

#[test]
fn database_file_never_contains_plaintext_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sshcraft.db");
    let key = test_key();
    {
        let store = Store::open(&path).unwrap();
        vault(&store)
            .import_key(&store, "k", &openssh(&key), None)
            .unwrap();
    }
    let bytes = std::fs::read(&path).unwrap();
    assert!(!contains(&bytes, b"OPENSSH PRIVATE KEY"));
    assert!(!contains(&bytes, &seed(&key)));
}

#[test]
fn sealed_secret_is_bound_to_key_id() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let a = v
        .import_key(&store, "a", &openssh(&test_key()), None)
        .unwrap();
    let b = v
        .import_key(&store, "b", &openssh(&test_key()), None)
        .unwrap();
    let mut rec_a = store.key_record(a.id).unwrap().unwrap();
    let rec_b = store.key_record(b.id).unwrap().unwrap();
    rec_a.sealed_secret = rec_b.sealed_secret;
    store.put_key_record(&rec_a).unwrap();
    assert!(matches!(
        v.private_key(&store, a.id),
        Err(VaultError::Corrupt)
    ));
}

#[derive(Clone, Default)]
struct SharedKey(std::sync::Arc<std::sync::Mutex<Option<[u8; 32]>>>);

impl MasterKeyProvider for SharedKey {
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>, VaultError> {
        Ok(self.0.lock().unwrap().map(Zeroizing::new))
    }
    fn save(&self, key: &[u8; 32]) -> Result<(), VaultError> {
        *self.0.lock().unwrap() = Some(*key);
        Ok(())
    }
}

#[test]
fn unlock_refuses_new_master_key_when_secrets_exist() {
    let store = Store::open_in_memory().unwrap();
    Vault::unlock(&store, Box::new(SharedKey::default()))
        .unwrap()
        .import_key(&store, "k", &openssh(&test_key()), None)
        .unwrap();
    let empty = SharedKey::default();
    let result = Vault::unlock(&store, Box::new(empty.clone()));
    assert!(matches!(result, Err(VaultError::MasterKeyMissing)));
    assert!(empty.0.lock().unwrap().is_none());
}

#[test]
fn unlock_reuses_saved_master_key() {
    let store = Store::open_in_memory().unwrap();
    let shared = SharedKey::default();
    let info = Vault::unlock(&store, Box::new(shared.clone()))
        .unwrap()
        .import_key(&store, "k", &openssh(&test_key()), None)
        .unwrap();
    let again = Vault::unlock(&store, Box::new(shared)).unwrap();
    assert!(again.private_key(&store, info.id).is_ok());
}

#[test]
fn key_info_json_has_no_secret() {
    let store = Store::open_in_memory().unwrap();
    let key = test_key();
    let info = vault(&store)
        .import_key(&store, "k", &openssh(&key), None)
        .unwrap();
    let json = serde_json::to_string(&info).unwrap();
    assert!(!json.contains("PRIVATE"));
    let seed_hex: String = seed(&key).iter().map(|b| format!("{b:02x}")).collect();
    assert!(!json.contains(&seed_hex[..16]));
    for field in ["secret", "private", "sealed"] {
        assert!(!json.contains(field), "KeyInfo must not expose {field}");
    }
}

#[test]
fn list_and_delete_keys() {
    let store = Store::open_in_memory().unwrap();
    let info = vault(&store).generate_ed25519(&store, "k").unwrap();
    assert_eq!(list_keys(&store).unwrap(), vec![info.clone()]);
    delete_key(&store, info.id).unwrap();
    assert!(list_keys(&store).unwrap().is_empty());
}
