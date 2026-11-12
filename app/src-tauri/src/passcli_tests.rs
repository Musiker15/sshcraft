use std::sync::Mutex;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, HashAlg, PrivateKey};
use stollen_core::store::Store;
use stollen_core::vault::{MemoryMasterKey, Vault};
use zeroize::Zeroizing;

use super::*;

const MARKER: &str = "MARKER-SECRET-4711";

const LIST_JSON: &str = r#"{
  "items": [
    {"id": "i1", "share_id": "s1", "vault_id": "v", "title": "server-root", "item_type": "ssh_key"},
    {"id": "i2", "share_id": "s1", "vault_id": "v", "title": "notes", "item_type": "custom"},
    {"id": "i3", "share_id": "s2", "vault_id": "v", "title": "deploy", "item_type": "ssh_key"}
  ],
  "folders": []
}"#;

fn view_json(private_key: &str) -> String {
    serde_json::json!({
        "item": {
            "id": "i1",
            "share_id": "s1",
            "content": {
                "title": "server-root",
                "content": { "SshKey": { "private_key": private_key, "public_key": "ssh-ed25519 AAAA" } }
            }
        }
    })
    .to_string()
}

struct FakePassCli {
    status: PassCliStatus,
    private_key: Mutex<String>,
}

impl FakePassCli {
    fn with_key(key: &str) -> Self {
        Self {
            status: PassCliStatus::Ready,
            private_key: Mutex::new(key.to_owned()),
        }
    }
}

impl PassCli for FakePassCli {
    fn status(&self) -> PassCliStatus {
        self.status
    }

    fn list_ssh_keys(&self) -> Result<Vec<PassKey>, PassCliError> {
        ensure_ready(self.status)?;
        parse_key_list(LIST_JSON)
    }

    fn private_key(
        &self,
        _share_id: &str,
        _item_id: &str,
    ) -> Result<Zeroizing<String>, PassCliError> {
        ensure_ready(self.status)?;
        parse_private_key(&view_json(&self.private_key.lock().unwrap()))
    }
}

fn test_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

fn vault_and_store() -> (Vault, Mutex<Store>) {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    (vault, Mutex::new(store))
}

#[test]
fn key_list_keeps_only_ssh_keys() {
    let keys = parse_key_list(LIST_JSON).unwrap();
    let titles: Vec<_> = keys.iter().map(|k| k.title.as_str()).collect();
    assert_eq!(titles, vec!["server-root", "deploy"]);
    assert_eq!(keys[1].share_id, "s2");
    assert_eq!(keys[1].item_id, "i3");
}

#[test]
fn private_key_is_read_from_ssh_key_content() {
    let key = parse_private_key(&view_json("PEM")).unwrap();
    assert_eq!(key.as_str(), "PEM");
}

#[test]
fn item_without_private_key_is_an_error() {
    let json = r#"{"item":{"content":{"content":{"Login":{"username":"x"}}}}}"#;
    assert!(matches!(
        parse_private_key(json),
        Err(PassCliError::NoPrivateKey)
    ));
    assert!(matches!(
        parse_private_key(&view_json("")),
        Err(PassCliError::NoPrivateKey)
    ));
}

#[test]
fn import_stores_the_key_encrypted() {
    let (vault, store) = vault_and_store();
    let key = test_key();
    let cli = FakePassCli::with_key(&key.to_openssh(LineEnding::LF).unwrap());
    let info = import_from_pass_cli(&cli, &vault, &store, "s1", "i1", "server-root").unwrap();
    assert_eq!(
        info.fingerprint,
        key.fingerprint(HashAlg::Sha256).to_string()
    );
    assert!(vault.private_key(&store.lock().unwrap(), info.id).is_ok());
}

#[test]
fn passphrase_protected_key_reports_encrypted() {
    let (vault, store) = vault_and_store();
    let encrypted = test_key().encrypt(&mut rand::rng(), "pw").unwrap();
    let cli = FakePassCli::with_key(&encrypted.to_openssh(LineEnding::LF).unwrap());
    let result = import_from_pass_cli(&cli, &vault, &store, "s1", "i1", "x");
    assert!(matches!(result, Err(PassCliError::Encrypted)));
}

#[test]
fn errors_never_contain_the_private_key() {
    let (vault, store) = vault_and_store();
    let broken = format!(
        "-----BEGIN OPENSSH PRIVATE KEY-----\n{MARKER}\n-----END OPENSSH PRIVATE KEY-----\n"
    );
    let cli = FakePassCli::with_key(&broken);
    let err = import_from_pass_cli(&cli, &vault, &store, "s1", "i1", "x").unwrap_err();
    assert!(!err.to_string().contains(MARKER), "{err}");
}

#[test]
fn process_failure_never_includes_stdout() {
    let stdout = format!("{{\"item\":{{\"private_key\":\"{MARKER}\"}}}}");
    let err = failure(
        Some(1),
        stdout.as_bytes(),
        b"Error: something went wrong\nmore detail\n",
    );
    let text = err.to_string();
    assert!(!text.contains(MARKER), "{text}");
    assert!(text.contains("something went wrong"));
    assert!(!text.contains("more detail"));
}

#[test]
fn logged_out_cli_reports_logged_out() {
    let cli = FakePassCli {
        status: PassCliStatus::LoggedOut,
        private_key: Mutex::new(String::new()),
    };
    assert!(matches!(cli.list_ssh_keys(), Err(PassCliError::LoggedOut)));
}
