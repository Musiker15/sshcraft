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

#[test]
fn sealed_secret_needs_the_same_label() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let sealed = v.seal_secret("device:a", b"secret bytes").unwrap();
    assert_eq!(
        &v.open_secret("device:a", &sealed).unwrap()[..],
        b"secret bytes"
    );
    assert!(v.open_secret("device:b", &sealed).is_err());
}

#[test]
fn received_key_keeps_its_id_and_location() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let id = sshcraft_model::model::KeyId::new();
    let server = sshcraft_model::model::ServerId::new();
    let info = v
        .store_key_with_id(&store, id, "synced", &key, Some(server))
        .unwrap();
    assert_eq!(info.id, id);
    assert_eq!(
        store.key_record(id).unwrap().unwrap().location,
        Some(server)
    );
    assert_eq!(openssh(&v.private_key(&store, id).unwrap()), openssh(&key));
}

use sshcraft_model::keytypes::{EcdsaCurveName, KeyKind, KeySpec};

fn encrypted_openssh(key: &PrivateKey, passphrase: &str) -> String {
    openssh(&key.encrypt(&mut rand::rng(), passphrase).unwrap())
}

fn rsa_bits(key: &PrivateKey) -> usize {
    let public = key.public_key();
    let n = public.key_data().rsa().unwrap().n();
    let bytes = n.as_positive_bytes().unwrap();
    bytes.len() * 8 - bytes[0].leading_zeros() as usize
}

#[test]
fn generates_every_supported_type() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let cases = [
        (KeySpec::Ed25519, "ssh-ed25519"),
        (
            KeySpec::Ecdsa {
                curve: EcdsaCurveName::Nistp256,
            },
            "ecdsa-sha2-nistp256",
        ),
        (
            KeySpec::Ecdsa {
                curve: EcdsaCurveName::Nistp384,
            },
            "ecdsa-sha2-nistp384",
        ),
        (
            KeySpec::Ecdsa {
                curve: EcdsaCurveName::Nistp521,
            },
            "ecdsa-sha2-nistp521",
        ),
    ];
    for (spec, algorithm) in cases {
        let info = v.generate(&store, "k", &spec, None).unwrap();
        assert_eq!(info.algorithm, algorithm);
        assert!(info.public_key.starts_with(algorithm));
        assert_eq!(info.kind, KeyKind::Vault);
        assert!(!info.protected);
        assert!(v.private_key(&store, info.id).is_ok());
    }
    for bits in [3072u32, 4096] {
        let info = v
            .generate(&store, "rsa", &KeySpec::Rsa { bits }, None)
            .unwrap();
        assert_eq!(info.algorithm, "ssh-rsa");
        let key = v.private_key(&store, info.id).unwrap();
        assert_eq!(rsa_bits(&key), bits as usize);
    }
}

#[test]
fn rejects_rsa_sizes_outside_3072_and_4096() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    for bits in [1024u32, 2048, 8192] {
        let result = v.generate(&store, "rsa", &KeySpec::Rsa { bits }, None);
        assert!(
            matches!(result, Err(VaultError::UnsupportedKeyType)),
            "{bits}"
        );
    }
    assert!(!store.has_key_records().unwrap());
}

#[test]
fn key_without_saved_passphrase_needs_it() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let info = v
        .import_key_protected(&store, "k", &encrypted_openssh(&key, "secret"), "secret")
        .unwrap();
    assert!(info.protected);
    assert!(matches!(
        v.private_key(&store, info.id),
        Err(VaultError::PassphraseRequired)
    ));
    assert!(matches!(
        v.unlock_key(&store, info.id, "wrong"),
        Err(VaultError::WrongPassphrase)
    ));
    let unlocked = v.unlock_key(&store, info.id, "secret").unwrap();
    assert_eq!(seed(&unlocked), seed(&key));
}

#[test]
fn protected_import_checks_the_passphrase() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let text = encrypted_openssh(&test_key(), "secret");
    assert!(matches!(
        v.import_key_protected(&store, "k", &text, "wrong"),
        Err(VaultError::WrongPassphrase)
    ));
    assert!(!store.has_key_records().unwrap());
}

#[test]
fn protected_key_never_lands_decrypted_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    let key = test_key();
    let passphrase = "correct horse battery";
    {
        let store = Store::open(&path).unwrap();
        let v = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        let info = v
            .import_key_protected(
                &store,
                "k",
                &encrypted_openssh(&key, passphrase),
                passphrase,
            )
            .unwrap();
        let stored = v.stored_openssh(&store, info.id).unwrap();
        let parsed = PrivateKey::from_openssh(stored.as_str()).unwrap();
        assert!(parsed.is_encrypted());
        assert!(!stored.contains(openssh(&key).as_str()));
    }
    let bytes = std::fs::read(&path).unwrap();
    assert!(!contains(&bytes, passphrase.as_bytes()));
    assert!(!contains(&bytes, &seed(&key)));
}

#[test]
fn saved_passphrase_stores_plain_key() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let info = v
        .import_key(
            &store,
            "k",
            &encrypted_openssh(&key, "secret"),
            Some("secret"),
        )
        .unwrap();
    assert!(!info.protected);
    assert_eq!(seed(&v.private_key(&store, info.id).unwrap()), seed(&key));
}

#[test]
fn remember_and_protect_switch_protection_and_keep_id_location_kind() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let id = sshcraft_model::model::KeyId::new();
    let server = sshcraft_model::model::ServerId::new();
    v.store_key_with_id(&store, id, "synced", &key, Some(server))
        .unwrap();

    let info = v.protect_key(&store, id, "pw").unwrap();
    assert!(info.protected);
    assert_eq!(info.id, id);
    assert_eq!(info.location, Some(server));
    assert_eq!(info.kind, KeyKind::Vault);
    assert!(matches!(
        v.private_key(&store, id),
        Err(VaultError::PassphraseRequired)
    ));

    assert!(matches!(
        v.remember_passphrase(&store, id, "nope"),
        Err(VaultError::WrongPassphrase)
    ));
    let info = v.remember_passphrase(&store, id, "pw").unwrap();
    assert!(!info.protected);
    assert_eq!(info.location, Some(server));
    assert_eq!(seed(&v.private_key(&store, id).unwrap()), seed(&key));
}

#[test]
fn generate_with_passphrase_is_protected() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let info = v
        .generate(&store, "k", &KeySpec::Ed25519, Some("pw"))
        .unwrap();
    assert!(info.protected);
    assert!(v.unlock_key(&store, info.id, "pw").is_ok());
}

#[test]
fn empty_passphrase_counts_as_none() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let info = v
        .generate(&store, "k", &KeySpec::Ed25519, Some(""))
        .unwrap();
    assert!(!info.protected);
    assert!(v.private_key(&store, info.id).is_ok());
    assert!(matches!(
        v.protect_key(&store, info.id, ""),
        Err(VaultError::WrongPassphrase)
    ));
}

#[test]
fn received_protected_text_stays_protected() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let id = sshcraft_model::model::KeyId::new();
    let info = v
        .store_openssh_with_id(&store, id, "k", &encrypted_openssh(&key, "pw"), None, None)
        .unwrap();
    assert!(info.protected);
    assert_eq!(info.id, id);
    assert!(v.unlock_key(&store, id, "pw").is_ok());
    assert!(matches!(
        v.store_openssh_with_id(&store, id, "k", "garbage", None, None),
        Err(VaultError::InvalidKey(_))
    ));
}

#[test]
fn device_key_is_device_kind_and_local() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let info = v.store_device_key(&store, "hello", &test_key()).unwrap();
    assert_eq!(info.kind, KeyKind::Device);
    assert_eq!(info.location, None);
    assert_eq!(
        store.key_record(info.id).unwrap().unwrap().kind,
        KeyKind::Device
    );
}

use russh::keys::ssh_key::certificate::{Builder, CertType};

/// A certificate for `key`, signed by a fresh CA, valid for user `deploy` from 0 to `until`.
fn certificate_for(key: &PrivateKey, cert_type: CertType, until: u64) -> (String, PrivateKey) {
    let ca = test_key();
    let mut builder = Builder::new_with_random_nonce(
        &mut rand::rng(),
        key.public_key().key_data().clone(),
        0,
        until,
    )
    .unwrap();
    builder.cert_type(cert_type).unwrap();
    builder.key_id("deploy-cert").unwrap();
    builder.valid_principal("deploy").unwrap();
    let cert = builder.sign(&ca).unwrap();
    (cert.to_openssh().unwrap(), ca)
}

#[test]
fn attaches_matching_user_certificate() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let info = v.import_key(&store, "k", &openssh(&key), None).unwrap();
    let (text, ca) = certificate_for(&key, CertType::User, u64::MAX);
    let info = v
        .attach_certificate(&store, info.id, &format!("  {text}\n"))
        .unwrap();
    let cert = info.certificate.expect("certificate info");
    assert_eq!(cert.key_id, "deploy-cert");
    assert_eq!(cert.principals, vec!["deploy".to_owned()]);
    assert_eq!(cert.valid_after, 0);
    assert_eq!(cert.valid_before, u64::MAX);
    assert_eq!(
        cert.ca_fingerprint,
        ca.public_key().fingerprint(HashAlg::Sha256).to_string()
    );
    // It is kept and listed.
    let parsed = certificate(&store, info.id).unwrap().expect("stored");
    assert_eq!(parsed.key_id(), "deploy-cert");
    let listed = list_keys(&store).unwrap();
    assert!(listed[0].certificate.is_some());
}

#[test]
fn rejects_certificate_for_other_key() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let info = v
        .import_key(&store, "k", &openssh(&test_key()), None)
        .unwrap();
    let (text, _) = certificate_for(&test_key(), CertType::User, u64::MAX);
    assert!(matches!(
        v.attach_certificate(&store, info.id, &text),
        Err(VaultError::CertificateMismatch)
    ));
    assert_eq!(
        store.key_record(info.id).unwrap().unwrap().certificate,
        None
    );
}

#[test]
fn rejects_host_certificate() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let info = v.import_key(&store, "k", &openssh(&key), None).unwrap();
    let (text, _) = certificate_for(&key, CertType::Host, u64::MAX);
    assert!(matches!(
        v.attach_certificate(&store, info.id, &text),
        Err(VaultError::InvalidCertificate(_))
    ));
}

#[test]
fn rejects_garbage_certificate() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let info = v
        .import_key(&store, "k", &openssh(&test_key()), None)
        .unwrap();
    for text in [
        "",
        "not a certificate",
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIB",
    ] {
        assert!(matches!(
            v.attach_certificate(&store, info.id, text),
            Err(VaultError::InvalidCertificate(_))
        ));
    }
}

#[test]
fn certificate_survives_protection_change_and_detach_removes_it() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let info = v.import_key(&store, "k", &openssh(&key), None).unwrap();
    let (text, _) = certificate_for(&key, CertType::User, u64::MAX);
    v.attach_certificate(&store, info.id, &text).unwrap();
    let info = v.protect_key(&store, info.id, "pw").unwrap();
    assert!(info.certificate.is_some());
    let info = v.detach_certificate(&store, info.id).unwrap();
    assert!(info.certificate.is_none());
    assert!(certificate(&store, info.id).unwrap().is_none());
}

#[test]
fn received_certificate_for_other_key_is_dropped() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let (theirs, _) = certificate_for(&test_key(), CertType::User, u64::MAX);
    let info = v
        .store_openssh_with_id(
            &store,
            sshcraft_model::model::KeyId::new(),
            "k",
            &openssh(&key),
            Some(&theirs),
            None,
        )
        .unwrap();
    assert!(info.certificate.is_none());
    let (mine, _) = certificate_for(&key, CertType::User, u64::MAX);
    let info = v
        .store_openssh_with_id(&store, info.id, "k", &openssh(&key), Some(&mine), None)
        .unwrap();
    assert!(info.certificate.is_some());
}

fn sk_key() -> PrivateKey {
    use russh::keys::ssh_key::private::{self, KeypairData};
    use russh::keys::ssh_key::public::{self, Ed25519PublicKey};
    let public = public::SkEd25519::new(Ed25519PublicKey([7u8; 32]), "ssh:");
    let pair = private::SkEd25519::new(public, 0x05, vec![1, 2, 3]).unwrap();
    PrivateKey::new(KeypairData::SkEd25519(pair), "token").unwrap()
}

#[test]
fn security_key_is_never_stored_protected() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    // ssh-keygen asks for a passphrase on sk keys too; the file holds only a key handle.
    let info = v
        .import_key_protected(&store, "token", &encrypted_openssh(&sk_key(), "pw"), "pw")
        .unwrap();
    assert!(!info.protected);
    assert!(v.private_key(&store, info.id).is_ok());
    assert!(matches!(
        v.protect_key(&store, info.id, "pw"),
        Err(VaultError::UnsupportedKeyType)
    ));
    let device = v.store_device_key(&store, "hello", &sk_key()).unwrap();
    assert!(matches!(
        v.protect_key(&store, device.id, "pw"),
        Err(VaultError::UnsupportedKeyType)
    ));
}

#[test]
fn certificate_with_broken_signature_is_refused() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let key = test_key();
    let info = v.import_key(&store, "k", &openssh(&key), None).unwrap();
    let (text, _) = certificate_for(&key, CertType::User, u64::MAX);
    // Change one byte of the CA signature at the end of the blob.
    let mut parts: Vec<String> = text.split(' ').map(str::to_owned).collect();
    let mut blob = data_encoding::BASE64.decode(parts[1].as_bytes()).unwrap();
    let last = blob.len() - 1;
    blob[last] ^= 0x01;
    parts[1] = data_encoding::BASE64.encode(&blob);
    let tampered = parts.join(" ");
    assert!(matches!(
        v.attach_certificate(&store, info.id, &tampered),
        Err(VaultError::InvalidCertificate(_))
    ));
}

/// The rebuild finds the master key the Tauri version stored: same keyring service name.
#[test]
fn keyring_service_is_unchanged() {
    assert_eq!(KEYRING_SERVICE, "de.musiker15.sshcraft");
}
