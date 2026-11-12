//! Key management. Only public facts (`KeyInfo`) cross into the UI.

use serde::Serialize;
use sshcraft_core::model::KeyId;
use std::sync::Arc;

use sshcraft_core::vault::{self, KeyInfo, Vault};
use tauri::State;
use zeroize::Zeroizing;

use super::{CommandError, CommandResult};
use crate::passcli::{self, PassCli, PassCliError, PassCliProcess, PassCliStatus, PassKey};
use crate::state::AppState;

#[derive(Serialize)]
pub struct VaultStatus {
    ok: bool,
    error: Option<String>,
}

#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> VaultStatus {
    match &state.vault {
        Ok(_) => VaultStatus {
            ok: true,
            error: None,
        },
        Err(e) => VaultStatus {
            ok: false,
            error: Some(e.clone()),
        },
    }
}

fn unlocked(state: &AppState) -> CommandResult<Arc<Vault>> {
    state
        .vault
        .clone()
        .map_err(|e| CommandError::new("vault_locked", e))
}

/// Runs blocking key work off the main thread, so the window and terminals stay responsive.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| CommandError::new("internal", e))?
}

#[derive(Serialize)]
pub struct HelloStatus {
    missing: bool,
}

#[tauri::command]
pub fn hello_status(state: State<'_, AppState>) -> HelloStatus {
    HelloStatus {
        missing: state.sessions.hello_missing(),
    }
}

#[tauri::command]
pub fn list_keys(state: State<'_, AppState>) -> CommandResult<Vec<KeyInfo>> {
    Ok(vault::list_keys(&state.store.lock().expect("store lock"))?)
}

#[tauri::command]
pub async fn import_key(
    state: State<'_, AppState>,
    name: String,
    text: String,
    passphrase: Option<String>,
) -> CommandResult<KeyInfo> {
    let vault = unlocked(&state)?;
    let store = state.store.clone();
    let text = Zeroizing::new(text);
    let passphrase = passphrase.map(Zeroizing::new);
    blocking(move || {
        let key = vault::parse_private_key(&text, passphrase.as_deref().map(String::as_str))?;
        let store = store.lock().expect("store lock");
        Ok(vault.store_key(&store, &name, &key)?)
    })
    .await
}

#[tauri::command]
pub async fn generate_key(state: State<'_, AppState>, name: String) -> CommandResult<KeyInfo> {
    let vault = unlocked(&state)?;
    let store = state.store.clone();
    blocking(move || {
        let store = store.lock().expect("store lock");
        Ok(vault.generate_ed25519(&store, &name)?)
    })
    .await
}

#[tauri::command]
pub async fn pass_cli_status() -> CommandResult<PassCliStatus> {
    blocking(|| Ok(passcli::local_status())).await
}

#[tauri::command]
pub async fn pass_cli_list_keys() -> CommandResult<Vec<PassKey>> {
    blocking(|| {
        let cli = PassCliProcess::locate().ok_or(PassCliError::NotInstalled)?;
        Ok(cli.list_ssh_keys()?)
    })
    .await
}

#[tauri::command]
pub async fn import_from_pass_cli(
    state: State<'_, AppState>,
    share_id: String,
    item_id: String,
    name: String,
) -> CommandResult<KeyInfo> {
    let vault = unlocked(&state)?;
    let store = state.store.clone();
    blocking(move || {
        let cli = PassCliProcess::locate().ok_or(PassCliError::NotInstalled)?;
        Ok(passcli::import_from_pass_cli(
            &cli, &vault, &store, &share_id, &item_id, &name,
        )?)
    })
    .await
}

#[tauri::command]
pub fn delete_key(state: State<'_, AppState>, id: KeyId) -> CommandResult<()> {
    Ok(vault::delete_key(
        &state.store.lock().expect("store lock"),
        id,
    )?)
}

/// Keys the Proton Pass agent offers. Only public facts, never key material.
#[tauri::command]
pub async fn list_agent_keys() -> CommandResult<Vec<sshcraft_ssh::AgentKey>> {
    use crate::sessions::{AgentConnector, WindowsAgent};
    let mut agent = WindowsAgent
        .connect()
        .await
        .map_err(|e| CommandError::new("agent_unavailable", e))?;
    sshcraft_ssh::list_agent_keys(&mut agent)
        .await
        .map_err(|e| CommandError::new("agent_unavailable", e))
}
