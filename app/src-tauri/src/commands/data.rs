//! Groups, hosts and stored host keys.

use sshcraft_core::model::{Group, GroupId, Host, HostId, effective_endpoint};
use tauri::State;

use super::{CommandError, CommandResult, ErrorCode};
use crate::settings::{self, AppSettings};
use crate::state::AppState;

fn store(state: &AppState) -> std::sync::MutexGuard<'_, sshcraft_core::store::Store> {
    state.store.lock().expect("store lock")
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> CommandResult<Vec<Group>> {
    Ok(store(&state).list_groups()?)
}

#[tauri::command]
pub fn save_group(state: State<'_, AppState>, group: Group) -> CommandResult<()> {
    Ok(store(&state).upsert_group(&group)?)
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, id: GroupId) -> CommandResult<()> {
    Ok(store(&state).delete_group(id)?)
}

#[tauri::command]
pub fn list_hosts(state: State<'_, AppState>) -> CommandResult<Vec<Host>> {
    Ok(store(&state).list_hosts()?)
}

#[tauri::command]
pub fn save_host(state: State<'_, AppState>, host: Host) -> CommandResult<()> {
    Ok(store(&state).upsert_host(&host)?)
}

#[tauri::command]
pub fn delete_host(state: State<'_, AppState>, id: HostId) -> CommandResult<()> {
    Ok(store(&state).delete_host(id)?)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    settings::load(&store(&state))
}

#[tauri::command]
pub fn save_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> CommandResult<AppSettings> {
    Ok(settings::save(&store(&state), settings)?)
}

/// The operating system's language, for the AUTO language setting.
#[tauri::command]
pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

/// Ids of the hosts connected to most recently, newest first.
#[tauri::command]
pub fn list_recent(state: State<'_, AppState>, limit: u32) -> CommandResult<Vec<HostId>> {
    Ok(store(&state).recent_hosts(limit.min(100))?)
}

/// Removes the stored host key for the host's effective address and port.
#[tauri::command]
pub fn forget_host_key(state: State<'_, AppState>, host_id: HostId) -> CommandResult<()> {
    let store = store(&state);
    let host = store
        .list_hosts()?
        .into_iter()
        .find(|h| h.id == host_id)
        .ok_or_else(|| CommandError::code(ErrorCode::NotFound))?;
    let group = match host.group_id {
        Some(gid) => store.list_groups()?.into_iter().find(|g| g.id == gid),
        None => None,
    };
    let (address, port) = effective_endpoint(&host, group.as_ref())?;
    Ok(store.forget_known_host(&address, port)?)
}
