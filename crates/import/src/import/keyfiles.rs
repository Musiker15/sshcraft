//! Private key files in a folder (`~/.ssh`): OpenSSH, PEM and PuTTY (`.ppk`).

use std::path::{Path, PathBuf};

use russh::keys::ssh_key::{HashAlg, PrivateKey};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use sshcraft_store::store::Store;
use sshcraft_store::vault::{KeyInfo, Vault, VaultError, parse_private_key};

/// Key files are small; anything bigger is not read.
pub const MAX_KEY_FILE: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyFormat {
    OpenSsh,
    Pem,
    Ppk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyFile {
    pub path: PathBuf,
    pub name: String,
    pub format: KeyFormat,
    pub encrypted: bool,
    /// Known without the passphrase for OpenSSH keys and unencrypted ones.
    pub algorithm: Option<String>,
    pub fingerprint: Option<String>,
}

fn skipped_name(name: &str) -> bool {
    name.ends_with(".pub")
        || name.starts_with("known_hosts")
        || name == "config"
        || name == "authorized_keys"
        || name == "authorized_keys2"
}

fn read_small(path: &Path) -> Option<Zeroizing<String>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_KEY_FILE {
        return None;
    }
    std::fs::read_to_string(path).ok().map(Zeroizing::new)
}

fn format_of(text: &str) -> Option<KeyFormat> {
    let text = text.trim_start();
    if text.starts_with("PuTTY-User-Key-File-") {
        Some(KeyFormat::Ppk)
    } else if text.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----") {
        Some(KeyFormat::OpenSsh)
    } else if text.starts_with("-----BEGIN") && text.contains("PRIVATE KEY-----") {
        Some(KeyFormat::Pem)
    } else {
        None
    }
}

fn describe(path: PathBuf, name: String, text: &str) -> Option<KeyFile> {
    let format = format_of(text)?;
    let public = |key: &PrivateKey| {
        (
            Some(key.algorithm().to_string()),
            Some(key.public_key().fingerprint(HashAlg::Sha256).to_string()),
        )
    };
    let (encrypted, (algorithm, fingerprint)) = match format {
        KeyFormat::OpenSsh => {
            let key = PrivateKey::from_openssh(text.trim()).ok()?;
            (key.is_encrypted(), public(&key))
        }
        KeyFormat::Pem | KeyFormat::Ppk => {
            match russh::keys::decode_secret_key(text, None) {
                Ok(key) => (false, public(&key)),
                // Only an encrypted key fails here; anything else is not a key.
                Err(_) if text.contains("ENCRYPTED") || text.contains("Encryption: aes") => {
                    (true, (None, None))
                }
                Err(_) => return None,
            }
        }
    };
    Some(KeyFile {
        path,
        name,
        format,
        encrypted,
        algorithm,
        fingerprint,
    })
}

/// The private key files directly in `dir`, sorted by name; public keys, `known_hosts`,
/// `config` and big files are not read.
pub fn scan(dir: &Path) -> Vec<KeyFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<KeyFile> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if skipped_name(&name) {
                return None;
            }
            let path = entry.path();
            let text = read_small(&path)?;
            describe(path, name, &text)
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Imports one key file into the vault, named after the file.
pub fn import(
    vault: &Vault,
    store: &Store,
    path: &Path,
    passphrase: Option<&str>,
) -> Result<KeyInfo, VaultError> {
    let text = read_small(path).ok_or_else(|| VaultError::InvalidKey("unreadable".into()))?;
    let key = parse_private_key(&text, passphrase)?;
    let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
    if let Some(known) = store
        .list_key_records()
        .map_err(|e| VaultError::InvalidKey(e.to_string()))?
        .into_iter()
        .find(|r| r.fingerprint == fingerprint)
    {
        let id = known.id;
        if known.certificate.is_none() {
            return Ok(attach_sibling_certificate(vault, store, path, id)
                .unwrap_or_else(|| KeyInfo::from(known)));
        }
        return Ok(KeyInfo::from(known));
    }
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "key".into());
    let info = vault.store_key(store, &name, &key)?;
    Ok(attach_sibling_certificate(vault, store, path, info.id).unwrap_or(info))
}

/// Attaches `<key file>-cert.pub` if it lies next to the key and fits it, as OpenSSH would use
/// it. A certificate that does not fit is left out; the key itself is imported anyway.
fn attach_sibling_certificate(
    vault: &Vault,
    store: &Store,
    path: &Path,
    id: sshcraft_model::model::KeyId,
) -> Option<KeyInfo> {
    let mut sibling = path.as_os_str().to_owned();
    sibling.push("-cert.pub");
    let text = read_small(Path::new(&sibling))?;
    vault.attach_certificate(store, id, &text).ok()
}
