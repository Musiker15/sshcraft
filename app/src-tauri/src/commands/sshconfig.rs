//! Preview and import of hosts from `~/.ssh/config`.

use std::path::PathBuf;

use serde::Serialize;
use sshcraft_core::sshconfig::{self, ImportCandidate};
use tauri::State;

use super::{CommandError, CommandResult, ErrorCode};
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

/// Fingerprints the agent holds, `None` if it cannot be reached or asked.
async fn agent_fingerprints() -> Option<Vec<String>> {
    let mut agent = WindowsAgent.connect().await.ok()?;
    sshcraft_ssh::list_agent_keys(&mut agent)
        .await
        .ok()
        .map(|keys| keys.into_iter().map(|k| k.fingerprint).collect())
}

async fn preview() -> CommandResult<SshConfigPreview> {
    let path = home()
        .map(|h| h.join(".ssh").join("config"))
        .ok_or_else(|| CommandError::new(ErrorCode::Io, "no home directory"))?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(CommandError::new(ErrorCode::Io, e)),
    };
    let parsed = sshconfig::parse(&text);
    let agent = agent_fingerprints().await;
    Ok(SshConfigPreview {
        candidates: sshconfig::plan_import(&parsed.hosts, &fingerprint_of, agent.as_deref()),
        skipped: parsed.skipped,
    })
}

#[tauri::command]
pub async fn ssh_config_preview() -> CommandResult<SshConfigPreview> {
    preview().await
}

/// Creates exactly the candidates the user confirmed in the preview, nothing re-read.
#[tauri::command]
pub fn ssh_config_import(
    state: State<'_, AppState>,
    candidates: Vec<ImportCandidate>,
) -> CommandResult<SshConfigImportResult> {
    let store = state.store.lock().expect("store lock");
    Ok(SshConfigImportResult {
        created: sshcraft_core::import::apply_import(&store, &candidates, None)?.created,
    })
}
