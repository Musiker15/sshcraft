//! Key management. Only public facts (`KeyInfo`) cross into the UI.

use serde::Serialize;
use stollen_core::model::KeyId;
use stollen_core::vault::{self, KeyInfo, Vault};
use tauri::State;

use super::{CommandError, CommandResult};
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

fn unlocked(state: &AppState) -> CommandResult<&Vault> {
    state
        .vault
        .as_deref()
        .map_err(|e| CommandError::new("vault_locked", e))
}

#[tauri::command]
pub fn list_keys(state: State<'_, AppState>) -> CommandResult<Vec<KeyInfo>> {
    Ok(vault::list_keys(&state.store.lock().expect("store lock"))?)
}

#[tauri::command]
pub fn import_key(
    state: State<'_, AppState>,
    name: String,
    text: String,
    passphrase: Option<String>,
) -> CommandResult<KeyInfo> {
    let vault = unlocked(&state)?;
    let store = state.store.lock().expect("store lock");
    Ok(vault.import_key(&store, &name, &text, passphrase.as_deref())?)
}

#[tauri::command]
pub fn import_key_file(
    state: State<'_, AppState>,
    name: String,
    path: String,
    passphrase: Option<String>,
) -> CommandResult<KeyInfo> {
    let text = zeroize::Zeroizing::new(
        std::fs::read_to_string(&path).map_err(|e| CommandError::new("io", e))?,
    );
    let vault = unlocked(&state)?;
    let store = state.store.lock().expect("store lock");
    Ok(vault.import_key(&store, &name, &text, passphrase.as_deref())?)
}

#[tauri::command]
pub fn generate_key(state: State<'_, AppState>, name: String) -> CommandResult<KeyInfo> {
    let vault = unlocked(&state)?;
    let store = state.store.lock().expect("store lock");
    Ok(vault.generate_ed25519(&store, &name)?)
}

#[tauri::command]
pub fn delete_key(state: State<'_, AppState>, id: KeyId) -> CommandResult<()> {
    Ok(vault::delete_key(
        &state.store.lock().expect("store lock"),
        id,
    )?)
}
