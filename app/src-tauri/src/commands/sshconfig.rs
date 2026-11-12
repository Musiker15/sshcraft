//! Preview and import of hosts from `~/.ssh/config`.

use std::path::PathBuf;

use serde::Serialize;
use stollen_core::model::{ConnectionSettings, Group, GroupId, Host, HostId};
use stollen_core::sshconfig::{self, ImportCandidate};
use tauri::State;

use super::{CommandError, CommandResult};
use crate::sessions::{AgentConnector, WindowsAgent};
use crate::state::AppState;

#[derive(Serialize)]
pub struct SshConfigPreview {
    candidates: Vec<ImportCandidate>,
    skipped: Vec<String>,
}

#[derive(Serialize)]
pub struct SshConfigImportResult {
    created: usize,
}

fn home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => home().map_or_else(|| PathBuf::from(path), |h| h.join(rest)),
        None => PathBuf::from(path),
    }
}

/// SHA256 fingerprint of the public key that belongs to an `IdentityFile` entry.
fn fingerprint_of(identity_file: &str) -> Option<String> {
    let path = expand(identity_file);
    let public = if path.extension().is_some_and(|e| e == "pub") {
        path
    } else {
        let mut with_pub = path.into_os_string();
        with_pub.push(".pub");
        PathBuf::from(with_pub)
    };
    let text = std::fs::read_to_string(public).ok()?;
    let key = russh::keys::PublicKey::from_openssh(text.trim()).ok()?;
    Some(key.fingerprint(russh::keys::HashAlg::Sha256).to_string())
}

async fn agent_fingerprints() -> Vec<String> {
    let Ok(mut agent) = WindowsAgent.connect().await else {
        return Vec::new();
    };
    stollen_ssh::list_agent_keys(&mut agent)
        .await
        .map(|keys| keys.into_iter().map(|k| k.fingerprint).collect())
        .unwrap_or_default()
}

async fn preview() -> CommandResult<SshConfigPreview> {
    let path = home()
        .map(|h| h.join(".ssh").join("config"))
        .ok_or_else(|| CommandError::new("io", "no home directory"))?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(CommandError::new("io", e)),
    };
    let parsed = sshconfig::parse(&text);
    let agent = agent_fingerprints().await;
    Ok(SshConfigPreview {
        candidates: sshconfig::plan_import(&parsed.hosts, &fingerprint_of, &agent),
        skipped: parsed.skipped,
    })
}

#[tauri::command]
pub async fn ssh_config_preview() -> CommandResult<SshConfigPreview> {
    preview().await
}

/// Creates the chosen hosts (and their groups). Hosts that already exist with the same name
/// and address are skipped.
#[tauri::command]
pub async fn ssh_config_import(
    state: State<'_, AppState>,
    aliases: Vec<String>,
) -> CommandResult<SshConfigImportResult> {
    let candidates = preview().await?.candidates;
    let store = state.store.lock().expect("store lock");
    let mut groups = store.list_groups()?;
    let hosts = store.list_hosts()?;
    let mut created = 0;

    for candidate in candidates
        .into_iter()
        .filter(|c| aliases.contains(&c.alias))
    {
        let group_id = match &candidate.group {
            Some(name) => Some(match groups.iter().find(|g| &g.name == name) {
                Some(g) => g.id,
                None => {
                    let group = Group {
                        id: GroupId::new(),
                        name: name.clone(),
                        defaults: ConnectionSettings {
                            address: Some(name.clone()),
                            ..Default::default()
                        },
                    };
                    store.upsert_group(&group)?;
                    let id = group.id;
                    groups.push(group);
                    id
                }
            }),
            None => None,
        };
        let address_of = |h: &Host| {
            h.settings.address.clone().or_else(|| {
                h.group_id
                    .and_then(|gid| groups.iter().find(|g| g.id == gid))
                    .and_then(|g| g.defaults.address.clone())
            })
        };
        let new_address = candidate
            .settings
            .address
            .clone()
            .or_else(|| candidate.group.clone());
        let exists = hosts
            .iter()
            .any(|h| h.name == candidate.alias && address_of(h) == new_address);
        if exists {
            continue;
        }
        store.upsert_host(&Host {
            id: HostId::new(),
            name: candidate.alias,
            group_id,
            settings: candidate.settings,
        })?;
        created += 1;
    }
    Ok(SshConfigImportResult { created })
}
