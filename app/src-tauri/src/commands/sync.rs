//! Sync servers: set up, switch on and off, devices, moving items between places.

use serde::{Deserialize, Serialize};
use sshcraft_core::model::ServerId;
use sshcraft_core::store::ItemKind;
use sshcraft_proto::identity::AccountKey;
use sshcraft_sync::account::{
    JoinAccount, NewAccount, Setup, account_key_qr_svg, create_account, join_account,
};
use sshcraft_sync::devices::{DeviceInfo, list_devices, revoke_device};
use sshcraft_sync::engine::Engine;
use tauri::State;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{CommandError, CommandResult};
use crate::state::AppState;
use crate::sync::{StatusView, note_change, now_ms, server_status, sync_error};

#[derive(Debug, Clone, Serialize)]
pub struct ServerView {
    pub id: ServerId,
    pub name: String,
    pub origin: String,
    pub sync_enabled: bool,
    pub sync_keys: bool,
    pub revoked: bool,
    pub status: StatusView,
}

#[derive(Deserialize)]
pub struct CreateInput {
    pub server_name: String,
    pub origin: String,
    pub api_key: Zeroizing<String>,
    pub name: String,
    pub email: String,
    pub device_name: String,
}

#[derive(Deserialize)]
pub struct JoinInput {
    pub server_name: String,
    pub origin: String,
    pub api_key: Zeroizing<String>,
    pub account_key: Zeroizing<String>,
    pub device_name: String,
}

impl std::fmt::Debug for CreateInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateInput")
            .field("server_name", &self.server_name)
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for JoinInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinInput")
            .field("server_name", &self.server_name)
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

/// Shown once after an account was created; the key is not stored anywhere.
#[derive(Serialize)]
pub struct AccountCreated {
    /// `None` when the account exists but setting it up here did not finish: the user keeps
    /// the key and finishes by joining with it.
    pub server_id: Option<ServerId>,
    pub account_key: Zeroizing<String>,
    pub qr_svg: String,
    /// Why the setup did not finish, if it did not.
    pub unfinished: Option<CommandError>,
}

pub(crate) fn vault(
    state: &AppState,
) -> CommandResult<std::sync::Arc<sshcraft_core::vault::Vault>> {
    state.vault.clone()
}

fn platform() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
pub fn list_servers(state: State<'_, AppState>) -> CommandResult<Vec<ServerView>> {
    let servers = state.store.lock().expect("store lock").servers()?;
    Ok(servers
        .into_iter()
        .map(|row| ServerView {
            status: server_status(row.revoked, state.sync.status(row.id)),
            id: row.id,
            name: row.name,
            origin: row.origin,
            sync_enabled: row.sync_enabled,
            sync_keys: row.sync_keys,
            revoked: row.revoked,
        })
        .collect())
}

#[tauri::command]
pub async fn create_sync_account(
    state: State<'_, AppState>,
    input: CreateInput,
) -> CommandResult<AccountCreated> {
    let vault = vault(&state)?;
    let new = NewAccount {
        server_name: input.server_name,
        origin: input.origin,
        api_key: input.api_key.to_string(),
        name: input.name,
        email: input.email,
        device_name: input.device_name,
        platform: platform().to_owned(),
    };
    let (setup, export) = create_account(&state.store, &vault, new, now_ms() / 1000)
        .await
        .map_err(sync_error)?;
    let (server_id, unfinished) = match setup {
        Setup::Ready(id) => {
            state.sync.start(id);
            (Some(id), None)
        }
        Setup::Unfinished(err) => (None, Some(sync_error(err))),
    };
    Ok(AccountCreated {
        server_id,
        qr_svg: account_key_qr_svg(&export),
        account_key: export,
        unfinished,
    })
}

#[tauri::command]
pub async fn join_sync_account(
    state: State<'_, AppState>,
    input: JoinInput,
) -> CommandResult<ServerId> {
    let vault = vault(&state)?;
    let join = JoinAccount {
        server_name: input.server_name,
        origin: input.origin,
        api_key: input.api_key.to_string(),
        account_key: input.account_key.to_string(),
        device_name: input.device_name,
        platform: platform().to_owned(),
    };
    let id = join_account(&state.store, &vault, join, now_ms() / 1000)
        .await
        .map_err(sync_error)?;
    state.sync.start(id);
    Ok(id)
}

/// Switches sync and key sync. With key sync off, keys at this server become local keys.
#[tauri::command]
pub async fn set_server_sync(
    state: State<'_, AppState>,
    id: ServerId,
    sync_enabled: bool,
    sync_keys: bool,
) -> CommandResult<()> {
    {
        let store = state.store.lock().expect("store lock");
        store.update_server_flags(id, sync_enabled, sync_keys)?;
        if !sync_keys {
            for key in store.list_key_records()? {
                if key.location == Some(id) {
                    store.set_location(ItemKind::Key, &key.id.0.to_string(), None)?;
                }
            }
        }
    }
    if sync_enabled {
        state.sync.start(id);
    } else {
        state.sync.stop(id);
    }
    Ok(())
}

/// Forgets a server on this device; its items stay here as local items.
#[tauri::command]
pub fn remove_server(state: State<'_, AppState>, id: ServerId) -> CommandResult<()> {
    state.sync.stop(id);
    Ok(state.store.lock().expect("store lock").remove_server(id)?)
}

#[tauri::command]
pub async fn list_sync_devices(
    state: State<'_, AppState>,
    id: ServerId,
) -> CommandResult<Vec<DeviceInfo>> {
    let vault = vault(&state)?;
    let mut engine = Engine::load(&state.store, &vault, id)
        .await
        .map_err(sync_error)?;
    list_devices(&mut engine, &state.store)
        .await
        .map_err(sync_error)
}

#[tauri::command]
pub async fn revoke_sync_device(
    state: State<'_, AppState>,
    id: ServerId,
    device_id: Uuid,
) -> CommandResult<()> {
    let vault = vault(&state)?;
    let mut engine = Engine::load(&state.store, &vault, id)
        .await
        .map_err(sync_error)?;
    revoke_device(
        &mut engine,
        &state.store,
        &vault,
        device_id,
        now_ms() / 1000,
    )
    .await
    .map_err(sync_error)?;
    state.sync.nudge(&[id]);
    Ok(())
}

/// Moves an item to a server or back to this device only.
#[tauri::command]
pub fn move_item(
    state: State<'_, AppState>,
    kind: ItemKind,
    item_id: String,
    location: Option<ServerId>,
) -> CommandResult<()> {
    let servers = {
        let store = state.store.lock().expect("store lock");
        crate::sync::share_rights(&store, crate::sync::Change::Move(kind, &item_id))?;
        let before = store.location_of(kind, &item_id)?;
        store.set_location(kind, &item_id, location)?;
        note_change(&store, kind, &item_id, before, now_ms())?
    };
    state.sync.nudge(&servers);
    Ok(())
}

/// Checks a pasted account key and returns it as a QR code, storing nothing.
pub fn account_key_qr_svg_checked(text: &str) -> CommandResult<String> {
    let key = AccountKey::import(text).map_err(|e| sync_error(e.into()))?;
    Ok(account_key_qr_svg(&key.export()))
}

#[tauri::command]
pub fn account_key_qr(text: String) -> CommandResult<String> {
    account_key_qr_svg_checked(&text)
}
