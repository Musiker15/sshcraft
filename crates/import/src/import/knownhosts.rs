//! `~/.ssh/known_hosts`: host keys to trust, and hosts to create.

use russh::keys::ssh_key::{HashAlg, PublicKey};
use serde::{Deserialize, Serialize};

use super::{ImportCandidate, MAX_NAME, clean};
use sshcraft_model::model::{ConnectionSettings, DEFAULT_PORT};
use sshcraft_store::store::{Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownEntry {
    pub address: String,
    pub port: u16,
    /// In the same form the app stores when connecting (`SHA256:…`).
    pub fingerprint: String,
    /// The key itself, so it can be exported again.
    #[serde(default)]
    pub algorithm: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHostsImport {
    pub entries: Vec<KnownEntry>,
    /// One host per address and port.
    pub candidates: Vec<ImportCandidate>,
    /// Lines that could not be used, by number and reason.
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownOutcome {
    pub added: usize,
    pub same: usize,
    /// A different key is already stored for these; it stays.
    pub conflicts: Vec<(String, u16)>,
}

/// One host name field entry: `host` or `[host]:port`.
fn host_and_port(name: &str) -> Option<(String, u16)> {
    if let Some(rest) = name.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        let port: u16 = port.parse().ok().filter(|p| *p > 0)?;
        return (!host.is_empty()).then(|| (host.to_owned(), port));
    }
    (!name.is_empty() && !name.contains(['[', ']'])).then(|| (name.to_owned(), DEFAULT_PORT))
}

pub fn parse(text: &str) -> KnownHostsImport {
    let mut out = KnownHostsImport::default();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('@') {
            out.skipped.push(format!("line {number}: certificate authority or revoked key"));
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(names), Some(kind), Some(data)) = (fields.next(), fields.next(), fields.next()) else {
            out.skipped.push(format!("line {number}: incomplete"));
            continue;
        };
        if names.starts_with('|') {
            out.skipped.push(format!("line {number}: hashed host name"));
            continue;
        }
        let Ok(key) = PublicKey::from_openssh(&format!("{kind} {data}")) else {
            out.skipped.push(format!("line {number}: unreadable key"));
            continue;
        };
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let mut usable = Vec::new();
        for name in names.split(',') {
            if name.starts_with('!') || name.contains(['*', '?']) {
                continue;
            }
            match host_and_port(name) {
                Some((host, port)) if host.chars().count() <= MAX_NAME => usable.push((host, port)),
                _ => {}
            }
        }
        if usable.is_empty() {
            out.skipped.push(format!("line {number}: no usable host name"));
            continue;
        }
        // One host per line: further names of the same line are the same machine.
        let mut first = true;
        for (address, port) in usable {
            let fresh = std::mem::take(&mut first);
            if fresh
                && !out
                    .candidates
                    .iter()
                    .any(|c| c.settings.address.as_deref() == Some(address.as_str()) && c.settings.port.unwrap_or(DEFAULT_PORT) == port)
            {
                let name = if port == DEFAULT_PORT { address.clone() } else { format!("{address}:{port}") };
                out.candidates.push(ImportCandidate {
                    name: clean(&name),
                    settings: ConnectionSettings {
                        address: Some(address.clone()),
                        port: (port != DEFAULT_PORT).then_some(port),
                        ..Default::default()
                    },
                    ..ImportCandidate::default()
                });
            }
            out.entries.push(KnownEntry {
                address,
                port,
                fingerprint: fingerprint.clone(),
                algorithm: Some(kind.to_owned()),
                public_key: Some(data.to_owned()),
            });
        }
    }
    out
}

/// Stores the host keys. A server new to SSHCraft takes all its keys from the file (one per
/// algorithm). For a server SSHCraft already knows, a key it does not know is not taken over:
/// the stored ones were confirmed when connecting, a file could be stale or planted.
pub fn apply(store: &Store, entries: &[KnownEntry]) -> Result<KnownOutcome, StoreError> {
    let mut outcome = KnownOutcome::default();
    let mut new_servers: Vec<(String, u16)> = Vec::new();
    for entry in entries {
        let server = (entry.address.clone(), entry.port);
        let known = store.host_keys(&entry.address, entry.port)?;
        if known.is_empty() {
            new_servers.push(server.clone());
        }
        let key = sshcraft_store::store::HostKey {
            address: entry.address.clone(),
            port: entry.port,
            algorithm: entry.algorithm.clone(),
            public_key: entry.public_key.clone(),
            fingerprint: entry.fingerprint.clone(),
            added_at: 0,
        };
        if known.iter().any(|k| k.fingerprint == entry.fingerprint) {
            // Fills in the key where only its fingerprint was known.
            store.add_host_key(&key)?;
            outcome.same += 1;
        } else if new_servers.contains(&server) {
            store.add_host_key(&key)?;
            outcome.added += 1;
        } else if !outcome.conflicts.contains(&server) {
            outcome.conflicts.push(server);
        }
    }
    Ok(outcome)
}

/// Host keys as `known_hosts` lines (`[address]:port` when the port is not 22). Keys known only
/// by their fingerprint cannot be written; their number comes back.
pub fn export(keys: &[sshcraft_store::store::HostKey]) -> (String, usize) {
    let mut text = String::new();
    let mut skipped = 0;
    for key in keys {
        let (Some(algorithm), Some(public_key)) = (&key.algorithm, &key.public_key) else {
            skipped += 1;
            continue;
        };
        // Nothing from the store may break the line format.
        let fine = |s: &str| !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, ',' | '#'));
        if !fine(&key.address) || !fine(algorithm) || !fine(public_key) {
            skipped += 1;
            continue;
        }
        let name = if key.port == DEFAULT_PORT { key.address.clone() } else { format!("[{}]:{}", key.address, key.port) };
        text.push_str(&format!("{name} {algorithm} {public_key}\n"));
    }
    (text, skipped)
}
