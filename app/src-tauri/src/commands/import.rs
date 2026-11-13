//! Import of hosts, host keys and key files from other programs, and the export.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sshcraft_core::export::{ExportFilter, export_csv, export_json};
use sshcraft_core::import::keyfiles::{self, KeyFile};
use sshcraft_core::import::knownhosts::{self, KnownOutcome};
use sshcraft_core::import::{
    ImportCandidate, ImportOutcome, ImportPlan, MAX_FILE, apply_import, csv, mobaxterm, putty,
    securecrt,
};
use sshcraft_core::model::ServerId;
use sshcraft_core::store::{ItemKind, Store};
use sshcraft_core::vault::KeyInfo;
use tauri::State;

use super::sshconfig::{home, preview as ssh_config_preview};
use super::{CommandError, CommandResult, ErrorCode};
use crate::state::AppState;
use crate::sync::{note_change, now_ms};

/// Where to import from.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportSource {
    SshConfig,
    KnownHosts { path: String },
    Csv { path: String },
    PuttyRegistry,
    PuttyReg { path: String },
    MobaXterm { path: String },
    SecureCrt { dir: String },
}

/// Where each source usually is on this computer, to fill in the dialog.
#[derive(Debug, Clone, Serialize)]
pub struct ImportDefaults {
    known_hosts: Option<String>,
    ssh_dir: Option<String>,
    mobaxterm: Option<String>,
    securecrt: Option<String>,
    /// PuTTY sessions can be read straight from the registry.
    putty_registry: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    plan: ImportPlan,
    /// For `known_hosts`: how many host keys the file holds.
    host_keys: Option<usize>,
}

/// Reads an import file of at most `limit` bytes.
pub fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, CommandError> {
    let meta =
        std::fs::metadata(path).map_err(|_| CommandError::code(ErrorCode::ImportUnreadable))?;
    if meta.len() > limit {
        return Err(CommandError::code(ErrorCode::ImportTooLarge));
    }
    std::fs::read(path).map_err(|_| CommandError::code(ErrorCode::ImportUnreadable))
}

fn read_text(path: &str) -> Result<String, CommandError> {
    let bytes = read_limited(Path::new(path), MAX_FILE)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Option<String> {
    paths
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn import_defaults() -> ImportDefaults {
    let home = home();
    let in_home = |rel: &[&str]| {
        home.as_ref().map(|h| {
            let mut path = h.clone();
            path.extend(rel);
            path
        })
    };
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    ImportDefaults {
        known_hosts: first_existing(in_home(&[".ssh", "known_hosts"])),
        ssh_dir: first_existing(in_home(&[".ssh"])),
        mobaxterm: first_existing(
            [
                in_home(&["Documents", "MobaXterm", "MobaXterm.ini"]),
                app_data
                    .as_ref()
                    .map(|a| a.join("MobaXterm").join("MobaXterm.ini")),
            ]
            .into_iter()
            .flatten(),
        ),
        securecrt: first_existing(
            [
                app_data
                    .as_ref()
                    .map(|a| a.join("VanDyke").join("Config").join("Sessions")),
                in_home(&[
                    "Library",
                    "Application Support",
                    "VanDyke",
                    "SecureCRT",
                    "Config",
                    "Sessions",
                ]),
                in_home(&[".vandyke", "SecureCRT", "Config", "Sessions"]),
            ]
            .into_iter()
            .flatten(),
        ),
        putty_registry: cfg!(windows),
    }
}

/// PuTTY's sessions straight from the Windows registry.
#[cfg(windows)]
fn putty_registry_sessions() -> Vec<putty::Session> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, REG_DWORD, REG_SZ};
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(putty::SESSIONS_KEY) else {
        return Vec::new();
    };
    root.enum_keys()
        .filter_map(Result::ok)
        .filter_map(|name| {
            let key = root.open_subkey(&name).ok()?;
            let values = key
                .enum_values()
                .filter_map(Result::ok)
                .filter_map(|(value_name, value)| {
                    let parsed = match value.vtype {
                        REG_SZ => putty::PuttyValue::Text(
                            String::from_utf16_lossy(
                                &value
                                    .bytes
                                    .as_chunks::<2>()
                                    .0
                                    .iter()
                                    .map(|c| u16::from_le_bytes(*c))
                                    .collect::<Vec<_>>(),
                            )
                            .trim_end_matches('\0')
                            .to_owned(),
                        ),
                        REG_DWORD => {
                            let bytes: [u8; 4] = value.bytes.get(..4)?.try_into().ok()?;
                            putty::PuttyValue::Number(u32::from_le_bytes(bytes))
                        }
                        _ => return None,
                    };
                    Some((value_name, parsed))
                })
                .collect();
            Some(putty::Session {
                name: putty::decode_name(&name),
                values,
            })
        })
        .collect()
}

#[cfg(not(windows))]
fn putty_registry_sessions() -> Vec<putty::Session> {
    Vec::new()
}

#[tauri::command]
pub async fn import_preview(source: ImportSource) -> CommandResult<ImportPreview> {
    let mut host_keys = None;
    let plan = match source {
        ImportSource::SshConfig => {
            let preview = ssh_config_preview().await?;
            ImportPlan {
                candidates: preview.candidates,
                skipped: preview.skipped,
            }
        }
        ImportSource::KnownHosts { path } => {
            let parsed = knownhosts::parse(&read_text(&path)?);
            host_keys = Some(parsed.entries.len());
            ImportPlan {
                candidates: parsed.candidates,
                skipped: parsed.skipped,
            }
        }
        ImportSource::Csv { path } => csv::parse(&read_text(&path)?),
        ImportSource::PuttyRegistry => putty::candidates(&putty_registry_sessions()),
        ImportSource::PuttyReg { path } => putty::candidates(&putty::parse_reg(&read_limited(
            Path::new(&path),
            MAX_FILE,
        )?)),
        ImportSource::MobaXterm { path } => {
            mobaxterm::parse(&read_limited(Path::new(&path), MAX_FILE)?)
        }
        ImportSource::SecureCrt { dir } => securecrt::parse_dir(Path::new(&dir)),
    };
    Ok(ImportPreview { plan, host_keys })
}

/// Creates the confirmed candidates at `location` and queues what was created for that
/// server. Returns the outcome and the servers to nudge.
pub fn apply_and_queue(
    store: &Store,
    candidates: &[ImportCandidate],
    location: Option<ServerId>,
) -> Result<(ImportOutcome, Vec<ServerId>), CommandError> {
    let outcome = apply_import(store, candidates, location)?;
    let mut servers = Vec::new();
    let now = now_ms();
    for group in &outcome.groups {
        servers.extend(note_change(
            store,
            ItemKind::Group,
            &group.0.to_string(),
            None,
            now,
        )?);
    }
    for host in &outcome.hosts {
        servers.extend(note_change(
            store,
            ItemKind::Host,
            &host.0.to_string(),
            None,
            now,
        )?);
    }
    servers.sort_by_key(|s| s.0);
    servers.dedup();
    Ok((outcome, servers))
}

#[tauri::command]
pub fn import_apply(
    state: State<'_, AppState>,
    candidates: Vec<ImportCandidate>,
    location: Option<ServerId>,
) -> CommandResult<ImportOutcome> {
    let (outcome, servers) = {
        let store = state.store.lock().expect("store lock");
        apply_and_queue(&store, &candidates, location)?
    };
    state.sync.nudge(&servers);
    Ok(outcome)
}

/// Takes over the host keys of a `known_hosts` file; differing known keys stay.
#[tauri::command]
pub fn import_known_hosts(state: State<'_, AppState>, path: String) -> CommandResult<KnownOutcome> {
    let parsed = knownhosts::parse(&read_text(&path)?);
    let store = state.store.lock().expect("store lock");
    Ok(knownhosts::apply(&store, &parsed.entries)?)
}

#[tauri::command]
pub fn scan_key_files(dir: String) -> Vec<KeyFile> {
    keyfiles::scan(Path::new(&dir))
}

#[tauri::command]
pub fn import_key_file(
    state: State<'_, AppState>,
    path: String,
    passphrase: Option<String>,
) -> CommandResult<KeyInfo> {
    let vault = state.vault.clone()?;
    let servers = {
        let store = state.store.lock().expect("store lock");
        let had_certificate: std::collections::HashSet<_> = store
            .list_key_records()?
            .into_iter()
            .filter(|r| r.certificate.is_some())
            .map(|r| r.id)
            .collect();
        let info = keyfiles::import(&vault, &store, Path::new(&path), passphrase.as_deref())?;
        // A certificate next to a key that is already synced goes to the other devices too,
        // unless the key came in a share (then it stays here).
        let servers = match info.location {
            Some(server)
                if info.certificate.is_some()
                    && !had_certificate.contains(&info.id)
                    && crate::sync::share_rights_for_key(&store, server, info.id).is_ok() =>
            {
                crate::commands::keys::key_changed(&store, info.id)?
            }
            _ => Vec::new(),
        };
        (info, servers)
    };
    state.sync.nudge(&servers.1);
    Ok(servers.0)
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Json,
}

/// Writes the export to the file the user picked; it holds no secrets.
#[tauri::command]
pub fn export_hosts(
    state: State<'_, AppState>,
    format: ExportFormat,
    filter: ExportFilter,
    path: String,
) -> CommandResult<()> {
    let text = {
        let store = state.store.lock().expect("store lock");
        match format {
            ExportFormat::Csv => export_csv(&store, &filter)?,
            ExportFormat::Json => export_json(&store, &filter)?,
        }
    };
    std::fs::write(&path, text).map_err(|e| CommandError::new(ErrorCode::Io, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshcraft_core::model::ConnectionSettings;
    use sshcraft_core::store::ServerRow;
    use uuid::Uuid;

    fn store_with_server() -> (Store, ServerId) {
        let store = Store::open_in_memory().unwrap();
        let id = ServerId::new();
        store
            .add_server(&ServerRow {
                id,
                name: "home".into(),
                origin: "https://sync.example.org".into(),
                account: [1; 32],
                device_id: Uuid::new_v4(),
                sealed_device: vec![0],
                sync_enabled: true,
                sync_keys: false,
                revoked: false,
                log_head: None,
                tracker: None,
                created_at: 1,
            })
            .unwrap();
        (store, id)
    }

    fn candidate(name: &str, path: &[&str]) -> ImportCandidate {
        ImportCandidate {
            name: name.into(),
            group_path: path.iter().map(|s| s.to_string()).collect(),
            settings: ConnectionSettings {
                address: Some(format!("{name}.example.org")),
                ..Default::default()
            },
            ..ImportCandidate::default()
        }
    }

    #[test]
    fn import_at_a_server_is_queued() {
        let (store, server) = store_with_server();
        let (outcome, servers) = apply_and_queue(
            &store,
            &[candidate("a", &["G", "H"]), candidate("b", &[])],
            Some(server),
        )
        .unwrap();
        assert_eq!(outcome.created, 2);
        assert_eq!(servers, vec![server]);
        let outbox = store.outbox(server).unwrap();
        assert_eq!(
            outbox.iter().filter(|(k, _)| *k == ItemKind::Group).count(),
            2
        );
        assert_eq!(
            outbox.iter().filter(|(k, _)| *k == ItemKind::Host).count(),
            2
        );
    }

    #[test]
    fn local_import_is_not_queued() {
        let (store, server) = store_with_server();
        let (_, servers) = apply_and_queue(&store, &[candidate("a", &["G"])], None).unwrap();
        assert!(servers.is_empty());
        assert!(store.outbox(server).unwrap().is_empty());
    }

    #[test]
    fn too_large_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.csv");
        std::fs::write(&path, "0123456789").unwrap();
        assert_eq!(
            read_limited(&path, 4).unwrap_err().code,
            ErrorCode::ImportTooLarge
        );
        assert!(read_limited(&path, 100).is_ok());
        assert_eq!(
            read_limited(&dir.path().join("missing"), 100)
                .unwrap_err()
                .code,
            ErrorCode::ImportUnreadable
        );
    }
}
