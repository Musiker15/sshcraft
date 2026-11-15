//! An encrypted backup file of everything on this device that is the user's own: groups, hosts,
//! identities, keys (as their OpenSSH text), passwords, snippets and host keys. It is sealed with
//! a key made from a passphrase (Argon2id), so it opens on any device, also after the system's
//! keyring is gone. Keys bound to this device (Windows Hello) cannot leave it and are left out.
//!
//! The file is text: a first line naming the format, a second with the key derivation
//! parameters (authenticated, so they cannot be lowered unnoticed), then the ciphertext in
//! Base64.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sshcraft_model::keytypes::KeyKind;
use sshcraft_model::model::{Group, Host, Identity, KeyId, PasswordId, ServerId};
use zeroize::Zeroizing;

use crate::snippets::{self, Snippet};
use crate::store::{HostKey, Store, StoreError};
use crate::vault::{Vault, VaultError};

const MAGIC: &str = "sshcraft-backup-1";
/// The shortest passphrase taken.
pub const MIN_PASSPHRASE: usize = 12;
/// The largest backup file read.
pub const MAX_FILE: usize = 256 * 1024 * 1024;
/// Argon2id: 64 MiB, three passes, one lane.
const KDF_MEMORY_KIB: u32 = 64 * 1024;
const KDF_PASSES: u32 = 3;
/// The most a file may ask for, so a crafted header cannot make the device allocate gigabytes.
const KDF_MAX_MEMORY_KIB: u32 = 1024 * 1024;
const KDF_MAX_PASSES: u32 = 16;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("the passphrase needs at least {MIN_PASSPHRASE} characters")]
    WeakPassphrase,
    #[error("this is not an SSHCraft backup file")]
    NotABackup,
    #[error("the passphrase is wrong, or the file was changed")]
    WrongPassphraseOrChanged,
    #[error("the backup file is too large")]
    TooLarge,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Vault(#[from] VaultError),
}

#[derive(Serialize, Deserialize)]
struct Header {
    kdf: String,
    memory_kib: u32,
    passes: u32,
    salt: String,
    nonce: String,
}

#[derive(Serialize, Deserialize)]
struct BackupKey {
    id: KeyId,
    name: String,
    /// The OpenSSH text as stored; still under its own passphrase when the key has one.
    openssh: Zeroizing<String>,
    certificate: Option<String>,
    location: Option<ServerId>,
}

#[derive(Serialize, Deserialize)]
struct BackupPassword {
    id: PasswordId,
    secret: Zeroizing<String>,
    location: Option<ServerId>,
}

#[derive(Serialize, Deserialize, Default)]
struct Content {
    created_at: i64,
    groups: Vec<Group>,
    hosts: Vec<Host>,
    identities: Vec<Identity>,
    keys: Vec<BackupKey>,
    passwords: Vec<BackupPassword>,
    snippets: Vec<Snippet>,
    host_keys: Vec<HostKey>,
}

/// What a backup held or a restore added, per kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub groups: usize,
    pub hosts: usize,
    pub identities: usize,
    pub keys: usize,
    pub passwords: usize,
    pub snippets: usize,
    pub host_keys: usize,
    /// Keys bound to this device, which a backup cannot hold.
    pub left_out: usize,
}

fn derive(passphrase: &str, salt: &[u8], memory_kib: u32, passes: u32) -> Result<Zeroizing<[u8; 32]>, BackupError> {
    let params = Params::new(memory_kib, passes, 1, Some(32)).map_err(|_| BackupError::NotABackup)?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
        .map_err(|_| BackupError::NotABackup)?;
    Ok(key)
}

fn b64(bytes: &[u8]) -> String {
    data_encoding::BASE64.encode(bytes)
}

fn unb64(text: &str) -> Result<Vec<u8>, BackupError> {
    data_encoding::BASE64.decode(text.trim().as_bytes()).map_err(|_| BackupError::NotABackup)
}

/// Everything into one backup text, sealed with `passphrase`.
pub fn create(store: &Store, vault: &Vault, passphrase: &str, now: i64) -> Result<(String, Counts), BackupError> {
    if passphrase.chars().count() < MIN_PASSPHRASE {
        return Err(BackupError::WeakPassphrase);
    }
    let mut content = Content {
        created_at: now,
        groups: store.list_groups()?,
        hosts: store.list_hosts()?,
        identities: store.list_identities()?,
        host_keys: store.all_host_keys()?,
        snippets: snippets::list(vault, store).map_err(|e| BackupError::Store(StoreError::Malformed(e.to_string())))?,
        ..Content::default()
    };
    let mut counts = Counts::default();
    for record in store.list_key_records()? {
        if record.kind == KeyKind::Device {
            counts.left_out += 1;
            continue;
        }
        content.keys.push(BackupKey {
            id: record.id,
            name: record.name.clone(),
            openssh: vault.stored_openssh(store, record.id)?,
            certificate: record.certificate.clone(),
            location: record.location,
        });
    }
    for id in store.list_password_ids()? {
        let location = store.password_record(id)?.and_then(|(_, l)| l);
        content.passwords.push(BackupPassword { id, secret: vault.password(store, id)?, location });
    }
    counts.groups = content.groups.len();
    counts.hosts = content.hosts.len();
    counts.identities = content.identities.len();
    counts.keys = content.keys.len();
    counts.passwords = content.passwords.len();
    counts.snippets = content.snippets.len();
    counts.host_keys = content.host_keys.len();

    let plain = Zeroizing::new(serde_json::to_vec(&content).map_err(|e| StoreError::Malformed(e.to_string()))?);
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    rand::rng().fill(&mut salt[..]);
    rand::rng().fill(&mut nonce[..]);
    let header = Header { kdf: "argon2id".into(), memory_kib: KDF_MEMORY_KIB, passes: KDF_PASSES, salt: b64(&salt), nonce: b64(&nonce) };
    let header_line = serde_json::to_string(&header).map_err(|e| StoreError::Malformed(e.to_string()))?;
    let key = derive(passphrase, &salt, header.memory_kib, header.passes)?;
    let aad = format!("{MAGIC}\n{header_line}");
    let cipher = XChaCha20Poly1305::new((&*key).into());
    let sealed = cipher.encrypt(&XNonce::from(nonce), Payload { msg: &plain, aad: aad.as_bytes() }).map_err(|_| BackupError::NotABackup)?;
    Ok((format!("{aad}\n{}\n", b64(&sealed)), counts))
}

fn open(text: &str, passphrase: &str) -> Result<Content, BackupError> {
    if text.len() > MAX_FILE {
        return Err(BackupError::TooLarge);
    }
    let mut lines = text.lines();
    let (Some(magic), Some(header_line), Some(body)) = (lines.next(), lines.next(), lines.next()) else {
        return Err(BackupError::NotABackup);
    };
    if magic.trim() != MAGIC {
        return Err(BackupError::NotABackup);
    }
    let header: Header = serde_json::from_str(header_line).map_err(|_| BackupError::NotABackup)?;
    if header.kdf != "argon2id" || header.memory_kib > KDF_MAX_MEMORY_KIB || header.passes > KDF_MAX_PASSES {
        return Err(BackupError::NotABackup);
    }
    let salt = unb64(&header.salt)?;
    let nonce: [u8; 24] = unb64(&header.nonce)?.try_into().map_err(|_| BackupError::NotABackup)?;
    let key = derive(passphrase, &salt, header.memory_kib, header.passes)?;
    let aad = format!("{MAGIC}\n{header_line}");
    let cipher = XChaCha20Poly1305::new((&*key).into());
    let plain = cipher
        .decrypt(&XNonce::from(nonce), Payload { msg: &unb64(body)?, aad: aad.as_bytes() })
        .map(Zeroizing::new)
        .map_err(|_| BackupError::WrongPassphraseOrChanged)?;
    serde_json::from_slice(&plain).map_err(|_| BackupError::NotABackup)
}

/// Adds from a backup what is missing on this device; nothing that is here is changed (local
/// data stays authoritative). Items keep their sync server only when this device knows it.
/// The whole file is checked first: a wrong passphrase or a changed file adds nothing.
pub fn restore_missing(store: &Store, vault: &Vault, text: &str, passphrase: &str) -> Result<Counts, BackupError> {
    let content = open(text, passphrase)?;
    let servers: Vec<ServerId> = store.servers()?.into_iter().map(|s| s.id).collect();
    let place = |location: Option<ServerId>| location.filter(|s| servers.contains(s));
    let mut counts = Counts::default();

    let have_groups: Vec<_> = store.list_groups()?.into_iter().map(|g| g.id).collect();
    // Parents first: a group whose parent is neither here nor in the file goes to the top.
    let mut pending: Vec<Group> = content.groups.into_iter().filter(|g| !have_groups.contains(&g.id)).collect();
    let mut placed: Vec<_> = have_groups.clone();
    while !pending.is_empty() {
        let before = pending.len();
        let mut rest = Vec::new();
        for mut group in pending {
            let parent_ready = group.parent_id.is_none_or(|p| placed.contains(&p));
            if parent_ready {
                group.location = place(group.location);
                store.upsert_group(&group)?;
                placed.push(group.id);
                counts.groups += 1;
            } else {
                rest.push(group);
            }
        }
        if rest.len() == before {
            // Parents that are nowhere: those groups go to the top.
            for mut group in rest.drain(..) {
                group.parent_id = None;
                group.location = place(group.location);
                store.upsert_group(&group)?;
                placed.push(group.id);
                counts.groups += 1;
            }
        }
        pending = rest;
    }

    let have_passwords = store.list_password_ids()?;
    for p in content.passwords.iter().filter(|p| !have_passwords.contains(&p.id)) {
        vault.store_password(store, p.id, &p.secret, place(p.location))?;
        counts.passwords += 1;
    }
    for k in content.keys.iter().filter(|k| store.key_record(k.id).ok().flatten().is_none()) {
        vault.store_openssh_with_id(store, k.id, &k.name, &k.openssh, k.certificate.as_deref(), place(k.location))?;
        counts.keys += 1;
    }
    let have_identities: Vec<_> = store.list_identities()?.into_iter().map(|i| i.id).collect();
    for mut identity in content.identities.into_iter().filter(|i| !have_identities.contains(&i.id)) {
        identity.location = place(identity.location);
        store.upsert_identity(&identity)?;
        counts.identities += 1;
    }
    let have_hosts: Vec<_> = store.list_hosts()?.into_iter().map(|h| h.id).collect();
    for mut host in content.hosts.into_iter().filter(|h| !have_hosts.contains(&h.id)) {
        if host.group_id.is_some_and(|g| !placed.contains(&g)) {
            host.group_id = None;
        }
        host.location = place(host.location);
        store.upsert_host(&host)?;
        counts.hosts += 1;
    }
    let have_snippets: Vec<_> =
        snippets::list(vault, store).map_err(|e| BackupError::Store(StoreError::Malformed(e.to_string())))?.into_iter().map(|s| s.id).collect();
    for snippet in content.snippets.iter().filter(|s| !have_snippets.contains(&s.id)) {
        snippets::save(vault, store, snippet).map_err(|e| BackupError::Store(StoreError::Malformed(e.to_string())))?;
        counts.snippets += 1;
    }
    // Host keys only for servers this device knows no key of yet (as with known_hosts): a
    // server it knows keeps the keys it confirmed.
    let known: Vec<(String, u16)> = store.all_host_keys()?.into_iter().map(|k| (k.address, k.port)).collect();
    for key in content.host_keys.iter().filter(|k| !known.contains(&(k.address.clone(), k.port))) {
        store.add_host_key(key)?;
        counts.host_keys += 1;
    }
    Ok(counts)
}

#[cfg(test)]
#[path = "backup_tests.rs"]
mod tests;
